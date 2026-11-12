//! Grips on the free edges.
//!
//! The painted bar is the border thickness and runs the whole edge, so a
//! vertical bar meets a horizontal one in the corner. The content is inset by
//! that same thickness. The hit target is at least [`GRAB`] and overlaps the
//! content when the border is thinner, so a hidden border can still be dragged.
//!
//! A double-click on a side grip asks the drawer to toggle full screen width.
//! That only applies while the anchor is the top or bottom edge.

use crate::geom::{self, Edge, GRAB};
use crate::message::{Grip, Message};
use iced::widget::{container, mouse_area, space, stack};
use iced::{Element, Length, Padding, Theme, alignment};

/// Content inset by the painted border, with the drag strips laid over it.
pub fn frame<'a>(edge: Edge, border: u32, body: Element<'a, Message>) -> Element<'a, Message> {
    let border = geom::clamp_border(border);
    let hit = geom::grip_span(border) as f32;
    let visual = border as f32;
    let content = container(body)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(insets(edge, visual));
    let mut layers = vec![content.into()];
    // Side strips go under the horizontal ones. A corner then resizes the row,
    // which is where that corner used to live in the frame.
    for (sign, align_x) in sides(edge) {
        layers.push(strip(
            false,
            align_x,
            alignment::Vertical::Top,
            sign,
            hit,
            visual,
        ));
    }
    for (sign, align_y) in ends(edge) {
        layers.push(strip(
            true,
            alignment::Horizontal::Left,
            align_y,
            sign,
            hit,
            visual,
        ));
    }
    stack(layers)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn insets(edge: Edge, px: f32) -> Padding {
    let px = px.max(0.0);
    match edge {
        Edge::Bottom => Padding {
            top: px,
            right: px,
            bottom: 0.0,
            left: px,
        },
        Edge::Top => Padding {
            top: 0.0,
            right: px,
            bottom: px,
            left: px,
        },
        Edge::Left => Padding {
            top: px,
            right: px,
            bottom: px,
            left: 0.0,
        },
        Edge::Right => Padding {
            top: px,
            right: 0.0,
            bottom: px,
            left: px,
        },
    }
}

/// Free side edges, as a column sign and the outer alignment.
fn sides(edge: Edge) -> Vec<(i32, alignment::Horizontal)> {
    match edge {
        Edge::Right => vec![(-1, alignment::Horizontal::Left)],
        Edge::Left => vec![(1, alignment::Horizontal::Right)],
        Edge::Top | Edge::Bottom => vec![
            (-1, alignment::Horizontal::Left),
            (1, alignment::Horizontal::Right),
        ],
    }
}

/// Free top or bottom edges, as a row sign and the outer alignment.
fn ends(edge: Edge) -> Vec<(i32, alignment::Vertical)> {
    match edge {
        Edge::Bottom => vec![(-1, alignment::Vertical::Top)],
        Edge::Top => vec![(1, alignment::Vertical::Bottom)],
        Edge::Left | Edge::Right => vec![
            (-1, alignment::Vertical::Top),
            (1, alignment::Vertical::Bottom),
        ],
    }
}

fn strip<'a>(
    horizontal: bool,
    align_x: alignment::Horizontal,
    align_y: alignment::Vertical,
    sign: i32,
    hit: f32,
    visual: f32,
) -> Element<'a, Message> {
    let (bar_w, bar_h, strip_w, strip_h, grip, interaction) = if horizontal {
        (
            Length::Fill,
            Length::Fixed(visual),
            Length::Fill,
            Length::Fixed(hit.max(GRAB as f32)),
            Grip {
                columns: 0,
                rows: sign,
            },
            iced::mouse::Interaction::ResizingVertically,
        )
    } else {
        (
            Length::Fixed(visual),
            Length::Fill,
            Length::Fixed(hit.max(GRAB as f32)),
            Length::Fill,
            Grip {
                columns: sign,
                rows: 0,
            },
            iced::mouse::Interaction::ResizingHorizontally,
        )
    };
    let painted: Element<'a, Message> = if visual > 0.0 {
        container(space())
            .width(bar_w)
            .height(bar_h)
            .style(solid)
            .into()
    } else {
        space().width(bar_w).height(bar_h).into()
    };
    let target = container(painted)
        .width(strip_w)
        .height(strip_h)
        .align_x(align_x)
        .align_y(align_y);
    let area = mouse_area(target)
        .on_press(Message::Grip(grip))
        .interaction(interaction);
    let area = if grip.columns != 0 {
        area.on_double_click(Message::ToggleWidth)
    } else {
        area
    };
    container(area)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(align_x)
        .align_y(align_y)
        .into()
}

fn solid(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(theme.extended_palette().background.strong.color.into()),
        ..container::Style::default()
    }
}
