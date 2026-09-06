package main

import (
	"bufio"
	"bytes"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"
)

var version = "dev"

type rpcRequest struct {
	ID      any            `json:"id"`
	Method  string         `json:"method"`
	Params  map[string]any `json:"params"`
	TraceID string         `json:"trace_id,omitempty"`
}

type rpcError struct {
	Code    string `json:"code"`
	Message string `json:"message"`
}

type rpcResponse struct {
	ID                any       `json:"id,omitempty"`
	OK                bool      `json:"ok"`
	Result            any       `json:"result,omitempty"`
	Error             *rpcError `json:"error,omitempty"`
	TraceID           string    `json:"trace_id,omitempty"`
	HandlerDurationUS *int64    `json:"handler_duration_us,omitempty"`
}

type rpcEvent struct {
	Event      string `json:"event"`
	StreamID   string `json:"stream_id,omitempty"`
	DataBase64 string `json:"data_base64,omitempty"`
	Error      string `json:"error,omitempty"`
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

		started := time.Now()
		resp := server.handleRequest(req)
		if validTraceID(req.TraceID) {
			duration := time.Since(started).Microseconds()
			resp.TraceID = req.TraceID
			resp.HandlerDurationUS = &duration
		}
		if err := writer.writeResponse(resp); err != nil {
			return err
		}
	}
}

// validTraceID accepts bounded UUID-shaped hexadecimal correlation labels without retaining arbitrary input.
func validTraceID(value string) bool {
	if len(value) != 36 {
		return false
	}
	for index := range value {
		if index == 8 || index == 13 || index == 18 || index == 23 {
			if value[index] != '-' {
				return false
			}
		} else if !((value[index] >= '0' && value[index] <= '9') ||
			(value[index] >= 'a' && value[index] <= 'f') || (value[index] >= 'A' && value[index] <= 'F')) {
			return false
		}
	}
	return true
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
