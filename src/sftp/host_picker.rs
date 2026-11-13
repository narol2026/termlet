//! Shared ordering for the SFTP local and remote host pickers.

// Rust guideline compliant 2026-10-09

use crate::app::App;
use crate::core::data::{Host, HostId};
use crate::sftp::SftpPane;

pub(super) fn visible_hosts(app: &App, pane: SftpPane) -> Vec<&Host> {
    let filter = app.sftp.state.host_picker(pane).filter.to_lowercase();
    let mut hosts = app
        .hosts
        .list
        .iter()
        .filter(|host| {
            filter.is_empty()
                || host.name.to_lowercase().contains(&filter)
                || host.username.to_lowercase().contains(&filter)
                || host.hostname.to_lowercase().contains(&filter)
        })
        .collect::<Vec<_>>();
    app.hosts.sort_by_recency(&mut hosts);
    hosts
}

pub(super) fn visible_ids(app: &App, pane: SftpPane) -> Vec<HostId> {
    visible_hosts(app, pane)
        .iter()
        .map(|host| host.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{visible_hosts, visible_ids};
    use crate::core::data::HostId;
    use crate::core::test_support::{app, host};

    #[test]
    fn visible_hosts_filter_and_follow_recent_order() {
        let mut app = app();
        app.hosts.list = vec![host(1, "web", "10.0.0.1"), host(2, "db", "10.0.0.2")];
        app.hosts.recent_ids = vec![HostId(2), HostId(1)];

        assert_eq!(
            visible_ids(&app, crate::sftp::SftpPane::Local),
            vec![HostId(2), HostId(1)]
        );

        app.sftp.state.left.host_picker.filter = "web".into();
        assert_eq!(
            visible_ids(&app, crate::sftp::SftpPane::Local),
            vec![HostId(1)]
        );
        assert_eq!(
            visible_hosts(&app, crate::sftp::SftpPane::Local)
                .iter()
                .map(|host| host.id)
                .collect::<Vec<_>>(),
            vec![HostId(1)],
            "the host list and its ids describe the same rows"
        );
    }

    #[test]
    fn the_host_picker_filter_matches_every_visible_field() {
        let mut app = app();
        app.hosts.list = vec![host(1, "web", "Example.COM"), host(2, "db", "10.0.0.2")];

        app.sftp.state.left.host_picker.filter = "weB".into();
        assert_eq!(
            visible_ids(&app, crate::sftp::SftpPane::Local),
            vec![HostId(1)]
        );

        app.sftp.state.left.host_picker.filter = "example.com".into();
        assert_eq!(
            visible_ids(&app, crate::sftp::SftpPane::Local),
            vec![HostId(1)]
        );

        app.sftp.state.left.host_picker.filter = "ROOT".into();
        assert_eq!(
            visible_ids(&app, crate::sftp::SftpPane::Local),
            vec![HostId(1), HostId(2)],
            "both fixtures use the root username"
        );
    }
}
