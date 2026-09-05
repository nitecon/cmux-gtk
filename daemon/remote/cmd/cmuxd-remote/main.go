package main

import (
	"bufio"
	"bytes"
	"encoding/base64"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"time"
)

var version = "dev"

type rpcRequest struct {
	ID     any            `json:"id"`
	Method string         `json:"method"`
	Params map[string]any `json:"params"`
}

type rpcError struct {
	Code    string `json:"code"`
	Message string `json:"message"`
}

type rpcResponse struct {
	ID     any       `json:"id,omitempty"`
	OK     bool      `json:"ok"`
	Result any       `json:"result,omitempty"`
	Error  *rpcError `json:"error,omitempty"`
}

type rpcEvent struct {
	Event      string `json:"event"`
	StreamID   string `json:"stream_id,omitempty"`
	DataBase64 string `json:"data_base64,omitempty"`
	Error      string `json:"error,omitempty"`
}

type streamState struct {
	conn          net.Conn
	readerStarted bool
}

type stdioFrameWriter struct {
	mu     sync.Mutex
	writer *bufio.Writer
}

type rpcServer struct {
	mu            sync.Mutex
	nextStreamID  uint64
	nextSessionID uint64
	streams       map[string]*streamState
	sessions      map[string]*sessionState
	frameWriter   *stdioFrameWriter
}

const maxRPCFrameBytes = 4 * 1024 * 1024

// main selects the daemon or relay CLI entry point and returns its process exit status.
func main() {
	if shouldRunCLIForInvocation(os.Args[0], os.Args[1:]) {
		os.Exit(runCLI(os.Args[1:]))
	}
	os.Exit(run(os.Args[1:], os.Stdin, os.Stdout, os.Stderr))
}

// shouldRunCLIForInvocation recognizes the cmux shim while preserving explicit daemon entry commands.
func shouldRunCLIForInvocation(argv0 string, args []string) bool {
	base := filepath.Base(argv0)
	if base == "cmux" {
		return true
	}
	if !strings.HasPrefix(base, "cmuxd-remote") || len(args) == 0 {
		return false
	}
	return !isDaemonEntryCommand(args[0])
}

// isDaemonEntryCommand distinguishes daemon lifecycle commands from forwarded CLI operations.
func isDaemonEntryCommand(arg string) bool {
	switch arg {
	case "version", "serve", "cli":
		return true
	default:
		return false
	}
}

// run dispatches daemon entry commands with injectable streams and conventional usage/error exit codes.
func run(args []string, stdin io.Reader, stdout, stderr io.Writer) int {
	if len(args) == 0 {
		usage(stderr)
		return 2
	}

	switch args[0] {
	case "version":
		_, _ = fmt.Fprintln(stdout, version)
		return 0
	case "serve":
		fs := flag.NewFlagSet("serve", flag.ContinueOnError)
		fs.SetOutput(stderr)
		stdio := fs.Bool("stdio", false, "serve over stdin/stdout")
		if err := fs.Parse(args[1:]); err != nil {
			return 2
		}
		if !*stdio {
			_, _ = fmt.Fprintln(stderr, "serve requires --stdio")
			return 2
		}
		if err := runStdioServer(stdin, stdout); err != nil {
			_, _ = fmt.Fprintf(stderr, "serve failed: %v\n", err)
			return 1
		}
		return 0
	case "cli":
		return runCLI(args[1:])
	default:
		usage(stderr)
		return 2
	}
}

// usage writes daemon invocation help to the supplied stream.
func usage(w io.Writer) {
	_, _ = fmt.Fprintln(w, "Usage:")
	_, _ = fmt.Fprintln(w, "  cmuxd-remote version")
	_, _ = fmt.Fprintln(w, "  cmuxd-remote serve --stdio")
	_, _ = fmt.Fprintln(w, "  cmuxd-remote cli <command> [args...]")
}

// runStdioServer serves bounded JSON request frames and retires all streams when input or output closes.
func runStdioServer(stdin io.Reader, stdout io.Writer) error {
	writer := &stdioFrameWriter{
		writer: bufio.NewWriter(stdout),
	}
	server := &rpcServer{
		nextStreamID:  1,
		nextSessionID: 1,
		streams:       map[string]*streamState{},
		sessions:      map[string]*sessionState{},
		frameWriter:   writer,
	}
	defer server.closeAll()

	reader := bufio.NewReaderSize(stdin, 64*1024)

	for {
		line, oversized, readErr := readRPCFrame(reader, maxRPCFrameBytes)
		if readErr != nil {
			if errors.Is(readErr, io.EOF) {
				return nil
			}
			return readErr
		}
		if oversized {
			if err := writer.writeResponse(rpcResponse{
				OK: false,
				Error: &rpcError{
					Code:    "invalid_request",
					Message: "request frame exceeds maximum size",
				},
			}); err != nil {
				return err
			}
			continue
		}
		line = bytes.TrimSuffix(line, []byte{'\n'})
		line = bytes.TrimSuffix(line, []byte{'\r'})
		if len(line) == 0 {
			continue
		}

		var req rpcRequest
		if err := json.Unmarshal(line, &req); err != nil {
			if err := writer.writeResponse(rpcResponse{
				OK: false,
				Error: &rpcError{
					Code:    "invalid_request",
					Message: "invalid JSON request",
				},
			}); err != nil {
				return err
			}
			continue
		}

		resp := server.handleRequest(req)
		if err := writer.writeResponse(resp); err != nil {
			return err
		}
	}
}

// setTCPNoDelay disables Nagle buffering on TCP streams while leaving other transports unchanged.
func setTCPNoDelay(conn net.Conn) {
	tcpConn, ok := conn.(*net.TCPConn)
	if !ok {
		return
	}
	_ = tcpConn.SetNoDelay(true)
}

// readRPCFrame bounds one newline-delimited request, drains oversized frames and accepts a final unterminated frame.
func readRPCFrame(reader *bufio.Reader, maxBytes int) ([]byte, bool, error) {
	frame := make([]byte, 0, 1024)
	for {
		chunk, err := reader.ReadSlice('\n')
		if len(chunk) > 0 {
			if len(frame)+len(chunk) > maxBytes {
				if errors.Is(err, bufio.ErrBufferFull) {
					if drainErr := discardUntilNewline(reader); drainErr != nil && !errors.Is(drainErr, io.EOF) {
						return nil, false, drainErr
					}
				}
				return nil, true, nil
			}
			frame = append(frame, chunk...)
		}

		if err == nil {
			return frame, false, nil
		}
		if errors.Is(err, bufio.ErrBufferFull) {
			continue
		}
		if errors.Is(err, io.EOF) {
			if len(frame) == 0 {
				return nil, false, io.EOF
			}
			return frame, false, nil
		}
		return nil, false, err
	}
}

// discardUntilNewline resynchronizes framing after overflow without retaining discarded bytes.
func discardUntilNewline(reader *bufio.Reader) error {
	for {
		_, err := reader.ReadSlice('\n')
		if err == nil || errors.Is(err, io.EOF) {
			return err
		}
		if errors.Is(err, bufio.ErrBufferFull) {
			continue
		}
		return err
	}
}

// writeResponse sends a response through the shared serialized output path.
func (w *stdioFrameWriter) writeResponse(resp rpcResponse) error {
	return w.writeJSONFrame(resp)
}

// writeEvent sends a stream event without interleaving its bytes with other writers.
func (w *stdioFrameWriter) writeEvent(event rpcEvent) error {
	return w.writeJSONFrame(event)
}

// writeJSONFrame encodes before locking, then writes and flushes one indivisible JSONL frame.
func (w *stdioFrameWriter) writeJSONFrame(payload any) error {
	data, err := json.Marshal(payload)
	if err != nil {
		return err
	}
	w.mu.Lock()
	defer w.mu.Unlock()
	if _, err := w.writer.Write(data); err != nil {
		return err
	}
	if err := w.writer.WriteByte('\n'); err != nil {
		return err
	}
	return w.writer.Flush()
}

// handleRequest validates method presence and dispatches the advertised proxy/session operations.
func (s *rpcServer) handleRequest(req rpcRequest) rpcResponse {
	if req.Method == "" {
		return rpcResponse{
			ID: req.ID,
			OK: false,
			Error: &rpcError{
				Code:    "invalid_request",
				Message: "method is required",
			},
		}
	}

	switch req.Method {
	case "hello":
		return rpcResponse{
			ID: req.ID,
			OK: true,
			Result: map[string]any{
				"name":    "cmuxd-remote",
				"version": version,
				"capabilities": []string{
					"session.basic",
					"session.spawn",
					"session.resize.min",
					"proxy.http_connect",
					"proxy.socks5",
					"proxy.stream",
					"proxy.stream.push",
				},
			},
		}
	case "ping":
		return rpcResponse{
			ID: req.ID,
			OK: true,
			Result: map[string]any{
				"pong": true,
			},
		}
	case "proxy.open":
		return s.handleProxyOpen(req)
	case "proxy.close":
		return s.handleProxyClose(req)
	case "proxy.write":
		return s.handleProxyWrite(req)
	case "proxy.stream.subscribe":
		return s.handleProxyStreamSubscribe(req)
	case "session.spawn":
		return s.handleSessionSpawn(req)
	case "stream.resize":
		return s.handleStreamResize(req)
	case "session.open":
		return s.handleSessionOpen(req)
	case "session.close":
		return s.handleSessionClose(req)
	case "session.attach":
		return s.handleSessionAttach(req)
	case "session.resize":
		return s.handleSessionResize(req)
	case "session.detach":
		return s.handleSessionDetach(req)
	case "session.status":
		return s.handleSessionStatus(req)
	default:
		return rpcResponse{
			ID: req.ID,
			OK: false,
			Error: &rpcError{
				Code:    "method_not_found",
				Message: fmt.Sprintf("unknown method %q", req.Method),
			},
		}
	}
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

	s.mu.Lock()
	streamID := fmt.Sprintf("s-%d", s.nextStreamID)
	s.nextStreamID++
	s.streams[streamID] = &streamState{conn: conn}
	s.mu.Unlock()

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
		go s.streamPump(streamID, conn)
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
	s.mu.Lock()
	streamID := fmt.Sprintf("s-%d", s.nextStreamID)
	s.nextStreamID++
	s.streams[streamID] = &streamState{conn: conn}
	s.mu.Unlock()

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

	s.mu.Lock()
	st, exists := s.streams[streamID]
	s.mu.Unlock()
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
	defer func() {
		if recovered := recover(); recovered != nil {
			_ = s.frameWriter.writeEvent(rpcEvent{
				Event:    "proxy.stream.error",
				StreamID: streamID,
				Error:    fmt.Sprintf("stream panic: %v", recovered),
			})
			s.dropStream(streamID)
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
				s.dropStream(streamID)
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
				s.dropStream(streamID)
				return
			}
			continue
		}

		if readErr == io.EOF {
			_ = s.frameWriter.writeEvent(rpcEvent{
				Event:      "proxy.stream.eof",
				StreamID:   streamID,
				DataBase64: "",
			})
		} else if !errors.Is(readErr, net.ErrClosed) {
			_ = s.frameWriter.writeEvent(rpcEvent{
				Event:    "proxy.stream.error",
				StreamID: streamID,
				Error:    readErr.Error(),
			})
		}

		s.dropStream(streamID)
		return
	}
}
