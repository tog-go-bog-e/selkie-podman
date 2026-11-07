use super::helpers::*;
use super::*;

impl PodmanDesktop {
    pub(super) fn table_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        match self.tab {
            Tab::Containers => self.container_row(self.visible_indices[ix], cx),
            Tab::Compose => self.compose_row(self.visible_indices[ix], cx),
            Tab::Images => self.image_row(self.visible_indices[ix], cx),
            Tab::Pods => self.pod_row(self.visible_indices[ix], cx),
            Tab::Volumes => self.volume_row(self.visible_indices[ix], cx),
            Tab::Networks => self.network_row(self.visible_indices[ix], cx),
            Tab::Machines => self.machine_row(self.visible_indices[ix], cx),
            Tab::Connections => self.connection_row(self.visible_indices[ix], cx),
            Tab::Dashboard | Tab::Accounts | Tab::Settings => div().into_any_element(),
        }
    }

    fn machine_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let machine = &self.local.machines[ix];
        let selected =
            matches!(&self.selection, Some(Selection::Machine(name)) if name == &machine.name);
        let compact = self.selection.is_some();
        let fixed_widths = if compact {
            110.0 + 80.0
        } else {
            110.0 + 80.0 + 100.0 + 110.0 + 170.0
        };
        let name_width = (self.list_content_width() - fixed_widths).max(80.0);
        let name = machine.name.clone();
        let menu_name = machine.name.clone();
        let group = SharedString::from(format!("machine-row-{ix}"));
        let (color, _) = state_colors(if machine.running { "running" } else { "exited" });

        let mut row = div()
            .group(group)
            .id(ix)
            .relative()
            .flex()
            .items_center()
            .w_full()
            .h(px(ROW_HEIGHT))
            .px_4()
            .bg(rgb(theme().bg))
            .border_b_1()
            .border_color(rgb(theme().border))
            .cursor_pointer()
            .when(selected, |el| el.bg(rgb(theme().accent_soft)))
            .when(!selected, |el| {
                el.hover(|style| style.bg(rgb(theme().hover_bg)))
            })
            .on_click(cx.listener(move |this, _, _, cx| this.select_machine(name.clone(), cx)))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.open_context_menu(
                        event.position,
                        Selection::Machine(menu_name.clone()),
                        window,
                        cx,
                    )
                }),
            )
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
                            .whitespace_nowrap()
                            .truncate()
                            .child(truncate_sm(&machine.name, name_width)),
                    )
                    .child(
                        div()
                            .w_full()
                            .text_xs()
                            .text_color(rgb(theme().faint))
                            .whitespace_nowrap()
                            .truncate()
                            .child(truncate_xs(
                                &if machine.default {
                                    format!("{} · default", machine.vm_type)
                                } else {
                                    machine.vm_type.clone()
                                },
                                name_width,
                            )),
                    ),
            )
            .child(cell(machine.state_label(), 110.).text_color(rgb(color)))
            .child(cell(machine.cpus.to_string(), 80.));

        if !compact {
            row = row
                .child(cell(machine.memory_label(), 100.))
                .child(cell(machine.disk_label(), 110.))
                .child(muted_cell(machine.last_up.clone(), 170.));
        }

        row.child(self.machine_row_actions(ix, machine, cx))
            .into_any_element()
    }

    fn connection_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let connection = &self.local.connections[ix];
        let selected = matches!(&self.selection, Some(Selection::Connection(name)) if name == &connection.name);
        let compact = self.selection.is_some();
        let fixed_widths = if compact { 120.0 } else { 220.0 + 120.0 };
        let flex_width = ((self.list_content_width() - fixed_widths) / 2.0).max(60.0);
        let name_width = flex_width;
        let uri_width = flex_width;
        let name = connection.name.clone();
        let menu_name = connection.name.clone();
        let group = SharedString::from(format!("connection-row-{ix}"));
        let in_use = self.settings.connection.as_deref() == Some(connection.name.as_str());
        let status = match (in_use, connection.default) {
            (true, true) => "in use · default",
            (true, false) => "in use",
            (false, true) => "default",
            (false, false) => "-",
        };
        let status_color = if in_use {
            theme().accent
        } else {
            theme().muted
        };

        let mut row = div()
            .group(group)
            .id(ix)
            .relative()
            .flex()
            .items_center()
            .w_full()
            .h(px(ROW_HEIGHT))
            .px_4()
            .bg(rgb(theme().bg))
            .border_b_1()
            .border_color(rgb(theme().border))
            .cursor_pointer()
            .when(selected, |el| el.bg(rgb(theme().accent_soft)))
            .when(!selected, |el| {
                el.hover(|style| style.bg(rgb(theme().hover_bg)))
            })
            .on_click(cx.listener(move |this, _, _, cx| this.select_connection(name.clone(), cx)))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.open_context_menu(
                        event.position,
                        Selection::Connection(menu_name.clone()),
                        window,
                        cx,
                    )
                }),
            )
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
                            .whitespace_nowrap()
                            .truncate()
                            .child(truncate_sm(&connection.name, name_width)),
                    )
                    .child(
                        div()
                            .w_full()
                            .text_xs()
                            .text_color(rgb(theme().faint))
                            .whitespace_nowrap()
                            .truncate()
                            .child(truncate_xs(&connection.identity_label(), name_width)),
                    ),
            );

        if !compact {
            row = row
                .child(
                    div().flex_1().min_w(px(0.)).overflow_hidden().child(
                        div()
                            .w_full()
                            .text_sm()
                            .whitespace_nowrap()
                            .truncate()
                            .child(truncate_sm(&connection.uri, uri_width)),
                    ),
                )
                .child(muted_cell(connection.identity_label(), 220.));
        }

        row.child(cell(status, 120.).text_color(rgb(status_color)))
            .child(self.connection_row_actions(ix, connection, in_use, cx))
            .into_any_element()
    }

    fn compose_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let project = &self.snapshot.compose[ix];
        let selected =
            matches!(&self.selection, Some(Selection::Compose(name)) if name == &project.name);
        let compact = self.selection.is_some();
        let fixed_widths = if compact {
            120.0 + 100.0
        } else {
            120.0 + 100.0 + 220.0
        };
        let name_width = (self.list_content_width() - fixed_widths).max(80.0);
        let project_name = project.name.clone();
        let menu_name = project.name.clone();
        let group = SharedString::from(format!("compose-row-{ix}"));
        let status_color = if project.running == project.total {
            state_colors("running").0
        } else if project.running == 0 {
            state_colors("exited").0
        } else {
            state_colors("paused").0
        };

        let mut row = div()
            .group(group)
            .id(ix)
            .relative()
            .flex()
            .items_center()
            .w_full()
            .h(px(ROW_HEIGHT))
            .px_4()
            .bg(rgb(theme().bg))
            .border_b_1()
            .border_color(rgb(theme().border))
            .cursor_pointer()
            .when(selected, |el| el.bg(rgb(theme().accent_soft)))
            .when(!selected, |el| {
                el.hover(|style| style.bg(rgb(theme().hover_bg)))
            })
            .on_click(
                cx.listener(move |this, _, _, cx| this.select_compose(project_name.clone(), cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.open_context_menu(
                        event.position,
                        Selection::Compose(menu_name.clone()),
                        window,
                        cx,
                    )
                }),
            )
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
                            .whitespace_nowrap()
                            .truncate()
                            .child(truncate_sm(&project.name, name_width)),
                    )
                    .child(
                        div()
                            .w_full()
                            .text_xs()
                            .text_color(rgb(theme().faint))
                            .whitespace_nowrap()
                            .truncate()
                            .child(truncate_xs(
                                &format!(
                                    "{} · {}",
                                    project.config_label(),
                                    project.working_dir.as_deref().unwrap_or("no path")
                                ),
                                name_width,
                            )),
                    ),
            )
            .child(cell(project.status_label(), 120.).text_color(rgb(status_color)))
            .child(cell(project.services.len().to_string(), 100.));

        if !compact {
            row = row.child(muted_cell(project.config_label(), 220.));
        }

        row.child(self.compose_row_actions(ix, project, cx))
            .into_any_element()
    }

    fn image_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let image = &self.snapshot.images[ix];
        let selected = matches!(&self.selection, Some(Selection::Image(id)) if id == &image.id);
        let compact = self.selection.is_some();
        let fixed_widths = if compact {
            90.0 + 104.0
        } else {
            104.0 + 90.0 + 104.0 + 150.0
        };
        let name_width = (self.list_content_width() - fixed_widths).max(80.0);
        let image_id = image.id.clone();
        let menu_id = image.id.clone();
        let group = SharedString::from(format!("img-row-{ix}"));

        let mut row = div()
            .group(group)
            .id(ix)
            .relative()
            .flex()
            .items_center()
            .w_full()
            .h(px(ROW_HEIGHT))
            .px_4()
            .bg(rgb(theme().bg))
            .border_b_1()
            .border_color(rgb(theme().border))
            .cursor_pointer()
            .when(selected, |el| el.bg(rgb(theme().accent_soft)))
            .when(!selected, |el| {
                el.hover(|style| style.bg(rgb(theme().hover_bg)))
            })
            .on_click(cx.listener(move |this, _, _, cx| this.select_image(image_id.clone(), cx)))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.open_context_menu(
                        event.position,
                        Selection::Image(menu_id.clone()),
                        window,
                        cx,
                    )
                }),
            )
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
                            .whitespace_nowrap()
                            .truncate()
                            .child(truncate_sm(&image.name, name_width)),
                    )
                    .child(
                        div()
                            .w_full()
                            .text_xs()
                            .text_color(rgb(theme().faint))
                            .child(truncate_xs(&image.digest_short(), name_width)),
                    ),
            );

        if !compact {
            row = row.child(muted_cell(image.id_short(), 104.));
        }
        row = row
            .child(cell(image.size_label(), 90.))
            .child(cell(image.containers.to_string(), 104.));
        if !compact {
            row = row.child(muted_cell(image.created.clone(), 150.));
        }
        row = row.child(self.image_row_actions(ix, image, cx));
        row.into_any_element()
    }

    fn container_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let container = &self.snapshot.containers[ix];
        let (color, color_bg) = state_colors(&container.state);
        let selected =
            matches!(&self.selection, Some(Selection::Container(id)) if id == &container.id);
        let compact = self.selection.is_some();
        let fixed_widths = if compact {
            104.0 + 140.0
        } else {
            104.0 + 140.0 + 160.0 + 104.0
        };
        let flex_width = ((self.list_content_width() - fixed_widths) / 2.0).max(40.0);
        let name_width = flex_width;
        let image_width = flex_width;
        let container_id = container.id.clone();
        let menu_id = container.id.clone();
        let group = SharedString::from(format!("row-{ix}"));
        let subtitle = match &container.compose {
            Some(compose) => match &compose.service {
                Some(service) => {
                    format!("{} · {}/{}", container.id_short(), compose.project, service)
                }
                None => format!("{} · {}", container.id_short(), compose.project),
            },
            None => container.id_short(),
        };

        let mut row = div()
            .group(group)
            .id(ix)
            .relative()
            .flex()
            .items_center()
            .w_full()
            .h(px(ROW_HEIGHT))
            .px_4()
            .bg(rgb(theme().bg))
            .border_b_1()
            .border_color(rgb(theme().border))
            .cursor_pointer()
            .when(selected, |el| el.bg(rgb(theme().accent_soft)))
            .when(!selected, |el| {
                el.hover(|style| style.bg(rgb(theme().hover_bg)))
            })
            .on_click(
                cx.listener(move |this, _, _, cx| this.select_container(container_id.clone(), cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.open_context_menu(
                        event.position,
                        Selection::Container(menu_id.clone()),
                        window,
                        cx,
                    )
                }),
            )
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
                            .whitespace_nowrap()
                            .truncate()
                            .child(truncate_sm(&container.name, name_width)),
                    )
                    .child(
                        div()
                            .w_full()
                            .text_xs()
                            .text_color(rgb(theme().faint))
                            .whitespace_nowrap()
                            .truncate()
                            .child(truncate_xs(&subtitle, name_width)),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_sm()
                    .text_color(rgb(theme().muted))
                    .whitespace_nowrap()
                    .truncate()
                    .child(truncate_sm(&container.image, image_width)),
            )
            .child(
                div()
                    .w(px(104.))
                    .child(state_pill(&container.state, color, color_bg)),
            )
            .child(muted_cell(ports_summary(&container.ports), 140.));

        if !compact {
            row = row
                .child(muted_cell(container.status.clone(), 160.))
                .child(muted_cell(container.id_short(), 104.));
        }

        row = row.child(self.container_row_actions(ix, container, cx));
        row.into_any_element()
    }

    fn pod_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let pod = &self.snapshot.pods[ix];
        let (color, color_bg) = state_colors(&pod.status);
        let selected = matches!(&self.selection, Some(Selection::Pod(id)) if id == &pod.id);
        let compact = self.selection.is_some();
        let fixed_widths = if compact {
            110.0 + 110.0
        } else {
            110.0 + 110.0 + 150.0 + 110.0
        };
        let name_width = (self.list_content_width() - fixed_widths).max(80.0);
        let pod_id = pod.id.clone();
        let menu_id = pod.id.clone();
        let group = SharedString::from(format!("pod-row-{ix}"));

        let mut row = div()
            .group(group)
            .id(ix)
            .relative()
            .flex()
            .items_center()
            .w_full()
            .h(px(ROW_HEIGHT))
            .px_4()
            .bg(rgb(theme().bg))
            .border_b_1()
            .border_color(rgb(theme().border))
            .cursor_pointer()
            .when(selected, |el| el.bg(rgb(theme().accent_soft)))
            .when(!selected, |el| {
                el.hover(|style| style.bg(rgb(theme().hover_bg)))
            })
            .on_click(cx.listener(move |this, _, _, cx| this.select_pod(pod_id.clone(), cx)))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.open_context_menu(
                        event.position,
                        Selection::Pod(menu_id.clone()),
                        window,
                        cx,
                    )
                }),
            )
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
                            .whitespace_nowrap()
                            .truncate()
                            .child(truncate_sm(&pod.name, name_width)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(theme().faint))
                            .child(truncate_xs(&pod.id_short(), name_width)),
                    ),
            )
            .child(
                div()
                    .w(px(110.))
                    .child(state_pill(&pod.status, color, color_bg)),
            )
            .child(cell(pod.containers.to_string(), 110.));

        if !compact {
            row = row
                .child(muted_cell(pod.created.clone(), 150.))
                .child(muted_cell(pod.id_short(), 110.));
        }

        row = row.child(self.pod_row_actions(ix, pod, cx));
        row.into_any_element()
    }

    fn volume_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let volume = &self.snapshot.volumes[ix];
        let selected =
            matches!(&self.selection, Some(Selection::Volume(name)) if name == &volume.name);
        let compact = self.selection.is_some();
        let fixed_widths = if compact {
            100.0 + 90.0
        } else {
            100.0 + 90.0 + 90.0 + 150.0
        };
        let name_width = (self.list_content_width() - fixed_widths).max(80.0);
        let volume_name = volume.name.clone();
        let menu_name = volume.name.clone();
        let group = SharedString::from(format!("volume-row-{ix}"));

        let mut row = div()
            .group(group)
            .id(ix)
            .relative()
            .flex()
            .items_center()
            .w_full()
            .h(px(ROW_HEIGHT))
            .px_4()
            .bg(rgb(theme().bg))
            .border_b_1()
            .border_color(rgb(theme().border))
            .cursor_pointer()
            .when(selected, |el| el.bg(rgb(theme().accent_soft)))
            .when(!selected, |el| {
                el.hover(|style| style.bg(rgb(theme().hover_bg)))
            })
            .on_click(
                cx.listener(move |this, _, _, cx| this.select_volume(volume_name.clone(), cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.open_context_menu(
                        event.position,
                        Selection::Volume(menu_name.clone()),
                        window,
                        cx,
                    )
                }),
            )
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
                            .whitespace_nowrap()
                            .truncate()
                            .child(truncate_sm(&volume.name, name_width)),
                    )
                    .child(
                        div()
                            .w_full()
                            .text_xs()
                            .text_color(rgb(theme().faint))
                            .whitespace_nowrap()
                            .truncate()
                            .child(truncate_xs(&volume.mountpoint, name_width)),
                    ),
            )
            .child(cell(volume.driver.clone(), 100.))
            .child(cell(volume.mounts.to_string(), 90.));

        if !compact {
            row = row
                .child(muted_cell(volume.scope.clone(), 90.))
                .child(muted_cell(volume.created.clone(), 150.));
        }

        row = row.child(self.volume_row_actions(ix, volume, cx));
        row.into_any_element()
    }

    fn network_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let network = &self.snapshot.networks[ix];
        let selected =
            matches!(&self.selection, Some(Selection::Network(name)) if name == &network.name);
        let compact = self.selection.is_some();
        let fixed_widths = if compact {
            100.0 + 170.0
        } else {
            100.0 + 170.0 + 110.0 + 80.0
        };
        let name_width = (self.list_content_width() - fixed_widths).max(80.0);
        let network_name = network.name.clone();
        let menu_name = network.name.clone();
        let group = SharedString::from(format!("network-row-{ix}"));

        let mut row = div()
            .group(group)
            .id(ix)
            .relative()
            .flex()
            .items_center()
            .w_full()
            .h(px(ROW_HEIGHT))
            .px_4()
            .bg(rgb(theme().bg))
            .border_b_1()
            .border_color(rgb(theme().border))
            .cursor_pointer()
            .when(selected, |el| el.bg(rgb(theme().accent_soft)))
            .when(!selected, |el| {
                el.hover(|style| style.bg(rgb(theme().hover_bg)))
            })
            .on_click(
                cx.listener(move |this, _, _, cx| this.select_network(network_name.clone(), cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.open_context_menu(
                        event.position,
                        Selection::Network(menu_name.clone()),
                        window,
                        cx,
                    )
                }),
            )
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
                            .whitespace_nowrap()
                            .truncate()
                            .child(truncate_sm(&network.name, name_width)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(theme().faint))
                            .child(truncate_xs(&network.id_short(), name_width)),
                    ),
            )
            .child(cell(network.driver.clone(), 100.))
            .child(muted_cell(network.subnet_label(), 170.));

        if !compact {
            row = row
                .child(muted_cell(network.interface.clone(), 110.))
                .child(cell(bool_label(network.dns), 80.));
        }

        row = row.child(self.network_row_actions(ix, network, cx));
        row.into_any_element()
    }

    fn container_row_actions(
        &self,
        ix: usize,
        container: &Container,
        cx: &mut Context<Self>,
    ) -> Div {
        let group = SharedString::from(format!("row-{ix}"));
        let busy = self.busy.is_some();
        let running = container.state == "running";
        let paused = container.state == "paused";
        let id = container.id.clone();
        let name = container.name.clone();

        let mut actions = div()
            .invisible()
            .group_hover(group, |style| style.visible())
            .absolute()
            .right_4()
            .top_0()
            .bottom_0()
            .flex()
            .items_center()
            .gap_1()
            .px_1()
            .rounded_md()
            .bg(rgb(theme().panel_bg))
            .border_1()
            .border_color(rgb(theme().border));

        if running {
            let stop_id = id.clone();
            actions = actions.child(row_action_button(
                SharedString::from(format!("row-{ix}-stop")),
                "Stop",
                !busy,
                theme().yellow,
                theme().yellow_bg,
                cx.listener(move |this, _, _window, cx| {
                    cx.stop_propagation();
                    this.container_action(stop_id.clone(), ContainerAction::Stop, cx);
                }),
            ));
            let pause_id = id.clone();
            actions = actions.child(row_action_button(
                SharedString::from(format!("row-{ix}-pause")),
                "Pause",
                !busy,
                theme().yellow,
                theme().yellow_bg,
                cx.listener(move |this, _, _window, cx| {
                    cx.stop_propagation();
                    this.container_action(pause_id.clone(), ContainerAction::Pause, cx);
                }),
            ));
            let restart_id = id.clone();
            actions = actions.child(row_action_button(
                SharedString::from(format!("row-{ix}-restart")),
                "Restart",
                !busy,
                theme().blue,
                theme().blue_bg,
                cx.listener(move |this, _, _window, cx| {
                    cx.stop_propagation();
                    this.container_action(restart_id.clone(), ContainerAction::Restart, cx);
                }),
            ));
        } else if paused {
            let unpause_id = id.clone();
            actions = actions.child(row_action_button(
                SharedString::from(format!("row-{ix}-unpause")),
                "Resume",
                !busy,
                theme().green,
                theme().green_bg,
                cx.listener(move |this, _, _window, cx| {
                    cx.stop_propagation();
                    this.container_action(unpause_id.clone(), ContainerAction::Unpause, cx);
                }),
            ));
            let stop_id = id.clone();
            actions = actions.child(row_action_button(
                SharedString::from(format!("row-{ix}-stop")),
                "Stop",
                !busy,
                theme().yellow,
                theme().yellow_bg,
                cx.listener(move |this, _, _window, cx| {
                    cx.stop_propagation();
                    this.container_action(stop_id.clone(), ContainerAction::Stop, cx);
                }),
            ));
        } else {
            let start_id = id.clone();
            actions = actions.child(row_action_button(
                SharedString::from(format!("row-{ix}-start")),
                "Start",
                !busy,
                theme().green,
                theme().green_bg,
                cx.listener(move |this, _, _window, cx| {
                    cx.stop_propagation();
                    this.container_action(start_id.clone(), ContainerAction::Start, cx);
                }),
            ));
        }

        let delete_id = id.clone();
        let delete_name = name;
        actions.child(row_action_button(
            SharedString::from(format!("row-{ix}-delete")),
            "Delete",
            !busy,
            theme().red,
            theme().red_bg,
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.open_modal(
                    Modal::DeleteContainer {
                        id: delete_id.clone(),
                        name: delete_name.clone(),
                    },
                    window,
                    cx,
                );
            }),
        ))
    }

    fn image_row_actions(&self, ix: usize, image: &Image, cx: &mut Context<Self>) -> Div {
        let group = SharedString::from(format!("img-row-{ix}"));
        let busy = self.busy.is_some();
        let id = image.id.clone();
        let name = image.name.clone();

        let mut actions = div()
            .invisible()
            .group_hover(group, |style| style.visible())
            .absolute()
            .right_4()
            .top_0()
            .bottom_0()
            .flex()
            .items_center()
            .gap_1()
            .px_1()
            .rounded_md()
            .bg(rgb(theme().panel_bg))
            .border_1()
            .border_color(rgb(theme().border));

        let tag_id = id.clone();
        let tag_source = name.clone();
        let run_image = image.name.clone();
        actions = actions.child(row_action_button(
            SharedString::from(format!("img-row-{ix}-run")),
            "Run",
            !busy,
            theme().green,
            theme().green_bg,
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.open_modal(Modal::run_container(run_image.clone()), window, cx);
            }),
        ));
        let push_image = image.name.clone();
        actions = actions.child(row_action_button(
            SharedString::from(format!("img-row-{ix}-push")),
            "Push",
            !busy && self.output_run.is_none(),
            theme().blue,
            theme().blue_bg,
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.open_modal(
                    Modal::PushImage {
                        destination: push_image.clone(),
                        image: push_image.clone(),
                    },
                    window,
                    cx,
                );
            }),
        ));
        actions = actions.child(row_action_button(
            SharedString::from(format!("img-row-{ix}-tag")),
            "Tag",
            !busy,
            theme().accent,
            theme().accent_soft,
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.open_modal(
                    Modal::TagImage {
                        id: tag_id.clone(),
                        source: tag_source.clone(),
                        input: String::new(),
                    },
                    window,
                    cx,
                );
            }),
        ));

        actions.child(row_action_button(
            SharedString::from(format!("img-row-{ix}-delete")),
            "Delete",
            !busy,
            theme().red,
            theme().red_bg,
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.open_modal(
                    Modal::DeleteImage {
                        id: id.clone(),
                        name: name.clone(),
                    },
                    window,
                    cx,
                );
            }),
        ))
    }

    fn pod_row_actions(&self, ix: usize, pod: &Pod, cx: &mut Context<Self>) -> Div {
        let group = SharedString::from(format!("pod-row-{ix}"));
        let busy = self.busy.is_some();
        let running = pod.status == "Running";
        let id = pod.id.clone();
        let name = pod.name.clone();

        let mut actions = div()
            .invisible()
            .group_hover(group, |style| style.visible())
            .absolute()
            .right_4()
            .top_0()
            .bottom_0()
            .flex()
            .items_center()
            .gap_1()
            .px_1()
            .rounded_md()
            .bg(rgb(theme().panel_bg))
            .border_1()
            .border_color(rgb(theme().border));

        if running {
            let stop_id = id.clone();
            actions = actions.child(row_action_button(
                SharedString::from(format!("pod-row-{ix}-stop")),
                "Stop",
                !busy,
                theme().yellow,
                theme().yellow_bg,
                cx.listener(move |this, _, _window, cx| {
                    cx.stop_propagation();
                    this.pod_action(stop_id.clone(), PodAction::Stop, cx);
                }),
            ));
            let restart_id = id.clone();
            actions = actions.child(row_action_button(
                SharedString::from(format!("pod-row-{ix}-restart")),
                "Restart",
                !busy,
                theme().blue,
                theme().blue_bg,
                cx.listener(move |this, _, _window, cx| {
                    cx.stop_propagation();
                    this.pod_action(restart_id.clone(), PodAction::Restart, cx);
                }),
            ));
        } else {
            let start_id = id.clone();
            actions = actions.child(row_action_button(
                SharedString::from(format!("pod-row-{ix}-start")),
                "Start",
                !busy,
                theme().green,
                theme().green_bg,
                cx.listener(move |this, _, _window, cx| {
                    cx.stop_propagation();
                    this.pod_action(start_id.clone(), PodAction::Start, cx);
                }),
            ));
        }

        let delete_id = id.clone();
        actions.child(row_action_button(
            SharedString::from(format!("pod-row-{ix}-delete")),
            "Delete",
            !busy,
            theme().red,
            theme().red_bg,
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.open_modal(
                    Modal::DeletePod {
                        id: delete_id.clone(),
                        name: name.clone(),
                    },
                    window,
                    cx,
                );
            }),
        ))
    }

    fn volume_row_actions(&self, ix: usize, volume: &Volume, cx: &mut Context<Self>) -> Div {
        let group = SharedString::from(format!("volume-row-{ix}"));
        let busy = self.busy.is_some();
        let name = volume.name.clone();

        div()
            .invisible()
            .group_hover(group, |style| style.visible())
            .absolute()
            .right_4()
            .top_0()
            .bottom_0()
            .flex()
            .items_center()
            .px_1()
            .rounded_md()
            .bg(rgb(theme().panel_bg))
            .border_1()
            .border_color(rgb(theme().border))
            .child(row_action_button(
                SharedString::from(format!("volume-row-{ix}-delete")),
                "Delete",
                !busy,
                theme().red,
                theme().red_bg,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.open_modal(Modal::DeleteVolume { name: name.clone() }, window, cx);
                }),
            ))
    }

    fn network_row_actions(&self, ix: usize, network: &Network, cx: &mut Context<Self>) -> Div {
        let group = SharedString::from(format!("network-row-{ix}"));
        let busy = self.busy.is_some();
        let name = network.name.clone();

        div()
            .invisible()
            .group_hover(group, |style| style.visible())
            .absolute()
            .right_4()
            .top_0()
            .bottom_0()
            .flex()
            .items_center()
            .px_1()
            .rounded_md()
            .bg(rgb(theme().panel_bg))
            .border_1()
            .border_color(rgb(theme().border))
            .child(row_action_button(
                SharedString::from(format!("network-row-{ix}-delete")),
                "Delete",
                !busy,
                theme().red,
                theme().red_bg,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.open_modal(Modal::DeleteNetwork { name: name.clone() }, window, cx);
                }),
            ))
    }

    fn compose_row_actions(
        &self,
        ix: usize,
        project: &ComposeProject,
        cx: &mut Context<Self>,
    ) -> Div {
        let group = SharedString::from(format!("compose-row-{ix}"));
        let available = self.output_run.is_none() && self.busy.is_none();
        let up_project = project.clone();
        let down_project = project.clone();

        div()
            .invisible()
            .group_hover(group, |style| style.visible())
            .absolute()
            .right_4()
            .top_0()
            .bottom_0()
            .flex()
            .items_center()
            .gap_1()
            .px_1()
            .rounded_md()
            .bg(rgb(theme().panel_bg))
            .border_1()
            .border_color(rgb(theme().border))
            .child(row_action_button(
                SharedString::from(format!("compose-row-{ix}-up")),
                "Up",
                available,
                theme().accent,
                theme().accent_soft,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.start_compose(&up_project, ComposeAction::Up, cx);
                }),
            ))
            .child(row_action_button(
                SharedString::from(format!("compose-row-{ix}-down")),
                "Down",
                available,
                theme().red,
                theme().red_bg,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.open_modal(
                        Modal::ComposeDown {
                            project: down_project.clone(),
                        },
                        window,
                        cx,
                    );
                }),
            ))
    }

    fn machine_row_actions(&self, ix: usize, machine: &Machine, cx: &mut Context<Self>) -> Div {
        let group = SharedString::from(format!("machine-row-{ix}"));
        let busy = self.busy.is_some();
        let stop_name = machine.name.clone();
        let start_name = machine.name.clone();

        div()
            .invisible()
            .group_hover(group, |style| style.visible())
            .absolute()
            .right_4()
            .top_0()
            .bottom_0()
            .flex()
            .items_center()
            .px_1()
            .rounded_md()
            .bg(rgb(theme().panel_bg))
            .border_1()
            .border_color(rgb(theme().border))
            .when(machine.running, |el| {
                el.child(row_action_button(
                    SharedString::from(format!("machine-row-{ix}-stop")),
                    "Stop",
                    !busy,
                    theme().red,
                    theme().red_bg,
                    cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.open_modal(
                            Modal::StopMachine {
                                name: stop_name.clone(),
                            },
                            window,
                            cx,
                        );
                    }),
                ))
            })
            .when(!machine.running, |el| {
                el.child(row_action_button(
                    SharedString::from(format!("machine-row-{ix}-start")),
                    "Start",
                    !busy,
                    theme().accent,
                    theme().accent_soft,
                    cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.machine_action(start_name.clone(), MachineAction::Start, cx);
                    }),
                ))
            })
    }

    fn connection_row_actions(
        &self,
        ix: usize,
        connection: &Connection,
        in_use: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let group = SharedString::from(format!("connection-row-{ix}"));
        let available = self.busy.is_none() && !in_use;
        let name = connection.name.clone();

        div()
            .invisible()
            .group_hover(group, |style| style.visible())
            .absolute()
            .right_4()
            .top_0()
            .bottom_0()
            .flex()
            .items_center()
            .px_1()
            .rounded_md()
            .bg(rgb(theme().panel_bg))
            .border_1()
            .border_color(rgb(theme().border))
            .child(row_action_button(
                SharedString::from(format!("connection-row-{ix}-use")),
                "Use",
                available,
                theme().accent,
                theme().accent_soft,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.use_connection(Some(name.clone()), cx);
                }),
            ))
    }
}
