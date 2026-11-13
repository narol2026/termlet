//! Dialogs for host export / import (age-encrypted export).
//!
//! Three kinds, one Stack slot (see `views/mod.rs`):
//! - `Export`: passphrase + confirm.
//! - `ImportPassword`: passphrase prompt for an age-encrypted file.
//! - `ImportConfirm`: parsed file summary (new / duplicate counts).

use iced::widget::{Id, column, container, row, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme};
use secrecy::{ExposeSecret, SecretString};

use crate::app::App;
use crate::core::i18n::t_static;
use crate::core::message::Message;
use crate::core::theme::ThemePalette;
use crate::core::widgets::labeled_password_input_with_id;
use crate::ui::confirm_dialog::{cancel_button, confirm_button};
use crate::ui::message_dialog;
use rust_i18n::t;

use super::state::{HostTransferDialog, HostTransferKind};

pub const HOST_IMPORT_PASSWORD_INPUT_ID: &str = "host_import_password_input";
pub const HOST_EXPORT_PASSWORD_INPUT_ID: &str = "host_export_password_input";

pub fn view_host_transfer_dialog(
    dialog: &HostTransferDialog,
    app: &App,
    p: ThemePalette,
) -> Element<'static, Message> {
    let anim = app.confirm_dialog_anim;
    match dialog.kind {
        HostTransferKind::Export => view_export(dialog, app, &p, anim),
        HostTransferKind::ImportPassword => view_import_password(dialog, &p, anim),
        HostTransferKind::ImportConfirm => view_import_confirm(dialog, &p, anim),
    }
}

fn view_export(
    dialog: &HostTransferDialog,
    app: &App,
    p: &ThemePalette,
    anim: f32,
) -> Element<'static, Message> {
    let count = app.hosts.list.len();
    let secure = !dialog.password_visible;

    let input: Element<'static, Message> = if dialog.busy {
        // Encryption is in flight (scrypt KDF ~1s) — swap the password row
        // for the busy indicator instead of stacking both.
        busy_indicator(dialog.anim_t, t_static(t!("hosts.export_encrypting")), p)
    } else {
        container(labeled_password_input_with_id(
            "",
            t_static(t!("hosts.export_password_placeholder")),
            &dialog.password,
            p,
            |s| {
                Message::Hosts(crate::hosts::message::Message::HostExportPasswordChanged(
                    SecretString::from(s),
                ))
            },
            secure,
            Message::Hosts(crate::hosts::message::Message::HostTransferTogglePasswordVisible),
            Some(Id::new(HOST_EXPORT_PASSWORD_INPUT_ID)),
        ))
        .width(Length::Fill)
        .into()
    };

    let mut content = column![input].spacing(10).width(Length::Fill);
    content = push_error(content, dialog.error.as_ref(), p);

    let actions: Vec<Element<'static, Message>> = vec![
        cancel_button(
            Message::Hosts(crate::hosts::message::Message::HostExportClose),
            p,
        ),
        confirm_button(
            t_static(t!("hosts.export_submit")),
            Message::Hosts(crate::hosts::message::Message::HostExportSubmit),
            p.accent,
            Color::WHITE,
            dialog.busy || dialog.password.expose_secret().trim().is_empty(),
        ),
    ];

    message_dialog::message_dialog(
        t_static(t!("hosts.export_dialog_title")),
        Some(t!("hosts.export_body", n = count).to_string()),
        content.into(),
        actions,
        Message::Hosts(crate::hosts::message::Message::HostExportClose),
        400.0,
        p,
        anim,
    )
}

fn view_import_password(
    dialog: &HostTransferDialog,
    p: &ThemePalette,
    anim: f32,
) -> Element<'static, Message> {
    let file_name = dialog
        .pending_path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let secure = !dialog.password_visible;

    let input: Element<'static, Message> = if dialog.busy {
        // Decryption is in flight (scrypt KDF ~1s) — swap the password row
        // for the busy indicator instead of stacking both.
        busy_indicator(dialog.anim_t, t_static(t!("hosts.import_decrypting")), p)
    } else {
        container(labeled_password_input_with_id(
            "",
            t_static(t!("hosts.import_password_placeholder")),
            &dialog.password,
            p,
            |s| {
                Message::Hosts(crate::hosts::message::Message::HostImportPasswordChanged(
                    SecretString::from(s),
                ))
            },
            secure,
            Message::Hosts(crate::hosts::message::Message::HostTransferTogglePasswordVisible),
            Some(Id::new(HOST_IMPORT_PASSWORD_INPUT_ID)),
        ))
        .width(Length::Fill)
        .into()
    };

    let mut content = column![input].spacing(10).width(Length::Fill);
    content = push_error(content, dialog.error.as_ref(), p);

    // The subtitle says "enter its passphrase to decrypt" — hide it while
    // the decryption is actually running.
    let body_text: Option<String> = if dialog.busy {
        None
    } else {
        Some(t!("hosts.import_password_hint", file = file_name).to_string())
    };

    let actions: Vec<Element<'static, Message>> = vec![
        cancel_button(
            Message::Hosts(crate::hosts::message::Message::HostImportClose),
            p,
        ),
        confirm_button(
            t_static(t!("hosts.import_submit")),
            Message::Hosts(crate::hosts::message::Message::HostImportPasswordSubmit),
            p.accent,
            Color::WHITE,
            dialog.busy || dialog.password.expose_secret().trim().is_empty(),
        ),
    ];

    message_dialog::message_dialog(
        t_static(t!("hosts.import_dialog_title")),
        body_text,
        content.into(),
        actions,
        Message::Hosts(crate::hosts::message::Message::HostImportClose),
        400.0,
        p,
        anim,
    )
}

fn view_import_confirm(
    dialog: &HostTransferDialog,
    p: &ThemePalette,
    anim: f32,
) -> Element<'static, Message> {
    let mut content = column![].spacing(10).width(Length::Fill);

    if dialog
        .pending_hosts
        .iter()
        .any(|h| h.auth_type == "Key" && h.key_path.is_none())
    {
        content = content.push(
            text(t_static(t!("hosts.import_confirm_key_warning")))
                .size(11)
                .color(p.warning)
                .width(Length::Fill)
                .align_x(Alignment::Center)
                .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
        );
    }

    let actions: Vec<Element<'static, Message>> = vec![
        cancel_button(
            Message::Hosts(crate::hosts::message::Message::HostImportClose),
            p,
        ),
        confirm_button(
            t_static(t!("hosts.import_confirm_submit")),
            Message::Hosts(crate::hosts::message::Message::HostImportConfirm),
            p.accent,
            Color::WHITE,
            dialog.busy,
        ),
    ];

    message_dialog::message_dialog(
        t_static(t!("hosts.import_confirm_title")),
        Some(
            t!(
                "hosts.import_confirm_body",
                new = dialog.new_count,
                dup = dialog.dup_count
            )
            .to_string(),
        ),
        content.into(),
        actions,
        Message::Hosts(crate::hosts::message::Message::HostImportClose),
        380.0,
        p,
        anim,
    )
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: 1.0,
    }
}

/// Progress bar with a shimmer segment scanning left→right, looping every
/// ~0.7s — same visual as the SSH Connecting state. `anim_t` is driven by
/// `AnimTick` (60fps). The bar stretches to fill the container width; `bar_w`
/// is only a reference width for the shimmer sweep math.
fn busy_indicator(anim_t: f32, label: &'static str, p: &ThemePalette) -> Element<'static, Message> {
    let bar_w = 280.0_f32; // reference width for the shimmer sweep
    let bar_h = 4.0_f32;
    let shimmer_w = 64.0_f32;
    let travel = bar_w + shimmer_w;
    let pos = (anim_t * (travel / 0.7)) % travel - shimmer_w;

    let left_w = pos.clamp(0.0, bar_w);
    let shim_start = pos.max(0.0_f32);
    let shim_end = (pos + shimmer_w).clamp(0.0, bar_w);
    let shim_w = (shim_end - shim_start).max(0.0);
    let right_w = (bar_w - shim_end).max(0.0);

    let track_color = lerp_color(p.surface, p.accent, 0.22);
    let radius = Border {
        radius: 2.0.into(),
        color: Color::TRANSPARENT,
        width: 0.0,
    };

    // Each segment gets a `FillPortion` proportional to its reference width so
    // the row always spans the full container width. `FillPortion(0)` would
    // greedily grab all leftover space in iced's flex layout, so clamp to 1.
    let seg = |w: f32, color: Color| -> Element<'static, Message> {
        container(text(""))
            .width(Length::FillPortion((w.max(0.0).round() as u16).max(1)))
            .height(Length::Fixed(bar_h))
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(color)),
                border: radius,
                ..Default::default()
            })
            .into()
    };

    let bar = row![
        seg(left_w, track_color),
        seg(shim_w, p.accent),
        seg(right_w, track_color),
    ]
    .spacing(0)
    .width(Length::Fill);

    column![bar, text(label).size(12).color(p.text_sec)]
        .spacing(6)
        .align_x(Alignment::Center)
        .width(Length::Fill)
        .padding(Padding::from([16.0, 0.0]))
        .into()
}

/// Return `col` with an inline error line appended when present.
fn push_error(
    col: iced::widget::Column<'static, Message>,
    error: Option<&String>,
    p: &ThemePalette,
) -> iced::widget::Column<'static, Message> {
    match error {
        Some(e) => col.push(
            text(e.clone())
                .size(11)
                .color(p.danger)
                .width(Length::Fill)
                .align_x(Alignment::Center)
                .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
        ),
        None => col,
    }
}
