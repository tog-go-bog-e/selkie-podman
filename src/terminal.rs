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
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const TERMINAL_ROWS: u16 = 24;
pub const TERMINAL_COLS: u16 = 50;
/// Lines of history kept by the emulator for scrolling back.
pub const TERMINAL_SCROLLBACK: usize = 2000;

/// Escape sequences that do not touch the grid, only the window around it.
#[derive(Clone, Default)]
struct TerminalEvents {
    title: Arc<Mutex<Option<String>>>,
}

impl vt100::Callbacks for TerminalEvents {
    fn set_window_title(&mut self, _screen: &mut vt100::Screen, title: &[u8]) {
        if let Ok(mut current) = self.title.lock() {
            *current = Some(String::from_utf8_lossy(title).into_owned());
        }
    }
}

/// One interactive client attached through a pseudo-terminal.
pub struct TerminalSession {
    /// Kept for `TIOCSWINSZ`; input goes through `writer`.
    master: Option<File>,
    /// Feeds the writer thread. Writes can block when the client stops reading
    /// (a large paste), so they never run on the UI thread.
    writer: Option<mpsc::Sender<Vec<u8>>>,
    stop: Arc<AtomicBool>,
    child: Option<Child>,
    parser: vt100::Parser<TerminalEvents>,
    title: Arc<Mutex<Option<String>>>,
    rows: u16,
    cols: u16,
    receiver: Receiver<Vec<u8>>,
    /// Set when the exec session has ended (shell exit, error or engine loss).
    pub closed: bool,
    /// The client's exit code once it has been reaped.
    pub exit_code: Option<i32>,
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
        // `openpty` descriptors are inherited across exec. Mark both
        // close-on-exec right away so neither the client (which must hold only
        // the slave, through 0/1/2) nor any podman process spawned concurrently
        // by another thread keeps a copy. A leaked master copy would stop the
        // shell from ever seeing a hangup when the session is dropped.
        // `dup2` in `pre_exec` clears the flag on the new 0/1/2.
        // SAFETY: both descriptors were just returned by `openpty`.
        unsafe {
            libc::fcntl(master_fd, libc::F_SETFD, libc::FD_CLOEXEC);
            libc::fcntl(slave_fd, libc::F_SETFD, libc::FD_CLOEXEC);
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
            File::from_raw_fd(master_fd)
        };

        // The reader uses a duplicate so it can exit when asked without fighting
        // the writer. The descriptor stays blocking (`O_NONBLOCK` would be
        // shared with the writer and truncate large pastes); the reader waits
        // with `poll` and a timeout so it still notices the stop flag.
        // SAFETY: `master` is a valid descriptor; `F_DUPFD_CLOEXEC` returns a
        // new close-on-exec one.
        let reader_fd = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) };
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

        // SAFETY: `master` is valid; the duplicate is close-on-exec.
        let writer_fd = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) };
        if writer_fd < 0 {
            let mut child = child;
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "could not duplicate the pseudo-terminal: {}",
                std::io::Error::last_os_error()
            ));
        }
        // SAFETY: `writer_fd` is a freshly duplicated descriptor we own.
        let mut writer_file = unsafe { File::from_raw_fd(writer_fd) };
        let (writer, input) = mpsc::channel::<Vec<u8>>();
        let mut child = child;
        let spawned = std::thread::Builder::new()
            .name("terminal-input".into())
            .spawn(move || {
                // Ends when the session drops its sender, or on a write error
                // (the client is gone).
                while let Ok(bytes) = input.recv() {
                    if writer_file.write_all(&bytes).is_err() {
                        break;
                    }
                }
            });
        if let Err(error) = spawned {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("could not start the terminal writer: {error}"));
        }

        let stop = Arc::new(AtomicBool::new(false));
        let reader_stop = Arc::clone(&stop);
        let title = Arc::new(Mutex::new(None));
        let callbacks = TerminalEvents {
            title: Arc::clone(&title),
        };
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("terminal".into())
            .spawn(move || {
                let mut buffer = [0u8; 8192];
                loop {
                    if reader_stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let mut pollfd = libc::pollfd {
                        fd: reader.as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    // SAFETY: one valid pollfd; 100 ms timeout.
                    let ready = unsafe { libc::poll(&mut pollfd, 1, 100) };
                    if ready == 0 {
                        continue;
                    }
                    if ready < 0 {
                        if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
                        {
                            continue;
                        }
                        break;
                    }
                    // POLLHUP/POLLERR fall through to `read`, which then reports
                    // EOF or EIO and ends the loop after any remaining output.
                    match (&reader).read(&mut buffer) {
                        Ok(0) => break,
                        Ok(read) => {
                            if sender.send(buffer[..read].to_vec()).is_err() {
                                break;
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        // EIO is the normal outcome once the child exits.
                        Err(_) => break,
                    }
                }
            })
            .ok();

        Ok(Self {
            master: Some(master),
            writer: Some(writer),
            stop,
            child: Some(child),
            parser: vt100::Parser::new_with_callbacks(
                NonZeroU16::new(rows).unwrap_or(NonZeroU16::MIN),
                NonZeroU16::new(cols).unwrap_or(NonZeroU16::MIN),
                TERMINAL_SCROLLBACK,
                callbacks,
            ),
            title,
            rows,
            cols,
            receiver,
            closed: false,
            exit_code: None,
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

    /// Whether the shell asked for pastes to be bracketed (`DECSET 2004`).
    pub fn bracketed_paste(&self) -> bool {
        self.parser.screen().bracketed_paste()
    }

    /// Mouse reporting mode requested by the running program (`DECSET 9/1000/1002/1003`).
    pub fn mouse_mode(&self) -> vt100::MouseProtocolMode {
        self.parser.screen().mouse_protocol_mode()
    }

    /// Wire format for mouse reports (`SGR` or the legacy encodings).
    pub fn mouse_encoding(&self) -> vt100::MouseProtocolEncoding {
        self.parser.screen().mouse_protocol_encoding()
    }

    /// Cursor shape requested with `DECSCUSR`.
    pub fn cursor_style(&self) -> vt100::CursorStyle {
        self.parser.screen().cursor_style()
    }

    /// Whether the requested cursor shape blinks.
    pub fn cursor_blinks(&self) -> bool {
        matches!(
            self.cursor_style(),
            vt100::CursorStyle::BlinkBlock
                | vt100::CursorStyle::BlinkUnderline
                | vt100::CursorStyle::BlinkVerticalBar
        )
    }

    /// Window title set through `OSC 0`/`OSC 2`, if any.
    pub fn title(&self) -> Option<String> {
        self.title
            .lock()
            .ok()
            .and_then(|title| title.clone())
            .filter(|title| !title.trim().is_empty())
    }

    pub fn write(&self, bytes: &[u8]) {
        if let Some(writer) = &self.writer {
            let _ = writer.send(bytes.to_vec());
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
        // sees EOF, and a missed wait leaves a zombie behind. Exit alone does
        // not close the session: output the reader has not forwarded yet (a
        // quick `podman exec` error, say) would be lost. The session closes
        // when the reader hits EOF/EIO and its channel disconnects.
        if let Some(mut child) = self.child.take() {
            match child.try_wait() {
                Ok(None) => self.child = Some(child),
                Ok(Some(status)) => self.exit_code = status.code(),
                Err(_) => {}
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
        self.writer.take();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// Run `script` in a session and return the screen text once it closes.
    fn run_to_close(script: &str) -> TerminalSession {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        let mut session = TerminalSession::start(command, 24, 120).expect("session");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !session.closed {
            assert!(Instant::now() < deadline, "session did not close");
            session.drain();
            std::thread::sleep(Duration::from_millis(10));
        }
        // Let the reap happen if the reader finished first.
        for _ in 0..100 {
            if session.exit_code.is_some() {
                break;
            }
            session.drain();
            std::thread::sleep(Duration::from_millis(10));
        }
        session
    }

    #[test]
    fn client_does_not_inherit_the_master() {
        let session = run_to_close("ls -l /proc/$$/fd");
        let text = session.screen().contents();
        assert!(text.contains("/dev/pts/"), "listing missing: {text}");
        assert!(
            !text.contains("ptmx"),
            "master leaked into the client: {text}"
        );
    }

    #[test]
    fn keeps_final_output_and_exit_code() {
        let session = run_to_close("printf 'last words'; exit 127");
        assert!(session.screen().contents().contains("last words"));
        assert_eq!(session.exit_code, Some(127));
    }

    #[test]
    fn large_input_is_delivered_in_full() {
        let mut command = Command::new("sh");
        // Raw mode so the line discipline does not cap or echo the input.
        command.args(["-c", "stty raw -echo; head -c 200000 | wc -c; sleep 0.2"]);
        let mut session = TerminalSession::start(command, 24, 80).expect("session");
        std::thread::sleep(Duration::from_millis(300));
        session.write(&vec![b'a'; 200_000]);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !session.closed {
            assert!(Instant::now() < deadline, "session did not close");
            session.drain();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            session.screen().contents().contains("200000"),
            "{}",
            session.screen().contents()
        );
    }
}
