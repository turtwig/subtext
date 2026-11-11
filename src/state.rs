//! Drawer layout and open tabs, stored under the config directory.
//!
//! Buffer text lives in the scratch file or the real file. `state.json`
//! only keeps the metadata needed to reopen those buffers.

use crate::editor::TabStop;
use crate::editor::tab::Record;
use crate::geom::Edge;
use crate::look::{self, FontChoice, TextUnit, ThemeChoice};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
    pub scratch: PathBuf,
    pub state: PathBuf,
}

impl Paths {
    pub fn resolve() -> Self {
        Self::under(config_root())
    }

    pub fn under(root: PathBuf) -> Self {
        Self {
            scratch: root.join("scratch"),
            state: root.join("state.json"),
            root,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Snapshot {
    pub anchor: Edge,
    pub columns: u32,
    pub rows: u32,
    pub active: usize,
    pub tabs: Vec<Record>,
    #[serde(default = "look::default_text_size")]
    pub text_size: f32,
    /// Unit of `text_size`. Defaults to points.
    #[serde(default)]
    pub text_unit: TextUnit,
    #[serde(default)]
    pub font: FontChoice,
    #[serde(default)]
    pub theme: ThemeChoice,
    #[serde(default = "default_line_numbers")]
    pub line_numbers: bool,
    #[serde(default)]
    pub word_wrap: bool,
    /// Tight padding and gutter around the glyphs. Off leaves the breezy layout.
    #[serde(default)]
    pub compact: bool,
    /// Pixel size of the drawer. Zero means the size comes from the column and row counts.
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    /// Width to return to after a double-click maximize.
    #[serde(default)]
    pub width_restore: u32,
    /// Top or bottom drawer is stretched across the output.
    #[serde(default)]
    pub span_width: bool,
    /// Saved size for a top or bottom drawer. Zero means it has not been stored yet.
    #[serde(default)]
    pub across_width: u32,
    #[serde(default)]
    pub across_height: u32,
    #[serde(default)]
    pub across_width_restore: u32,
    #[serde(default)]
    pub across_span: bool,
    /// Saved size for a left or right drawer.
    #[serde(default)]
    pub side_width: u32,
    #[serde(default)]
    pub side_height: u32,
    /// Plate alpha. The margin around the drawer stays clear.
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default)]
    pub blur: bool,
    /// Characters the Tab key inserts.
    #[serde(default)]
    pub tab_stop: TabStop,
    /// Enter repeats the leading indent of the current line.
    #[serde(default)]
    pub remember_indent: bool,
    /// Snap glyph strokes to the pixel grid. Off leaves the outlines unhinted.
    #[serde(default = "default_on")]
    pub hinting: bool,
    /// A click outside the drawer hides it. Off leaves that to Escape.
    #[serde(default = "default_on")]
    pub click_away: bool,
}

fn default_on() -> bool {
    true
}

fn default_opacity() -> f32 {
    1.0
}

fn default_line_numbers() -> bool {
    true
}

pub fn read(paths: &Paths) -> io::Result<Option<Snapshot>> {
    if !paths.state.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&paths.state)?;
    if text.trim().is_empty() {
        return Ok(None);
    }
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, format!("state.json: {error}")))
}

pub fn write(paths: &Paths, snapshot: &Snapshot) -> io::Result<()> {
    fs::create_dir_all(&paths.root)?;
    let text = serde_json::to_string_pretty(snapshot)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    let temporary = paths.state.with_extension("json.tmp");
    fs::write(&temporary, text)?;
    fs::rename(&temporary, &paths.state)?;
    Ok(())
}

fn config_root() -> PathBuf {
    dirs::config_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"))
        .join("subtext")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::Language;

    #[test]
    fn snapshot_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path().join("subtext"));
        let snapshot = Snapshot {
            anchor: Edge::Top,
            columns: 80,
            rows: 12,
            active: 0,
            tabs: vec![Record {
                id: "tab-1".into(),
                title: "Scratch 1".into(),
                file_path: None,
                scratch_path: Some(paths.scratch.join("scratch_1.txt")),
                language: Language::Markdown,
                cursor_line: 2,
                cursor_column: 4,
                scroll_lines: 1,
            }],
            text_size: 16.0,
            text_unit: TextUnit::Pt,
            font: FontChoice::SYSTEM,
            theme: ThemeChoice::Dracula,
            line_numbers: true,
            word_wrap: false,
            compact: true,
            width: 800,
            height: 400,
            width_restore: 640,
            span_width: true,
            across_width: 800,
            across_height: 400,
            across_width_restore: 640,
            across_span: true,
            side_width: 720,
            side_height: 900,
            opacity: 0.85,
            blur: true,
            tab_stop: TabStop::Spaces2,
            remember_indent: true,
            hinting: false,
            click_away: false,
        };
        write(&paths, &snapshot).unwrap();
        let loaded = read(&paths).unwrap().unwrap();
        assert_eq!(loaded, snapshot);
        assert!(paths.state.is_file());
        assert!(!paths.state.with_extension("json.tmp").exists());
    }

    #[test]
    fn older_state_leaves_pixel_size_unset() {
        let snapshot: Snapshot = serde_json::from_str(
            r#"{"anchor":"bottom","columns":80,"rows":10,"active":0,"tabs":[]}"#,
        )
        .unwrap();
        assert_eq!(snapshot.width, 0);
        assert_eq!(snapshot.height, 0);
        assert_eq!(snapshot.width_restore, 0);
        assert!(!snapshot.span_width);
        assert!(snapshot.line_numbers);
        assert!(!snapshot.word_wrap);
        assert!(!snapshot.compact);
        assert_eq!(snapshot.across_width, 0);
        assert_eq!(snapshot.side_width, 0);
        assert_eq!(snapshot.opacity, 1.0);
        assert!(!snapshot.blur);
        assert_eq!(snapshot.tab_stop, TabStop::Spaces4);
        assert!(!snapshot.remember_indent);
        assert!(snapshot.hinting);
        assert!(snapshot.click_away);
        assert_eq!(snapshot.text_size, 16.0);
        assert_eq!(snapshot.text_unit, TextUnit::Pt);
    }
}
