use super::helpers::*;
use super::*;

impl Selkie {
    // Sidebar
    // -----------------------------------------------------------------------

    fn brand(&self) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(30.))
                    .h(px(30.))
                    .rounded_lg()
                    .bg(rgb(theme().accent))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(theme().on_accent))
                            .child("P"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Selkie"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(theme().muted))
                            .child(format!("for {}", podman::engine().label())),
                    ),
            )
    }

    fn nav_item(
        &self,
        tab: Tab,
        id: &'static str,
        label: &'static str,
        count: Option<usize>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = self.tab == tab;

        let mut item = div()
            .id(id)
            .flex()
            .items_center()
            .justify_between()
            .h(px(38.))
            .px_3()
            .rounded_md()
            .cursor_pointer()
            .when(active, |el| el.bg(rgb(theme().accent_soft)))
            .when(!active, |el| {
                el.hover(|style| style.bg(rgb(theme().hover_bg)))
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                this.root_focus.focus(window);
                if this.tab != tab {
                    this.tab = tab;
                    this.sort = None;
                    this.settings.tab = tab;
                    this.settings.save();
                    this.search.clear();
                    this.account_menu = None;
                    this.clear_selection();
                    // The Dashboard data is only fetched while it is visible.
                    if tab == Tab::Dashboard {
                        this.refresh_dashboard(cx);
                    }
                    if tab == Tab::Accounts {
                        this.load_accounts();
                    }
                    if tab == Tab::Connections {
                        this.check_connections(false, cx);
                    }
                }
                cx.notify();
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(dot(if active {
                        theme().accent
                    } else {
                        theme().faint
                    }))
                    .child(
                        div()
                            .text_sm()
                            .text_color(if active {
                                rgb(theme().text)
                            } else {
                                rgb(theme().muted)
                            })
                            .child(label),
                    ),
            );

        if let Some(count) = count {
            item = item.child(
                div()
                    .text_xs()
                    .text_color(if active {
                        rgb(theme().accent)
                    } else {
                        rgb(theme().faint)
                    })
                    .child(count.to_string()),
            );
        }

        item
    }

    pub(super) fn sidebar(&self, cx: &mut Context<Self>) -> Div {
        let engine = podman::engine();
        let connected = self.error.is_none() && !self.snapshot.version.is_empty();
        let version = if self.snapshot.version.is_empty() {
            "not detected".to_string()
        } else {
            self.snapshot.version.clone()
        };

        div()
            .flex()
            .flex_col()
            .w(px(self.effective_sidebar_width()))
            .h_full()
            .flex_shrink_0()
            .bg(rgb(theme().sidebar_bg))
            .p_3()
            .child(self.brand())
            .child(div().mt_4().flex().flex_col().gap_1().child(self.nav_item(
                Tab::Dashboard,
                "nav-dashboard",
                "Dashboard",
                None,
                cx,
            )))
            .child(
                div()
                    .mt_6()
                    .mb_2()
                    .px_2()
                    .text_xs()
                    .text_color(rgb(theme().faint))
                    .child("WORKSPACE"),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(self.nav_item(
                        Tab::Containers,
                        "nav-containers",
                        "Containers",
                        Some(self.snapshot.containers.len()),
                        cx,
                    ))
                    .child(self.nav_item(
                        Tab::Compose,
                        "nav-compose",
                        "Compose",
                        Some(self.snapshot.compose.len()),
                        cx,
                    ))
                    .child(self.nav_item(
                        Tab::Images,
                        "nav-images",
                        "Images",
                        Some(self.snapshot.images.len()),
                        cx,
                    ))
                    .when(engine.has_pods(), |el| {
                        el.child(self.nav_item(
                            Tab::Pods,
                            "nav-pods",
                            "Pods",
                            Some(self.snapshot.pods.len()),
                            cx,
                        ))
                    })
                    .child(self.nav_item(
                        Tab::Volumes,
                        "nav-volumes",
                        "Volumes",
                        Some(self.snapshot.volumes.len()),
                        cx,
                    ))
                    .child(self.nav_item(
                        Tab::Networks,
                        "nav-networks",
                        "Networks",
                        Some(self.snapshot.networks.len()),
                        cx,
                    ))
                    .when(engine.has_secrets(), |el| {
                        el.child(self.nav_item(
                            Tab::Secrets,
                            "nav-secrets",
                            "Secrets",
                            Some(self.snapshot.secrets.len()),
                            cx,
                        ))
                    }),
            )
            .when(engine.has_machines() || engine.has_connections(), |el| {
                el.child(
                    div()
                        .mt_6()
                        .mb_2()
                        .px_2()
                        .text_xs()
                        .text_color(rgb(theme().faint))
                        .child("ENGINES"),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .when(engine.has_machines(), |el| {
                            el.child(self.nav_item(
                                Tab::Machines,
                                "nav-machines",
                                "Machines",
                                Some(self.local.machines.len()),
                                cx,
                            ))
                        })
                        .when(engine.has_connections(), |el| {
                            el.child(self.nav_item(
                                Tab::Connections,
                                "nav-connections",
                                "Connections",
                                Some(self.local.connections.len()),
                                cx,
                            ))
                        }),
                )
            })
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(self.nav_item(Tab::Accounts, "nav-accounts", "Accounts", None, cx))
                    .child(self.nav_item(Tab::Settings, "nav-settings", "Settings", None, cx)),
            )
            .child(div().h(px(8.)))
            .child(
                div()
                    .rounded_lg()
                    .bg(rgb(theme().panel_bg))
                    .border_1()
                    .border_color(rgb(theme().border))
                    .p_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(dot(if connected {
                                theme().green
                            } else {
                                theme().red
                            }))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(div().text_xs().text_color(rgb(theme().text)).child(
                                        if connected {
                                            format!("{} engine", engine.label())
                                        } else {
                                            "Engine offline".to_string()
                                        },
                                    ))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(theme().muted))
                                            .whitespace_nowrap()
                                            .truncate()
                                            .child(version),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(theme().faint))
                                            .whitespace_nowrap()
                                            .truncate()
                                            .child(match (&engine, &self.settings.connection) {
                                                (EngineKind::Podman, Some(name)) => {
                                                    format!("via {name}")
                                                }
                                                (EngineKind::Podman, None) => {
                                                    "local default".to_string()
                                                }
                                                (EngineKind::Docker, _) => "Docker API".to_string(),
                                            }),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .mt_2()
                            .text_xs()
                            .text_color(rgb(theme().faint))
                            .child("gpui · experimental"),
                    ),
            )
    }
}
