package main

import (
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/creack/pty"
)

// TestPTYCloseReapsIgnoringShell exercises escalation against a real shell that ignores HUP and TERM.
func TestPTYCloseReapsIgnoringShell(t *testing.T) {
	directory := t.TempDir()
	shell := filepath.Join(directory, "stubborn-shell")
	if err := os.WriteFile(shell, []byte("#!/bin/sh\ntrap '' HUP TERM\necho ready > ready\nwhile :; do :; done\n"), 0700); err != nil {
		t.Fatal(err)
	}
	conn, err := startPTY(shell, directory, 80, 24)
	if err != nil {
		t.Fatal(err)
	}
	// Ensure failures before Close also release the native child and descriptor.
	defer func() {
		_ = conn.cmd.Process.Kill()
		_ = conn.ptmx.Close()
		if conn.cmd.ProcessState == nil {
			_ = conn.cmd.Wait()
		}
	}()
	deadline := time.Now().Add(5 * time.Second)
	for {
		if _, err := os.Stat(filepath.Join(directory, "ready")); err == nil {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("shell did not install its signal handlers")
		}
		time.Sleep(10 * time.Millisecond)
	}
	started := time.Now()
	watchdog := time.AfterFunc(5*time.Second, func() { _ = conn.cmd.Process.Kill() })
	defer watchdog.Stop()
	if err := conn.Close(); err != nil {
		t.Fatal(err)
	}
	if time.Since(started) > 5*time.Second {
		t.Fatal("shell shutdown exceeded the CI lifecycle allowance")
	}
	if conn.cmd.ProcessState == nil {
		t.Fatal("shell was not reaped before Close returned")
	}
}

// TestPTYRejectsOversizedDimensions verifies rejected resize requests leave the native grid intact.
func TestPTYRejectsOversizedDimensions(t *testing.T) {
	if conn, err := startPTY("/bin/sh", t.TempDir(), 65536, 24); err == nil {
		_ = conn.Close()
		t.Fatal("oversized grid reached PTY startup")
	}
	conn, err := startPTY("/bin/sh", t.TempDir(), 80, 24)
	if err != nil {
		t.Fatal(err)
	}
	defer conn.Close()
	for _, dimensions := range [][2]int{{0, 24}, {80, -1}, {65536, 24}, {80, 65536}} {
		if err := conn.resize(dimensions[0], dimensions[1]); err == nil {
			t.Fatalf("invalid resize accepted: %v", dimensions)
		}
	}
	size, err := pty.GetsizeFull(conn.ptmx)
	if err != nil {
		t.Fatal(err)
	}
	if size.Cols != 80 || size.Rows != 24 {
		t.Fatalf("rejected resize mutated grid: %+v", size)
	}
}
