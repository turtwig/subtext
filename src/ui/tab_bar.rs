//! Tabs and the status readout share one row on the screen-edge side.
//! Theme, font, and the other choices live in the settings window.

use crate::editor::Tab;
use crate::geom::BAR;
use crate::message::Message;
use crate::ui::TAB_STRIP;

use iced::widget::{button, container, row, scrollable, text};
use iced::{Border, Element, Length, Theme, alignment};

pub fn view<'a>(
    tabs: &'a [Tab],
    active: usize,
    columns: u32,
    rows: u32,
    line_numbers: bool,
) -> Element<'a, Message> {
    let mut items: Vec<Element<'a, Message>> = Vec::with_capacity(tabs.len() + 4);
    for (index, tab) in tabs.iter().enumerate() {
        items.push(tab_chip(&tab.title, index, index == active));
    }
    items.push(text_button("+", Message::NewTab));
    items.push(text_button("Open", Message::Open));
    items.push(text_button("Save", Message::Save));
    items.push(text_button("Save as", Message::SaveAs));

    let strip = scrollable(
        row(items)
            .spacing(4)
            .align_y(alignment::Vertical::Center)
            .height(Length::Fill),
    )
    .id(TAB_STRIP.clone())
    .direction(scrollable::Direction::Horizontal(thin_scrollbar()))
    .width(Length::Fill)
    .height(Length::Fill)
    .style(thin_style);

    row![
        strip,
        text(format!(
            "{}:{}",
            tabs.get(active)
                .map(|tab| tab.cursor_line.saturating_add(1))
                .unwrap_or(1),
            tabs.get(active)
                .map(|tab| tab.cursor_column.saturating_add(1))
                .unwrap_or(1),
        ))
        .size(14),
        text(format!("{columns}×{rows}")).size(14),
        line_button(line_numbers),
        text_button("Settings", Message::ToggleSettings),
    ]
    .spacing(8)
    .padding([0, 8])
    .align_y(alignment::Vertical::Center)
    .height(Length::Fixed(BAR as f32))
    .into()
}

/// A hairline under the tabs. Vertical wheel is mapped to this axis by the app.
fn thin_scrollbar() -> scrollable::Scrollbar {
    scrollable::Scrollbar::new()
        .width(3)
        .scroller_width(3)
        .margin(0)
}

fn thin_style(theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let mut style = scrollable::default(theme, status);
    // The strip sits beside the status labels. A rail background here is what
    // flashes in the gap when the bar shows and hides.
    style.container.background = None;
    style.horizontal_rail.background = None;
    style.horizontal_rail.border.width = 0.0;
    style.horizontal_rail.scroller.border.width = 0.0;
    style.horizontal_rail.border.radius = 0.0.into();
    style.horizontal_rail.scroller.border.radius = 0.0.into();
    style
}

/// Title and close control share one chip. The close control sits inside it.
fn tab_chip(title: &str, index: usize, active: bool) -> Element<'_, Message> {
    let chip = move |theme: &Theme| {
        let pair = if active {
            theme.extended_palette().primary.base
        } else {
            theme.extended_palette().background.strong
        };
        container::Style {
            text_color: Some(pair.text),
            background: Some(pair.color.into()),
            border: Border {
                color: pair.color,
                width: 0.0,
                radius: 4.0.into(),
            },
            ..container::Style::default()
        }
    };
    container(
        row![
            button(text(title).size(14))
                .padding([2, 8])
                .style(move |theme, status| tab_text(theme, status, active))
                .on_press(Message::Select(index)),
            button(text("×").size(14))
                .padding([2, 6])
                .style(move |theme, status| tab_text(theme, status, active))
                .on_press(Message::Close(index)),
        ]
        .spacing(0)
        .align_y(alignment::Vertical::Center),
    )
    .style(chip)
    .into()
}

/// Same text color as the Lines button: primary text on the active chip,
/// ordinary text on the others. `button::text` would replace the chip color.
fn tab_text(theme: &Theme, status: button::Status, active: bool) -> button::Style {
    let palette = theme.extended_palette();
    let mut style = button::text(theme, status);
    style.text_color = if active {
        palette.primary.base.text
    } else {
        palette.background.base.text
    };
    // Lines-off fades its label on hover. The primary Lines button does not.
    if !active && matches!(status, button::Status::Hovered) {
        style.text_color = style.text_color.scale_alpha(0.8);
    }
    style
}

fn line_button(on: bool) -> Element<'static, Message> {
    let style = if on { button::primary } else { button::text };
    button(text("Lines").size(14))
        .padding([2, 8])
        .style(style)
        .on_press(Message::ToggleLineNumbers)
        .into()
}

fn text_button<'a>(label: &'static str, message: Message) -> Element<'a, Message> {
    button(text(label).size(14))
        .padding([2, 8])
        .style(button::text)
        .on_press(message)
        .into()
}
