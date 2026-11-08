use super::theme::theme;
use super::*;

pub(super) fn dot(color: u32) -> Div {
    div()
        .flex_shrink_0()
        .w(px(8.))
        .h(px(8.))
        .rounded_full()
        .bg(rgb(color))
}

/// A text caret. `visible` is toggled twice a second by the desktop tick loop
/// (`blink_caret`), so the element itself stays static: animating it with
/// `with_animation` keeps redrawing the whole window at display rate.
pub(super) fn caret(visible: bool) -> Div {
    div()
        .flex_shrink_0()
        .w(px(2.))
        .h(px(14.))
        .rounded_sm()
        .when(visible, |el| el.bg(rgb(theme().accent)))
}

/// Approximate character advance for the 14 px UI font.
const PX_PER_CHAR_SM: f32 = 8.0;
/// Approximate character advance for the 12 px secondary font.
const PX_PER_CHAR_XS: f32 = 6.9;

/// Shorten text to roughly fit `width`, appending an ellipsis when it does not.
/// GPUI 0.2's `truncate()` does not emit an ellipsis for these cells, so the
/// budget is computed here instead.
pub(super) fn truncate_text(text: &str, width: f32, px_per_char: f32) -> String {
    let budget = (width / px_per_char).floor().max(3.0) as usize;
    if text.chars().count() <= budget {
        return text.to_string();
    }
    let mut shortened: String = text.chars().take(budget - 1).collect();
    shortened.push('…');
    shortened
}

pub(super) fn truncate_sm(text: &str, width: f32) -> String {
    truncate_text(text, width, PX_PER_CHAR_SM)
}

pub(super) fn truncate_xs(text: &str, width: f32) -> String {
    truncate_text(text, width, PX_PER_CHAR_XS)
}

pub(super) fn cell(text: impl Into<SharedString>, width: f32) -> Div {
    let text: SharedString = text.into();
    div()
        .w(px(width))
        .text_sm()
        .whitespace_nowrap()
        .truncate()
        .child(truncate_sm(&text, width))
}

pub(super) fn muted_cell(text: impl Into<SharedString>, width: f32) -> Div {
    cell(text, width).text_color(rgb(theme().muted))
}

pub(super) fn section_title(text: &str) -> Div {
    div()
        .mt_1()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(theme().faint))
        .child(text.to_string())
}

pub(super) fn muted_text(text: impl Into<SharedString>) -> Div {
    div()
        .text_sm()
        .text_color(rgb(theme().muted))
        .child(text.into())
}

pub(super) fn mono_text(text: impl Into<SharedString>) -> Div {
    div()
        .text_xs()
        .font_family("monospace")
        .text_color(rgb(theme().text))
        .child(text.into())
}

pub(super) fn detail_row(label: &str, value: &str) -> Div {
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .text_xs()
                .text_color(rgb(theme().faint))
                .child(label.to_string()),
        )
        .child(
            div()
                .text_sm()
                .text_color(rgb(theme().text))
                .child(value.to_string()),
        )
}

pub(super) fn state_colors(state: &str) -> (u32, u32) {
    let t = theme();
    match state {
        "running" => (t.green, t.green_bg),
        "paused" | "restarting" => (t.yellow, t.yellow_bg),
        "exited" | "stopped" | "dead" => (t.red, t.red_bg),
        _ => (t.blue, t.blue_bg),
    }
}

pub(super) fn state_pill(label: &str, color: u32, background: u32) -> Div {
    div()
        .flex()
        .items_center()
        .gap_1()
        .px_2()
        .py(px(2.))
        .rounded_full()
        .bg(rgb(background))
        .child(dot(color))
        .child(
            div()
                .text_xs()
                .text_color(rgb(color))
                .child(label.to_string()),
        )
}

pub(super) fn pill_button(
    id: &'static str,
    label: &'static str,
    enabled: bool,
    fg: u32,
    bg: u32,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    pill_button_dyn(id, label, enabled, fg, bg, on_click)
}

/// Same as [`pill_button`] but with a label built at runtime (e.g. a count).
pub(super) fn pill_button_dyn(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    enabled: bool,
    fg: u32,
    bg: u32,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let label = label.into();
    div()
        .id(id.into())
        .px_3()
        .py_2()
        .rounded_md()
        .when(enabled, |el| {
            el.bg(rgb(bg))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(theme().hover_bg)))
                .on_click(on_click)
        })
        .child(
            div()
                .text_sm()
                .text_color(rgb(if enabled { fg } else { theme().faint }))
                .child(label),
        )
}

/// A checkbox shown at the start of a row while bulk selection is active.
pub(super) fn bulk_checkbox(checked: bool) -> Div {
    div()
        .flex_shrink_0()
        .w(px(16.))
        .h(px(16.))
        .mr_2()
        .rounded_sm()
        .border_1()
        .border_color(rgb(if checked {
            theme().accent
        } else {
            theme().border
        }))
        .when(checked, |el| el.bg(rgb(theme().accent_soft)))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .text_xs()
                .text_color(rgb(theme().accent))
                .child(if checked { "✓" } else { "" }),
        )
}

pub(super) fn small_button(
    id: &'static str,
    label: impl Into<SharedString>,
    active: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    small_button_dyn(id, label, active, on_click)
}

/// Same as [`small_button`] but with an id built at runtime (per-service chips).
pub(super) fn small_button_dyn(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    active: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let label = label.into();
    div()
        .id(id.into())
        .px_2()
        .py_1()
        .rounded_sm()
        .cursor_pointer()
        .when(active, |el| el.bg(rgb(theme().accent_soft)))
        .when(!active, |el| {
            el.hover(|style| style.bg(rgb(theme().hover_bg)))
        })
        .on_click(on_click)
        .child(
            div()
                .text_xs()
                .text_color(if active {
                    rgb(theme().accent)
                } else {
                    rgb(theme().muted)
                })
                .child(label),
        )
}

pub(super) fn row_action_button(
    id: SharedString,
    label: &'static str,
    enabled: bool,
    fg: u32,
    bg: u32,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded_sm()
        .when(enabled, |el| {
            el.bg(rgb(bg))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(theme().hover_bg)))
                .on_click(on_click)
        })
        .child(
            div()
                .text_xs()
                .text_color(rgb(if enabled { fg } else { theme().faint }))
                .child(label),
        )
}

pub(super) fn menu_item(
    id: &'static str,
    label: &'static str,
    enabled: bool,
    danger: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let fg = if !enabled {
        theme().faint
    } else if danger {
        theme().red
    } else {
        theme().text
    };

    div()
        .id(id)
        .w_full()
        .px_2()
        .py_1()
        .rounded_sm()
        .when(enabled, |el| {
            el.cursor_pointer()
                .hover(|style| style.bg(rgb(theme().hover_bg)))
                .on_click(on_click)
        })
        .child(div().text_sm().text_color(rgb(fg)).child(label))
}

pub(super) fn next_position(current: Option<usize>, len: usize, delta: isize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let max = len as isize - 1;
    Some(match current {
        Some(position) => (position as isize + delta).clamp(0, max) as usize,
        None if delta >= 0 => 0,
        None => max as usize,
    })
}

pub(super) fn ports_summary(ports: &str) -> String {
    let mut parts = ports.split(", ");
    let Some(first) = parts.next() else {
        return "-".to_string();
    };
    let extra = parts.count();
    if extra == 0 {
        first.to_string()
    } else {
        format!("{first} +{extra}")
    }
}

pub(super) fn matches_query(query: &str, fields: &[&String]) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    fields
        .iter()
        .any(|value| value.to_lowercase().contains(&query))
}

pub(super) fn or_dash(value: &str) -> &str {
    if value.is_empty() {
        "-"
    } else {
        value
    }
}

pub(super) fn bool_label(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

pub(super) fn join_or_dash(values: &[String]) -> String {
    if values.is_empty() {
        "-".to_string()
    } else {
        values.join(", ")
    }
}

pub(super) fn strip_ansi(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            match chars.peek() {
                // CSI: parameters up to a final byte in `@`..=`~`.
                Some('[') => {
                    chars.next();
                    for next in chars.by_ref() {
                        if ('@'..='~').contains(&next) {
                            break;
                        }
                    }
                }
                // OSC (titles, hyperlinks): ends with BEL or ST (`ESC \`).
                Some(']') => {
                    chars.next();
                    while let Some(next) = chars.next() {
                        if next == '\u{7}' {
                            break;
                        }
                        if next == '\u{1b}' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            }
        } else {
            output.push(ch);
        }
    }
    output
}

/// Longest line forwarded in one piece. A stream without newlines (binary
/// output, a progress bar redrawn with `\r`) is split at this length instead
/// of growing one buffer without bound.
pub(super) const MAX_LINE_BYTES: usize = 64 * 1024;

pub(super) fn spawn_line_reader<R: Read + Send + 'static>(reader: R, sender: mpsc::Sender<String>) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        while let Some(line) = read_capped_line(&mut reader, MAX_LINE_BYTES) {
            if sender.send(line).is_err() {
                break;
            }
        }
    });
}

/// Read one line of at most `max` bytes, without the line ending. `None` at
/// EOF or on a read error.
pub(super) fn read_capped_line(reader: &mut impl BufRead, max: usize) -> Option<String> {
    let mut buffer = Vec::new();
    match reader.take(max as u64).read_until(b'\n', &mut buffer) {
        Ok(0) | Err(_) => None,
        Ok(_) => {
            let line = String::from_utf8_lossy(&buffer);
            Some(line.trim_end_matches(['\n', '\r']).to_string())
        }
    }
}

pub(super) fn edit_text(text: &mut String, event: &KeyDownEvent, cx: &mut App) -> TextOutcome {
    let keystroke = &event.keystroke;

    if (keystroke.modifiers.platform || keystroke.modifiers.control) && keystroke.key == "v" {
        if let Some(pasted) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let cleaned: String = pasted.chars().filter(|ch| !ch.is_control()).collect();
            text.push_str(&cleaned);
            return TextOutcome::Edited;
        }
        return TextOutcome::Ignored;
    }

    if keystroke.modifiers.control || keystroke.modifiers.platform || keystroke.modifiers.alt {
        return TextOutcome::Ignored;
    }

    match keystroke.key.as_str() {
        "backspace" => {
            text.pop();
            TextOutcome::Edited
        }
        "space" => {
            text.push(' ');
            TextOutcome::Edited
        }
        "escape" => TextOutcome::Cancel,
        "enter" | "return" => TextOutcome::Submit,
        _ => match &keystroke.key_char {
            Some(ch) if !ch.is_empty() => {
                text.push_str(ch);
                TextOutcome::Edited
            }
            _ => TextOutcome::Ignored,
        },
    }
}
