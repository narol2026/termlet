//! Modal host that gives one overlay exclusive input ownership.
//!
//! The base widget keeps the same position in the widget tree while an overlay
//! is shown, preserving scroll state underneath. Mouse, keyboard, and touch
//! events are delivered only to the overlay while it exists.

// Rust guideline compliant 2026-10-09

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Tree, Widget};
use iced::advanced::{Clipboard, Shell, overlay, renderer};
use iced::widget::Space;
use iced::{Element, Event, Length, Rectangle, Size, Vector, mouse, touch};

use crate::core::message::Message;

/// Wraps a base widget and optionally gives an overlay exclusive interaction.
pub fn modal<'a>(
    base: Element<'a, Message>,
    overlay: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let has_overlay = overlay.is_some();
    let overlay =
        overlay.unwrap_or_else(|| Space::new().width(Length::Fill).height(Length::Fill).into());
    Element::new(ModalHost {
        base,
        overlay,
        has_overlay,
    })
}

struct ModalHost<'a> {
    base: Element<'a, Message>,
    overlay: Element<'a, Message>,
    has_overlay: bool,
}

impl Widget<Message, iced::Theme, iced::Renderer> for ModalHost<'_> {
    fn size(&self) -> Size<Length> {
        self.base.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.base
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.base.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            if self.has_overlay {
                mouse::Cursor::Unavailable
            } else {
                cursor
            },
            viewport,
        );
    }

    fn children(&self) -> Vec<Tree> {
        vec![
            Tree::new(self.base.as_widget()),
            Tree::new(self.overlay.as_widget()),
        ]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[self.base.as_widget(), self.overlay.as_widget()]);
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        if self.has_overlay {
            // `UserInterface` operates the overlay separately through this
            // widget's `overlay` method, where its own layout is available.
            return;
        }

        self.base
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if self.has_overlay && takes_input(event) {
            return;
        }

        self.base.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
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
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if self.has_overlay {
            mouse::Interaction::default()
        } else {
            self.base.as_widget().mouse_interaction(
                &tree.children[0],
                layout,
                cursor,
                viewport,
                renderer,
            )
        }
    }

    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'a>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, iced::Theme, iced::Renderer>> {
        if self.has_overlay {
            Some(overlay::Element::new(Box::new(ModalOverlay {
                content: &mut self.overlay,
                tree: &mut tree.children[1],
                bounds: layout.bounds(),
                viewport: *viewport,
                translation,
            })))
        } else {
            self.base.as_widget_mut().overlay(
                &mut tree.children[0],
                layout,
                renderer,
                viewport,
                translation,
            )
        }
    }
}

struct ModalOverlay<'a, 'b> {
    content: &'b mut Element<'a, Message>,
    tree: &'b mut Tree,
    bounds: Rectangle,
    viewport: Rectangle,
    translation: Vector,
}

impl overlay::Overlay<Message, iced::Theme, iced::Renderer> for ModalOverlay<'_, '_> {
    fn layout(&mut self, renderer: &iced::Renderer, _bounds: Size) -> layout::Node {
        let limits = layout::Limits::new(Size::ZERO, self.bounds.size());
        self.content
            .as_widget_mut()
            .layout(self.tree, renderer, &limits)
            .move_to(self.bounds.position() + self.translation)
    }

    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.content.as_widget().draw(
            self.tree,
            renderer,
            theme,
            style,
            layout,
            cursor,
            &self.viewport,
        );
    }

    fn operate(
        &mut self,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(self.tree, layout, renderer, operation);
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        self.content.as_widget_mut().update(
            self.tree,
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            &self.viewport,
        );
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            self.tree,
            layout,
            cursor,
            &self.viewport,
            renderer,
        )
    }
}

fn takes_input(event: &Event) -> bool {
    matches!(
        event,
        Event::Mouse(_)
            | Event::Keyboard(_)
            | Event::Touch(
                touch::Event::FingerPressed { .. }
                    | touch::Event::FingerMoved { .. }
                    | touch::Event::FingerLifted { .. }
                    | touch::Event::FingerLost { .. }
            )
    )
}

#[cfg(test)]
mod tests {
    use iced::widget::{Space, button, column, container};
    use iced::{Element, Length, mouse};
    use iced_test::simulator;

    use super::modal;
    use crate::core::message::Message;

    fn click_text(
        ui: &mut iced_test::Simulator<'_, Message, iced::Theme, iced::Renderer>,
        label: &str,
    ) {
        let point = ui
            .find(label)
            .expect("the widget is present")
            .visible_bounds()
            .expect("the widget is laid out")
            .center();
        ui.point_at(point);
        ui.simulate([
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ]);
    }

    fn base() -> Element<'static, Message> {
        button("base")
            .on_press(Message::ShowToast("base".to_string()))
            .into()
    }

    fn overlay() -> Element<'static, Message> {
        column![
            Space::new().width(Length::Fill).height(Length::Fill),
            button("overlay").on_press(Message::ShowWarningToast("overlay".to_string())),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    #[test]
    fn base_receives_pointer_input_without_a_modal() {
        let mut ui = simulator(modal(base(), None));

        click_text(&mut ui, "base");

        assert!(matches!(
            ui.into_messages().next(),
            Some(Message::ShowToast(message)) if message == "base"
        ));
    }

    #[test]
    fn modal_blocks_pointer_input_to_the_base() {
        let mut ui = simulator(modal(base(), Some(overlay())));

        ui.point_at(iced::Point::new(10.0, 10.0));
        ui.simulate([
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ]);

        let messages: Vec<_> = ui.into_messages().collect();
        assert!(
            !messages
                .iter()
                .any(|message| matches!(message, Message::ShowToast(text) if text == "base")),
            "the modal passed pointer input through to the base: {messages:?}"
        );
    }

    #[test]
    fn modal_still_receives_its_own_pointer_input() {
        let mut ui = simulator(modal(base(), Some(overlay())));

        click_text(&mut ui, "overlay");

        assert!(matches!(
            ui.into_messages().next(),
            Some(Message::ShowWarningToast(message)) if message == "overlay"
        ));
    }

    #[test]
    fn widget_operations_use_the_overlay_layout() {
        let overlay = container(button("overlay").on_press(Message::Noop));
        let mut ui = simulator(modal(Space::new().into(), Some(overlay.into())));

        ui.find("overlay")
            .expect("the overlay is found through its own layout");
    }
}
