//! The table search box and filtering.

use super::*;

impl Selkie {
    // -----------------------------------------------------------------------
    // Search
    // -----------------------------------------------------------------------

    pub(super) fn search_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match edit_text(&mut self.search, event, cx) {
            TextOutcome::Cancel => {
                cx.stop_propagation();
                if self.search.is_empty() {
                    self.root_focus.focus(window);
                } else {
                    self.search.clear();
                }
                cx.notify();
            }
            TextOutcome::Edited => {
                // Keep the caret visible while the user is typing.
                self.caret_on = true;
                cx.notify();
            }
            TextOutcome::Submit | TextOutcome::Ignored => {}
        }
    }

    pub(super) fn rebuild_filter(&mut self) {
        let query = self.search.trim();
        let indices = match self.tab {
            Tab::Containers => self
                .snapshot
                .containers
                .iter()
                .enumerate()
                .filter(|(_, container)| {
                    matches_query(
                        query,
                        &[
                            &container.name,
                            &container.image,
                            &container.id,
                            &container.state,
                            &container.status,
                        ],
                    )
                })
                .map(|(index, _)| index)
                .collect(),
            Tab::Compose => self
                .snapshot
                .compose
                .iter()
                .enumerate()
                .filter(|(_, project)| {
                    let files = project.config_files.join(" ");
                    let dir = project.working_dir.clone().unwrap_or_default();
                    let status = project.status_label();
                    let mut fields: Vec<&String> = vec![&project.name, &files, &dir, &status];
                    fields.extend(project.services.iter().map(|service| &service.name));
                    matches_query(query, &fields)
                })
                .map(|(index, _)| index)
                .collect(),
            Tab::Images => self
                .snapshot
                .images
                .iter()
                .enumerate()
                .filter(|(_, image)| matches_query(query, &[&image.name, &image.id, &image.digest]))
                .map(|(index, _)| index)
                .collect(),
            Tab::Pods => self
                .snapshot
                .pods
                .iter()
                .enumerate()
                .filter(|(_, pod)| matches_query(query, &[&pod.name, &pod.id, &pod.status]))
                .map(|(index, _)| index)
                .collect(),
            Tab::Volumes => self
                .snapshot
                .volumes
                .iter()
                .enumerate()
                .filter(|(_, volume)| {
                    matches_query(
                        query,
                        &[
                            &volume.name,
                            &volume.driver,
                            &volume.mountpoint,
                            &volume.scope,
                        ],
                    )
                })
                .map(|(index, _)| index)
                .collect(),
            Tab::Networks => self
                .snapshot
                .networks
                .iter()
                .enumerate()
                .filter(|(_, network)| {
                    matches_query(
                        query,
                        &[
                            &network.name,
                            &network.id,
                            &network.driver,
                            &network.interface,
                            &network.subnet_label(),
                        ],
                    )
                })
                .map(|(index, _)| index)
                .collect(),
            Tab::Secrets => self
                .snapshot
                .secrets
                .iter()
                .enumerate()
                .filter(|(_, secret)| {
                    let labels: Vec<String> = secret
                        .labels
                        .iter()
                        .map(|(key, value)| format!("{key}={value}"))
                        .collect();
                    let mut fields: Vec<&String> = vec![&secret.name, &secret.id, &secret.driver];
                    fields.extend(labels.iter());
                    matches_query(query, &fields)
                })
                .map(|(index, _)| index)
                .collect(),
            Tab::Machines => self
                .local
                .machines
                .iter()
                .enumerate()
                .filter(|(_, machine)| {
                    let state = machine.state_label().to_string();
                    matches_query(query, &[&machine.name, &machine.vm_type, &state])
                })
                .map(|(index, _)| index)
                .collect(),
            Tab::Connections => self
                .local
                .connections
                .iter()
                .enumerate()
                .filter(|(_, connection)| {
                    let key = self.connection_key(connection);
                    matches_query(query, &[&connection.name, &connection.uri, &key])
                })
                .map(|(index, _)| index)
                .collect(),
            Tab::Dashboard | Tab::Accounts | Tab::Settings => Vec::new(),
        };
        self.visible_indices = self.sorted(indices);
    }
}
