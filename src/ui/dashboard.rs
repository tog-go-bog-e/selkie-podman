use super::helpers::*;
use super::*;

impl Selkie {
    // -----------------------------------------------------------------------
    // Dashboard
    // -----------------------------------------------------------------------

    pub(super) fn dashboard_view(&self) -> AnyElement {
        let mut content = div()
            .id("dashboard")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .px_6()
            .pb_6()
            .flex()
            .flex_col()
            .gap_4();

        let running_containers = self
            .snapshot
            .containers
            .iter()
            .filter(|item| item.state == "running")
            .count();
        let running_pods = self
            .snapshot
            .pods
            .iter()
            .filter(|item| item.status == "Running")
            .count();
        let mounted_volumes = self
            .snapshot
            .volumes
            .iter()
            .filter(|item| item.mounts > 0)
            .count();
        let running_machines = self
            .local
            .machines
            .iter()
            .filter(|item| item.running)
            .count();

        let engine = podman::engine();
        let mut resources = vec![
            stat_card(
                "Containers",
                self.snapshot.containers.len().to_string(),
                format!("{running_containers} running"),
            ),
            stat_card(
                "Images",
                self.snapshot.images.len().to_string(),
                "in local storage".to_string(),
            ),
        ];
        if engine.has_pods() {
            resources.push(stat_card(
                "Pods",
                self.snapshot.pods.len().to_string(),
                format!("{running_pods} running"),
            ));
        }
        let mut storage = vec![
            stat_card(
                "Volumes",
                self.snapshot.volumes.len().to_string(),
                format!("{mounted_volumes} in use"),
            ),
            stat_card(
                "Networks",
                self.snapshot.networks.len().to_string(),
                "configured".to_string(),
            ),
        ];
        if engine.has_machines() {
            storage.push(stat_card(
                "Machines",
                self.local.machines.len().to_string(),
                format!("{running_machines} running"),
            ));
        }

        content = content
            .child(section_title("RESOURCES"))
            .child(card_row(resources))
            .child(card_row(storage));

        content = content.child(section_title("DISK USAGE"));
        match &self.dashboard {
            Some(data) if !data.disk.is_empty() => {
                content = content.child(card_row(
                    data.disk.iter().map(disk_card).collect::<Vec<_>>(),
                ));
            }
            Some(_) => {
                content = content.child(muted_text(format!(
                    "{} reported no disk usage.",
                    podman::engine().label()
                )));
            }
            None if self.loading => {
                content = content.child(muted_text("Measuring disk usage…"));
            }
            None => {
                content = content.child(muted_text("Disk usage is not available yet."));
            }
        }
        if let Some(error) = &self.dashboard_error {
            content = content.child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().red))
                    .child(error.clone()),
            );
        }

        content = content.child(section_title("ENGINE"));
        if let Some(data) = &self.dashboard {
            let info = &data.info;
            let connection = match (podman::engine(), &self.settings.connection) {
                (EngineKind::Docker, _) => "Docker API".to_string(),
                (EngineKind::PodmanApi, _) => "Podman API".to_string(),
                (EngineKind::Podman, Some(name)) => name.clone(),
                (EngineKind::Podman, None) => "Podman default".to_string(),
            };
            let socket = if info.socket_path.is_empty() {
                "-".to_string()
            } else {
                format!(
                    "{} ({})",
                    info.socket_path,
                    if info.socket_present {
                        "present"
                    } else {
                        "missing"
                    }
                )
            };

            content = content.child(
                div()
                    .rounded_lg()
                    .bg(rgb(theme().panel_bg))
                    .border_1()
                    .border_color(rgb(theme().border))
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(engine_row(
                        ("Version", &info.version),
                        ("OS / arch", &info.os_arch),
                    ))
                    .child(engine_row(
                        ("Host", &info.hostname),
                        ("Kernel", &info.kernel),
                    ))
                    .child(engine_row(
                        ("CPUs", &info.cpus.to_string()),
                        ("Memory", &info.memory_label()),
                    ))
                    .child(engine_row(
                        ("Runtime", &info.runtime),
                        ("Network", &info.network_backend),
                    ))
                    .child(engine_row(
                        ("Storage", &info.storage_driver),
                        ("Event log", &info.event_logger),
                    ))
                    .child(engine_row(
                        ("Mode", info.mode_label()),
                        ("Connection", &connection),
                    ))
                    .child(detail_row("Socket", &socket))
                    .child(detail_row("Graph root", &info.graph_root)),
            );
        } else if let Some(error) = &self.error {
            content = content.child(muted_text(format!("Engine details unavailable: {error}")));
        } else {
            content = content.child(muted_text("Loading engine details…"));
        }

        content.into_any_element()
    }
}

fn card_row(cards: Vec<Div>) -> Div {
    let mut row = div().flex().items_start().gap_3();
    for card in cards {
        row = row.child(card);
    }
    row
}

fn stat_card(label: &str, value: String, detail: String) -> Div {
    div()
        .flex_1()
        .min_w(px(0.))
        .rounded_lg()
        .bg(rgb(theme().panel_bg))
        .border_1()
        .border_color(rgb(theme().border))
        .p_4()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_xs()
                .text_color(rgb(theme().faint))
                .child(label.to_string()),
        )
        .child(
            div()
                .text_xl()
                .font_weight(FontWeight::SEMIBOLD)
                .child(value),
        )
        .child(div().text_xs().text_color(rgb(theme().muted)).child(detail))
}

fn disk_card(entry: &podman::DiskUsage) -> Div {
    let percent = entry.reclaimable_percent();
    let mut card = div()
        .flex_1()
        .min_w(px(0.))
        .rounded_lg()
        .bg(rgb(theme().panel_bg))
        .border_1()
        .border_color(rgb(theme().border))
        .p_4()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_xs()
                .text_color(rgb(theme().faint))
                .child(entry.kind.clone()),
        )
        .child(
            div()
                .text_lg()
                .font_weight(FontWeight::SEMIBOLD)
                .child(entry.size_label()),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(theme().muted))
                .child(entry.reclaimable_label()),
        );

    if percent > 0 {
        let color = if percent >= 50 {
            theme().red
        } else {
            theme().accent
        };
        card = card.child(
            div()
                .mt_1()
                .h(px(6.))
                .w(px(140.))
                .rounded_full()
                .bg(rgb(theme().border))
                .child(
                    div()
                        .h_full()
                        .w(px(140.0 * percent as f32 / 100.0))
                        .rounded_full()
                        .bg(rgb(color)),
                ),
        );
    }

    card
}

fn engine_row(left: (&str, &str), right: (&str, &str)) -> Div {
    div()
        .flex()
        .items_start()
        .gap_4()
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .child(detail_row(left.0, left.1)),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .child(detail_row(right.0, right.1)),
        )
}
