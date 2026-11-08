use gpui::Stateful;

use super::helpers::*;
use super::settings::{
    default_details_width, default_sidebar_width, SettingsSection, LOG_WINDOWS, REFRESH_OPTIONS,
};
use super::theme::{current_kind, ThemeKind};
use super::*;

impl Selkie {
    // -----------------------------------------------------------------------
    // Settings page
    // -----------------------------------------------------------------------

    /// Settings is a sub-navigation plus the selected pane, mirroring the
    /// upstream layout.
    pub(super) fn settings_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut nav = div()
            .w(px(180.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_1()
            .px_3()
            .pt_4()
            .border_r_1()
            .border_color(rgb(theme().border));

        for section in SettingsSection::ALL {
            nav = nav.child(self.settings_nav_item(section, cx));
        }

        let pane = match self.settings.settings_section {
            SettingsSection::Registries => self.registries_view(cx),
            SettingsSection::Appearance => self.appearance_view(cx),
            SettingsSection::Engine => self.engine_view(cx),
            SettingsSection::About => self.about_view(),
        };

        div()
            .flex()
            .flex_1()
            .min_h(px(0.))
            .child(nav)
            .child(pane)
            .into_any_element()
    }

    fn settings_nav_item(
        &self,
        section: SettingsSection,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = self.settings.settings_section == section;

        div()
            .id(section.id())
            .flex()
            .items_center()
            .h(px(34.))
            .px_3()
            .rounded_md()
            .cursor_pointer()
            .when(active, |el| el.bg(rgb(theme().accent_soft)))
            .when(!active, |el| {
                el.hover(|style| style.bg(rgb(theme().hover_bg)))
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.settings.settings_section != section {
                    this.settings.settings_section = section;
                    this.settings.save();
                    if section == SettingsSection::Registries {
                        this.load_accounts();
                    }
                    cx.notify();
                }
            }))
            .child(
                div()
                    .text_sm()
                    .text_color(if active {
                        rgb(theme().accent)
                    } else {
                        rgb(theme().text)
                    })
                    .child(section.label()),
            )
    }

    fn appearance_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut content = self.section_container("settings-appearance");

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

        content = content.child(panel(vec![
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

        content.into_any_element()
    }

    fn engine_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut engine_options = Vec::new();
        for kind in EngineKind::ALL {
            engine_options.push(
                small_button_dyn(
                    format!("engine-{kind:?}"),
                    kind.label(),
                    self.settings.engine == kind,
                    cx.listener(move |this, _, _, cx| this.use_engine(kind, cx)),
                )
                .into_any_element(),
            );
        }

        let mut content = self
            .section_container("settings-engine")
            .child(panel(vec![option_row(
                "Engine",
                "Podman uses its CLI; Docker uses its HTTP API".to_string(),
                engine_options,
            )]));

        if self.settings.engine == EngineKind::Podman {
            let connection = match &self.settings.connection {
                Some(name) => name.clone(),
                None => "local default".to_string(),
            };
            let mut controls = Vec::new();
            if self.settings.connection.is_some() {
                controls.push(
                    small_button(
                        "settings-use-default-engine",
                        "Use Podman default",
                        false,
                        cx.listener(|this, _, _, cx| this.use_connection(None, cx)),
                    )
                    .into_any_element(),
                );
            }
            let running = self
                .local
                .machines
                .iter()
                .filter(|item| item.running)
                .count();

            content = content
                .child(panel(vec![option_row(
                    "Connection",
                    format!("{connection} · switch in the Connections tab"),
                    controls,
                )]))
                .child(section_title("RESOURCES"))
                .child(panel(vec![
                    about_row(
                        "Machines",
                        format!("{} · {running} running", self.local.machines.len()),
                    ),
                    about_row("Connections", self.local.connections.len().to_string()),
                ]));
        } else {
            content = content.child(panel(vec![about_row(
                "Podman-only features",
                "pods, secrets, machines and connections are hidden".to_string(),
            )]));
        }

        content.into_any_element()
    }

    fn about_view(&self) -> AnyElement {
        let settings_path = super::settings::path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "not writable".to_string());
        let engine_version = if self.snapshot.version.is_empty() {
            "not detected".to_string()
        } else {
            self.snapshot.version.clone()
        };

        self.section_container("settings-about")
            .child(panel(vec![
                about_row("Version", env!("CARGO_PKG_VERSION").to_string()),
                about_row(podman::engine().label(), engine_version),
                about_row("Settings file", settings_path),
                about_row("Renderer", "gpui · experimental".to_string()),
            ]))
            .into_any_element()
    }

    /// Scrollable pane with the section title on top.
    fn section_container(&self, id: &'static str) -> Stateful<Div> {
        div()
            .id(id)
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .px_6()
            .pb_6()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .mt_4()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.settings.settings_section.label()),
            )
    }

    /// Switch palette without persisting through a cycle helper.
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
