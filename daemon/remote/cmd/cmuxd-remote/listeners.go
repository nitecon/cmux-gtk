package main

import (
	"encoding/binary"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
)

// remoteListener records a TCP socket owned by a descendant of one registered PTY.
type remoteListener struct {
	Address    string `json:"address"`
	Port       uint16 `json:"port"`
	PID        int    `json:"pid"`
	Provenance string `json:"provenance"`
}

// listenerProcess qualifies PID reuse and current ancestry from one bounded stat record.
type listenerProcess struct {
	pid, parent int
	start       string
}

// listenerRead rejects oversized procfs data rather than accepting a truncated ownership record.
func listenerRead(path string, limit int64) ([]byte, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	data, err := io.ReadAll(io.LimitReader(file, limit+1))
	if err == nil && int64(len(data)) > limit {
		err = fmt.Errorf("listener data limit exceeded")
	}
	return data, err
}

// listenerIdentity captures PID, parent and start ticks without command/environment inspection.
func listenerIdentity(pid int) (listenerProcess, error) {
	data, err := listenerRead(fmt.Sprintf("/proc/%d/stat", pid), 8192)
	if err != nil {
		return listenerProcess{}, err
	}
	end := strings.LastIndexByte(string(data), ')')
	if end < 0 {
		return listenerProcess{}, fmt.Errorf("invalid process identity")
	}
	fields := strings.Fields(string(data[end+1:]))
	if len(fields) < 20 {
		return listenerProcess{}, fmt.Errorf("invalid process identity")
	}
	parent, err := strconv.Atoi(fields[1])
	if err != nil {
		return listenerProcess{}, err
	}
	return listenerProcess{pid: pid, parent: parent, start: fields[19]}, nil
}

// listenerNames bounds directory enumeration, including descriptors and spawning threads.
func listenerNames(path string, limit int) ([]string, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	names, err := file.Readdirnames(limit + 1)
	if err == io.EOF {
		err = nil
	}
	if len(names) > limit {
		return nil, fmt.Errorf("listener directory limit exceeded")
	}
	return names, err
}

// listenerTree discovers up to 256 current descendants across all spawning threads.
func listenerTree(root listenerProcess) ([]listenerProcess, error) {
	pending := []listenerProcess{root}
	result := []listenerProcess{}
	seen := map[int]bool{}
	for len(pending) > 0 {
		parent := pending[len(pending)-1]
		pending = pending[:len(pending)-1]
		if seen[parent.pid] {
			continue
		}
		seen[parent.pid] = true
		if len(seen) > 256 {
			return nil, fmt.Errorf("listener process limit exceeded")
		}
		before, err := listenerIdentity(parent.pid)
		if os.IsNotExist(err) {
			continue
		}
		if err != nil {
			return nil, err
		}
		if before.start != parent.start {
			continue
		}
		tasks, err := listenerNames(fmt.Sprintf("/proc/%d/task", parent.pid), 1024)
		if os.IsNotExist(err) {
			continue
		}
		if err != nil {
			return nil, err
		}
		children := map[int]bool{}
		for _, task := range tasks {
			data, err := listenerRead(fmt.Sprintf("/proc/%d/task/%s/children", parent.pid, task), 65536)
			if os.IsNotExist(err) {
				continue
			}
			if err != nil {
				return nil, err
			}
			for _, text := range strings.Fields(string(data)) {
				pid, err := strconv.Atoi(text)
				if err != nil {
					return nil, err
				}
				children[pid] = true
				if len(children) > 256 {
					return nil, fmt.Errorf("listener child limit exceeded")
				}
			}
		}
		after, err := listenerIdentity(parent.pid)
		if os.IsNotExist(err) {
			continue
		}
		if err != nil {
			return nil, err
		}
		if after.start != parent.start {
			continue
		}
		result = append(result, parent)
		for pid := range children {
			child, err := listenerIdentity(pid)
			if os.IsNotExist(err) {
				continue
			}
			if err != nil {
				return nil, err
			}
			if child.parent == parent.pid {
				if len(pending)+len(seen) >= 256 {
					return nil, fmt.Errorf("listener process limit exceeded")
				}
				pending = append(pending, child)
			}
		}
	}
	return result, nil
}

// collectRemoteListeners intersects LISTEN tables with descriptor ownership and rechecks PID identity.
func collectRemoteListeners(root listenerProcess) ([]remoteListener, error) {
	processes, err := listenerTree(root)
	if err != nil {
		return nil, err
	}
	result := []remoteListener{}
	for _, process := range processes {
		base := fmt.Sprintf("/proc/%d", process.pid)
		names, err := listenerNames(filepath.Join(base, "fd"), 4096)
		if os.IsNotExist(err) {
			continue
		}
		if err != nil {
			return nil, err
		}
		sockets := map[string]bool{}
		for _, name := range names {
			link, err := os.Readlink(filepath.Join(base, "fd", name))
			if os.IsNotExist(err) {
				continue
			}
			if err != nil {
				return nil, err
			}
			if strings.HasPrefix(link, "socket:[") && strings.HasSuffix(link, "]") {
				sockets[strings.TrimSuffix(strings.TrimPrefix(link, "socket:["), "]")] = true
			}
		}
		owned := []remoteListener{}
		for _, table := range []string{"tcp", "tcp6"} {
			data, err := listenerRead(filepath.Join(base, "net", table), 2*1024*1024)
			if os.IsNotExist(err) {
				continue
			}
			if err != nil {
				return nil, err
			}
			for _, line := range strings.Split(string(data), "\n") {
				fields := strings.Fields(line)
				if len(fields) < 10 || fields[3] != "0A" || !sockets[fields[9]] {
					continue
				}
				address, port, ok := listenerAddress(fields[1])
				if !ok {
					continue
				}
				if len(owned)+len(result) >= 256 {
					return nil, fmt.Errorf("listener result limit exceeded")
				}
				owned = append(owned, remoteListener{address, port, process.pid, "remote"})
			}
		}
		current, err := listenerIdentity(process.pid)
		if os.IsNotExist(err) {
			continue
		}
		if err != nil {
			return nil, err
		}
		if current.start == process.start {
			result = append(result, owned...)
		}
	}
	current, err := listenerIdentity(root.pid)
	if err != nil || current.start != root.start {
		return nil, fmt.Errorf("terminal process changed during listener scan")
	}
	return result, nil
}

// listenerAddress decodes Linux native-endian address words and a hexadecimal TCP port.
func listenerAddress(value string) (string, uint16, bool) {
	address, portText, ok := strings.Cut(value, ":")
	if !ok || (len(address) != 8 && len(address) != 32) {
		return "", 0, false
	}
	port, err := strconv.ParseUint(portText, 16, 16)
	if err != nil {
		return "", 0, false
	}
	bytes := make([]byte, len(address)/2)
	for offset := 0; offset < len(address); offset += 8 {
		word, err := strconv.ParseUint(address[offset:offset+8], 16, 32)
		if err != nil {
			return "", 0, false
		}
		binary.NativeEndian.PutUint32(bytes[offset/2:], uint32(word))
	}
	return net.IP(bytes).String(), uint16(port), true
}

// handlePortsList scans only a registered PTY stream; retired streams cannot publish stale results.
func (s *rpcServer) handlePortsList(req rpcRequest) rpcResponse {
	fail := func(code, message string) rpcResponse {
		return rpcResponse{ID: req.ID, OK: false, Error: &rpcError{Code: code, Message: message}}
	}
	if runtime.GOOS != "linux" {
		return fail("unsupported", "listener discovery requires Linux")
	}
	id, ok := getStringParam(req.Params, "stream_id")
	if !ok || id == "" {
		return fail("invalid_params", "ports.list requires stream_id")
	}
	stream, ok := s.getStream(id)
	if !ok {
		return fail("not_found", "PTY stream not found")
	}
	pty, ok := stream.conn.(*ptyConn)
	if !ok || pty.cmd.Process == nil {
		return fail("invalid_params", "stream is not a PTY")
	}
	root, err := listenerIdentity(pty.cmd.Process.Pid)
	if err != nil {
		return fail("scan_failed", "terminal process unavailable")
	}
	ports, err := collectRemoteListeners(root)
	if err != nil {
		return fail("scan_failed", "listener scan unavailable or exceeded limits")
	}
	current, ok := s.getStream(id)
	if !ok || current != stream {
		return fail("not_found", "PTY stream closed during scan")
	}
	return rpcResponse{ID: req.ID, OK: true, Result: map[string]any{"stream_id": id, "ports": ports}}
}
