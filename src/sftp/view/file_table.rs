use iced::widget::{button, column, container, mouse_area, responsive, row, rule, text};
use iced::{Background, Border, Element, Length, Padding, Theme, mouse};

use crate::core::cursor_focus::CursorTarget;
use crate::core::data::{FileEntry, FileEntryType, SftpSortColumn, SftpSortDirection, format_size};
use crate::core::i18n::t_static;
use crate::core::message::Message;
use crate::core::theme::ThemePalette;
use crate::core::widgets::{cursor_tooltip_label, styled_scrollable};
use crate::sftp::SftpPane;
use crate::ui::cursor_area::CursorArea;
use rust_i18n::t;

use super::LUCIDE_FONT;

#[derive(Debug, Clone, Copy)]
struct FileTableLayout {
    name_portion: u16,
    date_portion: Option<u16>,
    size_portion: Option<u16>,
    kind_portion: Option<u16>,
    show_permissions: bool,
    file_name_width: f32,
}

#[allow(
    clippy::too_many_arguments,
    reason = "the parameter list mirrors the layout this widget builds"
)]
pub(super) fn view_file_table<'a>(
    entries: &'a [FileEntry],
    filter: &'a str,
    pane: SftpPane,
    path: &'a str,
    selected: &'a [String],
    hovered_name: Option<&'a str>,
    sort_column: SftpSortColumn,
    sort_direction: SftpSortDirection,
    scroll_id: &'static str,
    p: ThemePalette,
    scrollbar_visible: bool,
) -> Element<'a, Message> {
    responsive(move |size| {
        let layout = file_table_layout(size.width);
        view_file_table_sized(
            entries,
            filter,
            pane,
            path,
            selected,
            hovered_name,
            layout,
            sort_column,
            sort_direction,
            scroll_id,
            p,
            scrollbar_visible,
        )
    })
    .into()
}

#[allow(
    clippy::too_many_arguments,
    reason = "the parameter list mirrors the layout this widget builds"
)]
fn view_file_table_sized<'a>(
    entries: &'a [FileEntry],
    filter: &'a str,
    pane: SftpPane,
    path: &'a str,
    selected: &'a [String],
    hovered_name: Option<&'a str>,
    layout: FileTableLayout,
    sort_column: SftpSortColumn,
    sort_direction: SftpSortDirection,
    scroll_id: &'static str,
    p: ThemePalette,
    scrollbar_visible: bool,
) -> Element<'a, Message> {
    let header = column![
        container(file_header_row(
            layout,
            sort_column,
            sort_direction,
            pane,
            p
        ))
        .padding(Padding {
            left: 12.0,
            right: 12.0,
            top: 0.0,
            bottom: 0.0
        }),
        container(rule::horizontal(0))
            .width(Length::Fill)
            .height(1.0)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(p.border)),
                ..Default::default()
            }),
    ]
    .spacing(0)
    .width(Length::Fill);

    let mut rows: Vec<Element<'a, Message>> = vec![];

    // Parent directory entry
    if path != "/" {
        let parent_msg = if pane == SftpPane::Local {
            Message::Sftp(crate::sftp::message::Message::SftpLocalEntryClicked(
                "..".to_string(),
            ))
        } else {
            Message::Sftp(crate::sftp::message::Message::SftpRemoteEntryClicked(
                "..".to_string(),
            ))
        };
        rows.push(view_parent_row(
            parent_msg,
            None,
            pane,
            hovered_name == Some(".."),
            layout,
            p,
        ));
    }

    // Filtered entries
    let filter_lower = filter.to_lowercase();
    for entry in entries {
        if !filter.is_empty() && !entry.name.to_lowercase().contains(&filter_lower) {
            continue;
        }
        let msg = if pane == SftpPane::Local {
            Message::Sftp(crate::sftp::message::Message::SftpLocalEntryClicked(
                entry.name.clone(),
            ))
        } else {
            Message::Sftp(crate::sftp::message::Message::SftpRemoteEntryClicked(
                entry.name.clone(),
            ))
        };
        let double_click_msg = if pane == SftpPane::Local {
            Message::Sftp(crate::sftp::message::Message::SftpLocalEntryDoubleClicked(
                entry.name.clone(),
            ))
        } else {
            Message::Sftp(crate::sftp::message::Message::SftpRemoteEntryDoubleClicked(
                entry.name.clone(),
            ))
        };
        rows.push(view_file_row(
            entry,
            msg,
            Some(double_click_msg),
            pane,
            selected.iter().any(|s| s == &entry.name),
            hovered_name == Some(entry.name.as_str()),
            selected.to_vec(),
            layout,
            p,
        ));
    }

    if rows.is_empty() {
        let empty = container(
            text(t_static(t!("sftp.no_files")))
                .size(14)
                .color(p.text_sec),
        )
        .padding(20)
        .width(Length::Fill)
        .center_x(Length::Fill);
        return column![header, empty]
            .spacing(0)
            .height(Length::Fill)
            .into();
    }

    column![
        header,
        styled_scrollable(
            CursorArea::for_target(
                container(column(rows).spacing(1)).padding(Padding {
                    left: 12.0,
                    right: 12.0,
                    top: 6.0,
                    bottom: 0.0,
                }),
                if pane == SftpPane::Local {
                    CursorTarget::SftpLocalFiles
                } else {
                    CursorTarget::SftpRemoteFiles
                },
            ),
            p,
            scrollbar_visible,
        )
        .id(iced::widget::Id::from(scroll_id))
        .on_scroll(move |v| Message::Scrolled(scroll_id, v))
        .height(Length::Fill)
    ]
    .spacing(0)
    .height(Length::Fill)
    .into()
}

fn file_header_row<'a>(
    layout: FileTableLayout,
    sort_column: SftpSortColumn,
    sort_direction: SftpSortDirection,
    pane: SftpPane,
    p: ThemePalette,
) -> iced::widget::Row<'a, Message> {
    let render_header = |label: &str, col: SftpSortColumn, portion: u16| -> Element<'a, Message> {
        let is_active = sort_column == col;
        let content = if is_active {
            let icon = match sort_direction {
                SftpSortDirection::Ascending => lucide_icons::Icon::ChevronUp,
                SftpSortDirection::Descending => lucide_icons::Icon::ChevronDown,
            };
            row![
                text(label.to_owned()).size(13),
                text(char::from(icon)).font(LUCIDE_FONT).size(13),
            ]
            .align_y(iced::Alignment::Center)
            .spacing(4)
        } else {
            row![text(label.to_owned()).size(13)].align_y(iced::Alignment::Center)
        };
        button(
            container(content)
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Left),
        )
        .on_press(Message::Sftp(crate::sftp::message::Message::SftpSortBy {
            pane,
            column: col,
        }))
        .style(move |_: &Theme, status| header_button_style(p, is_active, status))
        .padding(Padding::from([4.0, 8.0]))
        .width(Length::FillPortion(portion))
        .cursor_interaction(mouse::Interaction::default())
        .into()
    };

    let mut cells: Vec<Element<'a, Message>> = vec![render_header(
        t_static(t!("sftp.col_name")),
        SftpSortColumn::Name,
        layout.name_portion,
    )];

    if let Some(portion) = layout.date_portion {
        cells.push(render_header(
            t_static(t!("sftp.col_date")),
            SftpSortColumn::DateModified,
            portion,
        ));
    }
    if let Some(portion) = layout.size_portion {
        cells.push(render_header(
            t_static(t!("sftp.col_size")),
            SftpSortColumn::Size,
            portion,
        ));
    }
    if let Some(portion) = layout.kind_portion {
        cells.push(render_header(
            t_static(t!("sftp.col_kind")),
            SftpSortColumn::Kind,
            portion,
        ));
    }

    row(cells).spacing(0)
}

fn header_button_style(
    p: ThemePalette,
    is_active: bool,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let hovered = matches!(status, iced::widget::button::Status::Hovered);
    let color = if hovered || is_active {
        p.accent
    } else {
        p.text_dim
    };
    iced::widget::button::Style {
        background: None,
        text_color: color,
        border: Border::default(),
        ..iced::widget::button::Style::default()
    }
}

fn view_parent_row<'a>(
    on_click: Message,
    on_double_click: Option<Message>,
    pane: SftpPane,
    hovered: bool,
    layout: FileTableLayout,
    p: ThemePalette,
) -> Element<'a, Message> {
    let mut name_parts: Vec<Element<'a, Message>> = vec![
        container(
            text(char::from(lucide_icons::Icon::Folder))
                .font(LUCIDE_FONT)
                .size(15)
                .color(p.accent),
        )
        .width(16)
        .center_x(16)
        .into(),
        file_name_text("..".to_string(), layout, p, false),
    ];

    if layout.show_permissions {
        name_parts.push(
            text("")
                .size(12)
                .width(Length::Fixed(74.0))
                .wrapping(iced::widget::text::Wrapping::None)
                .into(),
        );
    }

    let name_col =
        container(row(name_parts).spacing(6).align_y(iced::Alignment::Center)).clip(true);

    let mut cells: Vec<Element<'a, Message>> = vec![
        name_col
            .width(Length::FillPortion(layout.name_portion))
            .into(),
    ];

    if let Some(portion) = layout.date_portion {
        cells.push(
            text("")
                .size(13)
                .wrapping(iced::widget::text::Wrapping::None)
                .width(Length::FillPortion(portion))
                .into(),
        );
    }
    if let Some(portion) = layout.size_portion {
        cells.push(
            text("")
                .size(13)
                .wrapping(iced::widget::text::Wrapping::None)
                .width(Length::FillPortion(portion))
                .into(),
        );
    }
    if let Some(portion) = layout.kind_portion {
        cells.push(
            text("")
                .size(13)
                .wrapping(iced::widget::text::Wrapping::None)
                .width(Length::FillPortion(portion))
                .into(),
        );
    }

    let content = row(cells)
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .padding(Padding::from([4.0, 6.0]));

    let inner = container(content)
        .width(Length::Fill)
        .clip(true)
        .style(move |_: &Theme| container::Style {
            background: hovered.then_some(Background::Color(p.sftp_row_hover)),
            text_color: Some(p.text_pri),
            border: Border {
                radius: 4.0.into(),
                ..Border::default()
            },
            ..Default::default()
        });

    let mut area = mouse_area(inner)
        .on_press(on_click)
        .on_enter(Message::Sftp(
            crate::sftp::message::Message::SftpRowHoverEnter {
                pane,
                name: "..".to_string(),
            },
        ))
        .on_exit(Message::Sftp(
            crate::sftp::message::Message::SftpRowHoverExit {
                pane,
                name: "..".to_string(),
            },
        ));
    if let Some(msg) = on_double_click {
        area = area.on_double_click(msg);
    }
    area.into()
}

#[allow(
    clippy::too_many_arguments,
    reason = "the parameter list mirrors the layout this widget builds"
)]
fn view_file_row<'a>(
    entry: &'a FileEntry,
    on_click: Message,
    on_double_click: Option<Message>,
    pane: SftpPane,
    selected: bool,
    hovered: bool,
    selected_names: Vec<String>,
    layout: FileTableLayout,
    p: ThemePalette,
) -> Element<'a, Message> {
    let icon = file_entry_icon(entry);
    let icon_color = if selected {
        p.text_on_color
    } else {
        match entry.file_type {
            FileEntryType::Folder => p.accent,
            _ => p.text_sec,
        }
    };

    let name_text = entry.name.clone();
    let mut name_parts: Vec<Element<'a, Message>> = vec![
        container(
            text(char::from(icon))
                .font(LUCIDE_FONT)
                .size(15)
                .color(icon_color),
        )
        .width(16)
        .center_x(16)
        .into(),
    ];

    name_parts.push(file_name_text(name_text, layout, p, selected));

    let meta_color = if selected {
        p.text_on_color
    } else {
        p.text_dim
    };
    let meta_color_sec = if selected {
        p.text_on_color
    } else {
        p.text_sec
    };

    if layout.show_permissions {
        name_parts.push(
            text(entry.permissions.clone())
                .size(12)
                .color(meta_color)
                .width(Length::Fixed(74.0))
                .wrapping(iced::widget::text::Wrapping::None)
                .into(),
        );
    }

    let name_col =
        container(row(name_parts).spacing(6).align_y(iced::Alignment::Center)).clip(true);

    let mut cells: Vec<Element<'a, Message>> = vec![
        name_col
            .width(Length::FillPortion(layout.name_portion))
            .into(),
    ];

    if let Some(portion) = layout.date_portion {
        cells.push(
            text(entry.modified.clone())
                .size(13)
                .color(meta_color_sec)
                .wrapping(iced::widget::text::Wrapping::None)
                .width(Length::FillPortion(portion))
                .into(),
        );
    }

    if let Some(portion) = layout.size_portion {
        cells.push(
            text(entry.size.map_or_else(|| "-".to_string(), format_size))
                .size(13)
                .color(meta_color_sec)
                .wrapping(iced::widget::text::Wrapping::None)
                .width(Length::FillPortion(portion))
                .into(),
        );
    }

    if let Some(portion) = layout.kind_portion {
        cells.push(
            text(&entry.kind)
                .size(13)
                .color(meta_color_sec)
                .width(Length::FillPortion(portion))
                .ellipsis(text::Ellipsis::End)
                .wrapping(iced::widget::text::Wrapping::None)
                .into(),
        );
    }

    let content = row(cells)
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .padding(Padding::from([4.0, 6.0]));

    let row_widget = file_row_draggable(
        content,
        on_click,
        on_double_click,
        selected,
        hovered,
        pane,
        entry.name.clone(),
        matches!(
            entry.file_type,
            FileEntryType::File | FileEntryType::Symlink
        ),
        selected_names,
        p,
    );

    with_file_title(entry, row_widget, true, p)
}

/// Pick a Lucide icon for a file entry.
///
/// Folders and symlinks have a fixed icon. Regular files are mapped by
/// extension to a Lucide "file-*" icon where one exists, falling back to the
/// generic `File` icon when the type is not recognized.
fn file_entry_icon(entry: &FileEntry) -> lucide_icons::Icon {
    match entry.file_type {
        FileEntryType::Folder => lucide_icons::Icon::Folder,
        FileEntryType::Symlink => lucide_icons::Icon::Link,
        FileEntryType::File => {
            let ext = std::path::Path::new(&entry.name)
                .extension()
                .and_then(|extension| extension.to_str())
                .map(str::to_ascii_lowercase)
                .unwrap_or_default();

            match ext.as_str() {
                // Images
                "png" | "jpg" | "jpeg" | "gif" | "bmp" | "svg" | "webp" | "ico" | "tiff"
                | "tif" | "avif" => lucide_icons::Icon::FileImage,
                // Audio
                "mp3" | "wav" | "ogg" | "oga" | "flac" | "m4a" | "aac" | "opus" | "wma" | "mid"
                | "midi" => lucide_icons::Icon::FileAudio,
                // Video
                "mp4" | "mkv" | "avi" | "mov" | "webm" | "flv" | "wmv" | "m4v" | "mpg" | "mpeg"
                | "3gp" => lucide_icons::Icon::FileVideo2,
                // Archives / disk images
                "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "tbz2" | "xz" | "zst"
                | "lzma" | "br" | "jar" | "apk" | "deb" | "rpm" | "iso" | "dmg" => {
                    lucide_icons::Icon::FileArchive
                }
                // Source code / markup
                "rs" | "go" | "py" | "pyw" | "js" | "jsx" | "ts" | "tsx" | "java" | "c" | "h"
                | "cpp" | "hpp" | "cc" | "hh" | "cs" | "rb" | "php" | "swift" | "kt" | "kts"
                | "scala" | "lua" | "pl" | "r" | "dart" | "ex" | "exs" | "erl" | "hrl" | "hs"
                | "clj" | "cljs" | "cljc" | "groovy" | "vue" | "svelte" | "sql" | "html"
                | "htm" | "css" | "scss" | "sass" | "less" => lucide_icons::Icon::FileCode,
                // Structured data
                "json" | "jsonl" | "ndjson" | "geojson" => lucide_icons::Icon::FileBraces,
                // Config
                "yaml" | "yml" | "toml" | "ini" | "cfg" | "conf" | "config" | "properties" => {
                    lucide_icons::Icon::FileCog
                }
                // Documents / plain text
                "md" | "markdown" | "txt" | "text" | "log" | "rst" | "adoc" | "asciidoc"
                | "tex" | "rtf" | "pdf" | "doc" | "docx" | "odt" | "pages" => {
                    lucide_icons::Icon::FileText
                }
                // Spreadsheets / tabular data
                "csv" | "tsv" | "xls" | "xlsx" | "ods" | "numbers" => {
                    lucide_icons::Icon::FileSpreadsheet
                }
                // Presentations
                "ppt" | "pptx" | "odp" => lucide_icons::Icon::Presentation,
                // Databases
                "db" | "sqlite" | "sqlite3" | "mdb" | "accdb" => lucide_icons::Icon::Database,
                // Certificates / keys
                "pem" | "crt" | "cer" | "key" | "pub" | "p12" | "pfx" => {
                    lucide_icons::Icon::FileKey
                }
                // Fonts
                "ttf" | "otf" | "woff" | "woff2" | "eot" => lucide_icons::Icon::FileType,
                // Patches
                "diff" | "patch" => lucide_icons::Icon::FileDiff,
                // Shell scripts
                "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat" | "cmd" => {
                    lucide_icons::Icon::FileTerminal
                }
                _ => lucide_icons::Icon::File,
            }
        }
    }
}

fn file_name_text<'a>(
    name_text: String,
    layout: FileTableLayout,
    p: ThemePalette,
    selected: bool,
) -> Element<'a, Message> {
    let color = if selected {
        p.text_on_color
    } else {
        p.text_pri
    };
    text(name_text)
        .size(14)
        .color(color)
        .width(Length::Fixed(layout.file_name_width))
        .ellipsis(text::Ellipsis::End)
        .wrapping(iced::widget::text::Wrapping::None)
        .into()
}

fn with_file_title<'a>(
    entry: &'a FileEntry,
    row: Element<'a, Message>,
    show_title: bool,
    p: ThemePalette,
) -> Element<'a, Message> {
    if !show_title {
        return row;
    }

    cursor_tooltip_label(row, &entry.name, p, None).into()
}

#[allow(
    clippy::too_many_arguments,
    reason = "the parameter list mirrors the layout this widget builds"
)]
fn file_row_draggable<'a>(
    content: impl Into<Element<'a, Message>>,
    press_msg: Message,
    double_click_msg: Option<Message>,
    selected: bool,
    hovered: bool,
    pane: SftpPane,
    name: String,
    is_file: bool,
    selected_names: Vec<String>,
    p: ThemePalette,
) -> Element<'a, Message> {
    let inner = container(content)
        .width(Length::Fill)
        .clip(true)
        .style(move |_: &Theme| container::Style {
            background: if selected {
                Some(Background::Color(p.accent))
            } else if hovered {
                Some(Background::Color(p.sftp_row_hover))
            } else {
                None
            },
            text_color: Some(p.text_pri),
            border: Border {
                radius: 4.0.into(),
                ..Border::default()
            },
            ..Default::default()
        });

    let right_click_name = name.clone();
    let row = mouse_area(inner)
        .on_enter(Message::Sftp(
            crate::sftp::message::Message::SftpRowHoverEnter {
                pane,
                name: name.clone(),
            },
        ))
        .on_exit(Message::Sftp(
            crate::sftp::message::Message::SftpRowHoverExit {
                pane,
                name: name.clone(),
            },
        ))
        .on_right_press(Message::Sftp(
            crate::sftp::message::Message::SftpContextMenuOpen {
                pane,
                name: right_click_name,
            },
        ));

    let mut droppable = iced_drop::droppable(row)
        .drag_threshold(6.0)
        .drag_hide(false)
        .on_press(press_msg)
        .on_drop(move |_point, _bounds| {
            Message::Sftp(crate::sftp::message::Message::SftpFileDropped {
                pane,
                name: name.clone(),
                is_file,
                selected_names: selected_names.clone(),
            })
        });
    if let Some(msg) = double_click_msg {
        droppable = droppable.on_double_click(msg);
    }
    droppable.into()
}

fn file_table_layout(width: f32) -> FileTableLayout {
    let (name_portion, date_portion, size_portion, kind_portion, show_permissions) =
        if width < 320.0 {
            (1, None, None, None, false)
        } else if width < 420.0 {
            (5, None, Some(2), None, false)
        } else if width < 540.0 {
            (6, None, Some(2), Some(2), false)
        } else {
            (5, Some(3), Some(2), Some(2), true)
        };
    let file_name_width = file_name_available_width(
        width,
        name_portion,
        &[date_portion, size_portion, kind_portion],
        show_permissions,
    );

    FileTableLayout {
        name_portion,
        date_portion,
        size_portion,
        kind_portion,
        show_permissions,
        file_name_width,
    }
}

fn file_name_available_width(
    width: f32,
    name_portion: u16,
    other_portions: &[Option<u16>],
    show_permissions: bool,
) -> f32 {
    let total_portions = name_portion
        + other_portions
            .iter()
            .filter_map(|portion| *portion)
            .sum::<u16>();
    let name_column_width = width * f32::from(name_portion) / f32::from(total_portions);
    let reserved_width = if show_permissions { 128.0 } else { 42.0 };
    (name_column_width - reserved_width).clamp(40.0, 360.0)
}

#[cfg(test)]
mod tests {
    use super::{file_entry_icon, file_table_layout};
    use crate::core::data::{FileEntry, FileEntryType};
    use lucide_icons::Icon;

    fn entry(name: &str, file_type: FileEntryType) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            file_type,
            size: None,
            modified: String::new(),
            modified_epoch: None,
            permissions: String::new(),
            kind: String::new(),
        }
    }

    #[test]
    fn file_table_layout_prioritizes_names_on_narrow_widths() {
        let narrow = file_table_layout(300.0);
        let full = file_table_layout(640.0);

        assert!(narrow.date_portion.is_none());
        assert!(narrow.size_portion.is_none());
        assert!(!narrow.show_permissions);
        assert!(full.date_portion.is_some());
        assert!(full.show_permissions);
    }

    #[test]
    fn the_layout_crosses_each_breakpoint_in_order() {
        // Below 320px only the name column fits…
        let tiny = file_table_layout(319.0);
        assert!(tiny.date_portion.is_none() && tiny.size_portion.is_none());
        assert!(tiny.kind_portion.is_none() && !tiny.show_permissions);

        // …size joins at 320px…
        let small = file_table_layout(320.0);
        assert!(small.date_portion.is_none());
        assert!(small.size_portion.is_some() && small.kind_portion.is_none());
        assert!(!small.show_permissions);

        // …kind at 420px…
        let medium = file_table_layout(420.0);
        assert!(medium.date_portion.is_none());
        assert!(medium.kind_portion.is_some() && !medium.show_permissions);

        // …and the date plus permissions at 540px.
        let wide = file_table_layout(540.0);
        assert!(wide.date_portion.is_some() && wide.show_permissions);

        // The name column always keeps a readable minimum, however cramped the
        // window gets.
        for width in [120.0, 200.0, 319.0, 400.0, 900.0] {
            assert!(
                file_table_layout(width).file_name_width >= 40.0,
                "the name column at {width}px is too narrow to read"
            );
        }
    }

    #[test]
    fn folders_and_links_get_their_own_icon_whatever_the_name() {
        assert!(matches!(
            file_entry_icon(&entry("src", FileEntryType::Folder)),
            Icon::Folder
        ));
        assert!(matches!(
            file_entry_icon(&entry("src.rs", FileEntryType::Symlink)),
            Icon::Link
        ));
        assert!(
            matches!(
                file_entry_icon(&entry("archive.zip", FileEntryType::Symlink)),
                Icon::Link
            ),
            "a link is shown as a link, not as the type it points at"
        );
    }

    #[test]
    fn file_icons_follow_the_extension_case_insensitively() {
        for (name, expected) in [
            ("photo.PNG", Icon::FileImage),
            ("clip.mp4", Icon::FileVideo2),
            ("song.flac", Icon::FileAudio),
            ("bundle.tar.gz", Icon::FileArchive),
            ("main.rs", Icon::FileCode),
            ("data.json", Icon::FileBraces),
            ("config.TOML", Icon::FileCog),
            ("notes.md", Icon::FileText),
            ("table.csv", Icon::FileSpreadsheet),
            ("deck.pptx", Icon::Presentation),
            ("app.sqlite", Icon::Database),
            ("id_ed25519.pub", Icon::FileKey),
            ("Inter.woff2", Icon::FileType),
            ("fix.patch", Icon::FileDiff),
            ("deploy.sh", Icon::FileTerminal),
        ] {
            assert!(
                matches!(file_entry_icon(&entry(name, FileEntryType::File)), icon if icon as u32 == expected as u32),
                "{name} should be shown as {expected:?}"
            );
        }
    }

    #[test]
    fn unknown_or_missing_extensions_fall_back_to_the_generic_file_icon() {
        for name in ["README", "notes.unknownext", ".gitignore", "Makefile"] {
            assert!(
                matches!(
                    file_entry_icon(&entry(name, FileEntryType::File)),
                    Icon::File
                ),
                "{name} should use the generic file icon"
            );
        }
    }
}
