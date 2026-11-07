//! Keyboard navigation and the row context menu.

use super::*;

impl PodmanDesktop {
    // -----------------------------------------------------------------------
    // Keyboard navigation and context menu
    // -----------------------------------------------------------------------

    pub(super) fn root_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        let modifiers = event.keystroke.modifiers;

        if (modifiers.control || modifiers.platform) && key == "r" {
            self.refresh_dashboard(cx);
            return;
        }
        if (modifiers.control || modifiers.platform) && key == "q" {
            cx.quit();
            return;
        }
        if modifiers.control || modifiers.platform || modifiers.alt {
            return;
        }
        if self.modal.is_some() || self.search_focus.is_focused(window) {
            return;
        }

        match key {
            "escape" => {
                if self.account_menu.is_some() {
                    self.close_account_menu(cx);
                } else if self.context_menu.is_some() {
                    self.close_context_menu(cx);
                } else if self.selection.is_some() {
                    self.clear_selection();
                    cx.notify();
                } else if !self.search.is_empty() {
                    self.search.clear();
                    cx.notify();
                }
            }
            "/" => {
                if matches!(self.tab, Tab::Dashboard | Tab::Accounts | Tab::Settings) {
                    return;
                }
                self.close_context_menu(cx);
                self.caret_on = true;
                self.search_focus.focus(window);
                cx.notify();
            }
            "j" | "down" => {
                self.close_context_menu(cx);
                self.move_selection(1, cx);
            }
            "k" | "up" => {
                self.close_context_menu(cx);
                self.move_selection(-1, cx);
            }
            "enter" | "return" => {
                if self.selection.is_none() {
                    self.select_index(0, cx);
                    if !self.visible_indices.is_empty() {
                        self.rows_scroll.scroll_to_item(0, ScrollStrategy::Center);
                    }
                }
            }
            _ => {}
        }
    }

    pub(super) fn current_visible_position(&self) -> Option<usize> {
        match (&self.selection, self.tab) {
            (Some(Selection::Container(id)), Tab::Containers) => self
                .visible_indices
                .iter()
                .position(|&index| &self.snapshot.containers[index].id == id),
            (Some(Selection::Image(id)), Tab::Images) => self
                .visible_indices
                .iter()
                .position(|&index| &self.snapshot.images[index].id == id),
            (Some(Selection::Pod(id)), Tab::Pods) => self
                .visible_indices
                .iter()
                .position(|&index| &self.snapshot.pods[index].id == id),
            (Some(Selection::Volume(name)), Tab::Volumes) => self
                .visible_indices
                .iter()
                .position(|&index| &self.snapshot.volumes[index].name == name),
            (Some(Selection::Network(name)), Tab::Networks) => self
                .visible_indices
                .iter()
                .position(|&index| &self.snapshot.networks[index].name == name),
            _ => None,
        }
    }

    pub(super) fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(next) = next_position(
            self.current_visible_position(),
            self.visible_indices.len(),
            delta,
        ) else {
            return;
        };

        self.select_index(next, cx);
        self.rows_scroll
            .scroll_to_item(next, ScrollStrategy::Center);
    }

    pub(super) fn select_index(&mut self, position: usize, cx: &mut Context<Self>) {
        let Some(&index) = self.visible_indices.get(position) else {
            return;
        };
        match self.tab {
            Tab::Containers => {
                let id = self.snapshot.containers[index].id.clone();
                self.select_container(id, cx);
            }
            Tab::Compose => {
                let name = self.snapshot.compose[index].name.clone();
                self.select_compose(name, cx);
            }
            Tab::Images => {
                let id = self.snapshot.images[index].id.clone();
                self.select_image(id, cx);
            }
            Tab::Pods => {
                let id = self.snapshot.pods[index].id.clone();
                self.select_pod(id, cx);
            }
            Tab::Volumes => {
                let name = self.snapshot.volumes[index].name.clone();
                self.select_volume(name, cx);
            }
            Tab::Networks => {
                let name = self.snapshot.networks[index].name.clone();
                self.select_network(name, cx);
            }
            Tab::Machines => {
                let name = self.local.machines[index].name.clone();
                self.select_machine(name, cx);
            }
            Tab::Connections => {
                let name = self.local.connections[index].name.clone();
                self.select_connection(name, cx);
            }
            Tab::Dashboard | Tab::Accounts | Tab::Settings => {}
        }
    }

    pub(super) fn open_context_menu(
        &mut self,
        position: Point<Pixels>,
        selection: Selection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.root_focus.focus(window);
        match selection.clone() {
            Selection::Container(id) => self.select_container(id, cx),
            Selection::Compose(name) => self.select_compose(name, cx),
            Selection::Image(id) => self.select_image(id, cx),
            Selection::Pod(id) => self.select_pod(id, cx),
            Selection::Volume(name) => self.select_volume(name, cx),
            Selection::Network(name) => self.select_network(name, cx),
            Selection::Machine(name) => self.select_machine(name, cx),
            Selection::Connection(name) => self.select_connection(name, cx),
        }
        self.context_menu = Some(ContextMenuState {
            position,
            selection,
        });
        cx.notify();
    }

    pub(super) fn close_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.context_menu.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn copy_text(&mut self, text: String, label: &'static str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.toast(ToastKind::Success, format!("Copied {label}"), cx);
    }

    /// Persist the window size so the next launch opens with the same frame.
    pub(crate) fn remember_window(&mut self, bounds: Bounds<Pixels>, maximized: bool) {
        self.settings.remember_window(
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
            maximized,
        );
    }

    pub(super) fn cycle_log_window(&mut self, cx: &mut Context<Self>) {
        self.settings.next_log_window();
        self.apply_log_window(cx);
    }

    /// Store a log window picked in Settings and restart any live stream so the
    /// change takes effect immediately.
    pub(super) fn set_log_window(&mut self, window: &str, cx: &mut Context<Self>) {
        if self.settings.log_window == window {
            return;
        }
        self.settings.log_window = window.to_string();
        self.apply_log_window(cx);
    }

    pub(super) fn apply_log_window(&mut self, cx: &mut Context<Self>) {
        self.settings.save();
        match self.logs_target.clone() {
            Some(LogTarget::Container(id)) => self.start_logs(&id, cx),
            // Compose logs use `--tail`, so the container window does not apply;
            // restart them only to keep the setting consistent for later.
            Some(LogTarget::Compose { project, service }) => {
                let project = self
                    .snapshot
                    .compose
                    .iter()
                    .find(|item| item.name == project)
                    .cloned();
                if let Some(project) = project {
                    self.start_compose_logs(&project, service, cx);
                }
            }
            None => {}
        }
        cx.notify();
    }

    pub(super) fn load_details(&mut self, selection: Selection, cx: &mut Context<Self>) {
        self.details = DetailsState::Loading;
        cx.notify();

        let expected = selection.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    match &selection {
                        Selection::Container(id) => podman::inspect_container(id)
                            .map(|details| DetailsState::Container(Box::new(details))),
                        Selection::Pod(id) => podman::inspect_pod(id)
                            .map(|details| DetailsState::Pod(Box::new(details))),
                        Selection::Volume(name) => podman::inspect_volume(name)
                            .map(|details| DetailsState::Volume(Box::new(details))),
                        Selection::Network(name) => podman::inspect_network(name)
                            .map(|details| DetailsState::Network(Box::new(details))),
                        Selection::Image(_) | Selection::Compose(_) => Ok(DetailsState::None),
                        Selection::Machine(_) | Selection::Connection(_) => Ok(DetailsState::None),
                    }
                    .unwrap_or_else(DetailsState::Error)
                })
                .await;
            this.update(cx, |this, cx| {
                if this.selection.as_ref() == Some(&expected) {
                    this.details = result;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn reload_selection(&mut self, cx: &mut Context<Self>) {
        if let Some(selection) = self.selection.clone() {
            if matches!(
                selection,
                Selection::Container(_)
                    | Selection::Pod(_)
                    | Selection::Volume(_)
                    | Selection::Network(_)
            ) {
                self.load_details(selection, cx);
            }
        }
    }

    /// Drop the cached raw JSON whenever the selection changes.
    pub(super) fn reset_raw(&mut self) {
        self.raw_view = false;
        self.raw = None;
    }

    /// Toggle between the summary and the raw `inspect` JSON for the current
    /// selection. The JSON is fetched once and cached until the selection moves.
    pub(super) fn toggle_raw(&mut self, selection: Selection, cx: &mut Context<Self>) {
        if self.raw_view {
            self.raw_view = false;
            cx.notify();
            return;
        }
        let Some(kind) = inspect_kind(&selection) else {
            return;
        };
        let key = match &selection {
            Selection::Container(key)
            | Selection::Image(key)
            | Selection::Pod(key)
            | Selection::Volume(key)
            | Selection::Network(key) => key.clone(),
            Selection::Compose(_) | Selection::Machine(_) | Selection::Connection(_) => return,
        };

        self.raw_view = true;
        if self.raw.is_none() {
            self.raw = Some(RawState::Loading);
            let expected = selection;
            cx.spawn(async move |this, cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move { podman::inspect_json(kind, &key) })
                    .await;
                this.update(cx, |this, cx| {
                    if this.selection.as_ref() == Some(&expected) {
                        this.raw = Some(match result {
                            Ok(text) => RawState::Text(text),
                            Err(error) => RawState::Error(error),
                        });
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }
        cx.notify();
    }
}
