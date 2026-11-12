use crate::Primitive;
use crate::core::renderer::Quad;
use crate::core::{
    self, Background, Color, Point, Rectangle, Svg, Transformation,
};
use crate::graphics::damage;
use crate::graphics::layer;
use crate::graphics::text::{Editor, Paragraph, Text};
use crate::graphics::{self, Image};

use std::sync::Arc;

pub type Stack = layer::Stack<Layer>;

#[derive(Debug, Clone)]
pub struct Layer {
    pub bounds: Rectangle,
    pub quads: Vec<(Quad, Background)>,
    pub primitives: Vec<Item<Primitive>>,
    pub images: Vec<Image>,
    pub text: Vec<Item<Text>>,
    /// Copied from the hinting flag when this layer was recorded.
    pub hinting: bool,
}

impl Layer {
    pub fn draw_quad(
        &mut self,
        mut quad: Quad,
        background: Background,
        transformation: Transformation,
    ) {
        let background = crate::fade::background(background);
        quad.bounds = quad.bounds * transformation;
        self.quads.push((quad, background));
    }

    pub fn draw_paragraph(
        &mut self,
        paragraph: &Paragraph,
        position: Point,
        color: Color,
        clip_bounds: Rectangle,
        transformation: Transformation,
    ) {
        let color = crate::fade::color(color);
        let paragraph = Text::Paragraph {
            paragraph: paragraph.downgrade(),
            position,
            color,
            clip_bounds,
            transformation,
        };

        self.text.push(Item::Live(paragraph));
    }

    pub fn draw_editor(
        &mut self,
        editor: &Editor,
        position: Point,
        color: Color,
        clip_bounds: Rectangle,
        transformation: Transformation,
    ) {
        let color = crate::fade::color(color);
        let editor = Text::Editor {
            editor: editor.downgrade(),
            position,
            color,
            clip_bounds,
            transformation,
        };

        self.text.push(Item::Live(editor));
    }

    pub fn draw_text(
        &mut self,
        text: core::Text,
        position: Point,
        color: Color,
        clip_bounds: Rectangle,
        transformation: Transformation,
    ) {
        let color = crate::fade::color(color);
        let text = Text::Cached {
            content: text.content,
            bounds: Rectangle::new(position, text.bounds) * transformation,
            color,
            size: text.size * transformation.scale_factor(),
            line_height: text.line_height.to_absolute(text.size)
                * transformation.scale_factor(),
            font: text.font,
            align_x: text.align_x,
            align_y: text.align_y,
            shaping: text.shaping,
            clip_bounds: clip_bounds * transformation,
        };

        self.text.push(Item::Live(text));
    }

    pub fn draw_text_raw(
        &mut self,
        raw: graphics::text::Raw,
        transformation: Transformation,
    ) {
        let raw = Text::Raw {
            raw,
            transformation,
        };

        self.text.push(Item::Live(raw));
    }

    pub fn draw_text_group(
        &mut self,
        text: Vec<Text>,
        clip_bounds: Rectangle,
        transformation: Transformation,
    ) {
        self.text
            .push(Item::Group(text, clip_bounds, transformation));
    }

    pub fn draw_text_cache(
        &mut self,
        text: Arc<[Text]>,
        clip_bounds: Rectangle,
        transformation: Transformation,
    ) {
        self.text
            .push(Item::Cached(text, clip_bounds, transformation));
    }

    pub fn draw_image(&mut self, image: Image, transformation: Transformation) {
        match image {
            Image::Raster {
                image,
                bounds,
                clip_bounds,
            } => {
                self.draw_raster(image, bounds, clip_bounds, transformation);
            }
            Image::Vector {
                svg,
                bounds,
                clip_bounds,
            } => {
                self.draw_svg(svg, bounds, clip_bounds, transformation);
            }
        }
    }

    pub fn draw_raster(
        &mut self,
        image: core::Image,
        bounds: Rectangle,
        clip_bounds: Rectangle,
        transformation: Transformation,
    ) {
        let image = Image::Raster {
            image: core::Image {
                border_radius: image.border_radius
                    * transformation.scale_factor(),
                ..image
            },
            bounds: bounds * transformation,
            clip_bounds: clip_bounds * transformation,
        };

        self.images.push(image);
    }

    pub fn draw_svg(
        &mut self,
        svg: Svg,
        bounds: Rectangle,
        clip_bounds: Rectangle,
        transformation: Transformation,
    ) {
        let svg = Image::Vector {
            svg,
            bounds: bounds * transformation,
            clip_bounds: clip_bounds * transformation,
        };

        self.images.push(svg);
    }

    pub fn draw_primitive_group(
        &mut self,
        primitives: Vec<Primitive>,
        clip_bounds: Rectangle,
        transformation: Transformation,
    ) {
        self.primitives.push(Item::Group(
            primitives,
            clip_bounds * transformation,
            transformation,
        ));
    }

    pub fn draw_primitive_cache(
        &mut self,
        primitives: Arc<[Primitive]>,
        clip_bounds: Rectangle,
        transformation: Transformation,
    ) {
        self.primitives.push(Item::Cached(
            primitives,
            clip_bounds * transformation,
            transformation,
        ));
    }

    pub fn damage(previous: &Self, current: &Self) -> Vec<Rectangle> {
        if previous.bounds != current.bounds {
            return vec![previous.bounds, current.bounds];
        }
        // Hinting changes the glyph bitmaps without changing the text record.
        if previous.hinting != current.hinting {
            return vec![current.bounds];
        }

        let mut damage = damage::list(
            &previous.quads,
            &current.quads,
            |(quad, _)| {
                quad.bounds
                    .expand(1.0)
                    .intersection(&current.bounds)
                    .into_iter()
                    .collect()
            },
            |(quad_a, background_a), (quad_b, background_b)| {
                quad_a == quad_b && background_a == background_b
            },
        );

        let text = damage::diff(
            &previous.text,
            &current.text,
            |item| {
                item.as_slice()
                    .iter()
                    .filter_map(text_region)
                    .map(|bounds| bounds * item.transformation())
                    .collect()
            },
            |text_a, text_b| {
                // A scroll only changes the group transform. Editor equality
                // ignores the buffer, so a caret blink would leave the old glyphs.
                if text_a.transformation() != text_b.transformation()
                    || text_a.clip_bounds() != text_b.clip_bounds()
                {
                    let mut regions = Vec::new();
                    regions.extend(text_a.as_slice().iter().filter_map(|text| {
                        text_region(text)
                            .map(|bounds| bounds * text_a.transformation())
                    }));
                    regions.extend(text_b.as_slice().iter().filter_map(|text| {
                        text_region(text)
                            .map(|bounds| bounds * text_b.transformation())
                    }));
                    // Live text reports an infinite clip. Damaging that clears a
                    // full-screen strip and leaves the gutter flashing there.
                    if let Some(bounds) = finite_damage(text_a.clip_bounds()) {
                        regions.push(bounds);
                    }
                    if let Some(bounds) = finite_damage(text_b.clip_bounds()) {
                        regions.push(bounds);
                    }
                    return regions;
                }
                damage::list(
                    text_a.as_slice(),
                    text_b.as_slice(),
                    |text| {
                        text_region(text)
                            .into_iter()
                            .map(|bounds| bounds * text_a.transformation())
                            .collect()
                    },
                    |text_a, text_b| match (text_a, text_b) {
                        // Editor and paragraph equality ignore the buffer text,
                        // so a caret move or a status label would keep old glyphs.
                        (Text::Editor { .. }, Text::Editor { .. })
                        | (Text::Paragraph { .. }, Text::Paragraph { .. }) => false,
                        _ => text_a == text_b,
                    },
                )
            },
        );

        let primitives = damage::list(
            &previous.primitives,
            &current.primitives,
            |item| match item {
                Item::Live(primitive) => vec![primitive.visible_bounds()],
                Item::Group(primitives, group_bounds, transformation) => {
                    primitives
                        .as_slice()
                        .iter()
                        .map(Primitive::visible_bounds)
                        .map(|bounds| bounds * *transformation)
                        .filter_map(|bounds| bounds.intersection(group_bounds))
                        .collect()
                }
                Item::Cached(_primitives, bounds, _transformation) => {
                    vec![*bounds]
                }
            },
            |primitive_a, primitive_b| match (primitive_a, primitive_b) {
                (
                    Item::Cached(cache_a, bounds_a, transformation_a),
                    Item::Cached(cache_b, bounds_b, transformation_b),
                ) => {
                    Arc::ptr_eq(cache_a, cache_b)
                        && bounds_a == bounds_b
                        && transformation_a == transformation_b
                }
                _ => false,
            },
        );

        let images = damage::list(
            &previous.images,
            &current.images,
            |image| vec![image.bounds().expand(1.0)],
            Image::eq,
        );

        damage.extend(text);
        damage.extend(primitives);
        damage.extend(images);
        damage
    }
}

impl Default for Layer {
    fn default() -> Self {
        Self {
            bounds: Rectangle::INFINITE,
            quads: Vec::new(),
            primitives: Vec::new(),
            text: Vec::new(),
            images: Vec::new(),
            hinting: crate::hint::hinting(),
        }
    }
}

impl graphics::Layer for Layer {
    fn with_bounds(bounds: Rectangle) -> Self {
        Self {
            bounds,
            ..Self::default()
        }
    }

    fn bounds(&self) -> Rectangle {
        self.bounds
    }

    fn flush(&mut self) {}

    fn resize(&mut self, bounds: Rectangle) {
        self.bounds = bounds;
    }

    fn reset(&mut self) {
        self.bounds = Rectangle::INFINITE;

        self.quads.clear();
        self.primitives.clear();
        self.text.clear();
        self.images.clear();
        self.hinting = crate::hint::hinting();
    }

    fn start(&self) -> usize {
        if !self.quads.is_empty() {
            return 1;
        }

        if !self.primitives.is_empty() {
            return 2;
        }

        if !self.images.is_empty() {
            return 3;
        }

        if !self.text.is_empty() {
            return 4;
        }

        usize::MAX
    }

    fn end(&self) -> usize {
        if !self.text.is_empty() {
            return 4;
        }

        if !self.images.is_empty() {
            return 3;
        }

        if !self.primitives.is_empty() {
            return 2;
        }

        if !self.quads.is_empty() {
            return 1;
        }

        0
    }

    fn merge(&mut self, layer: &mut Self) {
        self.quads.append(&mut layer.quads);
        self.primitives.append(&mut layer.primitives);
        self.text.append(&mut layer.text);
        self.images.append(&mut layer.images);
    }
}

/// Region to redraw for one text primitive.
///
/// `fill_text` records the anchor box. Right alignment paints the glyphs
/// to the left of that box, and Center paints them around the anchor.
/// Damaging only the recorded box clears the editor margin and leaves the
/// old gutter digits where they were. Compact numbers are two digits wide,
/// so the missed strip is the tens digit and the ones digit still updates.
pub fn text_region(text: &Text) -> Option<Rectangle> {
    let Text::Cached {
        bounds,
        clip_bounds,
        align_x,
        align_y,
        ..
    } = text
    else {
        return text.visible_bounds();
    };
    let mut ink = *bounds;
    ink.x = match align_x {
        crate::core::text::Alignment::Right => bounds.x - bounds.width,
        crate::core::text::Alignment::Center => bounds.x - bounds.width / 2.0,
        crate::core::text::Alignment::Default
        | crate::core::text::Alignment::Left
        | crate::core::text::Alignment::Justified => bounds.x,
    };
    ink.y = match align_y {
        crate::core::alignment::Vertical::Center => {
            bounds.y - bounds.height / 2.0
        }
        crate::core::alignment::Vertical::Bottom => bounds.y - bounds.height,
        crate::core::alignment::Vertical::Top => bounds.y,
    };
    // Glyphs overhang the layout box by a pixel or two. Expand after the
    // clip so that ink is still damaged when it sits on the clip edge.
    ink.intersection(clip_bounds).map(|ink| ink.expand(2.0))
}

fn finite_damage(bounds: Rectangle) -> Option<Rectangle> {
    if bounds.x.is_finite()
        && bounds.y.is_finite()
        && bounds.width.is_finite()
        && bounds.height.is_finite()
        && bounds.width > 0.0
        && bounds.height > 0.0
        && bounds.width < 1.0e6
        && bounds.height < 1.0e6
    {
        Some(bounds)
    } else {
        None
    }
}

#[derive(Debug, Clone)]
pub enum Item<T> {
    Live(T),
    Group(Vec<T>, Rectangle, Transformation),
    Cached(Arc<[T]>, Rectangle, Transformation),
}

impl<T> Item<T> {
    pub fn transformation(&self) -> Transformation {
        match self {
            Item::Live(_) => Transformation::IDENTITY,
            Item::Group(_, _, transformation)
            | Item::Cached(_, _, transformation) => *transformation,
        }
    }

    pub fn clip_bounds(&self) -> Rectangle {
        match self {
            Item::Live(_) => Rectangle::INFINITE,
            Item::Group(_, clip_bounds, _)
            | Item::Cached(_, clip_bounds, _) => *clip_bounds,
        }
    }

    pub fn as_slice(&self) -> &[T] {
        match self {
            Item::Live(item) => std::slice::from_ref(item),
            Item::Group(group, _, _) => group.as_slice(),
            Item::Cached(cache, _, _) => cache,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::text::{Alignment, Shaping};
    use crate::core::{Color, Font, Pixels, Point, Size};
    use crate::graphics::text::Text;

    fn cached(content: &str, x: f32, align_x: Alignment) -> Text {
        Text::Cached {
            content: content.to_string(),
            bounds: Rectangle::new(Point::new(x, 30.0), Size::new(20.0, 22.0)),
            color: Color::WHITE,
            size: Pixels(11.2),
            line_height: Pixels(22.0),
            font: Font::MONOSPACE,
            align_x,
            align_y: crate::core::alignment::Vertical::Top,
            shaping: Shaping::Advanced,
            clip_bounds: Rectangle::new(Point::ORIGIN, Size::new(400.0, 300.0)),
        }
    }

    fn covers(regions: &[Rectangle], point: Point) -> bool {
        regions.iter().any(|region| region.contains(point))
    }

    #[test]
    fn right_aligned_damage_covers_the_digits_left_of_the_anchor() {
        let mut previous = Layer::default();
        let mut current = Layer::default();
        // Anchor at x=48. The 20px shaping box, and the digits, sit to its left.
        previous
            .text
            .push(Item::Live(cached("53", 48.0, Alignment::Right)));
        current
            .text
            .push(Item::Live(cached("73", 48.0, Alignment::Right)));
        let damage = Layer::damage(&previous, &current);
        assert!(
            covers(&damage, Point::new(30.0, 40.0)),
            "tens digit was not damaged: {damage:?}"
        );
        assert!(
            covers(&damage, Point::new(46.0, 40.0)),
            "ones digit was not damaged: {damage:?}"
        );
    }

    #[test]
    fn left_aligned_damage_stays_on_the_anchor_box() {
        let mut previous = Layer::default();
        let mut current = Layer::default();
        previous
            .text
            .push(Item::Live(cached("53", 48.0, Alignment::Left)));
        current
            .text
            .push(Item::Live(cached("73", 48.0, Alignment::Left)));
        let damage = Layer::damage(&previous, &current);
        assert!(
            covers(&damage, Point::new(50.0, 40.0)),
            "left-aligned text was not damaged: {damage:?}"
        );
        assert!(
            !covers(&damage, Point::new(20.0, 40.0)),
            "left-aligned damage slid off the text: {damage:?}"
        );
    }
}
