use super::helpers::*;
use super::*;

impl Selkie {
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
        if let Some(key) = podman::ssh::needs_unlock(&name) {
            if notify {
                // Ask for the passphrase, then re-run this check.
                self.pending_unlock = Some((name, key, SshUnlockTarget::Probe, None));
            } else {
                self.connection_checks.insert(name, ConnectionCheck::Locked);
            }
            cx.notify();
            return;
        }
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

    /// Create (`original` is `None`) or update a connection. Podman re-adds it
    /// under the new name and URI, and a rename keeps the default flag. The
    /// identity is checked for a passphrase: encrypted keys stay in the app's
    /// cache (Podman cannot read them) and are unlocked into a private agent;
    /// plain keys keep going into Podman's connection record.
    pub(super) fn save_connection(
        &mut self,
        original: Option<String>,
        name: String,
        uri: String,
        identity: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.busy.is_some() {
            self.toast(
                ToastKind::Error,
                "Wait for the running task before editing connections",
                cx,
            );
            return;
        }
        if self.output_run.is_some() {
            self.toast(
                ToastKind::Error,
                "Wait for the running build or compose command before editing connections",
                cx,
            );
            return;
        }
        let label = match &original {
            Some(old) => format!("Updating connection {old}…"),
            None => format!("Adding connection {name}…"),
        };
        self.busy = Some(label);
        cx.notify();

        let in_use = original
            .as_deref()
            .is_some_and(|old| self.uses_connection(old))
            || self.uses_connection(&name);
        let explicit = original
            .as_deref()
            .is_some_and(|old| self.settings.connection.as_deref() == Some(old));
        let task_original = original.clone();
        let task_name = name.clone();
        let task_uri = uri.clone();
        let task_identity = identity.clone();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let key = task_identity
                        .as_deref()
                        .map(podman::expand_identity)
                        .filter(|path| !path.is_empty());
                    let encrypted = match &key {
                        Some(path) => {
                            podman::ssh::key_needs_passphrase(std::path::Path::new(path))?
                        }
                        None => false,
                    };
                    // Podman's SSH client cannot read an encrypted Identity, so
                    // the app keeps it and authenticates through its own agent.
                    let podman_identity = if encrypted { None } else { key.as_deref() };
                    let outcome = match &task_original {
                        Some(old) => {
                            podman::connection_update(old, &task_name, &task_uri, podman_identity)
                        }
                        None => podman::connection_add(&task_name, &task_uri, podman_identity),
                    };
                    outcome.map(|_| (encrypted, key))
                })
                .await;

            this.update(cx, |this, cx| {
                this.busy = None;
                this.local_fetched = None;
                match result {
                    Ok((encrypted, key)) => {
                        // A rename moves the cached key with the connection.
                        if let Some(old) = &original {
                            if old != &name {
                                this.settings.connection_keys.remove(old);
                                podman::ssh::set_cached_key(old, None);
                            }
                        }
                        match &key {
                            Some(path) => {
                                this.settings
                                    .connection_keys
                                    .insert(name.clone(), path.clone());
                                podman::ssh::set_cached_key(&name, Some(path));
                                this.settings.last_connection_key = Some(path.clone());
                            }
                            None => {
                                this.settings.connection_keys.remove(&name);
                                podman::ssh::set_cached_key(&name, None);
                            }
                        }
                        this.settings.save();

                        if explicit && original.as_deref() != Some(name.as_str()) {
                            this.settings.connection = Some(name.clone());
                            this.settings.save();
                            podman::set_connection(Some(name.clone()));
                        }
                        if let Some(old) = &original {
                            this.connection_checks.remove(old);
                        }
                        this.connection_checks.remove(&name);

                        let verb = if original.is_some() {
                            "Updated"
                        } else {
                            "Added"
                        };
                        this.toast(ToastKind::Success, format!("{verb} connection {name}"), cx);
                        if in_use {
                            // The URI or identity may now point at another
                            // engine; also refreshes and restarts events.
                            this.reset_engine(cx);
                        } else {
                            this.refresh(cx);
                        }
                        // Only prompt when the key is actually needed now.
                        if encrypted && in_use {
                            if let Some(path) = key {
                                this.pending_unlock =
                                    Some((name.clone(), path, SshUnlockTarget::Activate, None));
                            }
                        }
                    }
                    Err(error) => {
                        this.toast(ToastKind::Error, error, cx);
                        this.refresh(cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The identity shown and edited for a connection: the app-wide cached key
    /// wins (encrypted keys live there), then Podman's own `Identity`.
    pub(super) fn connection_key(&self, connection: &Connection) -> String {
        self.settings
            .connection_keys
            .get(&connection.name)
            .cloned()
            .unwrap_or_else(|| connection.identity.clone())
    }

    /// Unlock an encrypted identity into the connection's private agent. The
    /// passphrase only lives for the duration of this call.
    pub(super) fn unlock_ssh_key(
        &mut self,
        connection: String,
        key: String,
        passphrase: String,
        target: SshUnlockTarget,
        cx: &mut Context<Self>,
    ) {
        if self.busy.is_some() {
            return;
        }
        self.busy = Some(format!("Unlocking {key}…"));
        cx.notify();

        let task_connection = connection.clone();
        let task_key = key.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    podman::ssh::add_key(
                        &task_connection,
                        std::path::Path::new(&task_key),
                        Some(&passphrase),
                    )
                })
                .await;

            this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    Ok(()) => {
                        this.toast(
                            ToastKind::Success,
                            format!("Unlocked the SSH key for {connection}"),
                            cx,
                        );
                        // The old "key locked" result no longer applies.
                        this.connection_checks.remove(&connection);
                        match target {
                            SshUnlockTarget::Activate => {
                                this.switch_connection(Some(connection.clone()), cx);
                                this.check_connection(connection.clone(), false, cx);
                            }
                            SshUnlockTarget::Probe => {
                                this.check_connection(connection.clone(), true, cx)
                            }
                            SshUnlockTarget::Only => {
                                // Refresh the reachability status now that the
                                // key is usable, without a second toast.
                                this.check_connection(connection.clone(), false, cx)
                            }
                        }
                    }
                    Err(error) => {
                        this.toast(
                            ToastKind::Error,
                            format!("Could not unlock the SSH key: {error}"),
                            cx,
                        );
                        // Re-open the prompt with the error, after the current
                        // frame has a window to focus it.
                        this.pending_unlock = Some((connection, key, target, Some(error)));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Remove a connection. If this app was using it, fall back to Podman's
    /// default engine instead of leaving every command pointed at a name that
    /// no longer exists.
    pub(super) fn remove_connection(&mut self, name: String, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            self.toast(
                ToastKind::Error,
                "Wait for the running task before deleting a connection",
                cx,
            );
            return;
        }
        if self.output_run.is_some() {
            self.toast(
                ToastKind::Error,
                "Wait for the running build or compose command before deleting a connection",
                cx,
            );
            return;
        }
        self.busy = Some(format!("Removing connection {name}…"));
        cx.notify();

        let explicit = self.settings.connection.as_deref() == Some(name.as_str());
        let in_use = self.uses_connection(&name);
        let task_name = name.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { podman::connection_remove(&task_name) })
                .await;
            this.update(cx, |this, cx| {
                this.busy = None;
                this.local_fetched = None;
                this.connection_checks.remove(&name);
                match result {
                    Ok(_) => {
                        // Drop the remembered key and stop its private agent.
                        this.settings.connection_keys.remove(&name);
                        this.settings.save();
                        podman::ssh::set_cached_key(&name, None);
                        if explicit {
                            // Clears the engine data, restarts the event stream
                            // and toasts the switch to Podman's default.
                            this.use_connection(None, cx);
                        } else if in_use {
                            // It was Podman's default, which now falls back to
                            // another engine.
                            this.toast(
                                ToastKind::Success,
                                format!("Removed connection {name}; using Podman's new default"),
                                cx,
                            );
                            this.reset_engine(cx);
                        } else {
                            this.toast(
                                ToastKind::Success,
                                format!("Removed connection {name}"),
                                cx,
                            );
                            this.refresh(cx);
                        }
                    }
                    Err(error) => {
                        this.toast(ToastKind::Error, error, cx);
                        this.refresh(cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Whether the app's commands go to `name`: either selected explicitly, or
    /// it is Podman's default and the app follows the default.
    pub(super) fn uses_connection(&self, name: &str) -> bool {
        match &self.settings.connection {
            Some(selected) => selected == name,
            None => self
                .local
                .connections
                .iter()
                .any(|connection| connection.default && connection.name == name),
        }
    }

    /// Short label and color for a connection's reachability cell.
    pub(super) fn connection_status(&self, name: &str) -> (&'static str, u32) {
        match self.connection_checks.get(name) {
            Some(ConnectionCheck::Checking) => ("checking…", theme().accent),
            Some(ConnectionCheck::Reachable) => ("reachable", theme().green),
            Some(ConnectionCheck::Locked) => ("key locked", theme().yellow),
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

        let key_state = podman::ssh::key_state(&connection.name);
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
            .child(mono_text(format!(
                "{}{}",
                self.connection_key(connection),
                match key_state {
                    podman::ssh::KeyState::Locked => " (locked)",
                    podman::ssh::KeyState::Unlocked => " (unlocked)",
                    podman::ssh::KeyState::Unencrypted | podman::ssh::KeyState::None => "",
                }
            )))
            .when(matches!(key_state, podman::ssh::KeyState::Locked), |el| {
                el.child(mono_text(
                    "The key is locked. Unlock it before checking reachability.",
                ))
            });

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

        if podman::ssh::needs_unlock(&connection.name).is_some() {
            let key_name = connection.name.clone();
            bar = bar.child(pill_button(
                "connection-unlock",
                "Unlock key",
                true,
                theme().yellow,
                theme().yellow_bg,
                cx.listener(move |this, _, _, cx| {
                    if let Some(key) = podman::ssh::cached_key(&key_name) {
                        this.pending_unlock =
                            Some((key_name.clone(), key, SshUnlockTarget::Only, None));
                        cx.notify();
                    }
                }),
            ));
        }

        let edit_connection = connection.clone();
        bar = bar.child(pill_button(
            "connection-edit",
            "Edit…",
            true,
            theme().muted,
            theme().hover_bg,
            cx.listener(move |this, _, window, cx| {
                let key = this.connection_key(&edit_connection);
                this.open_modal(Modal::edit_connection(&edit_connection, &key), window, cx);
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

        let delete_name = connection.name.clone();
        bar = bar.child(pill_button(
            "connection-delete",
            "Delete…",
            self.output_run.is_none(),
            theme().red,
            theme().red_bg,
            cx.listener(move |this, _, window, cx| {
                this.open_modal(Modal::delete_connection(delete_name.clone()), window, cx);
            }),
        ));

        bar
    }
}
