//! Shared host-key confirmation prompt widgets.
//!
//! SSH, SFTP, and port forwarding use the same key details and trust actions,
//! while each feature keeps ownership of its connection lifecycle.

// Rust guideline compliant 2026-10-09

use iced::widget::{Space, column, container, row, text};
use iced::{Background, Border, Element, Font, Length, Theme};

use crate::core::data::ServerKeyInfo;
use crate::core::i18n::t_static;
use crate::core::message::Message;
use crate::core::theme::ThemePalette;
use crate::ui::confirm_dialog::{accent_outline_button, cancel_button_with_label, confirm_button};
use crate::ui::message_dialog::message_dialog_frame;
use rust_i18n::t;

const LUCIDE_FONT: Font = Font {
    family: iced::font::Family::Name("lucide"),
    ..Font::DEFAULT
};

/// Horizontal and vertical spacing inside the host-key prompt.
const PROMPT_GAP: f32 = 12.0;
/// Gap between the prompt header and its key details.
const HEADER_GAP: f32 = 8.0;
/// Gap between a detail label and its value.
const FIELD_GAP: f32 = 2.0;
/// Padding used by both the prompt card and its nested detail card.
const CARD_PADDING: f32 = 12.0;
/// Detail-card corner radius, matching the surrounding connection panels.
const DETAIL_RADIUS: f32 = 6.0;
/// Prompt-card corner radius, matching the surrounding connection panels.
const PROMPT_RADIUS: f32 = 8.0;
/// Width used by the port-forward host-key modal.
const DIALOG_WIDTH: f32 = 520.0;

#[derive(Debug, Clone)]
pub(crate) struct TrustActions {
    pub cancel_label: String,
    pub cancel: Message,
    pub continue_message: Message,
    pub add_message: Message,
}

impl TrustActions {
    pub(crate) fn new(
        cancel_label: impl Into<String>,
        cancel: Message,
        continue_message: Message,
        add_message: Message,
    ) -> Self {
        Self {
            cancel_label: cancel_label.into(),
            cancel,
            continue_message,
            add_message,
        }
    }
}

/// Renders the shared inline host-key confirmation panel.
///
/// `context` names the feature that requested trust, such as a port-forward
/// name. SSH and SFTP pass `None`.
pub(crate) fn panel<'a>(
    key_info: &'a ServerKeyInfo,
    context: Option<&'a str>,
    p: ThemePalette,
) -> Element<'a, Message> {
    let content = column![header(p), body(key_info, context, p)].spacing(HEADER_GAP);
    let background = p.card_bg;
    let border = p.border;

    container(content)
        .padding(CARD_PADDING)
        .width(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(background)),
            border: Border {
                radius: PROMPT_RADIUS.into(),
                color: border,
                width: 1.0,
            },
            ..Default::default()
        })
        .into()
}

/// Renders the shared host-key confirmation modal.
pub(crate) fn dialog<'a>(
    key_info: &'a ServerKeyInfo,
    context: Option<&'a str>,
    actions: TrustActions,
    p: ThemePalette,
    anim: f32,
) -> Element<'a, Message> {
    let backdrop_message = actions.cancel.clone();
    let actions = action_row(actions, p);
    message_dialog_frame(
        header(p),
        body(key_info, context, p),
        actions,
        DIALOG_WIDTH,
        backdrop_message,
        &p,
        anim,
    )
}

/// Renders the shared Cancel, Continue, and Add and Continue actions.
pub(crate) fn action_row(actions: TrustActions, p: ThemePalette) -> Element<'static, Message> {
    let trust_actions = row![
        accent_outline_button(&t!("ssh.continue"), actions.continue_message, &p),
        confirm_button(
            &t!("ssh.add_and_continue"),
            actions.add_message,
            p.accent,
            iced::Color::WHITE,
            false,
        ),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    row![
        cancel_button_with_label(&actions.cancel_label, actions.cancel, &p),
        Space::new().width(Length::Fill),
        trust_actions,
    ]
    .width(Length::Fill)
    .align_y(iced::Alignment::Center)
    .into()
}

fn header(p: ThemePalette) -> Element<'static, Message> {
    row![
        container(
            text(char::from(lucide_icons::Icon::ShieldQuestion))
                .font(LUCIDE_FONT)
                .size(14)
                .color(p.accent),
        )
        .width(16)
        .center_x(16),
        text(t_static(t!("ssh.trust_host")))
            .size(14)
            .color(p.text_pri)
            .width(Length::Fill),
    ]
    .spacing(HEADER_GAP)
    .align_y(iced::Alignment::Center)
    .into()
}

fn body<'a>(
    key_info: &'a ServerKeyInfo,
    context: Option<&'a str>,
    p: ThemePalette,
) -> Element<'a, Message> {
    let explanation = text(t_static(t!("ssh_connect.host_key_explanation")))
        .size(12)
        .color(p.text_sec)
        .width(Length::Fill)
        .wrapping(text::Wrapping::WordOrGlyph);

    let mut details = column![].spacing(PROMPT_GAP).width(Length::Fill);
    if let Some(context) = context {
        details = details.push(
            text(context)
                .size(13)
                .color(p.text_pri)
                .width(Length::Fill)
                .wrapping(text::Wrapping::WordOrGlyph),
        );
    }
    details = details
        .push(field(
            t_static(t!("ssh.host_label")),
            format!("{}:{}", key_info.hostname, key_info.port),
            p,
        ))
        .push(field(
            t_static(t!("ssh.key_type")),
            key_info.key_type.clone(),
            p,
        ))
        .push(field(
            t_static(t!("ssh.fingerprint")),
            key_info.fingerprint.clone(),
            p,
        ));

    let surface = p.surface;
    let border = p.border;
    let details = container(details)
        .padding(CARD_PADDING)
        .width(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(surface)),
            border: Border {
                radius: DETAIL_RADIUS.into(),
                color: border,
                width: 1.0,
            },
            ..Default::default()
        });

    column![explanation, details]
        .spacing(PROMPT_GAP)
        .width(Length::Fill)
        .into()
}

fn field<'a>(label: &'static str, value: String, p: ThemePalette) -> Element<'a, Message> {
    column![
        text(label).size(12).color(p.text_pri),
        text(value)
            .size(12)
            .color(p.text_sec)
            .font(Font::MONOSPACE)
            .width(Length::Fill)
            .wrapping(text::Wrapping::WordOrGlyph),
    ]
    .spacing(FIELD_GAP)
    .into()
}

#[cfg(test)]
mod tests {
    use super::{TrustActions, action_row, panel};
    use crate::core::data::ServerKeyInfo;
    use crate::core::i18n::t_static;
    use crate::core::message::Message;
    use crate::core::theme::{ThemeMode, palette};
    use iced::widget::column;
    use rust_i18n::t;

    fn key_info() -> ServerKeyInfo {
        ServerKeyInfo {
            hostname: "example.test".into(),
            port: 2222,
            key_type: "ssh-ed25519".into(),
            public_key: "ssh-ed25519 AAAA test".into(),
            fingerprint: "SHA256:complete-fingerprint".into(),
        }
    }

    #[test]
    fn shared_prompt_shows_every_key_detail_and_routes_each_action() {
        let p = palette(ThemeMode::Dark);
        let key = key_info();
        let view = column![
            panel(&key, Some("database tunnel"), p),
            action_row(
                TrustActions::new(
                    t_static(t!("settings.cancel")),
                    Message::ShowToast("cancel".into()),
                    Message::ShowToast("continue".into()),
                    Message::ShowToast("add".into()),
                ),
                p,
            ),
        ];
        let mut ui = iced_test::simulator(view);

        ui.find("database tunnel")
            .expect("the requesting feature is visible");
        ui.find("example.test:2222")
            .expect("the host and port are visible");
        ui.find("ssh-ed25519")
            .expect("the key algorithm is visible");
        ui.find("SHA256:complete-fingerprint")
            .expect("the complete fingerprint is visible");

        for (label, expected) in [
            (t!("settings.cancel"), "cancel"),
            (t!("ssh.continue"), "continue"),
            (t!("ssh.add_and_continue"), "add"),
        ] {
            let mut ui = iced_test::simulator(column![action_row(
                TrustActions::new(
                    t_static(t!("settings.cancel")),
                    Message::ShowToast("cancel".into()),
                    Message::ShowToast("continue".into()),
                    Message::ShowToast("add".into()),
                ),
                p,
            )]);
            ui.click(label.to_string())
                .expect("host-key action is clickable");
            let messages: Vec<_> = ui.into_messages().collect();
            assert!(
                matches!(
                    messages.as_slice(),
                    [Message::ShowToast(message)] if message == expected
                ),
                "{label} should emit {expected}"
            );
        }
    }
}
