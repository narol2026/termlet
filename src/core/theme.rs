use iced::Color;
use iced_term::ColorPalette as TermColorPalette;

use crate::core::i18n::t_static;
use rust_i18n::t;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
    HighContrastDark,
    HighContrastLight,
    Dracula,
    Novel,
    Auto,
}

impl ThemeMode {
    /// Returns theme modes in the order shown by the right sidebar.
    #[must_use]
    pub const fn all() -> &'static [Self] {
        &[
            Self::Auto,
            Self::Dark,
            Self::Light,
            Self::HighContrastDark,
            Self::HighContrastLight,
            Self::Dracula,
            Self::Novel,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Dark => t_static(t!("theme.dark")),
            Self::Light => t_static(t!("theme.light")),
            Self::HighContrastDark => t_static(t!("theme.high_contrast_dark")),
            Self::HighContrastLight => t_static(t!("theme.high_contrast_light")),
            Self::Dracula => t_static(t!("theme.dracula")),
            Self::Novel => t_static(t!("theme.novel")),
            Self::Auto => t_static(t!("theme.auto")),
        }
    }

    pub fn icon(self) -> lucide_icons::Icon {
        match self {
            Self::Dark => lucide_icons::Icon::Moon,
            Self::Light => lucide_icons::Icon::Sun,
            Self::HighContrastDark => lucide_icons::Icon::Contrast,
            Self::HighContrastLight => lucide_icons::Icon::Contrast,
            Self::Dracula => lucide_icons::Icon::Moon,
            Self::Novel => lucide_icons::Icon::BookOpen,
            Self::Auto => lucide_icons::Icon::Monitor,
        }
    }

    pub fn to_str(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
            Self::HighContrastDark => "HighContrastDark",
            Self::HighContrastLight => "HighContrastLight",
            Self::Dracula => "Dracula",
            Self::Novel => "Novel",
            Self::Auto => "Auto",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Dark" => Some(Self::Dark),
            "Light" => Some(Self::Light),
            "HighContrastDark" => Some(Self::HighContrastDark),
            "HighContrastLight" => Some(Self::HighContrastLight),
            "Dracula" => Some(Self::Dracula),
            "Novel" => Some(Self::Novel),
            "Auto" => Some(Self::Auto),
            _ => None,
        }
    }

    /// Resolve the effective theme mode. `Auto` maps to `Dark` or `Light`
    /// based on the system preference; all other modes pass through unchanged.
    pub fn resolve(self, system_dark: bool) -> ThemeMode {
        match self {
            Self::Auto => {
                if system_dark {
                    Self::Dark
                } else {
                    Self::Light
                }
            }
            other => other,
        }
    }
}

/// Detect whether the operating system is using a dark colour scheme.
///
/// On Linux this queries the GNOME `gsettings` key and falls back to the
/// `GTK_THEME` environment variable.  On macOS it reads the
/// `AppleInterfaceStyle` user default.  Other platforms (including Windows)
/// return `false` — they can be extended later without changing the call
/// sites.
pub fn detect_system_dark_mode() -> bool {
    #[cfg(target_os = "linux")]
    {
        // Primary: GNOME / GTK colour-scheme key (available since GNOME 42).
        if let Ok(output) = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "color-scheme"])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.contains("prefer-dark") {
                return true;
            }
            if stdout.contains("default") {
                return false;
            }
        }
        // Fallback: check whether the GTK theme name hints at a dark variant.
        if let Ok(theme) = std::env::var("GTK_THEME") {
            return theme.to_lowercase().contains("dark");
        }
        false
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("defaults")
            .args(["read", "-g", "AppleInterfaceStyle"])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            return stdout.trim() == "Dark";
        }
        false
    }

    #[cfg(target_os = "windows")]
    {
        // A GUI-subsystem process has no console. Without CREATE_NO_WINDOW,
        // Windows briefly creates one for this console child at every startup.
        let mut command = std::process::Command::new("reg");
        command.args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "/v",
            "AppsUseLightTheme",
        ]);
        crate::core::process::suppress_console_window(&mut command);
        if let Ok(output) = command.output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // "AppsUseLightTheme    REG_DWORD    0x0" → dark
            // "AppsUseLightTheme    REG_DWORD    0x1" → light
            if stdout.contains("0x0") && !stdout.contains("0x00") {
                return true;
            }
        }
        false
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        false
    }
}

/// Return a best-effort dark-mode guess without spawning a subprocess.
///
/// Used only for the first frame while the real [`detect_system_dark_mode`]
/// result is being fetched in the background. It deliberately does not query
/// `gsettings`, `defaults`, or `reg`; on Linux it falls back to `GTK_THEME`.
pub fn detect_system_dark_mode_fast() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::env::var("GTK_THEME").is_ok_and(|theme| theme.to_lowercase().contains("dark"))
    }

    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

#[derive(Clone, Copy)]
pub struct ThemePalette {
    pub sidebar_bg: Color,
    pub content_bg: Color,
    pub card_bg: Color,
    pub card_hover: Color,
    pub item_hover: Color,
    pub item_active: Color,
    pub button_bg: Color,
    pub button_hover: Color,
    pub button_border: Color,
    pub button_border_hover: Color,
    pub accent: Color,
    pub accent_hover: Color,
    pub split_line: Color,
    pub text_pri: Color,
    pub text_sec: Color,
    pub text_dim: Color,
    pub text_on_color: Color,
    pub nav_active: Color,
    pub top_bar: Color,
    pub topbar_tab_inactive: Color,
    pub topbar_tab_active: Color,
    pub tab_close_hover: Color,
    pub sftp_row_hover: Color,
    pub border: Color,
    pub split_focus_border: Color,
    pub success: Color,
    pub terminal_bg: Color,
    /// Background for transient notices shown above terminal content.
    pub terminal_notice_bg: Color,
    pub surface: Color,
    pub stripe_bg: Color,
    pub tooltip_bg: Color,
    pub danger: Color,
    pub danger_hover: Color,
    pub warning: Color,
    pub purple: Color,
    pub backdrop: Color,
    pub card_shadow: Color,
    pub menu_shadow: Color,
    pub show_border: bool,
}

// Graphite Dark
// Neutral charcoal surfaces with a bright blue accent.
// Elevation rises: top_bar → sidebar → content → card → card_hover.
pub fn dark_palette() -> ThemePalette {
    ThemePalette {
        sidebar_bg: Color {
            r: 0.094,
            g: 0.094,
            b: 0.106,
            a: 1.0,
        }, // #18181B
        content_bg: Color {
            r: 0.118,
            g: 0.118,
            b: 0.129,
            a: 1.0,
        }, // #1E1E21
        card_bg: Color {
            r: 0.145,
            g: 0.145,
            b: 0.157,
            a: 1.0,
        }, // #252528
        card_hover: Color {
            r: 0.176,
            g: 0.176,
            b: 0.192,
            a: 1.0,
        }, // #2D2D31
        item_hover: Color {
            r: 0.176,
            g: 0.176,
            b: 0.192,
            a: 1.0,
        }, // #2D2D31 — same as card_hover
        item_active: Color {
            r: 0.208,
            g: 0.208,
            b: 0.227,
            a: 1.0,
        }, // #35353A
        button_bg: Color {
            r: 0.145,
            g: 0.145,
            b: 0.157,
            a: 1.0,
        }, // #252528 — same as card_bg
        button_hover: Color {
            r: 0.176,
            g: 0.176,
            b: 0.192,
            a: 1.0,
        }, // #2D2D31 — same as card_hover
        button_border: Color {
            r: 0.227,
            g: 0.227,
            b: 0.247,
            a: 1.0,
        }, // #3A3A3F
        button_border_hover: Color {
            r: 0.290,
            g: 0.290,
            b: 0.314,
            a: 1.0,
        }, // #4A4A50
        accent: Color {
            r: 0.290,
            g: 0.541,
            b: 0.957,
            a: 1.0,
        }, // #4A8AF4
        accent_hover: Color {
            r: 0.350,
            g: 0.541,
            b: 0.957,
            a: 1.0,
        }, // accent.r + 0.06
        split_line: Color {
            r: 0.290,
            g: 0.541,
            b: 0.957,
            a: 1.0,
        }, // #4A8AF4 — same as accent
        text_pri: Color {
            r: 1.000,
            g: 1.000,
            b: 1.000,
            a: 1.0,
        }, // #FFFFFF — pure white primary text
        text_sec: Color {
            r: 0.627,
            g: 0.627,
            b: 0.659,
            a: 1.0,
        }, // #A0A0A8
        text_dim: Color {
            r: 0.431,
            g: 0.431,
            b: 0.463,
            a: 1.0,
        }, // #6E6E76
        text_on_color: Color {
            r: 1.000,
            g: 1.000,
            b: 1.000,
            a: 1.0,
        }, // #FFFFFF
        nav_active: Color {
            r: 0.208,
            g: 0.208,
            b: 0.227,
            a: 1.0,
        }, // #35353A
        top_bar: Color {
            r: 0.078,
            g: 0.078,
            b: 0.086,
            a: 1.0,
        }, // #141416
        topbar_tab_inactive: Color {
            r: 0.145,
            g: 0.145,
            b: 0.157,
            a: 1.0,
        }, // #252528
        topbar_tab_active: Color {
            r: 0.208,
            g: 0.208,
            b: 0.227,
            a: 1.0,
        }, // #35353A
        tab_close_hover: Color {
            r: 0.290,
            g: 0.290,
            b: 0.314,
            a: 1.0,
        }, // #4A4A50 — lighter than active tab, visible hover circle
        sftp_row_hover: Color {
            r: 0.176,
            g: 0.176,
            b: 0.192,
            a: 1.0,
        }, // #2D2D31
        border: Color {
            r: 0.180,
            g: 0.180,
            b: 0.196,
            a: 1.0,
        }, // #2E2E32
        split_focus_border: Color {
            r: 0.478,
            g: 0.478,
            b: 0.502,
            a: 1.0,
        }, // #7A7A80
        success: Color {
            r: 0.341,
            g: 0.718,
            b: 0.478,
            a: 1.0,
        }, // #57B77A
        terminal_bg: Color {
            r: 0.090,
            g: 0.090,
            b: 0.102,
            a: 1.0,
        }, // #17171A
        terminal_notice_bg: Color {
            r: 0.208,
            g: 0.208,
            b: 0.227,
            a: 1.0,
        }, // #35353A
        surface: Color {
            r: 0.129,
            g: 0.129,
            b: 0.141,
            a: 1.0,
        }, // #212124
        stripe_bg: Color {
            r: 0.1315,
            g: 0.1315,
            b: 0.143,
            a: 1.0,
        }, // #222224 — 50% content_bg / card_bg blend
        tooltip_bg: Color {
            r: 0.145,
            g: 0.145,
            b: 0.157,
            a: 1.0,
        }, // #252528 — same as card_bg
        danger: Color {
            r: 0.878,
            g: 0.333,
            b: 0.416,
            a: 1.0,
        }, // #E0556A
        danger_hover: Color {
            r: 0.938,
            g: 0.333,
            b: 0.416,
            a: 1.0,
        }, // danger.r + 0.06
        warning: Color {
            r: 0.878,
            g: 0.620,
            b: 0.180,
            a: 1.0,
        }, // #E09E2E
        purple: Color {
            r: 0.557,
            g: 0.361,
            b: 0.749,
            a: 1.0,
        }, // #8E5CBF
        backdrop: Color {
            r: 0.000,
            g: 0.000,
            b: 0.000,
            a: 0.55,
        },
        card_shadow: Color {
            r: 0.000,
            g: 0.000,
            b: 0.000,
            a: 0.35,
        },
        menu_shadow: Color {
            r: 0.000,
            g: 0.000,
            b: 0.000,
            a: 0.45,
        },
        show_border: false,
    }
}

// VSCode-inspired Light
// Mirrors the dark theme's elevation hierarchy while following VSCode's Light
// Modern palette.  Elevation rises: top_bar (chrome, lowest) → sidebar →
// content (white, highest).  Borders are visible — in light themes, elevation
// differences alone are too subtle to separate regions.
pub fn light_palette() -> ThemePalette {
    ThemePalette {
        // elevation stack (chrome recedes, content comes forward)
        top_bar: Color {
            r: 0.973,
            g: 0.973,
            b: 0.973,
            a: 1.0,
        }, // #F8F8F8 — lowest chrome, mirrors dark top_bar (#141416)
        sidebar_bg: Color {
            r: 0.988,
            g: 0.988,
            b: 0.988,
            a: 1.0,
        }, // #FCFCFC — barely off-white sidebar
        content_bg: Color {
            r: 1.000,
            g: 1.000,
            b: 1.000,
            a: 1.0,
        }, // #FFFFFF — highest elevation (editor area)
        card_bg: Color {
            r: 1.000,
            g: 1.000,
            b: 1.000,
            a: 1.0,
        }, // #FFFFFF — cards sit on content
        card_hover: Color {
            r: 0.941,
            g: 0.941,
            b: 0.941,
            a: 1.0,
        }, // #F0F0F0 — hover darkens slightly
        item_hover: Color {
            r: 0.941,
            g: 0.941,
            b: 0.941,
            a: 1.0,
        }, // #F0F0F0 — same as card_hover
        item_active: Color {
            r: 0.910,
            g: 0.910,
            b: 0.910,
            a: 1.0,
        }, // #E8E8E8
        // interactive surfaces
        button_bg: Color {
            r: 1.000,
            g: 1.000,
            b: 1.000,
            a: 1.0,
        }, // #FFFFFF — same as card_bg
        button_hover: Color {
            r: 0.941,
            g: 0.941,
            b: 0.941,
            a: 1.0,
        }, // #F0F0F0 — same as card_hover
        button_border: Color {
            r: 0.831,
            g: 0.831,
            b: 0.831,
            a: 1.0,
        }, // #D4D4D4
        button_border_hover: Color {
            r: 0.678,
            g: 0.678,
            b: 0.678,
            a: 1.0,
        }, // #ADADAD
        // accent (VSCode blue)
        accent: Color {
            r: 0.000,
            g: 0.471,
            b: 0.831,
            a: 1.0,
        }, // #0078D4 — VSCode focus / accent blue
        accent_hover: Color {
            r: 0.063,
            g: 0.431,
            b: 0.745,
            a: 1.0,
        }, // #106EBE — darker on hover
        split_line: Color {
            r: 0.000,
            g: 0.471,
            b: 0.831,
            a: 1.0,
        }, // #0078D4 — same as accent
        // text hierarchy
        text_pri: Color {
            r: 0.122,
            g: 0.122,
            b: 0.122,
            a: 1.0,
        }, // #1F1F1F — near-black primary
        text_sec: Color {
            r: 0.380,
            g: 0.380,
            b: 0.380,
            a: 1.0,
        }, // #616161 — secondary grey
        text_dim: Color {
            r: 0.600,
            g: 0.600,
            b: 0.600,
            a: 1.0,
        }, // #999999 — dim / placeholder
        text_on_color: Color {
            r: 1.000,
            g: 1.000,
            b: 1.000,
            a: 1.0,
        }, // #FFFFFF — on accent / danger
        // navigation
        nav_active: Color {
            r: 0.894,
            g: 0.902,
            b: 0.945,
            a: 1.0,
        }, // #E4E6F1 — VSCode list.inactiveSelection (subtle blue tint)
        topbar_tab_inactive: Color {
            r: 0.973,
            g: 0.973,
            b: 0.973,
            a: 1.0,
        }, // #F8F8F8 — blends into top_bar chrome
        topbar_tab_active: Color {
            r: 1.000,
            g: 1.000,
            b: 1.000,
            a: 1.0,
        }, // #FFFFFF — active tab = content level
        tab_close_hover: Color {
            r: 0.831,
            g: 0.831,
            b: 0.831,
            a: 1.0,
        }, // #D4D4D4 — darker than active tab, visible hover circle
        sftp_row_hover: Color {
            r: 0.941,
            g: 0.941,
            b: 0.941,
            a: 1.0,
        }, // #F0F0F0 — same as card_hover
        // chrome & surfaces
        border: Color {
            r: 0.906,
            g: 0.906,
            b: 0.906,
            a: 1.0,
        }, // #E7E7E7 — VSCode panel border (subtle but visible)
        split_focus_border: Color {
            r: 0.000,
            g: 0.471,
            b: 0.831,
            a: 1.0,
        }, // #0078D4 — same as accent
        terminal_bg: Color {
            r: 1.000,
            g: 1.000,
            b: 1.000,
            a: 1.0,
        }, // #FFFFFF
        terminal_notice_bg: Color {
            r: 1.000,
            g: 1.000,
            b: 1.000,
            a: 1.0,
        }, // #FFFFFF
        surface: Color {
            r: 0.980,
            g: 0.980,
            b: 0.980,
            a: 1.0,
        }, // #FAFAFA — slightly off-white surface
        stripe_bg: Color {
            r: 1.000,
            g: 1.000,
            b: 1.000,
            a: 1.0,
        }, // #FFFFFF — content_bg and card_bg are equal
        tooltip_bg: Color {
            r: 1.000,
            g: 1.000,
            b: 1.000,
            a: 1.0,
        }, // #FFFFFF — same as card_bg
        // semantic colours (VSCode palette)
        success: Color {
            r: 0.220,
            g: 0.541,
            b: 0.204,
            a: 1.0,
        }, // #388A34 — VSCode terminal green
        danger: Color {
            r: 0.804,
            g: 0.192,
            b: 0.192,
            a: 1.0,
        }, // #CD3131 — VSCode terminal red
        danger_hover: Color {
            r: 0.659,
            g: 0.000,
            b: 0.000,
            a: 1.0,
        }, // #A80000 — darker red on hover
        warning: Color {
            r: 0.749,
            g: 0.533,
            b: 0.012,
            a: 1.0,
        }, // #BF8803 — amber warning
        purple: Color {
            r: 0.533,
            g: 0.239,
            b: 0.702,
            a: 1.0,
        }, // #883DB3 — VSCode magenta family
        // overlays
        backdrop: Color {
            r: 0.000,
            g: 0.000,
            b: 0.000,
            a: 0.30,
        },
        card_shadow: Color {
            r: 0.000,
            g: 0.000,
            b: 0.000,
            a: 0.06,
        },
        menu_shadow: Color {
            r: 0.000,
            g: 0.000,
            b: 0.000,
            a: 0.10,
        },
        show_border: true, // VSCode light themes rely on borders for separation
    }
}

// High Contrast Dark
pub fn high_contrast_dark_palette() -> ThemePalette {
    ThemePalette {
        sidebar_bg: Color {
            r: 0.000,
            g: 0.000,
            b: 0.000,
            a: 1.0,
        },
        content_bg: Color {
            r: 0.000,
            g: 0.000,
            b: 0.000,
            a: 1.0,
        }, // #000000 — HighContrastDark
        card_bg: Color {
            r: 0.078,
            g: 0.078,
            b: 0.078,
            a: 1.0,
        },
        card_hover: Color {
            r: 0.125,
            g: 0.125,
            b: 0.125,
            a: 1.0,
        },
        item_hover: Color {
            r: 0.125,
            g: 0.125,
            b: 0.125,
            a: 1.0,
        },
        item_active: Color {
            r: 0.165,
            g: 0.165,
            b: 0.165,
            a: 1.0,
        }, // #2A2A2A
        button_bg: Color {
            r: 0.125,
            g: 0.125,
            b: 0.125,
            a: 1.0,
        },
        button_hover: Color {
            r: 0.125,
            g: 0.125,
            b: 0.125,
            a: 1.0,
        },
        button_border: Color {
            r: 0.267,
            g: 0.267,
            b: 0.267,
            a: 1.0,
        }, // #444444
        button_border_hover: Color {
            r: 0.400,
            g: 0.400,
            b: 0.400,
            a: 1.0,
        }, // #666666
        accent: Color {
            r: 0.243,
            g: 0.467,
            b: 0.894,
            a: 1.0,
        },
        accent_hover: Color {
            r: 0.303,
            g: 0.467,
            b: 0.894,
            a: 1.0,
        },
        split_line: Color {
            r: 0.243,
            g: 0.467,
            b: 0.894,
            a: 1.0,
        }, // same as accent
        text_pri: Color {
            r: 1.000,
            g: 1.000,
            b: 1.000,
            a: 1.0,
        },
        text_sec: Color {
            r: 0.851,
            g: 0.851,
            b: 0.851,
            a: 1.0,
        },
        text_dim: Color {
            r: 0.600,
            g: 0.600,
            b: 0.600,
            a: 1.0,
        },
        text_on_color: Color {
            r: 1.000,
            g: 1.000,
            b: 1.000,
            a: 1.0,
        },
        nav_active: Color {
            r: 0.125,
            g: 0.125,
            b: 0.125,
            a: 1.0,
        },
        top_bar: Color {
            r: 0.000,
            g: 0.000,
            b: 0.000,
            a: 1.0,
        },
        topbar_tab_inactive: Color {
            r: 0.1,
            g: 0.1,
            b: 0.1,
            a: 1.0,
        },
        topbar_tab_active: Color {
            r: 0.25,
            g: 0.25,
            b: 0.25,
            a: 1.0,
        },
        tab_close_hover: Color {
            r: 0.353,
            g: 0.353,
            b: 0.353,
            a: 1.0,
        }, // #5A5A5A — lighter than active tab, visible hover circle
        sftp_row_hover: Color {
            r: 0.125,
            g: 0.125,
            b: 0.125,
            a: 1.0,
        },
        border: Color {
            r: 0.400,
            g: 0.400,
            b: 0.400,
            a: 1.0,
        },
        split_focus_border: Color {
            r: 0.800,
            g: 0.800,
            b: 0.800,
            a: 1.0,
        }, // #CCCCCC
        success: Color {
            r: 0.298,
            g: 0.859,
            b: 0.478,
            a: 1.0,
        },
        terminal_bg: Color {
            r: 0.000,
            g: 0.000,
            b: 0.000,
            a: 1.0,
        },
        terminal_notice_bg: Color {
            r: 0.250,
            g: 0.250,
            b: 0.250,
            a: 1.0,
        }, // #404040
        surface: Color {
            r: 0.078,
            g: 0.078,
            b: 0.078,
            a: 0.0,
        },
        stripe_bg: Color {
            r: 0.039,
            g: 0.039,
            b: 0.039,
            a: 1.0,
        }, // #0A0A0A — 50% content_bg / card_bg blend
        tooltip_bg: Color {
            r: 0.078,
            g: 0.078,
            b: 0.078,
            a: 1.0,
        }, // same as card_bg
        danger: Color {
            r: 1.000,
            g: 0.400,
            b: 0.400,
            a: 1.0,
        },
        danger_hover: Color {
            r: 1.000,
            g: 0.400,
            b: 0.400,
            a: 1.0,
        },
        warning: Color {
            r: 1.000,
            g: 0.753,
            b: 0.000,
            a: 1.0,
        }, // #FFC000
        purple: Color {
            r: 0.753,
            g: 0.612,
            b: 1.000,
            a: 1.0,
        }, // #C09CFF
        backdrop: Color {
            r: 0.000,
            g: 0.000,
            b: 0.000,
            a: 0.55,
        },
        card_shadow: Color {
            r: 0.000,
            g: 0.000,
            b: 0.000,
            a: 0.35,
        },
        menu_shadow: Color {
            r: 0.000,
            g: 0.000,
            b: 0.000,
            a: 0.45,
        },
        show_border: true,
    }
}

// High Contrast Light
pub fn high_contrast_light_palette() -> ThemePalette {
    ThemePalette {
        sidebar_bg: Color {
            r: 1.000,
            g: 1.000,
            b: 1.000,
            a: 1.0,
        },
        content_bg: Color {
            r: 1.000,
            g: 1.000,
            b: 1.000,
            a: 1.0,
        }, // #FFFFFF — HighContrastLight
        card_bg: Color {
            r: 1.000,
            g: 1.000,
            b: 1.000,
            a: 1.0,
        },
        card_hover: Color {
            r: 0.910,
            g: 0.929,
            b: 0.949,
            a: 1.0,
        }, // #E8EDF2 — subtle blue-gray, matches tab hover
        item_hover: Color {
            r: 0.910,
            g: 0.929,
            b: 0.949,
            a: 1.0,
        }, // #E8EDF2 — same as card_hover
        item_active: Color {
            r: 0.871,
            g: 0.890,
            b: 0.918,
            a: 1.0,
        }, // #DEE3EA
        button_bg: Color {
            r: 0.910,
            g: 0.929,
            b: 0.949,
            a: 1.0,
        },
        button_hover: Color {
            r: 0.910,
            g: 0.929,
            b: 0.949,
            a: 1.0,
        }, // #E8EDF2 — same as card_hover
        button_border: Color {
            r: 0.533,
            g: 0.533,
            b: 0.533,
            a: 1.0,
        }, // #888888
        button_border_hover: Color {
            r: 0.400,
            g: 0.400,
            b: 0.400,
            a: 1.0,
        }, // #666666
        accent: Color {
            r: 0.157,
            g: 0.365,
            b: 0.800,
            a: 1.0,
        }, // #285DCC
        accent_hover: Color {
            r: 0.217,
            g: 0.365,
            b: 0.800,
            a: 1.0,
        },
        split_line: Color {
            r: 0.157,
            g: 0.365,
            b: 0.800,
            a: 1.0,
        }, // #285DCC — same as accent
        text_pri: Color {
            r: 0.000,
            g: 0.000,
            b: 0.000,
            a: 1.0,
        },
        text_sec: Color {
            r: 0.200,
            g: 0.200,
            b: 0.200,
            a: 1.0,
        },
        text_dim: Color {
            r: 0.349,
            g: 0.349,
            b: 0.349,
            a: 1.0,
        },
        text_on_color: Color {
            r: 1.000,
            g: 1.000,
            b: 1.000,
            a: 1.0,
        },
        nav_active: Color {
            r: 0.910,
            g: 0.929,
            b: 0.949,
            a: 1.0,
        }, // #E8EDF2 — subtle blue-gray
        top_bar: Color {
            r: 1.000,
            g: 1.000,
            b: 1.000,
            a: 1.0,
        }, // #FFFFFF — HighContrastLight
        topbar_tab_inactive: Color {
            r: 1.000,
            g: 1.000,
            b: 1.000,
            a: 1.0,
        }, // blends into white top_bar
        topbar_tab_active: Color {
            r: 0.910,
            g: 0.929,
            b: 0.949,
            a: 1.0,
        }, // #E8EDF2 — subtle blue-gray
        tab_close_hover: Color {
            r: 0.761,
            g: 0.788,
            b: 0.824,
            a: 1.0,
        }, // #C2C9D2 — darker than active tab, visible hover circle
        sftp_row_hover: Color {
            r: 0.910,
            g: 0.929,
            b: 0.949,
            a: 1.0,
        }, // #E8EDF2
        border: Color {
            r: 0.400,
            g: 0.400,
            b: 0.400,
            a: 1.0,
        }, // #666666
        split_focus_border: Color {
            r: 0.200,
            g: 0.200,
            b: 0.200,
            a: 1.0,
        }, // #333333
        success: Color {
            r: 0.106,
            g: 0.533,
            b: 0.200,
            a: 1.0,
        },
        terminal_bg: Color {
            r: 1.000,
            g: 1.000,
            b: 1.000,
            a: 1.0,
        },
        terminal_notice_bg: Color {
            r: 0.910,
            g: 0.929,
            b: 0.949,
            a: 1.0,
        }, // #E8EDF2
        surface: Color {
            r: 0.929,
            g: 0.929,
            b: 0.929,
            a: 0.0,
        },
        stripe_bg: Color {
            r: 1.000,
            g: 1.000,
            b: 1.000,
            a: 1.0,
        }, // #FFFFFF — content_bg and card_bg are equal
        tooltip_bg: Color {
            r: 1.000,
            g: 1.000,
            b: 1.000,
            a: 1.0,
        }, // same as card_bg
        danger: Color {
            r: 0.800,
            g: 0.118,
            b: 0.118,
            a: 1.0,
        },
        danger_hover: Color {
            r: 0.860,
            g: 0.118,
            b: 0.118,
            a: 1.0,
        },
        warning: Color {
            r: 0.820,
            g: 0.510,
            b: 0.090,
            a: 1.0,
        }, // #D18217
        purple: Color {
            r: 0.361,
            g: 0.114,
            b: 0.549,
            a: 1.0,
        }, // #5C1D8C
        backdrop: Color {
            r: 0.000,
            g: 0.000,
            b: 0.000,
            a: 0.25,
        },
        card_shadow: Color {
            r: 0.000,
            g: 0.000,
            b: 0.000,
            a: 0.08,
        },
        menu_shadow: Color {
            r: 0.000,
            g: 0.000,
            b: 0.000,
            a: 0.15,
        },
        show_border: true,
    }
}

// Dracula
// The iconic purple-accented dark theme.
// Background hierarchy: top_bar → sidebar → content → card → card_hover.
pub fn dracula_palette() -> ThemePalette {
    ThemePalette {
        sidebar_bg: Color {
            r: 0.118,
            g: 0.122,
            b: 0.161,
            a: 1.0,
        }, // #1E1F29
        content_bg: Color {
            r: 0.157,
            g: 0.165,
            b: 0.212,
            a: 1.0,
        }, // #282A36
        card_bg: Color {
            r: 0.267,
            g: 0.278,
            b: 0.353,
            a: 1.0,
        }, // #44475A
        card_hover: Color {
            r: 0.322,
            g: 0.333,
            b: 0.443,
            a: 1.0,
        }, // #525571
        item_hover: Color {
            r: 0.322,
            g: 0.333,
            b: 0.443,
            a: 1.0,
        }, // #525571 — same as card_hover
        item_active: Color {
            r: 0.369,
            g: 0.388,
            b: 0.502,
            a: 1.0,
        }, // #5E6380
        button_bg: Color {
            r: 0.267,
            g: 0.278,
            b: 0.353,
            a: 1.0,
        }, // #44475A — same as card_bg
        button_hover: Color {
            r: 0.322,
            g: 0.333,
            b: 0.443,
            a: 1.0,
        }, // #525571 — same as card_hover
        button_border: Color::TRANSPARENT,
        button_border_hover: Color::TRANSPARENT,
        accent: Color {
            r: 0.741,
            g: 0.576,
            b: 0.976,
            a: 1.0,
        }, // #BD93F9
        accent_hover: Color {
            r: 0.792,
            g: 0.647,
            b: 0.980,
            a: 1.0,
        }, // #CAA5FA
        split_line: Color {
            r: 0.741,
            g: 0.576,
            b: 0.976,
            a: 1.0,
        }, // #BD93F9 — same as accent
        text_pri: Color {
            r: 0.973,
            g: 0.973,
            b: 0.949,
            a: 1.0,
        }, // #F8F8F2
        text_sec: Color {
            r: 0.698,
            g: 0.706,
            b: 0.761,
            a: 1.0,
        }, // #B2B4C2
        text_dim: Color {
            r: 0.384,
            g: 0.447,
            b: 0.643,
            a: 1.0,
        }, // #6272A4
        text_on_color: Color {
            r: 0.973,
            g: 0.973,
            b: 0.949,
            a: 1.0,
        }, // #F8F8F2
        nav_active: Color {
            r: 0.369,
            g: 0.388,
            b: 0.502,
            a: 1.0,
        }, // #5E6380
        top_bar: Color {
            r: 0.102,
            g: 0.106,
            b: 0.149,
            a: 1.0,
        }, // #1A1B26
        topbar_tab_inactive: Color {
            r: 0.267,
            g: 0.278,
            b: 0.353,
            a: 1.0,
        }, // #44475A
        topbar_tab_active: Color {
            r: 0.369,
            g: 0.388,
            b: 0.502,
            a: 1.0,
        }, // #5E6380
        tab_close_hover: Color {
            r: 0.431,
            g: 0.451,
            b: 0.580,
            a: 1.0,
        }, // #6E7394 — lighter than active tab, visible hover circle
        sftp_row_hover: Color {
            r: 0.322,
            g: 0.333,
            b: 0.443,
            a: 1.0,
        }, // #525571
        border: Color {
            r: 0.239,
            g: 0.247,
            b: 0.345,
            a: 1.0,
        }, // #3D3F58
        split_focus_border: Color {
            r: 0.784,
            g: 0.800,
            b: 0.847,
            a: 1.0,
        }, // #C8CCD8
        success: Color {
            r: 0.314,
            g: 0.980,
            b: 0.482,
            a: 1.0,
        }, // #50FA7B
        terminal_bg: Color {
            r: 0.157,
            g: 0.165,
            b: 0.212,
            a: 1.0,
        }, // #282A36
        terminal_notice_bg: Color {
            r: 0.369,
            g: 0.388,
            b: 0.502,
            a: 1.0,
        }, // #5E6380
        surface: Color {
            r: 0.176,
            g: 0.184,
            b: 0.239,
            a: 1.0,
        }, // #2D2F3D
        stripe_bg: Color {
            r: 0.212,
            g: 0.2215,
            b: 0.2825,
            a: 1.0,
        }, // #363848 — 50% content_bg / card_bg blend
        tooltip_bg: Color {
            r: 0.267,
            g: 0.278,
            b: 0.353,
            a: 1.0,
        }, // #44475A — same as card_bg
        danger: Color {
            r: 1.000,
            g: 0.333,
            b: 0.333,
            a: 1.0,
        }, // #FF5555
        danger_hover: Color {
            r: 1.000,
            g: 0.467,
            b: 0.467,
            a: 1.0,
        }, // #FF7777
        warning: Color {
            r: 1.000,
            g: 0.722,
            b: 0.424,
            a: 1.0,
        }, // #FFB86C
        purple: Color {
            r: 0.741,
            g: 0.576,
            b: 0.976,
            a: 1.0,
        }, // #BD93F9
        backdrop: Color {
            r: 0.000,
            g: 0.000,
            b: 0.000,
            a: 0.55,
        },
        card_shadow: Color {
            r: 0.000,
            g: 0.000,
            b: 0.000,
            a: 0.35,
        },
        menu_shadow: Color {
            r: 0.000,
            g: 0.000,
            b: 0.000,
            a: 0.45,
        },
        show_border: false,
    }
}

// Novel (warm paper / reading)
// A low-contrast, book-page inspired light theme. Warm cream surfaces with an
// ink-brown foreground and a muted terracotta accent. The terminal surface
// reuses the classic "Novel" scheme (`#dfdbc3` paper background, `#3b2322`
// ink foreground) so the UI chrome and the shell feel like one reading surface.
pub fn novel_palette() -> ThemePalette {
    ThemePalette {
        top_bar: Color {
            r: 0.925,
            g: 0.890,
            b: 0.808,
            a: 1.0,
        }, // #ECE3CE — lowest chrome, warm cream
        sidebar_bg: Color {
            r: 0.953,
            g: 0.922,
            b: 0.847,
            a: 1.0,
        }, // #F3EBD8 — paper sidebar
        content_bg: Color {
            r: 0.984,
            g: 0.965,
            b: 0.910,
            a: 1.0,
        }, // #FBF6E8 — highest elevation (reading surface)
        card_bg: Color {
            r: 0.984,
            g: 0.965,
            b: 0.910,
            a: 1.0,
        }, // #FBF6E8 — cards sit on content
        card_hover: Color {
            r: 0.941,
            g: 0.906,
            b: 0.812,
            a: 1.0,
        }, // #F0E7CF — hover darkens slightly
        item_hover: Color {
            r: 0.941,
            g: 0.906,
            b: 0.812,
            a: 1.0,
        }, // #F0E7CF — same as card_hover
        item_active: Color {
            r: 0.902,
            g: 0.855,
            b: 0.745,
            a: 1.0,
        }, // #E6DABE
        button_bg: Color {
            r: 0.984,
            g: 0.965,
            b: 0.910,
            a: 1.0,
        }, // #FBF6E8 — same as card_bg
        button_hover: Color {
            r: 0.941,
            g: 0.906,
            b: 0.812,
            a: 1.0,
        }, // #F0E7CF — same as card_hover
        button_border: Color {
            r: 0.863,
            g: 0.820,
            b: 0.710,
            a: 1.0,
        }, // #DCD1B5
        button_border_hover: Color {
            r: 0.737,
            g: 0.678,
            b: 0.549,
            a: 1.0,
        }, // #BCAD8C
        accent: Color {
            r: 0.612,
            g: 0.357,
            b: 0.200,
            a: 1.0,
        }, // #9C5B33 — muted terracotta
        accent_hover: Color {
            r: 0.522,
            g: 0.290,
            b: 0.153,
            a: 1.0,
        }, // #854A27 — darker on hover
        split_line: Color {
            r: 0.612,
            g: 0.357,
            b: 0.200,
            a: 1.0,
        }, // #9C5B33 — same as accent
        text_pri: Color {
            r: 0.227,
            g: 0.180,
            b: 0.145,
            a: 1.0,
        }, // #3A2E25 — warm ink
        text_sec: Color {
            r: 0.435,
            g: 0.384,
            b: 0.329,
            a: 1.0,
        }, // #6F6254
        text_dim: Color {
            r: 0.580,
            g: 0.529,
            b: 0.467,
            a: 1.0,
        }, // #948777
        text_on_color: Color {
            r: 0.984,
            g: 0.965,
            b: 0.910,
            a: 1.0,
        }, // #FBF6E8
        nav_active: Color {
            r: 0.902,
            g: 0.855,
            b: 0.745,
            a: 1.0,
        }, // #E6DABE
        topbar_tab_inactive: Color {
            r: 0.953,
            g: 0.922,
            b: 0.847,
            a: 1.0,
        }, // #F3EBD8
        topbar_tab_active: Color {
            r: 0.984,
            g: 0.965,
            b: 0.910,
            a: 1.0,
        }, // #FBF6E8 — active tab matches content
        tab_close_hover: Color {
            r: 0.831,
            g: 0.776,
            b: 0.643,
            a: 1.0,
        }, // #D4C6A4 — visible hover circle
        sftp_row_hover: Color {
            r: 0.941,
            g: 0.906,
            b: 0.812,
            a: 1.0,
        }, // #F0E7CF
        border: Color {
            r: 0.875,
            g: 0.831,
            b: 0.722,
            a: 1.0,
        }, // #DFD4B8
        split_focus_border: Color {
            r: 0.545,
            g: 0.486,
            b: 0.388,
            a: 1.0,
        }, // #8B7C63
        success: Color {
            r: 0.290,
            g: 0.522,
            b: 0.278,
            a: 1.0,
        }, // #4A8547
        terminal_bg: Color {
            r: 0.875,
            g: 0.859,
            b: 0.765,
            a: 1.0,
        }, // #DFDBC3 — classic Novel paper
        terminal_notice_bg: Color {
            r: 0.984,
            g: 0.965,
            b: 0.910,
            a: 1.0,
        }, // #FBF6E8
        surface: Color {
            r: 0.961,
            g: 0.933,
            b: 0.859,
            a: 1.0,
        }, // #F5EEDB — resting panel tone, one notch lighter than card_hover
        stripe_bg: Color {
            r: 0.984,
            g: 0.965,
            b: 0.910,
            a: 1.0,
        }, // #FBF6E8 — content_bg and card_bg are equal
        tooltip_bg: Color {
            r: 0.984,
            g: 0.965,
            b: 0.910,
            a: 1.0,
        }, // #FBF6E8 — same as card_bg
        danger: Color {
            r: 0.698,
            g: 0.227,
            b: 0.196,
            a: 1.0,
        }, // #B23A32
        danger_hover: Color {
            r: 0.765,
            g: 0.290,
            b: 0.259,
            a: 1.0,
        }, // #C34A42
        warning: Color {
            r: 0.753,
            g: 0.478,
            b: 0.165,
            a: 1.0,
        }, // #C07A2A
        purple: Color {
            r: 0.541,
            g: 0.357,
            b: 0.659,
            a: 1.0,
        }, // #8A5BA8
        backdrop: Color {
            r: 0.000,
            g: 0.000,
            b: 0.000,
            a: 0.30,
        },
        card_shadow: Color {
            r: 0.000,
            g: 0.000,
            b: 0.000,
            a: 0.06,
        },
        menu_shadow: Color {
            r: 0.000,
            g: 0.000,
            b: 0.000,
            a: 0.10,
        },
        show_border: true,
    }
}

pub fn palette(mode: ThemeMode) -> ThemePalette {
    match mode {
        ThemeMode::Dark => dark_palette(),
        ThemeMode::Light => light_palette(),
        ThemeMode::HighContrastDark => high_contrast_dark_palette(),
        ThemeMode::HighContrastLight => high_contrast_light_palette(),
        ThemeMode::Dracula => dracula_palette(),
        ThemeMode::Novel => novel_palette(),
        ThemeMode::Auto => {
            if detect_system_dark_mode() {
                dark_palette()
            } else {
                light_palette()
            }
        }
    }
}

// Terminal palettes

pub fn term_dark_palette() -> TermColorPalette {
    TermColorPalette {
        foreground: String::from("#dcdee0"),
        background: String::from("#17171a"),
        black: String::from("#1e1e21"),
        red: String::from("#e06c75"),
        green: String::from("#98c379"),
        yellow: String::from("#e5c07b"),
        blue: String::from("#61afef"),
        magenta: String::from("#c678dd"),
        cyan: String::from("#56b6c2"),
        white: String::from("#dcdee0"),
        bright_black: String::from("#5c5c63"),
        bright_red: String::from("#f0818a"),
        bright_green: String::from("#acd68a"),
        bright_yellow: String::from("#f0d290"),
        bright_blue: String::from("#7dc4f4"),
        bright_magenta: String::from("#da90e8"),
        bright_cyan: String::from("#71c9d4"),
        bright_white: String::from("#f0f0f2"),
        bright_foreground: None,
        dim_foreground: String::from("#83838a"),
        dim_black: String::from("#0f0f11"),
        dim_red: String::from("#8c3f44"),
        dim_green: String::from("#5e794a"),
        dim_yellow: String::from("#8f764b"),
        dim_blue: String::from("#3a698e"),
        dim_magenta: String::from("#7a4687"),
        dim_cyan: String::from("#346d75"),
        dim_white: String::from("#85858c"),
    }
}

pub fn term_light_palette() -> TermColorPalette {
    // VSCode Light Modern terminal ANSI colours — high contrast on white,
    // matching the UI accent and semantic colours.
    TermColorPalette {
        foreground: String::from("#1f1f1f"),
        background: String::from("#ffffff"),
        black: String::from("#000000"),
        red: String::from("#cd3131"),
        green: String::from("#0dbc79"),
        yellow: String::from("#e5e510"),
        blue: String::from("#2472c8"),
        magenta: String::from("#bc3fbc"),
        cyan: String::from("#11a8cd"),
        white: String::from("#d4d4d4"),
        bright_black: String::from("#767676"),
        bright_red: String::from("#f14c4c"),
        bright_green: String::from("#23d18b"),
        bright_yellow: String::from("#f5f543"),
        bright_blue: String::from("#3b8eea"),
        bright_magenta: String::from("#d670d6"),
        bright_cyan: String::from("#29b8db"),
        bright_white: String::from("#e5e5e5"),
        bright_foreground: None,
        dim_foreground: String::from("#999999"),
        dim_black: String::from("#000000"),
        dim_red: String::from("#a31515"),
        dim_green: String::from("#0a8f5c"),
        dim_yellow: String::from("#b8b800"),
        dim_blue: String::from("#1b57a0"),
        dim_magenta: String::from("#8f2f8f"),
        dim_cyan: String::from("#0d7f9a"),
        dim_white: String::from("#b3b3b3"),
    }
}

pub fn term_high_contrast_dark_palette() -> TermColorPalette {
    TermColorPalette {
        foreground: String::from("#ffffff"),
        background: String::from("#000000"),
        black: String::from("#000000"),
        red: String::from("#ff4444"),
        green: String::from("#44ff44"),
        yellow: String::from("#ffff44"),
        blue: String::from("#4488ff"),
        magenta: String::from("#ff44ff"),
        cyan: String::from("#44ffff"),
        white: String::from("#ffffff"),
        bright_black: String::from("#808080"),
        bright_red: String::from("#ff6666"),
        bright_green: String::from("#66ff66"),
        bright_yellow: String::from("#ffff66"),
        bright_blue: String::from("#6699ff"),
        bright_magenta: String::from("#ff66ff"),
        bright_cyan: String::from("#66ffff"),
        bright_white: String::from("#ffffff"),
        bright_foreground: None,
        dim_foreground: String::from("#c0c0c0"),
        dim_black: String::from("#000000"),
        dim_red: String::from("#cc0000"),
        dim_green: String::from("#00cc00"),
        dim_yellow: String::from("#cccc00"),
        dim_blue: String::from("#0044cc"),
        dim_magenta: String::from("#cc00cc"),
        dim_cyan: String::from("#00cccc"),
        dim_white: String::from("#cccccc"),
    }
}

pub fn term_high_contrast_light_palette() -> TermColorPalette {
    // Colors sourced from VSCode's High Contrast Light theme terminal ANSI registry.
    // Dim variants are derived by darkening normal colors (~0.75× per channel).
    TermColorPalette {
        foreground: String::from("#000000"),
        background: String::from("#ffffff"),
        black: String::from("#292929"),
        red: String::from("#cd3131"),
        green: String::from("#136c13"),
        yellow: String::from("#949800"),
        blue: String::from("#0451a5"),
        magenta: String::from("#bc05bc"),
        cyan: String::from("#0598bc"),
        white: String::from("#555555"),
        bright_black: String::from("#666666"),
        bright_red: String::from("#cd3131"),
        bright_green: String::from("#00bc00"),
        bright_yellow: String::from("#b5ba00"),
        bright_blue: String::from("#0451a5"),
        bright_magenta: String::from("#bc05bc"),
        bright_cyan: String::from("#0598bc"),
        bright_white: String::from("#a5a5a5"),
        bright_foreground: None,
        dim_foreground: String::from("#666666"),
        dim_black: String::from("#1f1f1f"),
        dim_red: String::from("#9a2525"),
        dim_green: String::from("#0e510e"),
        dim_yellow: String::from("#6f7200"),
        dim_blue: String::from("#033d7c"),
        dim_magenta: String::from("#8d048d"),
        dim_cyan: String::from("#04728d"),
        dim_white: String::from("#404040"),
    }
}

pub fn term_dracula_palette() -> TermColorPalette {
    TermColorPalette {
        foreground: String::from("#f8f8f2"),
        background: String::from("#282a36"),
        black: String::from("#21222c"),
        red: String::from("#ff5555"),
        green: String::from("#50fa7b"),
        yellow: String::from("#f1fa8c"),
        blue: String::from("#bd93f9"),
        magenta: String::from("#ff79c6"),
        cyan: String::from("#8be9fd"),
        white: String::from("#f8f8f2"),
        bright_black: String::from("#6272a4"),
        bright_red: String::from("#ff6e6e"),
        bright_green: String::from("#69ff94"),
        bright_yellow: String::from("#ffffa5"),
        bright_blue: String::from("#d6acff"),
        bright_magenta: String::from("#ff92df"),
        bright_cyan: String::from("#a4ffff"),
        bright_white: String::from("#ffffff"),
        bright_foreground: None,
        dim_foreground: String::from("#6272a4"),
        dim_black: String::from("#191a21"),
        dim_red: String::from("#aa3939"),
        dim_green: String::from("#38af56"),
        dim_yellow: String::from("#a9af62"),
        dim_blue: String::from("#8467ae"),
        dim_magenta: String::from("#b3558a"),
        dim_cyan: String::from("#62a3b1"),
        dim_white: String::from("#aeaeaa"),
    }
}

pub fn term_novel_palette() -> TermColorPalette {
    // Classic "Novel" terminal scheme — a warm, low-contrast paper palette.
    // Sourced from the original OS X Terminal / iTerm2 color set.
    TermColorPalette {
        foreground: String::from("#3b2322"),
        background: String::from("#dfdbc3"),
        black: String::from("#000000"),
        red: String::from("#cc0000"),
        green: String::from("#009600"),
        yellow: String::from("#d06b00"),
        blue: String::from("#0000cc"),
        magenta: String::from("#cc00cc"),
        cyan: String::from("#0087cc"),
        white: String::from("#cccccc"),
        bright_black: String::from("#808080"),
        bright_red: String::from("#cc0000"),
        bright_green: String::from("#009600"),
        bright_yellow: String::from("#d06b00"),
        bright_blue: String::from("#0000cc"),
        bright_magenta: String::from("#cc00cc"),
        bright_cyan: String::from("#0087cc"),
        bright_white: String::from("#ffffff"),
        bright_foreground: None,
        dim_foreground: String::from("#73635a"),
        dim_black: String::from("#000000"),
        dim_red: String::from("#8e2a19"),
        dim_green: String::from("#006b00"),
        dim_yellow: String::from("#94500f"),
        dim_blue: String::from("#00008e"),
        dim_magenta: String::from("#8e008e"),
        dim_cyan: String::from("#005f8e"),
        dim_white: String::from("#a3a3a3"),
    }
}

pub fn term_palette(mode: ThemeMode) -> TermColorPalette {
    match mode {
        ThemeMode::Dark => term_dark_palette(),
        ThemeMode::Light => term_light_palette(),
        ThemeMode::HighContrastDark => term_high_contrast_dark_palette(),
        ThemeMode::HighContrastLight => term_high_contrast_light_palette(),
        ThemeMode::Dracula => term_dracula_palette(),
        ThemeMode::Novel => term_novel_palette(),
        ThemeMode::Auto => {
            if detect_system_dark_mode() {
                term_dark_palette()
            } else {
                term_light_palette()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The modes a user can pick in Settings, in sidebar order. `Auto` is
    /// deliberately absent: it is a resolver, not a palette of its own.
    const CONCRETE_MODES: [ThemeMode; 6] = [
        ThemeMode::Dark,
        ThemeMode::Light,
        ThemeMode::HighContrastDark,
        ThemeMode::HighContrastLight,
        ThemeMode::Dracula,
        ThemeMode::Novel,
    ];

    /// Modes whose surfaces must read as "dark" (text lighter than background).
    const DARK_MODES: [ThemeMode; 3] = [
        ThemeMode::Dark,
        ThemeMode::HighContrastDark,
        ThemeMode::Dracula,
    ];

    /// Modes whose surfaces must read as "light" (text darker than background).
    const LIGHT_MODES: [ThemeMode; 3] = [
        ThemeMode::Light,
        ThemeMode::HighContrastLight,
        ThemeMode::Novel,
    ];

    /// The palette each concrete mode is documented to use, so the dispatch in
    /// [`palette`] is checked against the named palettes rather than only
    /// against itself.
    fn named_palette(mode: ThemeMode) -> ThemePalette {
        match mode {
            ThemeMode::Dark => dark_palette(),
            ThemeMode::Light => light_palette(),
            ThemeMode::HighContrastDark => high_contrast_dark_palette(),
            ThemeMode::HighContrastLight => high_contrast_light_palette(),
            ThemeMode::Dracula => dracula_palette(),
            ThemeMode::Novel => novel_palette(),
            ThemeMode::Auto => unreachable!("Auto resolves to a concrete mode"),
        }
    }

    /// The terminal palette a concrete mode is documented to use.
    fn named_term_palette(mode: ThemeMode) -> TermColorPalette {
        match mode {
            ThemeMode::Dark => term_dark_palette(),
            ThemeMode::Light => term_light_palette(),
            ThemeMode::HighContrastDark => term_high_contrast_dark_palette(),
            ThemeMode::HighContrastLight => term_high_contrast_light_palette(),
            ThemeMode::Dracula => term_dracula_palette(),
            ThemeMode::Novel => term_novel_palette(),
            ThemeMode::Auto => unreachable!("Auto resolves to a concrete mode"),
        }
    }

    /// WCAG 2.1 relative luminance.
    fn luminance(color: Color) -> f32 {
        fn linear(channel: f32) -> f32 {
            if channel <= 0.03928 {
                channel / 12.92
            } else {
                ((channel + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
    }

    /// WCAG 2.1 contrast ratio between two opaque colours (1.0 … 21.0).
    fn contrast_ratio(a: Color, b: Color) -> f32 {
        let (a, b) = (luminance(a), luminance(b));
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// Parse the `#rrggbb` strings handed to `iced_term` at startup. Anything
    /// else is silently invisible in the terminal, so the palette tests parse
    /// every colour instead of trusting the literals.
    fn hex_color(value: &str) -> Color {
        let digits = value
            .strip_prefix('#')
            .unwrap_or_else(|| panic!("{value} is missing its leading '#'"));
        assert_eq!(digits.len(), 6, "{value} is not a #rrggbb colour");
        let byte = |index: usize| {
            u8::from_str_radix(&digits[index..index + 2], 16)
                .unwrap_or_else(|error| panic!("{value} is not a #rrggbb colour: {error}"))
        };
        Color::from_rgb8(byte(0), byte(2), byte(4))
    }

    /// Every colour a terminal palette carries, named so a failure points at
    /// the field rather than at an index.
    fn term_colors(palette: &TermColorPalette) -> Vec<(&'static str, &str)> {
        let mut colors = vec![
            ("foreground", palette.foreground.as_str()),
            ("background", palette.background.as_str()),
            ("black", palette.black.as_str()),
            ("red", palette.red.as_str()),
            ("green", palette.green.as_str()),
            ("yellow", palette.yellow.as_str()),
            ("blue", palette.blue.as_str()),
            ("magenta", palette.magenta.as_str()),
            ("cyan", palette.cyan.as_str()),
            ("white", palette.white.as_str()),
            ("bright_black", palette.bright_black.as_str()),
            ("bright_red", palette.bright_red.as_str()),
            ("bright_green", palette.bright_green.as_str()),
            ("bright_yellow", palette.bright_yellow.as_str()),
            ("bright_blue", palette.bright_blue.as_str()),
            ("bright_magenta", palette.bright_magenta.as_str()),
            ("bright_cyan", palette.bright_cyan.as_str()),
            ("bright_white", palette.bright_white.as_str()),
            ("dim_foreground", palette.dim_foreground.as_str()),
            ("dim_black", palette.dim_black.as_str()),
            ("dim_red", palette.dim_red.as_str()),
            ("dim_green", palette.dim_green.as_str()),
            ("dim_yellow", palette.dim_yellow.as_str()),
            ("dim_blue", palette.dim_blue.as_str()),
            ("dim_magenta", palette.dim_magenta.as_str()),
            ("dim_cyan", palette.dim_cyan.as_str()),
            ("dim_white", palette.dim_white.as_str()),
        ];
        if let Some(bright_foreground) = palette.bright_foreground.as_deref() {
            colors.push(("bright_foreground", bright_foreground));
        }
        colors
    }

    #[test]
    fn mode_names_round_trip_through_their_persisted_form() {
        let mut names: Vec<&str> = Vec::new();
        for mode in CONCRETE_MODES.into_iter().chain([ThemeMode::Auto]) {
            let name = mode.to_str();
            assert_eq!(
                ThemeMode::from_str(name),
                Some(mode),
                "{name} did not survive a save/load round trip"
            );
            assert!(!names.contains(&name), "{name} is used by two modes");
            names.push(name);
        }

        // Preferences hold the persisted names, so near-misses must not load as
        // some default theme.
        assert_eq!(ThemeMode::from_str("dark"), None);
        assert_eq!(ThemeMode::from_str("Dracula "), None);
        assert_eq!(ThemeMode::from_str(""), None);
    }

    #[test]
    fn every_mode_has_its_own_label_and_an_icon() {
        let mut labels: Vec<&str> = Vec::new();
        for mode in CONCRETE_MODES.into_iter().chain([ThemeMode::Auto]) {
            let label = mode.label();
            assert!(!label.is_empty(), "{mode:?} has no label");
            assert!(!labels.contains(&label), "{label} labels two modes");
            labels.push(label);
        }

        // `lucide_icons::Icon` has no `PartialEq`, so the mapping is matched
        // structurally. Auto is a system-following choice rather than a palette,
        // and the rest match the look they apply.
        assert!(matches!(
            ThemeMode::Auto.icon(),
            lucide_icons::Icon::Monitor
        ));
        assert!(matches!(
            ThemeMode::Novel.icon(),
            lucide_icons::Icon::BookOpen
        ));
        assert!(matches!(ThemeMode::Dark.icon(), lucide_icons::Icon::Moon));
        assert!(matches!(
            ThemeMode::HighContrastDark.icon(),
            lucide_icons::Icon::Contrast
        ));
        assert!(matches!(
            ThemeMode::HighContrastLight.icon(),
            lucide_icons::Icon::Contrast
        ));
        assert!(matches!(
            ThemeMode::Dracula.icon(),
            lucide_icons::Icon::Moon
        ));
        assert!(matches!(ThemeMode::Light.icon(), lucide_icons::Icon::Sun));
    }

    #[test]
    fn auto_follows_the_system_and_others_pass_through() {
        assert_eq!(ThemeMode::Auto.resolve(true), ThemeMode::Dark);
        assert_eq!(ThemeMode::Auto.resolve(false), ThemeMode::Light);

        for mode in CONCRETE_MODES {
            assert_eq!(mode.resolve(true), mode, "{mode:?} must ignore the OS");
            assert_eq!(mode.resolve(false), mode, "{mode:?} must ignore the OS");
        }
    }

    #[test]
    fn palette_dispatch_returns_the_named_palette() {
        for mode in CONCRETE_MODES {
            let dispatched = palette(mode);
            let named = named_palette(mode);
            assert_eq!(dispatched.content_bg, named.content_bg, "{mode:?}");
            assert_eq!(dispatched.accent, named.accent, "{mode:?}");
            assert_eq!(dispatched.show_border, named.show_border, "{mode:?}");
        }

        // Auto picks one of the two system-following palettes; which one depends
        // on the desktop, so only membership is asserted here.
        let auto = palette(ThemeMode::Auto);
        assert!(
            auto.content_bg == dark_palette().content_bg
                || auto.content_bg == light_palette().content_bg,
            "Auto resolved to a palette that follows neither system setting"
        );
    }

    #[test]
    fn every_theme_keeps_body_text_readable_on_its_background() {
        for mode in CONCRETE_MODES {
            let p = named_palette(mode);

            // WCAG AAA for primary text, AA for secondary text.
            let primary = contrast_ratio(p.text_pri, p.content_bg);
            assert!(primary >= 7.0, "{mode:?} primary text is {primary:.2}:1");

            let secondary = contrast_ratio(p.text_sec, p.content_bg);
            assert!(
                secondary >= 4.5,
                "{mode:?} secondary text is {secondary:.2}:1"
            );

            assert_eq!(
                p.content_bg.a, 1.0,
                "{mode:?} content background is see-through"
            );
            assert_eq!(p.text_pri.a, 1.0, "{mode:?} primary text is translucent");
        }
    }

    #[test]
    fn dark_themes_put_light_text_on_dark_surfaces_and_light_themes_invert_that() {
        for mode in DARK_MODES {
            let p = named_palette(mode);
            assert!(
                luminance(p.text_pri) > luminance(p.content_bg),
                "{mode:?} is classified dark but its text is not lighter than its background"
            );
            assert!(
                luminance(p.content_bg) < 0.25,
                "{mode:?} background is too bright to read as a dark theme"
            );
        }

        for mode in LIGHT_MODES {
            let p = named_palette(mode);
            assert!(
                luminance(p.text_pri) < luminance(p.content_bg),
                "{mode:?} is classified light but its text is not darker than its background"
            );
            assert!(
                luminance(p.content_bg) > 0.6,
                "{mode:?} background is too dark to read as a light theme"
            );
        }
    }

    #[test]
    fn surfaces_rise_from_the_top_bar_to_the_cards() {
        // The documented elevation order is top_bar → sidebar → content → card.
        // Light themes flatten parts of that ramp by design, so the order is
        // non-decreasing; the point is that a raised surface never ends up
        // *darker* than the surface it sits on.
        for mode in CONCRETE_MODES {
            let p = named_palette(mode);
            let ramp = [
                ("top_bar", p.top_bar),
                ("sidebar_bg", p.sidebar_bg),
                ("content_bg", p.content_bg),
                ("card_bg", p.card_bg),
            ];
            for pair in ramp.windows(2) {
                let (lower_name, lower) = pair[0];
                let (upper_name, upper) = pair[1];
                assert!(
                    luminance(upper) >= luminance(lower) - f32::EPSILON,
                    "{mode:?}: {upper_name} is darker than the {lower_name} it sits on"
                );
            }
        }
    }

    #[test]
    fn high_contrast_themes_make_secondary_text_actually_high_contrast() {
        for (regular, high_contrast) in [
            (ThemeMode::Dark, ThemeMode::HighContrastDark),
            (ThemeMode::Light, ThemeMode::HighContrastLight),
        ] {
            let regular_palette = named_palette(regular);
            let hc_palette = named_palette(high_contrast);

            let regular_dim = contrast_ratio(regular_palette.text_dim, regular_palette.content_bg);
            let hc_dim = contrast_ratio(hc_palette.text_dim, hc_palette.content_bg);

            // The regular themes trade dim text down for calm surfaces…
            assert!(
                regular_dim < 4.5,
                "{regular:?} dim text is {regular_dim:.2}:1, which is already AA"
            );
            // …while the high-contrast variants keep even dim text at AAA.
            assert!(
                hc_dim >= 6.5,
                "{high_contrast:?} dim text is only {hc_dim:.2}:1"
            );
            assert!(
                hc_dim > regular_dim,
                "{high_contrast:?} does not improve on {regular:?}"
            );

            let hc_border = contrast_ratio(hc_palette.border, hc_palette.content_bg);
            let regular_border = contrast_ratio(regular_palette.border, regular_palette.content_bg);
            assert!(
                hc_border > regular_border,
                "{high_contrast:?} borders are not stronger than {regular:?}"
            );
        }

        // `show_border` is what keeps a card visible when its fill is close to
        // the surface behind it: both high-contrast themes switch it on for
        // definition, the light themes (whose card and content fills are nearly
        // identical) need it, and the two dark neutral themes do not.
        for mode in [
            ThemeMode::HighContrastDark,
            ThemeMode::HighContrastLight,
            ThemeMode::Light,
            ThemeMode::Novel,
        ] {
            assert!(named_palette(mode).show_border, "{mode:?} lost its borders");
        }
        assert!(!dark_palette().show_border);
        assert!(!dracula_palette().show_border);
    }

    #[test]
    fn terminal_palette_dispatch_returns_the_named_palette() {
        for mode in CONCRETE_MODES {
            let dispatched = term_palette(mode);
            let named = named_term_palette(mode);
            assert_eq!(dispatched.foreground, named.foreground, "{mode:?}");
            assert_eq!(dispatched.background, named.background, "{mode:?}");
            assert_eq!(dispatched.blue, named.blue, "{mode:?}");
            assert_eq!(dispatched.dim_cyan, named.dim_cyan, "{mode:?}");
            assert_eq!(
                dispatched.bright_foreground, named.bright_foreground,
                "{mode:?}"
            );
        }

        let auto = term_palette(ThemeMode::Auto);
        assert!(
            auto.background == term_dark_palette().background
                || auto.background == term_light_palette().background,
            "Auto resolved to a terminal palette that follows neither system setting"
        );
    }

    #[test]
    fn auto_keeps_the_terminal_and_ui_on_the_same_side_of_the_switch() {
        // A dark UI next to a light terminal (or the reverse) is the bug this
        // guards: the two `Auto` arms read the system setting independently.
        let ui_is_dark = palette(ThemeMode::Auto).content_bg == dark_palette().content_bg;
        let term_is_dark =
            term_palette(ThemeMode::Auto).background == term_dark_palette().background;
        assert_eq!(ui_is_dark, term_is_dark);
    }

    #[test]
    fn every_terminal_colour_is_a_parseable_hex_string() {
        for mode in CONCRETE_MODES {
            for (name, value) in term_colors(&named_term_palette(mode)) {
                let _ = hex_color(value);
                assert!(
                    value.starts_with('#'),
                    "{mode:?} {name} ({value}) is not a hex colour"
                );
            }
        }
    }

    #[test]
    fn terminal_text_reads_on_the_terminal_background() {
        for mode in CONCRETE_MODES {
            let p = named_term_palette(mode);
            let ratio = contrast_ratio(hex_color(&p.foreground), hex_color(&p.background));
            assert!(
                ratio >= 4.5,
                "{mode:?} terminal foreground is only {ratio:.2}:1 on its background"
            );
            assert_ne!(
                p.foreground, p.background,
                "{mode:?} renders text in the background colour"
            );
        }
    }

    /// The eight normal ANSI slots, paired with the field each one comes from.
    fn ansi_ramp(p: &TermColorPalette) -> [(&'static str, &str); 8] {
        [
            ("black", p.black.as_str()),
            ("red", p.red.as_str()),
            ("green", p.green.as_str()),
            ("yellow", p.yellow.as_str()),
            ("blue", p.blue.as_str()),
            ("magenta", p.magenta.as_str()),
            ("cyan", p.cyan.as_str()),
            ("white", p.white.as_str()),
        ]
    }

    #[test]
    fn terminal_ansi_ramp_slots_stay_distinct() {
        for mode in CONCRETE_MODES {
            let palette = named_term_palette(mode);
            let ramp = ansi_ramp(&palette);
            for (index, (name, value)) in ramp.iter().enumerate() {
                for (other_name, other) in &ramp[index + 1..] {
                    assert_ne!(
                        value, other,
                        "{mode:?} gives {name} and {other_name} the same colour"
                    );
                }
            }
        }
    }

    #[test]
    fn terminal_ansi_slots_are_not_painted_in_the_background_colour() {
        for mode in CONCRETE_MODES {
            let p = named_term_palette(mode);

            // An ANSI slot matching the background renders its text invisible,
            // so every slot except `black` must differ from it. `black` is
            // exempt on purpose: the high-contrast dark palette maps it onto
            // the background, which is the VS Code high-contrast convention.
            //
            // No contrast floor is asserted here: the light themes reuse VS
            // Code's ANSI ramp, where yellow on white is intentionally faint.
            let slots = ansi_ramp(&p)
                .into_iter()
                .filter(|(name, _)| *name != "black");
            for (name, value) in slots {
                assert_ne!(
                    value, p.background,
                    "{mode:?} draws {name} text in the background colour"
                );
            }
        }
    }
}
