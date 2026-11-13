// Rust guideline compliant 2026-10-09

use iced::widget::{Space, button, column, container, row, text};
use iced::{Background, Border, Color, Element, Length, Padding, Theme, mouse};

use crate::core::data::{format_size, format_speed};
use crate::core::i18n::t_static;
use crate::core::message::Message;
use crate::core::theme::ThemePalette;
use crate::core::widgets::{styled_btn, text_width_estimate};
use rust_i18n::t;

use crate::sftp::{
    SftpQueuedTransfer, SftpTransferDirection, SftpTransferResult, SftpTransferResultStatus,
};

use super::LUCIDE_FONT;

/// Shared text size for in-flight and queued transfer rows.
const TRANSFER_ROW_TEXT_SIZE: f32 = 15.0;

// Estimate the filename at its rendered size so the reason can follow the
// actual label instead of starting after a fixed-width blank column.
const RESULT_FILE_CELL_WIDTH: f32 = 7.0;
const RESULT_FILE_MAX_WIDTH: f32 = 180.0;

// Small enough that the reason reads as part of the file name label, while the
// outer row keeps its normal spacing around the icon and close button.
const RESULT_REASON_GAP: f32 = 4.0;

fn close_button_style(p: ThemePalette, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered);
    button::Style {
        background: Some(Background::Color(if hovered {
            p.item_hover
        } else {
            p.surface
        })),
        text_color: if hovered { p.text_pri } else { p.text_sec },
        border: Border {
            radius: 4.0.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

/// Render a finished transfer in the same bottom strip as live progress.
///
/// Long names and failure reasons end with an ellipsis so the strip stays one
/// stable height and never pushes the file browser upward.
pub(super) fn view_transfer_result(
    result: &SftpTransferResult,
    dismiss: Message,
    p: ThemePalette,
) -> Element<'_, Message> {
    let (status_label, status_color, status_icon) = match result.status {
        SftpTransferResultStatus::Succeeded => (
            Some(t_static(t!("sftp.transfer_completed"))),
            p.success,
            lucide_icons::Icon::Check,
        ),
        SftpTransferResultStatus::Failed => (None, p.danger, lucide_icons::Icon::AlertTriangle),
        SftpTransferResultStatus::Cancelled => (
            Some(t_static(t!("sftp.transfer_cancelled_status"))),
            p.warning,
            lucide_icons::Icon::AlertTriangle,
        ),
    };

    let close_btn = button(
        text(char::from(lucide_icons::Icon::X))
            .font(LUCIDE_FONT)
            .size(TRANSFER_ROW_TEXT_SIZE)
            .color(p.text_sec),
    )
    .on_press(dismiss)
    .padding(Padding::from([4.0, 6.0]))
    .style(move |_, status| close_button_style(p, status))
    .cursor_interaction(mouse::Interaction::default());

    let file_name_width = if result.error.is_some() {
        Length::Fixed(
            (text_width_estimate(&result.file_name, RESULT_FILE_CELL_WIDTH) + 2.0)
                .min(RESULT_FILE_MAX_WIDTH),
        )
    } else {
        Length::Fill
    };
    let mut content_row = row![
        text(&result.file_name)
            .size(TRANSFER_ROW_TEXT_SIZE)
            .color(p.text_pri)
            .width(file_name_width)
            .align_x(iced::alignment::Horizontal::Left)
            .align_y(iced::alignment::Vertical::Center)
            .ellipsis(text::Ellipsis::End)
            .wrapping(iced::widget::text::Wrapping::None),
    ]
    .align_y(iced::Alignment::Center);
    if let Some(error) = result.error.as_ref() {
        content_row = content_row
            .push(
                text(error)
                    .size(TRANSFER_ROW_TEXT_SIZE)
                    .color(p.danger)
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Left)
                    .align_y(iced::alignment::Vertical::Center)
                    .ellipsis(text::Ellipsis::End)
                    .wrapping(iced::widget::text::Wrapping::None),
            )
            .spacing(RESULT_REASON_GAP);
    } else if let Some(status_label) = status_label {
        content_row = content_row
            .push(
                text(status_label)
                    .size(TRANSFER_ROW_TEXT_SIZE)
                    .color(status_color)
                    .align_y(iced::alignment::Vertical::Center)
                    .width(Length::Shrink),
            )
            .spacing(8);
    }
    let result_row = row![
        text(char::from(status_icon))
            .font(LUCIDE_FONT)
            .size(TRANSFER_ROW_TEXT_SIZE)
            .color(status_color)
            .align_y(iced::alignment::Vertical::Center)
            .width(Length::Shrink),
        content_row.width(Length::Fill),
        close_btn,
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    column![
        container(Space::new().width(Length::Fill).height(2))
            .width(Length::Fill)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(status_color)),
                ..container::Style::default()
            }),
        container(result_row)
            .width(Length::Fill)
            .padding(Padding::from([8.0, 12.0]))
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(p.content_bg)),
                ..container::Style::default()
            })
            .clip(true),
    ]
    .width(Length::Fill)
    .into()
}

pub(super) fn view_queued_transfers<'a>(
    queue: &'a [SftpQueuedTransfer],
    p: ThemePalette,
) -> Element<'a, Message> {
    if queue.is_empty() {
        return Space::new()
            .width(Length::Shrink)
            .height(Length::Shrink)
            .into();
    }

    let queue_header: Element<'a, Message> = if queue.len() > 1 {
        container(
            row![
                text(t!("sftp.pending_count", count = queue.len().to_string()).to_string())
                    .size(13)
                    .color(p.text_sec)
                    .width(Length::Fill),
                button(
                    text(t_static(t!("sftp.clear_all")))
                        .size(13)
                        .color(p.text_sec),
                )
                .on_press(Message::Sftp(
                    crate::sftp::message::Message::SftpClearTransferQueue
                ))
                .padding(Padding::from([2.0, 6.0]))
                .style(move |_: &Theme, status| {
                    let hovered = matches!(status, button::Status::Hovered);
                    button::Style {
                        background: hovered.then_some(Background::Color(p.surface)),
                        text_color: if hovered { p.text_pri } else { p.text_sec },
                        border: Border {
                            radius: 3.0.into(),
                            ..Border::default()
                        },
                        ..button::Style::default()
                    }
                })
                .cursor_interaction(mouse::Interaction::default()),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .padding(Padding::from([4.0, 12.0]))
        .into()
    } else {
        Space::new()
            .width(Length::Shrink)
            .height(Length::Shrink)
            .into()
    };

    let items: Vec<Element<'a, Message>> = queue
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let icon = match item.direction {
                SftpTransferDirection::Upload => '↑',
                SftpTransferDirection::Download => '↓',
            };
            let size_str = if item.total_bytes > 0 {
                format_size(item.total_bytes)
            } else if item.total_files > 1 {
                t!("sftp.file_count", count = item.total_files.to_string()).to_string()
            } else {
                String::new()
            };
            container(
                row![
                    text(icon.to_string()).size(14).color(p.accent),
                    text(&item.file_name)
                        .size(TRANSFER_ROW_TEXT_SIZE)
                        .color(p.text_pri)
                        .width(Length::Fill)
                        .ellipsis(text::Ellipsis::End)
                        .wrapping(iced::widget::text::Wrapping::None),
                    text(size_str)
                        .size(TRANSFER_ROW_TEXT_SIZE)
                        .color(p.text_sec)
                        .width(Length::Shrink),
                    text(t_static(t!("sftp.waiting")))
                        .size(TRANSFER_ROW_TEXT_SIZE)
                        .color(p.text_sec)
                        .width(Length::Shrink),
                    button(
                        text(char::from(lucide_icons::Icon::X))
                            .font(LUCIDE_FONT)
                            .size(13)
                            .color(p.danger),
                    )
                    .on_press(Message::Sftp(
                        crate::sftp::message::Message::SftpCancelQueuedTransfer(i)
                    ))
                    .padding(Padding::from([2.0, 4.0]))
                    .style(move |_: &Theme, status| {
                        let hovered = matches!(status, button::Status::Hovered);
                        button::Style {
                            background: hovered.then_some(Background::Color(p.surface)),
                            text_color: p.danger,
                            border: Border {
                                radius: 3.0.into(),
                                ..Border::default()
                            },
                            ..button::Style::default()
                        }
                    })
                    .cursor_interaction(mouse::Interaction::default()),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            )
            .width(Length::Fill)
            .padding(Padding::from([4.0, 12.0]))
            .into()
        })
        .collect();

    container(
        column![
            // Simulated top border
            container(Space::new())
                .width(Length::Fill)
                .height(1)
                .style(move |_: &Theme| container::Style {
                    background: Some(Background::Color(p.border)),
                    ..Default::default()
                }),
            queue_header,
            column(items).spacing(0),
        ]
        .width(Length::Fill),
    )
    .width(Length::Fill)
    .style(move |_: &Theme| container::Style {
        background: Some(Background::Color(p.content_bg)),
        ..Default::default()
    })
    .into()
}

pub(super) fn view_transfer_progress<'a>(
    progress: &'a std::sync::Arc<std::sync::Mutex<crate::sftp::SftpTransferProgress>>,
    anim_t: f32,
    p: ThemePalette,
) -> Element<'a, Message> {
    let (
        file_name,
        direction,
        transferred_bytes,
        total_bytes,
        current_file,
        files_completed,
        total_files,
        resumed,
        start_time,
        speed,
        _,
    ) = progress.lock().map_or_else(
        |_| {
            (
                t!("sftp.transfer_tab").to_string(),
                SftpTransferDirection::Upload,
                0,
                0,
                String::new(),
                0,
                1,
                false,
                None,
                0.0,
                None,
            )
        },
        |progress| {
            (
                progress.file_name.clone(),
                progress.direction,
                progress.transferred_bytes,
                progress.total_bytes,
                progress.current_file.clone(),
                progress.files_completed,
                progress.total_files,
                progress.resumed,
                progress.start_time,
                progress.speed,
                progress.last_speed_update,
            )
        },
    );

    // Connection setup and transfer progress are independent: an empty file is
    // still connected, while a queued session can be ready before any bytes move.
    let connecting = start_time.is_none();
    let ratio = if total_bytes == 0 {
        0.0
    } else {
        (transferred_bytes as f32 / total_bytes as f32).clamp(0.0, 1.0)
    };
    let percent = (ratio * 100.0).round() as u8;
    let label = match (direction, resumed) {
        (SftpTransferDirection::Upload, true) => t!("sftp.resuming_upload"),
        (SftpTransferDirection::Download, true) => t!("sftp.resuming_download"),
        (SftpTransferDirection::Upload, false) => t!("sftp.status_uploading"),
        (SftpTransferDirection::Download, false) => t!("sftp.status_downloading"),
    };
    let description = if total_files > 1 {
        format!("{label} [{files_completed}/{total_files}] {current_file}")
    } else {
        format!("{label} {file_name}")
    };
    let progress_text = if connecting {
        t_static(t!("sftp.connecting")).to_string()
    } else if total_bytes == 0 {
        format_size(transferred_bytes).clone()
    } else {
        format!(
            "{}% · {} / {}",
            percent,
            format_size(transferred_bytes),
            format_size(total_bytes),
        )
    };
    let progress_text = if speed > 0.0 {
        format!("{}  {}", progress_text, format_speed(speed))
    } else {
        progress_text
    };

    let fill_portion = if total_bytes == 0 || connecting {
        0
    } else {
        (ratio * 10_000.0).round() as u16
    };
    let empty_portion = 10_000_u16.saturating_sub(fill_portion);
    let bar_segment = |portion: u16, color: Color| -> Element<'a, Message> {
        container(text(""))
            .width(Length::FillPortion(portion.max(1)))
            .height(Length::Fixed(5.0))
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(color)),
                border: Border {
                    radius: 2.5.into(),
                    ..Border::default()
                },
                ..Default::default()
            })
            .into()
    };

    column![
        // Simulated top border
        container(Space::new())
            .width(Length::Fill)
            .height(1)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(p.border)),
                ..Default::default()
            }),
        container(
            row![
                text(description)
                    .size(TRANSFER_ROW_TEXT_SIZE)
                    .color(p.text_pri)
                    .width(Length::FillPortion(5))
                    .ellipsis(text::Ellipsis::End)
                    .wrapping(iced::widget::text::Wrapping::None),
                row![
                    if connecting {
                        view_transfer_shimmer(anim_t, p)
                    } else {
                        row![
                            bar_segment(fill_portion, p.accent),
                            bar_segment(empty_portion, p.surface)
                        ]
                        .spacing(0)
                        .width(Length::Fill)
                        .into()
                    },
                    text(progress_text)
                        .size(TRANSFER_ROW_TEXT_SIZE)
                        .color(p.text_sec)
                        .width(Length::Shrink)
                        .wrapping(iced::widget::text::Wrapping::None),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center)
                .width(Length::FillPortion(4)),
                styled_btn(
                    t_static(t!("sftp.discard")),
                    Some(Message::Sftp(
                        crate::sftp::message::Message::SftpCancelTransferRequest
                    )),
                    p.danger,
                    Color::WHITE,
                    p
                ),
            ]
            .spacing(12)
            .align_y(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .padding(Padding::from([8.0, 12.0]))
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(p.content_bg)),
            ..Default::default()
        })
        .clip(true),
    ]
    .width(Length::Fill)
    .into()
}

/// Indeterminate shimmer bar while a transfer is establishing its SFTP
/// connection. Mirrors the ssh-connect bar.
fn view_transfer_shimmer<'a>(anim_t: f32, p: ThemePalette) -> Element<'a, Message> {
    const W: f32 = 400.0;
    const SHIMMER: f32 = 64.0;
    let pos = (anim_t * (W + SHIMMER) / 0.7) % (W + SHIMMER) - SHIMMER;
    let left = pos.clamp(0.0, W);
    let shim_start = pos.max(0.0);
    let shim_end = (pos + SHIMMER).clamp(0.0, W);
    let shim_w = (shim_end - shim_start).max(0.0);
    let right = (W - shim_end).max(0.0);
    let track = Color {
        r: p.surface.r + (p.accent.r - p.surface.r) * 0.22,
        g: p.surface.g + (p.accent.g - p.surface.g) * 0.22,
        b: p.surface.b + (p.accent.b - p.surface.b) * 0.22,
        a: p.surface.a + (p.accent.a - p.surface.a) * 0.22,
    };

    let seg = |w: f32, color: Color| -> Option<Element<'a, Message>> {
        if w <= 0.0 {
            return None;
        }
        Some(
            container(text(""))
                .width(Length::FillPortion((w * 100.0) as u16))
                .height(Length::Fixed(5.0))
                .style(move |_: &Theme| container::Style {
                    background: Some(Background::Color(color)),
                    border: Border {
                        radius: 2.5.into(),
                        ..Border::default()
                    },
                    ..Default::default()
                })
                .into(),
        )
    };

    let mut parts = row![].spacing(0).width(Length::Fill);
    for (w, color) in [(left, track), (shim_w, p.accent), (right, track)] {
        if let Some(segment) = seg(w, color) {
            parts = parts.push(segment);
        }
    }
    parts.into()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::core::theme::{ThemeMode, palette};
    use crate::sftp::SftpTransferProgress;

    fn progress(start_time: Option<std::time::Instant>) -> Arc<Mutex<SftpTransferProgress>> {
        Arc::new(Mutex::new(SftpTransferProgress {
            file_name: "empty.txt".into(),
            direction: SftpTransferDirection::Upload,
            transferred_bytes: 0,
            total_bytes: 0,
            cancelled: false,
            current_file: "empty.txt".into(),
            files_completed: 0,
            total_files: 1,
            resumed: false,
            start_time,
            speed: 0.0,
            last_speed_update: None,
        }))
    }

    #[test]
    fn a_connected_zero_byte_transfer_is_not_shown_as_connecting() {
        let progress = progress(Some(std::time::Instant::now()));
        let mut ui = iced_test::simulator(view_transfer_progress(
            &progress,
            0.0,
            palette(ThemeMode::Dark),
        ));

        ui.find("Connecting…")
            .expect_err("a connected transfer must not show the connecting label");
        ui.find("0 B")
            .expect("an empty transfer shows its byte count");
    }

    #[test]
    fn an_unconnected_transfer_is_shown_as_connecting() {
        let progress = progress(None);
        let mut ui = iced_test::simulator(view_transfer_progress(
            &progress,
            0.0,
            palette(ThemeMode::Dark),
        ));

        ui.find("Connecting…")
            .expect("an unconnected transfer shows the connecting label");
    }

    #[test]
    fn result_close_button_uses_the_host_picker_hover_style() {
        let p = palette(ThemeMode::Dark);
        let active = close_button_style(p, button::Status::Active);
        let hovered = close_button_style(p, button::Status::Hovered);

        assert_eq!(active.background, Some(Background::Color(p.surface)));
        assert_eq!(active.text_color, p.text_sec);
        assert_eq!(hovered.background, Some(Background::Color(p.item_hover)));
        assert_eq!(hovered.text_color, p.text_pri);
        assert_eq!(hovered.border.radius, 4.0.into());
    }

    #[test]
    fn failure_result_follows_the_success_layout_without_a_status_label() {
        let result = SftpTransferResult {
            file_name: "release.tar.gz".into(),
            status: SftpTransferResultStatus::Failed,
            error: Some("Download incomplete: received 512 of 1024 bytes".into()),
        };
        let mut ui = iced_test::simulator(view_transfer_result(
            &result,
            Message::Noop,
            palette(ThemeMode::Dark),
        ));

        let reason = ui
            .find("Download incomplete: received 512 of 1024 bytes")
            .expect("the failure reason is rendered")
            .visible_bounds()
            .expect("the failure reason is laid out");
        let file = ui
            .find("release.tar.gz")
            .expect("the file name is rendered")
            .visible_bounds()
            .expect("the file name is laid out");

        assert!(
            file.x < reason.x,
            "the file name should appear before the failure reason"
        );
        let gap = reason.x - (file.x + file.width);
        assert!(
            gap <= RESULT_REASON_GAP + 1.0,
            "the failure reason should sit directly beside the file name; gap was {gap}"
        );
        assert!(
            (file.center().y - reason.center().y).abs() <= 1.0,
            "the file name and failure reason should share one vertical center"
        );
        assert!(
            (file.height - reason.height).abs() <= 1.0,
            "the file name and failure reason should use one text size"
        );
        assert!(
            ui.find("Failed").is_err(),
            "the red icon already communicates failure"
        );
    }
}
