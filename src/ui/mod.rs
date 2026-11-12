//! Drawer layout. Resize grips are hairlines. The editor padding sits inside
//! the character grid's chrome so text is not flush with the frame.

mod band;
mod place;
mod resize;
pub mod settings;
mod tab_bar;

use crate::editor::Tab;
use crate::geom::{self, Edge};
use crate::message::Message;
use iced::highlighter;
use iced::widget::{button, column, container, row, text, text_editor, text_input};
use iced::{Color, Element, Font, Length, Pixels, Theme, alignment};

/// Stable id so show/tab-switch can focus the editor without a click.
pub const EDITOR_ID: iced::widget::Id = iced::widget::Id::new("editor");
/// Tab strip. A vertical wheel over it scrolls this horizontally.
pub const TAB_STRIP: iced::widget::Id = iced::widget::Id::new("tab-strip");
/// Find field. Ctrl+F focuses it.
pub const FIND_ID: iced::widget::Id = iced::widget::Id::new("find");

pub struct Drawer<'a> {
    pub tabs: &'a [Tab],
    pub active: usize,
    pub edge: Edge,
    pub columns: u32,
    pub rows: u32,
    pub height: u32,
    /// Padding around the glyphs, from the saved layout choice.
    pub pad: u32,
    /// Pre-computed gutter width (0 when line numbers are off).
    pub gutter: u32,
    pub text_size: f32,
    pub font: Font,
    pub syntax: highlighter::Theme,
    pub line_numbers: bool,
    pub word_wrap: bool,
    /// Title of a scratch tab waiting for a close confirmation.
    pub confirm_close: Option<&'a str>,
    /// The find bar is open. The query may be empty.
    pub find_open: bool,
    pub find_query: &'a str,
}

pub fn drawer<'a>(drawer: Drawer<'a>) -> Element<'a, Message> {
    let Some(tab) = drawer.tabs.get(drawer.active) else {
        return iced::widget::text("Subtext").into();
    };
    let cell = geom::Cell::for_text_size(drawer.text_size);
    let wrapping = if drawer.word_wrap {
        iced_core::text::Wrapping::Word
    } else {
        iced_core::text::Wrapping::None
    };
    // The renderer clips glyphs to this box. Without it, unwrapped lines paint
    // across the scrollbar. The plate behind this stack is the only fill, so
    // the editor itself stays clear and the caret row can show through.
    // The band paints the caret row across the gutter and the text after the
    // editor has published its layout, so the two cannot land a frame apart.
    let field: Element<'a, Message> = text_editor(&tab.content)
        .id(EDITOR_ID.clone())
        .font(drawer.font)
        .size(drawer.text_size)
        .line_height(iced_core::text::LineHeight::Absolute(Pixels(
            cell.height as f32,
        )))
        .padding(drawer.pad as f32)
        .wrapping(wrapping)
        .height(Length::Fill)
        .style(editor_style)
        .highlight(tab.language.token(), drawer.syntax)
        .key_binding(bind_editor_keys)
        .on_action(Message::Edit)
        .into();
    let editor: Element<'a, Message> = container(field)
        .clip(true)
        .width(Length::Fill)
        .height(Length::Fill)
        .into();
    let find_extra = if drawer.find_open { geom::FIND_BAR } else { 0 };
    let editor_row: Element<'a, Message> = band::band(
        editor,
        tab.snap_key(),
        tab.cursor_line,
        tab.cursor_column,
        drawer.rows,
        tab.line_count() as usize,
        drawer.text_size,
        cell.height as f32,
        drawer.pad as f32,
        if drawer.line_numbers {
            drawer.gutter as f32
        } else {
            0.0
        },
        drawer.edge,
        drawer.height,
        find_extra,
    )
    .into();
    let bar = tab_bar::view(
        drawer.tabs,
        drawer.active,
        drawer.columns,
        drawer.rows,
        drawer.line_numbers,
    );
    let confirm = drawer.confirm_close.map(confirm_close);
    let find = drawer
        .find_open
        .then(|| find_bar(drawer.find_query, find_count(tab, drawer.find_query)));
    let body = match drawer.edge {
        Edge::Bottom => {
            let mut parts = vec![editor_row];
            if let Some(find) = find {
                parts.push(find);
            }
            if let Some(confirm) = confirm {
                parts.push(confirm);
            }
            parts.push(bar);
            column(parts).into()
        }
        Edge::Top | Edge::Left | Edge::Right => {
            let mut parts = vec![bar];
            if let Some(confirm) = confirm {
                parts.push(confirm);
            }
            if let Some(find) = find {
                parts.push(find);
            }
            parts.push(editor_row);
            column(parts).into()
        }
    };
    match drawer.edge {
        Edge::Bottom => column([
            resize::horizontal(-1),
            row([resize::vertical(-1), body, resize::vertical(1)])
                .height(Length::Fill)
                .into(),
        ])
        .into(),
        Edge::Top => column([
            row([resize::vertical(-1), body, resize::vertical(1)])
                .height(Length::Fill)
                .into(),
            resize::horizontal(1),
        ])
        .into(),
        Edge::Left => column([
            resize::horizontal(-1),
            row([body, resize::vertical(1)]).height(Length::Fill).into(),
            resize::horizontal(1),
        ])
        .into(),
        Edge::Right => column([
            resize::horizontal(-1),
            row([resize::vertical(-1), body])
                .height(Length::Fill)
                .into(),
            resize::horizontal(1),
        ])
        .into(),
    }
}

/// Pins the drawer inside the output-sized surface.
///
/// The surface never changes size, so a grab cannot stretch the last frame.
/// `slide` is 0 when the drawer is fully past `edge`, and 1 when it is
/// resting. The plate is not faded: a transparent plate would uncover the
/// frost while the rectangle was still on screen.
pub fn capture_frame<'a>(
    edge: Edge,
    output: (u32, u32),
    drawer: (u32, u32),
    slide: f32,
    opacity: f32,
    body: Element<'a, Message>,
) -> Element<'a, Message> {
    let body = container(body)
        .width(Length::Fixed(drawer.0 as f32))
        .height(Length::Fixed(drawer.1 as f32))
        .clip(true)
        .style(move |theme: &Theme| plate(theme, opacity));
    let (x, y) = geom::slide_origin(edge, output, drawer, slide);
    place::pin(
        iced::Point::new(x as f32, y as f32),
        iced::Size::new(drawer.0 as f32, drawer.1 as f32),
        1.0,
        body,
    )
    .into()
}

fn find_count(tab: &Tab, query: &str) -> String {
    let cursor = tab.content.cursor();
    let caret = crate::editor::find::Span {
        line: cursor.position.line,
        column: cursor.position.column,
    };
    let anchor = cursor.selection.map(|position| crate::editor::find::Span {
        line: position.line,
        column: position.column,
    });
    crate::editor::find::readout(&tab.text(), query, caret, anchor).unwrap_or_default()
}

fn find_bar<'a>(query: &'a str, count: String) -> Element<'a, Message> {
    container(
        row![
            text_input("Find", query)
                .id(FIND_ID.clone())
                .on_input(Message::FindQuery)
                .on_submit(Message::FindSubmit)
                .padding([2, 8])
                .size(14.0)
                .width(Length::Fill),
            text(count).size(14).width(Length::Fixed(56.0)),
            find_button("Previous", Message::FindPrevious),
            find_button("Next", Message::FindNext),
            find_button("Close", Message::FindClose),
        ]
        .spacing(8)
        .align_y(alignment::Vertical::Center),
    )
    .padding([0, 8])
    .height(Length::Fixed(geom::FIND_BAR as f32))
    .align_y(alignment::Vertical::Center)
    .into()
}

fn find_button(label: &'static str, message: Message) -> Element<'static, Message> {
    button(text(label).size(14))
        .padding([2, 8])
        .style(button::text)
        .on_press(message)
        .into()
}

fn confirm_close(title: &str) -> Element<'_, Message> {
    use iced::widget::{button, row};
    container(
        row![
            text(format!("Close {title}? This scratch note will be deleted."))
                .size(14)
                .width(Length::Fill),
            button(text("Delete").size(14))
                .padding([4, 10])
                .style(button::danger)
                .on_press(Message::ConfirmClose),
            button(text("Keep").size(14))
                .padding([4, 10])
                .style(button::text)
                .on_press(Message::CancelClose),
        ]
        .spacing(8)
        .align_y(alignment::Vertical::Center),
    )
    .padding([4, 8])
    .style(|theme: &iced::Theme| {
        let mut color = theme.extended_palette().danger.weak.color;
        color.a = 0.35;
        container::Style {
            background: Some(color.into()),
            ..container::Style::default()
        }
    })
    .into()
}

fn plate(theme: &Theme, opacity: f32) -> container::Style {
    let mut color = theme.palette().background;
    color.a = opacity.clamp(0.0, 1.0);
    container::Style {
        background: Some(color.into()),
        ..container::Style::default()
    }
}

/// Tab inserts the configured indent. Every other key keeps the editor default.
fn bind_editor_keys(press: text_editor::KeyPress) -> Option<text_editor::Binding<Message>> {
    use iced::keyboard::Key;
    use iced::keyboard::key::Named;

    let focused = matches!(press.status, text_editor::Status::Focused { .. });
    if focused && !press.modifiers.command() && !press.modifiers.alt() {
        match press.key.as_ref() {
            Key::Named(Named::Tab) => {
                return Some(text_editor::Binding::Custom(if press.modifiers.shift() {
                    Message::Unindent
                } else {
                    Message::Indent
                }));
            }
            Key::Named(Named::Enter) => {
                return Some(text_editor::Binding::Custom(Message::BreakLine));
            }
            Key::Named(Named::Backspace) => {
                return Some(text_editor::Binding::Custom(Message::Backspace));
            }
            _ => {}
        }
    }
    text_editor::Binding::from_key_press(press)
}

fn editor_style(theme: &Theme, status: text_editor::Status) -> text_editor::Style {
    let mut style = text_editor::default(theme, status);
    style.background = Color::TRANSPARENT.into();
    style.border.width = 0.0;
    style
}

/// The part of a line that sits inside the text box.
///
/// `origin` is the line's top relative to that box. It is negative when the
/// line starts above the box.
pub(crate) fn visible_line(origin: f32, row_height: f32, box_height: f32) -> Option<(f32, f32)> {
    if !origin.is_finite() || !row_height.is_finite() || !box_height.is_finite() {
        return None;
    }
    if row_height <= 0.0 || box_height <= 0.0 {
        return None;
    }
    let top = origin.max(0.0);
    let bottom = (origin + row_height).min(box_height);
    let height = bottom - top;
    (height > 0.0).then_some((top, height))
}

#[cfg(test)]
mod tests {
    use super::visible_line;

    #[test]
    fn a_partial_top_line_keeps_the_visible_slice() {
        assert_eq!(visible_line(-8.0, 29.0, 100.0), Some((0.0, 21.0)));
    }

    #[test]
    fn a_partial_bottom_line_stops_at_the_text_box() {
        assert_eq!(visible_line(90.0, 29.0, 100.0), Some((90.0, 10.0)));
    }

    #[test]
    fn a_line_outside_the_text_box_is_not_drawn() {
        assert_eq!(visible_line(-40.0, 29.0, 100.0), None);
        assert_eq!(visible_line(100.0, 29.0, 100.0), None);
    }
}
