pub(crate) fn shell_escape(path: &str) -> String {
    let escaped = path.replace('\'', "'\\''");
    format!("'{escaped}'")
}

/// Split a remote path into its parent directory and final name segment.
pub(super) fn remote_parent_and_name(path: &str) -> (String, String) {
    match path.rsplit_once('/') {
        Some((parent, name)) if !name.is_empty() => (
            if parent.is_empty() {
                "/".to_string()
            } else {
                parent.to_string()
            },
            name.to_string(),
        ),
        _ => ("/".to_string(), path.to_string()),
    }
}

#[cfg(test)]
mod shell_escape_tests {
    use super::shell_escape;

    #[test]
    fn escapes_shell_metacharacters() {
        // Spaces, globs, $(), backticks must all stay literal inside
        // the single quotes — only `'` itself needs the '\'' dance.
        assert_eq!(shell_escape("my dir"), "'my dir'");
        assert_eq!(
            shell_escape("a;b$(rm -rf /)`touch /tmp/x`"),
            "'a;b$(rm -rf /)`touch /tmp/x`'"
        );
        assert_eq!(shell_escape("it's"), r"'it'\''s'");
        assert_eq!(shell_escape("/"), "'/'");
    }
}

#[cfg(test)]
mod remote_parent_and_name_tests {
    use super::remote_parent_and_name;

    #[test]
    fn a_path_splits_into_its_parent_and_last_segment() {
        assert_eq!(
            remote_parent_and_name("/srv/app/notes.txt"),
            ("/srv/app".to_string(), "notes.txt".to_string())
        );
        assert_eq!(
            remote_parent_and_name("/srv/app"),
            ("/srv".to_string(), "app".to_string())
        );
        // A name directly under the root keeps the root as its parent rather
        // than an empty string.
        assert_eq!(
            remote_parent_and_name("/notes.txt"),
            ("/".to_string(), "notes.txt".to_string())
        );
        // A path with no separator at all is a name in the current directory.
        assert_eq!(
            remote_parent_and_name("notes.txt"),
            ("/".to_string(), "notes.txt".to_string())
        );
    }

    #[test]
    fn a_path_with_no_name_segment_is_returned_untouched() {
        // Callers pass entries from a directory listing, which always have a
        // name segment; a trailing separator leaves nothing to split off, and
        // guessing there would be worse than handing the path back as it is.
        assert_eq!(
            remote_parent_and_name("/srv/app/"),
            ("/".to_string(), "/srv/app/".to_string())
        );
    }
}
