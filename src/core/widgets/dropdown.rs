//! Anchored dropdown and select-picker widgets.

// Rust guideline compliant 2026-10-09

use iced::advanced::layout::{self, Layout};
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::tree;
use iced::advanced::widget::{Operation, Tree, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::widget::{button, column, container, text, text_input};
use iced::{
    Background, Border, Element, Event, Length, Padding, Rectangle, Renderer, Size, Theme, Vector,
    mouse,
};

use crate::core::dropdown::{DropdownTarget, ROW_GAP};
use crate::core::focus_area::FocusArea;
use crate::core::message::Message;
use crate::core::theme::ThemePalette;

use super::common::LUCIDE_FONT;
use super::inputs::input_style_with_palette;
use super::metrics::{control_padding, text_width_estimate};
use super::scroll::{SCROLLBAR_MARGIN, styled_scrollable};

const DROPDOWN_MAX_HEIGHT: f32 = 300.0;

/// A dimmed, non-interactive placeholder row shown inside a dropdown/picker
/// popup when there is nothing to choose from (no items yet, or a search with
/// no matches). Padded to match a regular option row so it reads as one.
fn dropdown_empty_state(label: &str, p: ThemePalette) -> Element<'_, Message> {
    container(text(label).size(12).color(p.text_dim))
        .width(Length::Fill)
        .padding(Padding::from([8.0, 10.0]))
        .into()
}

// Anchor dropdown
//
// A widget that shows a dropdown overlay anchored below the trigger
// element when `open` is true. The overlay covers the full viewport:
// clicks on the dropdown items are forwarded normally, while clicks
// anywhere else emit `close_msg`.

#[derive(Default)]
struct AnchorDropdownState;

/// The overlay that draws the dropdown below the trigger.
struct AnchorDropdownOverlay<'a, 'b> {
    dropdown: &'b mut Element<'a, Message>,
    tree: &'b mut Tree,
    trigger_bounds: Rectangle,
    close_msg: Message,
    gap: f32,
}

impl overlay::Overlay<Message, Theme, Renderer> for AnchorDropdownOverlay<'_, '_> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let viewport = Rectangle::with_size(bounds);

        let max_w = self.trigger_bounds.width.min(viewport.width - 12.0);
        let dropdown_layout = self.dropdown.as_widget_mut().layout(
            self.tree,
            renderer,
            &layout::Limits::new(Size::ZERO, Size::new(max_w, viewport.height)),
        );

        let dropdown_size = dropdown_layout.bounds().size();

        let mut x = self.trigger_bounds.x;
        let mut y = self.trigger_bounds.y + self.trigger_bounds.height + self.gap;

        if x + dropdown_size.width > viewport.x + viewport.width {
            x = (viewport.x + viewport.width - dropdown_size.width - 6.0).max(6.0);
        }
        x = x.max(6.0);

        if y + dropdown_size.height > viewport.y + viewport.height {
            y = (self.trigger_bounds.y - dropdown_size.height - self.gap).max(6.0);
        }

        layout::Node::with_children(
            viewport.size(),
            vec![dropdown_layout.translate(Vector::new(x, y))],
        )
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        let Some(child_layout) = layout.children().next() else {
            return;
        };
        self.dropdown.as_widget().draw(
            self.tree,
            renderer,
            theme,
            style,
            child_layout,
            cursor,
            &Rectangle::with_size(Size::INFINITE),
        );
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        // A missing child means the layout is inconsistent; drop the event
        // rather than panicking inside the event pass.
        let Some(dropdown_layout) = layout.children().next() else {
            return;
        };
        let viewport = Rectangle::with_size(layout.bounds().size());

        let cursor_over_dropdown = cursor
            .position()
            .is_some_and(|pos| dropdown_layout.bounds().contains(pos));

        self.dropdown.as_widget_mut().update(
            self.tree,
            event,
            dropdown_layout,
            cursor,
            renderer,
            clipboard,
            shell,
            &viewport,
        );

        if !cursor_over_dropdown && matches!(event, Event::Mouse(mouse::Event::ButtonPressed(_))) {
            shell.publish(self.close_msg.clone());
        }

        // Swallow pointer events while the dropdown is open so they don't
        // pass through to the layer beneath the overlay (cards, inputs, rows
        // under the trigger). The event was already forwarded to the dropdown
        // content above, and an outside press was used to close the menu.
        if matches!(event, Event::Mouse(_)) {
            shell.capture_event();
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        if let Some(dropdown_layout) = layout.children().next()
            && cursor
                .position()
                .is_some_and(|pos| dropdown_layout.bounds().contains(pos))
        {
            return self.dropdown.as_widget().mouse_interaction(
                self.tree,
                dropdown_layout,
                cursor,
                &Rectangle::with_size(dropdown_layout.bounds().size()),
                renderer,
            );
        }
        mouse::Interaction::Idle
    }
}

pub struct AnchorDropdown<'a> {
    trigger: Element<'a, Message>,
    dropdown: Element<'a, Message>,
    open: bool,
    close_msg: Option<Message>,
    gap: f32,
}

// As with `CursorTooltip`: `Element` has no `Debug`, so only the plain fields
// are rendered.
impl std::fmt::Debug for AnchorDropdown<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnchorDropdown")
            .field("open", &self.open)
            .field("close_msg", &self.close_msg)
            .field("gap", &self.gap)
            .finish_non_exhaustive()
    }
}

impl<'a> AnchorDropdown<'a> {
    pub fn new(
        trigger: impl Into<Element<'a, Message>>,
        dropdown: impl Into<Element<'a, Message>>,
    ) -> Self {
        AnchorDropdown {
            trigger: trigger.into(),
            dropdown: dropdown.into(),
            open: false,
            close_msg: None,
            gap: 4.0,
        }
    }

    /// Set whether the dropdown is open.
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Set the message to publish when clicking outside the dropdown.
    pub fn close_msg(mut self, msg: Message) -> Self {
        self.close_msg = Some(msg);
        self
    }

    /// Set the gap between the trigger and the dropdown.
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }
}

impl Widget<Message, Theme, Renderer> for AnchorDropdown<'_> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.trigger), Tree::new(&self.dropdown)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[self.trigger.as_widget(), self.dropdown.as_widget()]);
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        // Mirror the trigger-only forwarding of update/draw so widget
        // operations reach the trigger content (dropdown content is operated
        // through the overlay, like events).
        self.trigger
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn state(&self) -> tree::State {
        tree::State::new(AnchorDropdownState)
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<AnchorDropdownState>()
    }

    fn size(&self) -> Size<Length> {
        self.trigger.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.trigger.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.trigger
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
        self.trigger.as_widget_mut().update(
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
        self.trigger.as_widget().mouse_interaction(
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
        self.trigger.as_widget().draw(
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
        let mut children = tree.children.iter_mut();

        let trigger_overlay = self.trigger.as_widget_mut().overlay(
            children.next()?,
            layout,
            renderer,
            viewport,
            translation,
        );

        let dropdown_overlay = if self.open {
            // Take the dropdown's tree node out here: a `?` inside the `map`
            // closure below would return from the closure, not this function.
            let dropdown_tree = children.next()?;
            self.close_msg.clone().map(|close_msg| {
                overlay::Element::new(Box::new(AnchorDropdownOverlay {
                    dropdown: &mut self.dropdown,
                    tree: dropdown_tree,
                    trigger_bounds: Rectangle {
                        x: layout.position().x + translation.x,
                        y: layout.position().y + translation.y,
                        width: layout.bounds().width,
                        height: layout.bounds().height,
                    },
                    close_msg,
                    gap: self.gap,
                }))
            })
        } else {
            None
        };

        (trigger_overlay.is_some() || dropdown_overlay.is_some()).then(|| {
            overlay::Group::with_children(
                trigger_overlay
                    .into_iter()
                    .chain(dropdown_overlay)
                    .collect(),
            )
            .overlay()
        })
    }
}

impl<'a> From<AnchorDropdown<'a>> for Element<'a, Message> {
    fn from(widget: AnchorDropdown<'a>) -> Element<'a, Message> {
        Element::new(widget)
    }
}

/// Create an anchor dropdown that positions its popup below the trigger.
pub fn anchor_dropdown<'a>(
    trigger: impl Into<Element<'a, Message>>,
    dropdown: impl Into<Element<'a, Message>>,
) -> AnchorDropdown<'a> {
    AnchorDropdown::new(trigger, dropdown)
}

/// Search configuration for a [`select_dropdown`] that filters its options.
pub struct DropdownSearch<'a> {
    /// Placeholder shown in the (empty) filter box.
    pub placeholder: &'a str,
    /// Current filter text. Live state owned by the caller, so the option list
    /// can be filtered on every frame.
    pub query: &'a str,
    /// Constructor for the message published on every keystroke in the filter
    /// box (the search input carries the typed text, so this must take a
    /// `String` — pass the enum's tuple variant, e.g. `Message::HostSearch`).
    pub on_input: fn(String) -> Message,
}

pub struct SelectOption {
    pub label: String,
    /// Optional secondary line rendered under the label (e.g. `user@host:port`
    /// for hosts). `None` renders a single-line row.
    pub subtitle: Option<String>,
    /// Whether the committed value is this option.
    pub active: bool,
    /// Whether keyboard focus is on this option.
    pub focused: bool,
    pub on_select: Message,
    /// Optional leading element shown before the label (e.g. a theme preview).
    pub leading: Option<Element<'static, Message>>,
}

/// Visual/behavioral variant for [`select_dropdown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectDropdownVariant {
    /// A value picker: the active option shows a check mark and the trigger's
    /// chevron stays neutral.
    #[default]
    Select,
    /// A menu of actions: no check column, and the chevron highlights with
    /// `p.accent` while the menu is open.
    ActionMenu,
}

/// Build a themed single-select dropdown: a trigger button showing the current
/// value plus a chevron, with a popup of [`SelectOption`] rows (check mark on
/// the active one). This is the shared style used by the settings theme and
/// language pickers and the port-forward "Via Host" / host-form Identity form
/// pickers.
///
/// `trigger_width` controls the trigger button width (and therefore the popup
/// width, which is anchored to the trigger). Settings passes [`Length::Fill`].
///
/// `search` optionally adds a filter `text_input` above the list (its `query`
/// is read live and the caller is expected to pre-filter `options` with it);
/// `empty_state` is a dimmed hint shown when `options` is empty. Settings
/// passes `None` for both.
#[allow(
    clippy::too_many_arguments,
    reason = "the parameter list mirrors the layout this widget builds"
)]
pub fn select_dropdown<'a>(
    current_label: impl Into<String>,
    leading: Option<Element<'static, Message>>,
    options: Vec<SelectOption>,
    open: bool,
    target: DropdownTarget,
    toggle_msg: Message,
    trigger_width: Length,
    search: Option<DropdownSearch<'a>>,
    empty_state: Option<&'a str>,
    p: ThemePalette,
) -> Element<'a, Message> {
    select_dropdown_with_variant(
        current_label,
        leading,
        options,
        open,
        target,
        toggle_msg,
        trigger_width,
        search,
        empty_state,
        SelectDropdownVariant::Select,
        p,
    )
}

/// Shared implementation of [`select_dropdown`] with an explicit visual
/// variant. Kept private: callers use [`select_dropdown`] for the common
/// picker case or [`action_menu_dropdown`] for action menus.
#[allow(
    clippy::too_many_arguments,
    reason = "the parameter list mirrors the layout this widget builds"
)]
fn select_dropdown_with_variant<'a>(
    current_label: impl Into<String>,
    leading: Option<Element<'static, Message>>,
    options: Vec<SelectOption>,
    open: bool,
    target: DropdownTarget,
    toggle_msg: Message,
    trigger_width: Length,
    search: Option<DropdownSearch<'a>>,
    empty_state: Option<&'a str>,
    variant: SelectDropdownVariant,
    p: ThemePalette,
) -> Element<'a, Message> {
    let current_label: String = current_label.into();
    let mut trigger_children: Vec<Element<'_, Message>> = Vec::with_capacity(4);
    if let Some(leading) = leading {
        trigger_children.push(leading);
    }
    trigger_children.push(
        text(current_label)
            .size(13)
            .color(p.text_pri)
            .width(Length::Fill)
            .ellipsis(text::Ellipsis::End)
            .wrapping(text::Wrapping::None)
            .into(),
    );
    trigger_children.push(
        text(char::from(lucide_icons::Icon::ChevronDown))
            .font(LUCIDE_FONT)
            .size(12)
            .color(if variant == SelectDropdownVariant::ActionMenu && open {
                p.accent
            } else {
                p.text_sec
            })
            .into(),
    );

    let trigger = button(
        iced::widget::Row::with_children(trigger_children)
            .spacing(6)
            .align_y(iced::Alignment::Center),
    )
    .on_press(toggle_msg.clone())
    .padding(control_padding(13.0, 10.0))
    .width(trigger_width)
    .style(move |_: &Theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        button::Style {
            background: Some(Background::Color(if hovered || open {
                p.button_hover
            } else {
                p.button_bg
            })),
            text_color: p.text_pri,
            border: Border {
                radius: 5.0.into(),
                color: if open {
                    p.accent
                } else if hovered {
                    p.button_border_hover
                } else {
                    p.button_border
                },
                width: 1.0,
            },
            ..button::Style::default()
        }
    })
    .cursor_interaction(mouse::Interaction::default());

    let trigger = FocusArea::new(trigger, iced::widget::Id::new(target.id()));
    let dropdown_content = select_dropdown_popup(options, search, empty_state, variant, target, p);

    anchor_dropdown(trigger, dropdown_content)
        .open(open)
        .close_msg(toggle_msg)
        .gap(4.0)
        .into()
}

/// A single row in an action-menu dropdown (a command rather than a selected
/// value).
pub struct ActionMenuItem {
    pub icon: lucide_icons::Icon,
    pub label: String,
    pub on_select: Message,
}

/// Build an action-menu dropdown through the shared [`select_dropdown`] in
/// [`SelectDropdownVariant::ActionMenu`] mode: no check column, an accent
/// chevron while open, and a trigger sized to its label. Shared by the host
/// and workspace Export/Import toolbars.
pub fn action_menu_dropdown(
    label: &'static str,
    open: bool,
    focused_index: Option<usize>,
    target: DropdownTarget,
    toggle_msg: Message,
    items: Vec<ActionMenuItem>,
    p: ThemePalette,
) -> Element<'static, Message> {
    // Size the trigger to its label so the chevron is right-aligned and the
    // anchored popup is wide enough for the action rows.
    const CHROME: f32 = 48.0;
    let trigger_width = Length::Fixed((text_width_estimate(label, 7.0) + CHROME).min(240.0));

    let options = items
        .into_iter()
        .enumerate()
        .map(|(index, item)| SelectOption {
            label: item.label,
            subtitle: None,
            active: false,
            focused: focused_index == Some(index),
            on_select: item.on_select,
            leading: Some(
                text(char::from(item.icon))
                    .font(LUCIDE_FONT)
                    .size(13)
                    .color(p.text_sec)
                    .into(),
            ),
        })
        .collect();

    select_dropdown_with_variant(
        label,
        None,
        options,
        open,
        target,
        toggle_msg,
        trigger_width,
        None,
        None,
        SelectDropdownVariant::ActionMenu,
        p,
    )
}

/// Assemble a dropdown popup: an optional search box above the scrollable
/// option list (or an empty-state hint), wrapped in the shared menu container.
fn select_dropdown_popup<'a>(
    options: Vec<SelectOption>,
    search: Option<DropdownSearch<'a>>,
    empty_state: Option<&'a str>,
    variant: SelectDropdownVariant,
    target: DropdownTarget,
    p: ThemePalette,
) -> Element<'a, Message> {
    let mut children: Vec<Element<'_, Message>> = Vec::with_capacity(2);

    if let Some(search) = search {
        let DropdownSearch {
            placeholder,
            query,
            on_input,
        } = search;
        children.push(
            text_input(placeholder, query)
                .id(iced::widget::Id::new(target.input_id()))
                .on_input(on_input)
                .padding(8)
                .size(12)
                .style(move |_: &Theme, s| input_style_with_palette(&p, s))
                .into(),
        );
    }

    if options.is_empty() {
        if let Some(label) = empty_state {
            children.push(dropdown_empty_state(label, p));
        }
    } else {
        children.push(select_dropdown_list(options, variant, target, p));
    }

    dropdown_menu_container(column(children).spacing(6), p)
}

/// The dropdown's option-list body: render the rows, cap them at
/// [`DROPDOWN_MAX_HEIGHT`], and show a scrollbar once they overflow.
fn select_dropdown_list(
    options: Vec<SelectOption>,
    variant: SelectDropdownVariant,
    target: DropdownTarget,
    p: ThemePalette,
) -> Element<'static, Message> {
    container(
        styled_scrollable(
            column(
                options
                    .into_iter()
                    .map(|o| select_option_row(o, variant, p)),
            )
            .spacing(ROW_GAP),
            p,
            true,
        )
        .id(iced::widget::Id::new(target.scroll_id()))
        .on_scroll(move |viewport| Message::Scrolled(target.scroll_id(), viewport))
        // Embed the scrollbar instead of letting it float over the option
        // rows. Without this, a visible scrollbar overlaps the right edge of
        // the options. `spacing` reserves layout space only while the bar is
        // actually visible.
        .spacing(SCROLLBAR_MARGIN),
    )
    .width(Length::Fill)
    .max_height(DROPDOWN_MAX_HEIGHT)
    .into()
}

/// Shared styled container for a dropdown popup body (e.g. search box + list).
fn dropdown_menu_container<'a>(
    content: impl Into<Element<'a, Message>>,
    p: ThemePalette,
) -> Element<'a, Message> {
    container(content)
        .padding(Padding::from([8.0, 8.0]))
        .width(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(p.content_bg)),
            border: Border {
                radius: 6.0.into(),
                color: p.border,
                width: 1.0,
            },
            ..container::Style::default()
        })
        .into()
}

fn select_option_row(
    option: SelectOption,
    variant: SelectDropdownVariant,
    p: ThemePalette,
) -> Element<'static, Message> {
    let SelectOption {
        label,
        subtitle,
        active,
        focused,
        on_select,
        leading,
    } = option;

    let title = text(label)
        .size(13)
        .color(if active || focused {
            p.text_pri
        } else {
            p.text_sec
        })
        .width(Length::Fill)
        .ellipsis(text::Ellipsis::End)
        .wrapping(text::Wrapping::None);

    // Single-line rows (the common case) skip the Column + heap Vec entirely;
    // only a subtitle wraps the label in a two-line column.
    let texts: Element<'static, Message> = if let Some(subtitle) = subtitle {
        iced::widget::Column::with_children(vec![
            title.into(),
            text(subtitle)
                .size(12)
                .color(p.text_dim)
                .width(Length::Fill)
                .ellipsis(text::Ellipsis::End)
                .wrapping(text::Wrapping::None)
                .into(),
        ])
        .spacing(1)
        .width(Length::Fill)
        .into()
    } else {
        title.into()
    };

    let mut content_children: Vec<Element<'static, Message>> = Vec::with_capacity(3);
    if let Some(leading) = leading {
        content_children.push(leading);
    }
    content_children.push(texts);
    if variant == SelectDropdownVariant::Select {
        let check = if active {
            text(char::from(lucide_icons::Icon::Check))
                .font(LUCIDE_FONT)
                .size(14)
                .color(p.accent)
        } else {
            text("").size(14)
        };
        content_children.push(check.into());
    }

    let row_content = iced::widget::Row::with_children(content_children)
        .align_y(iced::alignment::Vertical::Center)
        .spacing(6)
        .width(Length::Fill);

    let btn = button(row_content)
        .on_press(on_select)
        .width(Length::Fill)
        .padding(Padding::from([8.0, 12.0]))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            let bg = if focused {
                p.item_active
            } else if hovered {
                p.item_hover
            } else {
                p.surface
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: if focused {
                    Border {
                        color: p.accent,
                        width: 1.0,
                        radius: 8.0.into(),
                    }
                } else if p.show_border {
                    Border {
                        color: p.border,
                        width: 1.0,
                        radius: 8.0.into(),
                    }
                } else {
                    Border {
                        radius: 8.0.into(),
                        ..Border::default()
                    }
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default());

    container(btn).into()
}
