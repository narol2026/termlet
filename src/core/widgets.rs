//! Reusable widget primitives shared by application screens.

// Rust guideline compliant 2026-10-09

#![allow(
    unused_imports,
    reason = "the facade preserves the widget API used by application screens"
)]

mod buttons;
mod cards;
mod collection;
mod common;
mod dropdown;
mod forms;
mod inputs;
mod menu;
mod metrics;
mod scroll;
mod theme_preview;
mod tooltip;

#[doc(inline)]
pub use buttons::{
    secondary_btn, segment_btn, styled_btn, styled_btn_with_padding, toolbar_icon_btn,
    toolbar_outline_btn, toolbar_primary_btn,
};
#[doc(inline)]
pub use cards::{
    card_hover_btn, card_info, card_wrapper, card_wrapper_with_padding, icon_box, tag_chip,
    tag_select_option,
};
#[doc(inline)]
pub use collection::{CollectionLayout, collection_layout, collection_rows};
#[doc(inline)]
pub use dropdown::{
    ActionMenuItem, AnchorDropdown, DropdownSearch, SelectDropdownVariant, SelectOption,
    action_menu_dropdown, anchor_dropdown, select_dropdown,
};
#[doc(inline)]
pub use forms::{
    form_actions, form_actions_with_save, form_close_btn, form_header, form_panel,
    labeled_readonly, labeled_readonly_mono,
};
#[doc(inline)]
pub use inputs::{
    filter_input, input_style_with_palette, labeled_input, labeled_password_input,
    labeled_password_input_with_id, password_input_row, search_input,
};
#[doc(inline)]
pub use menu::menu_item;
#[doc(inline)]
pub use metrics::{
    CONTROL_HEIGHT, char_cells, control_padding, control_vertical_padding, text_width_estimate,
};
#[doc(inline)]
pub use scroll::{SCROLLBAR_HOVERED, scrollable_style, styled_scrollable};
#[doc(inline)]
pub use theme_preview::theme_preview_icon;
#[doc(inline)]
pub use tooltip::{
    CursorTooltip, TooltipPosition, cursor_tooltip, cursor_tooltip_label, cursor_tooltip_label_at,
};
