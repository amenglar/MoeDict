use gpui::{Hsla, hsla};

use crate::ast::{FontRole, SemanticColor};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemeId {
    #[default]
    Light,
    Dark,
    Sepia,
}

impl ThemeId {
    pub fn label(self) -> &'static str {
        match self {
            Self::Light => "浅色",
            Self::Dark => "深色",
            Self::Sepia => "护眼",
        }
    }

    pub fn all() -> &'static [Self] {
        &[Self::Light, Self::Dark, Self::Sepia]
    }

    pub fn from_settings(value: &str, system_dark: bool) -> Self {
        match value {
            "light" => Self::Light,
            "dark" => Self::Dark,
            "sepia" => Self::Sepia,
            _ if system_dark => Self::Dark,
            _ => Self::Light,
        }
    }

    pub fn setting_value(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::Sepia => "sepia",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Colors {
    pub bg: Hsla,
    pub panel: Hsla,
    pub elevated: Hsla,
    pub text: Hsla,
    pub secondary: Hsla,
    pub muted: Hsla,
    pub border: Hsla,
    pub accent: Hsla,
    pub headword: Hsla,
    pub phonetic: Hsla,
    pub pos: Hsla,
    pub example: Hsla,
    pub link: Hsla,
    pub warning: Hsla,
    pub note_bg: Hsla,
    pub note_border: Hsla,
    pub example_bg: Hsla,
    pub selection: Hsla,
}

fn c(h: f32, s: f32, l: f32, a: f32) -> Hsla {
    hsla(h / 360.0, s, l, a)
}

impl Colors {
    pub fn for_theme(id: ThemeId) -> Self {
        match id {
            ThemeId::Light => Self {
                bg: c(40.0, 0.20, 0.99, 1.0),
                panel: c(40.0, 0.16, 0.97, 1.0),
                elevated: c(40.0, 0.14, 1.0, 1.0),
                text: c(220.0, 0.16, 0.16, 1.0),
                secondary: c(220.0, 0.10, 0.34, 1.0),
                muted: c(220.0, 0.08, 0.52, 1.0),
                border: c(30.0, 0.14, 0.87, 1.0),
                accent: c(12.0, 0.62, 0.44, 1.0),
                headword: c(216.0, 0.58, 0.32, 1.0),
                phonetic: c(190.0, 0.38, 0.38, 1.0),
                pos: c(268.0, 0.40, 0.44, 1.0),
                example: c(204.0, 0.20, 0.30, 1.0),
                link: c(210.0, 0.66, 0.40, 1.0),
                warning: c(8.0, 0.66, 0.45, 1.0),
                note_bg: c(45.0, 0.60, 0.95, 1.0),
                note_border: c(42.0, 0.72, 0.66, 1.0),
                example_bg: c(205.0, 0.55, 0.96, 1.0),
                selection: c(210.0, 0.60, 0.86, 1.0),
            },
            ThemeId::Dark => Self {
                bg: c(222.0, 0.14, 0.11, 1.0),
                panel: c(222.0, 0.13, 0.14, 1.0),
                elevated: c(222.0, 0.12, 0.18, 1.0),
                text: c(210.0, 0.10, 0.86, 1.0),
                secondary: c(210.0, 0.08, 0.72, 1.0),
                muted: c(210.0, 0.06, 0.56, 1.0),
                border: c(220.0, 0.10, 0.24, 1.0),
                accent: c(18.0, 0.70, 0.68, 1.0),
                headword: c(205.0, 0.62, 0.74, 1.0),
                phonetic: c(178.0, 0.44, 0.66, 1.0),
                pos: c(270.0, 0.46, 0.76, 1.0),
                example: c(200.0, 0.14, 0.78, 1.0),
                link: c(202.0, 0.72, 0.70, 1.0),
                warning: c(12.0, 0.70, 0.68, 1.0),
                note_bg: c(38.0, 0.30, 0.20, 1.0),
                note_border: c(38.0, 0.52, 0.48, 1.0),
                example_bg: c(210.0, 0.20, 0.17, 1.0),
                selection: c(208.0, 0.42, 0.30, 1.0),
            },
            ThemeId::Sepia => Self {
                bg: c(39.0, 0.46, 0.93, 1.0),
                panel: c(39.0, 0.42, 0.90, 1.0),
                elevated: c(40.0, 0.40, 0.96, 1.0),
                text: c(28.0, 0.30, 0.20, 1.0),
                secondary: c(28.0, 0.22, 0.35, 1.0),
                muted: c(30.0, 0.16, 0.52, 1.0),
                border: c(36.0, 0.30, 0.80, 1.0),
                accent: c(14.0, 0.52, 0.42, 1.0),
                headword: c(24.0, 0.46, 0.30, 1.0),
                phonetic: c(174.0, 0.34, 0.36, 1.0),
                pos: c(268.0, 0.32, 0.42, 1.0),
                example: c(28.0, 0.20, 0.36, 1.0),
                link: c(206.0, 0.54, 0.38, 1.0),
                warning: c(10.0, 0.58, 0.42, 1.0),
                note_bg: c(42.0, 0.52, 0.88, 1.0),
                note_border: c(38.0, 0.50, 0.62, 1.0),
                example_bg: c(40.0, 0.36, 0.89, 1.0),
                selection: c(40.0, 0.44, 0.82, 1.0),
            },
        }
    }

    pub fn semantic(&self, color: SemanticColor) -> Hsla {
        match color {
            SemanticColor::Primary => self.text,
            SemanticColor::Secondary => self.secondary,
            SemanticColor::Muted => self.muted,
            SemanticColor::Accent => self.accent,
            SemanticColor::Headword => self.headword,
            SemanticColor::Phonetic => self.phonetic,
            SemanticColor::Pos => self.pos,
            SemanticColor::Example => self.example,
            SemanticColor::Link => self.link,
            SemanticColor::Warning => self.warning,
        }
    }

    pub fn font(&self, role: FontRole) -> &'static str {
        match role {
            FontRole::Sans => ".SystemUIFont",
            FontRole::Serif => "Georgia",
            FontRole::Phonetic => "Kingsoft Phonetic Plain",
            FontRole::Mono => "Consolas",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub id: ThemeId,
    pub colors: Colors,
    pub base_size: f32,
}

impl Theme {
    pub fn new(id: ThemeId, font_size: u16) -> Self {
        Self {
            id,
            colors: Colors::for_theme(id),
            base_size: font_size as f32,
        }
    }

    pub fn size(&self, delta: i8) -> f32 {
        (self.base_size + delta as f32 * 0.8).clamp(9.0, 40.0)
    }
}
