// Rust guideline compliant 2026-10-09

use iced::widget::{Id, Space, button, column, row, text, text_input};
use iced::{Background, Border, Color, Element, Length, Theme, mouse};

use crate::core::data::FileEntryType;
use crate::core::i18n::t_static;
use crate::core::message::Message;
use crate::core::theme::ThemePalette;
use crate::core::widgets::{control_padding, input_style_with_palette};
use crate::ui::confirm_dialog::message_confirm;
use crate::ui::message_dialog;
use rust_i18n::t;

use crate::sftp::{
    SftpContextMenu, SftpDeletePending, SftpNewFolderPending, SftpOverwritePending, SftpPane,
    SftpRenamePending,
};

use super::LUCIDE_FONT;

pub fn view_context_menu(menu: &SftpContextMenu, p: ThemePalette) -> Element<'_, Message> {
    const MENU_WIDTH: f32 = 168.0;
    use crate::core::context_menu::ContextMenuAction as Action;

    let pane = menu.pane;
    let directory = menu.directory.clone();
    let focused = menu
        .cursor
        .selected_or_first(crate::sftp::SftpContextMenu::actions())
        .copied();

    crate::ui::overlay::context_menu_frame(
        menu.position,
        move || {
            let upload_download_item = if pane == SftpPane::Local {
                crate::core::widgets::menu_item(
                    lucide_icons::Icon::Upload,
                    t_static(t!("sftp.upload")),
                    Message::Sftp(crate::sftp::message::Message::SftpUploadSelected),
                    false,
                    focused == Some(Action::SftpTransfer),
                    p,
                )
            } else {
                crate::core::widgets::menu_item(
                    lucide_icons::Icon::Download,
                    t_static(t!("sftp.download")),
                    Message::Sftp(crate::sftp::message::Message::SftpDownloadSelected),
                    false,
                    focused == Some(Action::SftpTransfer),
                    p,
                )
            };

            vec![
                upload_download_item,
                crate::core::widgets::menu_item(
                    lucide_icons::Icon::FolderPlus,
                    t_static(t!("sftp.new_folder")),
                    Message::Sftp(crate::sftp::message::Message::SftpNewFolderRequest {
                        pane,
                        directory: directory.clone(),
                    }),
                    false,
                    focused == Some(Action::SftpNewFolder),
                    p,
                ),
                crate::core::widgets::menu_item(
                    lucide_icons::Icon::Pencil,
                    t_static(t!("sftp.rename")),
                    Message::Sftp(crate::sftp::message::Message::SftpRenameRequest),
                    false,
                    focused == Some(Action::SftpRename),
                    p,
                ),
                crate::core::widgets::menu_item(
                    lucide_icons::Icon::Trash2,
                    t_static(t!("dialog.delete")),
                    Message::Sftp(crate::sftp::message::Message::SftpDeleteRequest),
                    true,
                    focused == Some(Action::SftpDelete),
                    p,
                ),
            ]
        },
        MENU_WIDTH,
        Message::Sftp(crate::sftp::message::Message::SftpContextMenuClose),
        p,
    )
}

pub fn view_overwrite_confirm<'a>(
    pending: &'a SftpOverwritePending,
    p: ThemePalette,
    has_queue: bool,
    anim: f32,
) -> Element<'a, Message> {
    let icon_color = p.warning;
    let text_pri = p.text_pri;
    let text_dim = p.text_dim;
    let accent = p.accent;
    let accent_hover = p.accent_hover;
    let ghost_bg = p.button_bg;
    let ghost_hover = p.button_hover;
    let ghost_border = p.button_border;
    let ghost_border_hover = p.button_border_hover;
    let danger = p.danger;

    let is_local_target = matches!(
        pending.target,
        crate::sftp::SftpTransferTarget::Local { .. }
    );
    let is_folder = pending.is_folder;
    let title = if is_folder {
        t!("sftp.overwrite_folder_title").to_string()
    } else {
        t!("sftp.overwrite_file_title").to_string()
    };
    let description = if is_folder {
        if is_local_target {
            t!(
                "sftp.overwrite_folder_body_local",
                name = pending.file_name.as_str()
            )
        } else {
            t!(
                "sftp.overwrite_folder_body_remote",
                name = pending.file_name.as_str()
            )
        }
    } else {
        if is_local_target {
            t!(
                "sftp.overwrite_file_body_local",
                name = pending.file_name.as_str()
            )
        } else {
            t!(
                "sftp.overwrite_file_body_remote",
                name = pending.file_name.as_str()
            )
        }
    };

    let header = row![
        text(char::from(lucide_icons::Icon::AlertTriangle))
            .font(LUCIDE_FONT)
            .size(18)
            .color(icon_color),
        text(title)
            .size(15)
            .color(text_pri)
            .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    // Large remote-side overwrites (≥ the atomic-size threshold) are replaced
    // by delete-then-upload, so an interrupted transfer can lose the original.
    // Make that risk explicit instead of pretending the overwrite is safe.
    let large_remote_overwrite =
        !is_local_target && pending.total_bytes >= crate::ssh::SFTP_ATOMIC_SIZE_THRESHOLD;

    let body: Element<'a, Message> = if large_remote_overwrite {
        let desc = text(description)
            .size(13)
            .color(text_dim)
            .width(Length::Fill)
            .wrapping(text::Wrapping::WordOrGlyph);
        let warn = text(t!(
            "sftp.overwrite_large_warning",
            size = pending.total_bytes / (1024 * 1024)
        ))
        .size(12)
        .color(icon_color)
        .width(Length::Fill)
        .wrapping(text::Wrapping::WordOrGlyph);
        column![desc, warn].spacing(8).into()
    } else {
        text(description)
            .size(13)
            .color(text_dim)
            .width(Length::Fill)
            .wrapping(text::Wrapping::WordOrGlyph)
            .into()
    };

    // Same plain surface as the neutral ghost buttons, but tinted danger:
    // red text + red border, with a faint red wash on hover. It reads as a
    // warning/abort affordance rather than a solid destructive action.
    let stop_btn = button(text(t_static(t!("sftp.stop"))).size(14).color(danger))
        .on_press(Message::Sftp(
            crate::sftp::message::Message::SftpOverwriteCancel,
        ))
        .padding(control_padding(14.0, 14.0))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(Background::Color(if hovered {
                    Color {
                        r: danger.r,
                        g: danger.g,
                        b: danger.b,
                        a: 0.16,
                    }
                } else {
                    ghost_bg
                })),
                text_color: danger,
                border: Border {
                    radius: 6.0.into(),
                    width: 1.0,
                    color: danger,
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default());

    let replace_btn = button(
        text(t_static(t!("sftp.replace")))
            .size(14)
            .color(p.text_on_color),
    )
    .on_press_maybe(if pending.in_progress {
        None
    } else {
        Some(Message::Sftp(
            crate::sftp::message::Message::SftpOverwriteReplace,
        ))
    })
    .padding(control_padding(14.0, 14.0))
    .style(move |_: &Theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        button::Style {
            background: Some(Background::Color(if hovered {
                accent_hover
            } else {
                accent
            })),
            text_color: p.text_on_color,
            border: Border {
                radius: 6.0.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    })
    .cursor_interaction(mouse::Interaction::default());

    let replace_all_btn = button(
        text(t_static(t!("sftp.replace_all")))
            .size(14)
            .color(p.text_on_color),
    )
    .on_press_maybe(if pending.in_progress {
        None
    } else {
        Some(Message::Sftp(
            crate::sftp::message::Message::SftpOverwriteReplaceAll,
        ))
    })
    .padding(control_padding(14.0, 14.0))
    .style(move |_: &Theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        button::Style {
            background: Some(Background::Color(if hovered {
                accent_hover
            } else {
                accent
            })),
            text_color: p.text_on_color,
            border: Border {
                radius: 6.0.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    })
    .cursor_interaction(mouse::Interaction::default());

    let duplicate_btn = button(
        text(t_static(t!("sftp.duplicate")))
            .size(14)
            .color(text_pri),
    )
    .on_press_maybe(if pending.in_progress {
        None
    } else {
        Some(Message::Sftp(
            crate::sftp::message::Message::SftpOverwriteDuplicate,
        ))
    })
    .padding(control_padding(14.0, 14.0))
    .style(move |_: &Theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        button::Style {
            background: Some(Background::Color(if hovered {
                ghost_hover
            } else {
                ghost_bg
            })),
            text_color: text_pri,
            border: Border {
                radius: 6.0.into(),
                width: 1.0,
                color: if hovered {
                    ghost_border_hover
                } else {
                    ghost_border
                },
            },
            ..button::Style::default()
        }
    })
    .cursor_interaction(mouse::Interaction::default());

    let merge_btn = button(text(t_static(t!("sftp.merge"))).size(14).color(text_pri))
        .on_press_maybe(if pending.in_progress {
            None
        } else {
            Some(Message::Sftp(
                crate::sftp::message::Message::SftpOverwriteMerge,
            ))
        })
        .padding(control_padding(14.0, 14.0))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(Background::Color(if hovered {
                    ghost_hover
                } else {
                    ghost_bg
                })),
                text_color: text_pri,
                border: Border {
                    radius: 6.0.into(),
                    width: 1.0,
                    color: if hovered {
                        ghost_border_hover
                    } else {
                        ghost_border
                    },
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default());

    // Actions row: Stop left, action buttons right

    let mut right_btns: Vec<Element<'_, Message>> = Vec::new();
    right_btns.push(replace_btn.into());
    if has_queue {
        right_btns.push(replace_all_btn.into());
    }
    right_btns.push(duplicate_btn.into());
    if pending.is_folder {
        right_btns.push(merge_btn.into());
    }

    let right_group = row(right_btns).spacing(8).align_y(iced::Alignment::Center);

    let actions = row![stop_btn, Space::new().width(Length::Fill), right_group]
        .spacing(0)
        .align_y(iced::Alignment::Center);

    let card_width = if has_queue {
        if pending.is_folder { 620.0 } else { 520.0 }
    } else {
        if pending.is_folder { 520.0 } else { 420.0 }
    };

    message_dialog::message_dialog_frame(
        header.into(),
        body,
        actions.into(),
        card_width,
        Message::Noop,
        &p,
        anim,
    )
}

pub fn view_cancel_transfer_confirm<'a>(
    file_name: &str,
    p: ThemePalette,
    anim: f32,
) -> Element<'a, Message> {
    let body = if file_name.is_empty() {
        t!("sftp.discard_confirm_body_empty").to_string()
    } else {
        t!("sftp.discard_confirm_body", name = file_name).to_string()
    };

    message_confirm(
        t_static(t!("sftp.discard_confirm_title")),
        body,
        t_static(t!("sftp.discard")),
        Message::Sftp(crate::sftp::message::Message::SftpCancelTransfer),
        &Message::Sftp(crate::sftp::message::Message::SftpCancelTransferCancel),
        None,
        p.danger,
        Some(Message::Sftp(
            crate::sftp::message::Message::SftpCancelTransferCancel,
        )),
        &p,
        anim,
    )
}

pub fn view_delete_confirm(
    pending: &SftpDeletePending,
    p: ThemePalette,
    anim: f32,
) -> Element<'_, Message> {
    let count = pending.names.len();
    let title = if count == 1 {
        match pending.file_types.first() {
            Some(FileEntryType::Folder) => t!("sftp.delete_folder_title").to_string(),
            Some(FileEntryType::Symlink) => t!("sftp.delete_symlink_title").to_string(),
            _ => t!("sftp.delete_file_title").to_string(),
        }
    } else {
        t!("sftp.delete_items_title", count = count).to_string()
    };
    let name_display = if count == 1 {
        pending
            .names
            .first()
            .map_or("", std::string::String::as_str)
            .to_string()
    } else {
        // List the first two names, then fold the rest into a total count.
        // zh uses the "等 N 个" idiom where N is the TOTAL (not the hidden
        // remainder), so pass `count` as the placeholder value.
        let shown = pending
            .names
            .iter()
            .take(2)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        if count > 2 {
            format!("{} {}", shown, t!("sftp.delete_items_total", count = count))
        } else {
            shown
        }
    };

    let text_pri = p.text_pri;
    let danger_color = p.danger;

    let header = row![
        text(char::from(lucide_icons::Icon::AlertTriangle))
            .font(LUCIDE_FONT)
            .size(18)
            .color(danger_color),
        text(title)
            .size(15)
            .color(text_pri)
            .width(Length::Fill)
            .wrapping(text::Wrapping::WordOrGlyph),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    let body = text(name_display)
        .size(13)
        .color(p.text_dim)
        .width(Length::Fill)
        .wrapping(text::Wrapping::WordOrGlyph);

    let cancel_btn = {
        let button_bg = p.button_bg;
        let button_hover = p.button_hover;
        button(
            text(t_static(t!("settings.cancel")))
                .size(14)
                .color(text_pri),
        )
        .on_press(Message::Sftp(
            crate::sftp::message::Message::SftpDeleteCancel,
        ))
        .padding(control_padding(14.0, 18.0))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(Background::Color(if hovered {
                    button_hover
                } else {
                    button_bg
                })),
                text_color: text_pri,
                border: Border {
                    radius: 6.0.into(),
                    width: 1.0,
                    color: if hovered {
                        p.button_border_hover
                    } else {
                        p.button_border
                    },
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default())
    };

    let confirm_label = if pending.in_progress {
        t_static(t!("sftp.deleting"))
    } else {
        t_static(t!("dialog.delete"))
    };

    let confirm_content: Element<'_, Message> = if pending.in_progress {
        // Self-animating ring indicator marking that the deletion is in flight.
        let indicator: Element<'static, Message> =
            crate::ui::loading_indicator::loading_indicator(14.0, p.text_on_color).into();
        row![
            indicator,
            text(confirm_label).size(14).color(p.text_on_color),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .into()
    } else {
        text(confirm_label).size(14).color(p.text_on_color).into()
    };

    let mut confirm_btn = button(confirm_content)
        .padding(control_padding(14.0, 18.0))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(Background::Color(if hovered {
                    p.danger_hover
                } else {
                    danger_color
                })),
                text_color: p.text_on_color,
                border: Border {
                    radius: 6.0.into(),
                    ..Border::default()
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default());
    if !pending.in_progress {
        confirm_btn = confirm_btn.on_press(Message::Sftp(
            crate::sftp::message::Message::SftpDeleteConfirm,
        ));
    }

    let footer = row![Space::new().width(Length::Fill), cancel_btn, confirm_btn]
        .spacing(10)
        .align_y(iced::Alignment::Center);

    message_dialog::message_dialog_frame(
        header.into(),
        body.into(),
        footer.into(),
        360.0,
        Message::Noop,
        &p,
        anim,
    )
}

pub fn view_rename_dialog(
    pending: &SftpRenamePending,
    p: ThemePalette,
    anim: f32,
) -> Element<'_, Message> {
    let title = match pending.file_type {
        FileEntryType::Folder => t!("sftp.rename_folder_title"),
        FileEntryType::Symlink => t!("sftp.rename_symlink_title"),
        FileEntryType::File => t!("sftp.rename_file_title"),
    };

    let text_pri = p.text_pri;
    let accent = p.accent;

    // Enter is handled by the global keyboard handler (like every other modal
    // dialog), so there is intentionally no `on_submit` here — adding one would
    // double-fire `SftpRenameConfirm` on Enter.
    let name_input = text_input(t_static(t!("sftp.new_name")), &pending.new_name)
        .id(Id::new("sftp_rename_name"))
        .on_input(|v0| Message::Sftp(crate::sftp::message::Message::SftpRenameNewNameChanged(v0)))
        .padding(control_padding(13.0, 8.0))
        .size(13)
        .style(move |_, s| input_style_with_palette(&p, s));

    let cancel_btn = {
        let button_bg = p.button_bg;
        let button_hover = p.button_hover;
        button(
            text(t_static(t!("settings.cancel")))
                .size(14)
                .color(text_pri),
        )
        .on_press(Message::Sftp(
            crate::sftp::message::Message::SftpRenameCancel,
        ))
        .padding(control_padding(14.0, 18.0))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(Background::Color(if hovered {
                    button_hover
                } else {
                    button_bg
                })),
                text_color: text_pri,
                border: Border {
                    radius: 6.0.into(),
                    width: 1.0,
                    color: if hovered {
                        p.button_border_hover
                    } else {
                        p.button_border
                    },
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default())
    };

    let confirm_label = if pending.in_progress {
        t_static(t!("sftp.renaming"))
    } else {
        t_static(t!("sftp.rename"))
    };

    let confirm_content: Element<'_, Message> = if pending.in_progress {
        // Self-animating ring indicator marking that the rename is in flight.
        let indicator: Element<'static, Message> =
            crate::ui::loading_indicator::loading_indicator(14.0, p.text_on_color).into();
        row![
            indicator,
            text(confirm_label).size(14).color(p.text_on_color),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .into()
    } else {
        text(confirm_label).size(14).color(p.text_on_color).into()
    };

    let mut confirm_btn = button(confirm_content)
        .padding(control_padding(14.0, 18.0))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(Background::Color(if hovered {
                    p.accent_hover
                } else {
                    accent
                })),
                text_color: p.text_on_color,
                border: Border {
                    radius: 6.0.into(),
                    ..Border::default()
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default());
    if !pending.in_progress {
        confirm_btn = confirm_btn.on_press(Message::Sftp(
            crate::sftp::message::Message::SftpRenameConfirm,
        ));
    }

    let actions: Vec<Element<'_, Message>> = vec![cancel_btn.into(), confirm_btn.into()];

    message_dialog::message_dialog(
        title,
        None::<String>,
        name_input.into(),
        actions,
        Message::Noop,
        360.0,
        &p,
        anim,
    )
}

pub fn view_new_folder_dialog(
    pending: &SftpNewFolderPending,
    p: ThemePalette,
    anim: f32,
) -> Element<'_, Message> {
    let text_pri = p.text_pri;
    let accent = p.accent;

    // Enter is handled by the global keyboard handler (like every other modal
    // dialog), so there is intentionally no `on_submit` here — adding one would
    // double-fire `SftpNewFolderConfirm` on Enter.
    let name_input = text_input(t_static(t!("sftp.folder_name")), &pending.name)
        .id(Id::new("sftp_new_folder_name"))
        .on_input(|v0| Message::Sftp(crate::sftp::message::Message::SftpNewFolderNameChanged(v0)))
        .padding(control_padding(13.0, 8.0))
        .size(13)
        .style(move |_, s| input_style_with_palette(&p, s));

    let cancel_btn = {
        let button_bg = p.button_bg;
        let button_hover = p.button_hover;
        button(
            text(t_static(t!("settings.cancel")))
                .size(14)
                .color(text_pri),
        )
        .on_press(Message::Sftp(
            crate::sftp::message::Message::SftpNewFolderCancel,
        ))
        .padding(control_padding(14.0, 18.0))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(Background::Color(if hovered {
                    button_hover
                } else {
                    button_bg
                })),
                text_color: text_pri,
                border: Border {
                    radius: 6.0.into(),
                    width: 1.0,
                    color: if hovered {
                        p.button_border_hover
                    } else {
                        p.button_border
                    },
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default())
    };

    let confirm_label = if pending.in_progress {
        t_static(t!("sftp.creating"))
    } else {
        t_static(t!("sftp.create"))
    };

    let confirm_content: Element<'_, Message> = if pending.in_progress {
        // Self-animating ring indicator marking that folder creation is in flight.
        let indicator: Element<'static, Message> =
            crate::ui::loading_indicator::loading_indicator(14.0, p.text_on_color).into();
        row![
            indicator,
            text(confirm_label).size(14).color(p.text_on_color),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .into()
    } else {
        text(confirm_label).size(14).color(p.text_on_color).into()
    };

    let mut confirm_btn = button(confirm_content)
        .padding(control_padding(14.0, 18.0))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(Background::Color(if hovered {
                    p.accent_hover
                } else {
                    accent
                })),
                text_color: p.text_on_color,
                border: Border {
                    radius: 6.0.into(),
                    ..Border::default()
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default());
    if !pending.in_progress {
        confirm_btn = confirm_btn.on_press(Message::Sftp(
            crate::sftp::message::Message::SftpNewFolderConfirm,
        ));
    }

    let actions: Vec<Element<'_, Message>> = vec![cancel_btn.into(), confirm_btn.into()];

    message_dialog::message_dialog(
        t_static(t!("sftp.new_folder")),
        None::<String>,
        name_input.into(),
        actions,
        Message::Noop,
        360.0,
        &p,
        anim,
    )
}
