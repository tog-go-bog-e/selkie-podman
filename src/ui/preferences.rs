use super::helpers::*;
use super::settings::{default_details_width, default_sidebar_width, LOG_WINDOWS, REFRESH_OPTIONS};
use super::theme::{current_kind, ThemeKind};
use super::*;

impl PodmanDesktop {
    // -----------------------------------------------------------------------
    // Settings page
    // -----------------------------------------------------------------------

    pub(super) fn settings_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut content = div()
            .id("settings")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .px_6()
            .pb_6()
            .flex()
            .flex_col()
            .gap_3();

        // Appearance
        let mut theme_options = Vec::new();
        for kind in [ThemeKind::Dark, ThemeKind::Light, ThemeKind::HighContrast] {
            theme_options.push(
                small_button_dyn(
                    format!("theme-{kind:?}"),
                    kind.label(),
                    current_kind() == kind,
                    cx.listener(move |this, _, _, cx| this.set_theme(kind, cx)),
                )
                .into_any_element(),
            );
        }
        let mut log_options = Vec::new();
        for window in LOG_WINDOWS {
            log_options.push(
                small_button_dyn(
                    window,
                    window,
                    self.settings.log_window == window,
                    cx.listener(move |this, _, _, cx| this.set_log_window(window, cx)),
                )
                .into_any_element(),
            );
        }
        let mut refresh_options = Vec::new();
        for secs in REFRESH_OPTIONS {
            refresh_options.push(
                small_button_dyn(
                    format!("refresh-{secs}"),
                    format!("{secs}s"),
                    self.settings.refresh_secs == secs,
                    cx.listener(move |this, _, _, cx| this.set_refresh(secs, cx)),
                )
                .into_any_element(),
            );
        }

        content = content.child(section_title("APPEARANCE")).child(panel(vec![
            option_row(
                "Theme",
                "Colour palette for every view".to_string(),
                theme_options,
            ),
            option_row(
                "Log window",
                "How much history the Logs tab streams".to_string(),
                log_options,
            ),
            option_row(
                "Auto refresh",
                "Polling only runs when no engine events arrive".to_string(),
                refresh_options,
            ),
        ]));

        // Panels
        content = content.child(section_title("PANELS")).child(panel(vec![
            option_row(
                "Sidebar width",
                format!("{} px", self.settings.sidebar_width.round()),
                vec![small_button(
                    "reset-sidebar-width",
                    "Reset",
                    false,
                    cx.listener(|this, _, _, cx| {
                        this.settings.sidebar_width = default_sidebar_width();
                        this.settings.save();
                        cx.notify();
                    }),
                )
                .into_any_element()],
            ),
            option_row(
                "Details width",
                format!("{} px", self.settings.details_width.round()),
                vec![small_button(
                    "reset-details-width",
                    "Reset",
                    false,
                    cx.listener(|this, _, _, cx| {
                        this.settings.details_width = default_details_width();
                        this.settings.save();
                        cx.notify();
                    }),
                )
                .into_any_element()],
            ),
        ]));

        // Engine
        let connection = match &self.settings.connection {
            Some(name) => name.clone(),
            None => "local default".to_string(),
        };
        let mut engine_controls = Vec::new();
        if self.settings.connection.is_some() {
            engine_controls.push(
                small_button(
                    "settings-use-default-engine",
                    "Use Podman default",
                    false,
                    cx.listener(|this, _, _, cx| this.use_connection(None, cx)),
                )
                .into_any_element(),
            );
        }
        content = content
            .child(section_title("ENGINE"))
            .child(panel(vec![option_row(
                "Connection",
                format!("{connection} · manage them in Connections"),
                engine_controls,
            )]));

        // About
        let settings_path = super::settings::path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "not writable".to_string());
        let podman = if self.snapshot.version.is_empty() {
            "not detected".to_string()
        } else {
            self.snapshot.version.clone()
        };
        content = content.child(section_title("ABOUT")).child(panel(vec![
            about_row("Version", env!("CARGO_PKG_VERSION").to_string()),
            about_row("Podman", podman),
            about_row("Settings file", settings_path),
            about_row("Renderer", "gpui · experimental".to_string()),
        ]));

        content.into_any_element()
    }

    /// Switch palette without persisting through the cycle helper.
    fn set_theme(&mut self, kind: ThemeKind, cx: &mut Context<Self>) {
        if current_kind() == kind {
            return;
        }
        self.settings.theme = kind;
        set_kind(kind);
        self.settings.save();
        cx.notify();
    }

    fn set_refresh(&mut self, secs: u64, cx: &mut Context<Self>) {
        if self.settings.refresh_secs == secs {
            return;
        }
        self.settings.refresh_secs = secs;
        self.settings.save();
        cx.notify();
    }
}

/// A bordered group of setting rows.
fn panel(rows: Vec<Div>) -> Div {
    let mut panel = div()
        .rounded_lg()
        .border_1()
        .border_color(rgb(theme().border))
        .bg(rgb(theme().panel_bg))
        .flex()
        .flex_col();
    for (index, row) in rows.into_iter().enumerate() {
        panel = panel.child(if index == 0 {
            row
        } else {
            row.border_t_1().border_color(rgb(theme().border))
        });
    }
    panel
}

/// A row with a label, a muted explanation and its controls on the right.
fn option_row(label: &str, detail: String, controls: Vec<AnyElement>) -> Div {
    let mut buttons = div().flex().items_center().gap_1().flex_shrink_0();
    for control in controls {
        buttons = buttons.child(control);
    }

    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .px_4()
        .py_3()
        .child(
            div()
                .flex()
                .flex_col()
                .min_w(px(0.))
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().text))
                        .child(label.to_string()),
                )
                .child(div().text_xs().text_color(rgb(theme().muted)).child(detail)),
        )
        .child(buttons)
}

/// A read-only key/value row for the About panel.
fn about_row(label: &str, value: String) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .px_4()
        .py_3()
        .child(
            div()
                .text_sm()
                .text_color(rgb(theme().text))
                .child(label.to_string()),
        )
        .child(
            div()
                .min_w(px(0.))
                .text_xs()
                .text_color(rgb(theme().muted))
                .truncate()
                .child(value),
        )
}
