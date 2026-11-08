//! Selection, the details pane data, log streams, stats and one-shot commands.

use super::*;

impl Selkie {
    // -----------------------------------------------------------------------
    // Selection, logs and commands
    // -----------------------------------------------------------------------

    pub(super) fn select_container(&mut self, id: String, cx: &mut Context<Self>) {
        self.context_menu = None;
        self.reset_raw();
        self.selection = Some(Selection::Container(id.clone()));
        self.details_tab = DetailsTab::Summary;
        self.details = DetailsState::Loading;
        self.stop_stats();
        self.stats.clear();
        self.stats_container = None;
        self.stats_error = None;
        self.stop_terminal();
        self.start_logs(&id, cx);
        self.load_details(Selection::Container(id), cx);
        cx.notify();
    }

    pub(super) fn select_compose(&mut self, name: String, cx: &mut Context<Self>) {
        self.context_menu = None;
        self.reset_raw();
        self.stop_logs();
        self.logs.clear();
        self.logs_error = None;
        self.selection = Some(Selection::Compose(name));
        self.details_tab = DetailsTab::Summary;
        self.details = DetailsState::None;
        cx.notify();
    }

    pub(super) fn select_machine(&mut self, name: String, cx: &mut Context<Self>) {
        self.context_menu = None;
        self.reset_raw();
        self.stop_logs();
        self.stop_terminal();
        self.logs.clear();
        self.logs_error = None;
        self.selection = Some(Selection::Machine(name));
        self.details_tab = DetailsTab::Summary;
        self.details = DetailsState::None;
        cx.notify();
    }

    pub(super) fn select_connection(&mut self, name: String, cx: &mut Context<Self>) {
        self.context_menu = None;
        self.reset_raw();
        self.stop_logs();
        self.logs.clear();
        self.logs_error = None;
        self.selection = Some(Selection::Connection(name));
        self.details_tab = DetailsTab::Summary;
        self.details = DetailsState::None;
        cx.notify();
    }

    /// Switch the engine the whole app talks to.
    pub(super) fn use_connection(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        if self.settings.connection == name {
            return;
        }
        if self.output_run.is_some() {
            self.toast(
                ToastKind::Error,
                "Wait for the running compose command before switching engines",
                cx,
            );
            return;
        }
        if let Some(name) = &name {
            if let Some(key) = podman::ssh::needs_unlock(name) {
                // Ask for the passphrase first; the switch continues after it
                // is accepted.
                self.pending_unlock = Some((name.clone(), key, SshUnlockTarget::Activate, None));
                cx.notify();
                return;
            }
        }
        self.switch_connection(name, cx);
    }

    /// Point every command at `name` (or Podman's default). Also used after an
    /// SSH key is unlocked, where the connection may already be selected but
    /// its engine data needs a fresh start.
    pub(super) fn switch_connection(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        if self.output_run.is_some() {
            self.toast(
                ToastKind::Error,
                "Wait for the running compose command before switching engines",
                cx,
            );
            return;
        }
        self.settings.connection = name.clone();
        self.settings.save();
        podman::set_connection(name.clone());
        self.reset_engine(cx);

        let message = match &name {
            Some(name) => format!("Using connection {name}"),
            None => "Using the Podman default engine".to_string(),
        };
        self.toast(ToastKind::Success, message, cx);
        cx.notify();
    }

    /// Switch the container engine (Podman CLI or the Docker HTTP API).
    pub(super) fn use_engine(&mut self, kind: EngineKind, cx: &mut Context<Self>) {
        if self.settings.engine == kind {
            return;
        }
        if self.output_run.is_some() {
            self.toast(
                ToastKind::Error,
                "Wait for the running build or compose command before switching engines",
                cx,
            );
            return;
        }
        self.settings.engine = kind;
        self.settings.save();
        podman::set_engine(kind);
        // Podman's named connections only exist for the CLI engine; clear or
        // restore the runtime value so switching back targets the saved one.
        if kind == EngineKind::Podman {
            podman::set_connection(self.settings.connection.clone());
        } else {
            podman::set_connection(None);
        }
        // Also falls back from a Podman-only view.
        self.reset_engine(cx);
        self.load_accounts();
        self.toast(
            ToastKind::Success,
            format!("Using the {} engine", kind.label()),
            cx,
        );
        cx.notify();
    }

    /// Fall back from a Podman-only view when Docker has no such feature.
    fn ensure_supported_tab(&mut self, cx: &mut Context<Self>) {
        if self.tab.supported_by(podman::engine()) {
            return;
        }
        self.tab = Tab::Containers;
        self.settings.tab = Tab::Containers;
        self.settings.save();
        self.sort = None;
        self.search.clear();
        self.account_menu = None;
        self.clear_selection();
        cx.notify();
    }

    /// The engine behind the app changed (another connection, or the current
    /// one now points elsewhere): drop the previous engine's data, streams and
    /// selection, keep the local machine/connection lists, and start over.
    pub(super) fn reset_engine(&mut self, cx: &mut Context<Self>) {
        self.engine_generation += 1;
        self.ensure_supported_tab(cx);
        self.stop_logs();
        self.stop_terminal();
        self.stop_events();
        self.dashboard = None;
        self.dashboard_fetched = None;
        self.dashboard_error = None;
        self.local = LocalState::default();
        self.local_fetched = None;
        self.logs.clear();
        self.logs_error = None;
        self.snapshot = Snapshot::default();
        self.error = None;
        self.selection = None;
        self.pending_unlock = None;
        self.details = DetailsState::None;
        self.details_tab = DetailsTab::Summary;
        self.raw_view = false;
        self.raw = None;
        self.refresh(cx);
        self.start_events(cx);
    }

    pub(super) fn machine_action(
        &mut self,
        name: String,
        action: MachineAction,
        cx: &mut Context<Self>,
    ) {
        let label = format!("{} machine {name}…", action.gerund());
        let success = format!("Machine {name} {}", action.done());
        self.run_task(
            label,
            success,
            AfterAction::Refresh,
            move || podman::machine_action(&name, action),
            cx,
        );
    }

    pub(super) fn select_image(&mut self, id: String, cx: &mut Context<Self>) {
        self.context_menu = None;
        self.reset_raw();
        self.reset_image_views();
        self.stop_logs();
        self.logs.clear();
        self.logs_error = None;
        self.selection = Some(Selection::Image(id));
        self.details_tab = DetailsTab::Summary;
        self.details = DetailsState::None;
        cx.notify();
    }

    pub(super) fn select_pod(&mut self, id: String, cx: &mut Context<Self>) {
        self.context_menu = None;
        self.reset_raw();
        self.stop_logs();
        self.logs.clear();
        self.logs_error = None;
        self.selection = Some(Selection::Pod(id.clone()));
        self.details_tab = DetailsTab::Summary;
        self.load_details(Selection::Pod(id), cx);
        cx.notify();
    }

    pub(super) fn select_volume(&mut self, name: String, cx: &mut Context<Self>) {
        self.context_menu = None;
        self.reset_raw();
        self.stop_logs();
        self.logs.clear();
        self.logs_error = None;
        self.selection = Some(Selection::Volume(name.clone()));
        self.details_tab = DetailsTab::Summary;
        self.load_details(Selection::Volume(name), cx);
        cx.notify();
    }

    pub(super) fn select_network(&mut self, name: String, cx: &mut Context<Self>) {
        self.context_menu = None;
        self.reset_raw();
        self.stop_logs();
        self.logs.clear();
        self.logs_error = None;
        self.selection = Some(Selection::Network(name.clone()));
        self.details_tab = DetailsTab::Summary;
        self.load_details(Selection::Network(name), cx);
        cx.notify();
    }

    pub(super) fn select_secret(&mut self, id: String, cx: &mut Context<Self>) {
        self.context_menu = None;
        self.reset_raw();
        self.stop_logs();
        self.logs.clear();
        self.logs_error = None;
        self.selection = Some(Selection::Secret(id));
        self.details_tab = DetailsTab::Summary;
        // Everything the summary shows comes from the list; only the JSON view
        // runs `podman secret inspect`.
        self.details = DetailsState::None;
        cx.notify();
    }

    pub(super) fn clear_selection(&mut self) {
        self.context_menu = None;
        self.reset_raw();
        if self.selection.is_some() {
            self.selection = None;
            self.details_tab = DetailsTab::Summary;
            self.details = DetailsState::None;
            self.stop_logs();
            self.logs.clear();
            self.logs_error = None;
            self.stop_stats();
            self.stats.clear();
            self.stats_container = None;
            self.stats_error = None;
            self.stop_terminal();
        }
    }

    pub(super) fn start_logs(&mut self, id: &str, cx: &mut Context<Self>) {
        let command = podman::container_logs_command(id, self.settings.log_window.as_str());
        self.start_log_stream(command, LogTarget::Container(id.to_string()), cx);
    }

    /// Stream `podman compose logs` for a project, or one of its services.
    pub(super) fn start_compose_logs(
        &mut self,
        project: &ComposeProject,
        service: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let target = LogTarget::Compose {
            project: project.name.clone(),
            service: service.clone(),
        };
        match podman::compose_logs_command(project, service.as_deref(), COMPOSE_LOG_TAIL) {
            Ok(command) => self.start_log_stream(command, target, cx),
            Err(error) => {
                // Still show the Logs tab with the reason instead of a stale stream.
                self.stop_logs();
                self.logs.clear();
                self.logs_error = Some(error);
                self.logs_target = Some(target);
                cx.notify();
            }
        }
    }

    pub(super) fn start_log_stream(
        &mut self,
        mut command: Command,
        target: LogTarget,
        cx: &mut Context<Self>,
    ) {
        self.stop_logs();
        self.logs.clear();
        self.logs_error = None;
        self.logs_limit = MAX_LOG_LINES;
        self.logs_loading_older = false;
        self.logs_follow = true;
        self.logs_target = Some(target);

        command.stdout(Stdio::piped()).stderr(Stdio::piped());

        // Do not leave `podman logs -f` behind if the app is killed without
        // unwinding (Drop would otherwise take care of it on a clean exit).
        kill_with_parent(&mut command);

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                self.logs_error = Some(format!("could not start log stream: {error}"));
                return;
            }
        };

        let (sender, receiver) = mpsc::channel::<String>();
        if let Some(stdout) = child.stdout.take() {
            spawn_line_reader(stdout, sender.clone());
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_line_reader(stderr, sender.clone());
        }
        drop(sender);

        // Drain everything that arrived since the last tick in one update and
        // always yield afterwards: a large backlog or a chatty container must not
        // keep the foreground executor busy line by line.
        let task = cx.spawn(async move |this, cx| loop {
            let mut lines = Vec::new();
            let mut disconnected = false;
            loop {
                match receiver.try_recv() {
                    Ok(line) => {
                        lines.push(line);
                        if lines.len() >= LOG_BATCH {
                            break;
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
            if !lines.is_empty()
                && this
                    .update(cx, |this, cx| {
                        this.push_logs(lines);
                        cx.notify();
                    })
                    .is_err()
            {
                break;
            }
            if disconnected {
                break;
            }
            cx.background_executor()
                .timer(Duration::from_millis(60))
                .await;
        });

        self.log_stream = Some(LogStream {
            child: Some(child),
            _task: task,
        });
    }

    pub(super) fn stop_logs(&mut self) {
        // Dropping the stream kills `podman logs -f` and cancels the reader task.
        self.log_stream = None;
        self.logs_target = None;
    }

    /// Start sampling `podman stats` for a container, keeping history in memory.
    pub(super) fn start_stats(&mut self, id: String, cx: &mut Context<Self>) {
        self.stop_stats();
        if self.stats_container.as_deref() != Some(id.as_str()) {
            self.stats.clear();
            self.stats_error = None;
            self.stats_container = Some(id.clone());
        }

        let task = cx.spawn(async move |this, cx| loop {
            let fetch_id = id.clone();
            let result = cx
                .background_executor()
                .spawn(async move { podman::stats(&fetch_id) })
                .await;

            let keep_going = this
                .update(cx, |this, cx| {
                    match result {
                        Ok(Some(sample)) => {
                            this.stats_error = None;
                            this.stats.push(sample);
                            if this.stats.len() > MAX_STATS_SAMPLES {
                                let excess = this.stats.len() - MAX_STATS_SAMPLES;
                                this.stats.drain(0..excess);
                            }
                        }
                        Ok(None) => {
                            this.stats_error = Some("Podman returned no statistics".to_string());
                        }
                        Err(error) => this.stats_error = Some(error),
                    }
                    cx.notify();

                    matches!(&this.selection, Some(Selection::Container(current)) if current == &id)
                        && this.details_tab == DetailsTab::Stats
                })
                .unwrap_or(false);

            if !keep_going {
                break;
            }
            cx.background_executor().timer(STATS_INTERVAL).await;
        });

        self.stats_task = Some(task);
    }

    pub(super) fn stop_stats(&mut self) {
        // Dropping the task stops the sampling loop.
        self.stats_task = None;
    }

    pub(super) fn push_logs(&mut self, lines: Vec<String>) {
        self.logs.extend(lines.iter().map(|line| strip_ansi(line)));
        // Trim once per batch; trimming per line is O(n) each time at the limit.
        if self.logs.len() > self.logs_limit {
            let excess = self.logs.len() - self.logs_limit;
            self.logs.drain(0..excess);
        }
        if self.logs_follow {
            self.logs_handle.scroll_to_bottom();
        }
    }

    /// Fetch earlier log output with `podman logs --tail` and prepend it.
    ///
    /// The requested size grows with the current buffer, so repeated clicks
    /// reach further back. Only lines older than the first visible line are
    /// merged, which keeps the live stream and the one-shot fetch from
    /// duplicating each other.
    pub(super) fn load_older_logs(&mut self, cx: &mut Context<Self>) {
        if self.logs_loading_older {
            return;
        }
        let Some(LogTarget::Container(id)) = self.logs_target.clone() else {
            return;
        };
        let requested = (self.logs.len() + LOG_CHUNK).min(MAX_LOG_HISTORY);
        if requested <= self.logs.len() {
            self.toast(ToastKind::Success, "Reached the log history limit", cx);
            return;
        }

        let mut command = podman::logs_tail_command(&id, requested);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        kill_with_parent(&mut command);

        self.logs_limit = requested;
        self.logs_loading_older = true;
        // Loading history means the user is reading older output; don't yank
        // the view back to the bottom while the fetch is in flight.
        self.logs_follow = false;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    command
                        .output()
                        .map_err(|error| format!("could not read older logs: {error}"))
                })
                .await;

            this.update(cx, |this, cx| {
                this.logs_loading_older = false;
                match result {
                    Ok(output) if output.status.success() => {
                        let text = String::from_utf8_lossy(&output.stdout);
                        let older = older_log_lines(&text, &this.logs);
                        if older.is_empty() {
                            this.toast(ToastKind::Success, "No older log output", cx);
                        } else {
                            let added = older.len();
                            let before = this.logs.len();
                            // Remember which line was at the top so the view can
                            // stay on the same content after prepending.
                            let anchor = this.logs_handle.top_item();
                            let mut merged = older;
                            merged.append(&mut this.logs);
                            this.logs = merged;
                            if this.logs.len() > this.logs_limit {
                                let excess = this.logs.len() - this.logs_limit;
                                this.logs.drain(0..excess);
                            }
                            let shift = this.logs.len().saturating_sub(before);
                            if shift > 0 && before > 0 {
                                this.logs_handle.scroll_to_top_of_item(anchor + shift);
                            }
                            this.toast(
                                ToastKind::Success,
                                format!("Loaded {added} older log lines"),
                                cx,
                            );
                        }
                    }
                    Ok(output) => {
                        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                        let message = if stderr.is_empty() {
                            format!("could not read older logs: {}", output.status)
                        } else {
                            stderr
                        };
                        this.toast(ToastKind::Error, message, cx);
                    }
                    Err(error) => this.toast(ToastKind::Error, error, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
