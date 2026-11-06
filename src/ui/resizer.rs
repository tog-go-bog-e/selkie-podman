//! Draggable splitters for the sidebar and the details pane.

use gpui::MouseMoveEvent;

use super::settings::{
    default_details_width, default_sidebar_width, DETAILS_MAX_WIDTH, DETAILS_MIN_WIDTH,
    PANEL_CONTENT_MIN_WIDTH, SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH,
};
use super::*;

impl PodmanDesktop {
    // -----------------------------------------------------------------------
    // Resizable panels
    // -----------------------------------------------------------------------

    /// A vertical splitter. The visible line is one pixel wide; the element is
    /// wider so it is comfortable to grab. Double-click resets the width.
    pub(super) fn resizer(&self, target: DragTarget, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.drag == Some(target);
        let id = match target {
            DragTarget::Sidebar => "resizer-sidebar",
            DragTarget::Details => "resizer-details",
        };

        div()
            .id(id)
            .flex_shrink_0()
            .flex()
            .justify_center()
            .w(px(6.))
            .h_full()
            .cursor_ew_resize()
            .when(active, |el| el.bg(rgb(theme().accent_soft)))
            .when(!active, |el| {
                el.hover(|style| style.bg(rgb(theme().hover_bg)))
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                    cx.stop_propagation();
                    if event.click_count >= 2 {
                        this.reset_panel_width(target, cx);
                    } else {
                        this.start_resize(target, f32::from(event.position.x), cx);
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.end_resize(cx);
                }),
            )
            .child(div().w(px(1.)).h_full().bg(rgb(if active {
                theme().accent
            } else {
                theme().border
            })))
    }

    fn start_resize(&mut self, target: DragTarget, x: f32, cx: &mut Context<Self>) {
        self.drag = Some(target);
        self.drag_start_x = x;
        self.drag_start_width = match target {
            DragTarget::Sidebar => self.effective_sidebar_width(),
            DragTarget::Details => self.effective_details_width(),
        };
        cx.notify();
    }

    pub(super) fn resize_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self.drag else {
            return;
        };
        let window_width = f32::from(window.bounds().size.width);
        if window_width > 0.0 {
            self.viewport_width = window_width;
        }

        let delta = f32::from(event.position.x) - self.drag_start_x;
        let (value, min, max) = match target {
            DragTarget::Sidebar => (
                self.drag_start_width + delta,
                SIDEBAR_MIN_WIDTH,
                self.sidebar_limit(),
            ),
            DragTarget::Details => (
                self.drag_start_width - delta,
                DETAILS_MIN_WIDTH,
                self.details_limit(),
            ),
        };
        let next = value.clamp(min, max);
        let current = match target {
            DragTarget::Sidebar => self.settings.sidebar_width,
            DragTarget::Details => self.settings.details_width,
        };
        if (next - current).abs() >= 0.5 {
            match target {
                DragTarget::Sidebar => self.settings.sidebar_width = next,
                DragTarget::Details => self.settings.details_width = next,
            }
            cx.notify();
        }
    }

    pub(super) fn end_resize(&mut self, cx: &mut Context<Self>) {
        if self.drag.take().is_some() {
            self.settings.save();
            cx.notify();
        }
    }

    fn reset_panel_width(&mut self, target: DragTarget, cx: &mut Context<Self>) {
        match target {
            DragTarget::Sidebar => self.settings.sidebar_width = default_sidebar_width(),
            DragTarget::Details => self.settings.details_width = default_details_width(),
        }
        self.settings.save();
        cx.notify();
    }

    /// Stored width, clamped so the list keeps a usable amount of space.
    pub(super) fn effective_sidebar_width(&self) -> f32 {
        self.settings
            .sidebar_width
            .clamp(SIDEBAR_MIN_WIDTH, self.sidebar_limit())
    }

    pub(super) fn effective_details_width(&self) -> f32 {
        self.settings
            .details_width
            .clamp(DETAILS_MIN_WIDTH, self.details_limit())
    }

    /// Width available to the table content: the window minus both side panels
    /// and the row padding. Used to budget text that must ellipsize.
    pub(super) fn list_content_width(&self) -> f32 {
        let details = if self.selection.is_some() {
            self.effective_details_width() + 6.0
        } else {
            0.0
        };
        (self.viewport_width - self.effective_sidebar_width() - 6.0 - details - 32.0).max(80.0)
    }

    fn sidebar_limit(&self) -> f32 {
        if self.viewport_width <= 0.0 {
            return SIDEBAR_MAX_WIDTH;
        }
        let reserved = if self.selection.is_some() {
            self.settings
                .details_width
                .clamp(DETAILS_MIN_WIDTH, DETAILS_MAX_WIDTH)
        } else {
            0.0
        };
        SIDEBAR_MAX_WIDTH
            .min(self.viewport_width - reserved - PANEL_CONTENT_MIN_WIDTH)
            .max(SIDEBAR_MIN_WIDTH)
    }

    fn details_limit(&self) -> f32 {
        if self.viewport_width <= 0.0 {
            return DETAILS_MAX_WIDTH;
        }
        DETAILS_MAX_WIDTH
            .min(self.viewport_width - self.effective_sidebar_width() - PANEL_CONTENT_MIN_WIDTH)
            .max(DETAILS_MIN_WIDTH)
    }
}
