use iced::widget::{column, container};
use iced::{Background, Element, Length, Theme};

use crate::app::App;
use crate::core::data::NavItem;
use crate::core::diagnostics::Level;
use crate::core::message::Message;
use crate::core::theme::{self};

use super::logs::view_logs;
use crate::hosts::view::view_hosts_page;
use crate::keychain::view::view_keychain;
use crate::known_hosts::view::view_known_hosts;
use crate::port_forward::view::view_port_forwards;
use crate::snippets::view::view_snippets;
use crate::views::settings::view_settings;

pub fn view_content(app: &App) -> Element<'_, Message> {
    let _guard = crate::core::diagnostics::perf_guard(
        Level::Debug,
        "VIEW",
        "view_content",
        std::time::Duration::from_millis(16),
    );
    let p = theme::palette(app.effective_theme_mode());

    let body: Element<'_, Message> = match app.active_nav {
        NavItem::Hosts => view_hosts_page(app),
        NavItem::Keychain => view_keychain(app),
        NavItem::PortForwarding => view_port_forwards(app),
        NavItem::Snippets => view_snippets(app),
        NavItem::KnownHosts => view_known_hosts(app),
        NavItem::Logs => view_logs(app),
        NavItem::Settings => view_settings(app),
    };

    container(column![body].width(Length::Fill).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(p.content_bg)),
            ..Default::default()
        })
        .into()
}
