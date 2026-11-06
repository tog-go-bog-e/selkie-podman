//! A pseudo-terminal running an interactive `podman` client.
//!
//! The PTY is allocated with `openpty`, the client (`podman exec` for a
//! container, `podman machine ssh` for a VM) is attached to the slave side, and
//! the master side is read on a thread and fed into a terminal emulator
//! ([`vt100`]) that the UI renders.

use std::fs::File;
use std::io::{Read, Write};
use std::num::NonZeroU16;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

pub const TERMINAL_ROWS: u16 = 24;
pub const TERMINAL_COLS: u16 = 50;
/// Lines of history kept by the emulator for scrolling back.
pub const TERMINAL_SCROLLBACK: usize = 2000;

/// One interactive client attached through a pseudo-terminal.
pub struct TerminalSession {
    master: Option<File>,
    stop: Arc<AtomicBool>,
    child: Option<Child>,
    parser: vt100::Parser,
    rows: u16,
    cols: u16,
    receiver: Receiver<Vec<u8>>,
    /// Set when the exec session has ended (shell exit, error or engine loss).
    pub closed: bool,
}

impl TerminalSession {
    pub fn start(mut command: Command, rows: u16, cols: u16) -> Result<Self, String> {
        let mut master_fd: libc::c_int = -1;
        let mut slave_fd: libc::c_int = -1;
        let winsize = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        // SAFETY: both descriptors and the winsize pointer are valid; openpty
        // fills the descriptors in.
        let opened = unsafe {
            libc::openpty(
                &mut master_fd,
                &mut slave_fd,
                std::ptr::null_mut(),
                std::ptr::null(),
                &winsize,
            )
        };
        if opened != 0 {
            return Err(format!(
                "could not allocate a pseudo-terminal: {}",
                std::io::Error::last_os_error()
            ));
        }

        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        // SAFETY: `pre_exec` runs in the forked child before exec; all calls are
        // async-signal-safe and use only the captured descriptor.
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::process::CommandExt;
            // If the parent dies between fork and `prctl`, the signal is lost; the
            // `getppid` check below closes that window (the usual PDEATHSIG idiom).
            let parent = std::process::id() as libc::pid_t;
            unsafe {
                command.pre_exec(move || {
                    // Die with the app even when it is killed without unwinding
                    // (SIGKILL). `setsid` detaches the *session*, not the parent,
                    // so the parent-death signal still applies.
                    if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if libc::getppid() != parent {
                        libc::_exit(1);
                    }
                    libc::setsid();
                    libc::ioctl(slave_fd, libc::TIOCSCTTY as _, 0);
                    libc::dup2(slave_fd, 0);
                    libc::dup2(slave_fd, 1);
                    libc::dup2(slave_fd, 2);
                    if slave_fd > 2 {
                        libc::close(slave_fd);
                    }
                    Ok(())
                });
            }
        }

        let child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                // SAFETY: these descriptors were created above and are not owned
                // by anything else yet.
                unsafe {
                    libc::close(master_fd);
                    libc::close(slave_fd);
                }
                return Err(format!("could not run podman: {error}"));
            }
        };

        // The parent keeps only the master side; the slave is held by the child.
        // SAFETY: `master_fd`/`slave_fd` are owned by this function at this point.
        let master = unsafe {
            libc::close(slave_fd);
            libc::fcntl(master_fd, libc::F_SETFD, libc::FD_CLOEXEC);
            File::from_raw_fd(master_fd)
        };

        // The reader uses a duplicate so it can exit when asked without fighting
        // the writer. `O_NONBLOCK` is shared by both descriptions, which is fine:
        // the reader polls and writes are tiny.
        // SAFETY: `master` is a valid descriptor; `dup` returns a new one.
        let reader_fd = unsafe { libc::dup(master.as_raw_fd()) };
        if reader_fd < 0 {
            let mut child = child;
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "could not duplicate the pseudo-terminal: {}",
                std::io::Error::last_os_error()
            ));
        }
        // SAFETY: `reader_fd` is a freshly duplicated descriptor we own.
        let reader = unsafe { File::from_raw_fd(reader_fd) };
        // SAFETY: valid descriptor; sets the non-blocking flag.
        unsafe {
            let flags = libc::fcntl(reader_fd, libc::F_GETFL);
            libc::fcntl(reader_fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }

        let stop = Arc::new(AtomicBool::new(false));
        let reader_stop = Arc::clone(&stop);
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("terminal".into())
            .spawn(move || {
                let mut buffer = [0u8; 8192];
                loop {
                    if reader_stop.load(Ordering::Relaxed) {
                        break;
                    }
                    match (&reader).read(&mut buffer) {
                        Ok(0) => break,
                        Ok(read) => {
                            if sender.send(buffer[..read].to_vec()).is_err() {
                                break;
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        // EIO is the normal outcome once the child exits.
                        Err(_) => break,
                    }
                }
            })
            .ok();

        Ok(Self {
            master: Some(master),
            stop,
            child: Some(child),
            parser: vt100::Parser::new(
                NonZeroU16::new(rows).unwrap_or(NonZeroU16::MIN),
                NonZeroU16::new(cols).unwrap_or(NonZeroU16::MIN),
                TERMINAL_SCROLLBACK,
            ),
            rows,
            cols,
            receiver,
            closed: false,
        })
    }

    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    /// Current grid size as `(rows, cols)`.
    pub fn size(&self) -> (u16, u16) {
        (self.rows, self.cols)
    }

    /// Resize the emulator grid and the kernel's terminal window size.
    ///
    /// `TIOCSWINSZ` makes the kernel send `SIGWINCH` to the foreground process
    /// group, so full-screen programs reflow along with our rendering.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        let rows = rows.max(2);
        let cols = cols.max(20);
        if rows == self.rows && cols == self.cols {
            return;
        }
        self.rows = rows;
        self.cols = cols;
        self.parser.set_size(
            NonZeroU16::new(rows).unwrap_or(NonZeroU16::MIN),
            NonZeroU16::new(cols).unwrap_or(NonZeroU16::MIN),
        );

        if let Some(master) = &self.master {
            let winsize = libc::winsize {
                ws_row: rows,
                ws_col: cols,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            // SAFETY: `master` is the live pty descriptor; TIOCSWINSZ only
            // updates the window size.
            unsafe {
                libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ as _, &winsize);
            }
        }
    }

    /// Rows scrolled back from the live view (0 = at the bottom).
    pub fn scrollback(&self) -> usize {
        self.parser.screen().scrollback()
    }

    /// Scroll by `delta` lines (positive scrolls back); clamped to the buffer.
    pub fn scroll_by(&mut self, delta: isize) -> bool {
        let current = self.parser.screen().scrollback() as isize;
        let max = self.parser.screen().scrollback_len() as isize;
        let target = (current + delta).clamp(0, max);
        if target == current {
            return false;
        }
        self.parser.screen_mut().set_scrollback(target as usize);
        true
    }

    /// Return to the live viewport.
    pub fn scroll_to_bottom(&mut self) {
        if self.scrollback() != 0 {
            self.parser.screen_mut().set_scrollback(0);
        }
    }

    /// Whether the shell requested application cursor keys (`DECCKM`).
    pub fn application_cursor(&self) -> bool {
        self.parser.screen().application_cursor()
    }

    pub fn write(&self, bytes: &[u8]) {
        if let Some(master) = &self.master {
            let mut master = master;
            let _ = master.write_all(bytes);
            let _ = master.flush();
        }
    }

    /// Process pending PTY output. Returns whether the screen changed.
    pub fn drain(&mut self) -> bool {
        let mut changed = false;
        loop {
            match self.receiver.try_recv() {
                Ok(bytes) => {
                    self.parser.process(&bytes);
                    changed = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.closed = true;
                    break;
                }
            }
        }

        // Always try to reap the client: it may exit before or after the reader
        // sees EOF, and a missed wait leaves a zombie behind.
        if let Some(mut child) = self.child.take() {
            match child.try_wait() {
                Ok(Some(_status)) => self.closed = true,
                Ok(None) => self.child = Some(child),
                Err(_) => self.closed = true,
            }
        }

        changed
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        // Close the PTY first: the shell sees a hangup and the `podman exec`
        // client can exit cleanly (killing it leaves a stale conmon behind).
        self.stop.store(true, Ordering::Relaxed);
        self.master.take();

        if let Some(mut child) = self.child.take() {
            std::thread::spawn(move || {
                for _ in 0..20 {
                    if let Ok(Some(_status)) = child.try_wait() {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                let _ = child.kill();
                let _ = child.wait();
            });
        }
    }
}
