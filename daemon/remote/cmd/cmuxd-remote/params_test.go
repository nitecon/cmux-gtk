package main

import (
	"encoding/json"
	"math"
	"net"
	"strconv"
	"testing"
	"time"
)

// TestIntegerParameterBounds exercises the native-width boundary for each externally supplied numeric form.
func TestIntegerParameterBounds(t *testing.T) {
	maxInt := int(^uint(0) >> 1)
	minInt := -maxInt - 1
	bound := math.Ldexp(1, strconv.IntSize-1)
	for _, tc := range []struct {
		name  string
		value any
		want  int
		valid bool
	}{
		{"maximum", maxInt, maxInt, true},
		{"minimum", minInt, minInt, true},
		{"signed", int64(maxInt), maxInt, true},
		{"unsigned", uint64(maxInt), maxInt, true},
		{"unsigned-overflow", uint64(maxInt) + 1, 0, false},
		{"unsigned-maximum", ^uint64(0), 0, false},
		{"json-maximum", json.Number(strconv.Itoa(maxInt)), maxInt, true},
		{"json-overflow", json.Number("18446744073709551615"), 0, false},
		{"float-minimum", -bound, minInt, true},
		{"float-upper-exclusive", bound, 0, false},
		{"float-below-minimum", math.Nextafter(-bound, math.Inf(-1)), 0, false},
		{"fraction", 12.5, 0, false},
		{"nan", math.NaN(), 0, false},
		{"positive-infinity", math.Inf(1), 0, false},
		{"negative-infinity", math.Inf(-1), 0, false},
		{"string", "123", 0, false},
		{"null", nil, 0, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			got, valid := getIntParam(map[string]any{"value": tc.value}, "value")
			if valid != tc.valid || (valid && got != tc.want) {
				t.Fatalf("getIntParam(%v) = (%d, %t), want (%d, %t)", tc.value, got, valid, tc.want, tc.valid)
			}
		})
	}
	if _, ok := getIntParam(nil, "missing"); ok {
		t.Fatal("absent parameter accepted")
	}
}

// TestTimeoutParameterBounds verifies defaults, explicit disabling and multiplication overflow rejection.
func TestTimeoutParameterBounds(t *testing.T) {
	for _, tc := range []struct {
		name   string
		params map[string]any
		want   time.Duration
		valid  bool
	}{
		{"default", nil, 8 * time.Second, true},
		{"disabled", map[string]any{"timeout_ms": 0}, 0, true},
		{"positive", map[string]any{"timeout_ms": 1200}, 1200 * time.Millisecond, true},
		{"negative", map[string]any{"timeout_ms": -1}, 0, false},
		{"fractional", map[string]any{"timeout_ms": 0.5}, 0, false},
		{"null", map[string]any{"timeout_ms": nil}, 0, false},
		{"overflow", map[string]any{"timeout_ms": json.Number("9223372036855")}, 0, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			got, err := getTimeoutParam(tc.params, 8*time.Second)
			if (err == nil) != tc.valid || (tc.valid && got != tc.want) {
				t.Fatalf("timeout = %s, error %v; want %s, valid %t", got, err, tc.want, tc.valid)
			}
		})
	}
}

// TestProxyRejectsInvalidTimeouts verifies dispatch rejects malformed deadlines before performing transport I/O.
func TestProxyRejectsInvalidTimeouts(t *testing.T) {
	client, peer := net.Pipe()
	defer client.Close()
	defer peer.Close()
	server := &rpcServer{streams: map[string]*streamState{"existing": {conn: client}}}
	for _, method := range []string{"proxy.open", "proxy.write"} {
		for _, value := range []any{-1, "100", nil, json.Number("9223372036855")} {
			response := server.handleRequest(rpcRequest{ID: 1, Method: method, Params: map[string]any{
				"host": "127.0.0.1", "port": 1,
				"stream_id": "existing", "data_base64": "",
				"timeout_ms": value,
			}})
			if response.OK || response.Error == nil || response.Error.Code != "invalid_params" {
				t.Fatalf("%s timeout %v returned %+v", method, value, response)
			}
		}
	}
}
