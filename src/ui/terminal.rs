use std::time::Duration;

use gpui::{
    FontWeight, KeyDownEvent, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Point, SharedString,
};

use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{
    Color as AnsiColor, CursorShape as TermCursorShape, NamedColor, Rgb,
};

use super::helpers::*;
use super::*;
use crate::terminal::{
    ansi_color, MouseEncoding, MouseMode, TerminalSession, TERMINAL_COLS, TERMINAL_ROWS,
};

/// Exit code of `podman exec` when the requested executable does not exist.
const NO_SHELL_EXIT: i32 = 127;

/// Height of one terminal row, in logical pixels.
const LINE_HEIGHT: f32 = 18.0;
/// Size of the terminal font, in logical pixels.
const FONT_SIZE: f32 = 12.0;
/// Families that look right for a terminal, most preferred first.
const PREFERRED_FONTS: &[&str] = &[
    "JetBrains Mono",
    "JetBrainsMono Nerd Font Mono",
    "Cascadia Mono",
    "Fira Code",
    "DejaVu Sans Mono",
    "Liberation Mono",
    "Noto Sans Mono",
    "Ubuntu Mono",
    "Consolas",
    "Menlo",
    "Monaco",
    "SF Mono",
];

/// The monospace font used to draw the grid, and the measured advance of a
/// single character. `TextSystem::advance` is not free, so both are resolved
/// once and reused.
#[derive(Clone)]
pub(super) struct TerminalMetrics {
    family: SharedString,
    char_width: f32,
}

/// A cell in the terminal viewport (not the scrollback as a whole).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct TermPos {
    row: u16,
    col: u16,
}

/// Everything that makes one run of cells share a rendered span.
#[derive(Clone, Copy, PartialEq, Eq)]
struct CellStyle {
    fg: u32,
    bg: u32,
    bold: bool,
    italic: bool,
    underline: bool,
}

impl Selkie {
    // -----------------------------------------------------------------------
    // Exec terminal
    // -----------------------------------------------------------------------

    pub(super) fn start_container_terminal(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.stop_terminal();
        self.terminal_target = Some(TerminalTarget::Container(id.clone()));
        self.terminal_label = "/bin/sh".to_string();

        let running = self
            .snapshot
            .containers
            .iter()
            .find(|container| container.id == id)
            .is_some_and(|container| container.state == "running");
        if !running {
            self.terminal_error = Some("Container is not running.".to_string());
            cx.notify();
            return;
        }

        let command = podman::exec_terminal_command(&id);
        self.open_terminal(command, window, cx);
    }

    /// Open an SSH session on a Podman machine in the details pane.
    pub(super) fn start_machine_terminal(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.stop_terminal();
        self.terminal_target = Some(TerminalTarget::Machine(name.clone()));
        self.terminal_label = format!("podman machine ssh {name}");

        let running = self
            .local
            .machines
            .iter()
            .find(|machine| machine.name == name)
            .is_some_and(|machine| machine.running);
        if !running {
            self.terminal_error = Some("Machine is not running.".to_string());
            cx.notify();
            return;
        }

        match podman::machine_ssh_command(&name) {
            Ok(command) => self.open_terminal(command, window, cx),
            Err(error) => {
                self.terminal_error = Some(error);
                cx.notify();
            }
        }
    }

    fn open_terminal(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        match TerminalSession::start(command, TERMINAL_ROWS, TERMINAL_COLS) {
            Ok(session) => {
                self.terminal = Some(session);
                self.spawn_terminal_poll(cx);
                self.terminal_focus.focus(window);
            }
            Err(error) => self.terminal_error = Some(error),
        }
        cx.notify();
    }

    pub(super) fn stop_terminal(&mut self) {
        self.terminal_task = None;
        self.terminal = None;
        self.terminal_target = None;
        self.terminal_selection = None;
        self.terminal_label.clear();
        self.terminal_error = None;
    }

    fn spawn_terminal_poll(&mut self, cx: &mut Context<Self>) {
        let task = cx.spawn(async move |this, cx| loop {
            let stop = this
                .update(cx, |this, cx| {
                    let selected = this.terminal_selected();

                    // The grid measures itself during prepaint; apply any size
                    // change here, where we have the session mutably.
                    let hint = this.terminal_size.get();
                    if hint.0 > 0 && hint.1 > 0 {
                        if let Some(session) = this.terminal.as_mut() {
                            if session.size() != hint {
                                session.resize(hint.0, hint.1);
                                cx.notify();
                            }
                        }
                    }

                    let stop = match this.terminal.as_mut() {
                        Some(session) => {
                            let was_closed = session.closed;
                            let changed = session.drain();
                            // OSC 52 copies have to reach the system clipboard
                            // through the window, which only the UI thread can do.
                            if let Some(text) = session.take_clipboard() {
                                if !text.is_empty() {
                                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                                }
                            }
                            let just_closed = !was_closed && session.closed;
                            if changed || just_closed {
                                cx.notify();
                            }
                            session.closed || !selected
                        }
                        None => true,
                    };

                    if !selected {
                        this.terminal = None;
                        this.terminal_target = None;
                        this.terminal_label.clear();
                        this.terminal_error = None;
                    }
                    stop
                })
                .unwrap_or(true);

            if stop {
                break;
            }
            cx.background_executor()
                .timer(Duration::from_millis(40))
                .await;
        });
        self.terminal_task = Some(task);
    }

    fn terminal_selected(&self) -> bool {
        match (&self.selection, &self.terminal_target) {
            (Some(Selection::Container(id)), Some(TerminalTarget::Container(target))) => {
                id == target
            }
            (Some(Selection::Machine(name)), Some(TerminalTarget::Machine(target))) => {
                name == target
            }
            _ => false,
        }
    }

    pub(super) fn terminal_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut view = div().flex_1().min_h(px(0.)).flex().flex_col();

        let closed = self.terminal.as_ref().is_some_and(|session| session.closed);
        // `podman exec` exits with 127 when the executable is missing: the
        // image has no `/bin/sh` (distroless, scratch).
        let no_shell = closed
            && matches!(self.terminal_target, Some(TerminalTarget::Container(_)))
            && self
                .terminal
                .as_ref()
                .is_some_and(|session| session.exit_code == Some(NO_SHELL_EXIT));
        let status = if self.terminal_error.is_some() {
            "error"
        } else if no_shell {
            "session ended: this image has no /bin/sh"
        } else if closed {
            "session ended"
        } else {
            "connected"
        };
        let status_color = if self.terminal_error.is_some() || closed {
            theme().red
        } else {
            theme().green
        };

        let scrollback = self
            .terminal
            .as_ref()
            .map_or(0, |session| session.scrollback());
        // Programs that set the window title (OSC 0/2) get to describe
        // themselves; otherwise show what we launched.
        let title = self
            .terminal
            .as_ref()
            .and_then(|session| session.title())
            .unwrap_or_else(|| self.terminal_label.clone());

        view = view.child(
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
                        .min_w(px(0.))
                        .child(
                            div()
                                .min_w(px(0.))
                                .text_xs()
                                .font_family("monospace")
                                .text_color(rgb(theme().text))
                                .truncate()
                                .child(title),
                        )
                        .child(div().text_xs().text_color(rgb(status_color)).child(status))
                        .when(scrollback > 0, |el| {
                            el.child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(theme().yellow))
                                    .child(format!("{scrollback} lines back")),
                            )
                        }),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .when(scrollback > 0, |el| {
                            el.child(small_button(
                                "terminal-live",
                                "Live",
                                false,
                                cx.listener(|this, _, window, cx| {
                                    if let Some(session) = this.terminal.as_mut() {
                                        session.scroll_to_bottom();
                                    }
                                    // Keep the keyboard on the grid: clicking a
                                    // header button must not steal focus.
                                    this.terminal_focus.focus(window);
                                    cx.notify();
                                }),
                            ))
                        })
                        .child(small_button(
                            "terminal-restart",
                            "Restart",
                            false,
                            cx.listener(|this, _, window, cx| match this.terminal_target.clone() {
                                Some(TerminalTarget::Container(id)) => {
                                    this.start_container_terminal(id, window, cx)
                                }
                                Some(TerminalTarget::Machine(name)) => {
                                    this.start_machine_terminal(name, window, cx)
                                }
                                None => {}
                            }),
                        )),
                ),
        );

        let body: AnyElement = if let Some(error) = &self.terminal_error {
            div()
                .p_4()
                .text_sm()
                .text_color(rgb(theme().red))
                .child(error.clone())
                .into_any_element()
        } else if self.terminal.is_none() {
            div()
                .p_4()
                .text_sm()
                .text_color(rgb(theme().faint))
                .child("Starting terminal…")
                .into_any_element()
        } else {
            self.terminal_grid(cx)
        };

        view.child(body).into_any_element()
    }

    /// Resolve the monospace font and its cell advance the first time a grid is
    /// painted. Both stay stable for the lifetime of the process.
    fn terminal_metrics(&self, text_system: &gpui::TextSystem) -> TerminalMetrics {
        if let Some(metrics) = self.terminal_font.borrow().as_ref() {
            return metrics.clone();
        }

        let family = pick_monospace_family(text_system.all_font_names());
        let font = gpui::font(family.clone());
        let font_id = text_system.resolve_font(&font);
        let char_width = text_system
            .advance(font_id, px(FONT_SIZE), 'M')
            .map(|size| f32::from(size.width))
            .unwrap_or(FONT_SIZE * 0.6);

        let metrics = TerminalMetrics { family, char_width };
        *self.terminal_font.borrow_mut() = Some(metrics.clone());
        metrics
    }

    fn terminal_grid(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(session) = self.terminal.as_ref() else {
            return div().into_any_element();
        };
        let metrics = self.terminal_metrics(cx.text_system());
        let family = metrics.family.clone();
        let char_width = metrics.char_width;
        let content = session.renderable();
        let (rows, cols) = session.size();

        // Programs choose the cursor shape with DECSCUSR; the default is a
        // steady block. The emulator reports a hidden shape when the program
        // asked for that (or when VI mode is idle).
        let (cursor_shape, hidden) = match content.cursor.shape {
            TermCursorShape::Block | TermCursorShape::HollowBlock => (CursorShape::Block, false),
            TermCursorShape::Underline => (CursorShape::Underline, false),
            TermCursorShape::Beam => (CursorShape::Bar, false),
            TermCursorShape::Hidden => (CursorShape::Block, true),
        };
        let cursor_row = content
            .cursor
            .point
            .line
            .0
            .clamp(0, rows.saturating_sub(1) as i32) as u16;
        let cursor_col = content
            .cursor
            .point
            .column
            .0
            .min(cols.saturating_sub(1) as usize) as u16;
        // The cursor belongs to the live viewport, and blinking shapes follow
        // the same 2 Hz tick as the text carets.
        let blinking = session.cursor_blinks();
        let show_cursor =
            !hidden && !session.closed && session.scrollback() == 0 && (!blinking || self.caret_on);

        let selection = self
            .terminal_selection
            .map(|(anchor, active)| terminal_selection_range(anchor, active));

        let mut lines = div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .w_full();

        for row in 0..rows {
            let mut line = div().flex().items_center().h(px(LINE_HEIGHT)).w_full();
            let mut text = String::new();
            let mut style: Option<CellStyle> = None;

            for col in 0..cols {
                let Some(cell) = session.cell(row, col) else {
                    continue;
                };
                // The wide character already covers two columns; its spacer is
                // not drawn.
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }

                let mut fg = terminal_color(cell.fg, content.colors, theme().text);
                let mut bg = terminal_color(cell.bg, content.colors, theme().bg);
                if cell.flags.contains(Flags::INVERSE) {
                    std::mem::swap(&mut fg, &mut bg);
                }
                if cell.flags.contains(Flags::DIM) {
                    fg = blend(fg, bg, 0.45);
                }

                let at_cursor = show_cursor && row == cursor_row && col == cursor_col;
                let block_cursor = at_cursor && cursor_shape == CursorShape::Block;
                if selection.is_some_and(|range| contains(range, row, col)) {
                    bg = theme().accent_soft;
                }
                if block_cursor {
                    std::mem::swap(&mut fg, &mut bg);
                }

                let cell_style = CellStyle {
                    fg,
                    bg,
                    bold: cell.flags.contains(Flags::BOLD),
                    italic: cell.flags.contains(Flags::ITALIC),
                    underline: cell.flags.intersects(Flags::ALL_UNDERLINES),
                };
                if style != Some(cell_style) {
                    if let Some(previous) = style {
                        line = line.child(terminal_run(&text, previous, &family));
                    }
                    text.clear();
                    style = Some(cell_style);
                }

                // A plain space collapses to zero width, so a coloured or
                // reversed background would never paint; use a non-breaking
                // space wherever the cell's background is not the default.
                if cell.flags.contains(Flags::HIDDEN) {
                    text.push(if bg != theme().bg { '\u{00A0}' } else { ' ' });
                } else if cell.c == ' ' && bg != theme().bg {
                    text.push('\u{00A0}');
                } else {
                    text.push(cell.c);
                    if let Some(zerowidth) = cell.zerowidth() {
                        text.extend(zerowidth.iter().copied());
                    }
                }
            }

            if let Some(style) = style {
                line = line.child(terminal_run(&text, style, &family));
            }
            lines = lines.child(line);
        }

        // A bar or underline cursor cannot be drawn by recolouring a cell, so
        // lay a small rectangle over it instead.
        if show_cursor && cursor_shape != CursorShape::Block {
            let (left, top, width, height) = match cursor_shape {
                CursorShape::Underline => (
                    f32::from(cursor_col) * char_width,
                    f32::from(cursor_row) * LINE_HEIGHT + LINE_HEIGHT - 2.0,
                    char_width,
                    2.0,
                ),
                _ => (
                    f32::from(cursor_col) * char_width,
                    f32::from(cursor_row) * LINE_HEIGHT,
                    2.0,
                    LINE_HEIGHT,
                ),
            };
            lines = lines.child(
                div()
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(width))
                    .h(px(height))
                    .bg(rgb(theme().text)),
            );
        }

        // The grid reports its own size here so the session can resize the
        // emulator and the pty to match the pane. The width comes from the
        // font's real advance: a guess would leave the last columns unclaimed.
        // The origin lets mouse events be mapped back to cells.
        let hint = self.terminal_size.clone();
        let origin = self.terminal_origin.clone();
        div()
            .on_children_prepainted(
                move |bounds: Vec<Bounds<Pixels>>, _window: &mut Window, _app: &mut App| {
                    let Some(bounds) = bounds.first() else {
                        return;
                    };
                    origin.set((f32::from(bounds.origin.x), f32::from(bounds.origin.y)));
                    // Half a pixel of slack: `7.2 * 100` is `720.00003` in f32,
                    // which would otherwise cost a whole column.
                    let cols = ((f32::from(bounds.size.width) + 0.5) / char_width).floor();
                    let rows = (f32::from(bounds.size.height) / LINE_HEIGHT).floor();
                    if cols >= 20.0 && rows >= 2.0 {
                        hint.set((rows.min(400.0) as u16, cols.min(600.0) as u16));
                    }
                },
            )
            .on_any_mouse_down(cx.listener(Self::terminal_mouse_down))
            .capture_any_mouse_up(cx.listener(Self::terminal_mouse_up))
            .on_mouse_move(cx.listener(Self::terminal_mouse_move))
            .id("terminal-grid")
            .flex_1()
            .min_h(px(0.))
            .overflow_hidden()
            .px_2()
            .py_1()
            .bg(rgb(theme().bg))
            .track_focus(&self.terminal_focus)
            .on_key_down(cx.listener(Self::terminal_key))
            .on_scroll_wheel(cx.listener(Self::terminal_scroll))
            .flex()
            .flex_col()
            .child(lines)
            .into_any_element()
    }

    /// Mouse wheel scrolls the emulator's scrollback, unless the running
    /// program asked to receive mouse events.
    fn terminal_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (sideways, lines) = match event.delta {
            ScrollDelta::Lines(point) => (point.x, point.y),
            ScrollDelta::Pixels(point) => (
                f32::from(point.x) / LINE_HEIGHT,
                f32::from(point.y) / LINE_HEIGHT,
            ),
        };
        if lines.abs() < 0.01 && sideways.abs() < 0.01 {
            return;
        }

        let Some(session) = self.terminal.as_ref() else {
            return;
        };
        if session.mouse_mode() != MouseMode::None {
            // One report per event: X11 delivers exactly one event per notch, and
            // the program's own bindings decide how far to scroll.
            let cell = self
                .terminal_cell_at(event.position)
                .unwrap_or(TermPos { row: 0, col: 0 });
            let button = if lines.abs() >= sideways.abs() {
                if lines > 0.0 {
                    TermMouseButton::WheelUp
                } else {
                    TermMouseButton::WheelDown
                }
            } else if sideways > 0.0 {
                TermMouseButton::WheelLeft
            } else {
                TermMouseButton::WheelRight
            };
            let bytes = encode_mouse(
                session.mouse_encoding(),
                MouseAction::Press,
                button,
                cell.col,
                cell.row,
                event.modifiers,
            );
            session.write(&bytes);
            return;
        }

        // One wheel notch moves three rows, like most terminal emulators.
        let delta = (lines * 3.0).round() as isize;
        if let Some(session) = self.terminal.as_mut() {
            if session.scroll_by(delta) {
                // Old coordinates no longer match what is drawn.
                self.terminal_selection = None;
                cx.notify();
            }
        }
    }

    fn terminal_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.terminal_focus.focus(window);
        let Some(cell) = self.terminal_cell_at(event.position) else {
            return;
        };
        let Some(session) = self.terminal.as_ref() else {
            return;
        };

        if session.mouse_mode() != MouseMode::None {
            let bytes = encode_mouse(
                session.mouse_encoding(),
                MouseAction::Press,
                term_mouse_button(event.button),
                cell.col,
                cell.row,
                event.modifiers,
            );
            session.write(&bytes);
            return;
        }

        if event.button != MouseButton::Left {
            return;
        }
        let last_col = session.size().1.saturating_sub(1);
        self.terminal_selection = match event.click_count {
            // Double-click selects the word, triple-click the whole line.
            2 => Some(self.terminal_word_at(cell)),
            count if count >= 3 => Some((
                TermPos {
                    row: cell.row,
                    col: 0,
                },
                TermPos {
                    row: cell.row,
                    col: last_col,
                },
            )),
            _ => Some((cell, cell)),
        };
        cx.notify();
    }

    fn terminal_mouse_up(
        &mut self,
        event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(cell) = self.terminal_cell_at(event.position) else {
            return;
        };
        let Some(session) = self.terminal.as_ref() else {
            return;
        };
        if session.mouse_mode() == MouseMode::None {
            // A click that did not turn into a drag leaves no selection.
            if let Some((anchor, active)) = self.terminal_selection {
                if anchor == active {
                    self.terminal_selection = None;
                    cx.notify();
                }
            }
            return;
        }
        let bytes = encode_mouse(
            session.mouse_encoding(),
            MouseAction::Release,
            term_mouse_button(event.button),
            cell.col,
            cell.row,
            event.modifiers,
        );
        session.write(&bytes);
        cx.notify();
    }

    fn terminal_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(cell) = self.terminal_cell_at(event.position) else {
            return;
        };
        let Some(session) = self.terminal.as_ref() else {
            return;
        };
        let mode = session.mouse_mode();

        if mode != MouseMode::None {
            let motion = match mode {
                MouseMode::Motion => true,
                MouseMode::Drag => event.pressed_button.is_some(),
                _ => false,
            };
            if motion {
                let bytes = encode_mouse(
                    session.mouse_encoding(),
                    MouseAction::Motion,
                    event
                        .pressed_button
                        .map_or(TermMouseButton::None, term_mouse_button),
                    cell.col,
                    cell.row,
                    event.modifiers,
                );
                session.write(&bytes);
            }
            return;
        }

        // Dragging out a selection.
        if event.pressed_button == Some(MouseButton::Left) {
            if let Some((anchor, active)) = self.terminal_selection {
                if active != cell {
                    self.terminal_selection = Some((anchor, cell));
                    cx.notify();
                }
            }
        }
    }

    /// Map a window position to the cell under it, clamped to the grid.
    fn terminal_cell_at(&self, position: Point<Pixels>) -> Option<TermPos> {
        let session = self.terminal.as_ref()?;
        let metrics = self.terminal_font.borrow();
        let char_width = metrics.as_ref()?.char_width;
        let (origin_x, origin_y) = self.terminal_origin.get();
        let (rows, cols) = session.size();

        let x = f32::from(position.x) - origin_x;
        let y = f32::from(position.y) - origin_y;
        if x < 0.0 || y < 0.0 {
            return None;
        }
        let col = (x / char_width)
            .floor()
            .min(f32::from(cols.saturating_sub(1)));
        let row = (y / LINE_HEIGHT)
            .floor()
            .min(f32::from(rows.saturating_sub(1)));
        Some(TermPos {
            row: row as u16,
            col: col as u16,
        })
    }

    /// The word under the cursor, for double-click selection.
    fn terminal_word_at(&self, cell: TermPos) -> (TermPos, TermPos) {
        let Some(session) = self.terminal.as_ref() else {
            return (cell, cell);
        };
        let cols = session.size().1;
        let is_word = |col: u16| {
            session.cell(cell.row, col).is_some_and(|cell| {
                !cell.c.is_whitespace() && !cell.flags.contains(Flags::WIDE_CHAR_SPACER)
            })
        };
        if !is_word(cell.col) {
            return (cell, cell);
        }
        let mut start = cell.col;
        while start > 0 && is_word(start - 1) {
            start -= 1;
        }
        let mut end = cell.col;
        while end + 1 < cols && is_word(end + 1) {
            end += 1;
        }
        (
            TermPos {
                row: cell.row,
                col: start,
            },
            TermPos {
                row: cell.row,
                col: end,
            },
        )
    }

    /// Copy the selected cells to the system clipboard. Returns whether there
    /// was a selection to copy.
    fn copy_terminal_selection(&mut self, cx: &mut Context<Self>) -> bool {
        let Some((anchor, active)) = self.terminal_selection else {
            return false;
        };
        let Some(session) = self.terminal.as_ref() else {
            return false;
        };
        let (start, end) = terminal_selection_range(anchor, active);
        let text = session.text_between((start.row, start.col), (end.row, end.col));
        if text.is_empty() {
            return false;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        true
    }

    /// Send the clipboard's text to the shell, bracketed if it asked for it.
    fn paste_into_terminal(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return false;
        };
        if text.is_empty() {
            return false;
        }
        let Some(session) = self.terminal.as_mut() else {
            return false;
        };
        if session.closed {
            return false;
        }
        session.scroll_to_bottom();
        session.write(&encode_paste(&text, session.bracketed_paste()));
        true
    }

    fn terminal_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        let keystroke = &event.keystroke;

        // Terminal shortcuts take precedence over the shell's Ctrl+letter
        // bindings.
        if keystroke.modifiers.control && keystroke.modifiers.shift {
            match keystroke.key.as_str() {
                "c" => {
                    if self.copy_terminal_selection(cx) {
                        cx.notify();
                    }
                    return;
                }
                "v" => {
                    if self.paste_into_terminal(cx) {
                        cx.notify();
                    }
                    return;
                }
                _ => {}
            }
        }

        // Full-screen programs put the terminal in application cursor mode and
        // expect `ESC O x` for the arrows.
        let application_cursor = self
            .terminal
            .as_ref()
            .is_some_and(|session| session.application_cursor());
        if let Some(bytes) = encode_key(
            keystroke.key.as_str(),
            keystroke.modifiers.control,
            keystroke.modifiers.alt,
            keystroke.key_char.as_deref(),
            application_cursor,
        ) {
            if let Some(session) = self.terminal.as_mut() {
                if !session.closed {
                    // Typing takes you back to the live viewport, like xterm.
                    session.scroll_to_bottom();
                    session.write(&bytes);
                }
            }
            cx.notify();
        }
    }
}

fn pick_monospace_family(names: Vec<String>) -> SharedString {
    for preferred in PREFERRED_FONTS {
        if names.iter().any(|name| name == preferred) {
            return SharedString::from(*preferred);
        }
    }
    // "monospace" is only a fontconfig alias, so prefer any family that
    // advertises itself as monospaced before settling for it.
    if let Some(name) = names
        .iter()
        .find(|name| name.to_ascii_lowercase().contains("mono"))
    {
        return SharedString::from(name.clone());
    }
    SharedString::from("monospace")
}

/// Cursor shape requested by the program (`DECSCUSR`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum CursorShape {
    Block,
    Underline,
    Bar,
}

/// Order two selection endpoints so the first one precedes the second.
fn terminal_selection_range(anchor: TermPos, active: TermPos) -> (TermPos, TermPos) {
    let anchor_first = (anchor.row, anchor.col) <= (active.row, active.col);
    if anchor_first {
        (anchor, active)
    } else {
        (active, anchor)
    }
}

/// Whether `(row, col)` falls inside an inclusive selection range.
fn contains(range: (TermPos, TermPos), row: u16, col: u16) -> bool {
    let (start, end) = range;
    let after_start = (row, col) >= (start.row, start.col);
    let before_end = (row, col) <= (end.row, end.col);
    after_start && before_end
}

/// Linear blend of two `0xRRGGBB` colors; `amount` is the weight of `to`.
fn blend(from: u32, to: u32, amount: f32) -> u32 {
    let channel = |shift: u32| {
        let start = ((from >> shift) & 0xff) as f32;
        let end = ((to >> shift) & 0xff) as f32;
        (start + (end - start) * amount).round() as u32
    };
    (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

fn term_mouse_button(button: MouseButton) -> TermMouseButton {
    match button {
        MouseButton::Left => TermMouseButton::Left,
        MouseButton::Middle => TermMouseButton::Middle,
        MouseButton::Right => TermMouseButton::Right,
        _ => TermMouseButton::None,
    }
}

/// Buttons the running program can be told about.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TermMouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
    WheelLeft,
    WheelRight,
    None,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MouseAction {
    Press,
    Release,
    Motion,
}

/// Encode a mouse report in the wire format the program asked for.
fn encode_mouse(
    encoding: MouseEncoding,
    action: MouseAction,
    button: TermMouseButton,
    col: u16,
    row: u16,
    modifiers: Modifiers,
) -> Vec<u8> {
    let base = match button {
        TermMouseButton::Left => 0,
        TermMouseButton::Middle => 1,
        TermMouseButton::Right => 2,
        TermMouseButton::WheelUp => 64,
        TermMouseButton::WheelDown => 65,
        TermMouseButton::WheelLeft => 66,
        TermMouseButton::WheelRight => 67,
        TermMouseButton::None => 3,
    };
    let modifiers = u32::from(modifiers.shift) * 4
        + u32::from(modifiers.alt) * 8
        + u32::from(modifiers.control) * 16;

    let code = match action {
        MouseAction::Motion => base + 32 + modifiers,
        // SGR keeps the button on release and marks it with `m` instead; the
        // legacy encodings report button 3.
        MouseAction::Release if encoding == MouseEncoding::Sgr => base + modifiers,
        MouseAction::Release => 3 + modifiers,
        MouseAction::Press => base + modifiers,
    };

    if encoding == MouseEncoding::Sgr {
        let terminator = if action == MouseAction::Release {
            'm'
        } else {
            'M'
        };
        return format!("\x1b[<{code};{};{}{terminator}", col + 1, row + 1).into_bytes();
    }

    // The legacy encodings carry one byte per field; the UTF-8 variant lifts
    // the 223-coordinate limit by sending code points instead.
    let mut bytes = vec![0x1b, b'[', b'M'];
    for value in [code, u32::from(col) + 1, u32::from(row) + 1] {
        let value = if encoding == MouseEncoding::Utf8 {
            32 + value
        } else {
            32 + value.min(223)
        };
        if let Some(character) = char::from_u32(value) {
            let mut buffer = [0u8; 4];
            bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
        }
    }
    bytes
}

/// Bytes for a paste. Bracketed paste keeps multi-line text from being run
/// line by line; without it the shell expects carriage returns.
fn encode_paste(text: &str, bracketed: bool) -> Vec<u8> {
    let normalized = text.replace("\r\n", "\n");
    if bracketed {
        // Pasted text must not be able to close the bracket itself.
        let sanitized = normalized.replace("\x1b[201~", "");
        let mut bytes = b"\x1b[200~".to_vec();
        bytes.extend_from_slice(sanitized.as_bytes());
        bytes.extend_from_slice(b"\x1b[201~");
        bytes
    } else {
        normalized.replace('\n', "\r").into_bytes()
    }
}

fn terminal_run(text: &str, style: CellStyle, family: &SharedString) -> Div {
    div()
        .flex_shrink_0()
        .whitespace_nowrap()
        .text_size(px(FONT_SIZE))
        .font_family(family.clone())
        .text_color(rgb(style.fg))
        .when(style.bg != theme().bg, |el| el.bg(rgb(style.bg)))
        .when(style.bold, |el| el.font_weight(FontWeight::BOLD))
        .when(style.italic, |el| el.italic())
        .when(style.underline, |el| el.underline())
        .child(text.to_string())
}

fn terminal_color(color: AnsiColor, colors: &Colors, default: u32) -> u32 {
    match color {
        AnsiColor::Named(named) => named_color(named, colors, default),
        AnsiColor::Indexed(index) => {
            colors[usize::from(index)].map_or_else(|| ansi_color(index), rgb_value)
        }
        AnsiColor::Spec(rgb) => rgb_value(rgb),
    }
}

/// Resolve an ANSI named colour against the palette (which OSC 4 can redefine)
/// and, for the default foreground/background, the app theme.
fn named_color(named: NamedColor, colors: &Colors, default: u32) -> u32 {
    if let Some(rgb) = colors[named] {
        return rgb_value(rgb);
    }
    match named {
        NamedColor::Foreground | NamedColor::BrightForeground => theme().text,
        NamedColor::Background => theme().bg,
        NamedColor::Cursor => default,
        NamedColor::DimForeground => blend(theme().text, theme().bg, 0.45),
        NamedColor::DimBlack => ansi_color(0),
        NamedColor::DimRed => ansi_color(1),
        NamedColor::DimGreen => ansi_color(2),
        NamedColor::DimYellow => ansi_color(3),
        NamedColor::DimBlue => ansi_color(4),
        NamedColor::DimMagenta => ansi_color(5),
        NamedColor::DimCyan => ansi_color(6),
        NamedColor::DimWhite => ansi_color(7),
        // Black through BrightWhite occupy the first sixteen indices.
        other => ansi_color(other as u8),
    }
}

fn rgb_value(rgb: Rgb) -> u32 {
    ((rgb.r as u32) << 16) | ((rgb.g as u32) << 8) | rgb.b as u32
}

/// Translate a GPUI keystroke into bytes for the PTY.
fn encode_key(
    key: &str,
    control: bool,
    alt: bool,
    key_char: Option<&str>,
    application_cursor: bool,
) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();

    if control {
        if key.len() == 1 {
            let character = key.chars().next()?;
            if character.is_ascii_alphabetic() {
                bytes.push(character.to_ascii_uppercase() as u8 - b'A' + 1);
                return Some(bytes);
            }
            if character == '[' {
                bytes.push(0x1b);
                return Some(bytes);
            }
        }
        match key {
            "space" => {
                bytes.push(0);
                return Some(bytes);
            }
            _ => return None,
        }
    }

    match key {
        "enter" | "return" => bytes.push(b'\r'),
        "tab" => bytes.push(b'\t'),
        "backspace" => bytes.push(0x7f),
        "escape" => bytes.push(0x1b),
        "up" => push_cursor_key(&mut bytes, b'A', application_cursor),
        "down" => push_cursor_key(&mut bytes, b'B', application_cursor),
        "right" => push_cursor_key(&mut bytes, b'C', application_cursor),
        "left" => push_cursor_key(&mut bytes, b'D', application_cursor),
        "home" => push_cursor_key(&mut bytes, b'H', application_cursor),
        "end" => push_cursor_key(&mut bytes, b'F', application_cursor),
        "delete" => bytes.extend_from_slice(b"\x1b[3~"),
        "pageup" => bytes.extend_from_slice(b"\x1b[5~"),
        "pagedown" => bytes.extend_from_slice(b"\x1b[6~"),
        _ => {
            let text = key_char?;
            bytes.extend_from_slice(text.as_bytes());
        }
    }

    if alt && !bytes.starts_with(b"\x1b") {
        bytes.insert(0, 0x1b);
    }
    Some(bytes)
}

fn push_cursor_key(bytes: &mut Vec<u8>, code: u8, application_cursor: bool) {
    bytes.extend_from_slice(if application_cursor {
        b"\x1bO"
    } else {
        b"\x1b["
    });
    bytes.push(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_common_keys() {
        assert_eq!(
            encode_key("a", false, false, Some("a"), false),
            Some(b"a".to_vec())
        );
        assert_eq!(
            encode_key("A", false, false, Some("A"), false),
            Some(b"A".to_vec())
        );
        assert_eq!(encode_key("c", true, false, None, false), Some(vec![3]));
        assert_eq!(
            encode_key("up", false, false, None, false),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            encode_key("enter", false, false, None, false),
            Some(b"\r".to_vec())
        );
        assert_eq!(
            encode_key("tab", false, false, None, false),
            Some(b"\t".to_vec())
        );
        assert_eq!(
            encode_key("backspace", false, false, None, false),
            Some(vec![0x7f])
        );
        assert_eq!(
            encode_key("a", false, true, Some("a"), false),
            Some(b"\x1ba".to_vec())
        );
        assert_eq!(encode_key("shift", false, false, None, false), None);

        // Application cursor mode (DECCKM) uses SS3 sequences.
        assert_eq!(
            encode_key("up", false, false, None, true),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            encode_key("left", false, false, None, true),
            Some(b"\x1bOD".to_vec())
        );
        assert_eq!(
            encode_key("home", false, false, None, true),
            Some(b"\x1bOH".to_vec())
        );
    }

    #[test]
    fn maps_ansi_colors() {
        assert_eq!(ansi_color(0), 0x1e1e1e);
        assert_eq!(ansi_color(7), 0xe5e5e5);
        assert_eq!(ansi_color(16), 0x000000);
        assert_eq!(ansi_color(21), 0x0000ff);
        assert_eq!(ansi_color(255), 0xeeeeee);
    }

    #[test]
    fn picks_a_preferred_monospace_font() {
        let names = vec![
            "Arial".to_string(),
            "DejaVu Sans Mono".to_string(),
            "Liberation Mono".to_string(),
        ];
        assert_eq!(pick_monospace_family(names).as_ref(), "DejaVu Sans Mono");
    }

    #[test]
    fn picks_any_monospace_font_when_none_is_preferred() {
        let names = vec!["Arial".to_string(), "SomeMonoFont".to_string()];
        assert_eq!(pick_monospace_family(names).as_ref(), "SomeMonoFont");
    }

    #[test]
    fn falls_back_to_the_generic_family() {
        let names = vec!["Arial".to_string()];
        assert_eq!(pick_monospace_family(names).as_ref(), "monospace");
    }

    fn modifiers(shift: bool, alt: bool, control: bool) -> Modifiers {
        Modifiers {
            shift,
            alt,
            control,
            ..Modifiers::default()
        }
    }

    #[test]
    fn encodes_sgr_mouse_reports() {
        let none = Modifiers::default();
        let sgr = MouseEncoding::Sgr;
        assert_eq!(
            encode_mouse(sgr, MouseAction::Press, TermMouseButton::Left, 0, 0, none),
            b"\x1b[<0;1;1M".to_vec()
        );
        assert_eq!(
            encode_mouse(sgr, MouseAction::Release, TermMouseButton::Left, 0, 0, none),
            b"\x1b[<0;1;1m".to_vec()
        );
        assert_eq!(
            encode_mouse(sgr, MouseAction::Motion, TermMouseButton::Left, 4, 2, none),
            b"\x1b[<32;5;3M".to_vec()
        );
        assert_eq!(
            encode_mouse(sgr, MouseAction::Motion, TermMouseButton::None, 1, 1, none),
            b"\x1b[<35;2;2M".to_vec()
        );
        assert_eq!(
            encode_mouse(
                sgr,
                MouseAction::Press,
                TermMouseButton::WheelUp,
                10,
                20,
                none
            ),
            b"\x1b[<64;11;21M".to_vec()
        );
        // Shift adds 4, alt 8 and ctrl 16.
        assert_eq!(
            encode_mouse(
                sgr,
                MouseAction::Press,
                TermMouseButton::Right,
                2,
                3,
                modifiers(true, true, true)
            ),
            b"\x1b[<30;3;4M".to_vec()
        );
    }

    #[test]
    fn encodes_legacy_mouse_reports() {
        let none = Modifiers::default();
        let legacy = MouseEncoding::Default;
        assert_eq!(
            encode_mouse(
                legacy,
                MouseAction::Press,
                TermMouseButton::Left,
                0,
                0,
                none
            ),
            vec![0x1b, b'[', b'M', 32, 33, 33]
        );
        // A release is reported as button 3 in the legacy formats.
        assert_eq!(
            encode_mouse(
                legacy,
                MouseAction::Release,
                TermMouseButton::Left,
                5,
                5,
                modifiers(true, false, false)
            ),
            vec![0x1b, b'[', b'M', 32 + 3 + 4, 32 + 6, 32 + 6]
        );
        // The UTF-8 encoding keeps working past the 223 column limit.
        let report = encode_mouse(
            MouseEncoding::Utf8,
            MouseAction::Press,
            TermMouseButton::Left,
            300,
            10,
            none,
        );
        assert_eq!(&report[..3], b"\x1b[M");
        let rest = std::str::from_utf8(&report[3..]).unwrap();
        let decoded: Vec<u32> = rest.chars().map(u32::from).collect();
        assert_eq!(decoded, vec![32, 32 + 301, 32 + 11]);
    }

    #[test]
    fn pastes_without_brackets_use_carriage_returns() {
        assert_eq!(encode_paste("a\nb", false), b"a\rb".to_vec());
        assert_eq!(encode_paste("a\r\nb", false), b"a\rb".to_vec());
    }

    #[test]
    fn bracketed_paste_is_wrapped_and_sanitized() {
        assert_eq!(
            encode_paste("line 1\nline 2", true),
            b"\x1b[200~line 1\nline 2\x1b[201~".to_vec()
        );
        assert_eq!(
            encode_paste("evil\x1b[201~rm -rf /", true),
            b"\x1b[200~evilrm -rf /\x1b[201~".to_vec()
        );
    }

    #[test]
    fn selection_range_is_ordered() {
        let first = TermPos { row: 1, col: 4 };
        let second = TermPos { row: 2, col: 1 };
        assert_eq!(terminal_selection_range(first, second), (first, second));
        assert_eq!(terminal_selection_range(second, first), (first, second));
        assert_eq!(terminal_selection_range(first, first), (first, first));
    }

    #[test]
    fn selection_contains_the_whole_span() {
        let start = TermPos { row: 1, col: 3 };
        let end = TermPos { row: 3, col: 2 };
        assert!(!contains((start, end), 1, 2));
        assert!(contains((start, end), 1, 3));
        assert!(contains((start, end), 2, 0));
        assert!(contains((start, end), 3, 2));
        assert!(!contains((start, end), 3, 3));
        assert!(!contains((start, end), 4, 0));
    }

    #[test]
    fn dimming_blends_towards_the_background() {
        assert_eq!(blend(0xffffff, 0x000000, 1.0), 0x000000);
        assert_eq!(blend(0xffffff, 0x000000, 0.0), 0xffffff);
        assert_eq!(blend(0xffffff, 0x000000, 0.5), 0x808080);
    }
}
