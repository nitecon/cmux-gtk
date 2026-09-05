package main

import (
	"bufio"
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"strings"
	"time"
)

// relayAuthState contains the identity and shared token used for relay challenge authentication.
type relayAuthState struct {
	RelayID    string `json:"relay_id"`
	RelayToken string `json:"relay_token"`
}

// readSocketAddrFile reads the socket address from ~/.cmux/socket_addr as a fallback
// when CMUX_SOCKET_PATH is not set. Written by the cmux app after the relay establishes.
func readSocketAddrFile() string {
	home, err := os.UserHomeDir()
	if err != nil {
		return ""
	}
	data, err := os.ReadFile(filepath.Join(home, ".cmux", "socket_addr"))
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(data))
}

// readRelayAuthFile loads complete credentials for a TCP relay port, returning nil for absent or invalid files.
func readRelayAuthFile(socketPath string) *relayAuthState {
	if strings.Contains(socketPath, ":") && !strings.HasPrefix(socketPath, "/") {
		_, port, err := net.SplitHostPort(socketPath)
		if err != nil || port == "" {
			return nil
		}
		home, err := os.UserHomeDir()
		if err != nil {
			return nil
		}
		data, err := os.ReadFile(filepath.Join(home, ".cmux", "relay", port+".auth"))
		if err != nil {
			return nil
		}
		var state relayAuthState
		if err := json.Unmarshal(data, &state); err != nil {
			return nil
		}
		if state.RelayID == "" || state.RelayToken == "" {
			return nil
		}
		return &state
	}
	return nil
}

// currentRelayAuth prefers a complete environment credential pair, falling back to the port-specific file.
func currentRelayAuth(socketPath string) *relayAuthState {
	relayID := strings.TrimSpace(os.Getenv("CMUX_RELAY_ID"))
	relayToken := strings.TrimSpace(os.Getenv("CMUX_RELAY_TOKEN"))
	if relayID != "" && relayToken != "" {
		return &relayAuthState{RelayID: relayID, RelayToken: relayToken}
	}
	return readRelayAuthFile(socketPath)
}

// dialSocket connects to the cmux socket. If addr contains a colon and doesn't
// start with '/', it's treated as a TCP address (host:port); otherwise Unix socket.
// For TCP connections, refreshAddr is used only to recover from a stale socket_addr
// rewrite, not to poll for relay readiness.
func dialSocket(addr string, refreshAddr func() string) (net.Conn, error) {
	if strings.Contains(addr, ":") && !strings.HasPrefix(addr, "/") {
		conn, connectedAddr, err := dialTCP(addr)
		if err != nil && refreshAddr != nil && isConnectionRefused(err) {
			if refreshedAddr := strings.TrimSpace(refreshAddr()); refreshedAddr != "" && refreshedAddr != addr {
				addr = refreshedAddr
				conn, connectedAddr, err = dialTCP(addr)
			}
		}
		if err != nil {
			return nil, err
		}
		if auth := currentRelayAuth(connectedAddr); auth != nil {
			if err := authenticateRelayConn(conn, auth); err != nil {
				conn.Close()
				return nil, err
			}
		}
		return conn, nil
	}
	return net.DialTimeout("unix", addr, 2*time.Second)
}

// dialTCP opens a TCP relay with a two-second connection timeout and disables Nagle buffering.
func dialTCP(addr string) (net.Conn, string, error) {
	conn, err := net.DialTimeout("tcp", addr, 2*time.Second)
	if err != nil {
		return nil, addr, err
	}
	setTCPNoDelay(conn)
	return conn, addr, nil
}

// isConnectionRefused recognizes refusal errors used to trigger one stale-address refresh.
func isConnectionRefused(err error) bool {
	if opErr, ok := err.(*net.OpError); ok {
		return strings.Contains(opErr.Err.Error(), "connection refused")
	}
	return strings.Contains(err.Error(), "connection refused")
}

// authenticateRelayConn validates the relay challenge, proves token possession and restores deadlines on success.
func authenticateRelayConn(conn net.Conn, auth *relayAuthState) error {
	reader := bufio.NewReader(conn)
	if err := conn.SetDeadline(time.Now().Add(5 * time.Second)); err != nil {
		return fmt.Errorf("set relay authentication deadline: %w", err)
	}

	var challenge struct {
		Protocol string `json:"protocol"`
		Version  int    `json:"version"`
		RelayID  string `json:"relay_id"`
		Nonce    string `json:"nonce"`
	}
	line, err := readRelayLine(reader, 64*1024)
	if err != nil {
		return fmt.Errorf("failed to read relay auth challenge: %w", err)
	}
	if err := json.Unmarshal([]byte(line), &challenge); err != nil {
		return fmt.Errorf("invalid relay auth challenge")
	}
	if challenge.Protocol != "cmux-relay-auth" || challenge.Version != 1 || challenge.RelayID != auth.RelayID || challenge.Nonce == "" {
		return fmt.Errorf("relay auth challenge mismatch")
	}

	tokenBytes, err := hex.DecodeString(auth.RelayToken)
	if err != nil {
		return fmt.Errorf("invalid relay auth token")
	}
	mac := computeRelayMAC(tokenBytes, auth.RelayID, challenge.Nonce, challenge.Version)
	payload, err := json.Marshal(map[string]any{
		"relay_id": auth.RelayID,
		"mac":      hex.EncodeToString(mac),
	})
	if err != nil {
		return fmt.Errorf("failed to encode relay auth response: %w", err)
	}
	if _, err := conn.Write(append(payload, '\n')); err != nil {
		return fmt.Errorf("failed to send relay auth response: %w", err)
	}

	line, err = readRelayLine(reader, 64*1024)
	if err != nil {
		return fmt.Errorf("failed to read relay auth result: %w", err)
	}
	var result struct {
		OK bool `json:"ok"`
	}
	if err := json.Unmarshal([]byte(line), &result); err != nil {
		return fmt.Errorf("invalid relay auth result")
	}
	if !result.OK {
		return fmt.Errorf("relay auth rejected")
	}
	return conn.SetDeadline(time.Time{})
}

// computeRelayMAC binds the relay identity, nonce and protocol version with HMAC-SHA256.
func computeRelayMAC(token []byte, relayID, nonce string, version int) []byte {
	mac := hmac.New(sha256.New, token)
	_, _ = io.WriteString(mac, fmt.Sprintf("relay_id=%s\nnonce=%s\nversion=%d", relayID, nonce, version))
	return mac.Sum(nil)
}

// socketRoundTrip sends a raw text line and reads a raw text response (v1).
func socketRoundTrip(socketPath, command string, refreshAddr func() string) (string, error) {
	conn, err := dialSocket(socketPath, refreshAddr)
	if err != nil {
		return "", fmt.Errorf("failed to connect to %s: %w", socketPath, err)
	}
	defer conn.Close()

	deadline := time.Now().Add(15 * time.Second)
	if err := conn.SetDeadline(deadline); err != nil {
		return "", fmt.Errorf("set request deadline: %w", err)
	}
	if _, err := fmt.Fprintf(conn, "%s\n", command); err != nil {
		return "", fmt.Errorf("failed to send command: %w", err)
	}

	// V1 handlers may return multiple lines (e.g. list_windows). Read until
	// the stream goes idle briefly after seeing at least one newline.
	reader := bufio.NewReader(conn)
	var response strings.Builder
	sawNewline := false

	for {
		readTimeout := 15 * time.Second
		if sawNewline {
			readTimeout = 120 * time.Millisecond
		}
		readDeadline := time.Now().Add(readTimeout)
		if readDeadline.After(deadline) {
			readDeadline = deadline
		}
		if err := conn.SetReadDeadline(readDeadline); err != nil {
			return "", fmt.Errorf("set response deadline: %w", err)
		}

		chunk, err := readRelayLine(reader, maxRPCFrameBytes-response.Len())
		if chunk != "" {
			response.WriteString(chunk)
			if strings.Contains(chunk, "\n") {
				sawNewline = true
			}
		}

		if err != nil {
			if netErr, ok := err.(net.Error); ok && netErr.Timeout() {
				if sawNewline && time.Now().Before(deadline) {
					break
				}
				return "", fmt.Errorf("failed to read response: timeout waiting for response")
			}
			if errors.Is(err, io.EOF) {
				break
			}
			return "", fmt.Errorf("failed to read response: %w", err)
		}
	}

	return strings.TrimRight(response.String(), "\n"), nil
}

// socketRoundTripV2 sends a JSON-RPC request and returns the result JSON.
func socketRoundTripV2(socketPath, method string, params map[string]any, refreshAddr func() string) (string, error) {
	conn, err := dialSocket(socketPath, refreshAddr)
	if err != nil {
		return "", fmt.Errorf("failed to connect to %s: %w", socketPath, err)
	}
	defer conn.Close()

	id := randomHex(8)
	req := map[string]any{
		"id":     id,
		"method": method,
	}
	if params != nil {
		req["params"] = params
	} else {
		req["params"] = map[string]any{}
	}

	payload, err := json.Marshal(req)
	if err != nil {
		return "", fmt.Errorf("failed to marshal request: %w", err)
	}

	line, err := exchangeRelayRequest(conn, payload, relayRequestTimeout(method, params))
	if err != nil {
		return "", err
	}

	// Parse the response to check for errors
	var resp map[string]any
	if err := json.Unmarshal([]byte(line), &resp); err != nil {
		return strings.TrimRight(line, "\n"), nil
	}

	if ok, _ := resp["ok"].(bool); !ok {
		if errObj, _ := resp["error"].(map[string]any); errObj != nil {
			code, _ := errObj["code"].(string)
			msg, _ := errObj["message"].(string)
			return "", fmt.Errorf("server error [%s]: %s", code, msg)
		}
		return "", fmt.Errorf("server returned error response")
	}

	// Return the result portion as JSON
	if result, ok := resp["result"]; ok {
		resultJSON, err := json.Marshal(result)
		if err != nil {
			return "", fmt.Errorf("failed to marshal result: %w", err)
		}
		return string(resultJSON), nil
	}

	return "{}", nil
}

// randomHex creates a hexadecimal request identity; entropy-read errors currently leave zero-valued bytes.
func randomHex(n int) string {
	b := make([]byte, n)
	_, _ = rand.Read(b)
	return hex.EncodeToString(b)
}

// readRelayLine bounds one line including its newline and fails without draining an oversized peer.
// Partial data and EOF/timeout are preserved for the legacy multiline response reader.
func readRelayLine(reader *bufio.Reader, limit int) (string, error) {
	var line strings.Builder
	for {
		chunk, err := reader.ReadSlice('\n')
		if len(chunk) > limit-line.Len() {
			return "", fmt.Errorf("relay response exceeds byte limit %d", limit)
		}
		line.Write(chunk)
		if !errors.Is(err, bufio.ErrBufferFull) {
			return line.String(), err
		}
	}
}

// relayRequestTimeout allows thirty seconds, extending explicit browser waits by five seconds.
func relayRequestTimeout(method string, params map[string]any) time.Duration {
	const fallback = 30 * time.Second
	if method != "browser.wait" {
		return fallback
	}
	milliseconds, ok := params["timeout_ms"].(float64)
	// JSON RPC parameters decode as float64. Bound conversion before duration arithmetic.
	const maxMilliseconds = float64((1<<63-1)/int64(time.Millisecond)) - 5000
	if !ok || milliseconds <= 0 || milliseconds >= maxMilliseconds {
		return fallback
	}
	requested := time.Duration(milliseconds)*time.Millisecond + 5*time.Second
	if requested > fallback {
		return requested
	}
	return fallback
}

// exchangeRelayRequest bounds both request writes and response reads with a single deadline.
// The caller owns closing the connection on every outcome.
func exchangeRelayRequest(conn net.Conn, payload []byte, timeout time.Duration) (string, error) {
	if err := conn.SetDeadline(time.Now().Add(timeout)); err != nil {
		return "", fmt.Errorf("set relay deadline: %w", err)
	}
	if _, err := conn.Write(append(payload, '\n')); err != nil {
		return "", fmt.Errorf("failed to send request: %w", err)
	}
	line, err := readRelayLine(bufio.NewReader(conn), maxRPCFrameBytes)
	if err != nil {
		return "", fmt.Errorf("failed to read response: %w", err)
	}
	return line, nil
}
