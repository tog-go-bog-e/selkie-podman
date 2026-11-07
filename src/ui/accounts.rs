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

    pub(super) fn accounts_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut content = div()
            .id("accounts")
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
                .child(section_title("REGISTRY ACCOUNTS"))
                .child(pill_button(
                    "account-login",
                    "Log in…",
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
        } else if self.accounts.is_empty() {
            content = content.child(
                div()
                    .text_sm()
                    .text_color(rgb(theme().muted))
                    .child("No registries are logged in. Log in to push or pull private images."),
            );
        } else {
            let mut list = div()
                .rounded_lg()
                .border_1()
                .border_color(rgb(theme().border))
                .bg(rgb(theme().panel_bg))
                .flex()
                .flex_col();

            for (index, account) in self.accounts.iter().enumerate() {
                let registry = account.registry.clone();
                list = list.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .px_4()
                        .py_3()
                        .when(index > 0, |el| {
                            el.border_t_1().border_color(rgb(theme().border))
                        })
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .min_w(px(0.))
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(theme().text))
                                        .truncate()
                                        .child(account.registry.clone()),
                                )
                                .child(div().text_xs().text_color(rgb(theme().muted)).child(
                                    format!("{} · {}", account.username, account.kind.label()),
                                )),
                        )
                        .child(small_button_dyn(
                            format!("account-logout-{index}"),
                            "Log out",
                            false,
                            cx.listener(move |this, _, window, cx| {
                                this.open_modal(
                                    Modal::RegistryLogout {
                                        registry: registry.clone(),
                                    },
                                    window,
                                    cx,
                                );
                            }),
                        )),
                );
            }
            content = content.child(list);
        }

        let files: Vec<String> = podman::auth_files()
            .into_iter()
            .filter(|path| path.exists())
            .map(|path| path.display().to_string())
            .collect();
        if !files.is_empty() {
            let mut list = div()
                .flex()
                .flex_col()
                .gap_1()
                .text_xs()
                .text_color(rgb(theme().faint));
            for path in files {
                list = list.child(path);
            }
            content = content.child(section_title("CREDENTIAL FILES")).child(list);
        }

        content.into_any_element()
    }
}
