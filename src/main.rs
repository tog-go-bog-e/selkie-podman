mod desktop;
mod podman;
mod terminal;
mod ui;

use std::time::Duration;

use gpui::{px, size, App, AppContext, Application, TitlebarOptions, WindowBounds, WindowOptions};

use ui::settings::Settings;

const DEFAULT_WIDTH: f32 = 1280.0;
const DEFAULT_HEIGHT: f32 = 800.0;
const MIN_WIDTH: f32 = 1040.0;
const MIN_HEIGHT: f32 = 600.0;

fn main() {
    // A second launch asks the running instance to come to the front and exits.
    let Some(signals) = desktop::claim_single_instance() else {
        return;
    };
    let tray_signals = desktop::start_tray();

    Application::new().run(move |cx: &mut App| {
        let mut settings = Settings::load();
        // A connection remembered from a previous run may have been removed from
        // the Podman configuration; fall back to the default engine if so.
        if let Some(name) = settings.connection.clone() {
            let known = podman::connection_list()
                .map(|list| list.iter().any(|connection| connection.name == name))
                .unwrap_or(true);
            if !known {
                settings.connection = None;
                settings.save();
            }
        }
        podman::set_connection(settings.connection.clone());

        let window_bounds = restored_bounds(cx, &settings);

        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(window_bounds),
                    // Sets WM_CLASS on X11 so desktop environments group the
                    // window with podman-desktop-rs.desktop (StartupWMClass).
                    app_id: Some("podman-desktop-rs".into()),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Podman Desktop - Rust".into()),
                        ..Default::default()
                    }),
                    window_min_size: Some(size(px(MIN_WIDTH), px(MIN_HEIGHT))),
                    ..Default::default()
                },
                |_window, cx| cx.new(ui::PodmanDesktop::new),
            )
            .expect("failed to open the main window");

        // GPUI does not set `_NET_WM_ICON`; give window managers that draw the
        // app icon themselves (Cinnamon, XFCE, KWin) something to show.
        desktop::install_window_icon();

        // Save the size immediately when the user closes the window, so a
        // resize just before closing is not lost to the polling interval.
        window
            .update(cx, |_this, window, cx| {
                let entity = cx.entity();
                window.on_window_should_close(cx, move |window, cx| {
                    entity.update(cx, |view, _cx| {
                        view.remember_window(window.bounds(), window.is_maximized());
                    });
                    // Closing docks the app in the tray instead of quitting; the
                    // tray's Show window (or a second launch) brings it back, and
                    // Quit there or Ctrl/Cmd+Q ends the process. Without an X
                    // display to unmap there is nothing to dock, so close.
                    if desktop::set_window_visible(false) {
                        false
                    } else {
                        cx.quit();
                        true
                    }
                });
            })
            .ok();

        // Single task for desktop signals and window-state polling: both need
        // the same window handle, which is not clonable.
        cx.spawn(async move |cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(500))
                .await;

            let mut activate = false;
            let mut refresh = false;
            let mut quit = false;
            while signals.try_recv().is_ok() {
                activate = true;
            }
            while let Ok(signal) = tray_signals.try_recv() {
                match signal {
                    desktop::Signal::Activate => activate = true,
                    desktop::Signal::Refresh => refresh = true,
                    desktop::Signal::Quit => quit = true,
                }
            }

            let Ok(keep_running) = window.update(cx, |view, window, cx| {
                // Only writes to disk when the size actually changed.
                view.remember_window(window.bounds(), window.is_maximized());
                if activate {
                    // The window may be unmapped while the app sits in the tray.
                    desktop::set_window_visible(true);
                    window.activate_window();
                }
                if refresh {
                    // An explicit refresh re-reads everything, cached data included.
                    view.refresh_dashboard(cx);
                }
                // Blink the text caret at 2 Hz while an input is focused.
                view.blink_caret(window, cx);
                if quit {
                    cx.quit();
                }
                !quit
            }) else {
                break;
            };
            if !keep_running {
                break;
            }
        })
        .detach();

        cx.activate(true);
    });
}

/// Restore the last window size and maximized state.
///
/// The position is left to the window manager: GPUI exposes no reliable window
/// origin on X11, and window managers apply requested positions inconsistently.
fn restored_bounds(cx: &App, settings: &Settings) -> WindowBounds {
    let (width, height) = settings
        .window
        .map(|saved| {
            (
                saved.width.clamp(MIN_WIDTH, 100_000.0),
                saved.height.clamp(MIN_HEIGHT, 100_000.0),
            )
        })
        .unwrap_or((DEFAULT_WIDTH, DEFAULT_HEIGHT));

    let centered = WindowBounds::centered(size(px(width), px(height)), cx);
    let maximized = settings.window.is_some_and(|saved| saved.maximized);
    match (maximized, centered) {
        (true, WindowBounds::Windowed(bounds)) => WindowBounds::Maximized(bounds),
        (_, bounds) => bounds,
    }
}
