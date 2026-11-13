// Source Code Pro font bytes embedded at compile time.
// https://github.com/adobe-fonts/source-code-pro
//
// The application UI deliberately bundles no typeface: it uses the system
// default sans-serif so the app feels native on every platform and we avoid
// redistributing a proprietary font. Source Code Pro stays embedded because
// terminal text needs a guaranteed monospace face that matches everywhere.

// Rust guideline compliant 2026-10-09

pub const SOURCE_CODE_PRO_REGULAR: &[u8] =
    include_bytes!("../../fonts/terminal/source-code-pro/SourceCodePro-Regular.ttf");
pub const SOURCE_CODE_PRO_BOLD: &[u8] =
    include_bytes!("../../fonts/terminal/source-code-pro/SourceCodePro-Bold.ttf");
pub const SOURCE_CODE_PRO_ITALIC: &[u8] =
    include_bytes!("../../fonts/terminal/source-code-pro/SourceCodePro-It.ttf");
pub const SOURCE_CODE_PRO_BOLD_ITALIC: &[u8] =
    include_bytes!("../../fonts/terminal/source-code-pro/SourceCodePro-BoldIt.ttf");

// Custom icon font generated from assets/icon-font/ SVGs via svgtofont.
pub const MY_ICONS_FONT_BYTES: &[u8] = include_bytes!("../../fonts/icons/my-icons.ttf");

pub mod my_icon {
    /// Unicode Private Use Area codepoints mapped by the my-icons font.
    pub const CENTOS: char = '\u{ea01}';
    pub const DEBIAN: char = '\u{ea02}';
    /// Reserved brand glyph; the login UI that used it was removed.
    #[allow(
        dead_code,
        reason = "reserved brand glyph; the UI that used it was removed"
    )]
    pub const GITHUB: char = '\u{ea03}';
    /// Reserved brand glyph; the login UI that used it was removed.
    #[allow(
        dead_code,
        reason = "reserved brand glyph; the UI that used it was removed"
    )]
    pub const GOOGLE: char = '\u{ea04}';
    pub const LINUX: char = '\u{ea05}';
    pub const MACOS: char = '\u{ea06}';
    pub const PANEL_RIGHT_OPEN: char = '\u{ea07}';
    pub const POINT: char = '\u{ea08}';
    pub const UBUNTU: char = '\u{ea09}';
    pub const WINDOWS: char = '\u{ea0a}';
}
