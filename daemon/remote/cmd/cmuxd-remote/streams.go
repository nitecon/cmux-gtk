package main

import (
	"encoding/base64"
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"strconv"
	"time"
)

// streamState owns one transport connection and tracks whether its sole output reader has started.
type streamState struct {
	conn          net.Conn
	readerStarted bool
}

// setTCPNoDelay disables Nagle buffering on TCP streams while leaving other transports unchanged.
func setTCPNoDelay(conn net.Conn) {
	tcpConn, ok := conn.(*net.TCPConn)
	if !ok {
		return
	}
	_ = tcpConn.SetNoDelay(true)
}

// handleProxyOpen validates the TCP target, connects off the registry lock and transfers ownership to the stream registry.
func (s *rpcServer) handleProxyOpen(req rpcRequest) rpcResponse {
	host, ok := getStringParam(req.Params, "host")
	if !ok || host == "" {
		return rpcResponse{
			ID: req.ID,
			OK: false,
			Error: &rpcError{
				Code:    "invalid_params",
				Message: "proxy.open requires host",
			},
		}
	}
	port, ok := getIntParam(req.Params, "port")
	if !ok || port <= 0 || port > 65535 {
		return rpcResponse{
			ID: req.ID,
			OK: false,
			Error: &rpcError{
				Code:    "invalid_params",
				Message: "proxy.open requires port in range 1-65535",
			},
		}
	}

	timeout, err := getTimeoutParam(req.Params, 10*time.Second)
	if err != nil {
		return rpcResponse{ID: req.ID, OK: false, Error: &rpcError{Code: "invalid_params", Message: err.Error()}}
	}

	conn, err := net.DialTimeout(
		"tcp",
		net.JoinHostPort(host, strconv.Itoa(port)),
		timeout,
	)
	if err != nil {
		return rpcResponse{
			ID: req.ID,
			OK: false,
			Error: &rpcError{
				Code:    "open_failed",
				Message: err.Error(),
			},
		}
	}
	setTCPNoDelay(conn)

	streamID := s.registerStream(conn)

	return rpcResponse{
		ID: req.ID,
		OK: true,
		Result: map[string]any{
			"stream_id": streamID,
		},
	}
}

// handleProxyClose validates the stream identity and closes it idempotently through shared ownership.
func (s *rpcServer) handleProxyClose(req rpcRequest) rpcResponse {
	streamID, ok := getStringParam(req.Params, "stream_id")
	if !ok || streamID == "" {
		return rpcResponse{
			ID: req.ID,
			OK: false,
			Error: &rpcError{
				Code:    "invalid_params",
				Message: "proxy.close requires stream_id",
			},
		}
	}

	s.dropStream(streamID)
	return rpcResponse{
		ID: req.ID,
		OK: true,
		Result: map[string]any{
			"closed": true,
		},
	}
}

// handleProxyWrite decodes and writes the complete payload, returning transport failures without holding the registry lock.
func (s *rpcServer) handleProxyWrite(req rpcRequest) rpcResponse {
	streamID, ok := getStringParam(req.Params, "stream_id")
	if !ok || streamID == "" {
		return rpcResponse{
			ID: req.ID,
			OK: false,
			Error: &rpcError{
				Code:    "invalid_params",
				Message: "proxy.write requires stream_id",
			},
		}
	}
	dataBase64, ok := getStringParam(req.Params, "data_base64")
	if !ok {
		return rpcResponse{
			ID: req.ID,
			OK: false,
			Error: &rpcError{
				Code:    "invalid_params",
				Message: "proxy.write requires data_base64",
			},
		}
	}
	payload, err := base64.StdEncoding.DecodeString(dataBase64)
	if err != nil {
		return rpcResponse{
			ID: req.ID,
			OK: false,
			Error: &rpcError{
				Code:    "invalid_params",
				Message: "data_base64 must be valid base64",
			},
		}
	}

	state, found := s.getStream(streamID)
	if !found {
		return rpcResponse{
			ID: req.ID,
			OK: false,
			Error: &rpcError{
				Code:    "not_found",
				Message: "stream not found",
			},
		}
	}
	conn := state.conn

	timeout, err := getTimeoutParam(req.Params, 8*time.Second)
	if err != nil {
		return rpcResponse{ID: req.ID, OK: false, Error: &rpcError{Code: "invalid_params", Message: err.Error()}}
	}
	if timeout > 0 {
		if err := conn.SetWriteDeadline(time.Now().Add(timeout)); err != nil {
			return rpcResponse{
				ID: req.ID,
				OK: false,
				Error: &rpcError{
					Code:    "stream_error",
					Message: err.Error(),
				},
			}
		}
		defer conn.SetWriteDeadline(time.Time{})
	}

	total := 0
	for total < len(payload) {
		written, writeErr := conn.Write(payload[total:])
		if written == 0 && writeErr == nil {
			return rpcResponse{
				ID: req.ID,
				OK: false,
				Error: &rpcError{
					Code:    "stream_error",
					Message: "write made no progress",
				},
			}
		}
		total += written
		if writeErr != nil {
			return rpcResponse{
				ID: req.ID,
				OK: false,
				Error: &rpcError{
					Code:    "stream_error",
					Message: writeErr.Error(),
				},
			}
		}
	}

	return rpcResponse{
		ID: req.ID,
		OK: true,
		Result: map[string]any{
			"written": total,
		},
	}
}

// handleProxyStreamSubscribe starts at most one event reader per registered stream under concurrent subscriptions.
func (s *rpcServer) handleProxyStreamSubscribe(req rpcRequest) rpcResponse {
	streamID, ok := getStringParam(req.Params, "stream_id")
	if !ok || streamID == "" {
		return rpcResponse{
			ID: req.ID,
			OK: false,
			Error: &rpcError{
				Code:    "invalid_params",
				Message: "proxy.stream.subscribe requires stream_id",
			},
		}
	}

	halfClose := false
	if value, exists := req.Params["half_close"]; exists {
		var valid bool
		halfClose, valid = value.(bool)
		if !valid {
			return rpcResponse{ID: req.ID, OK: false, Error: &rpcError{Code: "invalid_params", Message: "half_close must be boolean"}}
		}
	}

	s.mu.Lock()
	state, found := s.streams[streamID]
	if !found {
		s.mu.Unlock()
		return rpcResponse{
			ID: req.ID,
			OK: false,
			Error: &rpcError{
				Code:    "not_found",
				Message: "stream not found",
			},
		}
	}
	alreadySubscribed := state.readerStarted
	if !alreadySubscribed {
		state.readerStarted = true
	}
	conn := state.conn
	s.mu.Unlock()

	if !alreadySubscribed {
		go s.streamPumpMode(streamID, conn, halfClose)
	}

	return rpcResponse{
		ID: req.ID,
		OK: true,
		Result: map[string]any{
			"subscribed":         true,
			"already_subscribed": alreadySubscribed,
		},
	}
}

// handleSessionSpawn resolves launch defaults, starts a PTY and registers its connection for stream I/O.
func (s *rpcServer) handleSessionSpawn(req rpcRequest) rpcResponse {
	// Get optional cols/rows (default 80x24)
	cols := 80
	rows := 24
	if c, ok := getIntParam(req.Params, "cols"); ok && c > 0 {
		cols = c
	}
	if r, ok := getIntParam(req.Params, "rows"); ok && r > 0 {
		rows = r
	}

	// Get optional shell (default to user's login shell or /bin/sh)
	shell := os.Getenv("SHELL")
	if shell == "" {
		shell = "/bin/sh"
	}

	directory, _ := getStringParam(req.Params, "cwd")
	conn, err := startPTY(shell, directory, cols, rows)
	if err != nil {
		return rpcResponse{
			ID: req.ID,
			OK: false,
			Error: &rpcError{
				Code:    "spawn_failed",
				Message: err.Error(),
			},
		}
	}

	// Create a stream wrapping the PTY master fd
	streamID := s.registerStream(conn)

	return rpcResponse{
		ID: req.ID,
		OK: true,
		Result: map[string]any{
			"stream_id": streamID,
			"shell":     shell,
			"cols":      cols,
			"rows":      rows,
		},
	}
}

// handleStreamResize validates positive dimensions and resizes an existing PTY stream when applicable.
func (s *rpcServer) handleStreamResize(req rpcRequest) rpcResponse {
	streamID, ok := getStringParam(req.Params, "stream_id")
	if !ok || streamID == "" {
		return rpcResponse{
			ID: req.ID, OK: false,
			Error: &rpcError{Code: "invalid_params", Message: "stream.resize requires stream_id"},
		}
	}
	cols, ok := getIntParam(req.Params, "cols")
	if !ok || cols <= 0 {
		return rpcResponse{
			ID: req.ID, OK: false,
			Error: &rpcError{Code: "invalid_params", Message: "stream.resize requires cols > 0"},
		}
	}
	rows, ok := getIntParam(req.Params, "rows")
	if !ok || rows <= 0 {
		return rpcResponse{
			ID: req.ID, OK: false,
			Error: &rpcError{Code: "invalid_params", Message: "stream.resize requires rows > 0"},
		}
	}

	st, exists := s.getStream(streamID)
	if !exists {
		return rpcResponse{
			ID: req.ID, OK: false,
			Error: &rpcError{Code: "not_found", Message: "stream not found"},
		}
	}
	if pc, ok := st.conn.(*ptyConn); ok {
		if err := pc.resize(cols, rows); err != nil {
			return rpcResponse{
				ID: req.ID, OK: false,
				Error: &rpcError{Code: "resize_failed", Message: err.Error()},
			}
		}
	}
	return rpcResponse{ID: req.ID, OK: true, Result: map[string]any{"resized": true}}
}

// getStream borrows a stream registration under the server lock; the connection may be closed concurrently.
func (s *rpcServer) getStream(streamID string) (*streamState, bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	state, ok := s.streams[streamID]
	return state, ok
}

// dropStream removes ownership under lock and closes the connection once outside the server lock.
func (s *rpcServer) dropStream(streamID string) {
	s.mu.Lock()
	state, ok := s.streams[streamID]
	if ok {
		delete(s.streams, streamID)
	}
	s.mu.Unlock()
	if ok {
		_ = state.conn.Close()
	}
}

// closeAll drains stream/session registrations and releases native connections without holding the registry lock.
func (s *rpcServer) closeAll() {
	s.mu.Lock()
	streams := make([]net.Conn, 0, len(s.streams))
	for id, state := range s.streams {
		delete(s.streams, id)
		streams = append(streams, state.conn)
	}
	for id := range s.sessions {
		delete(s.sessions, id)
	}
	s.mu.Unlock()
	for _, conn := range streams {
		_ = conn.Close()
	}
}

// streamPump forwards encoded output until EOF, read failure or failed delivery, then retires its stream.
func (s *rpcServer) streamPump(streamID string, conn net.Conn) {
	s.streamPumpMode(streamID, conn, false)
}

// streamPumpMode optionally preserves a TCP stream's writable direction after clean read EOF.
func (s *rpcServer) streamPumpMode(streamID string, conn net.Conn, halfClose bool) {
	keepWritable := false
	defer func() {
		if !keepWritable {
			s.dropStream(streamID)
		}
	}()
	defer func() {
		if recovered := recover(); recovered != nil {
			_ = s.frameWriter.writeEvent(rpcEvent{
				Event:    "proxy.stream.error",
				StreamID: streamID,
				Error:    fmt.Sprintf("stream panic: %v", recovered),
			})
		}
	}()

	buffer := make([]byte, 32768)
	for {
		n, readErr := conn.Read(buffer)
		data := buffer[:max(0, n)]
		if len(data) > 0 {
			if err := s.frameWriter.writeEvent(rpcEvent{
				Event:      "proxy.stream.data",
				StreamID:   streamID,
				DataBase64: base64.StdEncoding.EncodeToString(data),
			}); err != nil {
				return
			}
		}

		if readErr == nil {
			if n == 0 {
				_ = s.frameWriter.writeEvent(rpcEvent{
					Event:    "proxy.stream.error",
					StreamID: streamID,
					Error:    "read made no progress",
				})
				return
			}
			continue
		}

		if readErr == io.EOF && halfClose {
			if _, tcp := conn.(*net.TCPConn); tcp {
				if s.frameWriter.writeEvent(rpcEvent{Event: "proxy.stream.read_eof", StreamID: streamID}) == nil {
					keepWritable = true
				}
				return
			}
		}
		if readErr == io.EOF {
			_ = s.frameWriter.writeEvent(rpcEvent{
				Event:      "proxy.stream.eof",
				StreamID:   streamID,
				DataBase64: "",
			})
		} else if !errors.Is(readErr, net.ErrClosed) && !errors.Is(readErr, os.ErrClosed) {
			_ = s.frameWriter.writeEvent(rpcEvent{
				Event:    "proxy.stream.error",
				StreamID: streamID,
				Error:    readErr.Error(),
			})
		}

		return
	}
}

// registerStream transfers connection ownership into the registry and returns its unique stream identity.
func (s *rpcServer) registerStream(conn net.Conn) string {
	s.mu.Lock()
	defer s.mu.Unlock()
	streamID := fmt.Sprintf("s-%d", s.nextStreamID)
	s.nextStreamID++
	s.streams[streamID] = &streamState{conn: conn}
	return streamID
}

// handleProxyShutdownWrite sends TCP FIN without retiring the stream's readable response direction.
// PTYs and other non-TCP streams are rejected; only the registered connection is eligible.
func (s *rpcServer) handleProxyShutdownWrite(req rpcRequest) rpcResponse {
	fail := func(code, message string) rpcResponse {
		return rpcResponse{ID: req.ID, OK: false, Error: &rpcError{Code: code, Message: message}}
	}
	id, ok := getStringParam(req.Params, "stream_id")
	if !ok || id == "" {
		return fail("invalid_params", "proxy.shutdown_write requires stream_id")
	}
	stream, ok := s.getStream(id)
	if !ok {
		return fail("not_found", "stream not found")
	}
	connection, ok := stream.conn.(*net.TCPConn)
	if !ok {
		return fail("invalid_params", "stream does not support TCP half-close")
	}
	if err := connection.CloseWrite(); err != nil {
		return fail("write_failed", "TCP write shutdown failed")
	}
	return rpcResponse{ID: req.ID, OK: true, Result: map[string]any{"write_closed": true}}
}
