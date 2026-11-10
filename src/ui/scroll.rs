//! Vertical scrollbar for the text editor.
//!
//! The editor already consumes wheel events, so this track only draws the
//! thumb and reports a press. Dragging uses the window cursor.

use crate::geom::{self, Edge};
use crate::message::Message;
use iced::widget::{column, container, mouse_area, space};
use iced::{Border, Element, Length, Theme};

pub fn bar<'a>(
    edge: Edge,
    window_height: u32,
    line_count: usize,
    scroll_lines: i32,
    rows: u32,
    extra: u32,
) -> Element<'a, Message> {
    let (_top, track_h) = geom::editor_track(edge, window_height, extra);
    let (thumb_h, offset) = geom::thumb_span(track_h, rows, line_count.max(1) as u32, scroll_lines);
    mouse_area(
        container(
            column![
                space().height(Length::Fixed(offset.max(0.0))),
                container(space())
                    .height(Length::Fixed(thumb_h.max(1.0)))
                    .width(Length::Fill)
                    .style(thumb_style),
            ]
            .spacing(0),
        )
        .width(Length::Fixed(geom::SCROLLBAR as f32))
        .height(Length::Fill)
        .style(track_style),
    )
    .on_press(Message::ScrollGrab)
    .into()
}

fn track_style(theme: &Theme) -> container::Style {
    let mut color = theme.extended_palette().background.strong.color;
    color.a = 0.45;
    container::Style {
        background: Some(color.into()),
        ..container::Style::default()
    }
}

fn thumb_style(theme: &Theme) -> container::Style {
    let mut color = theme.extended_palette().primary.strong.color;
    color.a = 0.75;
    container::Style {
        background: Some(color.into()),
        border: Border {
            color,
            width: 0.0,
            radius: 2.0.into(),
        },
        ..container::Style::default()
    }
}
