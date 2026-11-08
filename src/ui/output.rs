use super::helpers::*;
use super::*;

impl Selkie {
    // -----------------------------------------------------------------------
    // Streamed command output (compose actions, image builds)
    // -----------------------------------------------------------------------

    pub(super) fn output_panel(&self, cx: &mut Context<Self>) -> Div {
        let Some(title) = &self.output_title else {
            return div();
        };
        let running = self.output_run.is_some();

        let status = if running { "running…" } else { "finished" };
        let status_color = if running {
            theme().accent
        } else {
            theme().muted
        };

        let body: AnyElement = if self.output_lines.is_empty() {
            div()
                .p_4()
                .text_sm()
                .text_color(rgb(theme().faint))
                .child("Waiting for output…")
                .into_any_element()
        } else {
            let mut lines = div()
                .id("output-scroll")
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .track_scroll(&self.output_handle)
                .px_3()
                .py_2()
                .flex()
                .flex_col();
            for line in &self.output_lines {
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

        div()
            .flex_shrink_0()
            .h(px(220.))
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(rgb(theme().border))
            .bg(rgb(theme().sidebar_bg))
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
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title.clone()),
                            )
                            .child(div().text_xs().text_color(rgb(status_color)).child(status)),
                    )
                    .when(!running, |el| {
                        el.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .child(small_button(
                                    "output-clear",
                                    "Clear",
                                    false,
                                    cx.listener(|this, _, _, cx| this.clear_output(cx)),
                                ))
                                .child(small_button(
                                    "output-close",
                                    "×",
                                    false,
                                    cx.listener(|this, _, _, cx| this.clear_output(cx)),
                                )),
                        )
                    }),
            )
            .child(body)
    }
}

impl Selkie {
    // -----------------------------------------------------------------------
    // Long-running commands with streamed output (compose, image builds)
    // -----------------------------------------------------------------------

    pub(super) fn start_compose(
        &mut self,
        project: &ComposeProject,
        action: ComposeAction,
        cx: &mut Context<Self>,
    ) {
        let command = match podman::compose_command(project, action) {
            Ok(command) => command,
            Err(error) => {
                self.toast(ToastKind::Error, error, cx);
                return;
            }
        };

        self.start_output(
            OutputSpec {
                title: format!("compose {} · {}", action.verb(), project.name),
                command_line: format!(
                    "$ {} compose -p {} {}",
                    podman::engine().binary(),
                    project.name,
                    action.verb()
                ),
                success: format!(
                    "compose {} {} {}",
                    action.verb(),
                    project.name,
                    action.done()
                ),
                failure: format!("compose {} {} failed", action.verb(), project.name),
            },
            command,
            None,
            cx,
        );
    }

    /// Run a command, streaming stdout and stderr into the output panel.
    ///
    /// `input` is written to stdin and then closed (used for `--password-stdin`
    /// so credentials never appear on the command line).
    pub(super) fn start_output(
        &mut self,
        spec: OutputSpec,
        mut command: Command,
        input: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let OutputSpec {
            title,
            command_line,
            success,
            failure,
        } = spec;
        if self.output_run.is_some() {
            self.toast(
                ToastKind::Error,
                "A build or compose command is already running",
                cx,
            );
            return;
        }

        command
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        kill_with_parent(&mut command);

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                self.toast(
                    ToastKind::Error,
                    format!("could not run {}: {error}", podman::engine().binary()),
                    cx,
                );
                return;
            }
        };

        if let Some(input) = input {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(input.as_bytes());
                let _ = stdin.write_all(b"\n");
            }
        }

        self.output_lines.clear();
        self.output_lines.push(command_line);
        self.output_title = Some(title);
        self.output_handle.scroll_to_bottom();

        let (sender, receiver) = mpsc::channel::<String>();
        if let Some(stdout) = child.stdout.take() {
            spawn_line_reader(stdout, sender.clone());
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_line_reader(stderr, sender.clone());
        }
        drop(sender);

        let task = cx.spawn(async move |this, cx| loop {
            let finished = this
                .update(cx, |this, cx| this.poll_output(cx))
                .unwrap_or(true);
            if finished {
                break;
            }
            cx.background_executor()
                .timer(Duration::from_millis(80))
                .await;
        });

        self.output_run = Some(OutputRun {
            success,
            failure,
            child: Some(child),
            status: None,
            receiver,
            _task: task,
        });
        cx.notify();
    }

    /// Drain output and report whether the command has finished.
    pub(super) fn poll_output(&mut self, cx: &mut Context<Self>) -> bool {
        let (lines, output_done, exited) = {
            let Some(run) = self.output_run.as_mut() else {
                return true;
            };

            let mut lines = Vec::new();
            let mut output_done = false;
            loop {
                match run.receiver.try_recv() {
                    Ok(line) => lines.push(line),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        output_done = true;
                        break;
                    }
                }
            }

            if run.status.is_none() {
                if let Some(child) = run.child.as_mut() {
                    if let Ok(Some(status)) = child.try_wait() {
                        run.status = Some(status);
                    }
                }
            }

            (lines, output_done, run.status.is_some())
        };

        let pushed = !lines.is_empty();
        for line in lines {
            self.push_output_line(line);
        }
        if pushed {
            cx.notify();
        }
        if output_done && exited {
            self.finish_output(cx);
            return true;
        }
        false
    }

    pub(super) fn push_output_line(&mut self, line: String) {
        self.output_lines.push(strip_ansi(&line));
        if self.output_lines.len() > MAX_OUTPUT_LINES {
            let excess = self.output_lines.len() - MAX_OUTPUT_LINES;
            self.output_lines.drain(0..excess);
        }
        self.output_handle.scroll_to_bottom();
    }

    pub(super) fn finish_output(&mut self, cx: &mut Context<Self>) {
        let title = self.output_title.clone().unwrap_or_default();
        let Some(run) = self.output_run.take() else {
            return;
        };
        let success = run.status.map(|status| status.success()).unwrap_or(false);
        let summary = match run.status {
            Some(status) if status.success() => format!("── {title} finished ──"),
            Some(status) => format!("── {title} failed: {status} ──"),
            None => format!("── {title} finished ──"),
        };
        self.output_lines.push(summary);
        self.output_handle.scroll_to_bottom();

        if success {
            self.toast(ToastKind::Success, run.success.clone(), cx);
        } else {
            self.toast(ToastKind::Error, run.failure.clone(), cx);
        }

        // `machine init` and friends run here; re-read local state.
        self.local_fetched = None;
        self.refresh(cx);
        cx.notify();
    }

    pub(super) fn clear_output(&mut self, cx: &mut Context<Self>) {
        if self.output_run.is_some() {
            return;
        }
        self.output_lines.clear();
        self.output_title = None;
        cx.notify();
    }

    pub(super) fn container_action(
        &mut self,
        id: String,
        action: ContainerAction,
        cx: &mut Context<Self>,
    ) {
        let label = format!("{}…", action.label());
        let success = action.done().to_string();
        self.run_task(
            label,
            success,
            AfterAction::RefreshLogs,
            move || podman::container_action(&id, action),
            cx,
        );
    }

    pub(super) fn pod_action(&mut self, id: String, action: PodAction, cx: &mut Context<Self>) {
        let label = format!("{}…", action.label());
        let success = action.done().to_string();
        self.run_task(
            label,
            success,
            AfterAction::Refresh,
            move || podman::pod_action(&id, action),
            cx,
        );
    }

    pub(super) fn run_task<F>(
        &mut self,
        label: String,
        success: String,
        after: AfterAction,
        task: F,
        cx: &mut Context<Self>,
    ) where
        F: FnOnce() -> Result<String, String> + Send + 'static,
    {
        if self.busy.is_some() {
            return;
        }
        self.busy = Some(label);
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { task() }).await;
            this.update(cx, |this, cx| {
                this.busy = None;
                // Any action may have changed a machine or connection.
                this.local_fetched = None;
                match result {
                    Ok(_) => {
                        this.toast(ToastKind::Success, success, cx);
                        match after {
                            AfterAction::Refresh => {
                                this.refresh(cx);
                                this.reload_selection(cx);
                            }
                            AfterAction::RefreshLogs => {
                                this.refresh(cx);
                                this.reload_selection(cx);
                                if let Some(Selection::Container(id)) = &this.selection {
                                    let id = id.clone();
                                    this.start_logs(&id, cx);
                                }
                            }
                            AfterAction::Deselect => {
                                this.clear_selection();
                                this.refresh(cx);
                            }
                        }
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
