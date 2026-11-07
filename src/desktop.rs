//! Desktop integration: single-instance handling, the system tray icon and the
//! X11 window icon.
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
//!
//! Window managers that draw the app icon themselves (Cinnamon/Muffin, XFCE,
//! KWin) read it from the `_NET_WM_ICON` property, which GPUI never sets; see
//! [`install_window_icon`].

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
/// Drawn at runtime so the binary stays self-contained and no icon theme
/// installation is required. Every pixel is sampled 4×4 times so the rounded
/// corners and the glyph stay smooth at each requested size.
fn icon_rgba(size: usize) -> Vec<u8> {
    const BACKGROUND: [f32; 3] = [137.0, 44.0, 160.0];
    const FOREGROUND: [f32; 3] = [255.0, 255.0, 255.0];
    const SUB: usize = 4;

    let size_f = size as f32;
    let radius = size_f * 5.0 / 22.0;
    // Round to whole-pixel strokes so the glyph stays bold and crisp; ceilings
    // would leave it thin at small sizes, which supersampling turns to mush.
    let scale = ((size_f * 0.72 / 7.0).round()).max(2.0);
    let mut data = vec![0u8; size * size * 4];

    for y in 0..size {
        for x in 0..size {
            let mut background = 0u32;
            let mut foreground = 0u32;
            for sy in 0..SUB {
                for sx in 0..SUB {
                    let px = x as f32 + (sx as f32 + 0.5) / SUB as f32;
                    let py = y as f32 + (sy as f32 + 0.5) / SUB as f32;
                    if !inside_rounded_square(px, py, size_f, radius) {
                        continue;
                    }
                    if is_glyph(px, py, size_f, scale) {
                        foreground += 1;
                    } else {
                        background += 1;
                    }
                }
            }
            let covered = background + foreground;
            if covered == 0 {
                continue;
            }
            let offset = (y * size + x) * 4;
            for channel in 0..3 {
                let value = (BACKGROUND[channel] * background as f32
                    + FOREGROUND[channel] * foreground as f32)
                    / covered as f32;
                data[offset + channel] = value.round() as u8;
            }
            data[offset + 3] = (covered * 255 / (SUB * SUB) as u32) as u8;
        }
    }
    data
}

/// The 22×22 icon the tray protocol expects, as ARGB32 in network byte order.
fn icon() -> ksni::Icon {
    const SIZE: usize = 22;
    let rgba = icon_rgba(SIZE);
    let data = rgba
        .chunks(4)
        .flat_map(|pixel| [pixel[3], pixel[0], pixel[1], pixel[2]])
        .collect();

    ksni::Icon {
        width: SIZE as i32,
        height: SIZE as i32,
        data,
    }
}

fn inside_rounded_square(x: f32, y: f32, size: f32, radius: f32) -> bool {
    let dx = (radius - x).max(x - (size - radius)).max(0.0);
    let dy = (radius - y).max(y - (size - radius)).max(0.0);
    dx * dx + dy * dy <= radius * radius
}

fn is_glyph(x: f32, y: f32, size: f32, scale: f32) -> bool {
    // 5x7 bitmap of "P", scaled up to fit the icon.
    const GLYPH: [&[u8]; 7] = [
        b"11110", b"10001", b"10001", b"11110", b"10000", b"10000", b"10000",
    ];
    let width = 5.0 * scale;
    let height = 7.0 * scale;
    let offset_x = (size - width) / 2.0;
    let offset_y = (size - height) / 2.0;

    if x < offset_x || y < offset_y || x >= offset_x + width || y >= offset_y + height {
        return false;
    }
    let column = (((x - offset_x) / scale) as usize).min(4);
    let row = (((y - offset_y) / scale) as usize).min(6);
    GLYPH[row][column] == b'1'
}

/// Hide or show the main window without destroying it.
///
/// GPUI has no hide API, and letting the window close would remove it for good,
/// so the close request is vetoed and the X11 window is unmapped instead: the
/// process, the tray icon and the GTK-free UI state all stay alive and the
/// window can be mapped again from the tray. Returns whether a window was found.
pub fn set_window_visible(visible: bool) -> bool {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::ConnectionExt;

    if std::env::var_os("DISPLAY").is_none() {
        return false;
    }
    let Ok((connection, screen)) = x11rb::connect(None) else {
        return false;
    };
    let root = connection.setup().roots[screen].root;
    let Ok(Some(window)) = find_window(&connection, root, std::process::id()) else {
        return false;
    };

    let result = if visible {
        connection.map_window(window).map(|_| ())
    } else {
        connection.unmap_window(window).map(|_| ())
    };
    result.ok();
    let _ = connection.flush();
    true
}

/// Give the main window a real icon on X11.
///
/// The window belongs to GPUI's X connection, but X properties are global, so a
/// short-lived second connection can set `_NET_WM_ICON` once the window
/// appears. Without it Cinnamon/Muffin and other window managers that draw the
/// icon in the titlebar or window list fall back to a generic one. No-op when
/// there is no X display.
pub fn install_window_icon() {
    if std::env::var_os("DISPLAY").is_none() {
        return;
    }
    std::thread::Builder::new()
        .name("window-icon".into())
        .spawn(|| {
            if let Err(error) = set_window_icon() {
                eprintln!("podman-desktop-rs: window icon unavailable: {error}");
            }
        })
        .ok();
}

fn set_window_icon() -> Result<(), String> {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, PropMode};
    use x11rb::wrapper::ConnectionExt as _;

    let (connection, screen) = x11rb::connect(None).map_err(|error| error.to_string())?;
    let root = connection.setup().roots[screen].root;

    let icon_atom = connection
        .intern_atom(false, b"_NET_WM_ICON")
        .map_err(|error| error.to_string())?
        .reply()
        .map_err(|error| error.to_string())?
        .atom;
    let mut data = Vec::new();
    for size in [16usize, 22, 24, 32, 48, 64, 128] {
        let rgba = icon_rgba(size);
        data.push(size as u32);
        data.push(size as u32);
        data.extend(rgba.chunks(4).map(|pixel| {
            (u32::from(pixel[3]) << 24)
                | (u32::from(pixel[0]) << 16)
                | (u32::from(pixel[1]) << 8)
                | u32::from(pixel[2])
        }));
    }

    // The window appears on GPUI's connection right after this is called and the
    // window manager still has to reparent it, so keep looking and re-apply the
    // property until it sticks.
    let pid = std::process::id();
    for _ in 0..100 {
        let Some(window) = find_window(&connection, root, pid)? else {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        };
        connection
            .change_property32(
                PropMode::REPLACE,
                window,
                icon_atom,
                AtomEnum::CARDINAL,
                &data,
            )
            .map_err(|error| error.to_string())?;
        connection.flush().map_err(|error| error.to_string())?;

        let stuck = connection
            .get_property(false, window, icon_atom, AtomEnum::CARDINAL, 0, 1)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .is_some_and(|reply| !reply.value.is_empty());
        if stuck {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

/// The first window owned by `pid` anywhere in the tree.
///
/// Window managers reparent client windows under a frame, so the app's window is
/// usually not a direct child of the root; a bounded breadth-first walk finds it
/// without depending on the frame layout.
fn find_window(
    connection: &impl x11rb::connection::Connection,
    root: x11rb::protocol::xproto::Window,
    pid: u32,
) -> Result<Option<x11rb::protocol::xproto::Window>, String> {
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};

    let pid_atom = connection
        .intern_atom(false, b"_NET_WM_PID")
        .map_err(|error| error.to_string())?
        .reply()
        .map_err(|error| error.to_string())?
        .atom;

    let mut queue = std::collections::VecDeque::from([root]);
    let mut visited = 0;
    while let Some(parent) = queue.pop_front() {
        visited += 1;
        if visited > 1000 {
            break;
        }
        // A window can disappear mid-walk; that is not an error for us.
        let Ok(cookie) = connection.query_tree(parent) else {
            continue;
        };
        let Ok(tree) = cookie.reply() else {
            continue;
        };
        for &child in &tree.children {
            let Ok(cookie) =
                connection.get_property(false, child, pid_atom, AtomEnum::CARDINAL, 0, 1)
            else {
                continue;
            };
            let Ok(property) = cookie.reply() else {
                continue;
            };
            if property.value32().and_then(|mut values| values.next()) == Some(pid) {
                return Ok(Some(child));
            }
            queue.push_back(child);
        }
    }
    Ok(None)
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
    fn icon_scales_to_any_size_with_soft_edges() {
        for size in [16usize, 22, 48, 128] {
            let pixels = icon_rgba(size);
            assert_eq!(pixels.len(), size * size * 4, "size {size}");

            let alpha = |x: usize, y: usize| pixels[(y * size + x) * 4 + 3];
            assert_eq!(alpha(0, 0), 0, "corner of {size}");
            assert_eq!(alpha(size - 1, size - 1), 0, "corner of {size}");
            assert_eq!(alpha(size / 2, size / 2), 255, "center of {size}");

            let faded = (0..size * size).any(|index| {
                let value = pixels[index * 4 + 3];
                value != 0 && value != 255
            });
            assert!(faded, "no anti-aliased edge at {size}");
        }
    }

    #[test]
    fn glyph_is_centered() {
        assert!(is_glyph(11.0, 11.0, 22.0, 2.0));
        assert!(!is_glyph(2.0, 2.0, 22.0, 2.0));
    }

    #[test]
    fn paths_are_short_enough_for_unix_sockets() {
        let (lock, socket) = state_paths();
        assert!(socket.as_os_str().len() < 108);
        assert!(lock.as_os_str().len() < 108);
    }
}
