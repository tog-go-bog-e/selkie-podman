//! Color palettes and the current theme.

use std::cell::Cell;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeKind {
    #[default]
    Dark,
    Light,
    HighContrast,
}

impl ThemeKind {
    pub fn label(self) -> &'static str {
        match self {
            ThemeKind::Dark => "Dark",
            ThemeKind::Light => "Light",
            ThemeKind::HighContrast => "High contrast",
        }
    }

    pub fn next(self) -> Self {
        match self {
            ThemeKind::Dark => ThemeKind::Light,
            ThemeKind::Light => ThemeKind::HighContrast,
            ThemeKind::HighContrast => ThemeKind::Dark,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub bg: u32,
    pub sidebar_bg: u32,
    pub panel_bg: u32,
    pub hover_bg: u32,
    pub border: u32,
    pub text: u32,
    pub muted: u32,
    pub faint: u32,
    pub accent: u32,
    pub accent_soft: u32,
    pub green: u32,
    pub green_bg: u32,
    pub red: u32,
    pub red_bg: u32,
    pub yellow: u32,
    pub yellow_bg: u32,
    pub blue: u32,
    pub blue_bg: u32,
    pub on_accent: u32,
    pub scrim: u32,
}

impl Theme {
    const DARK: Theme = Theme {
        bg: 0x13151a,
        sidebar_bg: 0x171a20,
        panel_bg: 0x1c1f27,
        hover_bg: 0x232733,
        border: 0x2a2e3a,
        text: 0xe7e9ef,
        muted: 0x8d94a5,
        faint: 0x5f6675,
        accent: 0xa259f5,
        accent_soft: 0x2b1f42,
        green: 0x4ade80,
        green_bg: 0x17301f,
        red: 0xf87171,
        red_bg: 0x331a1e,
        yellow: 0xfbbf24,
        yellow_bg: 0x332a12,
        blue: 0x60a5fa,
        blue_bg: 0x1f2a3d,
        on_accent: 0xffffff,
        scrim: 0x000000b4,
    };

    const LIGHT: Theme = Theme {
        bg: 0xf5f6f8,
        sidebar_bg: 0xeceef2,
        panel_bg: 0xe4e7ed,
        hover_bg: 0xdde2ea,
        border: 0xd0d6df,
        text: 0x1b1f26,
        muted: 0x5a6372,
        faint: 0x8a93a1,
        accent: 0x7c3aed,
        accent_soft: 0xede9fe,
        green: 0x15803d,
        green_bg: 0xdcfce7,
        red: 0xb91c1c,
        red_bg: 0xfee2e2,
        yellow: 0xa16207,
        yellow_bg: 0xfef3c7,
        blue: 0x1d4ed8,
        blue_bg: 0xdbeafe,
        on_accent: 0xffffff,
        scrim: 0x00000066,
    };

    const HIGH_CONTRAST: Theme = Theme {
        bg: 0x000000,
        sidebar_bg: 0x0a0a0a,
        panel_bg: 0x141414,
        hover_bg: 0x262626,
        border: 0x7a7a7a,
        text: 0xffffff,
        muted: 0xd4d4d4,
        faint: 0xa3a3a3,
        accent: 0xd8b4fe,
        accent_soft: 0x2a1040,
        green: 0x4ade80,
        green_bg: 0x00331a,
        red: 0xff6b6b,
        red_bg: 0x3a0000,
        yellow: 0xffd400,
        yellow_bg: 0x332a00,
        blue: 0x7dd3fc,
        blue_bg: 0x002a3a,
        on_accent: 0x000000,
        scrim: 0x000000d9,
    };

    pub fn for_kind(kind: ThemeKind) -> Theme {
        match kind {
            ThemeKind::Dark => Theme::DARK,
            ThemeKind::Light => Theme::LIGHT,
            ThemeKind::HighContrast => Theme::HIGH_CONTRAST,
        }
    }
}

thread_local! {
    static CURRENT: Cell<ThemeKind> = const { Cell::new(ThemeKind::Dark) };
}

pub fn theme() -> Theme {
    Theme::for_kind(current_kind())
}

pub fn current_kind() -> ThemeKind {
    CURRENT.with(Cell::get)
}

pub fn set_kind(kind: ThemeKind) {
    CURRENT.with(|current| current.set(kind));
}
