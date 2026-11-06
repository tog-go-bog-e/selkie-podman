use super::helpers::*;
use super::*;

impl PodmanDesktop {
    // -----------------------------------------------------------------------
    // Compose project details and actions
    // -----------------------------------------------------------------------

    pub(super) fn compose_summary(
        &self,
        project: Option<&ComposeProject>,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut content = div()
            .id("compose-summary")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .p_4()
            .flex()
            .flex_col()
            .gap_3();

        let Some(project) = project else {
            return content
                .child(section_title("DETAILS"))
                .child(muted_text("This project is no longer listed by Podman."));
        };

        content = content
            .child(section_title("GENERAL"))
            .child(detail_row("Project", &project.name))
            .child(detail_row(
                "Containers",
                &format!("{} of {} running", project.running, project.total),
            ))
            .child(detail_row("Services", &project.services.len().to_string()));

        content = content.child(section_title("CONFIGURATION"));
        if project.config_files.is_empty() {
            content = content.child(muted_text(
                "No compose file recorded; actions run in the working directory.",
            ));
        } else {
            for file in &project.config_files {
                content = content.child(mono_text(file.clone()));
            }
        }
        if let Some(dir) = &project.working_dir {
            content = content.child(detail_row("Working directory", dir));
        }

        content = content.child(section_title("SERVICES"));
        if project.services.is_empty() {
            content = content.child(muted_text("No services"));
        } else {
            for service in &project.services {
                content = content.child(detail_row(
                    &service.name,
                    &format!("{} of {} running", service.running, service.containers),
                ));
            }
        }

        content
    }

    pub(super) fn compose_action_bar(
        &self,
        project: Option<&ComposeProject>,
        cx: &mut Context<Self>,
    ) -> Div {
        let bar = div()
            .flex_shrink_0()
            .border_t_1()
            .border_color(rgb(theme().border))
            .p_3()
            .flex()
            .items_center()
            .gap_2();

        if let Some(label) = &self.busy {
            return bar.child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().accent))
                    .child(label.clone()),
            );
        }

        let Some(project) = project else {
            return bar;
        };
        let available = self.output_run.is_none();
        let up_project = project.clone();
        let down_project = project.clone();
        let restart_project = project.clone();
        let build_project = project.clone();

        bar.child(pill_button(
            "compose-up",
            "Up",
            available,
            theme().accent,
            theme().accent_soft,
            cx.listener(move |this, _, _, cx| {
                this.start_compose(&up_project, ComposeAction::Up, cx);
            }),
        ))
        .child(pill_button(
            "compose-restart",
            "Restart",
            available,
            theme().muted,
            theme().hover_bg,
            cx.listener(move |this, _, _, cx| {
                this.start_compose(&restart_project, ComposeAction::Restart, cx);
            }),
        ))
        .child(pill_button(
            "compose-build",
            "Build",
            available,
            theme().muted,
            theme().hover_bg,
            cx.listener(move |this, _, _, cx| {
                this.start_compose(&build_project, ComposeAction::Build, cx);
            }),
        ))
        .child(pill_button(
            "compose-down",
            "Down",
            available,
            theme().red,
            theme().red_bg,
            cx.listener(move |this, _, window, cx| {
                this.open_modal(
                    Modal::ComposeDown {
                        project: down_project.clone(),
                    },
                    window,
                    cx,
                );
            }),
        ))
    }
}
