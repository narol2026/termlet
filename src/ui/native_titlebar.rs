//! Widget wrappers that publish geometry to the Windows title-bar hit tester.
//!
//! On other platforms these helpers return the content unchanged, so the view
//! code has no platform branches around individual controls.

use iced::Element;

use crate::core::message::Message;

#[derive(Debug, Clone, Copy)]
enum Zone {
    Caption,
    Client,
    Maximize,
    ScrolledClient,
    TabStripClip,
}

/// Wrap the complete top bar as the native caption area.
pub fn caption_zone(content: Element<'_, Message>) -> Element<'_, Message> {
    wrap(content, Zone::Caption)
}

/// Keep a control in the client area so it receives normal mouse input.
pub fn client_zone(content: Element<'_, Message>) -> Element<'_, Message> {
    wrap(content, Zone::Client)
}

/// Expose the maximize/restore button as the native `HTMAXBUTTON` zone so
/// Windows 11 can show Snap Layouts when the pointer hovers over it.
pub fn maximize_zone(content: Element<'_, Message>) -> Element<'_, Message> {
    wrap(content, Zone::Maximize)
}

/// Keep a tab inside the horizontally scrolled strip in the client area.
pub fn scrolled_client_zone(content: Element<'_, Message>) -> Element<'_, Message> {
    wrap(content, Zone::ScrolledClient)
}

/// Mark the visible tab-strip viewport; later scrolled tabs clip to it.
pub fn tab_strip_clip(content: Element<'_, Message>) -> Element<'_, Message> {
    wrap(content, Zone::TabStripClip)
}

#[cfg(not(target_os = "windows"))]
fn wrap(content: Element<'_, Message>, _zone: Zone) -> Element<'_, Message> {
    content
}

#[cfg(target_os = "windows")]
fn wrap(content: Element<'_, Message>, zone: Zone) -> Element<'_, Message> {
    Element::new(NativeZone { content, zone })
}

#[cfg(target_os = "windows")]
struct NativeZone<'a> {
    content: Element<'a, Message>,
    zone: Zone,
}

#[cfg(target_os = "windows")]
impl iced::advanced::widget::Widget<Message, iced::Theme, iced::Renderer> for NativeZone<'_> {
    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> iced::advanced::widget::tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<iced::advanced::widget::Tree> {
        self.content.as_widget().children()
    }

    fn diff(&self, tree: &mut iced::advanced::widget::Tree) {
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
        tree: &mut iced::advanced::widget::Tree,
        renderer: &iced::Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }

    fn draw(
        &self,
        tree: &iced::advanced::widget::Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &iced::advanced::renderer::Style,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::mouse::Cursor,
        viewport: &iced::Rectangle,
    ) {
        let bounds = layout.bounds();
        match self.zone {
            Zone::Caption => crate::core::window_chrome::record_caption(bounds),
            Zone::Client => crate::core::window_chrome::record_client(bounds),
            Zone::Maximize => crate::core::window_chrome::record_maximize(bounds),
            Zone::ScrolledClient => {
                // The scrollable translates its content while drawing. The
                // `viewport` it passes to the content carries that same
                // translation, so derive the real offset from it instead of
                // trusting a cached value that may have been clamped away.
                crate::core::window_chrome::record_scrolled_client(bounds, *viewport);
            }
            Zone::TabStripClip => crate::core::window_chrome::record_tab_strip_clip(bounds),
        }

        self.content
            .as_widget()
            .draw(tree, renderer, theme, style, layout, cursor, viewport);

        if matches!(self.zone, Zone::Caption) {
            crate::core::window_chrome::commit_frame();
        }
    }

    fn mouse_interaction(
        &self,
        tree: &iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::mouse::Cursor,
        viewport: &iced::Rectangle,
        renderer: &iced::Renderer,
    ) -> iced::mouse::Interaction {
        self.content
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn operate(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(tree, layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        event: &iced::Event,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut iced::advanced::Shell<'_, Message>,
        viewport: &iced::Rectangle,
    ) {
        self.content.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &iced::Rectangle,
        translation: iced::Vector,
    ) -> Option<iced::advanced::overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation)
    }
}
