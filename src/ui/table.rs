use super::helpers::*;
use super::*;

impl Selkie {
    // Header, toolbar, table
    // -----------------------------------------------------------------------

    pub(super) fn header(&self, cx: &mut Context<Self>) -> Div {
        let title = match self.tab {
            Tab::Dashboard => "Dashboard",
            Tab::Containers => "Containers",
            Tab::Compose => "Compose",
            Tab::Images => "Images",
            Tab::Pods => "Pods",
            Tab::Volumes => "Volumes",
            Tab::Networks => "Networks",
            Tab::Secrets => "Secrets",
            Tab::Machines => "Machines",
            Tab::Connections => "Connections",
            Tab::Accounts => "Accounts",
            Tab::Settings => "Settings",
        };

        div()
            .flex()
            .items_center()
            .justify_between()
            .px_6()
            .pt_5()
            .pb_3()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(theme().muted))
                            .child(self.subtitle()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(theme().faint))
                            .child(self.refresh_label()),
                    )
                    .child(pill_button(
                        "refresh",
                        "Refresh",
                        true,
                        theme().accent,
                        theme().accent_soft,
                        cx.listener(|this, _, _, cx| this.refresh_dashboard(cx)),
                    )),
            )
    }

    fn search_box(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.search_focus.is_focused(window);
        let placeholder = match self.tab {
            Tab::Dashboard => "Search…",
            Tab::Containers => "Search containers…",
            Tab::Compose => "Search projects…",
            Tab::Images => "Search images…",
            Tab::Pods => "Search pods…",
            Tab::Volumes => "Search volumes…",
            Tab::Networks => "Search networks…",
            Tab::Secrets => "Search secrets…",
            Tab::Machines => "Search machines…",
            Tab::Connections => "Search connections…",
            Tab::Accounts | Tab::Settings => "Search…",
        };

        let mut input = div()
            .id("search")
            .flex()
            .items_center()
            .gap_2()
            .flex_1()
            .max_w(px(360.))
            .min_w(px(160.))
            .h(px(32.))
            .px_3()
            .rounded_md()
            .bg(rgb(theme().panel_bg))
            .border_1()
            .border_color(if focused {
                rgb(theme().accent)
            } else {
                rgb(theme().border)
            })
            .track_focus(&self.search_focus)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, _cx| {
                    this.caret_on = true;
                    this.search_focus.focus(window);
                }),
            )
            .on_key_down(cx.listener(Self::search_key));

        if self.search.is_empty() {
            input = input.child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .gap_1()
                    .when(focused, |el| el.child(caret(self.caret_on)))
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(theme().faint))
                            .child(placeholder),
                    ),
            );
        } else {
            input = input.child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_sm()
                            .whitespace_nowrap()
                            .truncate()
                            .child(self.search.clone()),
                    )
                    .when(focused, |el| el.child(caret(self.caret_on))),
            );
        }

        if !self.search.is_empty() {
            input = input.child(
                div()
                    .id("search-clear")
                    .flex_shrink_0()
                    .px_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(theme().hover_bg)))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.search.clear();
                        cx.notify();
                    }))
                    .child(div().text_xs().text_color(rgb(theme().muted)).child("×")),
            );
        }

        input
    }

    pub(super) fn toolbar(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let matches = self.visible_indices.len();
        let total = match self.tab {
            Tab::Dashboard => 0,
            Tab::Containers => self.snapshot.containers.len(),
            Tab::Compose => self.snapshot.compose.len(),
            Tab::Images => self.snapshot.images.len(),
            Tab::Pods => self.snapshot.pods.len(),
            Tab::Volumes => self.snapshot.volumes.len(),
            Tab::Networks => self.snapshot.networks.len(),
            Tab::Secrets => self.snapshot.secrets.len(),
            Tab::Machines => self.local.machines.len(),
            Tab::Connections => self.local.connections.len(),
            Tab::Accounts => self.accounts.len(),
            Tab::Settings => 0,
        };
        let filtered = !self.search.trim().is_empty();
        let busy = self.busy.is_some();

        div()
            .flex()
            .items_center()
            .justify_between()
            .px_6()
            .pb_3()
            .child(self.search_box(window, cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .when(filtered, |el| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(rgb(theme().faint))
                                .child(format!("{matches} of {total}")),
                        )
                    })
                    .when(self.tab == Tab::Images, |el| {
                        el.child(pill_button(
                            "build-image",
                            "Build image",
                            !busy && self.output_run.is_none(),
                            theme().muted,
                            theme().hover_bg,
                            cx.listener(|this, _, window, cx| {
                                this.open_modal(
                                    Modal::BuildImage {
                                        form: BuildForm::default(),
                                    },
                                    window,
                                    cx,
                                )
                            }),
                        ))
                        .child(pill_button(
                            "pull-image",
                            "Pull image",
                            !busy,
                            theme().accent,
                            theme().accent_soft,
                            cx.listener(|this, _, window, cx| {
                                this.open_modal(
                                    Modal::PullImage {
                                        input: String::new(),
                                    },
                                    window,
                                    cx,
                                )
                            }),
                        ))
                        .child(pill_button(
                            "login-registry",
                            "Login",
                            !busy && self.output_run.is_none(),
                            theme().accent,
                            theme().accent_soft,
                            cx.listener(|this, _, window, cx| {
                                this.open_modal(
                                    Modal::RegistryLogin {
                                        registry: String::new(),
                                        username: String::new(),
                                        password: String::new(),
                                        field: LoginField::Registry,
                                    },
                                    window,
                                    cx,
                                )
                            }),
                        ))
                        .child(pill_button(
                            "logout-registry",
                            "Logout",
                            !busy,
                            theme().muted,
                            theme().hover_bg,
                            cx.listener(|this, _, window, cx| {
                                this.open_modal(
                                    Modal::RegistryLogout {
                                        registry: String::new(),
                                    },
                                    window,
                                    cx,
                                )
                            }),
                        ))
                    })
                    .when(self.tab == Tab::Pods, |el| {
                        el.child(pill_button(
                            "create-pod",
                            "Create pod",
                            !busy,
                            theme().accent,
                            theme().accent_soft,
                            cx.listener(|this, _, window, cx| {
                                this.open_modal(Modal::create_pod(), window, cx)
                            }),
                        ))
                        .child(pill_button(
                            "play-kube",
                            "Play kube",
                            !busy && self.output_run.is_none(),
                            theme().muted,
                            theme().hover_bg,
                            cx.listener(|this, _, window, cx| {
                                this.open_modal(
                                    Modal::PlayKube {
                                        path: String::new(),
                                    },
                                    window,
                                    cx,
                                )
                            }),
                        ))
                    })
                    .when(self.tab == Tab::Volumes, |el| {
                        el.child(pill_button(
                            "create-volume",
                            "Create volume",
                            !busy,
                            theme().accent,
                            theme().accent_soft,
                            cx.listener(|this, _, window, cx| {
                                this.open_modal(Modal::create_volume(), window, cx)
                            }),
                        ))
                        .child(pill_button(
                            "prune-volumes",
                            "Prune",
                            !busy,
                            theme().muted,
                            theme().hover_bg,
                            cx.listener(|this, _, window, cx| {
                                this.open_modal(Modal::PruneVolumes, window, cx)
                            }),
                        ))
                    })
                    .when(self.tab == Tab::Secrets, |el| {
                        el.child(pill_button(
                            "create-secret",
                            "Create secret",
                            !busy,
                            theme().accent,
                            theme().accent_soft,
                            cx.listener(|this, _, window, cx| {
                                this.open_modal(Modal::create_secret(), window, cx)
                            }),
                        ))
                    })
                    .when(self.tab == Tab::Networks, |el| {
                        el.child(pill_button(
                            "create-network",
                            "Create network",
                            !busy,
                            theme().accent,
                            theme().accent_soft,
                            cx.listener(|this, _, window, cx| {
                                this.open_modal(Modal::create_network(), window, cx)
                            }),
                        ))
                        .child(pill_button(
                            "prune-networks",
                            "Prune",
                            !busy,
                            theme().muted,
                            theme().hover_bg,
                            cx.listener(|this, _, window, cx| {
                                this.open_modal(Modal::PruneNetworks, window, cx)
                            }),
                        ))
                    })
                    .when(self.tab == Tab::Machines, |el| {
                        el.child(pill_button(
                            "create-machine",
                            "Create machine",
                            !busy && self.output_run.is_none(),
                            theme().accent,
                            theme().accent_soft,
                            cx.listener(|this, _, window, cx| {
                                this.open_modal(Modal::create_machine(), window, cx)
                            }),
                        ))
                    })
                    .when(self.tab == Tab::Connections, |el| {
                        el.child(pill_button(
                            "create-connection",
                            "Add connection",
                            !busy,
                            theme().accent,
                            theme().accent_soft,
                            cx.listener(|this, _, window, cx| {
                                this.open_modal(Modal::create_connection(), window, cx)
                            }),
                        ))
                        .child(pill_button(
                            "check-connections",
                            "Check all",
                            !busy,
                            theme().muted,
                            theme().hover_bg,
                            cx.listener(|this, _, _, cx| this.check_connections(true, cx)),
                        ))
                    })
                    .when(
                        self.tab == Tab::Connections && self.settings.connection.is_some(),
                        |el| {
                            el.child(pill_button(
                                "use-default-connection",
                                "Use Podman default",
                                !busy,
                                theme().muted,
                                theme().hover_bg,
                                cx.listener(|this, _, _, cx| this.use_connection(None, cx)),
                            ))
                        },
                    ),
            )
    }

    pub(super) fn table_header(&self, cx: &mut Context<Self>) -> Div {
        let compact = self.selection.is_some();
        let header = div()
            .flex()
            .items_center()
            .h(px(32.))
            .px_4()
            .bg(rgb(theme().panel_bg))
            .border_b_1()
            .border_color(rgb(theme().border))
            .text_xs()
            .text_color(rgb(theme().muted))
            .font_weight(FontWeight::SEMIBOLD);

        match (self.tab, compact) {
            (Tab::Dashboard | Tab::Accounts | Tab::Settings, _) => header,
            (Tab::Containers, false) => header
                .child(self.sortable_header("NAME", 0, None, cx))
                .child(self.sortable_header("IMAGE", 1, None, cx))
                .child(self.sortable_header("STATE", 2, Some(104.), cx))
                .child(self.sortable_header("PORTS", 3, Some(140.), cx))
                .child(self.sortable_header("STATUS", 4, Some(160.), cx))
                .child(self.sortable_header("ID", 5, Some(104.), cx)),
            (Tab::Containers, true) => header
                .child(self.sortable_header("NAME", 0, None, cx))
                .child(self.sortable_header("IMAGE", 1, None, cx))
                .child(self.sortable_header("STATE", 2, Some(104.), cx))
                .child(self.sortable_header("PORTS", 3, Some(140.), cx)),
            (Tab::Compose, false) => header
                .child(self.sortable_header("PROJECT", 0, None, cx))
                .child(self.sortable_header("STATUS", 1, Some(120.), cx))
                .child(self.sortable_header("SERVICES", 2, Some(100.), cx))
                .child(self.sortable_header("CONFIG", 3, Some(220.), cx)),
            (Tab::Compose, true) => header
                .child(self.sortable_header("PROJECT", 0, None, cx))
                .child(self.sortable_header("STATUS", 1, Some(120.), cx))
                .child(self.sortable_header("SERVICES", 2, Some(100.), cx)),
            (Tab::Images, false) => header
                .child(self.sortable_header("IMAGE", 0, None, cx))
                .child(self.sortable_header("IMAGE ID", 1, Some(104.), cx))
                .child(self.sortable_header("SIZE", 2, Some(90.), cx))
                .child(self.sortable_header("IN USE", 3, Some(104.), cx))
                .child(self.sortable_header("CREATED", 4, Some(150.), cx)),
            (Tab::Images, true) => header
                .child(self.sortable_header("IMAGE", 0, None, cx))
                .child(self.sortable_header("SIZE", 2, Some(90.), cx))
                .child(self.sortable_header("IN USE", 3, Some(104.), cx)),
            (Tab::Pods, false) => header
                .child(self.sortable_header("NAME", 0, None, cx))
                .child(self.sortable_header("STATUS", 1, Some(110.), cx))
                .child(self.sortable_header("CONTAINERS", 2, Some(110.), cx))
                .child(self.sortable_header("CREATED", 3, Some(150.), cx))
                .child(self.sortable_header("ID", 4, Some(110.), cx)),
            (Tab::Pods, true) => header
                .child(self.sortable_header("NAME", 0, None, cx))
                .child(self.sortable_header("STATUS", 1, Some(110.), cx))
                .child(self.sortable_header("CONTAINERS", 2, Some(110.), cx)),
            (Tab::Volumes, false) => header
                .child(self.sortable_header("NAME", 0, None, cx))
                .child(self.sortable_header("DRIVER", 1, Some(100.), cx))
                .child(self.sortable_header("MOUNTS", 2, Some(90.), cx))
                .child(self.sortable_header("SCOPE", 3, Some(90.), cx))
                .child(self.sortable_header("CREATED", 4, Some(150.), cx)),
            (Tab::Volumes, true) => header
                .child(self.sortable_header("NAME", 0, None, cx))
                .child(self.sortable_header("DRIVER", 1, Some(100.), cx))
                .child(self.sortable_header("MOUNTS", 2, Some(90.), cx)),
            (Tab::Networks, false) => header
                .child(self.sortable_header("NAME", 0, None, cx))
                .child(self.sortable_header("DRIVER", 1, Some(100.), cx))
                .child(self.sortable_header("SUBNET", 2, Some(170.), cx))
                .child(self.sortable_header("INTERFACE", 3, Some(110.), cx))
                .child(self.sortable_header("DNS", 4, Some(80.), cx)),
            (Tab::Networks, true) => header
                .child(self.sortable_header("NAME", 0, None, cx))
                .child(self.sortable_header("DRIVER", 1, Some(100.), cx))
                .child(self.sortable_header("SUBNET", 2, Some(170.), cx)),
            (Tab::Secrets, false) => header
                .child(self.sortable_header("NAME", 0, None, cx))
                .child(self.sortable_header("DRIVER", 1, Some(100.), cx))
                .child(self.sortable_header("CREATED", 2, Some(150.), cx))
                .child(self.sortable_header("UPDATED", 3, Some(150.), cx)),
            (Tab::Secrets, true) => header
                .child(self.sortable_header("NAME", 0, None, cx))
                .child(self.sortable_header("CREATED", 2, Some(150.), cx)),
            (Tab::Machines, false) => header
                .child(self.sortable_header("NAME", 0, None, cx))
                .child(self.sortable_header("STATE", 1, Some(110.), cx))
                .child(self.sortable_header("CPUS", 2, Some(80.), cx))
                .child(self.sortable_header("MEMORY", 3, Some(100.), cx))
                .child(self.sortable_header("DISK", 4, Some(110.), cx))
                .child(self.sortable_header("LAST UP", 5, Some(170.), cx)),
            (Tab::Machines, true) => header
                .child(self.sortable_header("NAME", 0, None, cx))
                .child(self.sortable_header("STATE", 1, Some(110.), cx))
                .child(self.sortable_header("CPUS", 2, Some(80.), cx)),
            (Tab::Connections, false) => header
                .child(self.sortable_header("NAME", 0, None, cx))
                .child(self.sortable_header("URI", 1, None, cx))
                .child(self.sortable_header("IDENTITY", 2, Some(220.), cx))
                .child(self.sortable_header("STATUS", 3, Some(120.), cx)),
            (Tab::Connections, true) => header
                .child(self.sortable_header("NAME", 0, None, cx))
                .child(self.sortable_header("STATUS", 3, Some(120.), cx)),
        }
    }

    /// A clickable header cell. `column` is the per-tab data column (not the
    /// display position), so compact tables can hide columns safely.
    fn sortable_header(
        &self,
        label: &'static str,
        column: usize,
        width: Option<f32>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let state = self.sort.filter(|state| state.column == column);
        let marker = match state {
            Some(state) if state.ascending => " ▲",
            Some(_) => " ▼",
            None => "",
        };
        let cell = div()
            .id(SharedString::from(format!("sort-{column}")))
            .flex()
            .items_center()
            .cursor_pointer()
            .when(state.is_some(), |el| el.text_color(rgb(theme().accent)))
            .hover(|style| style.text_color(rgb(theme().text)))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_sort(column, cx)))
            .child(format!("{label}{marker}"));
        match width {
            Some(width) => cell.w(px(width)),
            None => cell.flex_1(),
        }
    }

    pub(super) fn body(&self, cx: &mut Context<Self>) -> Div {
        let count = self.rows_len();

        div()
            .flex_1()
            .min_h(px(0.))
            .overflow_hidden()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, _cx| this.root_focus.focus(window)),
            )
            .child(if count == 0 {
                self.empty_state(cx).into_any_element()
            } else {
                uniform_list(
                    "rows",
                    count,
                    cx.processor(|this, range: std::ops::Range<usize>, _window, cx| {
                        range.map(|ix| this.table_row(ix, cx)).collect::<Vec<_>>()
                    }),
                )
                .track_scroll(self.rows_scroll.clone())
                .h_full()
                .into_any_element()
            })
    }

    fn empty_state(&self, cx: &mut Context<Self>) -> Div {
        let searching = !self.search.trim().is_empty();
        let (title, message, action) = if searching {
            (
                "No matches",
                format!("Nothing matches “{}”.", self.search.trim()),
                "Clear search",
            )
        } else {
            match &self.error {
                Some(error) => ("Cannot reach the Podman engine", error.clone(), "Retry"),
                None if self.loading => {
                    ("Loading…", "Asking podman for data.".to_string(), "Retry")
                }
                None => match self.tab {
                    Tab::Dashboard => (
                        "Nothing to show",
                        "The dashboard renders resource and engine details instead of a table."
                            .to_string(),
                        "Retry",
                    ),
                    Tab::Containers => (
                        "No containers",
                        "Nothing is running or stopped on this engine yet.".to_string(),
                        "Retry",
                    ),
                    Tab::Compose => (
                        "No compose projects",
                        "Start a project with `podman compose up -d` and its containers will show up here."
                            .to_string(),
                        "Retry",
                    ),
                    Tab::Images => (
                        "No images",
                        "Pull an image with the button above and it will show up here.".to_string(),
                        "Retry",
                    ),
                    Tab::Pods => (
                        "No pods",
                        "Create a pod with the button above, or play a Kubernetes YAML file."
                            .to_string(),
                        "Retry",
                    ),
                    Tab::Volumes => (
                        "No volumes",
                        "Create a volume with the button above and it will show up here."
                            .to_string(),
                        "Retry",
                    ),
                    Tab::Networks => (
                        "No networks",
                        "Create a network with the button above and it will show up here."
                            .to_string(),
                        "Retry",
                    ),
                    Tab::Secrets => (
                        "No secrets",
                        "Create one with the button above, or from a terminal: printf \"secretdata\" | podman secret create mysecret -"
                            .to_string(),
                        "Retry",
                    ),
                    Tab::Machines => (
                        "No machines",
                        "Create a Podman machine with the button above, then start it to run containers inside it."
                            .to_string(),
                        "Retry",
                    ),
                    Tab::Connections => (
                        "No connections",
                        "The app is using Podman's default engine. Add an SSH or Unix-socket connection here, or with `podman system connection add`."
                            .to_string(),
                        "Retry",
                    ),
                    Tab::Accounts => (
                        "No registry accounts",
                        "Log in to a registry to push or pull private images.".to_string(),
                        "Retry",
                    ),
                    Tab::Settings => (
                        "Nothing to show",
                        "Settings renders its own page instead of a table.".to_string(),
                        "Retry",
                    ),
                },
            }
        };

        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .h_full()
            .gap_3()
            .px_6()
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(theme().muted))
                    .text_center()
                    .max_w(px(460.))
                    .child(message),
            )
            .child(pill_button(
                "retry",
                action,
                true,
                theme().accent,
                theme().accent_soft,
                cx.listener(move |this, _, _, cx| {
                    if searching {
                        this.search.clear();
                    } else {
                        this.refresh(cx);
                    }
                    cx.notify();
                }),
            ))
    }
}
