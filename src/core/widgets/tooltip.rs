//! Cursor-following tooltip widgets.

// Rust guideline compliant 2026-10-09

use std::time::{Duration, Instant};

use iced::advanced::layout::{self, Layout};
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::tree;
use iced::advanced::widget::{Operation, Tree, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::widget::{container, text};
use iced::window;
use iced::{
    Background, Border, Element, Event, Length, Padding, Point, Rectangle, Renderer, Size, Theme,
    Vector, mouse,
};

use crate::core::message::Message;
use crate::core::theme::ThemePalette;

/// Where the tooltip should appear relative to the hovered element.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
#[allow(dead_code, reason = "reserved tooltip placements")]
pub enum TooltipPosition {
    /// Position at the cursor's bottom-right (existing behaviour).
    #[default]
    FollowCursor,
    /// Position above the content element, centred horizontally.
    Top,
    /// Position to the right of the content element, centred vertically.
    Right,
    /// Position below the content element, centred horizontally.
    Bottom,
    /// Position to the left of the content element, centred vertically.
    Left,
}

/// State machine for the cursor-tracking tooltip.
#[derive(Default)]
struct CursorTooltipState {
    phase: CursorTooltipPhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
enum CursorTooltipPhase {
    #[default]
    Idle,
    /// Cursor has entered the content; waiting for the delay to expire.
    Hovered { at: Instant },
    /// Tooltip is visible and positioned at the given cursor coordinates
    /// (relative to the content element).
    Open { cursor_position: Point },
}

struct CursorTooltipOverlay<'a, 'b> {
    tooltip: &'b mut Element<'a, Message>,
    tree: &'b mut Tree,
    cursor_position: Point,
    content_position: Point,
    content_bounds: Rectangle,
    gap: f32,
    padding: f32,
    snap_within_viewport: bool,
    position: TooltipPosition,
}

impl overlay::Overlay<Message, Theme, Renderer> for CursorTooltipOverlay<'_, '_> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let viewport = Rectangle::with_size(bounds);

        let tooltip_layout = self.tooltip.as_widget_mut().layout(
            self.tree,
            renderer,
            &layout::Limits::new(
                Size::ZERO,
                if self.snap_within_viewport {
                    viewport.size()
                } else {
                    Size::INFINITE
                },
            )
            .shrink(iced::Padding::new(self.padding)),
        );

        let text_bounds = tooltip_layout.bounds();

        let tooltip_w = text_bounds.width + self.padding * 2.0;
        let tooltip_h = text_bounds.height + self.padding * 2.0;

        let content = Rectangle {
            x: self.content_position.x,
            y: self.content_position.y,
            width: self.content_bounds.width,
            height: self.content_bounds.height,
        };

        let mut tooltip_bounds = match self.position {
            TooltipPosition::FollowCursor => {
                let translation = self.content_position - self.content_bounds.position();
                let offset = Vector::new(
                    self.cursor_position.x + self.gap,
                    self.cursor_position.y + self.gap,
                ) + translation;
                Rectangle {
                    x: offset.x - self.padding,
                    y: offset.y - self.padding,
                    width: tooltip_w,
                    height: tooltip_h,
                }
            }
            TooltipPosition::Top => Rectangle {
                x: content.x + content.width / 2.0 - tooltip_w / 2.0,
                y: content.y - self.gap - tooltip_h,
                width: tooltip_w,
                height: tooltip_h,
            },
            TooltipPosition::Right => Rectangle {
                x: content.x + content.width + self.gap,
                y: content.y + content.height / 2.0 - tooltip_h / 2.0,
                width: tooltip_w,
                height: tooltip_h,
            },
            TooltipPosition::Bottom => Rectangle {
                x: content.x + content.width / 2.0 - tooltip_w / 2.0,
                y: content.y + content.height + self.gap,
                width: tooltip_w,
                height: tooltip_h,
            },
            TooltipPosition::Left => Rectangle {
                x: content.x - self.gap - tooltip_w,
                y: content.y + content.height / 2.0 - tooltip_h / 2.0,
                width: tooltip_w,
                height: tooltip_h,
            },
        };

        // Keeps the tooltip inside the window.
        if self.snap_within_viewport {
            if tooltip_bounds.x < viewport.x {
                tooltip_bounds.x = viewport.x;
            } else if viewport.x + viewport.width < tooltip_bounds.x + tooltip_bounds.width {
                tooltip_bounds.x = viewport.x + viewport.width - tooltip_bounds.width;
            }

            if tooltip_bounds.y < viewport.y {
                tooltip_bounds.y = viewport.y;
            } else if viewport.y + viewport.height < tooltip_bounds.y + tooltip_bounds.height {
                tooltip_bounds.y = viewport.y + viewport.height - tooltip_bounds.height;
            }
        }

        layout::Node::with_children(
            tooltip_bounds.size(),
            vec![tooltip_layout.translate(Vector::new(self.padding, self.padding))],
        )
        .translate(Vector::new(tooltip_bounds.x, tooltip_bounds.y))
    }

    fn operate(&mut self, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        // Mirrors the forwarding [`CursorTooltip`] does for its content: without
        // it the tooltip is drawn but stays invisible to widget operations, so
        // nothing — a selector in a UI test, say — can read what it shows.
        if let Some(child_layout) = layout.children().next() {
            self.tooltip
                .as_widget_mut()
                .operate(self.tree, child_layout, renderer, operation);
        }
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        // Draw the tooltip content – the caller has already styled it
        // (background, border, text colour) so we donʼt add another
        // background here.
        // A missing child means the layout is inconsistent; skip the frame
        // rather than panicking inside the render pass.
        let Some(child_layout) = layout.children().next() else {
            return;
        };
        self.tooltip.as_widget().draw(
            self.tree,
            renderer,
            theme,
            style,
            child_layout,
            cursor,
            &Rectangle::with_size(Size::INFINITE),
        );
    }
}

pub struct CursorTooltip<'a> {
    content: Element<'a, Message>,
    tooltip: Element<'a, Message>,
    delay: Duration,
    gap: f32,
    padding: f32,
    snap_within_viewport: bool,
    position: TooltipPosition,
}

// `Element` is not `Debug`, so the two element fields are deliberately left out
// of the rendering rather than borrowing a `Debug` impl iced does not provide.
impl std::fmt::Debug for CursorTooltip<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CursorTooltip")
            .field("delay", &self.delay)
            .field("gap", &self.gap)
            .field("padding", &self.padding)
            .field("snap_within_viewport", &self.snap_within_viewport)
            .field("position", &self.position)
            .finish_non_exhaustive()
    }
}

#[allow(dead_code, reason = "unused constructor constant kept beside `new`")]
impl<'a> CursorTooltip<'a> {
    const DEFAULT_PADDING: f32 = 0.0;

    fn new(
        content: impl Into<Element<'a, Message>>,
        tooltip: impl Into<Element<'a, Message>>,
    ) -> Self {
        CursorTooltip {
            content: content.into(),
            tooltip: tooltip.into(),
            delay: Duration::from_millis(600),
            gap: 4.0,
            padding: Self::DEFAULT_PADDING,
            snap_within_viewport: true,
            position: TooltipPosition::default(),
        }
    }

    /// Set the hover delay before the tooltip appears.
    fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    /// Set the gap between the cursor and the tooltip.
    fn gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    /// Set the padding inside the tooltip.
    fn padding(mut self, padding: f32) -> Self {
        self.padding = padding;
        self
    }

    /// Set where the tooltip appears relative to the hovered element.
    fn position(mut self, position: TooltipPosition) -> Self {
        self.position = position;
        self
    }
}

impl Widget<Message, Theme, Renderer> for CursorTooltip<'_> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content), Tree::new(&self.tooltip)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[self.content.as_widget(), self.tooltip.as_widget()]);
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        // Forward widget operations (focus, is_focused, scroll_into_view, …)
        // to the content so focusable children (e.g. the inline tab-rename
        // text input) can be reached programmatically. Without this, the
        // default no-op swallows every operation targeting the wrapped tab.
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn state(&self) -> tree::State {
        tree::State::new(CursorTooltipState::default())
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<CursorTooltipState>()
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
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
        if let Event::Mouse(_) | Event::Window(window::Event::RedrawRequested(_)) = event {
            let state = tree.state.downcast_mut::<CursorTooltipState>();
            let now = Instant::now();
            let cursor_position = cursor.position_over(layout.bounds());

            match (state.phase, cursor_position) {
                (CursorTooltipPhase::Idle, Some(cursor_position)) => {
                    if self.delay == Duration::ZERO {
                        state.phase = CursorTooltipPhase::Open { cursor_position };
                        shell.invalidate_layout();
                    } else {
                        state.phase = CursorTooltipPhase::Hovered { at: now };
                    }
                    shell.request_redraw_at(now + self.delay);
                }
                (CursorTooltipPhase::Hovered { .. }, None) => {
                    state.phase = CursorTooltipPhase::Idle;
                }
                (CursorTooltipPhase::Hovered { at, .. }, _) if at.elapsed() < self.delay => {
                    shell.request_redraw_at((now + self.delay).checked_sub(at.elapsed()).unwrap());
                }
                (CursorTooltipPhase::Hovered { .. }, Some(cursor_position)) => {
                    state.phase = CursorTooltipPhase::Open { cursor_position };
                    shell.invalidate_layout();
                }
                (CursorTooltipPhase::Open { .. }, None) => {
                    state.phase = CursorTooltipPhase::Idle;
                    shell.invalidate_layout();

                    if !matches!(event, Event::Window(window::Event::RedrawRequested(_)),) {
                        shell.request_redraw();
                    }
                }
                (CursorTooltipPhase::Open { .. }, Some(_)) | (CursorTooltipPhase::Idle, None) => (),
            }
        }

        self.content.as_widget_mut().update(
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
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
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
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
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
        let state = tree.state.downcast_ref::<CursorTooltipState>();

        let mut children = tree.children.iter_mut();

        let content = self.content.as_widget_mut().overlay(
            children.next()?,
            layout,
            renderer,
            viewport,
            translation,
        );

        let tooltip = if let CursorTooltipPhase::Open { cursor_position } = state.phase {
            Some(overlay::Element::new(Box::new(CursorTooltipOverlay {
                tooltip: &mut self.tooltip,
                tree: children.next()?,
                cursor_position,
                content_position: layout.position() + translation,
                content_bounds: layout.bounds(),
                gap: self.gap,
                padding: self.padding,
                snap_within_viewport: self.snap_within_viewport,
                position: self.position,
            })))
        } else {
            None
        };

        (content.is_some() || tooltip.is_some()).then(|| {
            overlay::Group::with_children(content.into_iter().chain(tooltip).collect()).overlay()
        })
    }
}

impl<'a> From<CursorTooltip<'a>> for Element<'a, Message> {
    fn from(tooltip: CursorTooltip<'a>) -> Element<'a, Message> {
        Element::new(tooltip)
    }
}

/// Create a tooltip that follows the cursor and appears at its
/// bottom-right after a 600 ms hover.
///
/// The `tooltip` element should already be styled (background, border,
/// padding).  Use [`cursor_tooltip`] with a pre-built container, or
/// the higher-level [`cursor_tooltip_label`].
pub fn cursor_tooltip<'a>(
    content: impl Into<Element<'a, Message>>,
    tooltip: impl Into<Element<'a, Message>>,
) -> CursorTooltip<'a> {
    CursorTooltip::new(content, tooltip)
}

/// Convenience helper: a cursor-following tooltip showing a single
/// line of text with the standard surface-background styling.
///
/// `max_width` constrains the tooltip to at most one line with an
/// ellipsis when the text exceeds the given width.  Pass `None` to
/// keep the existing unbounded wrapping behaviour.
pub fn cursor_tooltip_label<'a>(
    content: impl Into<Element<'a, Message>>,
    label: &str,
    p: ThemePalette,
    max_width: Option<f32>,
) -> CursorTooltip<'a> {
    cursor_tooltip_label_at(content, label, p, TooltipPosition::default(), max_width)
}

/// Like [`cursor_tooltip_label`] but with an explicit [`TooltipPosition`].
pub fn cursor_tooltip_label_at<'a>(
    content: impl Into<Element<'a, Message>>,
    label: &str,
    p: ThemePalette,
    position: TooltipPosition,
    max_width: Option<f32>,
) -> CursorTooltip<'a> {
    let text_elem: Element<'_, Message> = if let Some(_w) = max_width {
        text(label.to_string())
            .size(14)
            .color(p.text_pri)
            .wrapping(iced::widget::text::Wrapping::None)
            .ellipsis(iced::widget::text::Ellipsis::End)
            .into()
    } else {
        text(label.to_string()).size(14).color(p.text_pri).into()
    };

    let mut styled = container(text_elem)
        .padding(Padding::from([6.0, 10.0]))
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(p.tooltip_bg)),
            text_color: Some(p.text_pri),
            border: Border {
                color: p.border,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        });

    if let Some(w) = max_width {
        styled = styled.max_width(w);
    }

    cursor_tooltip(content, styled).position(position)
}
