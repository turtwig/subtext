//! Font, text size, and theme. Stored in `state.json` and applied live.

use iced::Font;
use iced::highlighter;
use serde::{Deserialize, Serialize};

/// How the size number is read. Points match a terminal at 96 logical DPI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TextUnit {
    #[default]
    Pt,
    Px,
}

impl TextUnit {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pt => "pt",
            Self::Px => "px",
        }
    }

    /// iced draws logical pixels. One point is 96/72 of a pixel.
    pub fn to_pixels(self, size: f32) -> f32 {
        match self {
            Self::Px => size,
            Self::Pt => size * 96.0 / 72.0,
        }
    }

    pub fn from_pixels(self, pixels: f32) -> f32 {
        match self {
            Self::Px => pixels,
            Self::Pt => pixels * 72.0 / 96.0,
        }
    }

    fn limits(self) -> (f32, f32) {
        match self {
            Self::Pt => (8.0, 36.0),
            Self::Px => (10.0, 48.0),
        }
    }
}

impl std::fmt::Display for TextUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

pub fn clamp_text_size(size: f32, unit: TextUnit) -> f32 {
    let (min, max) = unit.limits();
    if !size.is_finite() {
        return default_text_size();
    }
    size.round().clamp(min, max)
}

pub fn default_text_size() -> f32 {
    crate::geom::TEXT_SIZE
}

/// A face the editor can draw. `System` is the generic monospace font.
/// Every other value is a family name from the installed fonts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontChoice {
    name: &'static str,
}

impl Default for FontChoice {
    fn default() -> Self {
        Self::SYSTEM
    }
}

impl FontChoice {
    pub const SYSTEM: Self = Self { name: "System" };

    /// System, then every installed family that claims to be monospace.
    pub fn installed() -> &'static [Self] {
        static CHOICES: std::sync::OnceLock<Vec<FontChoice>> = std::sync::OnceLock::new();
        CHOICES.get_or_init(|| {
            let mut choices = Vec::new();
            choices.push(Self::SYSTEM);
            for name in monospace_families() {
                if name == Self::SYSTEM.name {
                    continue;
                }
                choices.push(Self {
                    name: Box::leak(name.into_boxed_str()),
                });
            }
            choices
        })
    }

    pub fn label(self) -> &'static str {
        self.name
    }

    pub fn font(self) -> Font {
        if self == Self::SYSTEM {
            Font::MONOSPACE
        } else {
            Font::with_name(self.name)
        }
    }

    fn from_saved(raw: &str) -> Self {
        if raw.eq_ignore_ascii_case("system") {
            return Self::SYSTEM;
        }
        Self::installed()
            .iter()
            .find(|choice| choice.name == raw)
            .copied()
            .unwrap_or(Self::SYSTEM)
    }
}

/// Family names of installed faces whose font file says they are monospace.
fn monospace_families() -> Vec<String> {
    let Ok(mut system) = iced_graphics::text::font_system().write() else {
        return Vec::new();
    };
    let mut seen = std::collections::HashSet::new();
    let mut names = Vec::new();
    for face in system.raw().db().faces() {
        if !face.monospaced {
            continue;
        }
        let Some((name, _)) = face.families.first() else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() || !seen.insert(name.to_lowercase()) {
            continue;
        }
        names.push(name.to_string());
    }
    drop(system);
    names.sort_by_key(|name| name.to_lowercase());
    names
}

impl Serialize for FontChoice {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name)
    }
}

impl<'de> Deserialize<'de> for FontChoice {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self::from_saved(&String::deserialize(deserializer)?))
    }
}

impl std::fmt::Display for FontChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Face for the tab row, find bar, and settings. `MatchEditor` follows [`FontChoice`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UiFont {
    #[default]
    MatchEditor,
    Face(FontChoice),
}

impl UiFont {
    pub fn installed() -> &'static [Self] {
        static CHOICES: std::sync::OnceLock<Vec<UiFont>> = std::sync::OnceLock::new();
        CHOICES.get_or_init(|| {
            let mut choices = Vec::new();
            choices.push(Self::MatchEditor);
            choices.extend(FontChoice::installed().iter().copied().map(Self::Face));
            choices
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::MatchEditor => "Match Editor",
            Self::Face(face) => face.label(),
        }
    }

    pub fn resolve(self, editor: FontChoice) -> Font {
        match self {
            Self::MatchEditor => editor.font(),
            Self::Face(face) => face.font(),
        }
    }
}

impl std::fmt::Display for UiFont {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

impl Serialize for UiFont {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::MatchEditor => serializer.serialize_str("match"),
            Self::Face(face) => face.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for UiFont {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        if raw.eq_ignore_ascii_case("match") {
            return Ok(Self::MatchEditor);
        }
        Ok(Self::Face(FontChoice::from_saved(&raw)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeChoice {
    Light,
    Dark,
    #[default]
    Dracula,
    Nord,
    SolarizedLight,
    SolarizedDark,
    GruvboxLight,
    GruvboxDark,
    CatppuccinLatte,
    CatppuccinFrappe,
    CatppuccinMacchiato,
    CatppuccinMocha,
    TokyoNight,
    TokyoNightStorm,
    TokyoNightLight,
    KanagawaWave,
    KanagawaDragon,
    KanagawaLotus,
    Moonfly,
    Nightfly,
    Oxocarbon,
    Ferra,
}

impl ThemeChoice {
    pub const ALL: &'static [Self] = &[
        Self::Light,
        Self::Dark,
        Self::Dracula,
        Self::Nord,
        Self::SolarizedLight,
        Self::SolarizedDark,
        Self::GruvboxLight,
        Self::GruvboxDark,
        Self::CatppuccinLatte,
        Self::CatppuccinFrappe,
        Self::CatppuccinMacchiato,
        Self::CatppuccinMocha,
        Self::TokyoNight,
        Self::TokyoNightStorm,
        Self::TokyoNightLight,
        Self::KanagawaWave,
        Self::KanagawaDragon,
        Self::KanagawaLotus,
        Self::Moonfly,
        Self::Nightfly,
        Self::Oxocarbon,
        Self::Ferra,
    ];

    pub fn iced(self) -> iced::Theme {
        match self {
            Self::Light => iced::Theme::Light,
            Self::Dark => iced::Theme::Dark,
            Self::Dracula => iced::Theme::Dracula,
            Self::Nord => iced::Theme::Nord,
            Self::SolarizedLight => iced::Theme::SolarizedLight,
            Self::SolarizedDark => iced::Theme::SolarizedDark,
            Self::GruvboxLight => iced::Theme::GruvboxLight,
            Self::GruvboxDark => iced::Theme::GruvboxDark,
            Self::CatppuccinLatte => iced::Theme::CatppuccinLatte,
            Self::CatppuccinFrappe => iced::Theme::CatppuccinFrappe,
            Self::CatppuccinMacchiato => iced::Theme::CatppuccinMacchiato,
            Self::CatppuccinMocha => iced::Theme::CatppuccinMocha,
            Self::TokyoNight => iced::Theme::TokyoNight,
            Self::TokyoNightStorm => iced::Theme::TokyoNightStorm,
            Self::TokyoNightLight => iced::Theme::TokyoNightLight,
            Self::KanagawaWave => iced::Theme::KanagawaWave,
            Self::KanagawaDragon => iced::Theme::KanagawaDragon,
            Self::KanagawaLotus => iced::Theme::KanagawaLotus,
            Self::Moonfly => iced::Theme::Moonfly,
            Self::Nightfly => iced::Theme::Nightfly,
            Self::Oxocarbon => iced::Theme::Oxocarbon,
            Self::Ferra => iced::Theme::Ferra,
        }
    }

    /// The highlighter has no Dracula palette. Light UI themes get a light
    /// syntax theme; everything else stays on Base16 Ocean.
    pub fn syntax(self) -> highlighter::Theme {
        match self {
            Self::Light
            | Self::SolarizedLight
            | Self::GruvboxLight
            | Self::CatppuccinLatte
            | Self::TokyoNightLight
            | Self::KanagawaLotus => highlighter::Theme::InspiredGitHub,
            _ => highlighter::Theme::Base16Ocean,
        }
    }
}

impl std::fmt::Display for ThemeChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
            Self::Dracula => "Dracula",
            Self::Nord => "Nord",
            Self::SolarizedLight => "Solarized Light",
            Self::SolarizedDark => "Solarized Dark",
            Self::GruvboxLight => "Gruvbox Light",
            Self::GruvboxDark => "Gruvbox Dark",
            Self::CatppuccinLatte => "Catppuccin Latte",
            Self::CatppuccinFrappe => "Catppuccin Frappé",
            Self::CatppuccinMacchiato => "Catppuccin Macchiato",
            Self::CatppuccinMocha => "Catppuccin Mocha",
            Self::TokyoNight => "Tokyo Night",
            Self::TokyoNightStorm => "Tokyo Night Storm",
            Self::TokyoNightLight => "Tokyo Night Light",
            Self::KanagawaWave => "Kanagawa Wave",
            Self::KanagawaDragon => "Kanagawa Dragon",
            Self::KanagawaLotus => "Kanagawa Lotus",
            Self::Moonfly => "Moonfly",
            Self::Nightfly => "Nightfly",
            Self::Oxocarbon => "Oxocarbon",
            Self::Ferra => "Ferra",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_saved_font_falls_back_to_system() {
        let font: FontChoice = serde_json::from_str("\"jetbrains-mono\"").unwrap();
        assert_eq!(font, FontChoice::SYSTEM);
        let system: FontChoice = serde_json::from_str("\"system\"").unwrap();
        assert_eq!(system, FontChoice::SYSTEM);
        assert_eq!(system.font(), Font::MONOSPACE);
    }

    #[test]
    fn installed_fonts_start_with_system() {
        let fonts = FontChoice::installed();
        assert_eq!(fonts.first().copied(), Some(FontChoice::SYSTEM));
    }

    #[test]
    fn sixteen_points_is_the_terminal_pixel_size() {
        let pixels = TextUnit::Pt.to_pixels(16.0);
        assert!((pixels - 21.333333).abs() < 0.001);
        assert_eq!(
            clamp_text_size(TextUnit::Px.from_pixels(pixels), TextUnit::Px),
            21.0
        );
        assert_eq!(
            clamp_text_size(TextUnit::Pt.from_pixels(21.0), TextUnit::Pt),
            16.0
        );
    }
}
