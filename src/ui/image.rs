use std::cmp::Ordering;

use super::helpers::*;
use super::*;
use crate::podman::human_size;

/// Rows rendered for one directory before the list is clipped.
const MAX_FILE_ROWS: usize = 400;
/// Lines rendered for a text file preview.
const MAX_FILE_LINES: usize = 2000;

impl PodmanDesktop {
    // -----------------------------------------------------------------------
    // Image history and file browser
    // -----------------------------------------------------------------------

    pub(super) fn reset_image_views(&mut self) {
        self.history = None;
        self.history_image = None;
        self.files = None;
        self.files_image = None;
        self.files_dir.clear();
        self.file_view = None;
    }

    pub(super) fn load_image_history(&mut self, id: String, cx: &mut Context<Self>) {
        if self.history_image.as_ref() == Some(&id) && self.history.is_some() {
            return;
        }
        self.history_image = Some(id.clone());
        self.history = Some(HistoryState::Loading);
        cx.notify();

        let fetch_id = id.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { podman::image_history(&fetch_id) })
                .await;
            this.update(cx, |this, cx| {
                if this.history_image.as_deref() != Some(id.as_str()) {
                    return;
                }
                this.history = Some(match result {
                    Ok(entries) => HistoryState::Ready(entries),
                    Err(error) => HistoryState::Error(error),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn load_image_files(&mut self, id: String, cx: &mut Context<Self>) {
        if self.files_image.as_ref() == Some(&id) && self.files.is_some() {
            return;
        }
        self.files_image = Some(id.clone());
        self.files = Some(FilesState {
            entries: Vec::new(),
            loading: true,
            error: None,
        });
        self.files_dir.clear();
        self.file_view = None;
        cx.notify();

        let fetch_id = id.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { podman::image_files(&fetch_id) })
                .await;
            this.update(cx, |this, cx| {
                if this.files_image.as_deref() != Some(id.as_str()) {
                    return;
                }
                this.files = Some(match result {
                    Ok(entries) => FilesState {
                        entries,
                        loading: false,
                        error: None,
                    },
                    Err(error) => FilesState {
                        entries: Vec::new(),
                        loading: false,
                        error: Some(error),
                    },
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn open_image_file(
        &mut self,
        id: String,
        path: String,
        size: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        self.file_view = Some(FileViewState {
            path: path.clone(),
            size,
            content: FileContent::Loading,
        });
        cx.notify();

        let fetch_path = path.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    podman::image_file_content(&id, &fetch_path, podman::IMAGE_FILE_LIMIT)
                })
                .await;
            this.update(cx, |this, cx| {
                let Some(view) = this.file_view.as_mut() else {
                    return;
                };
                if view.path != path {
                    return;
                }
                view.content = match result {
                    Ok(bytes) => {
                        let truncated = bytes.len() >= podman::IMAGE_FILE_LIMIT;
                        match String::from_utf8(bytes) {
                            Ok(text) if !text.contains('\0') => {
                                FileContent::Text { text, truncated }
                            }
                            Ok(text) => FileContent::Binary { size: text.len() },
                            Err(error) => FileContent::Binary {
                                size: error.as_bytes().len(),
                            },
                        }
                    }
                    Err(error) => FileContent::Error(error),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Show a file that needs no fetch (currently only symlinks).
    fn show_image_link(&mut self, path: String, target: Option<String>, cx: &mut Context<Self>) {
        let text = match target {
            Some(target) => format!("→ {target}"),
            None => "(link)".to_string(),
        };
        self.file_view = Some(FileViewState {
            path,
            size: None,
            content: FileContent::Text {
                text,
                truncated: false,
            },
        });
        cx.notify();
    }

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

    pub(super) fn image_history_view(&self) -> AnyElement {
        let mut view = div().flex_1().min_h(px(0.)).flex().flex_col();

        let label = match &self.history {
            Some(HistoryState::Ready(entries)) => format!("{} layers", entries.len()),
            Some(HistoryState::Error(_)) => "unavailable".to_string(),
            _ => "loading…".to_string(),
        };
        view = view.child(
            div()
                .flex()
                .items_center()
                .px_4()
                .py_2()
                .border_b_1()
                .border_color(rgb(theme().border))
                .child(div().text_xs().text_color(rgb(theme().muted)).child(label)),
        );

        let body: AnyElement = match &self.history {
            None | Some(HistoryState::Loading) => div()
                .p_4()
                .text_sm()
                .text_color(rgb(theme().faint))
                .child("Reading `podman history`…")
                .into_any_element(),
            Some(HistoryState::Error(error)) => div()
                .p_4()
                .text_sm()
                .text_color(rgb(theme().red))
                .child(error.clone())
                .into_any_element(),
            Some(HistoryState::Ready(entries)) if entries.is_empty() => div()
                .p_4()
                .text_sm()
                .text_color(rgb(theme().faint))
                .child("Podman reported no history for this image.")
                .into_any_element(),
            Some(HistoryState::Ready(entries)) => {
                let mut list = div()
                    .id("history-scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col();
                for (index, entry) in entries.iter().enumerate() {
                    let mut row =
                        div()
                            .id(SharedString::from(format!("history-{index}")))
                            .px_4()
                            .py_2()
                            .border_b_1()
                            .border_color(rgb(theme().border))
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_family("monospace")
                                            .text_color(rgb(theme().accent))
                                            .child(entry.id_label()),
                                    )
                                    .child(div().text_xs().text_color(rgb(theme().muted)).child(
                                        format!("{} · {}", entry.size_label(), entry.created),
                                    )),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .font_family("monospace")
                                    .text_color(rgb(theme().text))
                                    .child(entry.created_by.clone()),
                            );
                    if !entry.comment.is_empty() {
                        row = row.child(
                            div()
                                .text_xs()
                                .text_color(rgb(theme().faint))
                                .child(entry.comment.clone()),
                        );
                    }
                    list = list.child(row);
                }
                list.into_any_element()
            }
        };

        view.child(body).into_any_element()
    }

    pub(super) fn image_files_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(image) = self.files_image.clone() else {
            return div()
                .p_4()
                .text_sm()
                .text_color(rgb(theme().faint))
                .child("Select an image to browse its filesystem.")
                .into_any_element();
        };

        if let Some(view) = &self.file_view {
            return self.image_file_preview(view, cx);
        }

        let path = if self.files_dir.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", self.files_dir)
        };
        let mut header = div()
            .flex()
            .items_center()
            .justify_between()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(rgb(theme().border))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_xs()
                    .font_family("monospace")
                    .text_color(rgb(theme().text))
                    .child(truncate_xs(&path, self.effective_details_width() - 90.0)),
            );
        if !self.files_dir.is_empty() {
            let up = self.parent_dir();
            header = header.child(small_button(
                "files-up",
                "Up",
                false,
                cx.listener(move |this, _, _, cx| {
                    this.files_dir = up.clone();
                    this.file_view = None;
                    cx.notify();
                }),
            ));
        }

        let view = div().flex_1().min_h(px(0.)).flex().flex_col().child(header);

        let body: AnyElement = match &self.files {
            None => div()
                .p_4()
                .text_sm()
                .text_color(rgb(theme().faint))
                .child("Loading…")
                .into_any_element(),
            Some(state) if state.loading => div()
                .p_4()
                .text_sm()
                .text_color(rgb(theme().faint))
                .child("Exporting the image filesystem…")
                .into_any_element(),
            Some(state) if state.error.is_some() => div()
                .p_4()
                .text_sm()
                .text_color(rgb(theme().red))
                .child(state.error.clone().unwrap_or_default())
                .into_any_element(),
            Some(state) => {
                let mut entries: Vec<&podman::ImageFile> = state
                    .entries
                    .iter()
                    .filter(|entry| entry.parent() == self.files_dir)
                    .collect();
                entries.sort_by(|a, b| {
                    let dirs = (a.kind != podman::ImageFileKind::Directory)
                        .cmp(&(b.kind != podman::ImageFileKind::Directory));
                    if dirs == Ordering::Equal {
                        a.name().to_lowercase().cmp(&b.name().to_lowercase())
                    } else {
                        dirs
                    }
                });

                if entries.is_empty() {
                    div()
                        .p_4()
                        .text_sm()
                        .text_color(rgb(theme().faint))
                        .child("This directory is empty.")
                        .into_any_element()
                } else {
                    let mut list = div()
                        .id("files-scroll")
                        .flex_1()
                        .min_h(px(0.))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col();
                    let withheld = entries.len().saturating_sub(MAX_FILE_ROWS);
                    for entry in entries.iter().take(MAX_FILE_ROWS) {
                        list = list.child(self.file_row(&image, entry, cx));
                    }
                    if withheld > 0 {
                        list = list.child(
                            div()
                                .px_4()
                                .py_2()
                                .text_xs()
                                .text_color(rgb(theme().faint))
                                .child(format!("…and {withheld} more entries")),
                        );
                    }
                    list.into_any_element()
                }
            }
        };

        view.child(body).into_any_element()
    }

    fn file_row(
        &self,
        image: &str,
        entry: &podman::ImageFile,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_dir = entry.kind == podman::ImageFileKind::Directory;
        let is_link = entry.kind == podman::ImageFileKind::Link;
        let marker_color = if is_dir {
            theme().accent
        } else if is_link {
            theme().yellow
        } else {
            theme().faint
        };
        let name = entry.name().to_string();
        let path = entry.path.clone();
        let size = entry.size;
        let target = entry.target.clone();
        let link_target = entry.target.clone();
        let image = image.to_string();

        div()
            .id(SharedString::from(format!("file-{}", entry.path)))
            .flex()
            .items_center()
            .gap_2()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(rgb(theme().border))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(theme().hover_bg)))
            .on_click(cx.listener(move |this, _, _, cx| {
                if is_dir {
                    this.files_dir = path.clone();
                    this.file_view = None;
                    cx.notify();
                } else if is_link {
                    this.show_image_link(path.clone(), link_target.clone(), cx);
                } else {
                    this.open_image_file(image.clone(), path.clone(), Some(size), cx);
                }
            }))
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(marker_color))
                    .child(if is_dir {
                        "dir"
                    } else if is_link {
                        "→"
                    } else {
                        "·"
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_xs()
                    .text_color(rgb(theme().text))
                    .child(truncate_xs(&name, self.effective_details_width() - 130.0)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().muted))
                    .child(if is_dir {
                        String::new()
                    } else if let (true, Some(target)) = (is_link, target.as_deref()) {
                        truncate_xs(target, 120.0)
                    } else {
                        human_size(entry.size)
                    }),
            )
            .into_any_element()
    }

    fn image_file_preview(&self, view: &FileViewState, cx: &mut Context<Self>) -> AnyElement {
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(rgb(theme().border))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_xs()
                    .font_family("monospace")
                    .text_color(rgb(theme().text))
                    .child(truncate_xs(
                        &format!("/{}", view.path),
                        self.effective_details_width() - 90.0,
                    )),
            )
            .child(small_button(
                "file-back",
                "Back",
                false,
                cx.listener(|this, _, _, cx| {
                    this.file_view = None;
                    cx.notify();
                }),
            ));

        let body: AnyElement = match &view.content {
            FileContent::Loading => div()
                .p_4()
                .text_sm()
                .text_color(rgb(theme().faint))
                .child("Reading the file…")
                .into_any_element(),
            FileContent::Error(error) => div()
                .p_4()
                .text_sm()
                .text_color(rgb(theme().red))
                .child(error.clone())
                .into_any_element(),
            FileContent::Binary { size } => {
                let total = view.size.unwrap_or(*size as u64);
                div()
                    .p_4()
                    .text_sm()
                    .text_color(rgb(theme().muted))
                    .child(format!("Binary file ({}).", human_size(total)))
                    .into_any_element()
            }
            FileContent::Text { text, truncated } => {
                let mut list = div()
                    .id("file-scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .p_3()
                    .flex()
                    .flex_col();
                for line in text.lines().take(MAX_FILE_LINES) {
                    list = list.child(
                        div()
                            .text_xs()
                            .font_family("monospace")
                            .text_color(rgb(theme().text))
                            .child(if line.is_empty() {
                                " ".to_string()
                            } else {
                                line.to_string()
                            }),
                    );
                }
                let mut body = list;
                if *truncated {
                    body = body.child(div().pt_3().text_xs().text_color(rgb(theme().faint)).child(
                        format!(
                            "Showing the first {} of the file.",
                            human_size(podman::IMAGE_FILE_LIMIT as u64)
                        ),
                    ));
                }
                body.into_any_element()
            }
        };

        div()
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .child(header)
            .child(body)
            .into_any_element()
    }

    fn parent_dir(&self) -> String {
        self.files_dir
            .rsplit_once('/')
            .map_or(String::new(), |(parent, _)| parent.to_string())
    }
}
