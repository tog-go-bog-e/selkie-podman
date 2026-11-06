//! Persisted user settings.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::theme::ThemeKind;
use super::Tab;

pub const REFRESH_OPTIONS: [u64; 4] = [3, 5, 10, 30];
pub const LOG_WINDOWS: [&str; 4] = ["5m", "10m", "30m", "1h"];

pub const SIDEBAR_MIN_WIDTH: f32 = 160.0;
pub const SIDEBAR_MAX_WIDTH: f32 = 400.0;
pub const DETAILS_MIN_WIDTH: f32 = 320.0;
pub const DETAILS_MAX_WIDTH: f32 = 900.0;
/// Minimum width kept for the list when both side panels are shown.
pub const PANEL_CONTENT_MIN_WIDTH: f32 = 320.0;

pub fn default_sidebar_width() -> f32 {
    224.0
}

pub fn default_details_width() -> f32 {
    400.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub theme: ThemeKind,
    #[serde(default)]
    pub tab: Tab,
    #[serde(default = "default_refresh")]
    pub refresh_secs: u64,
    #[serde(default = "default_log_window")]
    pub log_window: String,
    /// Width of the navigation sidebar, in logical pixels.
    #[serde(default = "default_sidebar_width")]
    pub sidebar_width: f32,
    /// Width of the details pane, in logical pixels.
    #[serde(default = "default_details_width")]
    pub details_width: f32,
    #[serde(default)]
    pub window: Option<WindowState>,
    /// Named Podman connection used for every command; `None` means Podman's
    /// default (usually the local engine).
    #[serde(default)]
    pub connection: Option<String>,
}

/// Last known window size, in logical pixels.
///
/// The position is deliberately not stored: GPUI's X11 backend does not expose
/// a reliable window position and window managers disagree on how to apply a
/// requested one, so placement is left to the window manager.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    pub width: f32,
    pub height: f32,
    #[serde(default)]
    pub maximized: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeKind::Dark,
            tab: Tab::Containers,
            refresh_secs: default_refresh(),
            log_window: default_log_window(),
            sidebar_width: default_sidebar_width(),
            details_width: default_details_width(),
            window: None,
            connection: None,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let mut settings = load();
        if !LOG_WINDOWS.contains(&settings.log_window.as_str()) {
            settings.log_window = default_log_window();
        }
        settings.refresh_secs = settings.refresh_secs.max(1);
        settings.sidebar_width = clamp_panel_width(
            settings.sidebar_width,
            SIDEBAR_MIN_WIDTH,
            SIDEBAR_MAX_WIDTH,
            default_sidebar_width(),
        );
        settings.details_width = clamp_panel_width(
            settings.details_width,
            DETAILS_MIN_WIDTH,
            DETAILS_MAX_WIDTH,
            default_details_width(),
        );
        if settings.window.map(WindowState::is_valid) == Some(false) {
            settings.window = None;
        }
        settings
    }

    pub fn save(&self) {
        save(self);
    }

    pub fn next_refresh(&mut self) {
        self.refresh_secs = next_option(&REFRESH_OPTIONS, self.refresh_secs);
    }

    pub fn next_log_window(&mut self) {
        let current = LOG_WINDOWS
            .iter()
            .position(|window| *window == self.log_window)
            .unwrap_or(1);
        self.log_window = LOG_WINDOWS[(current + 1) % LOG_WINDOWS.len()].to_string();
    }

    /// Store the current window size. Returns whether anything changed.
    pub fn remember_window(&mut self, width: f32, height: f32, maximized: bool) -> bool {
        let Some(next) = self.merged_window(width, height, maximized) else {
            return false;
        };
        if self.window == Some(next) {
            return false;
        }
        self.window = Some(next);
        self.save();
        true
    }

    fn merged_window(&self, width: f32, height: f32, maximized: bool) -> Option<WindowState> {
        // While maximized the platform reports the full-screen size; keep the
        // last windowed size so unmaximizing restores it.
        let next = match self.window {
            Some(current) if maximized => WindowState {
                maximized: true,
                ..current
            },
            _ => WindowState {
                width,
                height,
                maximized,
            },
        };
        next.is_valid().then_some(next)
    }
}

impl WindowState {
    fn is_valid(self) -> bool {
        self.width.is_finite() && self.height.is_finite() && self.width >= 1.0 && self.height >= 1.0
    }
}

/// Repair a stored panel width: non-finite values fall back to the default and
/// out-of-range values are clamped.
fn clamp_panel_width(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if !value.is_finite() {
        return fallback;
    }
    value.clamp(min, max)
}

fn default_refresh() -> u64 {
    3
}

fn default_log_window() -> String {
    "10m".to_string()
}

fn next_option<T: Copy + PartialEq>(options: &[T], current: T) -> T {
    let index = options
        .iter()
        .position(|option| *option == current)
        .unwrap_or(0);
    options[(index + 1) % options.len()]
}

pub fn load() -> Settings {
    let Some(path) = settings_path() else {
        return Settings::default();
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|contents| serde_json::from_str(&contents).ok())
        .unwrap_or_default()
}

pub fn save(settings: &Settings) {
    let Some(path) = settings_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(contents) = serde_json::to_string_pretty(settings) {
        let _ = std::fs::write(path, contents);
    }
}

fn settings_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("podman-desktop-rs").join("settings.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_refresh_options() {
        let mut settings = Settings::default();
        assert_eq!(settings.refresh_secs, 3);
        settings.next_refresh();
        assert_eq!(settings.refresh_secs, 5);
        for _ in 0..3 {
            settings.next_refresh();
        }
        assert_eq!(settings.refresh_secs, 3);
    }

    #[test]
    fn cycles_log_windows() {
        let mut settings = Settings::default();
        assert_eq!(settings.log_window, "10m");
        settings.next_log_window();
        assert_eq!(settings.log_window, "30m");
        for _ in 0..3 {
            settings.next_log_window();
        }
        assert_eq!(settings.log_window, "10m");
    }

    #[test]
    fn keeps_windowed_size_while_maximized() {
        let mut settings = Settings::default();
        assert!(settings.window.is_none());

        let windowed = settings
            .merged_window(1200.0, 700.0, false)
            .expect("valid size");
        assert_eq!(
            windowed,
            WindowState {
                width: 1200.0,
                height: 700.0,
                maximized: false,
            }
        );
        settings.window = Some(windowed);

        // Maximizing keeps the windowed restore size.
        let maximized = settings
            .merged_window(1920.0, 1080.0, true)
            .expect("valid size");
        assert!(maximized.maximized);
        assert_eq!(maximized.width, 1200.0);
        assert_eq!(maximized.height, 700.0);
    }

    #[test]
    fn rejects_invalid_window_geometry() {
        let settings = Settings::default();
        assert!(settings.merged_window(0.0, 0.0, false).is_none());
        assert!(settings.merged_window(f32::NAN, 600.0, false).is_none());
    }

    #[test]
    fn loads_legacy_settings_with_default_widths() {
        // Settings files written before the panels were resizable must still load.
        let settings: Settings = serde_json::from_str(
            r#"{"theme":"dark","tab":"Containers","refresh_secs":5,"log_window":"10m"}"#,
        )
        .expect("legacy settings should deserialize");
        assert_eq!(settings.sidebar_width, default_sidebar_width());
        assert_eq!(settings.details_width, default_details_width());
    }

    #[test]
    fn clamps_panel_widths() {
        assert_eq!(Settings::default().sidebar_width, default_sidebar_width());
        assert_eq!(Settings::default().details_width, default_details_width());

        assert_eq!(
            clamp_panel_width(10.0, SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH, 224.0),
            SIDEBAR_MIN_WIDTH
        );
        assert_eq!(
            clamp_panel_width(10_000.0, DETAILS_MIN_WIDTH, DETAILS_MAX_WIDTH, 400.0),
            DETAILS_MAX_WIDTH
        );
        assert_eq!(
            clamp_panel_width(f32::NAN, DETAILS_MIN_WIDTH, DETAILS_MAX_WIDTH, 400.0),
            400.0
        );
        assert_eq!(
            clamp_panel_width(512.0, DETAILS_MIN_WIDTH, DETAILS_MAX_WIDTH, 400.0),
            512.0
        );
    }
}
