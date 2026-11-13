//! Split drag-preview overlay widget.

// Rust guideline compliant 2026-10-09

use iced::advanced::widget::{Operation, Tree, Widget};
use iced::advanced::{Clipboard, Layout, Renderer as _, Shell, overlay, renderer};
use iced::mouse::Cursor;
use iced::{Border, Color, Element, Rectangle, Size, Theme, Vector};

use crate::core::message::Message;
use crate::terminal::DragPreview;
use crate::terminal::split::preview_geometry;

pub(super) fn split_preview_wrapper<'a>(
    content: Element<'a, Message>,
    preview: Option<&DragPreview>,
) -> Element<'a, Message> {
    Element::new(SplitPreviewWrapper {
        content,
        preview: preview.copied(),
    })
}

struct SplitPreviewWrapper<'a> {
    content: Element<'a, Message>,
    preview: Option<DragPreview>,
}

impl Widget<Message, Theme, iced::Renderer> for SplitPreviewWrapper<'_> {
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

    fn size(&self) -> iced::Size<iced::Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> iced::Size<iced::Length> {
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
        self.content
            .as_widget()
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
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
        // No drag in progress: this wrapper is transparent, so forward any child
        // overlays (e.g. the sidebar's anchored dropdown popups) to the root
        // layer — otherwise returning `None` here would silently swallow them.
        let Some(preview) = self.preview else {
            return self.content.as_widget_mut().overlay(
                tree,
                layout,
                renderer,
                viewport,
                translation,
            );
        };
        Some(overlay::Element::new(Box::new(PreviewOverlay {
            preview,
            content_tree: tree,
            content: &mut self.content,
        })))
    }
}

struct PreviewOverlay<'a, 'b> {
    preview: DragPreview,
    content_tree: &'b mut Tree,
    content: &'b mut Element<'a, Message>,
}

impl overlay::Overlay<Message, Theme, iced::Renderer> for PreviewOverlay<'_, '_> {
    fn layout(&mut self, renderer: &iced::Renderer, bounds: Size) -> iced::advanced::layout::Node {
        self.content.as_widget_mut().layout(
            self.content_tree,
            renderer,
            &iced::advanced::layout::Limits::new(Size::ZERO, bounds),
        )
    }

    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &iced::advanced::renderer::Style,
        _layout: Layout<'_>,
        _cursor: Cursor,
    ) {
        let Some(geometry) = preview_geometry(self.preview.bounds, self.preview.grid) else {
            return;
        };

        let line_color = Color::from_rgba(0.25, 0.58, 1.0, 0.72);
        let fill_color = Color::from_rgba(0.25, 0.58, 1.0, 0.16);

        if let Some(highlight) = geometry.highlight {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: highlight,
                    border: Border::default(),
                    shadow: iced::Shadow::default(),
                    snap: false,
                },
                fill_color,
            );
        }

        if let Some(divider) = geometry.divider {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: divider,
                    border: Border::default(),
                    shadow: iced::Shadow::default(),
                    snap: false,
                },
                line_color,
            );
        }

        for outline in geometry.outline {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: outline,
                    border: Border::default(),
                    shadow: iced::Shadow::default(),
                    snap: false,
                },
                line_color,
            );
        }
    }

    fn mouse_interaction(
        &self,
        _layout: Layout<'_>,
        _cursor: Cursor,
        _renderer: &iced::Renderer,
    ) -> iced::mouse::Interaction {
        iced::mouse::Interaction::default()
    }
}
