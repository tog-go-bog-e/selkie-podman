//! Bulk selection: check several rows and delete them in one action.
//!
//! Only the list tabs whose rows have a "Delete" action participate. Podman
//! accepts one object per command, so the deletions run sequentially in one
//! background task and partial failures are reported by name.

use super::*;

/// Space taken by the per-row checkbox (16 px box plus an 8 px gap).
pub(super) const BULK_CHECKBOX_WIDTH: f32 = 24.0;

/// Resource lists that support bulk deletion.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BulkKind {
    Container,
    Image,
    Pod,
    Volume,
    Network,
    Secret,
}

impl BulkKind {
    pub(super) fn from_tab(tab: Tab) -> Option<Self> {
        match tab {
            Tab::Containers => Some(Self::Container),
            Tab::Images => Some(Self::Image),
            Tab::Pods => Some(Self::Pod),
            Tab::Volumes => Some(Self::Volume),
            Tab::Networks => Some(Self::Network),
            Tab::Secrets => Some(Self::Secret),
            Tab::Compose
            | Tab::Machines
            | Tab::Connections
            | Tab::Accounts
            | Tab::Dashboard
            | Tab::Settings => None,
        }
    }

    pub(super) fn from_selection(selection: &Selection) -> Option<Self> {
        match selection {
            Selection::Container(_) => Some(Self::Container),
            Selection::Image(_) => Some(Self::Image),
            Selection::Pod(_) => Some(Self::Pod),
            Selection::Volume(_) => Some(Self::Volume),
            Selection::Network(_) => Some(Self::Network),
            Selection::Secret(_) => Some(Self::Secret),
            Selection::Compose(_) | Selection::Machine(_) | Selection::Connection(_) => None,
        }
    }

    pub(super) fn noun(self) -> &'static str {
        match self {
            Self::Container => "container",
            Self::Image => "image",
            Self::Pod => "pod",
            Self::Volume => "volume",
            Self::Network => "network",
            Self::Secret => "secret",
        }
    }

    pub(super) fn plural(self) -> &'static str {
        match self {
            Self::Container => "containers",
            Self::Image => "images",
            Self::Pod => "pods",
            Self::Volume => "volumes",
            Self::Network => "networks",
            Self::Secret => "secrets",
        }
    }

    pub(super) fn noun_for(self, count: usize) -> &'static str {
        if count == 1 {
            self.noun()
        } else {
            self.plural()
        }
    }

    /// What happens to the selected objects, for the confirmation dialog.
    pub(super) fn consequence(self) -> &'static str {
        match self {
            Self::Container => "will be force-removed",
            Self::Image => "will be force-removed",
            Self::Pod => "and their containers will be force-removed",
            Self::Volume => "will be force-removed; volumes in use by a container are kept",
            Self::Network => "will be force-removed; connected containers may be affected",
            Self::Secret => "will be removed",
        }
    }

    /// Remove one object. `key` is the ID for pods, images, containers and
    /// secrets, and the name for volumes and networks.
    pub(super) fn remove(self, key: &str) -> Result<String, String> {
        match self {
            Self::Container => podman::remove_container(key),
            Self::Image => podman::remove_image(key),
            Self::Pod => podman::remove_pod(key),
            Self::Volume => podman::remove_volume(key),
            Self::Network => podman::remove_network(key),
            Self::Secret => podman::remove_secret(key),
        }
    }
}

/// Add or remove `selection`, preserving the order in which rows were checked.
fn toggle(list: &mut Vec<Selection>, selection: Selection) {
    if let Some(position) = list.iter().position(|item| item == &selection) {
        list.remove(position);
    } else {
        list.push(selection);
    }
}

impl Selkie {
    /// Enter bulk-selection mode on the current list tab.
    ///
    /// The details pane is closed so the list spans the window, and the search
    /// filter is cleared so "Select all" means every row.
    pub(super) fn start_multi(&mut self, cx: &mut Context<Self>) {
        if BulkKind::from_tab(self.tab).is_none() {
            return;
        }
        self.clear_selection();
        self.search.clear();
        self.multi.clear();
        self.multi_mode = true;
        self.context_menu = None;
        cx.notify();
    }

    /// Leave bulk-selection mode and forget the checked rows.
    pub(super) fn clear_multi(&mut self) {
        self.multi_mode = false;
        self.multi.clear();
    }

    /// Leave bulk-selection mode from a button or `Escape`.
    pub(super) fn cancel_multi(&mut self, cx: &mut Context<Self>) {
        if self.multi_mode || !self.multi.is_empty() {
            self.clear_multi();
            cx.notify();
        }
    }

    /// Uncheck every row but stay in bulk-selection mode.
    pub(super) fn clear_multi_selection(&mut self, cx: &mut Context<Self>) {
        if !self.multi.is_empty() {
            self.multi.clear();
            cx.notify();
        }
    }

    /// Check or uncheck one row.
    pub(super) fn toggle_multi(&mut self, selection: Selection, cx: &mut Context<Self>) {
        if !self.multi_mode {
            return;
        }
        toggle(&mut self.multi, selection);
        cx.notify();
    }

    pub(super) fn is_multi(&self, selection: &Selection) -> bool {
        self.multi.contains(selection)
    }

    /// Check every row currently shown by the filter.
    pub(super) fn select_all_visible(&mut self, cx: &mut Context<Self>) {
        if !self.multi_mode {
            return;
        }
        self.multi = self
            .visible_indices
            .iter()
            .filter_map(|&index| self.selection_at(index))
            .collect();
        cx.notify();
    }

    /// Drop checked rows that disappeared from the snapshot (another client
    /// deleted them, or a filter changed the list).
    pub(super) fn prune_multi(&mut self) {
        if self.multi.is_empty() {
            return;
        }
        let gone: Vec<Selection> = self
            .multi
            .iter()
            .filter(|selection| {
                BulkKind::from_selection(selection).is_none() || self.is_gone(selection)
            })
            .cloned()
            .collect();
        self.multi.retain(|selection| !gone.contains(selection));
    }

    /// The selection value for a visible row (`visible_indices` entry).
    pub(super) fn selection_at(&self, index: usize) -> Option<Selection> {
        match self.tab {
            Tab::Containers => self
                .snapshot
                .containers
                .get(index)
                .map(|item| Selection::Container(item.id.clone())),
            Tab::Images => self
                .snapshot
                .images
                .get(index)
                .map(|item| Selection::Image(item.id.clone())),
            Tab::Pods => self
                .snapshot
                .pods
                .get(index)
                .map(|item| Selection::Pod(item.id.clone())),
            Tab::Volumes => self
                .snapshot
                .volumes
                .get(index)
                .map(|item| Selection::Volume(item.name.clone())),
            Tab::Networks => self
                .snapshot
                .networks
                .get(index)
                .map(|item| Selection::Network(item.name.clone())),
            Tab::Secrets => self
                .snapshot
                .secrets
                .get(index)
                .map(|item| Selection::Secret(item.id.clone())),
            Tab::Compose
            | Tab::Machines
            | Tab::Connections
            | Tab::Accounts
            | Tab::Dashboard
            | Tab::Settings => None,
        }
    }

    /// The key Podman needs to remove an object, plus its display name.
    fn bulk_item(&self, selection: &Selection) -> Option<(String, String)> {
        match selection {
            Selection::Container(id) => self
                .snapshot
                .containers
                .iter()
                .find(|item| &item.id == id)
                .map(|item| (item.id.clone(), item.name.clone())),
            Selection::Image(id) => self
                .snapshot
                .images
                .iter()
                .find(|item| &item.id == id)
                .map(|item| (item.id.clone(), item.name.clone())),
            Selection::Pod(id) => self
                .snapshot
                .pods
                .iter()
                .find(|item| &item.id == id)
                .map(|item| (item.id.clone(), item.name.clone())),
            Selection::Volume(name) => Some((name.clone(), name.clone())),
            Selection::Network(name) => Some((name.clone(), name.clone())),
            Selection::Secret(id) => self
                .snapshot
                .secrets
                .iter()
                .find(|item| &item.id == id)
                .map(|item| (item.id.clone(), item.name.clone())),
            Selection::Compose(_) | Selection::Machine(_) | Selection::Connection(_) => None,
        }
    }

    /// Ask for confirmation before removing every checked row.
    pub(super) fn open_bulk_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(kind) = BulkKind::from_tab(self.tab) else {
            return;
        };
        if self.multi.is_empty() || self.busy.is_some() {
            return;
        }
        let mut items = Vec::new();
        for &index in &self.visible_indices {
            let Some(selection) = self.selection_at(index) else {
                continue;
            };
            if !self.is_multi(&selection) {
                continue;
            }
            if let Some(item) = self.bulk_item(&selection) {
                items.push(item);
            }
        }
        if items.is_empty() {
            return;
        }
        self.open_modal(Modal::DeleteSelected { kind, items }, window, cx);
    }

    /// Delete the confirmed rows one by one, then refresh and report partial
    /// failures. Podman has no multi-object delete that reports per-object
    /// results, so the removals stay separate commands.
    pub(super) fn run_bulk_delete(
        &mut self,
        kind: BulkKind,
        items: Vec<(String, String)>,
        cx: &mut Context<Self>,
    ) {
        if self.busy.is_some() {
            return;
        }
        let count = items.len();
        let noun = kind.noun_for(count);
        self.busy = Some(format!("Deleting {count} {noun}…"));
        self.clear_multi();
        cx.notify();

        cx.spawn(async move |this, cx| {
            let failures = cx
                .background_executor()
                .spawn(async move {
                    let mut failures = Vec::new();
                    for (key, name) in &items {
                        if let Err(error) = kind.remove(key) {
                            failures.push(format!("{name}: {error}"));
                        }
                    }
                    failures
                })
                .await;

            this.update(cx, |this, cx| {
                this.busy = None;
                // Any action may have changed a machine or connection.
                this.local_fetched = None;
                // The checked rows were the current tab's selection candidates;
                // drop any details pane that pointed at one of them.
                this.clear_selection();
                this.refresh(cx);

                let failed = failures.len();
                if failed == 0 {
                    this.toast(ToastKind::Success, format!("Deleted {count} {noun}"), cx);
                } else if failed == count {
                    this.toast(
                        ToastKind::Error,
                        format!("Could not delete {count} {noun}: {}", failures[0]),
                        cx,
                    );
                } else {
                    this.toast(
                        ToastKind::Error,
                        format!(
                            "Deleted {} of {count} {noun}; {failed} failed ({})",
                            count - failed,
                            failures[0]
                        ),
                        cx,
                    );
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_bulk_tabs_to_kinds() {
        assert!(matches!(
            BulkKind::from_tab(Tab::Containers),
            Some(BulkKind::Container)
        ));
        assert!(matches!(
            BulkKind::from_tab(Tab::Secrets),
            Some(BulkKind::Secret)
        ));
        assert!(BulkKind::from_tab(Tab::Compose).is_none());
        assert!(BulkKind::from_tab(Tab::Machines).is_none());
        assert!(BulkKind::from_tab(Tab::Connections).is_none());
    }

    #[test]
    fn names_agree_with_the_count() {
        assert_eq!(BulkKind::Container.noun_for(1), "container");
        assert_eq!(BulkKind::Container.noun_for(2), "containers");
        assert_eq!(BulkKind::Volume.noun_for(3), "volumes");
    }

    #[test]
    fn toggling_keeps_click_order_and_deduplicates() {
        let mut list = Vec::new();
        toggle(&mut list, Selection::Container("a".into()));
        toggle(&mut list, Selection::Container("b".into()));
        toggle(&mut list, Selection::Image("img".into()));
        assert_eq!(list.len(), 3);

        toggle(&mut list, Selection::Container("a".into()));
        assert_eq!(
            list,
            vec![
                Selection::Container("b".into()),
                Selection::Image("img".into())
            ]
        );
    }
}
