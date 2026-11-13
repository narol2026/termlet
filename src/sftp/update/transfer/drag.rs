//! Handles internal drag targets and file-row hover state.

// Rust guideline compliant 2026-10-09

use iced::Task;
use iced::widget::Id;

use crate::app::App;
use crate::core::message::Message;
use crate::sftp::SftpPane;
use crate::sftp::message::Message as DomainMessage;

use super::super::shared::{LOCAL_DROP_ZONE_ID, REMOTE_DROP_ZONE_ID, activate_pane};

pub(super) fn update(app: &mut App, msg: DomainMessage) -> Task<Message> {
    match msg {
        DomainMessage::SftpFileDropped {
            pane,
            name,
            is_file,
            selected_names,
            ..
        } => {
            let point = app.window.cursor_pos;
            iced_drop::zones_on_point(
                move |zones| {
                    Message::Sftp(DomainMessage::SftpDropZonesFound {
                        pane,
                        name: name.clone(),
                        is_file,
                        selected_names: selected_names.clone(),
                        zones,
                    })
                },
                point,
                Some(drop_zone_ids()),
                None,
            )
        }
        DomainMessage::SftpRowHoverEnter { pane, name } => {
            activate_pane(&mut app.sftp.state, pane);
            app.sftp.state.pane_mut(pane).hovered = Some(name);
            Task::none()
        }
        DomainMessage::SftpRowHoverExit { pane, name } => {
            let pane_state = app.sftp.state.pane_mut(pane);
            if pane_state.hovered.as_deref() == Some(name.as_str()) {
                pane_state.hovered = None;
            }
            Task::none()
        }
        DomainMessage::SftpDropZonesFound {
            pane,
            name,
            selected_names,
            zones,
            ..
        } => {
            let target = match pane {
                SftpPane::Local => Id::from(REMOTE_DROP_ZONE_ID),
                SftpPane::Remote => Id::from(LOCAL_DROP_ZONE_ID),
            };
            if !zones.iter().any(|(zone, _)| *zone == target) {
                return Task::none();
            }
            let pane_state = app.sftp.state.pane_mut(pane);
            pane_state.selected = if selected_names.len() > 1 && selected_names.contains(&name) {
                selected_names
            } else {
                vec![name.clone()]
            };
            pane_state.last_clicked = Some(name);
            let message = match pane {
                SftpPane::Local => DomainMessage::SftpUploadSelected,
                SftpPane::Remote => DomainMessage::SftpDownloadSelected,
            };
            super::super::update(app, message)
        }
        _ => Task::none(),
    }
}

fn drop_zone_ids() -> Vec<Id> {
    vec![Id::from(LOCAL_DROP_ZONE_ID), Id::from(REMOTE_DROP_ZONE_ID)]
}
