//! Desktop integration: single-instance handling and a system tray icon.
//!
//! The single instance is enforced with an exclusive `flock` on a file in
//! `$XDG_RUNTIME_DIR` (falling back to the temp directory). The first instance
//! listens on a Unix socket next to that lock; further launches connect to it
//! and ask it to raise its window before exiting.
//!
//! The tray icon is a freedesktop StatusNotifierItem served over D-Bus, so it
//! needs a host such as KDE, XFCE or GNOME with the AppIndicator extension.
//! When no host is available the icon is simply not shown and the app keeps
//! working.

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use ksni::blocking::TrayMethods;

/// Messages sent to the running app from the tray or from a second launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Activate,
    Refresh,
    Quit,
}

/// Claim the single-instance lock.
///
/// Returns a receiver of signals when this process is the primary instance.
/// Returns `None` after asking an already running instance to come to the
/// front, in which case the caller should exit immediately.
pub fn claim_single_instance() -> Option<Receiver<Signal>> {
    let (lock_path, socket_path) = state_paths();
    let Ok(lock) = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
    else {
        // Without a writable runtime directory the app still works, it just
        // cannot coordinate with other instances.
        return Some(idle_signals());
    };

    // SAFETY: `lock` owns this descriptor, and it stays open for the life of
    // the process (the file is leaked below on success).
    let acquired = unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
    if !acquired {
        request_activation(&socket_path);
        return None;
    }
    // Hold the lock until the process exits. There is no destructor because
    // releasing it early would let a second instance take over while this one
    // is still running.
    std::mem::forget(lock);

    // The lock is ours, so a leftover socket belongs to a dead instance.
    let _ = std::fs::remove_file(&socket_path);
    let (sender, signals) = mpsc::channel();

    if let Ok(listener) = UnixListener::bind(&socket_path) {
        std::thread::Builder::new()
            .name("single-instance".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let mut stream = stream;
                    stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
                    let mut message = String::new();
                    if stream.read_to_string(&mut message).is_err() {
                        continue;
                    }
                    let signal = match message.trim() {
                        "activate" => Signal::Activate,
                        "refresh" => Signal::Refresh,
                        "quit" => Signal::Quit,
                        _ => continue,
                    };
                    if sender.send(signal).is_err() {
                        break;
                    }
                }
            })
            .ok();
    }

    Some(signals)
}

/// A receiver that never yields anything (used when coordination is unavailable).
fn idle_signals() -> Receiver<Signal> {
    let (_sender, receiver) = mpsc::channel();
    receiver
}

fn request_activation(socket_path: &PathBuf) -> bool {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Ok(mut stream) = std::os::unix::net::UnixStream::connect(socket_path) {
            return stream.write_all(b"activate\n").is_ok();
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn state_paths() -> (PathBuf, PathBuf) {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);

    let mut lock = base.join("podman-desktop-rs.lock");
    let mut socket = base.join("podman-desktop-rs.sock");
    // Unix socket paths are limited to roughly 108 bytes; fall back to a short
    // user-specific name if the runtime directory is too deep.
    if socket.as_os_str().len() > 100 || lock.as_os_str().len() > 100 {
        // SAFETY: `getuid` has no preconditions.
        let uid = unsafe { libc::getuid() };
        let fallback = std::env::temp_dir();
        lock = fallback.join(format!("podman-desktop-rs-{uid}.lock"));
        socket = fallback.join(format!("podman-desktop-rs-{uid}.sock"));
    }
    (lock, socket)
}

/// Start the tray icon in the background and return its signal receiver.
pub fn start_tray() -> Receiver<Signal> {
    let (sender, signals) = mpsc::channel();
    std::thread::Builder::new()
        .name("tray".into())
        .spawn(move || {
            let tray = AppTray { sender };
            match tray.assume_sni_available(true).spawn() {
                Ok(_handle) => loop {
                    std::thread::park();
                },
                // No StatusNotifierItem host: keep running without a tray icon.
                Err(error) => eprintln!("podman-desktop-rs: tray icon unavailable: {error}"),
            }
        })
        .ok();
    signals
}

struct AppTray {
    sender: Sender<Signal>,
}

impl AppTray {
    fn emit(&self, signal: Signal) {
        let _ = self.sender.send(signal);
    }
}

impl ksni::Tray for AppTray {
    fn id(&self) -> String {
        "podman-desktop-rs".into()
    }

    fn title(&self) -> String {
        "Podman Desktop".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::ApplicationStatus
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![icon()]
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "Podman Desktop".into(),
            description: "Containers, images, pods, volumes and networks".into(),
            icon_pixmap: vec![icon()],
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.emit(Signal::Activate);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{MenuItem, StandardItem};
        vec![
            StandardItem {
                label: "Show window".into(),
                activate: Box::new(|tray: &mut Self| tray.emit(Signal::Activate)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Refresh".into(),
                activate: Box::new(|tray: &mut Self| tray.emit(Signal::Refresh)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|tray: &mut Self| tray.emit(Signal::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// The app icon: a rounded Podman-purple square with a white "P".
///
/// Generated at runtime so the binary stays self-contained and no icon theme
/// installation is required.
fn icon() -> ksni::Icon {
    const SIZE: usize = 22;
    const BACKGROUND: [u8; 3] = [0x89, 0x2c, 0xa0];
    const FOREGROUND: [u8; 3] = [0xff, 0xff, 0xff];

    let mut data = vec![0u8; SIZE * SIZE * 4];
    for y in 0..SIZE {
        for x in 0..SIZE {
            if !inside_rounded_square(x, y) {
                continue;
            }
            let color = if is_glyph(x, y) {
                FOREGROUND
            } else {
                BACKGROUND
            };
            let offset = (y * SIZE + x) * 4;
            // ARGB32, network byte order.
            data[offset] = 0xff;
            data[offset + 1..offset + 4].copy_from_slice(&color);
        }
    }

    ksni::Icon {
        width: SIZE as i32,
        height: SIZE as i32,
        data,
    }
}

fn inside_rounded_square(x: usize, y: usize) -> bool {
    const SIZE: i32 = 22;
    const RADIUS: i32 = 5;
    let (x, y) = (x as i32, y as i32);
    let nearest_x = x.clamp(RADIUS, SIZE - RADIUS - 1);
    let nearest_y = y.clamp(RADIUS, SIZE - RADIUS - 1);
    let dx = x - nearest_x;
    let dy = y - nearest_y;
    dx * dx + dy * dy <= RADIUS * RADIUS
}

fn is_glyph(x: usize, y: usize) -> bool {
    // 5x7 bitmap of "P", scaled up to fit the 22x22 icon.
    const GLYPH: [&[u8]; 7] = [
        b"11110", b"10001", b"10001", b"11110", b"10000", b"10000", b"10000",
    ];
    const SCALE: usize = 2;
    const WIDTH: usize = 5 * SCALE;
    const HEIGHT: usize = 7 * SCALE;
    const OFFSET_X: usize = (22 - WIDTH) / 2;
    const OFFSET_Y: usize = (22 - HEIGHT) / 2;

    if x < OFFSET_X || y < OFFSET_Y || x >= OFFSET_X + WIDTH || y >= OFFSET_Y + HEIGHT {
        return false;
    }
    let column = (x - OFFSET_X) / SCALE;
    let row = (y - OFFSET_Y) / SCALE;
    GLYPH[row][column] == b'1'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_has_expected_layout() {
        let icon = icon();
        assert_eq!(icon.width, 22);
        assert_eq!(icon.height, 22);
        assert_eq!(icon.data.len(), 22 * 22 * 4);

        // Corners are transparent, the center of the glyph is opaque.
        let alpha = |x: usize, y: usize| icon.data[(y * 22 + x) * 4];
        assert_eq!(alpha(0, 0), 0);
        assert_eq!(alpha(21, 0), 0);
        assert_eq!(alpha(0, 21), 0);
        assert_eq!(alpha(11, 11), 0xff);
    }

    #[test]
    fn glyph_is_centered() {
        assert!(is_glyph(11, 11));
        assert!(!is_glyph(2, 2));
    }

    #[test]
    fn paths_are_short_enough_for_unix_sockets() {
        let (lock, socket) = state_paths();
        assert!(socket.as_os_str().len() < 108);
        assert!(lock.as_os_str().len() < 108);
    }
}
