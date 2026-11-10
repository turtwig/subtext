//! Settings live on their own overlay surface, above the drawer, so the
//! editor stays visible while a choice changes.

use crate::editor::{Language, TabStop};
use crate::geom::Edge;
use crate::look::{FontChoice, TextUnit, ThemeChoice};
use crate::message::Message;
use iced::widget::{button, column, container, pick_list, row, slider, text};
use iced::{Border, Element, Length, Theme, alignment};

pub struct Model {
    pub theme: ThemeChoice,
    pub font: FontChoice,
    pub text_size: f32,
    pub text_unit: TextUnit,
    pub word_wrap: bool,
    pub tab_stop: TabStop,
    pub remember_indent: bool,
    pub hinting: bool,
    pub click_away: bool,
    pub opacity: f32,
    pub blur: bool,
    pub edge: Edge,
    pub language: Language,
}

pub fn view(model: Model) -> Element<'static, Message> {
    let theme = pick_list(ThemeChoice::ALL, Some(model.theme), Message::Theme)
        .width(Length::Fill)
        .menu_height(Length::Fixed(260.0))
        .text_size(14.0);
    let font = pick_list(FontChoice::installed(), Some(model.font), Message::Font)
        .width(Length::Fill)
        .menu_height(Length::Fixed(260.0))
        .text_size(14.0);
    let syntax = pick_list(Language::ALL, Some(model.language), Message::Language)
        .width(Length::Fill)
        .menu_height(Length::Fixed(220.0))
        .text_size(14.0);
    let tabs = pick_list(TabStop::ALL, Some(model.tab_stop), Message::TabStop)
        .width(Length::Fill)
        .text_size(14.0);
    let edges = row([
        edge_button(Edge::Bottom, model.edge),
        edge_button(Edge::Top, model.edge),
        edge_button(Edge::Left, model.edge),
        edge_button(Edge::Right, model.edge),
    ])
    .spacing(6);

    container(
        column![
            row![
                text("Settings").size(18),
                iced::widget::space::horizontal(),
                text_button("Close", Message::ToggleSettings),
            ]
            .align_y(alignment::Vertical::Center),
            labeled("Theme", theme),
            labeled("Font", font),
            labeled(
                "Hint",
                toggle_button("Hinting", model.hinting, Message::ToggleHinting),
            ),
            labeled("Syntax", syntax),
            labeled(
                "Size",
                row![
                    text_button("A−", Message::FontSmaller),
                    text(format!("{:.0}", model.text_size)).size(14),
                    unit_button(TextUnit::Pt, model.text_unit),
                    unit_button(TextUnit::Px, model.text_unit),
                    text_button("A+", Message::FontLarger),
                ]
                .spacing(8)
                .align_y(alignment::Vertical::Center),
            ),
            labeled(
                "Wrap",
                toggle_button("Word wrap", model.word_wrap, Message::ToggleWrap),
            ),
            labeled("Tabs", tabs),
            labeled(
                "Enter",
                toggle_button(
                    "Remember indent",
                    model.remember_indent,
                    Message::ToggleRememberIndent,
                ),
            ),
            labeled(
                "Opacity",
                row![
                    slider(0.2..=1.0, model.opacity, Message::Opacity)
                        .step(0.05_f32)
                        .width(Length::Fill),
                    text(format!("{:.0}%", model.opacity * 100.0))
                        .size(14)
                        .width(Length::Fixed(40.0)),
                ]
                .spacing(8)
                .align_y(alignment::Vertical::Center),
            ),
            labeled(
                "Blur",
                toggle_button("Background", model.blur, Message::ToggleBlur),
            ),
            labeled("Edge", edges),
            labeled(
                "Close",
                toggle_button("Automatically", model.click_away, Message::ToggleClickAway),
            ),
        ]
        .spacing(12)
        .padding(16),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(move |theme: &Theme| {
        let mut color = theme.palette().background;
        color.a = model.opacity.clamp(0.0, 1.0);
        container::Style {
            background: Some(color.into()),
            border: Border {
                color: theme.extended_palette().background.strong.color,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..container::Style::default()
        }
    })
    .into()
}

fn labeled<'a>(
    label: &'static str,
    control: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    row![
        text(label).size(14).width(Length::Fixed(72.0)),
        control.into(),
    ]
    .spacing(8)
    .align_y(alignment::Vertical::Center)
    .into()
}

fn edge_button(edge: Edge, current: Edge) -> Element<'static, Message> {
    let style = if edge == current {
        button::primary
    } else {
        button::text
    };
    button(text(edge.to_string()).size(14))
        .padding([4, 8])
        .width(Length::Fill)
        .style(style)
        .on_press(Message::SetEdge(edge))
        .into()
}

fn toggle_button(label: &'static str, on: bool, message: Message) -> Element<'static, Message> {
    let style = if on { button::primary } else { button::text };
    button(text(label).size(14))
        .padding([4, 10])
        .style(style)
        .on_press(message)
        .into()
}

fn unit_button(unit: TextUnit, current: TextUnit) -> Element<'static, Message> {
    let style = if unit == current {
        button::primary
    } else {
        button::text
    };
    button(text(unit.label()).size(14))
        .padding([4, 8])
        .style(style)
        .on_press(Message::SetTextUnit(unit))
        .into()
}

fn text_button(label: &'static str, message: Message) -> Element<'static, Message> {
    button(text(label).size(14))
        .padding([4, 8])
        .style(button::text)
        .on_press(message)
        .into()
}
