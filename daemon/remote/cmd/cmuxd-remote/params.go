package main

import (
	"encoding/json"
	"fmt"
	"math"
	"strconv"
	"time"
)

// getStringParam accepts only present non-null string parameters.
func getStringParam(params map[string]any, key string) (string, bool) {
	if params == nil {
		return "", false
	}
	raw, ok := params[key]
	if !ok || raw == nil {
		return "", false
	}
	value, ok := raw.(string)
	return value, ok
}

// getIntParam accepts only integral numeric values representable by the native int type.
func getIntParam(params map[string]any, key string) (int, bool) {
	if params == nil {
		return 0, false
	}
	raw, ok := params[key]
	if !ok || raw == nil {
		return 0, false
	}
	switch value := raw.(type) {
	case int:
		return value, true
	case int8:
		return int(value), true
	case int16:
		return int(value), true
	case int32:
		return int(value), true
	case int64:
		return checkedSignedInt(value)
	case uint:
		return checkedUnsignedInt(uint64(value))
	case uint8:
		return checkedUnsignedInt(uint64(value))
	case uint16:
		return checkedUnsignedInt(uint64(value))
	case uint32:
		return checkedUnsignedInt(uint64(value))
	case uint64:
		return checkedUnsignedInt(uint64(value))
	case float64:
		// The upper bound is exclusive: float64(maxInt) rounds up on 64-bit hosts.
		bound := math.Ldexp(1, strconv.IntSize-1)
		if math.Trunc(value) != value || value < -bound || value >= bound {
			return 0, false
		}
		return int(value), true
	case json.Number:
		n, err := value.Int64()
		if err != nil {
			return 0, false
		}
		return checkedSignedInt(n)
	default:
		return 0, false
	}
}

// checkedSignedInt rejects narrowing conversions that would wrap on a smaller native integer width.
func checkedSignedInt(value int64) (int, bool) {
	converted := int(value)
	return converted, int64(converted) == value
}

// checkedUnsignedInt rejects values above the largest positive native integer.
func checkedUnsignedInt(value uint64) (int, bool) {
	return int(value), value <= uint64(^uint(0)>>1)
}

// getTimeoutParam applies an omitted default and accepts nonnegative, representable milliseconds.
// Zero explicitly disables the transport deadline; malformed or overflowing values are errors.
func getTimeoutParam(params map[string]any, fallback time.Duration) (time.Duration, error) {
	if _, present := params["timeout_ms"]; !present {
		return fallback, nil
	}
	milliseconds, ok := getIntParam(params, "timeout_ms")
	const maxMilliseconds = int64(1<<63-1) / int64(time.Millisecond)
	if !ok || milliseconds < 0 || int64(milliseconds) > maxMilliseconds {
		return 0, fmt.Errorf("timeout_ms must be a nonnegative integer no greater than %d", maxMilliseconds)
	}
	return time.Duration(milliseconds) * time.Millisecond, nil
}
