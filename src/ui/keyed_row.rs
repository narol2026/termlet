//! A horizontal row that keeps child state continuity by key.
//!
//! `iced`'s built-in `Row` diffs its children by *index*, so reordering the
//! children reassigns each child's widget state to the wrong slot. That breaks
//! `iced_drop`'s drag state mid-drag (the dragged tab's `Action::Drag` would
//! jump to another tab). [`KeyedRow`] instead reconciles children by `Key`, so
//! each child's state — including an in-flight drag — follows it when the order
//! changes.
//!
//! It also smooths each child's horizontal position toward its flex target, so
//! a reorder (or any other horizontal movement) *slides* children into place
//! instead of teleporting them.
//!
//! The slide offset is baked into the layout node (rather than applied as a
//! draw-time translation), so hit-testing — `mouse_interaction`, `operate`, and
//! event dispatch to children — tracks the interpolated position exactly.

use std::collections::HashMap;

use iced::advanced::layout::{self, Node};
use iced::advanced::mouse::Cursor;
use iced::advanced::overlay;
use iced::advanced::renderer::Style as RendererStyle;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::widget::{Operation, Widget};
use iced::advanced::{Clipboard, Layout, Shell};
use iced::alignment::Vertical;
use iced::time::Instant;
use iced::window;
use iced::{Alignment, Element, Event, Length, Padding, Pixels, Rectangle, Size, Theme, Vector};

use crate::core::message::Message;

/// Time constant (seconds) for the slide. A child closes the gap to its target
/// x exponentially with this time constant, so the motion is frame-rate
/// independent.
const SLIDE_TAU: f32 = 0.05;

/// Pixel distance below which a child is considered settled and snapped.
const SLIDE_SNAP: f32 = 0.5;

/// Upper bound on the per-frame `dt` (seconds) to avoid a large jump after a
/// long pause between updates.
const SLIDE_MAX_DT: f32 = 0.1;

/// A container that distributes its contents horizontally while keeping
/// continuity of each child's state across reordering, keyed by `Key`.
pub struct KeyedRow<'a, Key> {
    spacing: f32,
    padding: Padding,
    width: Length,
    height: Length,
    align: Alignment,
    keys: Vec<Key>,
    children: Vec<Element<'a, Message>>,
}

// `Element` is not `Debug`, so the child elements are left out and only the
// keys and layout numbers are rendered.
impl<Key: std::fmt::Debug> std::fmt::Debug for KeyedRow<'_, Key> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyedRow")
            .field("spacing", &self.spacing)
            .field("padding", &self.padding)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("align", &self.align)
            .field("keys", &self.keys)
            .finish_non_exhaustive()
    }
}

struct KeyAnim {
    /// Flex target x (relative), refreshed by `layout`.
    target_x: f32,
    /// Smoothed (rendered) x (relative), advanced by `update` and baked into
    /// the layout node so hit-testing matches the drawn position.
    rendered_x: f32,
}

struct RowState<Key> {
    keys: Vec<Key>,
    anims: HashMap<Key, KeyAnim>,
    /// When the smoothing was last advanced, for frame-rate-independent decay.
    last_tick: Option<Instant>,
    /// The `RedrawRequested` instant last advanced, so the runtime's layout
    /// re-dispatch (same event, same instant) doesn't advance the slide twice.
    last_frame: Option<Instant>,
}

impl<'a, Key> KeyedRow<'a, Key>
where
    Key: Copy + PartialEq + Eq + std::hash::Hash + 'static,
{
    pub fn new() -> Self {
        Self {
            spacing: 0.0,
            padding: Padding::ZERO,
            width: Length::Shrink,
            height: Length::Shrink,
            align: Alignment::Start,
            keys: Vec::new(),
            children: Vec::new(),
        }
    }

    /// Sets the horizontal spacing _between_ elements.
    pub fn spacing(mut self, amount: impl Into<Pixels>) -> Self {
        self.spacing = amount.into().0;
        self
    }

    pub fn align_y(mut self, align: impl Into<Vertical>) -> Self {
        self.align = Alignment::from(align.into());
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    pub fn push(mut self, key: Key, child: impl Into<Element<'a, Message>>) -> Self {
        let child = child.into();
        let child_size = child.as_widget().size_hint();

        if !child_size.is_void() {
            self.width = self.width.enclose(child_size.width);
            self.height = self.height.enclose(child_size.height);
            self.keys.push(key);
            self.children.push(child);
        }

        self
    }
}

impl<Key> Default for KeyedRow<'_, Key>
where
    Key: Copy + PartialEq + Eq + std::hash::Hash + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<Key> Widget<Message, Theme, iced::Renderer> for KeyedRow<'_, Key>
where
    Key: Copy + PartialEq + Eq + std::hash::Hash + 'static,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<RowState<Key>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(RowState {
            keys: self.keys.clone(),
            anims: HashMap::new(),
            last_tick: None,
            last_frame: None,
        })
    }

    fn children(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        let Tree {
            state, children, ..
        } = tree;
        let state = state.downcast_mut::<RowState<Key>>();

        let old_keys = std::mem::take(&mut state.keys);
        let mut old_pairs: Vec<(Key, Tree)> =
            old_keys.into_iter().zip(std::mem::take(children)).collect();

        // Reconcile children by key: reuse an old child tree for a matching key
        // (preserving its state, e.g. an in-flight drag), create a fresh tree for
        // a new key.
        let mut reordered: Vec<Tree> = Vec::with_capacity(self.children.len());
        for (new_key, new_child) in self.keys.iter().zip(self.children.iter()) {
            let child_tree = old_pairs
                .iter()
                .position(|(k, _)| k == new_key)
                .map_or_else(
                    || Tree::new(new_child.as_widget()),
                    |pos| old_pairs.swap_remove(pos).1,
                );
            reordered.push(child_tree);
        }

        *children = reordered;
        state.keys.clone_from(&self.keys);
        state.anims.retain(|key, _| self.keys.contains(key));

        for (child_tree, new_child) in children.iter_mut().zip(self.children.iter()) {
            child_tree.diff(new_child.as_widget());
        }
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> Node {
        let node = layout::flex::resolve(
            layout::flex::Axis::Horizontal,
            renderer,
            limits,
            self.width,
            self.height,
            self.padding,
            self.spacing,
            self.align,
            &mut self.children,
            &mut tree.children,
        );

        // Record each key's flex target x, then rebuild the node with every
        // child shifted by its current offset (`rendered_x - target_x`) so the
        // animation's visual position is baked into layout. Hit-testing
        // (`operate`, `mouse_interaction`, child `update`, `overlay`) then sees
        // exactly what `draw` paints.
        //
        // `Node` has no `children_mut`, so the flex children can't be translated
        // in place; we clone the per-child nodes instead. The clone is small
        // (one leaf node per tab) and only runs while a slide is in flight.
        let RowState { anims, .. } = tree.state.downcast_mut::<RowState<Key>>();
        anims.retain(|key, _| self.keys.contains(key));

        for (key, child) in self.keys.iter().zip(node.children()) {
            let target_x = child.bounds().x;
            let anim = anims.entry(*key).or_insert_with(|| KeyAnim {
                target_x,
                rendered_x: target_x,
            });
            anim.target_x = target_x;
        }

        let bounds = node.bounds();
        let shifted_children: Vec<Node> = node
            .children()
            .iter()
            .zip(self.keys.iter())
            .map(|(child, key)| {
                let offset = anims
                    .get(key)
                    .map_or(0.0, |anim| anim.rendered_x - anim.target_x);
                child.clone().translate(Vector::new(offset, 0.0))
            })
            .collect();

        Node::with_children(bounds.size(), shifted_children).move_to(bounds.position())
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds());
        operation.traverse(&mut |operation| {
            self.children
                .iter_mut()
                .zip(&mut tree.children)
                .zip(layout.children())
                .for_each(|((child, state), layout)| {
                    child
                        .as_widget_mut()
                        .operate(state, layout, renderer, operation);
                });
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        for ((child, tree), layout) in self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            child.as_widget_mut().update(
                tree, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
        }

        let now = Instant::now();
        let RowState {
            anims,
            last_tick,
            last_frame,
            ..
        } = tree.state.downcast_mut::<RowState<Key>>();

        // Invalidating layout makes the runtime re-dispatch the *same*
        // `RedrawRequested` event (identical `Instant`) so widgets can react to
        // the new layout. Skip that re-dispatch so the slide advances at most
        // once per frame, while still keeping the animation alive.
        if let Event::Window(window::Event::RedrawRequested(instant)) = event {
            if *last_frame == Some(*instant) {
                if anims
                    .values()
                    .any(|anim| (anim.target_x - anim.rendered_x).abs() >= SLIDE_SNAP)
                {
                    shell.request_redraw();
                }
                return;
            }
            *last_frame = Some(*instant);
        }

        // Advance the slide every frame (frame-rate independent) and keep
        // requesting frames while any child is still converging on its target.
        let dt = last_tick
            .map_or(0.016, |last| (now - last).as_secs_f32())
            .min(SLIDE_MAX_DT);
        *last_tick = Some(now);

        let mut animating = false;
        let mut moved = false;
        for anim in anims.values_mut() {
            let delta = anim.target_x - anim.rendered_x;
            if delta.abs() < SLIDE_SNAP {
                if anim.rendered_x != anim.target_x {
                    anim.rendered_x = anim.target_x;
                    moved = true;
                }
            } else {
                let factor = 1.0 - (-dt / SLIDE_TAU).exp();
                anim.rendered_x += delta * factor;
                moved = true;
                animating = true;
            }
        }

        // Any change to `rendered_x` must trigger a relayout: the offset is now
        // baked into the layout node, so without it hit-testing would keep
        // using the stale position.
        if moved {
            shell.invalidate_layout();
        }
        if animating {
            shell.request_redraw();
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
        self.children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((child, tree), layout)| {
                child
                    .as_widget()
                    .mouse_interaction(tree, layout, cursor, viewport, renderer)
            })
            .max()
            .unwrap_or_default()
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &RendererStyle,
        layout: Layout<'_>,
        cursor: Cursor,
        viewport: &Rectangle,
    ) {
        if !layout.bounds().intersects(viewport) {
            return;
        }

        // The slide offset is baked into `layout` (see `layout`), so children
        // are drawn directly at their interpolated positions.
        for ((child, tree), child_layout) in self
            .children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
        {
            if !child_layout.bounds().intersects(viewport) {
                continue;
            }

            child
                .as_widget()
                .draw(tree, renderer, theme, style, child_layout, cursor, viewport);
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
        overlay::from_children(
            &mut self.children,
            tree,
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Key> From<KeyedRow<'a, Key>> for Element<'a, Message>
where
    Key: Copy + PartialEq + Eq + std::hash::Hash + 'static,
{
    fn from(row: KeyedRow<'a, Key>) -> Self {
        Self::new(row)
    }
}
