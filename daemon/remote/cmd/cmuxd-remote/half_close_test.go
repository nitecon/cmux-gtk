package main

import (
	"io"
	"net"
	"testing"
	"time"
)

// TestProxyHalfClosePreservesResponse uses a real server that replies only after receiving request EOF.
func TestProxyHalfClosePreservesResponse(t *testing.T) {
	listener, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	completed := make(chan error, 1)
	go func() {
		connection, err := listener.Accept()
		if err != nil {
			completed <- err
			return
		}
		defer connection.Close()
		_ = connection.SetDeadline(time.Now().Add(5 * time.Second))
		request, err := io.ReadAll(io.LimitReader(connection, 1024))
		if err == nil && string(request) != "request" {
			err = io.ErrUnexpectedEOF
		}
		if err == nil {
			_, err = connection.Write([]byte("response"))
		}
		completed <- err
	}()
	connection, err := net.DialTimeout("tcp", listener.Addr().String(), time.Second)
	if err != nil {
		t.Fatal(err)
	}
	_ = connection.SetDeadline(time.Now().Add(5 * time.Second))
	server := &rpcServer{nextStreamID: 1, streams: map[string]*streamState{}}
	defer server.closeAll()
	id := server.registerStream(connection)
	if _, err = connection.Write([]byte("request")); err != nil {
		t.Fatal(err)
	}
	result := server.handleProxyShutdownWrite(rpcRequest{ID: 1, Params: map[string]any{"stream_id": id}})
	if !result.OK {
		t.Fatalf("shutdown: %#v", result.Error)
	}
	response, err := io.ReadAll(io.LimitReader(connection, 1024))
	if err != nil || string(response) != "response" {
		t.Fatalf("response %q: %v", response, err)
	}
	if err := <-completed; err != nil {
		t.Fatal(err)
	}
	left, right := net.Pipe()
	defer right.Close()
	pipeID := server.registerStream(left)
	if server.handleProxyShutdownWrite(rpcRequest{ID: 2, Params: map[string]any{"stream_id": pipeID}}).OK {
		t.Fatal("non-TCP stream accepted")
	}
}
