//! Gutter, caret band, and scrollbar for one editor.
//!
//! `view` runs before the editor lays the buffer out, so a snap read there is
//! the previous frame: numbers land on the old scroll while the glyphs use the
//! new one. These are painted at draw time, after this widget has laid the
//! editor out and that layout has published the snap.

use crate::editor::wrap::{self, Viewport};
use crate::geom;
use crate::message::Message;
use crate::ui::visible_line;
use iced::advanced::layout::{self, Layout, Node};
use iced::advanced::renderer;
use iced::advanced::text::{self, Text};
use iced::advanced::widget::{Tree, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::{
    Border, Color, Element, Event, Font, Length, Pixels, Point, Rectangle, Size, Vector, mouse,
    overlay,
};

pub struct Band<'a, Renderer> {
    editor: Element<'a, Message, iced::Theme, Renderer>,
    /// Lines, bytes, and the first and last line. `None` before the buffer exists.
    key: Option<(usize, usize, u64, u64)>,
    caret_line: usize,
    caret_column: usize,
    rows: u32,
    /// Buffer lines, used only when no real snap has been published yet.
    fallback_lines: usize,
    text_size: f32,
    /// Cell height. Used when the snap still has the editor's initial 1px line.
    row_height: f32,
    pad: f32,
    gutter: f32,
    /// Editor face. Line numbers stay with the buffer, not the UI face.
    number_font: Font,
}

pub fn band<'a, Renderer>(
    editor: impl Into<Element<'a, Message, iced::Theme, Renderer>>,
    key: Option<(usize, usize, u64, u64)>,
    caret_line: usize,
    caret_column: usize,
    rows: u32,
    fallback_lines: usize,
    text_size: f32,
    row_height: f32,
    pad: f32,
    gutter: f32,
    number_font: Font,
) -> Band<'a, Renderer> {
    Band {
        editor: editor.into(),
        key,
        caret_line,
        caret_column,
        rows,
        fallback_lines,
        text_size,
        row_height,
        pad,
        gutter,
        number_font,
    }
}

/// The editor's published rows, and the line pitch those rows were measured with.
fn placed(
    key: Option<(usize, usize, u64, u64)>,
    rows: u32,
    caret_line: usize,
    caret_column: usize,
    fallback_lines: usize,
    row_height: f32,
) -> (Viewport, f32) {
    let pitch = row_height.max(1.0);
    // `with_text` publishes before the widget has a size or a real line height.
    // That snap is one pixel per line, which parks the caret at the top.
    let snap = key.and_then(|(lines, bytes, first, last)| {
        cosmic_text::layout_snap(lines, bytes, first, last)
    });
    if let Some(snap) = snap.as_ref().filter(|snap| snap.line_height >= 2.0) {
        return (
            wrap::from_snap(snap, rows, caret_line, caret_column),
            snap.line_height,
        );
    }
    let lines = fallback_lines.max(1);
    let ends = vec![vec![0usize]; lines];
    (
        wrap::from_ends(&ends, 0, rows, caret_line, caret_column),
        pitch,
    )
}

fn gutter_text_size(text_size: f32) -> f32 {
    (text_size * 0.7).clamp(10.0, 16.0)
}

impl<'a, Renderer> Widget<Message, iced::Theme, Renderer> for Band<'a, Renderer>
where
    Renderer: iced::advanced::Renderer + text::Renderer<Font = Font>,
{
    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Fill,
        }
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.editor)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.editor));
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> Node {
        if tree.children.is_empty() {
            tree.children.push(Tree::new(&self.editor));
        }
        let size = limits.resolve(Length::Fill, Length::Fill, Size::ZERO);
        let scroll_w = geom::SCROLLBAR as f32;
        let gutter = self.gutter.min((size.width - scroll_w).max(0.0));
        let editor_w = (size.width - gutter - scroll_w).max(0.0);
        let editor_limits = layout::Limits::new(
            Size::new(editor_w, size.height),
            Size::new(editor_w, size.height),
        );
        let editor = self
            .editor
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, &editor_limits)
            .move_to(Point::new(gutter, 0.0));
        Node::with_children(size, vec![editor])
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        let Some(child) = layout.children().next() else {
            return;
        };
        self.editor
            .as_widget_mut()
            .operate(&mut tree.children[0], child, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
            && cursor
                .position()
                .is_some_and(|point| scrollbar_bounds(layout.bounds()).contains(point))
        {
            shell.publish(Message::ScrollGrab);
            shell.capture_event();
            return;
        }
        let Some(child) = layout.children().next() else {
            return;
        };
        self.editor.as_widget_mut().update(
            &mut tree.children[0],
            event,
            child,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        if cursor
            .position()
            .is_some_and(|point| scrollbar_bounds(layout.bounds()).contains(point))
        {
            return mouse::Interaction::default();
        }
        let Some(child) = layout.children().next() else {
            return mouse::Interaction::default();
        };
        self.editor.as_widget().mouse_interaction(
            &tree.children[0],
            child,
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let Some(visible) = bounds.intersection(viewport) else {
            return;
        };
        let (view, pitch) = placed(
            self.key,
            self.rows,
            self.caret_line,
            self.caret_column,
            self.fallback_lines,
            self.row_height,
        );
        let scroll_w = geom::SCROLLBAR as f32;
        let gutter = self.gutter.min((bounds.width - scroll_w).max(0.0));
        // The text fills the band, so the last line ends where the scrollbar does.
        // `pad` stays in the gutter, beside the digits.
        let text_box = Rectangle {
            x: bounds.x,
            y: bounds.y,
            width: (bounds.width - scroll_w).max(0.0),
            height: bounds.height,
        };
        let highlight = caret_color(theme);
        for (index, slot) in view.slots.iter().enumerate() {
            if !slot.current {
                continue;
            }
            let origin = view.lead + index as f32 * pitch;
            let Some((y, height)) = visible_line(origin, pitch, text_box.height) else {
                continue;
            };
            let bar = Rectangle {
                x: text_box.x,
                y: text_box.y + y,
                width: text_box.width,
                height,
            };
            if let Some(bar) = bar.intersection(&visible) {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: bar,
                        ..renderer::Quad::default()
                    },
                    highlight,
                );
            }
        }

        if gutter > 0.0 {
            let side = self.pad / 2.0;
            let number_w = (gutter - side * 2.0).max(0.0);
            let size = gutter_text_size(self.text_size);
            for (index, slot) in view.slots.iter().enumerate() {
                let Some(number) = slot.number else {
                    continue;
                };
                let origin = view.lead + index as f32 * pitch;
                // The point is the anchor. Right is the right edge of the
                // digits, and Top is the top of the line box. Passing the
                // top-left instead parks the number half a line high and
                // against the left edge, which clips 10 and up to one digit.
                renderer.fill_text(
                    Text {
                        content: number.to_string(),
                        bounds: Size::new(number_w, pitch),
                        size: Pixels(size),
                        line_height: text::LineHeight::Absolute(Pixels(pitch)),
                        font: self.number_font,
                        align_x: text::Alignment::Right,
                        align_y: alignment_top(),
                        shaping: text::Shaping::Advanced,
                        wrapping: text::Wrapping::None,
                    },
                    Point::new(bounds.x + gutter - side, text_box.y + origin),
                    number_color(theme, slot.current),
                    text_box,
                );
            }
        }

        if let Some(child) = layout.children().next()
            && let Some(editor_clip) = child.bounds().intersection(&visible)
        {
            self.editor.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                child,
                cursor,
                &editor_clip,
            );
        }

        paint_scrollbar(
            renderer,
            theme,
            scrollbar_bounds(bounds),
            &visible,
            pitch,
            view.total,
            view.scroll,
            view.lead,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, Renderer>> {
        let Some(child) = layout.children().next() else {
            return None;
        };
        self.editor.as_widget_mut().overlay(
            &mut tree.children[0],
            child,
            renderer,
            viewport,
            translation,
        )
    }
}

fn alignment_top() -> iced::alignment::Vertical {
    iced::alignment::Vertical::Top
}

fn caret_color(theme: &iced::Theme) -> Color {
    let mut color = theme.extended_palette().primary.weak.color;
    color.a = 0.35;
    color
}

fn number_color(theme: &iced::Theme, current: bool) -> Color {
    theme
        .palette()
        .text
        .scale_alpha(if current { 0.9 } else { 0.4 })
}

fn scrollbar_bounds(bounds: Rectangle) -> Rectangle {
    let width = geom::SCROLLBAR as f32;
    Rectangle {
        x: bounds.x + bounds.width - width,
        y: bounds.y,
        width: width.min(bounds.width),
        height: bounds.height,
    }
}

fn paint_scrollbar<Renderer>(
    renderer: &mut Renderer,
    theme: &iced::Theme,
    track: Rectangle,
    viewport: &Rectangle,
    line_h: f32,
    total: u32,
    scroll: i32,
    lead: f32,
) where
    Renderer: iced::advanced::Renderer,
{
    let Some(track) = track.intersection(viewport) else {
        return;
    };
    let mut track_color = theme.extended_palette().background.strong.color;
    track_color.a = 0.45;
    renderer.fill_quad(
        renderer::Quad {
            bounds: track,
            ..renderer::Quad::default()
        },
        track_color,
    );
    let (thumb_h, offset) = geom::thumb_span_px(track.height, line_h, total, scroll, lead);
    let thumb = Rectangle {
        x: track.x,
        y: track.y + offset,
        width: track.width,
        height: thumb_h.max(1.0),
    };
    let Some(thumb) = thumb.intersection(&track) else {
        return;
    };
    let mut thumb_color = theme.extended_palette().primary.strong.color;
    thumb_color.a = 0.75;
    renderer.fill_quad(
        renderer::Quad {
            bounds: thumb,
            border: Border {
                color: thumb_color,
                width: 0.0,
                radius: 2.0.into(),
            },
            ..renderer::Quad::default()
        },
        thumb_color,
    );
}

#[cfg(test)]
mod damage {
    use iced_tiny_skia::core::text::{Alignment, Shaping};
    use iced_tiny_skia::core::{Color, Font, Pixels, Point, Rectangle, Size};
    use iced_tiny_skia::graphics::text::Text;

    fn cached(content: &str, x: f32, align_x: Alignment) -> Text {
        Text::Cached {
            content: content.to_string(),
            bounds: Rectangle::new(Point::new(x, 30.0), Size::new(20.0, 22.0)),
            color: Color::WHITE,
            size: Pixels(11.2),
            line_height: Pixels(22.0),
            font: Font::MONOSPACE,
            align_x,
            align_y: iced_tiny_skia::core::alignment::Vertical::Top,
            shaping: Shaping::Advanced,
            clip_bounds: Rectangle::new(Point::ORIGIN, Size::new(400.0, 300.0)),
        }
    }

    #[test]
    fn right_aligned_gutter_damage_covers_both_digits() {
        // The anchor is the right edge. Compact reserves a 20px box to its left.
        let region =
            iced_tiny_skia::text_region(&cached("53", 48.0, Alignment::Right)).expect("ink");
        assert!(region.contains(Point::new(30.0, 40.0)), "{region:?}");
        assert!(region.contains(Point::new(46.0, 40.0)), "{region:?}");
    }

    #[test]
    fn left_aligned_damage_stays_on_the_anchor_box() {
        let region =
            iced_tiny_skia::text_region(&cached("53", 48.0, Alignment::Left)).expect("ink");
        assert!(region.contains(Point::new(50.0, 40.0)), "{region:?}");
        assert!(!region.contains(Point::new(20.0, 40.0)), "{region:?}");
    }
}

impl<'a, Renderer> From<Band<'a, Renderer>> for Element<'a, Message, iced::Theme, Renderer>
where
    Renderer: iced::advanced::Renderer + text::Renderer<Font = Font> + 'a,
{
    fn from(band: Band<'a, Renderer>) -> Self {
        Element::new(band)
    }
}
