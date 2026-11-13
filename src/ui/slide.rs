//! Reusable slide and fixed-width wrapper widgets.
//!
//! [`slide_wrapper`] draws its child translated vertically. `progress` is
//! `1.0` at rest; `< 1.0` shifts the child up by `(1.0 - progress) * height`.
//! [`fixed_width_wrapper`] keeps its child at its intrinsic width while the
//! parent container clips the visible portion, preventing text reflow while a
//! panel slides in from the right.

use iced::advanced::widget::{Operation, Tree, Widget};
use iced::advanced::{Clipboard, Layout, Renderer as _, Shell, overlay};
use iced::mouse::Cursor;
use iced::{Element, Length, Rectangle, Theme, Vector};

use crate::core::message::Message;

struct SlideWrapper<'a> {
    content: Element<'a, Message>,
    progress: f32,
    fixed_width: bool,
}

pub fn slide_wrapper(content: Element<'_, Message>, progress: f32) -> Element<'_, Message> {
    Element::new(SlideWrapper {
        content,
        progress,
        fixed_width: false,
    })
}

/// Wrap `content` so it is laid out at its intrinsic width even when the
/// parent container animates a smaller clip width.  Used by right-side slide
/// panels and bottom-right popups so text does not reflow while sliding.
pub fn fixed_width_wrapper(content: Element<'_, Message>) -> Element<'_, Message> {
    Element::new(SlideWrapper {
        content,
        progress: 1.0,
        fixed_width: true,
    })
}

/// Build a right-side panel that slides in from the right.
///
/// The panel content is laid out at its final `width` during the whole
/// animation, while the outer container animates its visible clip width.
pub fn slide_panel_from_right(
    content: Element<'_, Message>,
    width: f32,
    visible: bool,
    progress: f32,
) -> Element<'_, Message> {
    let eased = if visible {
        crate::ui::helpers::ease_out_cubic(progress)
    } else {
        0.0
    };

    iced::widget::container(fixed_width_wrapper(content))
        .width(Length::Fixed(eased * width))
        .height(Length::Fill)
        .clip(true)
        .into()
}

impl Widget<Message, Theme, iced::Renderer> for SlideWrapper<'_> {
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
        if self.fixed_width {
            // Panels sliding in from the right must keep their intrinsic width
            // while the parent container clips the visible portion. Don't let
            // the animated (smaller) parent width re-layout the child at
            // intermediate widths, which causes text reflow.
            let child_limits = iced::advanced::layout::Limits::new(
                iced::Size::ZERO,
                iced::Size::new(f32::INFINITY, limits.max().height),
            );
            self.content
                .as_widget_mut()
                .layout(tree, renderer, &child_limits)
        } else {
            self.content.as_widget_mut().layout(tree, renderer, limits)
        }
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
        if self.fixed_width {
            self.content
                .as_widget()
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
            return;
        }

        let progress = self.progress.clamp(0.0, 1.0);
        if progress < 1.0 {
            // Slide up by the fraction of the child's own height that is still
            // "out of view"; at progress 0.0 the child sits fully above its
            // resting position and is clipped by the parent's bounds.
            let offset = -(1.0 - progress) * layout.bounds().height;
            let transformation = iced::Transformation::translate(0.0, offset);
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
