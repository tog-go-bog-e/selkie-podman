use super::helpers::*;
use super::*;

impl PodmanDesktop {
    // -----------------------------------------------------------------------
    // Registry accounts
    // -----------------------------------------------------------------------

    /// Read the credentials Podman would use. Cheap enough to redo on tab
    /// switches and refreshes: it only parses the local auth file.
    pub(super) fn load_accounts(&mut self) {
        match podman::accounts() {
            Ok(accounts) => {
                self.accounts = accounts;
                self.accounts_error = None;
            }
            Err(error) => self.accounts_error = Some(error),
        }
    }

    pub(super) fn open_account_menu(
        &mut self,
        position: Point<Pixels>,
        registry: String,
        cx: &mut Context<Self>,
    ) {
        self.account_menu = Some(AccountMenuState { position, registry });
        cx.notify();
    }

    pub(super) fn close_account_menu(&mut self, cx: &mut Context<Self>) {
        if self.account_menu.take().is_some() {
            cx.notify();
        }
    }

    /// The `⋮` menu on a credential row.
    pub(super) fn account_menu_view(&self, cx: &mut Context<Self>) -> Div {
        let Some((position, registry)) = self
            .account_menu
            .as_ref()
            .map(|menu| (menu.position, menu.registry.clone()))
        else {
            return div();
        };

        // Keep the menu inside the window when the row is near the right edge.
        let left = f32::from(position.x).min((self.viewport_width - 208.0).max(0.0));

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
                        cx.listener(|this, _, _, cx| this.close_account_menu(cx)),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|this, _, _, cx| this.close_account_menu(cx)),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left(px(left))
                    .top(position.y)
                    .w(px(200.))
                    .p_1()
                    .rounded_md()
                    .bg(rgb(theme().panel_bg))
                    .border_1()
                    .border_color(rgb(theme().border))
                    .occlude()
                    .child(menu_item(
                        "account-menu-logout",
                        "Log out",
                        true,
                        false,
                        cx.listener(move |this, _, window, cx| {
                            this.close_account_menu(cx);
                            this.open_modal(
                                Modal::RegistryLogout {
                                    registry: registry.clone(),
                                },
                                window,
                                cx,
                            );
                        }),
                    )),
            )
    }

    /// Credential table plus the well-known registry providers. Shared by the
    /// Accounts tab and the Registries pane of Settings.
    pub(super) fn registries_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut content = div()
            .id("registries")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .px_6()
            .pb_6()
            .flex()
            .flex_col()
            .gap_3();

        content = content.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .mt_2()
                .child(
                    div()
                        .text_base()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Registries"),
                )
                .child(pill_button(
                    "registry-add",
                    "Add registry",
                    true,
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
                        );
                    }),
                )),
        );

        if let Some(error) = &self.accounts_error {
            content = content.child(
                div()
                    .text_sm()
                    .text_color(rgb(theme().red))
                    .child(error.clone()),
            );
        }

        let mut table = div()
            .rounded_lg()
            .border_1()
            .border_color(rgb(theme().border))
            .bg(rgb(theme().panel_bg))
            .flex()
            .flex_col();

        table = table.child(
            div()
                .flex()
                .items_center()
                .h(px(34.))
                .px_4()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(theme().muted))
                .child(div().flex_1().child("REGISTRY LOCATION"))
                .child(div().w(px(200.)).child("USERNAME"))
                .child(div().w(px(150.)).child("PASSWORD"))
                .child(div().w(px(32.))),
        );

        if self.accounts.is_empty() {
            table = table.child(
                div()
                    .border_t_1()
                    .border_color(rgb(theme().border))
                    .px_4()
                    .py_4()
                    .text_sm()
                    .text_color(rgb(theme().muted))
                    .child("No registries are configured. Add one to push or pull private images."),
            );
        } else {
            for (index, account) in self.accounts.iter().enumerate() {
                let registry = account.registry.clone();
                table = table.child(
                    div()
                        .flex()
                        .items_center()
                        .h(px(44.))
                        .px_4()
                        .border_t_1()
                        .border_color(rgb(theme().border))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .text_sm()
                                .text_color(rgb(theme().text))
                                .truncate()
                                .child(account.registry.clone()),
                        )
                        .child(
                            div()
                                .w(px(200.))
                                .min_w(px(0.))
                                .text_sm()
                                .text_color(rgb(theme().muted))
                                .truncate()
                                .child(account.username.clone()),
                        )
                        .child(
                            div()
                                .w(px(150.))
                                .text_xs()
                                .text_color(rgb(theme().faint))
                                .child(account.kind.masked()),
                        )
                        .child(
                            div().w(px(32.)).flex().justify_end().child(
                                div()
                                    .id(SharedString::from(format!("account-menu-{index}")))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .w(px(26.))
                                    .h(px(26.))
                                    .rounded_sm()
                                    .cursor_pointer()
                                    .text_sm()
                                    .text_color(rgb(theme().muted))
                                    .hover(|style| style.bg(rgb(theme().hover_bg)))
                                    .on_click(cx.listener(
                                        move |this, event: &ClickEvent, _, cx| {
                                            this.open_account_menu(
                                                event.position(),
                                                registry.clone(),
                                                cx,
                                            );
                                        },
                                    ))
                                    .child("⋮"),
                            ),
                        ),
                );
            }
        }
        content = content.child(table);

        // Well-known registries, so a first login needs no typing.
        content = content.child(section_title("PROVIDERS"));
        let mut provider_list = div()
            .rounded_lg()
            .border_1()
            .border_color(rgb(theme().border))
            .bg(rgb(theme().panel_bg))
            .flex()
            .flex_col();

        for (index, (name, host, mark, color)) in providers().into_iter().enumerate() {
            let username = self
                .accounts
                .iter()
                .find(|account| account.registry == host)
                .map(|account| account.username.clone());
            let registry: String = host.to_string();
            provider_list = provider_list.child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .h(px(54.))
                    .px_4()
                    .when(index > 0, |el| {
                        el.border_t_1().border_color(rgb(theme().border))
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .w(px(28.))
                            .h(px(28.))
                            .rounded_md()
                            .bg(rgb(color))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(theme().bg))
                                    .child(mark),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_sm()
                            .text_color(rgb(theme().text))
                            .truncate()
                            .child(name),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .when_some(username, |el, username| {
                                el.child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(theme().muted))
                                        .child(username),
                                )
                            })
                            .child(small_button_dyn(
                                format!("provider-{index}"),
                                "Configure",
                                false,
                                cx.listener(move |this, _, window, cx| {
                                    this.open_modal(
                                        Modal::RegistryLogin {
                                            registry: registry.clone(),
                                            username: String::new(),
                                            password: String::new(),
                                            field: LoginField::Username,
                                        },
                                        window,
                                        cx,
                                    );
                                }),
                            )),
                    ),
            );
        }
        content = content.child(provider_list);

        let files: Vec<String> = podman::auth_files()
            .into_iter()
            .filter(|path| path.exists())
            .map(|path| path.display().to_string())
            .collect();
        if !files.is_empty() {
            content = content.child(section_title("CREDENTIAL FILES"));
            let mut list = div()
                .flex()
                .flex_col()
                .gap_1()
                .text_xs()
                .text_color(rgb(theme().faint));
            for path in files {
                list = list.child(path);
            }
            content = content.child(list);
        }

        content.into_any_element()
    }
}

/// Name, host, mark and colour for the registries people log in to most.
fn providers() -> [(&'static str, &'static str, &'static str, u32); 4] {
    [
        ("Docker Hub", "docker.io", "D", theme().blue),
        ("Red Hat Quay", "quay.io", "Q", theme().red),
        ("GitHub", "ghcr.io", "G", theme().text),
        ("Google Container Registry", "gcr.io", "GCR", theme().yellow),
    ]
}
