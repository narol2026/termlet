use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use crate::sftp::SftpPane;

/// One rendered breadcrumb and the directory it navigates to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BreadcrumbItem {
    label: String,
    target: String,
}

impl BreadcrumbItem {
    pub(crate) fn label(&self) -> &str {
        &self.label
    }

    pub(crate) fn target(&self) -> &str {
        &self.target
    }

    fn new(label: String, target: String) -> Self {
        Self { label, target }
    }
}

/// A root and its clickable descendant directories.
///
/// Local paths use the host platform's native [`Path`] components, while SSH
/// paths use POSIX slash semantics regardless of the client's platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Breadcrumb {
    root: BreadcrumbItem,
    directories: Vec<BreadcrumbItem>,
}

impl Breadcrumb {
    pub(crate) fn for_pane(path: &str, pane: SftpPane) -> Self {
        match pane {
            SftpPane::Local => Self::local(Path::new(path)),
            SftpPane::Remote => Self::remote(path),
        }
    }

    fn local(path: &Path) -> Self {
        let root_component_count = path
            .components()
            .take_while(|component| matches!(component, Component::Prefix(_) | Component::RootDir))
            .count();
        let root_path = path.components().take(root_component_count).fold(
            PathBuf::new(),
            |mut root, component| {
                root.push(component.as_os_str());
                root
            },
        );

        let root = if root_path.as_os_str().is_empty() {
            BreadcrumbItem::new(".".to_string(), ".".to_string())
        } else {
            BreadcrumbItem::new(
                local_root_label(&root_path),
                root_path.to_string_lossy().into_owned(),
            )
        };

        let mut current = root_path;
        let directories = path
            .components()
            .skip(root_component_count)
            .filter_map(|component| {
                let label = match component {
                    Component::Normal(name) => name,
                    Component::ParentDir => OsStr::new(".."),
                    Component::CurDir if current.as_os_str().is_empty() => return None,
                    Component::CurDir => OsStr::new("."),
                    Component::Prefix(_) | Component::RootDir => return None,
                };
                current.push(label);
                Some(BreadcrumbItem::new(
                    label.to_string_lossy().into_owned(),
                    current.to_string_lossy().into_owned(),
                ))
            })
            .collect();

        Self { root, directories }
    }

    fn remote(path: &str) -> Self {
        let absolute = path.starts_with('/');
        let root = if absolute {
            BreadcrumbItem::new("/".to_string(), "/".to_string())
        } else {
            BreadcrumbItem::new(".".to_string(), ".".to_string())
        };

        let mut current = if absolute {
            "/".to_string()
        } else {
            String::new()
        };
        let directories = path
            .split('/')
            .filter(|part| !part.is_empty())
            .map(|part| {
                if !current.is_empty() && !current.ends_with('/') {
                    current.push('/');
                }
                current.push_str(part);
                BreadcrumbItem::new(part.to_string(), current.clone())
            })
            .collect();

        Self { root, directories }
    }

    pub(crate) fn root(&self) -> &BreadcrumbItem {
        &self.root
    }

    pub(crate) fn directories(&self) -> &[BreadcrumbItem] {
        &self.directories
    }

    /// Target represented by the collapsed prefix after hiding `hidden_count`
    /// directories.
    pub(crate) fn target_after_hidden(&self, hidden_count: usize) -> &str {
        if hidden_count == 0 {
            return self.root.target();
        }
        self.directories
            .get(hidden_count - 1)
            .map_or_else(|| self.root.target(), BreadcrumbItem::target)
    }
}

fn local_root_label(root: &Path) -> String {
    let display = root.to_string_lossy();
    let label = display.trim_end_matches(['/', '\\']);
    if label.is_empty() {
        std::path::MAIN_SEPARATOR.to_string()
    } else {
        label.to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Breadcrumb, BreadcrumbItem};

    #[test]
    fn remote_path_uses_posix_ancestors() {
        let breadcrumb = Breadcrumb::remote("/home/user");

        assert_eq!(breadcrumb.root().label(), "/");
        assert_eq!(breadcrumb.root().target(), "/");
        assert_eq!(
            breadcrumb
                .directories()
                .iter()
                .map(|item| (item.label(), item.target()))
                .collect::<Vec<_>>(),
            [("home", "/home"), ("user", "/home/user")]
        );
        assert_eq!(breadcrumb.target_after_hidden(0), "/");
        assert_eq!(breadcrumb.target_after_hidden(1), "/home");
        assert_eq!(breadcrumb.target_after_hidden(2), "/home/user");
    }

    #[test]
    fn remote_relative_path_keeps_a_relative_root() {
        let breadcrumb = Breadcrumb::remote("work/app");

        assert_eq!(breadcrumb.root().label(), ".");
        assert_eq!(breadcrumb.root().target(), ".");
        assert_eq!(
            breadcrumb
                .directories()
                .iter()
                .map(|item| (item.label(), item.target()))
                .collect::<Vec<_>>(),
            [("work", "work"), ("app", "work/app")]
        );
    }

    #[test]
    fn local_relative_path_keeps_a_relative_root() {
        let breadcrumb = Breadcrumb::local(Path::new("work/app"));

        assert_eq!(breadcrumb.root().label(), ".");
        assert_eq!(breadcrumb.root().target(), ".");
        assert_eq!(
            breadcrumb
                .directories()
                .iter()
                .map(BreadcrumbItem::label)
                .collect::<Vec<_>>(),
            ["work", "app"]
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_local_path_uses_native_parent_targets() {
        let breadcrumb = Breadcrumb::local(Path::new(r"C:\Users\17538"));

        assert_eq!(breadcrumb.root().label(), "C:");
        assert_eq!(breadcrumb.root().target(), r"C:\");
        assert_eq!(
            breadcrumb
                .directories()
                .iter()
                .map(|item| (item.label(), item.target()))
                .collect::<Vec<_>>(),
            [("Users", r"C:\Users"), ("17538", r"C:\Users\17538")]
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_local_path_normalizes_forward_slashes_for_targets() {
        let breadcrumb = Breadcrumb::local(Path::new("D:/work/app"));

        assert_eq!(breadcrumb.root().label(), "D:");
        assert_eq!(breadcrumb.root().target(), r"D:\");
        assert_eq!(
            breadcrumb
                .directories()
                .iter()
                .map(|item| (item.label(), item.target()))
                .collect::<Vec<_>>(),
            [("work", r"D:\work"), ("app", r"D:\work\app")]
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_unc_path_uses_the_share_as_the_root() {
        let breadcrumb = Breadcrumb::local(Path::new(r"\\server\share\Users\me"));

        assert_eq!(breadcrumb.root().label(), r"\\server\share");
        assert_eq!(breadcrumb.root().target(), r"\\server\share\");
        assert_eq!(
            breadcrumb
                .directories()
                .iter()
                .map(BreadcrumbItem::label)
                .collect::<Vec<_>>(),
            ["Users", "me"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_local_path_uses_its_filesystem_root() {
        let breadcrumb = Breadcrumb::local(Path::new("/home/user"));

        assert_eq!(breadcrumb.root().label(), "/");
        assert_eq!(breadcrumb.root().target(), "/");
        assert_eq!(
            breadcrumb
                .directories()
                .iter()
                .map(|item| (item.label(), item.target()))
                .collect::<Vec<_>>(),
            [("home", "/home"), ("user", "/home/user")]
        );
    }
}
