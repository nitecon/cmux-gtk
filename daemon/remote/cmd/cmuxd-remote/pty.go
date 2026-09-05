package main

import (
	"net"
	"os"
	"os/exec"
	"syscall"
	"time"

	"github.com/creack/pty"
)

// ptyConn adapts a Unix PTY to stream transport. The stream registry owns its
// single Close call, which also terminates and reaps the child shell.
type ptyConn struct {
	ptmx *os.File
	cmd  *exec.Cmd
}

// startPTY launches a login shell with the requested directory and initial grid.
// The caller must register the returned connection or close it on failure.
func startPTY(shell, directory string, cols, rows int) (*ptyConn, error) {
	cmd := exec.Command(shell, "-l")
	cmd.Dir = directory
	cmd.Env = append(os.Environ(), "TERM=xterm-256color")
	ptmx, err := pty.StartWithSize(cmd, &pty.Winsize{Cols: uint16(cols), Rows: uint16(rows)})
	if err != nil {
		return nil, err
	}
	return &ptyConn{ptmx: ptmx, cmd: cmd}, nil
}

// Read receives terminal output from the PTY master.
func (p *ptyConn) Read(b []byte) (int, error) { return p.ptmx.Read(b) }

// Write delivers input to the child terminal.
func (p *ptyConn) Write(b []byte) (int, error) { return p.ptmx.Write(b) }

// Close releases the PTY and reaps the shell after requesting termination.
// Call exactly once through stream ownership; child exit errors are best-effort.
func (p *ptyConn) Close() error {
	_ = p.ptmx.Close()
	if p.cmd != nil && p.cmd.Process != nil {
		_ = p.cmd.Process.Signal(syscall.SIGTERM)
		_ = p.cmd.Wait()
	}
	return nil
}

// LocalAddr returns nil because a PTY has no network endpoint.
func (p *ptyConn) LocalAddr() net.Addr { return nil }

// RemoteAddr returns nil because the peer is a child process rather than a socket.
func (p *ptyConn) RemoteAddr() net.Addr { return nil }

// SetDeadline preserves the stream adapter's no-op deadline behavior for PTYs.
func (p *ptyConn) SetDeadline(time.Time) error { return nil }

// SetReadDeadline accepts the transport interface without changing PTY reads.
func (p *ptyConn) SetReadDeadline(time.Time) error { return nil }

// SetWriteDeadline accepts the transport interface without changing PTY writes.
func (p *ptyConn) SetWriteDeadline(time.Time) error { return nil }

// resize applies the grid dimensions to the PTY through its native window-size ioctl.
func (p *ptyConn) resize(cols, rows int) error {
	return pty.Setsize(p.ptmx, &pty.Winsize{Cols: uint16(cols), Rows: uint16(rows)})
}
