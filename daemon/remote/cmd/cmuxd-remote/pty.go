package main

import (
	"errors"
	"fmt"
	"net"
	"os"
	"os/exec"
	"syscall"
	"time"
	"unsafe"

	"github.com/creack/pty"
)

const ptyTerminationGrace = 500 * time.Millisecond

// ptyWindowSize rejects dimensions that cannot be represented by the native PTY ABI.
func ptyWindowSize(cols, rows int) (*pty.Winsize, error) {
	if cols < 1 || rows < 1 || cols > 65535 || rows > 65535 {
		return nil, errors.New("PTY dimensions must be between 1 and 65535")
	}
	return &pty.Winsize{Cols: uint16(cols), Rows: uint16(rows)}, nil
}

// ptyConn adapts a Unix PTY to stream transport. The stream registry owns its
// single Close call, which also terminates and reaps the child shell.
type ptyConn struct {
	ptmx *os.File
	cmd  *exec.Cmd
}

// startPTY launches a login shell with the requested directory and initial grid.
// The caller must register the returned connection or close it on failure.
func startPTY(shell, directory string, cols, rows int) (*ptyConn, error) {
	size, err := ptyWindowSize(cols, rows)
	if err != nil {
		return nil, err
	}
	cmd := exec.Command(shell, "-l")
	cmd.Dir = directory
	cmd.Env = append(os.Environ(), "TERM=xterm-256color")
	ptmx, err := pty.StartWithSize(cmd, size)
	if err != nil {
		return nil, err
	}
	conn := &ptyConn{ptmx: ptmx, cmd: cmd}
	pollable, err := pollablePTY(ptmx)
	if err != nil {
		_ = conn.Close()
		return nil, fmt.Errorf("prepare PTY polling: %w", err)
	}
	_ = ptmx.Close()
	conn.ptmx = pollable
	return conn, nil
}

// Read receives terminal output from the PTY master.
func (p *ptyConn) Read(b []byte) (int, error) { return p.ptmx.Read(b) }

// Write delivers input to the child terminal.
func (p *ptyConn) Write(b []byte) (int, error) { return p.ptmx.Write(b) }

// Close releases the PTY, allows a short termination grace period, then kills and reaps a stubborn shell.
// Call exactly once through stream ownership; child exit errors are best-effort.
func (p *ptyConn) Close() error {
	_ = p.ptmx.Close()
	if p.cmd != nil && p.cmd.Process != nil {
		_ = p.cmd.Process.Signal(syscall.SIGTERM)
		done := make(chan struct{})
		go func() {
			_ = p.cmd.Wait()
			close(done)
		}()
		timer := time.NewTimer(ptyTerminationGrace)
		defer timer.Stop()
		select {
		case <-done:
		case <-timer.C:
			_ = p.cmd.Process.Kill()
			<-done
		}
	}
	return nil
}

// LocalAddr returns nil because a PTY has no network endpoint.
func (p *ptyConn) LocalAddr() net.Addr { return nil }

// RemoteAddr returns nil because the peer is a child process rather than a socket.
func (p *ptyConn) RemoteAddr() net.Addr { return nil }

// SetDeadline updates pending and future PTY reads and writes; zero clears the deadline.
func (p *ptyConn) SetDeadline(deadline time.Time) error { return p.ptmx.SetDeadline(deadline) }

// SetReadDeadline bounds pending and future terminal output reads.
func (p *ptyConn) SetReadDeadline(deadline time.Time) error { return p.ptmx.SetReadDeadline(deadline) }

// SetWriteDeadline bounds input writes when the child stops consuming terminal data.
func (p *ptyConn) SetWriteDeadline(deadline time.Time) error {
	return p.ptmx.SetWriteDeadline(deadline)
}

// resize applies the grid dimensions to the PTY through its native window-size ioctl.
func (p *ptyConn) resize(cols, rows int) error {
	size, err := ptyWindowSize(cols, rows)
	if err != nil {
		return err
	}
	raw, err := p.ptmx.SyscallConn()
	if err != nil {
		return err
	}
	var ioctlErr syscall.Errno
	// Control retains descriptor ownership and avoids Fd switching the file to blocking I/O.
	err = raw.Control(func(fd uintptr) {
		_, _, ioctlErr = syscall.Syscall(syscall.SYS_IOCTL, fd, syscall.TIOCSWINSZ, uintptr(unsafe.Pointer(size)))
	})
	if err != nil {
		return err
	}
	if ioctlErr != 0 {
		return ioctlErr
	}
	return nil
}

// pollablePTY duplicates the master into a nonblocking os.File registered with Go's I/O poller.
// The caller retains the original file and must close it after a successful transfer.
func pollablePTY(master *os.File) (*os.File, error) {
	// Protect the brief dup/close-on-exec interval from concurrent child launches.
	syscall.ForkLock.RLock()
	fd, err := syscall.Dup(int(master.Fd()))
	if err == nil {
		syscall.CloseOnExec(fd)
	}
	syscall.ForkLock.RUnlock()
	if err != nil {
		return nil, err
	}
	if err := syscall.SetNonblock(fd, true); err != nil {
		_ = syscall.Close(fd)
		return nil, err
	}
	file := os.NewFile(uintptr(fd), master.Name())
	if file == nil {
		_ = syscall.Close(fd)
		return nil, errors.New("invalid duplicated PTY descriptor")
	}
	if err := file.SetDeadline(time.Time{}); err != nil {
		_ = file.Close()
		return nil, err
	}
	return file, nil
}
