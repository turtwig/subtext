//! Settings live on their own overlay surface, above the drawer, so the
//! editor stays visible while a choice changes. The control sits on the
//! title's line. A second line is only there when the control still does
//! not say what it changes.

use crate::editor::{Language, TabStop};
use crate::geom::Edge;
use crate::look::{FontChoice, TextUnit, ThemeChoice};
use crate::message::Message;
use crate::output::{self, Screen};
use iced::widget::{button, column, container, pick_list, row, slider, text};
use iced::{Border, Element, Length, Theme, alignment};

/// Room for the longest title, "Compact layout", on one line.
const LABEL: f32 = 136.0;

pub struct Model {
    pub theme: ThemeChoice,
    pub font: FontChoice,
    pub text_size: f32,
    pub text_unit: TextUnit,
    pub word_wrap: bool,
    pub compact: bool,
    pub tab_stop: TabStop,
    pub remember_indent: bool,
    pub hinting: bool,
    pub click_away: bool,
    pub opacity: f32,
    pub blur: bool,
    pub edge: Edge,
    pub language: Language,
    pub follow_cursor: bool,
    pub screens: Vec<Screen>,
    /// Pinned output. `None` while follow-cursor is on.
    pub selected_output: Option<String>,
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
    let displays = output::choices(&model.screens);
    let selected = model
        .selected_output
        .as_ref()
        .and_then(|name| displays.iter().find(|choice| choice.name == *name).cloned());
    let display = pick_list(displays, selected, |choice| Message::ShowOn(choice.name))
        .placeholder(if model.follow_cursor {
            "Follows the cursor"
        } else {
            "Primary"
        })
        .width(Length::Fill)
        .menu_height(Length::Fixed(220.0))
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
            explained(
                "Hinting",
                toggle_button("Glyph hinting", model.hinting, Message::ToggleHinting),
                "Snaps glyphs to the pixel grid.",
            ),
            labeled("Highlighting", syntax),
            labeled(
                "Text size",
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
                "Wrapping",
                toggle_button("Wrap lines", model.word_wrap, Message::ToggleWrap),
            ),
            explained(
                "Compact layout",
                toggle_button("Reduce margins", model.compact, Message::ToggleCompact),
                "Less padding and a narrower gutter.",
            ),
            labeled("Tab inserts", tabs),
            labeled(
                "Indent on Enter",
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
                toggle_button("Blur background", model.blur, Message::ToggleBlur),
            ),
            labeled("Screen edge", edges),
            labeled(
                "Follow cursor",
                toggle_button(
                    "Open on Active Display",
                    model.follow_cursor,
                    Message::ToggleFollowCursor
                ),
            ),
            explained("Show on", display, "Picking a display turns follow off.",),
            labeled(
                "Click outside",
                toggle_button(
                    "Automatically dismiss Subtext",
                    model.click_away,
                    Message::ToggleClickAway
                ),
            ),
        ]
        .spacing(14)
        .padding(14),
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
    title: &'static str,
    control: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    row![
        text(title).size(14).width(Length::Fixed(LABEL)),
        control.into(),
    ]
    .spacing(8)
    .align_y(alignment::Vertical::Center)
    .into()
}

/// Title and control on one line, then a single explanation under the control.
fn explained<'a>(
    title: &'static str,
    control: impl Into<Element<'a, Message>>,
    help: &'static str,
) -> Element<'a, Message> {
    column![
        labeled(title, control),
        row![
            iced::widget::space::horizontal().width(Length::Fixed(LABEL + 8.0)),
            text(help)
                .size(12)
                .width(Length::Fill)
                .wrapping(text::Wrapping::None)
                .style(note_style),
        ]
        .align_y(alignment::Vertical::Center),
    ]
    .spacing(1)
    .into()
}

fn note_style(theme: &Theme) -> text::Style {
    let mut color = theme.extended_palette().background.base.text;
    color.a = 0.72;
    text::Style { color: Some(color) }
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
