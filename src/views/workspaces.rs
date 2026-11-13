use iced::widget::{Space, column, container, row, text};
use iced::{Alignment, Border, Element, Font, Length, Theme};

use crate::app::App;
use crate::core::cursor_focus::CursorTarget;
use crate::core::dropdown::{DropdownTarget, TransferMenuAction};
use crate::core::i18n::t_static;
use crate::core::message::Message;
use crate::core::theme::ThemePalette;
use crate::core::widgets::{
    ActionMenuItem, CONTROL_HEIGHT, TooltipPosition, action_menu_dropdown, card_hover_btn,
    card_wrapper_with_padding, control_padding, control_vertical_padding, cursor_tooltip_label_at,
};
use crate::ui::cursor_area::CursorArea;
use rust_i18n::t;
const WORKSPACE_ITEM_HEIGHT: f32 = 30.0;
const WORKSPACE_ITEM_RADIUS: f32 = 8.0;
pub(crate) const WORKSPACE_ROW_GAP: f32 = 8.0;
const WORKSPACE_CARD_PADDING: f32 = 20.0;
const WORKSPACE_CARD_GAP: f32 = 12.0;
/// Distance from the settings scroll origin to the first workspace row.
///
/// This includes the page padding, title, workspace card padding and its
/// header. Keep it in sync with `view_settings` and the card layout above.
pub(crate) const WORKSPACE_LIST_TOP_PADDING: f32 =
    crate::views::settings::SETTINGS_CONTENT_TOP_PADDING
        + crate::views::settings::SETTINGS_HEADER_HEIGHT
        + crate::views::settings::SETTINGS_SECTION_GAP
        + WORKSPACE_CARD_PADDING
        + CONTROL_HEIGHT
        + WORKSPACE_CARD_GAP;

/// Height of one workspace card including its control padding.
pub(crate) fn workspace_row_height() -> f32 {
    WORKSPACE_ITEM_HEIGHT + 2.0 * control_vertical_padding(13.0)
}

const LUCIDE_FONT: Font = Font {
    family: iced::font::Family::Name("lucide"),
    ..Font::DEFAULT
};

pub fn view_workspaces_card(app: &App, p: ThemePalette) -> Element<'_, Message> {
    let state = &app.workspace_manager;

    let title = text(t!("workspace.title").to_string())
        .size(13)
        .color(p.text_pri);
    let header = container(
        row![
            title,
            Space::new().width(Length::Fill),
            view_transfer_dropdown(app, p)
        ]
        .align_y(Alignment::Center),
    )
    .height(Length::Fixed(CONTROL_HEIGHT));

    let mut list_col = column![].spacing(WORKSPACE_ROW_GAP).width(Length::Fill);
    for saved in &state.list {
        let hovered = state.hovered_id.as_deref() == Some(saved.id.as_str());
        let selected = state.cursor.selected_key() == Some(&saved.id);
        list_col = list_col.push(workspace_row(saved, hovered, selected, p));
    }
    if state.list.is_empty() {
        list_col = list_col.push(
            text(t!("workspace.empty").to_string())
                .size(13)
                .color(p.text_dim)
                .width(Length::Fill),
        );
    }

    CursorArea::for_target(
        container(column![header, list_col].spacing(WORKSPACE_CARD_GAP))
            .padding(WORKSPACE_CARD_PADDING)
            .width(Length::Fill)
            .style(move |_: &Theme| container::Style {
                background: None,
                border: Border {
                    radius: 10.0.into(),
                    color: p.border,
                    width: 1.0,
                },
                ..container::Style::default()
            }),
        CursorTarget::Workspaces,
    )
    .into()
}

/// Toolbar dropdown that folds workspace Export / Import into a single control.
/// Rendered through the shared `select_dropdown` in action-menu mode so its
/// trigger and popup rows match the host transfer dropdown.
fn view_transfer_dropdown(app: &App, p: ThemePalette) -> Element<'static, Message> {
    action_menu_dropdown(
        t_static(t!("workspace.transfer_menu_btn")),
        app.active_dropdown == Some(DropdownTarget::WorkspaceTransferMenu),
        app.workspace_manager
            .transfer_menu
            .active_index(TransferMenuAction::all()),
        DropdownTarget::WorkspaceTransferMenu,
        Message::WorkspaceTransferMenuToggle,
        vec![
            ActionMenuItem {
                icon: lucide_icons::Icon::Download,
                label: t_static(t!("workspace.export_btn")).to_string(),
                on_select: Message::WorkspaceExportRequest,
            },
            ActionMenuItem {
                icon: lucide_icons::Icon::Upload,
                label: t_static(t!("workspace.import_btn")).to_string(),
                on_select: Message::WorkspaceImportRequest,
            },
        ],
        p,
    )
}

fn workspace_row(
    saved: &crate::workspace::SavedWorkspace,
    hovered: bool,
    selected: bool,
    p: ThemePalette,
) -> Element<'_, Message> {
    let title = row![
        text(char::from(lucide_icons::Icon::LayoutGrid))
            .font(LUCIDE_FONT)
            .size(13)
            .color(p.text_sec),
        text(saved.name.clone())
            .size(13)
            .color(p.text_pri)
            .width(Length::Fill)
            .ellipsis(text::Ellipsis::End)
            .wrapping(text::Wrapping::None),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .width(Length::Fill);

    let content = row![title, workspace_actions(saved, hovered, p),]
        .spacing(12)
        .align_y(Alignment::Center)
        .height(Length::Fixed(WORKSPACE_ITEM_HEIGHT));

    card_wrapper_with_padding(
        content.into(),
        hovered,
        selected,
        None,
        None,
        WORKSPACE_ITEM_RADIUS,
        true,
        Message::WorkspaceHoverEnter(saved.id.clone()),
        Message::WorkspaceHoverLeave(saved.id.clone()),
        Some(Message::WorkspaceSelect(saved.id.clone())),
        None,
        None,
        control_padding(13.0, 10.0),
        p,
    )
}

fn workspace_actions(
    saved: &crate::workspace::SavedWorkspace,
    hovered: bool,
    p: ThemePalette,
) -> Element<'_, Message> {
    if !hovered {
        return Space::new().width(0).height(0).into();
    }

    row![
        cursor_tooltip_label_at(
            card_hover_btn(
                lucide_icons::Icon::Play,
                Message::WorkspaceApply(saved.id.clone()),
                p,
            ),
            t_static(t!("workspace.apply")),
            p,
            TooltipPosition::Bottom,
            None,
        ),
        cursor_tooltip_label_at(
            card_hover_btn(
                lucide_icons::Icon::Pencil,
                Message::WorkspaceRenameStart(saved.id.clone()),
                p,
            ),
            t_static(t!("workspace.rename")),
            p,
            TooltipPosition::Bottom,
            None,
        ),
        cursor_tooltip_label_at(
            card_hover_btn(
                lucide_icons::Icon::Trash2,
                Message::WorkspaceDeleteRequest(saved.id.clone()),
                p,
            ),
            t_static(t!("workspace.delete")),
            p,
            TooltipPosition::Bottom,
            None,
        ),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .into()
}
