//! Grips on the free edges. The hit target is a few pixels, drawn as a hairline
//! in the window background so it does not read as a thick frame.
//!
//! A double-click on a side grip asks the drawer to toggle full screen width.
//! That only applies while the anchor is the top or bottom edge.

use crate::geom::HANDLE;
use crate::message::{Grip, Message};
use iced::widget::{container, mouse_area, space};
use iced::{Border, Element, Length, Theme};

pub fn horizontal<'a>(rows_sign: i32) -> Element<'a, Message> {
    grip(
        Length::Fill,
        Length::Fixed(HANDLE as f32),
        Grip {
            columns: 0,
            rows: rows_sign,
        },
        iced::mouse::Interaction::ResizingVertically,
    )
}

pub fn vertical<'a>(columns_sign: i32) -> Element<'a, Message> {
    grip(
        Length::Fixed(HANDLE as f32),
        Length::Fill,
        Grip {
            columns: columns_sign,
            rows: 0,
        },
        iced::mouse::Interaction::ResizingHorizontally,
    )
}

fn grip<'a>(
    width: Length,
    height: Length,
    grip: Grip,
    interaction: iced::mouse::Interaction,
) -> Element<'a, Message> {
    let area = mouse_area(container(space()).width(width).height(height).style(style))
        .on_press(Message::Grip(grip))
        .interaction(interaction);
    // Side grips (a column sign) toggle full width on double-click. The press
    // that starts the click also arms a drag; the handler cancels that drag.
    if grip.columns != 0 {
        area.on_double_click(Message::ToggleWidth).into()
    } else {
        area.into()
    }
}

fn style(theme: &Theme) -> container::Style {
    let line = theme.extended_palette().background.strong.color;
    container::Style {
        background: Some(theme.palette().background.into()),
        border: Border {
            color: line,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}
