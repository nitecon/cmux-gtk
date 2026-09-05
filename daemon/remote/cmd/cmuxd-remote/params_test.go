package main

import (
	"encoding/json"
	"math"
	"strconv"
	"testing"
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
