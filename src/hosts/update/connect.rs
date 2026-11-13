//! Starts SSH connection flows from saved hosts.

// Rust guideline compliant 2026-10-09

use iced::Task;

use crate::app::{App, DraftId};
use crate::core::data::{AuthType, Host, LogCategory};
use crate::core::message::Message;
use crate::hosts::message::Message as DomainMessage;
use crate::terminal::TabKind;

pub(super) fn update(app: &mut App, msg: &DomainMessage) -> Task<Message> {
    match msg {
        DomainMessage::ConnectHost(id) => {
            let id = *id;
            app.hosts.form.visible = false;
            app.hosts.form.reset();
            let Some(host) = app.hosts.list.iter().find(|host| host.id == id).cloned() else {
                return Task::none();
            };

            let target = format!("{}@{}:{}", host.username, host.hostname, host.port);
            app.add_log(LogCategory::Terminal, format!("Connecting to {target}"));
            app.hosts.record_recent(id);
            let recent_ids = app.hosts.recent_ids.clone();
            let recent_save =
                app.storage_write_task(move |storage| storage.save_recent_ids(&recent_ids));
            let draft_id = app.active_draft_id();
            start_ssh_connect(app, host, draft_id);
            recent_save
        }
        _ => Task::none(),
    }
}

pub(super) fn start_ssh_connect(app: &mut App, host: Host, new_tab_id: Option<DraftId>) {
    let target = format!("{}@{}:{}", host.username, host.hostname, host.port);
    let jump_host = host.jump_host_id.and_then(|jump_host_id| {
        app.hosts
            .list
            .iter()
            .find(|candidate| candidate.id == jump_host_id)
            .cloned()
    });
    let first_hop_label = jump_host.as_ref().map_or_else(
        || target.clone(),
        |jump| format!("{}@{}:{}", jump.username, jump.hostname, jump.port),
    );
    let needs_password = jump_host
        .as_ref()
        .is_some_and(|jump| jump.auth_type == AuthType::Password && !jump.has_password())
        || (jump_host.is_none() && host.auth_type == AuthType::Password && !host.has_password());

    // The host form is also rendered on the connect screen. Clear it so a form
    // left open in either surface cannot leak into the connection view.
    app.hosts.form.visible = false;
    app.hosts.form_closing = false;
    app.hosts.form_anim = 0.0;
    app.hosts.form.reset();

    let draft_id = new_tab_id.unwrap_or_else(|| {
        let id = super::new_tab::allocate_draft(app);
        app.allocate_tab_id(TabKind::Draft(id));
        id
    });

    let (initial_state, initial_logs) = if needs_password {
        (
            crate::ssh_connect::SshConnectionState::WaitingForPassword,
            vec![format!("Password required for {first_hop_label}")],
        )
    } else {
        (
            crate::ssh_connect::SshConnectionState::Connecting,
            vec![format!("Starting SSH authentication for {first_hop_label}")],
        )
    };

    if let Some(draft) = app.terminal.drafts.get_mut(&draft_id) {
        let mut attempt = crate::ssh_connect::ConnectionAttempt::new(host, jump_host);
        attempt.state = initial_state;
        draft.ssh = crate::ssh_connect::SshConnectState {
            attempt,
            logs: initial_logs,
            anim_t: 0.0,
        };
    }

    app.seed_tab_width_anims();
    if let Some(tab_id) = app.tab_id_for_draft(draft_id) {
        app.activate_tab(tab_id);
    }
}
