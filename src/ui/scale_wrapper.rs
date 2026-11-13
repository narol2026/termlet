//! Reusable scale transform wrapper widget.
//!
//! Draws its child scaled around the child's own center. `scale` is `1.0` at
//! rest; `< 1.0` draws the child smaller, anchored at its center. Layout
//! bounds — and therefore hit-testing — stay at full size, so interaction is
//! unaffected. Used for the spotlight / message-dialog zoom-in entrance.

use iced::advanced::widget::{Operation, Tree, Widget};
use iced::advanced::{Clipboard, Layout, Renderer as _, Shell, overlay};
use iced::mouse::Cursor;
use iced::{Element, Length, Rectangle, Theme, Vector};

use crate::core::message::Message;

struct ScaleWrapper<'a> {
    content: Element<'a, Message>,
    scale: f32,
}

pub fn scale_wrapper(content: Element<'_, Message>, scale: f32) -> Element<'_, Message> {
    Element::new(ScaleWrapper { content, scale })
}

impl Widget<Message, Theme, iced::Renderer> for ScaleWrapper<'_> {
    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> iced::advanced::widget::tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }

    fn size(&self) -> iced::Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> iced::Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &iced::advanced::renderer::Style,
        layout: Layout<'_>,
        cursor: Cursor,
        viewport: &Rectangle,
    ) {
        if self.scale < 1.0 {
            let bounds = layout.bounds();
            // Scale around the child's center: T(c) * S(s) * T(-c).
            let transformation = iced::Transformation::translate(
                bounds.x + bounds.width / 2.0,
                bounds.y + bounds.height / 2.0,
            ) * iced::Transformation::scale(self.scale)
                * iced::Transformation::translate(
                    -bounds.x - bounds.width / 2.0,
                    -bounds.y - bounds.height / 2.0,
                );
            renderer.with_transformation(transformation, |renderer| {
                self.content
                    .as_widget()
                    .draw(tree, renderer, theme, style, layout, cursor, viewport);
            });
        } else {
            self.content
                .as_widget()
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> iced::mouse::Interaction {
        self.content
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(tree, layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &iced::Event,
        layout: Layout<'_>,
        cursor: Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation)
    }
}
