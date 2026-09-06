package main

import (
	"fmt"
	"net"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"
)

// TestRemoteListenerHelper is invoked by a real PTY shell to own an actual TCP listener.
func TestRemoteListenerHelper(t *testing.T) {
	if os.Getenv("CMUX_LISTENER_HELPER") != "1" {
		return
	}
	listener, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	directory := os.Getenv("CMUX_LISTENER_DIRECTORY")
	if err := os.WriteFile(filepath.Join(directory, "port"), []byte(strconv.Itoa(listener.Addr().(*net.TCPAddr).Port)), 0600); err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(15 * time.Second)
	for time.Now().Before(deadline) {
		if _, err := os.Stat(filepath.Join(directory, "stop")); err == nil {
			return
		}
		time.Sleep(20 * time.Millisecond)
	}
	t.Fatal("listener helper was not stopped")
}

// TestRemotePortsList attributes a real PTY child, excludes the caller's socket and rejects a retired stream.
func TestRemotePortsList(t *testing.T) {
	t.Setenv("SHELL", "/bin/sh")
	directory := t.TempDir()
	t.Setenv("CMUX_LISTENER_DIRECTORY", directory)
	foreign, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer foreign.Close()
	server := &rpcServer{nextStreamID: 1, streams: map[string]*streamState{}}
	defer server.closeAll()
	opened := server.handleSessionSpawn(rpcRequest{ID: 1, Params: map[string]any{"cwd": directory}})
	if !opened.OK {
		t.Fatalf("spawn: %#v", opened.Error)
	}
	id := opened.Result.(map[string]any)["stream_id"].(string)
	stream, _ := server.getStream(id)
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	quoted := "'" + strings.ReplaceAll(executable, "'", "'\\''") + "'"
	command := fmt.Sprintf("CMUX_LISTENER_HELPER=1 %s -test.run '^TestRemoteListenerHelper$'\n", quoted)
	if _, err := stream.conn.Write([]byte(command)); err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(5 * time.Second)
	port := 0
	for time.Now().Before(deadline) {
		data, err := os.ReadFile(filepath.Join(directory, "port"))
		if err == nil {
			port, _ = strconv.Atoi(string(data))
			if port > 0 {
				break
			}
		}
		time.Sleep(20 * time.Millisecond)
	}
	if port == 0 {
		t.Fatal("PTY listener did not start")
	}
	response := server.handlePortsList(rpcRequest{ID: 2, Params: map[string]any{"stream_id": id}})
	if !response.OK {
		t.Fatalf("scan: %#v", response.Error)
	}
	ports := response.Result.(map[string]any)["ports"].([]remoteListener)
	found := false
	for _, row := range ports {
		if int(row.Port) == foreign.Addr().(*net.TCPAddr).Port {
			t.Fatal("unrelated listener attributed")
		}
		if int(row.Port) == port && row.Address == "127.0.0.1" && row.Provenance == "remote" {
			found = true
		}
	}
	if !found {
		t.Fatalf("missing owned port: %#v", ports)
	}
	if err := os.WriteFile(filepath.Join(directory, "stop"), nil, 0600); err != nil {
		t.Fatal(err)
	}
	server.closeAll()
	if server.handlePortsList(rpcRequest{ID: 3, Params: map[string]any{"stream_id": id}}).OK {
		t.Fatal("retired stream accepted")
	}
}
