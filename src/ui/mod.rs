//! Drawer layout. Resize grips are hairlines. The editor padding sits inside
//! the character grid's chrome so text is not flush with the frame.

mod place;
mod resize;
mod scroll;
pub mod settings;
mod tab_bar;

use crate::editor::Tab;
use crate::editor::wrap::Slot;
use crate::geom::{self, Edge};
use crate::message::Message;
use iced::highlighter;
use iced::widget::{button, column, container, row, space, stack, text, text_editor, text_input};
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
    let view = tab.viewport(drawer.rows);
    // The renderer clips glyphs to this box. Without it, unwrapped lines paint
    // across the scrollbar. The plate behind this stack is the only fill, so
    // the editor itself stays clear and the caret row can show through.
    // One band sits behind the gutter and the text, so the padding between
    // them does not split the highlight.
    let highlight: Element<'a, Message> =
        line_highlight(&view.slots, cell.height as f32, view.lead);
    let field: Element<'a, Message> = text_editor(&tab.content)
        .id(EDITOR_ID.clone())
        .font(drawer.font)
        .size(drawer.text_size)
        .line_height(iced_core::text::LineHeight::Absolute(Pixels(
            cell.height as f32,
        )))
        .padding(geom::EDITOR_PAD as f32)
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
    let mut band_parts: Vec<Element<'a, Message>> = Vec::with_capacity(2);
    if drawer.line_numbers {
        band_parts.push(gutter(&view.slots, drawer.text_size, cell, view.lead));
    }
    band_parts.push(editor);
    let band: Element<'a, Message> = row(band_parts).height(Length::Fill).into();
    let text_and_gutter: Element<'a, Message> = stack([highlight, band])
        .width(Length::Fill)
        .height(Length::Fill)
        .into();
    let mut editor_parts: Vec<Element<'a, Message>> = Vec::with_capacity(2);
    editor_parts.push(text_and_gutter);
    let find_extra = if drawer.find_open { geom::FIND_BAR } else { 0 };
    editor_parts.push(scroll::bar(
        drawer.edge,
        drawer.height,
        view.total as usize,
        view.scroll,
        drawer.rows,
        find_extra,
    ));
    let editor_row: Element<'a, Message> = row(editor_parts).height(Length::Fill).into();
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

fn line_highlight(slots: &[Slot], row_height: f32, lead: f32) -> Element<'static, Message> {
    let (pad, heights) = row_boxes(slots.len(), row_height, lead);
    let mut rows = Vec::with_capacity(slots.len() + 1);
    rows.push(space().height(Length::Fixed(pad)).into());
    for (slot, height) in slots.iter().zip(heights) {
        rows.push(
            container(space())
                .width(Length::Fill)
                .height(Length::Fixed(height))
                .style(if slot.current { caret_row } else { clear_row })
                .into(),
        );
    }
    container(column(rows).spacing(0).width(Length::Fill))
        .clip(true)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn caret_row(theme: &Theme) -> container::Style {
    let mut color = theme.extended_palette().primary.weak.color;
    color.a = 0.35;
    container::Style {
        background: Some(color.into()),
        ..container::Style::default()
    }
}

fn clear_row(_theme: &Theme) -> container::Style {
    container::Style::default()
}

fn gutter(
    slots: &[Slot],
    text_size: f32,
    cell: geom::Cell,
    lead: f32,
) -> Element<'static, Message> {
    let (pad, heights) = row_boxes(slots.len(), cell.height as f32, lead);
    let numbers: Vec<Element<'static, Message>> = slots
        .iter()
        .zip(heights)
        .map(|(slot, height)| {
            let label = slot
                .number
                .map(|number| number.to_string())
                .unwrap_or_default();
            let current = slot.current;
            container(
                text(label)
                    .size(gutter_text_size(text_size))
                    .font(Font::MONOSPACE)
                    .line_height(iced_core::text::LineHeight::Absolute(Pixels(height)))
                    .align_x(alignment::Horizontal::Right)
                    .align_y(alignment::Vertical::Center)
                    .width(Length::Fill)
                    .style(move |theme: &iced::Theme| iced::widget::text::Style {
                        color: Some(theme.palette().text.scale_alpha(if current {
                            0.9
                        } else {
                            0.4
                        })),
                    }),
            )
            .height(Length::Fixed(height))
            .width(Length::Fill)
            .into()
        })
        .collect();
    container(column(numbers).spacing(0))
        .width(Length::Fixed(geom::gutter_width(cell) as f32))
        .height(Length::Fill)
        .clip(true)
        .padding(iced::Padding {
            top: pad,
            right: 8.0,
            bottom: 0.0,
            left: 8.0,
        })
        .into()
}

/// Top inset and per-row heights for the gutter and the caret band.
///
/// The editor scrolls by pixels and clips to the text box. A row that starts
/// above that box is shorter by the hidden pixels, so the number stays on the
/// glyphs instead of sliding into the padding.
fn row_boxes(count: usize, row_height: f32, lead: f32) -> (f32, Vec<f32>) {
    let lead = if lead.is_finite() { lead } else { 0.0 };
    let hidden = if lead < 0.0 { -lead } else { 0.0 };
    let pad = geom::EDITOR_PAD as f32 + lead.max(0.0);
    let heights = (0..count)
        .map(|index| {
            if index == 0 {
                (row_height - hidden).max(1.0)
            } else {
                row_height
            }
        })
        .collect();
    (pad, heights)
}

/// Line numbers are smaller and dimmer than the buffer text.
fn gutter_text_size(text_size: f32) -> f32 {
    (text_size * 0.7).clamp(10.0, 16.0)
}
