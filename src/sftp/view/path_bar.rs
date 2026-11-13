use iced::widget::{Space, button, container, responsive, row, text, text_input};
use iced::{Background, Border, Color, Element, Length, Padding, Theme, mouse};

use crate::core::i18n::t_static;
use crate::core::message::Message;
use crate::core::theme::ThemePalette;
use crate::core::widgets::{cursor_tooltip_label, input_style_with_palette, text_width_estimate};
use crate::sftp::SftpPane;
use crate::sftp::breadcrumb::Breadcrumb;
use rust_i18n::t;

use super::{LUCIDE_FONT, icon_btn, icon_nav_btn};

const MIN_BREADCRUMB_SEGMENT_WIDTH: f32 = 90.0;
/// Minimum readable width a tail segment is allowed to shrink to.
const SEGMENT_MIN_WIDTH: f32 = 60.0;
const BREADCRUMB_ITEM_SPACING: f32 = 2.0;
/// Estimated width of one " › " separator at size 13 (≈ 3 narrow cells).
const BREADCRUMB_SEP_WIDTH: f32 = 22.0;

pub(super) fn view_path_bar<'a>(
    path: &'a str,
    pane: SftpPane,
    editing: Option<&str>,
    can_back: bool,
    can_forward: bool,
    p: ThemePalette,
) -> Element<'a, Message> {
    let back_btn = if pane == SftpPane::Local {
        icon_nav_btn(
            lucide_icons::Icon::ChevronLeft,
            can_back,
            Message::Sftp(crate::sftp::message::Message::SftpLocalBack),
            p,
        )
    } else {
        icon_nav_btn(
            lucide_icons::Icon::ChevronLeft,
            can_back,
            Message::Sftp(crate::sftp::message::Message::SftpRemoteBack),
            p,
        )
    };

    let forward_btn = if pane == SftpPane::Local {
        icon_nav_btn(
            lucide_icons::Icon::ChevronRight,
            can_forward,
            Message::Sftp(crate::sftp::message::Message::SftpLocalForward),
            p,
        )
    } else {
        icon_nav_btn(
            lucide_icons::Icon::ChevronRight,
            can_forward,
            Message::Sftp(crate::sftp::message::Message::SftpRemoteForward),
            p,
        )
    };

    let nav_btns: Element<'a, Message> = row![back_btn, forward_btn]
        .spacing(4)
        .align_y(iced::Alignment::Center)
        .into();

    let sp = Space::new().width(Length::Fixed(8.0));

    if let Some(value) = editing {
        let input = text_input(
            if pane == SftpPane::Local {
                t_static(t!("sftp.left.pane.path"))
            } else {
                t_static(t!("sftp.right.pane.path"))
            },
            value,
        )
        .id(iced::widget::Id::from(if pane == SftpPane::Local {
            "sftp_path_input_local"
        } else {
            "sftp_path_input_remote"
        }))
        .on_input(move |v| {
            Message::Sftp(crate::sftp::message::Message::SftpPathEditChange(pane, v))
        })
        .on_submit(Message::Sftp(
            crate::sftp::message::Message::SftpPathEditSubmit(pane),
        ))
        .padding(4)
        .size(14)
        .width(Length::Fill)
        .style(move |_t, s| input_style_with_palette(&p, s));

        container(
            row![
                nav_btns,
                sp,
                input,
                icon_btn(
                    lucide_icons::Icon::X,
                    p.text_sec,
                    Message::Sftp(crate::sftp::message::Message::SftpPathEditCancel(pane)),
                    p,
                    "",
                )
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center),
        )
        // 2px top/bottom mirrors the breadcrumb button's own padding so both
        // modes render the same bar height and focus doesn't shift the layout.
        .padding(Padding::from([2.0, 0.0]))
        .into()
    } else {
        // responsive only wraps the breadcrumb area — the row layout stays intact
        let breadcrumb =
            responsive(move |size| view_breadcrumb_segments(path, pane, p, size.width))
                .height(Length::Shrink);

        container(
            row![
                nav_btns,
                sp,
                breadcrumb,
                icon_btn(
                    lucide_icons::Icon::Pencil,
                    p.text_sec,
                    Message::Sftp(crate::sftp::message::Message::SftpPathEditStart(pane)),
                    p,
                    "",
                ),
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding::from([0.0, 0.0]))
        .into()
    }
}

/// Estimated width of the fully-expanded breadcrumb row (root + every segment
/// with its " › " separator). Each segment is icon(14) + inner spacing(4) +
/// label text + button padding(6+6); items are spaced 2px apart.
fn estimated_full_breadcrumb_width(breadcrumb: &Breadcrumb) -> f32 {
    let mut width = segment_width_estimate(breadcrumb.root().label());
    for directory in breadcrumb.directories() {
        width += 2.0 * BREADCRUMB_ITEM_SPACING
            + BREADCRUMB_SEP_WIDTH
            + segment_width_estimate(directory.label());
    }
    width
}

fn breadcrumb_message(pane: SftpPane, target: &str) -> Message {
    if pane == SftpPane::Local {
        Message::Sftp(crate::sftp::message::Message::SftpLocalNavigate(
            target.to_string(),
        ))
    } else {
        Message::Sftp(crate::sftp::message::Message::SftpRemoteNavigate(
            target.to_string(),
        ))
    }
}

/// Estimated rendered width of one breadcrumb segment: folder icon(14) +
/// icon/text spacing(4) + label text + button padding(6+6).
fn segment_width_estimate(label: &str) -> f32 {
    text_width_estimate(label, 8.0) + 14.0 + 4.0 + 12.0
}

/// Shrink segments flexbox-style: every segment keeps its minimum width, then
/// the remaining space is handed back in proportion to how much each segment
/// can still shrink. Long labels therefore keep more absolute width than short
/// ones, and the row is filled exactly whenever it overflows.
fn distribute_segment_widths(naturals: &[f32], mins: &[f32], available: f32) -> Vec<f32> {
    let natural_sum: f32 = naturals.iter().sum();
    if natural_sum <= available {
        return naturals.to_vec();
    }

    let min_sum: f32 = mins.iter().sum();
    let free = available - min_sum;
    if free < 0.0 {
        // Even the minimum widths overflow: shrink them all proportionally.
        let ratio = (available / min_sum.max(f32::EPSILON)).clamp(0.0, 1.0);
        return mins.iter().map(|m| m * ratio).collect();
    }

    let shrinkable: Vec<f32> = naturals
        .iter()
        .zip(mins)
        .map(|(n, m)| (n - m).max(0.0))
        .collect();
    let shrinkable_sum: f32 = shrinkable.iter().sum();
    if shrinkable_sum <= f32::EPSILON {
        return mins.to_vec();
    }

    naturals
        .iter()
        .zip(mins)
        .zip(&shrinkable)
        .map(|((_n, m), s)| m + free * s / shrinkable_sum)
        .collect()
}

fn view_breadcrumb_segments<'a>(
    path: &str,
    pane: SftpPane,
    p: ThemePalette,
    available_width: f32,
) -> Element<'a, Message> {
    let breadcrumb = Breadcrumb::for_pane(path, pane);
    let root = breadcrumb.root();
    let directories = breadcrumb.directories();
    let total = directories.len();

    // responsive already subtracts nav buttons + spacer + edit button, so
    // `available_width` is the breadcrumb area width from the row layout.
    // Measure the fully-expanded row first; only fall back to collapsing the
    // prefix behind "…" if it genuinely overflows. Short names (even a deeply
    // nested all-short path) therefore render fully instead of being folded.
    let fits_full = estimated_full_breadcrumb_width(&breadcrumb) <= available_width;

    let root_msg = breadcrumb_message(pane, root.target());

    let mut items: Vec<Element<'a, Message>> = vec![];

    if fits_full {
        // Everything fits: show root + every component at natural width.
        // `available_width` is only a hard safety cap and never binds here.
        items.push(breadcrumb_segment(
            root.label(),
            root_msg,
            directories.is_empty(),
            p,
            available_width,
        ));
        for (i, directory) in directories.iter().enumerate() {
            items.push(text(" › ").size(13).color(p.text_dim).into());
            let is_last = i == total - 1;
            let msg = breadcrumb_message(pane, directory.target());
            items.push(breadcrumb_segment(
                directory.label(),
                msg,
                is_last,
                p,
                available_width,
            ));
        }
    } else {
        // Doesn't fit fully: estimate how many tail segments fit and collapse
        // the prefix behind "…". Root always takes 1 slot, "…" takes 1.
        let max_fit = ((available_width / MIN_BREADCRUMB_SEGMENT_WIDTH) as usize).max(2);
        let visible_tail = max_fit.saturating_sub(2).max(1);
        let skip = total.saturating_sub(visible_tail);

        // Root and "…" keep their natural width; tail segments may shrink down
        // to SEGMENT_MIN_WIDTH. Distribute the leftover budget with flex-shrink
        // so long names keep more width instead of everyone being capped at an
        // average `per_segment_max`.
        let mut naturals = vec![segment_width_estimate("/"), segment_width_estimate("…")];
        let mut mins = vec![naturals[0], naturals[1]];
        naturals[0] = segment_width_estimate(root.label());
        for directory in &directories[skip..] {
            let natural = segment_width_estimate(directory.label());
            naturals.push(natural);
            mins.push(natural.min(SEGMENT_MIN_WIDTH));
        }

        let fixed_overhead =
            (visible_tail + 1) as f32 * (2.0 * BREADCRUMB_ITEM_SPACING + BREADCRUMB_SEP_WIDTH);
        let segment_budget = (available_width - fixed_overhead).max(0.0);
        let widths = distribute_segment_widths(&naturals, &mins, segment_budget);

        items.push(breadcrumb_segment(
            root.label(),
            root_msg,
            false,
            p,
            widths[0],
        ));

        let hidden_path = breadcrumb.target_after_hidden(skip);
        let collapse_msg = breadcrumb_message(pane, hidden_path);
        items.push(text(" › ").size(13).color(p.text_dim).into());
        items.push(
            cursor_tooltip_label(
                breadcrumb_segment("…", collapse_msg, false, p, widths[1]),
                hidden_path,
                p,
                None,
            )
            .into(),
        );

        for (idx, (i, directory)) in directories.iter().enumerate().skip(skip).enumerate() {
            items.push(text(" › ").size(13).color(p.text_dim).into());
            let is_last = i == total - 1;
            let msg = breadcrumb_message(pane, directory.target());
            items.push(breadcrumb_segment(
                directory.label(),
                msg,
                is_last,
                p,
                widths[idx + 2],
            ));
        }
    }

    // edit affordance: clicking the empty space switches to edit mode
    let row_widget = row(items).spacing(2).align_y(iced::Alignment::Center);
    let inner = container(row_widget).width(Length::Fill).clip(true);

    button(inner)
        .on_press(Message::Sftp(
            crate::sftp::message::Message::SftpPathEditStart(pane),
        ))
        .padding(Padding {
            top: 2.0,
            right: 10.0,
            bottom: 2.0,
            left: 10.0,
        })
        .width(Length::Fill)
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: hovered.then_some(Background::Color(p.surface)),
                text_color: p.text_pri,
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default())
        .into()
}

fn breadcrumb_segment<'a>(
    label: &str,
    msg: Message,
    is_current: bool,
    p: ThemePalette,
    max_width: f32,
) -> Element<'a, Message> {
    let label_owned = label.to_string();
    let content = container(
        button(
            row![
                text(char::from(lucide_icons::Icon::Folder))
                    .font(LUCIDE_FONT)
                    .size(14)
                    .color(p.text_sec),
                text(label_owned.clone())
                    .size(14)
                    .width(Length::Fill)
                    .ellipsis(text::Ellipsis::End)
                    .wrapping(text::Wrapping::None)
                    .color(if is_current { p.text_pri } else { p.accent }),
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center),
        )
        .on_press(msg)
        .padding(Padding::from([4.0, 6.0]))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: hovered.then_some(Background::Color(p.item_hover)),
                border: Border {
                    radius: 3.0.into(),
                    ..Border::default()
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default()),
    )
    .max_width(max_width)
    .width(Length::Shrink);

    cursor_tooltip_label(content, &label_owned, p, None).into()
}

#[cfg(test)]
mod tests {
    use super::{distribute_segment_widths, estimated_full_breadcrumb_width};
    use crate::sftp::SftpPane;
    use crate::sftp::breadcrumb::Breadcrumb;

    #[test]
    fn estimated_breadcrumb_width_scales_with_label_length() {
        // A 10-deep path of single-char segments fits a 900px window.
        let comps = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"];
        let path = format!("/{}", comps.join("/"));
        let breadcrumb = Breadcrumb::for_pane(&path, SftpPane::Remote);
        assert!(
            estimated_full_breadcrumb_width(&breadcrumb) < 900.0,
            "short names should fit a 900px window"
        );

        // Same depth but long names overflows.
        let long: Vec<String> = comps
            .iter()
            .map(|c| format!("a_very_long_directory_{c}"))
            .collect();
        let long_path = format!("/{}", long.join("/"));
        let long_breadcrumb = Breadcrumb::for_pane(&long_path, SftpPane::Remote);
        assert!(
            estimated_full_breadcrumb_width(&long_breadcrumb) > 900.0,
            "long names should overflow a 900px window"
        );

        // CJK glyphs count double vs ASCII.
        let cjk =
            estimated_full_breadcrumb_width(&Breadcrumb::for_pane("/中文目录", SftpPane::Remote));
        let ascii =
            estimated_full_breadcrumb_width(&Breadcrumb::for_pane("/abcd", SftpPane::Remote));
        assert!(cjk > ascii, "wide glyphs should cost more than narrow ones");
    }

    #[test]
    fn distribute_shrinks_long_segments_and_fills_exactly() {
        // root, "…", a long tail segment, and a short tail segment.
        let naturals = vec![38.0, 38.0, 200.0, 20.0];
        let mins = vec![38.0, 38.0, 60.0, 20.0]; // short tail keeps natural width
        let widths = distribute_segment_widths(&naturals, &mins, 200.0);

        // Root and "…" never shrink; the short tail keeps its natural width.
        assert_eq!(widths[0], 38.0);
        assert_eq!(widths[1], 38.0);
        assert_eq!(widths[3], 20.0);

        // The long tail shrinks from 200 toward 60, but keeps more than min.
        assert!(widths[2] > 60.0 && widths[2] < 200.0);

        // The row fills the available budget exactly (no wasted space).
        let sum: f32 = widths.iter().sum();
        assert!((sum - 200.0).abs() < 0.5, "sum was {sum}");
    }

    #[test]
    fn distribute_returns_naturals_when_everything_fits() {
        let naturals = vec![38.0, 38.0, 120.0];
        let mins = vec![38.0, 38.0, 60.0];
        assert_eq!(distribute_segment_widths(&naturals, &mins, 500.0), naturals);
    }
}
