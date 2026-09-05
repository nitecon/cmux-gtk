package main

import (
	"bufio"
	"errors"
	"io"
	"net"
	"strings"
	"testing"
	"time"
)

// TestReadRelayLineBounds verifies framing, partial EOF and limits across reader fragments.
func TestReadRelayLineBounds(t *testing.T) {
	for _, tc := range []struct {
		name, input, want  string
		limit              int
		wantEOF, oversized bool
	}{
		{"exact", "abc\n", "abc\n", 4, false, false},
		{"empty", "", "", 4, true, false},
		{"partial", "abc", "abc", 4, true, false},
		{"oversized", "abcd\n", "", 4, false, true},
		{"fragmented", strings.Repeat("x", 10000) + "\n", "", 8000, false, true},
		{"zero", "\n", "", 0, false, true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			got, err := readRelayLine(bufio.NewReaderSize(strings.NewReader(tc.input), 16), tc.limit)
			if got != tc.want {
				t.Fatalf("got %q, want %q", got, tc.want)
			}
			if tc.oversized {
				if err == nil || !strings.Contains(err.Error(), "exceeds byte limit") {
					t.Fatalf("expected size error, got %v", err)
				}
			} else if tc.wantEOF {
				if !errors.Is(err, io.EOF) {
					t.Fatalf("expected EOF, got %v", err)
				}
			} else if err != nil {
				t.Fatal(err)
			}
		})
	}
}

// TestRelayExchangeDeadline exercises both an unread request and a peer that never responds.
func TestRelayExchangeDeadline(t *testing.T) {
	for _, readRequest := range []bool{false, true} {
		name := "write"
		if readRequest {
			name = "read"
		}
		t.Run(name, func(t *testing.T) {
			client, peer := net.Pipe()
			defer client.Close()
			defer peer.Close()
			watchdog := time.AfterFunc(5*time.Second, func() { _ = client.Close(); _ = peer.Close() })
			defer watchdog.Stop()
			if readRequest {
				go func() { _, _ = bufio.NewReader(peer).ReadString('\n') }()
			}
			_, err := exchangeRelayRequest(client, []byte(`{"method":"ping"}`), 50*time.Millisecond)
			var netErr net.Error
			if !errors.As(err, &netErr) || !netErr.Timeout() {
				t.Fatalf("expected %s timeout, got %v", name, err)
			}
		})
	}
}

// TestRelayRejectsOversizedResponses checks both legacy aggregate and JSON line response limits over sockets.
func TestRelayRejectsOversizedResponses(t *testing.T) {
	for _, protocol := range []string{"v1", "v2"} {
		t.Run(protocol, func(t *testing.T) {
			// Short lines ensure v1 bounds the entire response, not merely each line.
			response := strings.Repeat("0123456789abcdef\n", maxRPCFrameBytes/17+1)
			if protocol == "v2" {
				response = strings.Repeat("x", maxRPCFrameBytes+1)
			}
			path := startMockSocket(t, response)
			var err error
			if protocol == "v1" {
				_, err = socketRoundTrip(path, "ping", nil)
			} else {
				_, err = socketRoundTripV2(path, "system.ping", nil, nil)
			}
			if err == nil || !strings.Contains(err.Error(), "exceeds byte limit") {
				t.Fatalf("expected bounded response error, got %v", err)
			}
		})
	}
}

// TestRelayWaitBudget verifies explicit long waits receive transport overhead without extending other methods.
func TestRelayWaitBudget(t *testing.T) {
	params := map[string]any{"timeout_ms": float64(60000)}
	if got := relayRequestTimeout("browser.wait", params); got != 65*time.Second {
		t.Fatalf("wait budget: %s", got)
	}
	if got := relayRequestTimeout("system.ping", params); got != 30*time.Second {
		t.Fatalf("ping budget: %s", got)
	}
	params["timeout_ms"] = float64(1e30)
	if got := relayRequestTimeout("browser.wait", params); got != 30*time.Second {
		t.Fatalf("overflow budget: %s", got)
	}
}
