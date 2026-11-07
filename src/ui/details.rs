use super::helpers::*;
use super::*;

impl PodmanDesktop {
    // -----------------------------------------------------------------------
    // Details pane
    // -----------------------------------------------------------------------

    pub(super) fn details_panel(&self, cx: &mut Context<Self>) -> Div {
        let Some(selection) = &self.selection else {
            return div();
        };

        let shell = div()
            .w(px(self.effective_details_width()))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(rgb(theme().sidebar_bg));

        if self.raw_view && inspect_kind(selection).is_some() {
            let (title, subtitle) = self.details_title(selection);
            return shell
                .child(self.details_header(&title, &subtitle, self.raw_toggle(selection, cx), cx))
                .child(self.raw_json_view(cx));
        }

        match selection {
            Selection::Container(id) => {
                let container = self.snapshot.containers.iter().find(|item| &item.id == id);
                let name = container
                    .map(|item| item.name.as_str())
                    .unwrap_or("Container");
                let subtitle = container
                    .map(|item| item.image.as_str())
                    .unwrap_or(id.as_str());
                let (color, color_bg) = container
                    .map(|item| state_colors(&item.state))
                    .unwrap_or_else(|| state_colors("unknown"));
                let state = container
                    .map(|item| item.state.as_str())
                    .unwrap_or("unknown");

                let mut panel = shell
                    .child(self.details_header(name, subtitle, self.raw_toggle(selection, cx), cx))
                    .child(
                        div()
                            .flex_shrink_0()
                            .px_4()
                            .pt_3()
                            .child(state_pill(state, color, color_bg)),
                    );
                panel = panel.child(self.details_tabs(cx));
                panel = panel.child(match self.details_tab {
                    DetailsTab::Summary | DetailsTab::History | DetailsTab::Files => {
                        self.summary_view(cx).into_any_element()
                    }
                    DetailsTab::Logs => self.logs_view(cx).into_any_element(),
                    DetailsTab::Stats => self.stats_view(cx).into_any_element(),
                    DetailsTab::Terminal => self.terminal_view(cx),
                });
                panel.child(self.container_action_bar(container, cx))
            }
            Selection::Compose(name) => {
                let project = self.snapshot.compose.iter().find(|item| &item.name == name);
                let title = project
                    .map(|item| item.name.as_str())
                    .unwrap_or(name.as_str());
                let subtitle = project
                    .map(|item| item.config_label())
                    .unwrap_or_else(|| "-".to_string());
                let state = match project {
                    Some(project) if project.total > 0 && project.running == project.total => {
                        "running"
                    }
                    Some(_) => "exited",
                    None => "unknown",
                };
                let (color, color_bg) = state_colors(state);
                let status = project
                    .map(|item| item.status_label())
                    .unwrap_or_else(|| "unknown".to_string());

                let mut panel = shell
                    .child(self.details_header(title, &subtitle, None, cx))
                    .child(
                        div()
                            .flex_shrink_0()
                            .px_4()
                            .pt_3()
                            .child(state_pill(&status, color, color_bg)),
                    );
                panel = panel.child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .gap_1()
                        .px_4()
                        .pt_3()
                        .child(self.details_tab_button(
                            "tab-summary",
                            "Summary",
                            DetailsTab::Summary,
                            cx,
                        ))
                        .child(self.details_tab_button("tab-logs", "Logs", DetailsTab::Logs, cx)),
                );
                panel = panel.child(match self.details_tab {
                    DetailsTab::Logs => self.logs_view(cx).into_any_element(),
                    _ => self.compose_summary(project, cx).into_any_element(),
                });
                panel.child(self.compose_action_bar(project, cx))
            }
            Selection::Image(id) => {
                let image = self.snapshot.images.iter().find(|item| &item.id == id);
                let name = image.map(|item| item.name.as_str()).unwrap_or("Image");

                let mut panel = shell.child(self.details_header(
                    name,
                    id.as_str(),
                    self.raw_toggle(selection, cx),
                    cx,
                ));
                panel = panel.child(self.image_tabs(cx));
                panel = panel.child(match self.details_tab {
                    DetailsTab::History => self.image_history_view(),
                    DetailsTab::Files => self.image_files_view(cx),
                    _ => self.image_summary(image).into_any_element(),
                });
                panel.child(self.image_action_bar(image, cx))
            }
            Selection::Pod(id) => {
                let pod = self.snapshot.pods.iter().find(|item| &item.id == id);
                let name = pod.map(|item| item.name.as_str()).unwrap_or("Pod");
                let status = pod.map(|item| item.status.as_str()).unwrap_or("unknown");
                let (color, color_bg) = state_colors(status);

                let mut panel = shell
                    .child(self.details_header(name, id, self.raw_toggle(selection, cx), cx))
                    .child(
                        div()
                            .flex_shrink_0()
                            .px_4()
                            .pt_3()
                            .child(state_pill(status, color, color_bg)),
                    );
                panel = panel.child(self.pod_summary(cx));
                panel.child(self.pod_action_bar(pod, cx))
            }
            Selection::Volume(name) => {
                let volume = self.snapshot.volumes.iter().find(|item| &item.name == name);
                let title = volume.map(|item| item.name.as_str()).unwrap_or("Volume");
                let subtitle = volume
                    .map(|item| item.mountpoint.as_str())
                    .unwrap_or(name.as_str());

                let mut panel = shell.child(self.details_header(
                    title,
                    subtitle,
                    self.raw_toggle(selection, cx),
                    cx,
                ));
                panel = panel.child(self.volume_summary(cx));
                panel.child(self.volume_action_bar(volume, cx))
            }
            Selection::Network(name) => {
                let network = self
                    .snapshot
                    .networks
                    .iter()
                    .find(|item| &item.name == name);
                let title = network.map(|item| item.name.as_str()).unwrap_or("Network");
                let subtitle = network
                    .map(|item| item.subnet_label())
                    .unwrap_or_else(|| "-".to_string());

                let mut panel = shell.child(self.details_header(
                    title,
                    &subtitle,
                    self.raw_toggle(selection, cx),
                    cx,
                ));
                panel = panel.child(self.network_summary(cx));
                panel.child(self.network_action_bar(network, cx))
            }
            Selection::Secret(id) => {
                let secret = self.snapshot.secrets.iter().find(|item| &item.id == id);
                let title = secret.map(|item| item.name.as_str()).unwrap_or("Secret");

                let mut panel =
                    shell.child(self.details_header(title, id, self.raw_toggle(selection, cx), cx));
                panel = panel.child(self.secret_summary(secret));
                panel.child(self.secret_action_bar(secret, cx))
            }
            Selection::Machine(name) => {
                let machine = self.local.machines.iter().find(|item| &item.name == name);
                let title = machine.map(|item| item.name.as_str()).unwrap_or("Machine");
                let subtitle = machine.map(|item| item.vm_type.as_str()).unwrap_or("-");
                let state = machine.map(|item| item.state_label()).unwrap_or("unknown");
                let (color, color_bg) = state_colors(state);

                let mut panel = shell
                    .child(self.details_header(title, subtitle, None, cx))
                    .child(
                        div()
                            .flex_shrink_0()
                            .px_4()
                            .pt_3()
                            .child(state_pill(state, color, color_bg)),
                    );
                panel = panel.child(match self.details_tab {
                    DetailsTab::Terminal => self.terminal_view(cx),
                    _ => self.machine_summary(machine).into_any_element(),
                });
                panel.child(self.machine_action_bar(machine, cx))
            }
            Selection::Connection(name) => {
                let connection = self
                    .local
                    .connections
                    .iter()
                    .find(|item| &item.name == name);
                let title = connection
                    .map(|item| item.name.as_str())
                    .unwrap_or("Connection");
                let subtitle = connection.map(|item| item.uri.as_str()).unwrap_or("-");
                let in_use = self.settings.connection.as_deref() == Some(name.as_str());
                let (color, color_bg) = if in_use {
                    (theme().accent, theme().accent_soft)
                } else {
                    (theme().muted, theme().panel_bg)
                };

                let mut panel = shell
                    .child(self.details_header(title, subtitle, None, cx))
                    .child(div().flex_shrink_0().px_4().pt_3().child(state_pill(
                        if in_use { "in use" } else { "not in use" },
                        color,
                        color_bg,
                    )));
                panel = panel.child(self.connection_summary(connection, in_use));
                panel.child(self.connection_action_bar(connection, in_use, cx))
            }
        }
    }

    fn details_header(
        &self,
        title: &str,
        subtitle: &str,
        raw: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> Div {
        let text_width = (self.effective_details_width() - 32.0 - 24.0 - 8.0 - 48.0).max(60.0);
        div()
            .flex_shrink_0()
            .border_b_1()
            .border_color(rgb(theme().border))
            .p_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.))
                            .child(
                                div()
                                    .w_full()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .whitespace_nowrap()
                                    .truncate()
                                    .child(truncate_sm(title, text_width)),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .text_xs()
                                    .text_color(rgb(theme().muted))
                                    .whitespace_nowrap()
                                    .truncate()
                                    .child(truncate_xs(subtitle, text_width)),
                            ),
                    )
                    .when_some(raw, |el, raw| el.child(raw))
                    .child(
                        div()
                            .id("details-close")
                            .flex_shrink_0()
                            .w(px(24.))
                            .h(px(24.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(theme().hover_bg)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.clear_selection();
                                cx.notify();
                            }))
                            .child(div().text_color(rgb(theme().muted)).child("×")),
                    ),
            )
    }

    /// Title and subtitle for the raw JSON view, where the summary branch is
    /// not rendered and therefore cannot supply them.
    fn details_title(&self, selection: &Selection) -> (String, String) {
        match selection {
            Selection::Container(id) => {
                let item = self.snapshot.containers.iter().find(|item| &item.id == id);
                (
                    item.map(|item| item.name.clone())
                        .unwrap_or_else(|| "Container".to_string()),
                    item.map(|item| item.image.clone())
                        .unwrap_or_else(|| id.clone()),
                )
            }
            Selection::Image(id) => {
                let name = self
                    .snapshot
                    .images
                    .iter()
                    .find(|item| &item.id == id)
                    .map(|item| item.name.clone())
                    .unwrap_or_else(|| "Image".to_string());
                (name, id.clone())
            }
            Selection::Pod(id) => {
                let name = self
                    .snapshot
                    .pods
                    .iter()
                    .find(|item| &item.id == id)
                    .map(|item| item.name.clone())
                    .unwrap_or_else(|| "Pod".to_string());
                (name, id.clone())
            }
            Selection::Volume(name) => {
                let item = self.snapshot.volumes.iter().find(|item| &item.name == name);
                (
                    name.clone(),
                    item.map(|item| item.mountpoint.clone()).unwrap_or_default(),
                )
            }
            Selection::Network(name) => {
                let item = self
                    .snapshot
                    .networks
                    .iter()
                    .find(|item| &item.name == name);
                (
                    name.clone(),
                    item.map(|item| item.subnet_label())
                        .unwrap_or_else(|| "-".to_string()),
                )
            }
            Selection::Secret(id) => {
                let name = self
                    .snapshot
                    .secrets
                    .iter()
                    .find(|item| &item.id == id)
                    .map(|item| item.name.clone())
                    .unwrap_or_else(|| "Secret".to_string());
                (name, id.clone())
            }
            Selection::Compose(_) | Selection::Machine(_) | Selection::Connection(_) => {
                (String::new(), String::new())
            }
        }
    }

    /// Toggle between the summary and the raw `inspect` JSON.
    fn raw_toggle(&self, selection: &Selection, cx: &mut Context<Self>) -> Option<AnyElement> {
        inspect_kind(selection)?;
        let active = self.raw_view;
        let selection = selection.clone();
        Some(
            small_button(
                if active { "raw-on" } else { "raw-off" },
                if active { "Summary" } else { "JSON" },
                active,
                cx.listener(move |this, _, _, cx| this.toggle_raw(selection.clone(), cx)),
            )
            .into_any_element(),
        )
    }

    fn raw_json_view(&self, cx: &mut Context<Self>) -> AnyElement {
        match &self.raw {
            Some(RawState::Text(text)) => {
                let text = text.clone();
                let copy = text.clone();
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .px_4()
                            .py_2()
                            .border_b_1()
                            .border_color(rgb(theme().border))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(theme().muted))
                                    .child(format!(
                                        "{} lines · inspect JSON",
                                        text.lines().count()
                                    )),
                            )
                            .child(small_button(
                                "raw-copy",
                                "Copy",
                                false,
                                cx.listener(move |this, _, _, cx| {
                                    this.copy_text(copy.clone(), "inspect JSON", cx);
                                }),
                            )),
                    )
                    .child(
                        div()
                            .id("raw-json-scroll")
                            .flex_1()
                            .min_h(px(0.))
                            .overflow_y_scroll()
                            .p_4()
                            .child(
                                div()
                                    .text_xs()
                                    .font_family("monospace")
                                    .whitespace_normal()
                                    .text_color(rgb(theme().text))
                                    .child(text),
                            ),
                    )
                    .into_any_element()
            }
            Some(RawState::Error(error)) => div()
                .p_4()
                .text_sm()
                .text_color(rgb(theme().red))
                .child(error.clone())
                .into_any_element(),
            Some(RawState::Loading) | None => div()
                .p_4()
                .text_sm()
                .text_color(rgb(theme().faint))
                .child("Loading inspect JSON…")
                .into_any_element(),
        }
    }

    fn image_tabs(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_1()
            .px_4()
            .pt_3()
            .child(self.details_tab_button("tab-summary", "Summary", DetailsTab::Summary, cx))
            .child(self.details_tab_button("tab-history", "History", DetailsTab::History, cx))
            .child(self.details_tab_button("tab-files", "Files", DetailsTab::Files, cx))
    }

    fn details_tabs(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_1()
            .px_4()
            .pt_3()
            .child(self.details_tab_button("tab-summary", "Summary", DetailsTab::Summary, cx))
            .child(self.details_tab_button(
                "tab-logs",
                &format!("Logs ({})", self.logs.len()),
                DetailsTab::Logs,
                cx,
            ))
            .child(self.details_tab_button("tab-stats", "Stats", DetailsTab::Stats, cx))
            .child(self.details_tab_button("tab-terminal", "Terminal", DetailsTab::Terminal, cx))
    }

    fn details_tab_button(
        &self,
        id: &'static str,
        label: &str,
        tab: DetailsTab,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = self.details_tab == tab;
        div()
            .id(id)
            .px_2()
            .py_1()
            .rounded_sm()
            .cursor_pointer()
            .when(active, |el| el.bg(rgb(theme().accent_soft)))
            .when(!active, |el| {
                el.hover(|style| style.bg(rgb(theme().hover_bg)))
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                this.details_tab = tab;
                match tab {
                    DetailsTab::Stats => {
                        if let Some(Selection::Container(id)) = this.selection.clone() {
                            this.start_stats(id, cx);
                        }
                    }
                    DetailsTab::Terminal => {
                        this.stop_stats();
                        if let Some(Selection::Container(id)) = this.selection.clone() {
                            let reuse = this.terminal.is_some()
                                && matches!(
                                    &this.terminal_target,
                                    Some(TerminalTarget::Container(target)) if target == &id
                                );
                            if reuse {
                                this.terminal_focus.focus(window);
                            } else {
                                this.start_container_terminal(id, window, cx);
                            }
                        }
                    }
                    DetailsTab::History => {
                        this.stop_stats();
                        if let Some(Selection::Image(id)) = this.selection.clone() {
                            this.load_image_history(id, cx);
                        }
                    }
                    DetailsTab::Files => {
                        this.stop_stats();
                        if let Some(Selection::Image(id)) = this.selection.clone() {
                            this.load_image_files(id, cx);
                        }
                    }
                    DetailsTab::Summary | DetailsTab::Logs => {
                        this.stop_stats();
                        if let Some(Selection::Compose(name)) = this.selection.clone() {
                            let project = this
                                .snapshot
                                .compose
                                .iter()
                                .find(|item| item.name == name)
                                .cloned();
                            if let Some(project) = project {
                                let reuse = matches!(
                                    &this.logs_target,
                                    Some(LogTarget::Compose {
                                        project: current,
                                        service: None,
                                    }) if current == &project.name
                                );
                                if !reuse {
                                    this.start_compose_logs(&project, None, cx);
                                }
                            }
                        }
                    }
                }
                cx.notify();
            }))
            .child(
                div()
                    .text_xs()
                    .text_color(if active {
                        rgb(theme().accent)
                    } else {
                        rgb(theme().muted)
                    })
                    .child(label.to_string()),
            )
    }

    fn summary_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut content = div()
            .id("details-summary")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .p_4()
            .flex()
            .flex_col()
            .gap_3();

        match &self.details {
            DetailsState::None => {}
            DetailsState::Loading => {
                content = content
                    .child(section_title("DETAILS"))
                    .child(muted_text("Loading…"));
            }
            DetailsState::Error(error) => {
                content = content
                    .child(section_title("DETAILS"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(theme().red))
                            .child(error.clone()),
                    )
                    .child(pill_button(
                        "reload-details",
                        "Retry",
                        true,
                        theme().accent,
                        theme().accent_soft,
                        cx.listener(|this, _, _, cx| this.reload_selection(cx)),
                    ));
            }
            DetailsState::Container(details) => {
                content = content
                    .child(section_title("GENERAL"))
                    .child(detail_row("Name", or_dash(&details.name)))
                    .child(detail_row("ID", or_dash(&details.id)))
                    .child(detail_row("Image", or_dash(&details.image)))
                    .child(detail_row("Status", or_dash(&details.status)))
                    .child(detail_row("Created", or_dash(&details.created)))
                    .child(detail_row("PID", &details.pid.to_string()))
                    .child(detail_row("Exit code", &details.exit_code.to_string()))
                    .child(detail_row("Networks", &join_or_dash(&details.networks)))
                    .child(section_title("COMMAND"))
                    .child(mono_text(or_dash(&details.command).to_string()));

                if let Some(project) = &details.compose_project {
                    content = content
                        .child(section_title("COMPOSE"))
                        .child(detail_row("Project", project))
                        .child(detail_row(
                            "Service",
                            or_dash(details.compose_service.as_deref().unwrap_or("")),
                        ));
                }

                content = content.child(section_title("MOUNTS"));
                if details.mounts.is_empty() {
                    content = content.child(muted_text("No mounts"));
                } else {
                    for mount in &details.mounts {
                        content = content.child(
                            div()
                                .flex()
                                .flex_col()
                                .child(mono_text(format!(
                                    "{} → {}",
                                    mount.source, mount.destination
                                )))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(theme().faint))
                                        .child(or_dash(&mount.kind).to_string()),
                                ),
                        );
                    }
                }

                content = content.child(section_title("ENVIRONMENT"));
                if details.env.is_empty() {
                    content = content.child(muted_text("No environment variables"));
                } else {
                    for variable in &details.env {
                        content = content.child(mono_text(variable.clone()));
                    }
                }
            }
            _ => {}
        }

        content
    }

    fn logs_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let compose = match &self.logs_target {
            Some(LogTarget::Compose { project, service }) => {
                Some((project.clone(), service.clone()))
            }
            _ => None,
        };
        let compose_project = compose.as_ref().and_then(|(project, _)| {
            self.snapshot
                .compose
                .iter()
                .find(|item| &item.name == project)
                .cloned()
        });

        let mut label = format!("{} lines", self.logs.len());
        if let Some((project, service)) = &compose {
            label.push_str(" · ");
            match service {
                Some(service) => label.push_str(&format!("{project} / {service}")),
                None => label.push_str(&format!("{project} / all services")),
            }
        }

        // `--tail` is a snapshot size, not a time window, so the window and
        // "load older" controls only make sense for container logs.
        let controls = div()
            .flex()
            .items_center()
            .gap_1()
            .when(compose.is_none(), |el| {
                el.child(small_button(
                    "logs-older",
                    if self.logs_loading_older {
                        "Loading…"
                    } else {
                        "Load older"
                    },
                    false,
                    cx.listener(|this, _, _, cx| this.load_older_logs(cx)),
                ))
                .child(small_button(
                    "logs-window",
                    format!("last {}", self.settings.log_window),
                    false,
                    cx.listener(|this, _, _, cx| this.cycle_log_window(cx)),
                ))
            })
            .child(small_button(
                "logs-follow",
                if self.logs_follow {
                    "Follow on"
                } else {
                    "Follow off"
                },
                self.logs_follow,
                cx.listener(|this, _, _, cx| {
                    this.logs_follow = !this.logs_follow;
                    if this.logs_follow {
                        this.logs_handle.scroll_to_bottom();
                    }
                    cx.notify();
                }),
            ))
            .child(small_button(
                "logs-clear",
                "Clear",
                false,
                cx.listener(|this, _, _, cx| {
                    this.logs.clear();
                    this.logs_limit = MAX_LOG_LINES;
                    cx.notify();
                }),
            ))
            .when(!self.logs.is_empty(), |el| {
                el.child(small_button(
                    "logs-copy",
                    "Copy",
                    false,
                    cx.listener(|this, _, _, cx| {
                        let text = this.logs.join("\n");
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                        this.toast(ToastKind::Success, "Logs copied", cx);
                    }),
                ))
            });

        let mut view = div().flex_1().min_h(px(0.)).flex().flex_col().child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .px_4()
                .py_2()
                .border_b_1()
                .border_color(rgb(theme().border))
                .child(div().text_xs().text_color(rgb(theme().muted)).child(label))
                .child(controls),
        );

        if let Some(project) = &compose_project {
            let mut chips = div()
                .flex()
                .items_center()
                .gap_1()
                .px_4()
                .py_2()
                .border_b_1()
                .border_color(rgb(theme().border));

            let all_selected = compose
                .as_ref()
                .is_some_and(|(_, service)| service.is_none());
            let all_project = project.clone();
            chips = chips.child(small_button_dyn(
                SharedString::from("logs-service-all"),
                "All services",
                all_selected,
                cx.listener(move |this, _, _, cx| this.start_compose_logs(&all_project, None, cx)),
            ));

            for service in &project.services {
                let selected = compose
                    .as_ref()
                    .is_some_and(|(_, current)| current.as_deref() == Some(service.name.as_str()));
                let chip_project = project.clone();
                let name = service.name.clone();
                chips = chips.child(small_button_dyn(
                    SharedString::from(format!("logs-service-{name}")),
                    name.clone(),
                    selected,
                    cx.listener(move |this, _, _, cx| {
                        this.start_compose_logs(&chip_project, Some(name.clone()), cx);
                    }),
                ));
            }

            view = view.child(chips);
        }

        let empty_hint = if compose.is_some() {
            format!("Waiting for compose log output (last {COMPOSE_LOG_TAIL} lines)…")
        } else {
            format!(
                "Waiting for log output (last {})…",
                self.settings.log_window
            )
        };

        let body: AnyElement = if let Some(error) = &self.logs_error {
            div()
                .p_4()
                .text_sm()
                .text_color(rgb(theme().red))
                .child(error.clone())
                .into_any_element()
        } else if self.logs.is_empty() {
            div()
                .p_4()
                .text_sm()
                .text_color(rgb(theme().faint))
                .child(empty_hint)
                .into_any_element()
        } else {
            let mut lines = div()
                .id("logs-scroll")
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .track_scroll(&self.logs_handle)
                .px_3()
                .py_2()
                .flex()
                .flex_col();
            for line in &self.logs {
                lines = lines.child(
                    div()
                        .text_xs()
                        .font_family("monospace")
                        .text_color(rgb(theme().text))
                        .child(line.clone()),
                );
            }
            lines.into_any_element()
        };

        view.child(body)
    }

    fn stats_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut view = div().flex_1().min_h(px(0.)).flex().flex_col();

        view = view.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .px_4()
                .py_2()
                .border_b_1()
                .border_color(rgb(theme().border))
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(theme().muted))
                        .child(format!(
                            "{} samples · every {}s",
                            self.stats.len(),
                            STATS_INTERVAL.as_secs()
                        )),
                )
                .child(small_button(
                    "stats-clear",
                    "Clear",
                    false,
                    cx.listener(|this, _, _, cx| {
                        this.stats.clear();
                        cx.notify();
                    }),
                )),
        );

        let running = match &self.selection {
            Some(Selection::Container(id)) => self
                .snapshot
                .containers
                .iter()
                .find(|item| &item.id == id)
                .is_some_and(|item| item.state == "running"),
            _ => false,
        };

        let body: AnyElement = if let Some(error) = &self.stats_error {
            div()
                .p_4()
                .text_sm()
                .text_color(rgb(theme().red))
                .child(error.clone())
                .into_any_element()
        } else if self.stats.is_empty() {
            div()
                .p_4()
                .text_sm()
                .text_color(rgb(theme().faint))
                .child("Collecting statistics…")
                .into_any_element()
        } else if !running {
            div()
                .p_4()
                .text_sm()
                .text_color(rgb(theme().faint))
                .child("Container is not running. Sampling resumes when it starts.")
                .into_any_element()
        } else {
            self.stats_charts().into_any_element()
        };

        view.child(body)
    }

    fn stats_charts(&self) -> AnyElement {
        let Some(latest) = self.stats.last() else {
            return div().into_any_element();
        };

        let cpu_values: Vec<f64> = self.stats.iter().map(|sample| sample.cpu_percent).collect();
        let cpu_max = cpu_values.iter().copied().fold(100.0, f64::max);

        let memory_values: Vec<f64> = self
            .stats
            .iter()
            .map(|sample| sample.mem_usage as f64)
            .collect();
        let memory_max = memory_values.iter().copied().fold(1.0, f64::max) * 1.1;

        let mut network_values = Vec::with_capacity(self.stats.len());
        let mut previous: Option<&StatsSample> = None;
        for sample in &self.stats {
            let rate = previous
                .map(|previous| {
                    let elapsed = sample
                        .at
                        .saturating_duration_since(previous.at)
                        .as_secs_f64()
                        .max(0.001);
                    let bytes = sample.net_in.saturating_sub(previous.net_in)
                        + sample.net_out.saturating_sub(previous.net_out);
                    bytes as f64 / elapsed
                })
                .unwrap_or_default();
            network_values.push(rate);
            previous = Some(sample);
        }
        let network_max = network_values.iter().copied().fold(1.0, f64::max);
        let latest_rate = network_values.last().copied().unwrap_or_default();

        div()
            .id("stats-scroll")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .p_4()
            .flex()
            .flex_col()
            .gap_4()
            .child(stats_chart(
                "CPU",
                format!("{:.1}%", latest.cpu_percent),
                &cpu_values,
                cpu_max,
                theme().accent,
            ))
            .child(stats_chart(
                "MEMORY",
                format!("{} ({:.1}%)", latest.memory_label(), latest.mem_percent),
                &memory_values,
                memory_max,
                theme().blue,
            ))
            .child(stats_chart(
                "NETWORK",
                format!("{}/s", crate::podman::human_size(latest_rate as u64)),
                &network_values,
                network_max,
                theme().green,
            ))
            .child(section_title("DETAILS"))
            .child(detail_row("Network total", &latest.network_label()))
            .child(detail_row("Block I/O", &latest.block_label()))
            .child(detail_row("PIDs", &latest.pids.to_string()))
            .into_any_element()
    }

    fn container_action_bar(&self, container: Option<&Container>, cx: &mut Context<Self>) -> Div {
        let mut bar = div()
            .flex_shrink_0()
            .border_t_1()
            .border_color(rgb(theme().border))
            .p_3()
            .flex()
            .items_center()
            .gap_2();

        if let Some(label) = &self.busy {
            return bar.child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().accent))
                    .child(label.clone()),
            );
        }

        let Some(container) = container else {
            return bar;
        };

        let running = container.state == "running";
        let paused = container.state == "paused";
        let id = container.id.clone();
        let name = container.name.clone();

        let start_id = id.clone();
        let pause_id = id.clone();
        let unpause_id = id.clone();
        let stop_id = id.clone();
        let restart_id = id.clone();
        let delete_id = id.clone();
        let delete_name = name.clone();

        bar = if paused {
            bar.child(pill_button(
                "action-resume",
                "Resume",
                true,
                theme().green,
                theme().green_bg,
                cx.listener(move |this, _, _, cx| {
                    this.container_action(unpause_id.clone(), ContainerAction::Unpause, cx)
                }),
            ))
        } else {
            bar.child(pill_button(
                "action-start",
                "Start",
                !running,
                theme().green,
                theme().green_bg,
                cx.listener(move |this, _, _, cx| {
                    this.container_action(start_id.clone(), ContainerAction::Start, cx)
                }),
            ))
        };

        if running {
            bar = bar.child(pill_button(
                "action-pause",
                "Pause",
                true,
                theme().yellow,
                theme().yellow_bg,
                cx.listener(move |this, _, _, cx| {
                    this.container_action(pause_id.clone(), ContainerAction::Pause, cx)
                }),
            ));
        }

        bar = bar
            .child(pill_button(
                "action-stop",
                "Stop",
                running || paused,
                theme().yellow,
                theme().yellow_bg,
                cx.listener(move |this, _, _, cx| {
                    this.container_action(stop_id.clone(), ContainerAction::Stop, cx)
                }),
            ))
            .child(pill_button(
                "action-restart",
                "Restart",
                running,
                theme().blue,
                theme().blue_bg,
                cx.listener(move |this, _, _, cx| {
                    this.container_action(restart_id.clone(), ContainerAction::Restart, cx)
                }),
            ))
            .child(pill_button(
                "action-delete",
                "Delete",
                true,
                theme().red,
                theme().red_bg,
                cx.listener(move |this, _, window, cx| {
                    this.open_modal(
                        Modal::DeleteContainer {
                            id: delete_id.clone(),
                            name: delete_name.clone(),
                        },
                        window,
                        cx,
                    )
                }),
            ));

        bar
    }

    fn image_summary(&self, image: Option<&Image>) -> impl IntoElement {
        let mut content = div()
            .id("image-summary")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .p_4()
            .flex()
            .flex_col()
            .gap_3();

        match image {
            None => {
                content = content.child(muted_text("This image no longer exists."));
            }
            Some(image) => {
                content = content
                    .child(section_title("GENERAL"))
                    .child(detail_row("Name", or_dash(&image.name)))
                    .child(detail_row("ID", or_dash(&image.id)))
                    .child(detail_row("Digest", or_dash(image.digest_short().as_str())))
                    .child(detail_row("Size", &image.size_label()))
                    .child(detail_row("Created", or_dash(&image.created)))
                    .child(detail_row(
                        "Containers using it",
                        &image.containers.to_string(),
                    ));
            }
        }

        content
    }

    fn image_action_bar(&self, image: Option<&Image>, cx: &mut Context<Self>) -> Div {
        let mut bar = div()
            .flex_shrink_0()
            .border_t_1()
            .border_color(rgb(theme().border))
            .p_3()
            .flex()
            .items_center()
            .gap_2();

        if let Some(label) = &self.busy {
            return bar.child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().accent))
                    .child(label.clone()),
            );
        }

        let Some(image) = image else {
            return bar;
        };

        let id = image.id.clone();
        let name = image.name.clone();
        let tag_id = id.clone();
        let tag_source = name.clone();
        let run_image = name.clone();
        let push_image = name.clone();

        bar = bar
            .child(pill_button(
                "image-run",
                "Run",
                true,
                theme().green,
                theme().green_bg,
                cx.listener(move |this, _, window, cx| {
                    this.open_modal(Modal::run_container(run_image.clone()), window, cx)
                }),
            ))
            .child(pill_button(
                "image-push",
                "Push",
                self.output_run.is_none(),
                theme().blue,
                theme().blue_bg,
                cx.listener(move |this, _, window, cx| {
                    this.open_modal(
                        Modal::PushImage {
                            destination: push_image.clone(),
                            image: push_image.clone(),
                        },
                        window,
                        cx,
                    )
                }),
            ))
            .child(pill_button(
                "image-tag",
                "Tag",
                true,
                theme().accent,
                theme().accent_soft,
                cx.listener(move |this, _, window, cx| {
                    this.open_modal(
                        Modal::TagImage {
                            id: tag_id.clone(),
                            source: tag_source.clone(),
                            input: String::new(),
                        },
                        window,
                        cx,
                    )
                }),
            ))
            .child(pill_button(
                "image-delete",
                "Delete",
                true,
                theme().red,
                theme().red_bg,
                cx.listener(move |this, _, window, cx| {
                    this.open_modal(
                        Modal::DeleteImage {
                            id: id.clone(),
                            name: name.clone(),
                        },
                        window,
                        cx,
                    )
                }),
            ));

        bar
    }

    fn pod_summary(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut content = div()
            .id("pod-summary")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .p_4()
            .flex()
            .flex_col()
            .gap_3();

        match &self.details {
            DetailsState::None => {}
            DetailsState::Loading => {
                content = content
                    .child(section_title("DETAILS"))
                    .child(muted_text("Loading…"));
            }
            DetailsState::Error(error) => {
                content = content
                    .child(section_title("DETAILS"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(theme().red))
                            .child(error.clone()),
                    )
                    .child(pill_button(
                        "reload-pod",
                        "Retry",
                        true,
                        theme().accent,
                        theme().accent_soft,
                        cx.listener(|this, _, _, cx| this.reload_selection(cx)),
                    ));
            }
            DetailsState::Pod(details) => {
                content = content
                    .child(section_title("GENERAL"))
                    .child(detail_row("Name", or_dash(&details.name)))
                    .child(detail_row("ID", or_dash(&details.id)))
                    .child(detail_row("State", or_dash(&details.state)))
                    .child(detail_row("Created", or_dash(&details.created)))
                    .child(detail_row("Containers", &details.containers.to_string()))
                    .child(detail_row("Exit policy", or_dash(&details.exit_policy)))
                    .child(detail_row("Hostname", or_dash(&details.hostname)))
                    .child(detail_row(
                        "Infra container",
                        bool_label(details.create_infra),
                    ));
            }
            _ => {}
        }

        content
    }

    fn volume_summary(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut content = div()
            .id("volume-summary")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .p_4()
            .flex()
            .flex_col()
            .gap_3();

        match &self.details {
            DetailsState::None => {}
            DetailsState::Loading => {
                content = content
                    .child(section_title("DETAILS"))
                    .child(muted_text("Loading…"));
            }
            DetailsState::Error(error) => {
                content = content
                    .child(section_title("DETAILS"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(theme().red))
                            .child(error.clone()),
                    )
                    .child(pill_button(
                        "reload-volume",
                        "Retry",
                        true,
                        theme().accent,
                        theme().accent_soft,
                        cx.listener(|this, _, _, cx| this.reload_selection(cx)),
                    ));
            }
            DetailsState::Volume(details) => {
                content = content
                    .child(section_title("GENERAL"))
                    .child(detail_row("Name", or_dash(&details.name)))
                    .child(detail_row("Driver", or_dash(&details.driver)))
                    .child(detail_row("Scope", or_dash(&details.scope)))
                    .child(detail_row("Created", or_dash(&details.created)))
                    .child(detail_row("Mounts", &details.mounts.to_string()))
                    .child(detail_row("Anonymous", bool_label(details.anonymous)))
                    .child(section_title("MOUNTPOINT"))
                    .child(mono_text(or_dash(&details.mountpoint).to_string()));
            }
            _ => {}
        }

        content
    }

    /// Everything here comes from the list: the secret's data is never read.
    fn secret_summary(&self, secret: Option<&Secret>) -> impl IntoElement {
        let mut content = div()
            .id("secret-summary")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .p_4()
            .flex()
            .flex_col()
            .gap_3();

        let Some(secret) = secret else {
            return content.child(muted_text("This secret no longer exists."));
        };
        let engine = podman::connection().unwrap_or_else(|| "default".to_string());

        content = content
            .child(section_title("DETAILS"))
            .child(detail_row("Name", or_dash(&secret.name)))
            .child(detail_row("ID", or_dash(&secret.id)))
            .child(detail_row("Driver", or_dash(&secret.driver)))
            .child(detail_row("Created", or_dash(&secret.created)))
            .child(detail_row("Updated", or_dash(&secret.updated)))
            .child(detail_row("Engine", &engine))
            .child(detail_row("Engine type", "podman"))
            .child(section_title("LABELS"));

        if secret.labels.is_empty() {
            content = content.child(muted_text("No labels"));
        } else {
            for (key, value) in &secret.labels {
                content = content.child(detail_row(key, or_dash(value)));
            }
        }
        content
    }

    fn secret_action_bar(&self, secret: Option<&Secret>, cx: &mut Context<Self>) -> Div {
        let bar = div()
            .flex_shrink_0()
            .border_t_1()
            .border_color(rgb(theme().border))
            .p_3()
            .flex()
            .items_center()
            .gap_2();

        if let Some(label) = &self.busy {
            return bar.child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().accent))
                    .child(label.clone()),
            );
        }

        let Some(secret) = secret else {
            return bar;
        };
        let id = secret.id.clone();
        let name = secret.name.clone();

        bar.child(pill_button(
            "secret-delete",
            "Delete",
            true,
            theme().red,
            theme().red_bg,
            cx.listener(move |this, _, window, cx| {
                this.open_modal(
                    Modal::DeleteSecret {
                        id: id.clone(),
                        name: name.clone(),
                    },
                    window,
                    cx,
                )
            }),
        ))
    }

    fn network_summary(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut content = div()
            .id("network-summary")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .p_4()
            .flex()
            .flex_col()
            .gap_3();

        match &self.details {
            DetailsState::None => {}
            DetailsState::Loading => {
                content = content
                    .child(section_title("DETAILS"))
                    .child(muted_text("Loading…"));
            }
            DetailsState::Error(error) => {
                content = content
                    .child(section_title("DETAILS"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(theme().red))
                            .child(error.clone()),
                    )
                    .child(pill_button(
                        "reload-network",
                        "Retry",
                        true,
                        theme().accent,
                        theme().accent_soft,
                        cx.listener(|this, _, _, cx| this.reload_selection(cx)),
                    ));
            }
            DetailsState::Network(details) => {
                content = content
                    .child(section_title("GENERAL"))
                    .child(detail_row("Name", or_dash(&details.name)))
                    .child(detail_row("ID", or_dash(&details.id)))
                    .child(detail_row("Driver", or_dash(&details.driver)))
                    .child(detail_row("Interface", or_dash(&details.interface)))
                    .child(detail_row("Created", or_dash(&details.created)))
                    .child(detail_row("Internal", bool_label(details.internal)))
                    .child(detail_row("IPv6", bool_label(details.ipv6)))
                    .child(detail_row("DNS", bool_label(details.dns)))
                    .child(section_title("SUBNETS"));

                if details.subnets.is_empty() {
                    content = content.child(muted_text("No subnets"));
                } else {
                    for subnet in &details.subnets {
                        content = content
                            .child(mono_text(format!("{} → {}", subnet.subnet, subnet.gateway)));
                    }
                }

                content = content.child(section_title("CONNECTED CONTAINERS"));
                if details.containers.is_empty() {
                    content = content.child(muted_text("None"));
                } else {
                    for container in &details.containers {
                        content = content.child(mono_text(container.clone()));
                    }
                }
            }
            _ => {}
        }

        content
    }

    fn pod_action_bar(&self, pod: Option<&Pod>, cx: &mut Context<Self>) -> Div {
        let mut bar = div()
            .flex_shrink_0()
            .border_t_1()
            .border_color(rgb(theme().border))
            .p_3()
            .flex()
            .items_center()
            .gap_2();

        if let Some(label) = &self.busy {
            return bar.child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().accent))
                    .child(label.clone()),
            );
        }

        let Some(pod) = pod else {
            return bar;
        };

        let running = pod.status == "Running";
        let id = pod.id.clone();
        let name = pod.name.clone();

        let start_id = id.clone();
        let stop_id = id.clone();
        let restart_id = id.clone();
        let delete_id = id.clone();

        bar = bar
            .child(pill_button(
                "pod-start",
                "Start",
                !running,
                theme().green,
                theme().green_bg,
                cx.listener(move |this, _, _, cx| {
                    this.pod_action(start_id.clone(), PodAction::Start, cx)
                }),
            ))
            .child(pill_button(
                "pod-stop",
                "Stop",
                running,
                theme().yellow,
                theme().yellow_bg,
                cx.listener(move |this, _, _, cx| {
                    this.pod_action(stop_id.clone(), PodAction::Stop, cx)
                }),
            ))
            .child(pill_button(
                "pod-restart",
                "Restart",
                running,
                theme().blue,
                theme().blue_bg,
                cx.listener(move |this, _, _, cx| {
                    this.pod_action(restart_id.clone(), PodAction::Restart, cx)
                }),
            ))
            .child(pill_button(
                "pod-delete",
                "Delete",
                true,
                theme().red,
                theme().red_bg,
                cx.listener(move |this, _, window, cx| {
                    this.open_modal(
                        Modal::DeletePod {
                            id: delete_id.clone(),
                            name: name.clone(),
                        },
                        window,
                        cx,
                    )
                }),
            ));

        bar
    }

    fn volume_action_bar(&self, volume: Option<&Volume>, cx: &mut Context<Self>) -> Div {
        let bar = div()
            .flex_shrink_0()
            .border_t_1()
            .border_color(rgb(theme().border))
            .p_3()
            .flex()
            .items_center()
            .gap_2();

        if let Some(label) = &self.busy {
            return bar.child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().accent))
                    .child(label.clone()),
            );
        }

        let Some(volume) = volume else {
            return bar;
        };
        let name = volume.name.clone();

        bar.child(pill_button(
            "volume-delete",
            "Delete",
            true,
            theme().red,
            theme().red_bg,
            cx.listener(move |this, _, window, cx| {
                this.open_modal(Modal::DeleteVolume { name: name.clone() }, window, cx)
            }),
        ))
    }

    fn network_action_bar(&self, network: Option<&Network>, cx: &mut Context<Self>) -> Div {
        let bar = div()
            .flex_shrink_0()
            .border_t_1()
            .border_color(rgb(theme().border))
            .p_3()
            .flex()
            .items_center()
            .gap_2();

        if let Some(label) = &self.busy {
            return bar.child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().accent))
                    .child(label.clone()),
            );
        }

        let Some(network) = network else {
            return bar;
        };
        let name = network.name.clone();

        bar.child(pill_button(
            "network-delete",
            "Delete",
            true,
            theme().red,
            theme().red_bg,
            cx.listener(move |this, _, window, cx| {
                this.open_modal(Modal::DeleteNetwork { name: name.clone() }, window, cx)
            }),
        ))
    }
}

fn stats_chart(title: &str, value: String, values: &[f64], max: f64, color: u32) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(section_title(title))
                .child(div().text_xs().text_color(rgb(theme().text)).child(value)),
        )
        .child(graph_bars(values, max, color))
}

/// A compact bar sparkline; one 2px bar per sample, newest on the right.
fn graph_bars(values: &[f64], max: f64, color: u32) -> Div {
    const HEIGHT: f32 = 56.0;
    let mut graph = div()
        .flex()
        .items_end()
        .justify_end()
        .w_full()
        .h(px(HEIGHT))
        .gap(px(1.));
    for value in values {
        let ratio = if max > 0.0 {
            (value / max).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let height = (ratio as f32 * HEIGHT).max(1.0);
        graph = graph.child(div().w(px(2.)).h(px(height)).rounded_sm().bg(rgb(color)));
    }

    div()
        .w_full()
        .rounded_sm()
        .bg(rgb(theme().panel_bg))
        .px_2()
        .py_1()
        .child(graph)
}
