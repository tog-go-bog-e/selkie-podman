use super::helpers::*;
use super::*;

/// `(label, value, placeholder, required)` for a text input in a modal.
type ModalField<'a> = (&'static str, &'a str, &'static str, bool);

/// `(title, body, inputs, confirm label, danger)`
type ModalSpec<'a> = (
    &'static str,
    Option<String>,
    Vec<ModalField<'a>>,
    &'static str,
    bool,
);

impl PodmanDesktop {
    // Status bar, toasts and modal
    // -----------------------------------------------------------------------

    pub(super) fn status_bar(&self) -> Div {
        let (color, label) = match &self.error {
            Some(error) => (theme().red, format!("podman error · {error}")),
            None if self.snapshot.version.is_empty() => {
                (theme().yellow, "waiting for podman…".to_string())
            }
            None => (theme().green, self.snapshot.version.clone()),
        };

        let right = match &self.busy {
            Some(label) => div()
                .text_xs()
                .text_color(rgb(theme().accent))
                .child(label.clone()),
            None => {
                let refresh = if self.events_connected {
                    "live".to_string()
                } else {
                    format!("auto {}s", self.settings.refresh_secs)
                };
                div().whitespace_nowrap().child(format!(
                    "j/k · enter · / search · esc · ctrl+q quit · {refresh}"
                ))
            }
        };

        div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(30.))
            .px_4()
            .bg(rgb(theme().sidebar_bg))
            .border_t_1()
            .border_color(rgb(theme().border))
            .text_xs()
            .text_color(rgb(theme().muted))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_w(px(0.))
                    .child(dot(color))
                    .child(div().whitespace_nowrap().truncate().child(label)),
            )
            .child(right)
    }

    pub(super) fn toast(
        &mut self,
        kind: ToastKind,
        message: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        let id = self.next_toast_id;
        self.next_toast_id += 1;
        self.toasts.push(Toast {
            id,
            kind,
            message: message.into(),
        });
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
        cx.notify();

        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TOAST_TTL).await;
            this.update(cx, |this, cx| {
                this.toasts.retain(|toast| toast.id != id);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn toasts_view(&self) -> Div {
        let mut column = div()
            .absolute()
            .bottom_4()
            .right_4()
            .w(px(360.))
            .flex()
            .flex_col()
            .gap_2();

        for toast in &self.toasts {
            let (fg, bg) = match toast.kind {
                ToastKind::Success => (theme().green, theme().green_bg),
                ToastKind::Error => (theme().red, theme().red_bg),
            };
            column = column.child(
                div()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(bg))
                    .border_1()
                    .border_color(rgb(fg))
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(fg))
                            .child(toast.message.clone()),
                    ),
            );
        }

        column
    }

    pub(super) fn context_menu_view(&self, cx: &mut Context<Self>) -> Div {
        let Some(menu) = &self.context_menu else {
            return div();
        };
        let busy = self.busy.is_some();
        let mut items = div().flex().flex_col();

        match &menu.selection {
            Selection::Container(id) => {
                if let Some(container) = self
                    .snapshot
                    .containers
                    .iter()
                    .find(|item| &item.id == id)
                    .cloned()
                {
                    let running = container.state == "running";
                    let paused = container.state == "paused";
                    let action_id = container.id.clone();
                    let name = container.name.clone();

                    if running {
                        let stop_id = action_id.clone();
                        items = items.child(menu_item(
                            "ctx-stop",
                            "Stop",
                            !busy,
                            false,
                            cx.listener(move |this, _, _, cx| {
                                this.close_context_menu(cx);
                                this.container_action(stop_id.clone(), ContainerAction::Stop, cx);
                            }),
                        ));
                        let pause_id = action_id.clone();
                        items = items.child(menu_item(
                            "ctx-pause",
                            "Pause",
                            !busy,
                            false,
                            cx.listener(move |this, _, _, cx| {
                                this.close_context_menu(cx);
                                this.container_action(pause_id.clone(), ContainerAction::Pause, cx);
                            }),
                        ));
                        let restart_id = action_id.clone();
                        items = items.child(menu_item(
                            "ctx-restart",
                            "Restart",
                            !busy,
                            false,
                            cx.listener(move |this, _, _, cx| {
                                this.close_context_menu(cx);
                                this.container_action(
                                    restart_id.clone(),
                                    ContainerAction::Restart,
                                    cx,
                                );
                            }),
                        ));
                    } else if paused {
                        let unpause_id = action_id.clone();
                        items = items.child(menu_item(
                            "ctx-unpause",
                            "Resume",
                            !busy,
                            false,
                            cx.listener(move |this, _, _, cx| {
                                this.close_context_menu(cx);
                                this.container_action(
                                    unpause_id.clone(),
                                    ContainerAction::Unpause,
                                    cx,
                                );
                            }),
                        ));
                        let stop_id = action_id.clone();
                        items = items.child(menu_item(
                            "ctx-stop",
                            "Stop",
                            !busy,
                            false,
                            cx.listener(move |this, _, _, cx| {
                                this.close_context_menu(cx);
                                this.container_action(stop_id.clone(), ContainerAction::Stop, cx);
                            }),
                        ));
                    } else {
                        let start_id = action_id.clone();
                        items = items.child(menu_item(
                            "ctx-start",
                            "Start",
                            !busy,
                            false,
                            cx.listener(move |this, _, _, cx| {
                                this.close_context_menu(cx);
                                this.container_action(start_id.clone(), ContainerAction::Start, cx);
                            }),
                        ));
                    }

                    let rename_id = action_id.clone();
                    let rename_name = name.clone();
                    items = items.child(menu_item(
                        "ctx-rename",
                        "Rename…",
                        !busy,
                        false,
                        cx.listener(move |this, _, window, cx| {
                            this.close_context_menu(cx);
                            this.open_modal(
                                Modal::RenameContainer {
                                    id: rename_id.clone(),
                                    name: rename_name.clone(),
                                    input: String::new(),
                                },
                                window,
                                cx,
                            );
                        }),
                    ));

                    let delete_id = action_id.clone();
                    let delete_name = name.clone();
                    items = items.child(menu_item(
                        "ctx-delete",
                        "Delete…",
                        !busy,
                        true,
                        cx.listener(move |this, _, window, cx| {
                            this.close_context_menu(cx);
                            this.open_modal(
                                Modal::DeleteContainer {
                                    id: delete_id.clone(),
                                    name: delete_name.clone(),
                                },
                                window,
                                cx,
                            );
                        }),
                    ));

                    let copy_id = action_id.clone();
                    items = items.child(menu_item(
                        "ctx-copy-id",
                        "Copy ID",
                        true,
                        false,
                        cx.listener(move |this, _, _, cx| {
                            this.close_context_menu(cx);
                            this.copy_text(copy_id.clone(), "container ID", cx);
                        }),
                    ));
                    let copy_name = name.clone();
                    items = items.child(menu_item(
                        "ctx-copy-name",
                        "Copy name",
                        true,
                        false,
                        cx.listener(move |this, _, _, cx| {
                            this.close_context_menu(cx);
                            this.copy_text(copy_name.clone(), "container name", cx);
                        }),
                    ));
                }
            }
            Selection::Image(id) => {
                if let Some(image) = self
                    .snapshot
                    .images
                    .iter()
                    .find(|item| &item.id == id)
                    .cloned()
                {
                    let image_id = image.id.clone();
                    let image_name = image.name.clone();

                    let run_image = image_name.clone();
                    items = items.child(menu_item(
                        "ctx-image-run",
                        "Run…",
                        !busy,
                        false,
                        cx.listener(move |this, _, window, cx| {
                            this.close_context_menu(cx);
                            this.open_modal(Modal::run_container(run_image.clone()), window, cx);
                        }),
                    ));

                    let push_image = image_name.clone();
                    items = items.child(menu_item(
                        "ctx-image-push",
                        "Push…",
                        !busy && self.output_run.is_none(),
                        false,
                        cx.listener(move |this, _, window, cx| {
                            this.close_context_menu(cx);
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

                    let tag_id = image_id.clone();
                    let tag_source = image_name.clone();
                    items = items.child(menu_item(
                        "ctx-tag",
                        "Tag…",
                        !busy,
                        false,
                        cx.listener(move |this, _, window, cx| {
                            this.close_context_menu(cx);
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

                    let delete_id = image_id.clone();
                    let delete_name = image_name.clone();
                    items = items.child(menu_item(
                        "ctx-image-delete",
                        "Delete…",
                        !busy,
                        true,
                        cx.listener(move |this, _, window, cx| {
                            this.close_context_menu(cx);
                            this.open_modal(
                                Modal::DeleteImage {
                                    id: delete_id.clone(),
                                    name: delete_name.clone(),
                                },
                                window,
                                cx,
                            );
                        }),
                    ));

                    let copy_id = image_id.clone();
                    items = items.child(menu_item(
                        "ctx-copy-id",
                        "Copy ID",
                        true,
                        false,
                        cx.listener(move |this, _, _, cx| {
                            this.close_context_menu(cx);
                            this.copy_text(copy_id.clone(), "image ID", cx);
                        }),
                    ));
                    let copy_name = image_name.clone();
                    items = items.child(menu_item(
                        "ctx-copy-name",
                        "Copy name",
                        true,
                        false,
                        cx.listener(move |this, _, _, cx| {
                            this.close_context_menu(cx);
                            this.copy_text(copy_name.clone(), "image name", cx);
                        }),
                    ));
                }
            }
            Selection::Pod(id) => {
                if let Some(pod) = self
                    .snapshot
                    .pods
                    .iter()
                    .find(|item| &item.id == id)
                    .cloned()
                {
                    let running = pod.status == "Running";
                    let pod_id = pod.id.clone();
                    let pod_name = pod.name.clone();

                    if running {
                        let stop_id = pod_id.clone();
                        items = items.child(menu_item(
                            "ctx-pod-stop",
                            "Stop",
                            !busy,
                            false,
                            cx.listener(move |this, _, _, cx| {
                                this.close_context_menu(cx);
                                this.pod_action(stop_id.clone(), PodAction::Stop, cx);
                            }),
                        ));
                        let restart_id = pod_id.clone();
                        items = items.child(menu_item(
                            "ctx-pod-restart",
                            "Restart",
                            !busy,
                            false,
                            cx.listener(move |this, _, _, cx| {
                                this.close_context_menu(cx);
                                this.pod_action(restart_id.clone(), PodAction::Restart, cx);
                            }),
                        ));
                    } else {
                        let start_id = pod_id.clone();
                        items = items.child(menu_item(
                            "ctx-pod-start",
                            "Start",
                            !busy,
                            false,
                            cx.listener(move |this, _, _, cx| {
                                this.close_context_menu(cx);
                                this.pod_action(start_id.clone(), PodAction::Start, cx);
                            }),
                        ));
                    }

                    let delete_id = pod_id.clone();
                    let delete_name = pod_name.clone();
                    items = items.child(menu_item(
                        "ctx-pod-delete",
                        "Delete…",
                        !busy,
                        true,
                        cx.listener(move |this, _, window, cx| {
                            this.close_context_menu(cx);
                            this.open_modal(
                                Modal::DeletePod {
                                    id: delete_id.clone(),
                                    name: delete_name.clone(),
                                },
                                window,
                                cx,
                            );
                        }),
                    ));

                    let copy_id = pod_id.clone();
                    items = items.child(menu_item(
                        "ctx-pod-copy-id",
                        "Copy ID",
                        true,
                        false,
                        cx.listener(move |this, _, _, cx| {
                            this.close_context_menu(cx);
                            this.copy_text(copy_id.clone(), "pod ID", cx);
                        }),
                    ));
                    let copy_name = pod_name.clone();
                    items = items.child(menu_item(
                        "ctx-pod-copy-name",
                        "Copy name",
                        true,
                        false,
                        cx.listener(move |this, _, _, cx| {
                            this.close_context_menu(cx);
                            this.copy_text(copy_name.clone(), "pod name", cx);
                        }),
                    ));
                }
            }
            Selection::Volume(name) => {
                if let Some(volume) = self
                    .snapshot
                    .volumes
                    .iter()
                    .find(|item| &item.name == name)
                    .cloned()
                {
                    let delete_name = volume.name.clone();
                    items = items.child(menu_item(
                        "ctx-volume-delete",
                        "Delete…",
                        !busy,
                        true,
                        cx.listener(move |this, _, window, cx| {
                            this.close_context_menu(cx);
                            this.open_modal(
                                Modal::DeleteVolume {
                                    name: delete_name.clone(),
                                },
                                window,
                                cx,
                            );
                        }),
                    ));

                    let copy_name = volume.name.clone();
                    items = items.child(menu_item(
                        "ctx-volume-copy-name",
                        "Copy name",
                        true,
                        false,
                        cx.listener(move |this, _, _, cx| {
                            this.close_context_menu(cx);
                            this.copy_text(copy_name.clone(), "volume name", cx);
                        }),
                    ));
                    let copy_path = volume.mountpoint.clone();
                    items = items.child(menu_item(
                        "ctx-volume-copy-path",
                        "Copy mountpoint",
                        true,
                        false,
                        cx.listener(move |this, _, _, cx| {
                            this.close_context_menu(cx);
                            this.copy_text(copy_path.clone(), "mountpoint", cx);
                        }),
                    ));
                }
            }
            Selection::Secret(id) => {
                if let Some(secret) = self
                    .snapshot
                    .secrets
                    .iter()
                    .find(|item| &item.id == id)
                    .cloned()
                {
                    let delete_id = secret.id.clone();
                    let delete_name = secret.name.clone();
                    items = items.child(menu_item(
                        "ctx-secret-delete",
                        "Delete…",
                        !busy,
                        true,
                        cx.listener(move |this, _, window, cx| {
                            this.close_context_menu(cx);
                            this.open_modal(
                                Modal::DeleteSecret {
                                    id: delete_id.clone(),
                                    name: delete_name.clone(),
                                },
                                window,
                                cx,
                            );
                        }),
                    ));

                    let copy_name = secret.name.clone();
                    items = items.child(menu_item(
                        "ctx-secret-copy-name",
                        "Copy name",
                        true,
                        false,
                        cx.listener(move |this, _, _, cx| {
                            this.close_context_menu(cx);
                            this.copy_text(copy_name.clone(), "secret name", cx);
                        }),
                    ));
                    let copy_id = secret.id.clone();
                    items = items.child(menu_item(
                        "ctx-secret-copy-id",
                        "Copy ID",
                        true,
                        false,
                        cx.listener(move |this, _, _, cx| {
                            this.close_context_menu(cx);
                            this.copy_text(copy_id.clone(), "secret ID", cx);
                        }),
                    ));
                }
            }
            Selection::Compose(name) => {
                let project = self
                    .snapshot
                    .compose
                    .iter()
                    .find(|item| &item.name == name)
                    .cloned();
                if let Some(project) = project {
                    let available = !busy && self.output_run.is_none();

                    let up_project = project.clone();
                    items = items.child(menu_item(
                        "ctx-compose-up",
                        "Up (compose up -d)",
                        available,
                        false,
                        cx.listener(move |this, _, _, cx| {
                            this.close_context_menu(cx);
                            this.start_compose(&up_project, ComposeAction::Up, cx);
                        }),
                    ));

                    let down_project = project.clone();
                    let restart_project = project.clone();
                    items = items.child(menu_item(
                        "ctx-compose-restart",
                        "Restart",
                        available,
                        false,
                        cx.listener(move |this, _, _, cx| {
                            this.close_context_menu(cx);
                            this.start_compose(&restart_project, ComposeAction::Restart, cx);
                        }),
                    ));
                    let build_project = project.clone();
                    items = items.child(menu_item(
                        "ctx-compose-build",
                        "Build",
                        available,
                        false,
                        cx.listener(move |this, _, _, cx| {
                            this.close_context_menu(cx);
                            this.start_compose(&build_project, ComposeAction::Build, cx);
                        }),
                    ));
                    items = items.child(menu_item(
                        "ctx-compose-down",
                        "Down…",
                        available,
                        true,
                        cx.listener(move |this, _, window, cx| {
                            this.close_context_menu(cx);
                            this.open_modal(
                                Modal::ComposeDown {
                                    project: down_project.clone(),
                                },
                                window,
                                cx,
                            );
                        }),
                    ));

                    let copy_name = project.name.clone();
                    items = items.child(menu_item(
                        "ctx-compose-copy-name",
                        "Copy project name",
                        true,
                        false,
                        cx.listener(move |this, _, _, cx| {
                            this.close_context_menu(cx);
                            this.copy_text(copy_name.clone(), "project name", cx);
                        }),
                    ));
                }
            }
            Selection::Network(name) => {
                if let Some(network) = self
                    .snapshot
                    .networks
                    .iter()
                    .find(|item| &item.name == name)
                    .cloned()
                {
                    let delete_name = network.name.clone();
                    items = items.child(menu_item(
                        "ctx-network-delete",
                        "Delete…",
                        !busy,
                        true,
                        cx.listener(move |this, _, window, cx| {
                            this.close_context_menu(cx);
                            this.open_modal(
                                Modal::DeleteNetwork {
                                    name: delete_name.clone(),
                                },
                                window,
                                cx,
                            );
                        }),
                    ));

                    let copy_id = network.id.clone();
                    items = items.child(menu_item(
                        "ctx-network-copy-id",
                        "Copy ID",
                        true,
                        false,
                        cx.listener(move |this, _, _, cx| {
                            this.close_context_menu(cx);
                            this.copy_text(copy_id.clone(), "network ID", cx);
                        }),
                    ));
                    let copy_name = network.name.clone();
                    items = items.child(menu_item(
                        "ctx-network-copy-name",
                        "Copy name",
                        true,
                        false,
                        cx.listener(move |this, _, _, cx| {
                            this.close_context_menu(cx);
                            this.copy_text(copy_name.clone(), "network name", cx);
                        }),
                    ));
                }
            }
            Selection::Machine(name) => {
                let machine = self
                    .local
                    .machines
                    .iter()
                    .find(|item| &item.name == name)
                    .cloned();
                if let Some(machine) = machine {
                    if machine.running {
                        let stop_name = machine.name.clone();
                        items = items.child(menu_item(
                            "ctx-machine-stop",
                            "Stop…",
                            !busy,
                            true,
                            cx.listener(move |this, _, window, cx| {
                                this.close_context_menu(cx);
                                this.open_modal(
                                    Modal::StopMachine {
                                        name: stop_name.clone(),
                                    },
                                    window,
                                    cx,
                                );
                            }),
                        ));
                    } else {
                        let start_name = machine.name.clone();
                        items = items.child(menu_item(
                            "ctx-machine-start",
                            "Start",
                            !busy,
                            false,
                            cx.listener(move |this, _, _, cx| {
                                this.close_context_menu(cx);
                                this.machine_action(start_name.clone(), MachineAction::Start, cx);
                            }),
                        ));
                    }

                    let copy_name = machine.name.clone();
                    items = items.child(menu_item(
                        "ctx-machine-copy-name",
                        "Copy name",
                        true,
                        false,
                        cx.listener(move |this, _, _, cx| {
                            this.close_context_menu(cx);
                            this.copy_text(copy_name.clone(), "machine name", cx);
                        }),
                    ));

                    if machine.running {
                        let ssh_name = machine.name.clone();
                        items = items.child(menu_item(
                            "ctx-machine-terminal",
                            "Terminal (SSH)",
                            !busy,
                            false,
                            cx.listener(move |this, _, window, cx| {
                                this.close_context_menu(cx);
                                this.details_tab = DetailsTab::Terminal;
                                this.start_machine_terminal(ssh_name.clone(), window, cx);
                            }),
                        ));
                    }

                    let delete_name = machine.name.clone();
                    items = items.child(menu_item(
                        "ctx-machine-delete",
                        "Delete…",
                        !busy,
                        true,
                        cx.listener(move |this, _, window, cx| {
                            this.close_context_menu(cx);
                            this.open_modal(
                                Modal::DeleteMachine {
                                    name: delete_name.clone(),
                                },
                                window,
                                cx,
                            );
                        }),
                    ));
                }
            }
            Selection::Connection(name) => {
                let connection = self
                    .local
                    .connections
                    .iter()
                    .find(|item| &item.name == name)
                    .cloned();
                if let Some(connection) = connection {
                    let in_use =
                        self.settings.connection.as_deref() == Some(connection.name.as_str());
                    if !in_use {
                        let use_name = connection.name.clone();
                        items = items.child(menu_item(
                            "ctx-connection-use",
                            "Use in this app",
                            !busy,
                            false,
                            cx.listener(move |this, _, _, cx| {
                                this.close_context_menu(cx);
                                this.use_connection(Some(use_name.clone()), cx);
                            }),
                        ));
                    }
                    if self.settings.connection.is_some() {
                        items = items.child(menu_item(
                            "ctx-connection-use-default",
                            "Use Podman default",
                            !busy,
                            false,
                            cx.listener(move |this, _, _, cx| {
                                this.close_context_menu(cx);
                                this.use_connection(None, cx);
                            }),
                        ));
                    }

                    let uri = connection.uri.clone();
                    items = items.child(menu_item(
                        "ctx-connection-copy-uri",
                        "Copy URI",
                        true,
                        false,
                        cx.listener(move |this, _, _, cx| {
                            this.close_context_menu(cx);
                            this.copy_text(uri.clone(), "connection URI", cx);
                        }),
                    ));
                }
            }
        }

        let position = menu.position;
        div()
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h_full()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w_full()
                    .h_full()
                    .occlude()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.close_context_menu(cx)),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|this, _, _, cx| this.close_context_menu(cx)),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left(position.x)
                    .top(position.y)
                    .w(px(220.))
                    .p_1()
                    .rounded_md()
                    .bg(rgb(theme().panel_bg))
                    .border_1()
                    .border_color(rgb(theme().border))
                    .occlude()
                    .child(items),
            )
    }

    pub(super) fn modal_overlay(&self, cx: &mut Context<Self>) -> Div {
        let Some(modal) = self.modal.as_ref() else {
            return div();
        };

        // The build dialog has its own layout: platform cards, a key/value
        // argument table and collapsible advanced fields.
        if let Modal::BuildImage { form } = modal {
            return self.build_modal_overlay(form, cx);
        }

        let masked_password = match modal {
            Modal::RegistryLogin { password, .. } => "•".repeat(password.chars().count()),
            Modal::CreateSecret { data, .. } => "•".repeat(data.chars().count()),
            _ => String::new(),
        };
        let (title, body, inputs, confirm, danger): ModalSpec = match modal {
            Modal::DeleteContainer { name, .. } => (
                "Delete container?",
                Some(format!(
                    "“{name}” will be force-removed. This cannot be undone."
                )),
                Vec::new(),
                "Delete",
                true,
            ),
            Modal::RenameContainer { name, input, .. } => (
                "Rename container",
                Some(format!("Current name: {name}")),
                vec![("New name", input.as_str(), "type a name…", true)],
                "Rename",
                false,
            ),
            Modal::DeleteImage { name, .. } => (
                "Delete image?",
                Some(format!(
                    "“{name}” will be force-removed. This cannot be undone."
                )),
                Vec::new(),
                "Delete",
                true,
            ),
            Modal::DeletePod { name, .. } => (
                "Delete pod?",
                Some(format!(
                    "Pod “{name}” and its containers will be force-removed. This cannot be undone."
                )),
                Vec::new(),
                "Delete",
                true,
            ),
            Modal::DeleteSecret { name, .. } => (
                "Delete secret?",
                Some(format!(
                    "Secret “{name}” will be removed. This cannot be undone."
                )),
                Vec::new(),
                "Delete",
                true,
            ),
            Modal::DeleteVolume { name } => (
                "Delete volume?",
                Some(format!(
                    "Volume “{name}” will be force-removed. This cannot be undone."
                )),
                Vec::new(),
                "Delete",
                true,
            ),
            Modal::DeleteNetwork { name } => (
                "Delete network?",
                Some(format!(
                    "Network “{name}” will be force-removed. Connected containers may be affected."
                )),
                Vec::new(),
                "Delete",
                true,
            ),
            Modal::ComposeDown { project } => (
                "Stop compose project?",
                Some(format!(
                    "All containers in “{}” will be stopped and removed. Named volumes and images are kept.",
                    project.name
                )),
                Vec::new(),
                "Down",
                true,
            ),
            Modal::StopMachine { name } => (
                "Stop machine?",
                Some(format!(
                    "All containers running on “{name}” will be stopped. This cannot be undone."
                )),
                Vec::new(),
                "Stop",
                true,
            ),
            Modal::PullImage { input } => (
                "Pull image",
                None,
                vec![("Image reference", input.as_str(), "type a reference…", true)],
                "Pull",
                false,
            ),
            Modal::TagImage { input, source, .. } => (
                "Tag image",
                Some(format!("Source: {source}")),
                vec![("New tag", input.as_str(), "type a tag…", true)],
                "Tag",
                false,
            ),
            Modal::BuildImage { .. } => unreachable!("rendered by build_modal_overlay"),
            Modal::RunContainer {
                image,
                name,
                ports,
                env,
                volumes,
                secrets,
                command,
                ..
            } => (
                "Run container",
                Some(format!(
                    "Image: {image}. The container starts detached; use its Terminal tab for an interactive shell."
                )),
                vec![
                    (
                        "Name",
                        name.as_str(),
                        "empty = generated by Podman",
                        false,
                    ),
                    (
                        "Ports",
                        ports.as_str(),
                        "e.g. 8080:80 (space-separated)",
                        false,
                    ),
                    (
                        "Environment",
                        env.as_str(),
                        "KEY=value KEY2=value2",
                        false,
                    ),
                    (
                        "Volumes",
                        volumes.as_str(),
                        "host:/container or volume:/container",
                        false,
                    ),
                    (
                        "Secrets",
                        secrets.as_str(),
                        "name, name:/path (file) or name:VAR (env)",
                        false,
                    ),
                    (
                        "Command",
                        command.as_str(),
                        "empty = image default (space-separated)",
                        false,
                    ),
                ],
                "Run",
                false,
            ),
            Modal::PushImage { image, destination } => (
                "Push image",
                Some(format!("Image: {image}")),
                vec![(
                    "Destination",
                    destination.as_str(),
                    "e.g. quay.io/user/app:latest",
                    true,
                )],
                "Push",
                false,
            ),
            Modal::RegistryLogin {
                registry, username, ..
            } => (
                "Registry login",
                Some(
                    "The password is sent to Podman on stdin, never on the command line."
                        .to_string(),
                ),
                vec![
                    (
                        "Registry",
                        registry.as_str(),
                        "e.g. quay.io or localhost:5000",
                        true,
                    ),
                    ("Username", username.as_str(), "registry user", true),
                    (
                        "Password",
                        masked_password.as_str(),
                        "password",
                        true,
                    ),
                ],
                "Log in",
                false,
            ),
            Modal::RegistryLogout { registry } => (
                "Registry logout",
                Some("Leave the registry empty to log out of all registries.".to_string()),
                vec![(
                    "Registry",
                    registry.as_str(),
                    "empty = all registries",
                    false,
                )],
                "Log out",
                false,
            ),
            Modal::CreateSecret { name, .. } => (
                "Create secret",
                Some(
                    "Runs `podman secret create`. The data is sent to Podman on stdin and is never shown again."
                        .to_string(),
                ),
                vec![
                    ("Name", name.as_str(), "e.g. db-password", true),
                    ("Data", masked_password.as_str(), "secret data", true),
                ],
                "Create",
                false,
            ),
            Modal::CreateVolume { name, driver, .. } => (
                "Create volume",
                Some(
                    "Runs `podman volume create`. The driver is optional and defaults to the engine's own."
                        .to_string(),
                ),
                vec![
                    ("Name", name.as_str(), "e.g. app-data", true),
                    (
                        "Driver",
                        driver.as_str(),
                        "empty = engine default (usually local)",
                        false,
                    ),
                ],
                "Create",
                false,
            ),
            Modal::CreateNetwork {
                name,
                driver,
                subnet,
                gateway,
                ..
            } => (
                "Create network",
                Some(
                    "Runs `podman network create`. Driver, subnet and gateway are optional and default to the engine's own."
                        .to_string(),
                ),
                vec![
                    ("Name", name.as_str(), "e.g. app-net", true),
                    ("Driver", driver.as_str(), "empty = bridge", false),
                    (
                        "Subnet",
                        subnet.as_str(),
                        "empty = auto, e.g. 10.89.0.0/24",
                        false,
                    ),
                    ("Gateway", gateway.as_str(), "empty = auto, e.g. 10.89.0.1", false),
                ],
                "Create",
                false,
            ),
            Modal::CreatePod { name, ports, .. } => (
                "Create pod",
                Some(
                    "Runs `podman pod create`. Port mappings are published by the pod's infra container."
                        .to_string(),
                ),
                vec![
                    ("Name", name.as_str(), "e.g. web", true),
                    (
                        "Ports",
                        ports.as_str(),
                        "empty = none, e.g. 8080:80",
                        false,
                    ),
                ],
                "Create",
                false,
            ),
            Modal::PlayKube { path } => (
                "Play Kubernetes YAML",
                Some(
                    "Runs `podman kube play` and streams its output below. Existing resources may be recreated."
                        .to_string(),
                ),
                vec![(
                    "YAML file",
                    path.as_str(),
                    "/path/to/pod.yaml",
                    true,
                )],
                "Play",
                false,
            ),
            Modal::PruneVolumes => (
                "Prune volumes?",
                Some(
                    "All unused volumes will be removed. Volumes attached to a container are kept."
                        .to_string(),
                ),
                Vec::new(),
                "Prune",
                true,
            ),
            Modal::PruneNetworks => (
                "Prune networks?",
                Some(
                    "All unused networks will be removed. Networks in use by containers are kept."
                        .to_string(),
                ),
                Vec::new(),
                "Prune",
                true,
            ),
            Modal::CreateMachine {
                name,
                cpus,
                memory,
                disk,
                ..
            } => (
                "Create machine",
                Some(
                    "Runs `podman machine init` and streams its output below. Podman downloads the VM image on first use, so this can take several minutes."
                        .to_string(),
                ),
                vec![
                    (
                        "Name",
                        name.as_str(),
                        "empty = podman-machine-default",
                        false,
                    ),
                    ("CPUs", cpus.as_str(), "empty = Podman default (6)", false),
                    (
                        "Memory (MiB)",
                        memory.as_str(),
                        "empty = Podman default (2048)",
                        false,
                    ),
                    (
                        "Disk (GiB)",
                        disk.as_str(),
                        "empty = Podman default (100)",
                        false,
                    ),
                ],
                "Create",
                false,
            ),
            Modal::DeleteMachine { name } => (
                "Delete machine?",
                Some(format!(
                    "Machine “{name}” will be force-removed, including the containers and volumes inside it. This cannot be undone."
                )),
                Vec::new(),
                "Delete",
                true,
            ),
        };

        let focused = match modal {
            Modal::RunContainer { field, .. } => field.index(),
            Modal::RegistryLogin { field, .. } => field.index(),
            Modal::CreateVolume { field, .. } => field.index(),
            Modal::CreateSecret { field, .. } => field.index(),
            Modal::CreateNetwork { field, .. } => field.index(),
            Modal::CreatePod { field, .. } => field.index(),
            Modal::CreateMachine { field, .. } => field.index(),
            _ => 0,
        };
        let streaming = matches!(
            modal,
            Modal::PushImage { .. }
                | Modal::RegistryLogin { .. }
                | Modal::PlayKube { .. }
                | Modal::CreateMachine { .. }
        );
        let enabled = self.busy.is_none()
            && !inputs
                .iter()
                .any(|(_, value, _, required)| *required && value.trim().is_empty())
            && !(streaming && self.output_run.is_some());
        let (confirm_fg, confirm_bg) = if danger {
            (0xfee2e2, 0x7f1d1d)
        } else {
            (theme().accent, theme().accent_soft)
        };

        let mut dialog = div()
            .track_focus(&self.modal_focus)
            .on_key_down(cx.listener(Self::modal_key))
            .w(px(480.))
            .p_5()
            .rounded_lg()
            .bg(rgb(theme().panel_bg))
            .border_1()
            .border_color(rgb(theme().border))
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            );

        if let Some(body) = body {
            dialog = dialog.child(
                div()
                    .mt_3()
                    .text_sm()
                    .text_color(rgb(theme().muted))
                    .child(body),
            );
        }

        for (index, (label, value, placeholder, _required)) in inputs.iter().enumerate() {
            let (display, color) = if value.is_empty() {
                (placeholder.to_string(), theme().faint)
            } else {
                (value.to_string(), theme().text)
            };
            let active = index == focused;
            let id = SharedString::from(format!("modal-input-{index}"));

            // The caret sits before a placeholder (empty value) and after the text
            // otherwise, matching how the field appends characters.
            let mut text_row = div().flex().items_center().gap_1();
            if active && value.is_empty() {
                text_row = text_row.child(caret(self.caret_on));
            }
            text_row = text_row.child(div().text_sm().text_color(rgb(color)).child(display));
            if active && !value.is_empty() {
                text_row = text_row.child(caret(self.caret_on));
            }

            dialog = dialog
                .child(
                    div()
                        .mt_4()
                        .text_xs()
                        .text_color(rgb(theme().faint))
                        .child(*label),
                )
                .child(
                    div()
                        .id(id)
                        .mt_1()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(rgb(theme().bg))
                        .border_1()
                        .border_color(rgb(if active {
                            theme().accent
                        } else {
                            theme().border
                        }))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, window, _cx| {
                                this.modal_focus.focus(window);
                                match &mut this.modal {
                                    Some(Modal::RunContainer { field, .. }) => {
                                        *field = RunField::from_index(index);
                                    }
                                    Some(Modal::RegistryLogin { field, .. }) => {
                                        *field = LoginField::from_index(index);
                                    }
                                    Some(Modal::CreateVolume { field, .. }) => {
                                        *field = VolumeField::from_index(index);
                                    }
                                    Some(Modal::CreateSecret { field, .. }) => {
                                        *field = SecretField::from_index(index);
                                    }
                                    Some(Modal::CreateNetwork { field, .. }) => {
                                        *field = NetworkField::from_index(index);
                                    }
                                    Some(Modal::CreatePod { field, .. }) => {
                                        *field = PodField::from_index(index);
                                    }
                                    Some(Modal::CreateMachine { field, .. }) => {
                                        *field = MachineField::from_index(index);
                                    }
                                    _ => {}
                                }
                            }),
                        )
                        .child(text_row),
                );
        }

        dialog = dialog.child(
            div()
                .flex()
                .justify_end()
                .gap_2()
                .mt_5()
                .child(pill_button(
                    "modal-cancel",
                    "Cancel",
                    true,
                    theme().muted,
                    theme().hover_bg,
                    cx.listener(|this, _, window, cx| this.close_modal(window, cx)),
                ))
                .child(pill_button(
                    "modal-confirm",
                    confirm,
                    enabled,
                    confirm_fg,
                    confirm_bg,
                    cx.listener(|this, _, window, cx| this.confirm_modal(window, cx)),
                )),
        );

        div()
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h_full()
            .occlude()
            .bg(rgba(theme().scrim))
            .flex()
            .items_center()
            .justify_center()
            .child(dialog)
    }

    /// The build dialog, laid out like upstream Podman Desktop: a wide form
    /// with key/value build-argument rows, selectable platform cards and an
    /// expandable advanced section.
    fn build_modal_overlay(&self, form: &BuildForm, cx: &mut Context<Self>) -> Div {
        let enabled = self.busy.is_none()
            && self.output_run.is_none()
            && !form.containerfile.trim().is_empty()
            && !form.tag.trim().is_empty();

        // Keep the form inside the window so the buttons stay reachable on small
        // screens; the fields scroll when they do not fit.
        let body_max = (self.viewport_height - 110.0).clamp(180.0, 680.0);

        let mut body = div()
            .id("build-form")
            .flex()
            .flex_col()
            .max_h(px(body_max))
            .mt_4()
            .pr_2()
            .overflow_y_scroll();

        body = body
            .child(field_label("Containerfile path"))
            .child(form_input(
                "build-containerfile",
                &form.containerfile,
                "Containerfile to build",
                form.field == BuildField::Containerfile,
                self.caret_on,
                cx.listener(|this, _, window, _| {
                    this.modal_focus.focus(window);
                    if let Some(Modal::BuildImage { form }) = &mut this.modal {
                        form.field = BuildField::Containerfile;
                    }
                }),
            ))
            .child(field_label("Image name"))
            .child(form_input(
                "build-tag",
                &form.tag,
                "Image name (e.g. quay.io/namespace/my-custom-image)",
                form.field == BuildField::Tag,
                self.caret_on,
                cx.listener(|this, _, window, _| {
                    this.modal_focus.focus(window);
                    if let Some(Modal::BuildImage { form }) = &mut this.modal {
                        form.field = BuildField::Tag;
                    }
                }),
            ));

        if form.show_options {
            body = body
                .child(field_label("Build context directory"))
                .child(form_input(
                    "build-context",
                    &form.context,
                    "Directory to build in",
                    form.field == BuildField::Context,
                    self.caret_on,
                    cx.listener(|this, _, window, _| {
                        this.modal_focus.focus(window);
                        if let Some(Modal::BuildImage { form }) = &mut this.modal {
                            form.field = BuildField::Context;
                        }
                    }),
                ));

            body = body.child(field_label("Build arguments"));
            for (index, arg) in form.args.iter().enumerate() {
                body = body.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .mt_2()
                        .child(div().flex_1().child(form_input(
                            SharedString::from(format!("build-arg-key-{index}")),
                            &arg.key,
                            "Key",
                            form.field == BuildField::ArgKey(index),
                            self.caret_on,
                            cx.listener(move |this, _, window, _| {
                                this.modal_focus.focus(window);
                                if let Some(Modal::BuildImage { form }) = &mut this.modal {
                                    form.field = BuildField::ArgKey(index);
                                }
                            }),
                        )))
                        .child(div().flex_1().child(form_input(
                            SharedString::from(format!("build-arg-value-{index}")),
                            &arg.value,
                            "Value",
                            form.field == BuildField::ArgValue(index),
                            self.caret_on,
                            cx.listener(move |this, _, window, _| {
                                this.modal_focus.focus(window);
                                if let Some(Modal::BuildImage { form }) = &mut this.modal {
                                    form.field = BuildField::ArgValue(index);
                                }
                            }),
                        )))
                        .child(circle_button(
                            SharedString::from(format!("build-arg-remove-{index}")),
                            "−",
                            form.args.len() > 1,
                            cx.listener(move |this, _, window, _| {
                                if let Some(Modal::BuildImage { form }) = &mut this.modal {
                                    if form.args.len() > 1 {
                                        form.args.remove(index);
                                        form.field = match form.field {
                                            BuildField::ArgKey(row) | BuildField::ArgValue(row)
                                                if row == index =>
                                            {
                                                BuildField::Tag
                                            }
                                            BuildField::ArgKey(row) if row > index => {
                                                BuildField::ArgKey(row - 1)
                                            }
                                            BuildField::ArgValue(row) if row > index => {
                                                BuildField::ArgValue(row - 1)
                                            }
                                            other => other,
                                        };
                                    }
                                }
                                this.modal_focus.focus(window);
                            }),
                        ))
                        .child(circle_button(
                            SharedString::from(format!("build-arg-add-{index}")),
                            "+",
                            true,
                            cx.listener(move |this, _, window, _| {
                                if let Some(Modal::BuildImage { form }) = &mut this.modal {
                                    form.args.insert(index + 1, BuildArg::default());
                                    form.field = BuildField::ArgKey(index + 1);
                                }
                                this.modal_focus.focus(window);
                            }),
                        )),
                );
            }

            body = body.child(field_label("Platform"));
            let mut cards: Vec<AnyElement> = Vec::new();
            let default_platform = default_platform();
            for preset in &BUILD_PLATFORMS {
                let platform = preset.platform;
                cards.push(platform_card(
                    SharedString::from(format!("build-platform-{}", preset.arch)),
                    preset.description,
                    preset.arch,
                    preset.platform == default_platform,
                    form.selected(preset.platform),
                    cx.listener(move |this, _, window, _| {
                        if let Some(Modal::BuildImage { form }) = &mut this.modal {
                            form.toggle_platform(platform);
                        }
                        this.modal_focus.focus(window);
                    }),
                ));
            }
            for platform in &form.custom_platforms {
                let value = platform.clone();
                cards.push(platform_card(
                    SharedString::from(format!("build-platform-{platform}")),
                    platform,
                    &platform_arch(platform),
                    false,
                    form.selected(platform),
                    cx.listener(move |this, _, window, _| {
                        if let Some(Modal::BuildImage { form }) = &mut this.modal {
                            form.toggle_platform(&value);
                        }
                        this.modal_focus.focus(window);
                    }),
                ));
            }

            if form.new_platform_open {
                let active = form.field == BuildField::NewPlatform;
                cards.push(
                    div()
                        .flex_shrink_0()
                        .w(px(190.))
                        .p_3()
                        .rounded_md()
                        .bg(rgb(theme().bg))
                        .border_1()
                        .border_color(rgb(if active {
                            theme().accent
                        } else {
                            theme().border
                        }))
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(theme().faint))
                                .child("New platform"),
                        )
                        .child(form_input(
                            "build-new-platform",
                            &form.new_platform,
                            "e.g. linux/riscv64",
                            active,
                            self.caret_on,
                            cx.listener(|this, _, window, _| {
                                this.modal_focus.focus(window);
                                if let Some(Modal::BuildImage { form }) = &mut this.modal {
                                    form.field = BuildField::NewPlatform;
                                }
                            }),
                        ))
                        .child(
                            div()
                                .mt_2()
                                .text_xs()
                                .text_color(rgb(theme().faint))
                                .child("Enter to add"),
                        )
                        .into_any_element(),
                );
            } else {
                cards.push(
                    div()
                        .id("build-new-platform")
                        .flex_shrink_0()
                        .w(px(190.))
                        .h(px(92.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .rounded_md()
                        .border_1()
                        .border_color(rgb(theme().border))
                        .bg(rgb(theme().bg))
                        .cursor_pointer()
                        .hover(|style| style.border_color(rgb(theme().accent)))
                        .on_click(cx.listener(|this, _, window, _| {
                            this.modal_focus.focus(window);
                            if let Some(Modal::BuildImage { form }) = &mut this.modal {
                                form.new_platform_open = true;
                                form.field = BuildField::NewPlatform;
                            }
                        }))
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(theme().accent))
                                .child("+ New platform"),
                        )
                        .into_any_element(),
                );
            }

            let mut grid = div().flex().flex_col();
            let mut cards = cards.into_iter();
            loop {
                let mut row = div().flex().gap_2().mt_2();
                let mut any = false;
                for _ in 0..3 {
                    if let Some(card) = cards.next() {
                        row = row.child(card);
                        any = true;
                    }
                }
                if !any {
                    break;
                }
                grid = grid.child(row);
            }
            body = body.child(grid);

            body = body.child(field_label("Registry Validation")).child(
                div()
                    .id("build-validate-registries")
                    .mt_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(Modal::BuildImage { form }) = &mut this.modal {
                            form.validate_registries = !form.validate_registries;
                        }
                        cx.notify();
                    }))
                    .child(checkbox(form.validate_registries))
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(theme().text))
                            .child("Validate registries before building"),
                    ),
            );
        }

        body = body.child(
            div()
                .id("build-more-options")
                .mt_3()
                .flex()
                .items_center()
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(Modal::BuildImage { form }) = &mut this.modal {
                        form.show_options = !form.show_options;
                        if !form.show_options {
                            form.new_platform_open = false;
                            form.field = BuildField::Tag;
                        }
                    }
                    cx.notify();
                }))
                .child(div().text_sm().text_color(rgb(theme().muted)).child(
                    if form.show_options {
                        "Less options…"
                    } else {
                        "More options…"
                    },
                )),
        );

        let dialog = div()
            .track_focus(&self.modal_focus)
            .on_key_down(cx.listener(Self::modal_key))
            .w(px(660.))
            .flex()
            .flex_col()
            .p_5()
            .rounded_lg()
            .bg(rgb(theme().panel_bg))
            .border_1()
            .border_color(rgb(theme().border))
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Build image from Containerfile"),
            )
            .child(div().mt_1().text_sm().text_color(rgb(theme().muted)).child(
                "Runs `podman build`; the context defaults to the Containerfile's directory.",
            ))
            .child(body)
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .mt_5()
                    .child(pill_button(
                        "modal-cancel",
                        "Cancel",
                        true,
                        theme().muted,
                        theme().hover_bg,
                        cx.listener(|this, _, window, cx| this.close_modal(window, cx)),
                    ))
                    .child(pill_button(
                        "modal-confirm",
                        "Build",
                        enabled,
                        theme().accent,
                        theme().accent_soft,
                        cx.listener(|this, _, window, cx| this.confirm_modal(window, cx)),
                    )),
            );

        div()
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h_full()
            .occlude()
            .bg(rgba(theme().scrim))
            .flex()
            .items_center()
            .justify_center()
            .child(dialog)
    }
}

fn field_label(text: &'static str) -> Div {
    div()
        .mt_3()
        .text_xs()
        .text_color(rgb(theme().faint))
        .child(text)
}

/// Text input used by the build dialog.
fn form_input(
    id: impl Into<SharedString>,
    value: &str,
    placeholder: &str,
    active: bool,
    caret_on: bool,
    on_mouse_down: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let (display, color) = if value.is_empty() {
        (placeholder.to_string(), theme().faint)
    } else {
        (value.to_string(), theme().text)
    };
    // The caret sits before a placeholder (empty value) and after the text
    // otherwise, matching how the field appends characters.
    let mut text_row = div().flex().items_center().gap_1();
    if active && value.is_empty() {
        text_row = text_row.child(caret(caret_on));
    }
    text_row = text_row.child(div().text_sm().text_color(rgb(color)).child(display));
    if active && !value.is_empty() {
        text_row = text_row.child(caret(caret_on));
    }

    div()
        .id(id.into())
        .mt_1()
        .px_3()
        .py_2()
        .rounded_md()
        .bg(rgb(theme().bg))
        .border_1()
        .border_color(rgb(if active {
            theme().accent
        } else {
            theme().border
        }))
        .on_mouse_down(MouseButton::Left, on_mouse_down)
        .child(text_row)
        .into_any_element()
}

fn checkbox(checked: bool) -> Div {
    div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .w(px(16.))
        .h(px(16.))
        .rounded_sm()
        .border_1()
        .border_color(rgb(if checked {
            theme().accent
        } else {
            theme().faint
        }))
        .when(checked, |el| el.bg(rgb(theme().accent)))
        .child(
            div()
                .text_xs()
                .text_color(rgb(theme().on_accent))
                .child(if checked { "✓" } else { "" }),
        )
}

fn platform_chip(label: &str) -> Div {
    div()
        .px_2()
        .py(px(1.))
        .rounded_full()
        .border_1()
        .border_color(rgb(theme().border))
        .bg(rgb(theme().hover_bg))
        .child(
            div()
                .text_xs()
                .text_color(rgb(theme().muted))
                .child(label.to_string()),
        )
}

fn platform_card(
    id: impl Into<SharedString>,
    description: &str,
    arch: &str,
    default: bool,
    selected: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(id.into())
        .flex_shrink_0()
        .w(px(190.))
        .h(px(92.))
        .p_3()
        .rounded_md()
        .bg(rgb(theme().bg))
        .border_1()
        .border_color(rgb(if selected {
            theme().accent
        } else {
            theme().border
        }))
        .cursor_pointer()
        .hover(|style| style.border_color(rgb(theme().accent)))
        .on_click(on_click)
        .child(
            div()
                .flex()
                .items_start()
                .gap_2()
                .child(checkbox(selected))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_sm()
                        .text_color(rgb(theme().text))
                        .whitespace_normal()
                        .child(description.to_string()),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .mt_2()
                .child(platform_chip(arch))
                .when(default, |el| el.child(platform_chip("Default"))),
        )
        .into_any_element()
}

fn circle_button(
    id: impl Into<SharedString>,
    glyph: &'static str,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(id.into())
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .w(px(26.))
        .h(px(26.))
        .rounded_full()
        .when(enabled, |el| {
            el.bg(rgb(theme().accent_soft))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(theme().hover_bg)))
                .on_click(on_click)
        })
        .child(
            div()
                .text_sm()
                .text_color(rgb(if enabled {
                    theme().accent
                } else {
                    theme().faint
                }))
                .child(glyph),
        )
        .into_any_element()
}

/// `linux/riscv64` → `RISCV64`; used for the chip on custom platform cards.
fn platform_arch(platform: &str) -> String {
    platform
        .split('/')
        .nth(1)
        .unwrap_or(platform)
        .to_uppercase()
}
