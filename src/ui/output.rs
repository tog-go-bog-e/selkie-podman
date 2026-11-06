use super::helpers::*;
use super::*;

impl PodmanDesktop {
    // -----------------------------------------------------------------------
    // Streamed command output (compose actions, image builds)
    // -----------------------------------------------------------------------

    pub(super) fn output_panel(&self, cx: &mut Context<Self>) -> Div {
        let Some(title) = &self.output_title else {
            return div();
        };
        let running = self.output_run.is_some();

        let status = if running { "running…" } else { "finished" };
        let status_color = if running {
            theme().accent
        } else {
            theme().muted
        };

        let body: AnyElement = if self.output_lines.is_empty() {
            div()
                .p_4()
                .text_sm()
                .text_color(rgb(theme().faint))
                .child("Waiting for output…")
                .into_any_element()
        } else {
            let mut lines = div()
                .id("output-scroll")
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .track_scroll(&self.output_handle)
                .px_3()
                .py_2()
                .flex()
                .flex_col();
            for line in &self.output_lines {
                lines = lines.child(
                    div()
                        .text_xs()
                        .font_family("monospace")
                        .text_color(rgb(theme().text))
                        .child(line.clone()),
                );
            }
            lines.into_any_element()
        };

        div()
            .flex_shrink_0()
            .h(px(220.))
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(rgb(theme().border))
            .bg(rgb(theme().sidebar_bg))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .py_2()
                    .border_b_1()
                    .border_color(rgb(theme().border))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title.clone()),
                            )
                            .child(div().text_xs().text_color(rgb(status_color)).child(status)),
                    )
                    .when(!running, |el| {
                        el.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .child(small_button(
                                    "output-clear",
                                    "Clear",
                                    false,
                                    cx.listener(|this, _, _, cx| this.clear_output(cx)),
                                ))
                                .child(small_button(
                                    "output-close",
                                    "×",
                                    false,
                                    cx.listener(|this, _, _, cx| this.clear_output(cx)),
                                )),
                        )
                    }),
            )
            .child(body)
    }
}
