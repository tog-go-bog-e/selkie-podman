//! Snapshot loading and the refresh cycle.

use super::*;

impl PodmanDesktop {
    // -----------------------------------------------------------------------
    // Data loading
    // -----------------------------------------------------------------------

    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        // Login and logout run through the output panel and end in a refresh, so
        // the credential list follows them.
        let registries_visible = self.tab == Tab::Accounts
            || (self.tab == Tab::Settings
                && self.settings.settings_section == settings::SettingsSection::Registries);
        if registries_visible {
            self.load_accounts();
        }
        if self.loading {
            // Remember the request so an event arriving during a fetch still
            // results in a fresh snapshot once the current one lands.
            self.refresh_pending = true;
            return;
        }
        self.loading = true;
        cx.notify();

        // `podman info` and `podman system df` are only worth their extra
        // processes while the Dashboard is on screen, and `podman info` alone
        // can take seconds on some hosts, so the result is cached.
        let want_dashboard = self.tab == Tab::Dashboard
            && self
                .dashboard_fetched
                .is_none_or(|at| at.elapsed() >= DASHBOARD_TTL);

        let want_local = matches!(self.tab, Tab::Machines | Tab::Connections)
            || self
                .local_fetched
                .is_none_or(|at| at.elapsed() >= LOCAL_TTL);

        cx.spawn(async move |this, cx| {
            let (local, result, dashboard) = cx
                .background_executor()
                .spawn(async move {
                    let local = want_local.then(podman::local_state);
                    let snapshot = podman::snapshot();
                    let dashboard = if want_dashboard {
                        Some(podman::dashboard())
                    } else {
                        None
                    };
                    (local, snapshot, dashboard)
                })
                .await;
            this.update(cx, |this, cx| {
                this.loading = false;
                // Machines and connections are local configuration and stay
                // available even when the selected engine cannot be reached.
                if let Some(local) = local {
                    this.local = local;
                    this.local_fetched = Some(Instant::now());
                }
                if let Some(dashboard) = dashboard {
                    this.dashboard_fetched = Some(Instant::now());
                    match dashboard {
                        Ok(data) => {
                            this.dashboard = Some(Box::new(data));
                            this.dashboard_error = None;
                        }
                        Err(error) => this.dashboard_error = Some(error),
                    }
                }
                match result {
                    Ok(snapshot) => {
                        this.snapshot = snapshot;
                        this.error = None;
                        this.last_updated = Some(Instant::now());
                        if this.selection_is_gone() {
                            this.clear_selection();
                        }
                        let menu_gone = this
                            .context_menu
                            .as_ref()
                            .is_some_and(|menu| this.is_gone(&menu.selection));
                        if menu_gone {
                            this.context_menu = None;
                        }
                    }
                    Err(error) => {
                        // Keep the last good data visible and surface the error.
                        this.error = Some(error);
                    }
                }
                let again = std::mem::take(&mut this.refresh_pending);
                cx.notify();
                if again {
                    this.refresh(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Refresh, and re-read the Dashboard's slow `podman info`/`system df` data.
    pub(crate) fn refresh_dashboard(&mut self, cx: &mut Context<Self>) {
        self.dashboard_fetched = None;
        self.local_fetched = None;
        self.refresh(cx);
    }

    /// Toggle the text caret while a text input has focus.
    ///
    /// Called from the 500 ms desktop tick in `main.rs`: a GPUI animation would
    /// redraw the whole window at display rate just to blink a 2 px bar.
    pub(crate) fn blink_caret(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.modal.is_some() || self.search_focus.is_focused(window) {
            self.caret_on = !self.caret_on;
            cx.notify();
        } else if self.terminal_focus.is_focused(window)
            && self
                .terminal
                .as_ref()
                .is_some_and(|session| session.cursor_blinks())
        {
            // Only a blinking cursor shape needs the redraw.
            self.caret_on = !self.caret_on;
            cx.notify();
        }
    }

    pub(super) fn selection_is_gone(&self) -> bool {
        self.selection
            .as_ref()
            .is_some_and(|selection| self.is_gone(selection))
    }

    pub(super) fn is_gone(&self, selection: &Selection) -> bool {
        match selection {
            Selection::Container(id) => !self.snapshot.containers.iter().any(|item| &item.id == id),
            Selection::Compose(name) => {
                !self.snapshot.compose.iter().any(|item| &item.name == name)
            }
            Selection::Image(id) => !self.snapshot.images.iter().any(|item| &item.id == id),
            Selection::Pod(id) => !self.snapshot.pods.iter().any(|item| &item.id == id),
            Selection::Volume(name) => !self.snapshot.volumes.iter().any(|item| &item.name == name),
            Selection::Network(name) => {
                !self.snapshot.networks.iter().any(|item| &item.name == name)
            }
            Selection::Secret(id) => !self.snapshot.secrets.iter().any(|item| &item.id == id),
            Selection::Machine(name) => !self.local.machines.iter().any(|item| &item.name == name),
            Selection::Connection(name) => {
                !self.local.connections.iter().any(|item| &item.name == name)
            }
        }
    }
}
