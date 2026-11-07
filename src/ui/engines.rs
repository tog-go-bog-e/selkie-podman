use super::helpers::*;
use super::*;

impl PodmanDesktop {
    // -----------------------------------------------------------------------
    // Machines and connections
    // -----------------------------------------------------------------------

    /// Probe the configured connections (`podman --connection <name> info`).
    ///
    /// With `all`, every connection is probed again; otherwise only the ones
    /// without a result yet (a new connection, or the first visit to the tab).
    pub(super) fn check_connections(&mut self, all: bool, cx: &mut Context<Self>) {
        let names: Vec<String> = self
            .local
            .connections
            .iter()
            .map(|connection| connection.name.clone())
            .collect();
        self.connection_checks
            .retain(|name, _| names.iter().any(|known| known == name));
        let pending: Vec<String> = names
            .into_iter()
            .filter(|name| match self.connection_checks.get(name) {
                Some(ConnectionCheck::Checking) => false,
                Some(_) => all,
                None => true,
            })
            .collect();
        for name in pending {
            self.check_connection(name, false, cx);
        }
    }

    /// Probe one connection. `notify` shows the result as a toast, for checks
    /// the user asked for explicitly.
    pub(super) fn check_connection(&mut self, name: String, notify: bool, cx: &mut Context<Self>) {
        self.connection_checks
            .insert(name.clone(), ConnectionCheck::Checking);
        cx.notify();

        let probe = name.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { podman::probe_connection(&probe) })
                .await;
            this.update(cx, |this, cx| {
                if notify {
                    match &result {
                        Ok(()) => {
                            this.toast(ToastKind::Success, format!("{name} is reachable"), cx)
                        }
                        Err(error) => this.toast(
                            ToastKind::Error,
                            format!("{name} is unreachable: {error}"),
                            cx,
                        ),
                    }
                }
                let check = match result {
                    Ok(()) => ConnectionCheck::Reachable,
                    Err(error) => ConnectionCheck::Unreachable(error),
                };
                this.connection_checks.insert(name, check);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Short label and color for a connection's reachability cell.
    pub(super) fn connection_status(&self, name: &str) -> (&'static str, u32) {
        match self.connection_checks.get(name) {
            Some(ConnectionCheck::Checking) => ("checking…", theme().accent),
            Some(ConnectionCheck::Reachable) => ("reachable", theme().green),
            Some(ConnectionCheck::Unreachable(_)) => ("unreachable", theme().red),
            None => ("not checked", theme().faint),
        }
    }

    pub(super) fn machine_summary(&self, machine: Option<&Machine>) -> impl IntoElement {
        let mut content = div()
            .id("machine-summary")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .p_4()
            .flex()
            .flex_col()
            .gap_3();

        let Some(machine) = machine else {
            return content
                .child(section_title("DETAILS"))
                .child(muted_text("This machine is no longer listed by Podman."));
        };

        content = content
            .child(section_title("GENERAL"))
            .child(detail_row("Name", &machine.name))
            .child(detail_row("State", machine.state_label()))
            .child(detail_row("Default", bool_label(machine.default)))
            .child(detail_row("Driver", &machine.vm_type))
            .child(detail_row("CPUs", &machine.cpus.to_string()))
            .child(detail_row("Memory", &machine.memory_label()))
            .child(detail_row("Disk", &machine.disk_label()))
            .child(detail_row("Last up", &machine.last_up));

        content
    }

    pub(super) fn machine_action_bar(
        &self,
        machine: Option<&Machine>,
        cx: &mut Context<Self>,
    ) -> Div {
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

        let Some(machine) = machine else {
            return bar;
        };
        let name = machine.name.clone();

        if machine.running {
            bar = bar.child(pill_button(
                "machine-stop",
                "Stop",
                true,
                theme().red,
                theme().red_bg,
                cx.listener({
                    let name = name.clone();
                    move |this, _, window, cx| {
                        this.open_modal(Modal::StopMachine { name: name.clone() }, window, cx);
                    }
                }),
            ));
            let ssh_name = name.clone();
            bar = bar.child(pill_button(
                "machine-terminal",
                "Terminal",
                true,
                theme().accent,
                theme().accent_soft,
                cx.listener(move |this, _, window, cx| {
                    this.details_tab = DetailsTab::Terminal;
                    this.start_machine_terminal(ssh_name.clone(), window, cx);
                }),
            ));
        } else {
            let start_name = name.clone();
            bar = bar.child(pill_button(
                "machine-start",
                "Start",
                true,
                theme().accent,
                theme().accent_soft,
                cx.listener(move |this, _, _, cx| {
                    this.machine_action(start_name.clone(), MachineAction::Start, cx);
                }),
            ));
        }

        bar.child(pill_button(
            "machine-delete",
            "Delete…",
            true,
            theme().red,
            theme().red_bg,
            cx.listener(move |this, _, window, cx| {
                this.open_modal(Modal::DeleteMachine { name: name.clone() }, window, cx);
            }),
        ))
    }

    pub(super) fn connection_summary(
        &self,
        connection: Option<&Connection>,
        in_use: bool,
    ) -> impl IntoElement {
        let mut content = div()
            .id("connection-summary")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .p_4()
            .flex()
            .flex_col()
            .gap_3();

        let Some(connection) = connection else {
            return content
                .child(section_title("DETAILS"))
                .child(muted_text("This connection is no longer configured."));
        };

        let (status, status_color) = self.connection_status(&connection.name);
        let unreachable = match self.connection_checks.get(&connection.name) {
            Some(ConnectionCheck::Unreachable(error)) => Some(error.clone()),
            _ => None,
        };

        content = content
            .child(section_title("GENERAL"))
            .child(detail_row("Name", &connection.name))
            .child(detail_row("Podman default", bool_label(connection.default)))
            .child(detail_row("Used by this app", bool_label(in_use)))
            .child(section_title("REACHABILITY"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(dot(status_color))
                    .child(div().text_sm().text_color(rgb(status_color)).child(status)),
            )
            .when_some(unreachable, |el, error| el.child(mono_text(error)))
            .child(section_title("URI"))
            .child(mono_text(connection.uri.clone()))
            .child(section_title("IDENTITY"))
            .child(mono_text(connection.identity_label()));

        content
    }

    pub(super) fn connection_action_bar(
        &self,
        connection: Option<&Connection>,
        in_use: bool,
        cx: &mut Context<Self>,
    ) -> Div {
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

        let Some(connection) = connection else {
            return bar;
        };

        let check_name = connection.name.clone();
        let checking = matches!(
            self.connection_checks.get(&connection.name),
            Some(ConnectionCheck::Checking)
        );
        bar = bar.child(pill_button(
            "connection-check",
            "Check",
            !checking,
            theme().muted,
            theme().hover_bg,
            cx.listener(move |this, _, _, cx| {
                this.check_connection(check_name.clone(), true, cx);
            }),
        ));

        if !in_use {
            let name = connection.name.clone();
            bar = bar.child(pill_button(
                "connection-use",
                "Use in this app",
                true,
                theme().accent,
                theme().accent_soft,
                cx.listener(move |this, _, _, cx| this.use_connection(Some(name.clone()), cx)),
            ));
        }

        if self.settings.connection.is_some() {
            bar = bar.child(pill_button(
                "connection-use-default",
                "Use Podman default",
                true,
                theme().muted,
                theme().hover_bg,
                cx.listener(|this, _, _, cx| this.use_connection(None, cx)),
            ));
        }

        bar
    }
}
