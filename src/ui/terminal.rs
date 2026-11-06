use std::time::Duration;

use gpui::{FontWeight, KeyDownEvent, MouseButton, SharedString};

use super::helpers::*;
use super::*;
use crate::terminal::{TerminalSession, TERMINAL_COLS, TERMINAL_ROWS};

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

impl PodmanDesktop {
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
        let status = if self.terminal_error.is_some() {
            "error"
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
                                .flex_shrink_0()
                                .text_xs()
                                .font_family("monospace")
                                .text_color(rgb(theme().text))
                                .child(self.terminal_label.clone()),
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
        if let Some(metrics) = self.terminal_metrics.borrow().as_ref() {
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
        *self.terminal_metrics.borrow_mut() = Some(metrics.clone());
        metrics
    }

    fn terminal_grid(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(session) = self.terminal.as_ref() else {
            return div().into_any_element();
        };
        let metrics = self.terminal_metrics(cx.text_system());
        let family = metrics.family.clone();
        let screen = session.screen();
        let (rows, cols) = screen.size();
        let (rows, cols) = (rows.get(), cols.get());
        let (cursor_row, cursor_col) = screen.cursor_position();
        // The block cursor belongs to the live viewport.
        let show_cursor = !screen.hide_cursor() && !session.closed && screen.scrollback() == 0;

        let mut lines = div().flex().flex_col().flex_1().min_h(px(0.)).w_full();

        for row in 0..rows {
            let mut line = div().flex().items_center().h(px(LINE_HEIGHT)).w_full();
            let mut text = String::new();
            let mut style: Option<(u32, u32, bool)> = None;

            for col in 0..cols {
                let Some(cell) = screen.cell(row, col) else {
                    continue;
                };
                if cell.is_wide_continuation() {
                    continue;
                }

                let mut fg = terminal_color(cell.fgcolor(), theme().text);
                let mut bg = terminal_color(cell.bgcolor(), theme().bg);
                if cell.inverse() {
                    std::mem::swap(&mut fg, &mut bg);
                }
                if show_cursor && row == cursor_row && col == cursor_col {
                    std::mem::swap(&mut fg, &mut bg);
                }

                let cell_style = (fg, bg, cell.bold());
                if style != Some(cell_style) {
                    if let Some(previous) = style {
                        line = line.child(terminal_run(&text, previous, &family));
                    }
                    text.clear();
                    style = Some(cell_style);
                }
                let contents = cell.contents();
                if show_cursor && row == cursor_row && col == cursor_col {
                    // A plain space collapses to zero width, so the reversed
                    // background would never paint; use a non-breaking space.
                    if contents.trim().is_empty() {
                        text.push('\u{00A0}');
                    } else {
                        text.push_str(contents);
                    }
                } else {
                    text.push_str(contents);
                }
            }

            if let Some(style) = style {
                line = line.child(terminal_run(&text, style, &family));
            }
            lines = lines.child(line);
        }

        // The grid reports its own size here so the session can resize the
        // emulator and the pty to match the pane. The width comes from the
        // font's real advance: a guess would leave the last columns unclaimed.
        let hint = self.terminal_size.clone();
        let char_width = metrics.char_width;
        div()
            .on_children_prepainted(
                move |bounds: Vec<Bounds<Pixels>>, _window: &mut Window, _app: &mut App| {
                    let Some(bounds) = bounds.first() else {
                        return;
                    };
                    // Half a pixel of slack: `7.2 * 100` is `720.00003` in f32,
                    // which would otherwise cost a whole column.
                    let cols = ((f32::from(bounds.size.width) + 0.5) / char_width).floor();
                    let rows = (f32::from(bounds.size.height) / LINE_HEIGHT).floor();
                    if cols >= 20.0 && rows >= 2.0 {
                        hint.set((rows.min(400.0) as u16, cols.min(600.0) as u16));
                    }
                },
            )
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
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.terminal_focus.focus(window);
                    cx.notify();
                }),
            )
            .flex()
            .flex_col()
            .child(lines)
            .into_any_element()
    }

    /// Mouse wheel scrolls through the emulator's scrollback.
    fn terminal_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let lines = match event.delta {
            ScrollDelta::Lines(point) => point.y,
            ScrollDelta::Pixels(point) => f32::from(point.y) / LINE_HEIGHT,
        };
        if lines.abs() < 0.01 {
            return;
        }
        // One wheel notch moves three rows, like most terminal emulators.
        let delta = (lines * 3.0).round() as isize;
        if let Some(session) = self.terminal.as_mut() {
            if session.scroll_by(delta) {
                cx.notify();
            }
        }
    }

    fn terminal_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        let keystroke = &event.keystroke;
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
            if let Some(session) = &self.terminal {
                if !session.closed {
                    session.write(&bytes);
                }
            }
        }
        cx.notify();
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

fn terminal_run(text: &str, style: (u32, u32, bool), family: &SharedString) -> Div {
    let (fg, bg, bold) = style;
    div()
        .flex_shrink_0()
        .whitespace_nowrap()
        .text_size(px(FONT_SIZE))
        .font_family(family.clone())
        .text_color(rgb(fg))
        .when(bg != theme().bg, |el| el.bg(rgb(bg)))
        .when(bold, |el| el.font_weight(FontWeight::BOLD))
        .child(text.to_string())
}

fn terminal_color(color: vt100::Color, default: u32) -> u32 {
    match color {
        vt100::Color::Default => default,
        vt100::Color::Idx(index) => ansi_color(index),
        vt100::Color::Rgb(red, green, blue) => {
            ((red as u32) << 16) | ((green as u32) << 8) | blue as u32
        }
    }
}

fn ansi_color(index: u8) -> u32 {
    const BASIC: [u32; 16] = [
        0x1e1e1e, 0xcd3131, 0x0dbc79, 0xe5e510, 0x2472c8, 0xbc3fbc, 0x11a8cd, 0xe5e5e5, 0x666666,
        0xf14c4c, 0x23d18b, 0xf5f543, 0x3b8eea, 0xd670d6, 0x29b8db, 0xffffff,
    ];
    match index {
        0..=15 => BASIC[index as usize],
        16..=231 => {
            const STEPS: [u32; 6] = [0, 95, 135, 175, 215, 255];
            let index = index - 16;
            let red = STEPS[(index / 36) as usize];
            let green = STEPS[((index % 36) / 6) as usize];
            let blue = STEPS[(index % 6) as usize];
            (red << 16) | (green << 8) | blue
        }
        _ => {
            let level = 8 + 10 * (index - 232) as u32;
            (level << 16) | (level << 8) | level
        }
    }
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
}
