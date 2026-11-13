use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Locale {
    #[serde(rename = "en")]
    En,
    #[serde(rename = "zh-CN")]
    ZhCN,
    #[serde(rename = "zh-TW")]
    ZhTW,
}

impl Locale {
    pub const ALL: &'static [Locale] = &[Locale::En, Locale::ZhCN, Locale::ZhTW];

    pub fn label(self) -> &'static str {
        match self {
            Locale::En => "English",
            Locale::ZhCN => "简体中文",
            Locale::ZhTW => "繁體中文",
        }
    }

    pub fn to_str(self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::ZhCN => "zh-CN",
            Locale::ZhTW => "zh-TW",
        }
    }

    pub fn from_str(s: &str) -> Option<Locale> {
        match s {
            "en" => Some(Locale::En),
            "zh-CN" => Some(Locale::ZhCN),
            "zh-TW" => Some(Locale::ZhTW),
            _ => None,
        }
    }
}
