//! Puts the drawer at a window position.
//!
//! A transform that leaves the layout bounds where they were does not damage
//! the trail. This widget moves the child's bounds, so a slide clears the
//! pixels it left behind.

use iced::advanced::layout::{self, Layout, Node};
use iced::advanced::renderer;
use iced::advanced::widget::{Tree, Widget};
use iced::advanced::{Clipboard, Shell, mouse, overlay};
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector};

pub struct Pin<'a, Message, Theme, Renderer> {
    at: Point,
    child_size: Size,
    /// 1 is solid. Lower values fade the drawer with the short slide.
    fade: f32,
    content: Element<'a, Message, Theme, Renderer>,
}

pub fn pin<'a, Message, Theme, Renderer>(
    at: Point,
    child_size: Size,
    fade: f32,
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
) -> Pin<'a, Message, Theme, Renderer>
where
    Renderer: iced::advanced::Renderer,
{
    Pin {
        at,
        child_size,
        fade,
        content: content.into(),
    }
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Pin<'a, Message, Theme, Renderer>
where
    Renderer: iced::advanced::Renderer,
{
    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Fill,
        }
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> Node {
        let size = limits.resolve(Length::Fill, Length::Fill, Size::ZERO);
        if tree.children.is_empty() {
            tree.children.push(Tree::new(&self.content));
        }
        let child_limits = layout::Limits::new(self.child_size, self.child_size);
        let child =
            self.content
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, &child_limits);
        Node::with_children(size, vec![child.move_to(self.at)])
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
        self.content
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
        let Some(child) = layout.children().next() else {
            return;
        };
        self.content.as_widget_mut().update(
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
        let Some(child) = layout.children().next() else {
            return mouse::Interaction::None;
        };
        self.content.as_widget().mouse_interaction(
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
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let Some(child) = layout.children().next() else {
            return;
        };
        let Some(visible) = child.bounds().intersection(viewport) else {
            return;
        };
        let _fade = crate::fade::Reset::push(self.fade);
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            child,
            cursor,
            &visible,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let Some(child) = layout.children().next() else {
            return None;
        };
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            child,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message, Theme, Renderer> From<Pin<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: iced::advanced::Renderer + 'a,
{
    fn from(pin: Pin<'a, Message, Theme, Renderer>) -> Self {
        Element::new(pin)
    }
}
