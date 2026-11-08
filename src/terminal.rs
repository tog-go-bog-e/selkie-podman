//! A pseudo-terminal running an interactive `podman` client.
//!
//! The PTY is allocated with `openpty`, the client (`podman exec` for a
//! container, `podman machine ssh` for a VM) is attached to the slave side, and
//! the master side is read on a thread and fed into a terminal emulator
//! ([`alacritty_terminal`]) that the UI renders. Using the same emulator core as
//! Alacritty buys true colour, OSC 4 palettes, OSC 52 clipboard access, wide and
//! combining characters, and the kitty keyboard protocol for free.

use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::Cell;
use alacritty_terminal::term::{Config, RenderableContent, Term, TermMode};
use alacritty_terminal::vte::ansi::{NamedColor, Processor, Rgb};

pub const TERMINAL_ROWS: u16 = 24;
pub const TERMINAL_COLS: u16 = 50;
/// Lines of history kept by the emulator for scrolling back.
pub const TERMINAL_SCROLLBACK: usize = 2000;

/// Mouse reporting requested by the running program (`DECSET 9/1000/1002/1003`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MouseMode {
    /// No mouse reporting.
    None,
    /// Report clicks only.
    Click,
    /// Report clicks and motion while a button is held.
    Drag,
    /// Report all motion.
    Motion,
}

/// Wire format for mouse reports.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MouseEncoding {
    /// The legacy `ESC [ M` encoding.
    Default,
    /// The UTF-8 flavour of the legacy encoding.
    Utf8,
    /// The extended SGR encoding.
    Sgr,
}

/// Grid dimensions handed to the emulator.
#[derive(Clone, Copy)]
struct GridSize {
    rows: usize,
    cols: usize,
}

impl GridSize {
    fn new(rows: u16, cols: u16) -> Self {
        Self {
            rows: rows.max(2) as usize,
            cols: cols.max(20) as usize,
        }
    }
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

/// The emulator's side channel: everything the escape parser reports that is
/// not a change to the grid.
#[derive(Clone, Default)]
struct TerminalEvents {
    title: Arc<Mutex<Option<String>>>,
    /// Text copied with OSC 52, waiting for the UI thread to put it on the
    /// system clipboard.
    clipboard: Arc<Mutex<Vec<String>>>,
    /// Answers the emulator sends back to the running program (device
    /// attributes, colour queries, …) go through the normal input writer.
    writer: Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>>,
}

impl TerminalEvents {
    fn write(&self, bytes: Vec<u8>) {
        if let Ok(writer) = self.writer.lock() {
            if let Some(writer) = writer.as_ref() {
                let _ = writer.send(bytes);
            }
        }
    }
}

impl EventListener for TerminalEvents {
    fn send_event(&self, event: Event) {
        match event {
            Event::Title(title) => {
                if let Ok(mut current) = self.title.lock() {
                    *current = Some(title);
                }
            }
            Event::ResetTitle => {
                if let Ok(mut current) = self.title.lock() {
                    *current = None;
                }
            }
            // Replies to queries such as device attributes or cursor position.
            Event::PtyWrite(text) => self.write(text.into_bytes()),
            Event::ClipboardStore(_, text) => {
                if let Ok(mut pending) = self.clipboard.lock() {
                    pending.push(text);
                }
            }
            // OSC 52 loads would need the system clipboard, which only the UI
            // thread can read. Reply with an empty selection so a program that
            // waits for an answer does not stall.
            Event::ClipboardLoad(_, format) => self.write(format("").into_bytes()),
            Event::ColorRequest(index, format) => {
                self.write(format(color_request(index)).into_bytes());
            }
            _ => {}
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
    term: Term<TerminalEvents>,
    parser: Processor,
    events: TerminalEvents,
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
        let events = TerminalEvents {
            title: Arc::new(Mutex::new(None)),
            clipboard: Arc::new(Mutex::new(Vec::new())),
            writer: Arc::new(Mutex::new(Some(writer.clone()))),
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

        let config = Config {
            scrolling_history: TERMINAL_SCROLLBACK,
            ..Config::default()
        };
        let term = Term::new(config, &GridSize::new(rows, cols), events.clone());

        Ok(Self {
            master: Some(master),
            writer: Some(writer),
            stop,
            child: Some(child),
            term,
            parser: Processor::new(),
            events,
            rows,
            cols,
            receiver,
            closed: false,
            exit_code: None,
        })
    }

    /// Emulator content for the current viewport.
    pub fn renderable(&self) -> RenderableContent<'_> {
        self.term.renderable_content()
    }

    /// The cell at a viewport coordinate, regardless of scrollback.
    pub fn cell(&self, row: u16, col: u16) -> Option<&Cell> {
        let offset = self.term.grid().display_offset() as i32;
        // Scrolling back moves the viewport into the history, whose lines are
        // negative. `row` is always relative to the top of the viewport.
        let top = -offset;
        let bottom = top + self.term.screen_lines() as i32 - 1;
        let line = top + row as i32;
        if line > bottom || col as usize >= self.term.columns() {
            return None;
        }
        Some(&self.term.grid()[Line(line)][Column(col as usize)])
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
        self.term.resize(GridSize::new(rows, cols));

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
        self.term.grid().display_offset()
    }

    /// Scroll by `delta` lines (positive scrolls back); clamped to the buffer.
    pub fn scroll_by(&mut self, delta: isize) -> bool {
        let current = self.scrollback() as isize;
        let max = self.term.history_size() as isize;
        let target = (current + delta).clamp(0, max);
        if target == current {
            return false;
        }
        self.term
            .scroll_display(Scroll::Delta((target - current) as i32));
        true
    }

    /// Return to the live viewport.
    pub fn scroll_to_bottom(&mut self) {
        if self.scrollback() != 0 {
            self.term.scroll_display(Scroll::Bottom);
        }
    }

    /// Whether the shell requested application cursor keys (`DECCKM`).
    pub fn application_cursor(&self) -> bool {
        self.term.mode().contains(TermMode::APP_CURSOR)
    }

    /// Whether the shell asked for pastes to be bracketed (`DECSET 2004`).
    pub fn bracketed_paste(&self) -> bool {
        self.term.mode().contains(TermMode::BRACKETED_PASTE)
    }

    /// Mouse reporting mode requested by the running program.
    pub fn mouse_mode(&self) -> MouseMode {
        let mode = self.term.mode();
        if mode.contains(TermMode::MOUSE_MOTION) {
            MouseMode::Motion
        } else if mode.contains(TermMode::MOUSE_DRAG) {
            MouseMode::Drag
        } else if mode.contains(TermMode::MOUSE_REPORT_CLICK) {
            MouseMode::Click
        } else {
            MouseMode::None
        }
    }

    /// Wire format for mouse reports (`SGR` or the legacy encodings).
    pub fn mouse_encoding(&self) -> MouseEncoding {
        let mode = self.term.mode();
        if mode.contains(TermMode::SGR_MOUSE) {
            MouseEncoding::Sgr
        } else if mode.contains(TermMode::UTF8_MOUSE) {
            MouseEncoding::Utf8
        } else {
            MouseEncoding::Default
        }
    }

    /// Whether the requested cursor shape blinks (`DECSCUSR`/`DECSET 12`).
    pub fn cursor_blinks(&self) -> bool {
        self.term.cursor_style().blinking
    }

    /// Window title set through `OSC 0`/`OSC 2`, if any.
    pub fn title(&self) -> Option<String> {
        self.events
            .title
            .lock()
            .ok()
            .and_then(|title| title.clone())
            .filter(|title| !title.trim().is_empty())
    }

    /// Text copied by the running program with OSC 52, if any. The clipboard
    /// belongs to the window, so the UI thread applies this.
    pub fn take_clipboard(&self) -> Option<String> {
        let mut pending = self.events.clipboard.lock().ok()?;
        let text = pending.pop()?;
        pending.clear();
        Some(text)
    }

    /// The whole viewport as text, one trimmed line per row. Used by tests.
    #[cfg(test)]
    pub fn contents(&self) -> String {
        let start = Point::new(Line(0), Column(0));
        let end = Point::new(
            Line(self.term.screen_lines() as i32 - 1),
            self.term.last_column(),
        );
        self.term.bounds_to_string(start, end)
    }

    /// Text in an inclusive viewport cell range, with trailing blanks removed.
    pub fn text_between(&self, start: (u16, u16), end: (u16, u16)) -> String {
        text_between(&self.term, start, end)
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
                    self.parser.advance(&mut self.term, &bytes);
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

/// Text in an inclusive cell range, with trailing blanks removed per line.
fn text_between<T: EventListener>(term: &Term<T>, start: (u16, u16), end: (u16, u16)) -> String {
    let offset = term.grid().display_offset() as i32;
    let point =
        |(row, col): (u16, u16)| Point::new(Line(row as i32 - offset), Column(col as usize));
    term.bounds_to_string(point(start), point(end))
}

/// The colour the emulator reports for an OSC 4/10/11 query.
fn color_request(index: usize) -> Rgb {
    let (r, g, b) = match index {
        0..=255 => {
            let rgb = ansi_color(index as u8);
            (
                ((rgb >> 16) & 0xff) as u8,
                ((rgb >> 8) & 0xff) as u8,
                (rgb & 0xff) as u8,
            )
        }
        index if index == NamedColor::Background as usize => (0x1e, 0x1e, 0x1e),
        _ => (0xe5, 0xe5, 0xe5),
    };
    Rgb { r, g, b }
}

/// One entry of the xterm-256 palette as `0xRRGGBB`.
pub fn ansi_color(index: u8) -> u32 {
    const BASIC: [u32; 16] = [
        0x1e1e1e, 0xcd3131, 0x0dbc79, 0xe5e510, 0x2472c8, 0xbc3fbc, 0x11a8cd, 0xe5e5e5, 0x666666,
        0xf14c4c, 0x23d18b, 0xf5f543, 0x3b8eea, 0xd670d6, 0x29b8db, 0xffffff,
    ];
    match index {
        0..=15 => BASIC[index as usize],
        16..=231 => {
            const STEPS: [u32; 6] = [0, 95, 135, 175, 215, 255];
            let index = index - 16;
            let red = STEPS[(index / 36) as usize];
            let green = STEPS[((index % 36) / 6) as usize];
            let blue = STEPS[(index % 6) as usize];
            (red << 16) | (green << 8) | blue
        }
        _ => {
            let level = 8 + 10 * (index - 232) as u32;
            (level << 16) | (level << 8) | level
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
        let text = session.contents();
        assert!(text.contains("/dev/pts/"), "listing missing: {text}");
        assert!(
            !text.contains("ptmx"),
            "master leaked into the client: {text}"
        );
    }

    #[test]
    fn keeps_final_output_and_exit_code() {
        let session = run_to_close("printf 'last words'; exit 127");
        assert!(session.contents().contains("last words"));
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
            session.contents().contains("200000"),
            "{}",
            session.contents()
        );
    }

    /// Build an emulator without a process, for parser-level tests.
    fn mock_term(content: &str) -> Term<TerminalEvents> {
        let size = GridSize::new(4, 20);
        let mut term = Term::new(Config::default(), &size, TerminalEvents::default());
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, content.as_bytes());
        term
    }

    #[test]
    fn selection_text_trims_trailing_blanks() {
        let term = mock_term("hello world\r\nsecond line\r\n");
        assert_eq!(text_between(&term, (0, 0), (0, 4)), "hello");
        assert_eq!(text_between(&term, (0, 6), (1, 5)), "world\nsecond");
    }

    #[test]
    fn records_the_window_title() {
        let session = run_to_close("printf '\\033]2;selkie\\007'");
        assert_eq!(session.title().as_deref(), Some("selkie"));
    }

    #[test]
    fn maps_ansi_colors() {
        assert_eq!(ansi_color(0), 0x1e1e1e);
        assert_eq!(ansi_color(7), 0xe5e5e5);
        assert_eq!(ansi_color(16), 0x000000);
        assert_eq!(ansi_color(21), 0x0000ff);
        assert_eq!(ansi_color(255), 0xeeeeee);
    }
}
