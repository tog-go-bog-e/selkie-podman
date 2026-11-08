//! Column sorting.

use super::*;

impl Selkie {
    // -----------------------------------------------------------------------
    // Sorting
    // -----------------------------------------------------------------------

    pub(super) fn toggle_sort(&mut self, column: usize, cx: &mut Context<Self>) {
        self.sort = Some(match self.sort {
            Some(state) if state.column == column => SortState {
                column,
                ascending: !state.ascending,
            },
            _ => SortState {
                column,
                ascending: true,
            },
        });
        cx.notify();
    }

    pub(super) fn sorted(&self, indices: Vec<usize>) -> Vec<usize> {
        let Some(sort) = self.sort else {
            return indices;
        };
        let mut keyed: Vec<(SortKey, usize)> = indices
            .into_iter()
            .map(|index| (self.sort_key(index), index))
            .collect();
        keyed.sort_by(|a, b| a.0.compare(&b.0));
        if !sort.ascending {
            keyed.reverse();
        }
        keyed.into_iter().map(|(_, index)| index).collect()
    }

    /// Column indices are per tab, matching the non-compact table header.
    pub(super) fn sort_key(&self, index: usize) -> SortKey {
        let column = self.sort.map(|state| state.column).unwrap_or(0);
        let text = |value: &str| SortKey::Text(value.to_string());
        let number = |value: u64| SortKey::Number(value);

        match self.tab {
            Tab::Containers => {
                let item = &self.snapshot.containers[index];
                match column {
                    1 => text(&item.image),
                    2 => text(&item.state),
                    3 => text(&item.ports),
                    4 => text(&item.status),
                    5 => text(&item.id),
                    _ => text(&item.name),
                }
            }
            Tab::Compose => {
                let item = &self.snapshot.compose[index];
                match column {
                    1 => text(&item.status_label()),
                    2 => number(item.total as u64),
                    3 => text(&item.config_label()),
                    _ => text(&item.name),
                }
            }
            Tab::Images => {
                let item = &self.snapshot.images[index];
                match column {
                    1 => text(&item.id),
                    2 => number(item.size),
                    3 => number(item.containers),
                    4 => text(&item.created),
                    _ => text(&item.name),
                }
            }
            Tab::Pods => {
                let item = &self.snapshot.pods[index];
                match column {
                    1 => text(&item.status),
                    2 => number(item.containers),
                    3 => text(&item.created),
                    4 => text(&item.id),
                    _ => text(&item.name),
                }
            }
            Tab::Volumes => {
                let item = &self.snapshot.volumes[index];
                match column {
                    1 => text(&item.driver),
                    2 => number(item.mounts),
                    3 => text(&item.scope),
                    4 => text(&item.created),
                    _ => text(&item.name),
                }
            }
            Tab::Secrets => {
                let item = &self.snapshot.secrets[index];
                match column {
                    1 => text(&item.driver),
                    // RFC 3339 sorts chronologically as text.
                    2 => text(&item.created_at),
                    3 => text(&item.updated),
                    4 => text(&item.id),
                    _ => text(&item.name),
                }
            }
            Tab::Networks => {
                let item = &self.snapshot.networks[index];
                match column {
                    1 => text(&item.driver),
                    2 => text(&item.subnet_label()),
                    3 => text(&item.interface),
                    4 => text(bool_label(item.dns)),
                    _ => text(&item.name),
                }
            }
            Tab::Machines => {
                let item = &self.local.machines[index];
                match column {
                    1 => text(item.state_label()),
                    2 => number(item.cpus),
                    3 => number(item.memory),
                    4 => number(item.disk_size),
                    5 => text(&item.last_up),
                    _ => text(&item.name),
                }
            }
            Tab::Connections => {
                let item = &self.local.connections[index];
                match column {
                    1 => text(&item.uri),
                    2 => text(&self.connection_key(item)),
                    3 => number(match self.connection_checks.get(&item.name) {
                        Some(ConnectionCheck::Reachable) => 0,
                        Some(ConnectionCheck::Checking) => 1,
                        Some(ConnectionCheck::Locked) => 2,
                        None => 3,
                        Some(ConnectionCheck::Unreachable(_)) => 4,
                    }),
                    _ => text(&item.name),
                }
            }
            Tab::Dashboard | Tab::Accounts | Tab::Settings => text(""),
        }
    }

    pub(super) fn rows_len(&self) -> usize {
        self.visible_indices.len()
    }
}
