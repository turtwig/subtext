//! Drawer layout and open tabs, stored under the config directory.
//!
//! Buffer text lives in the scratch file or the real file. `state.json`
//! only keeps the metadata needed to reopen those buffers.

use crate::editor::TabStop;
use crate::editor::tab::Record;
use crate::geom::Edge;
use crate::look::{self, FontChoice, TextUnit, ThemeChoice};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
    /// The drawer opens on the display under the pointer.
    /// Mutually exclusive with a pinned `output_name`.
    #[serde(default)]
    pub follow_cursor: bool,
    /// Pinned output name. Empty means the primary display.
    #[serde(default)]
    pub output_name: String,
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
    atomic_write(&paths.state, text.as_bytes())
}

/// Replace `path` by writing a sibling temp file and renaming it into place.
///
/// A crash during the write leaves the previous file intact. A symlink is
/// followed, so the link itself stays and the target is replaced. The temp
/// name is unique per attempt, and a sibling lock keeps two processes from
/// renaming over the same file at once.
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic(path, bytes, Replace::Always)
}

/// Replace `path` only when its current bytes are still `expected`.
///
/// A missing file matches an empty `expected`, which is how a new scratch is
/// created. Any other change returns [`file_changed`] and leaves the file
/// untouched, so the caller can read it again and merge.
pub(crate) fn atomic_write_if_matches(
    path: &Path,
    bytes: &[u8],
    expected: &[u8],
) -> io::Result<()> {
    write_atomic(path, bytes, Replace::IfMatches(expected))
}

/// Create `path` only when it is still absent.
pub(crate) fn atomic_write_if_absent(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic(path, bytes, Replace::IfAbsent)
}

/// The file was replaced by someone else between the read and the write.
pub(crate) fn file_changed() -> io::Error {
    io::Error::new(io::ErrorKind::AlreadyExists, FileChanged)
}

pub(crate) fn is_file_changed(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|inner| inner.is::<FileChanged>())
}

#[derive(Debug)]
struct FileChanged;

impl std::fmt::Display for FileChanged {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("file changed while saving")
    }
}

impl std::error::Error for FileChanged {}

enum Replace<'a> {
    Always,
    IfMatches(&'a [u8]),
    IfAbsent,
}

fn write_atomic(path: &Path, bytes: &[u8], replace: Replace<'_>) -> io::Result<()> {
    let path = if path
        .symlink_metadata()
        .is_ok_and(|meta| meta.file_type().is_symlink())
    {
        fs::canonicalize(path)?
    } else {
        path.to_path_buf()
    };
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let _lock = LockFile::acquire(&path)?;
    match replace {
        Replace::Always => {}
        Replace::IfMatches(expected) => match fs::read(&path) {
            Ok(current) if current == expected => {}
            Ok(_) => return Err(file_changed()),
            Err(error) if error.kind() == io::ErrorKind::NotFound && expected.is_empty() => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Err(file_changed()),
            Err(error) => return Err(error),
        },
        Replace::IfAbsent => {
            if fs::symlink_metadata(&path).is_ok() {
                return Err(file_changed());
            }
        }
    }
    let mut temporary = temporary_path(&path);
    let mut file = None;
    for _ in 0..8 {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(created) => {
                file = Some(created);
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                temporary = temporary_path(&path);
            }
            Err(error) => return Err(error),
        }
    }
    let Some(mut file) = file else {
        return Err(io::Error::other("could not create a temporary file"));
    };
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        if let Ok(meta) = fs::metadata(&path) {
            let _ = fs::set_permissions(&temporary, meta.permissions());
        }
        fs::rename(&temporary, &path)?;
        if let Some(parent) = path.parent()
            && let Ok(dir) = fs::File::open(parent)
        {
            let _ = dir.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| std::ffi::OsString::from("subtext"));
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_nanos())
        .unwrap_or(0);
    name.push(format!(".subtext-{}.{nanos}.{seq}", std::process::id()));
    path.with_file_name(name)
}

fn lock_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| std::ffi::OsString::from("subtext"));
    name.push(".subtext-lock");
    path.with_file_name(name)
}

/// How long a lock may sit before another process treats it as a crash.
const LOCK_STALE: Duration = Duration::from_secs(15);

struct LockFile(PathBuf);

impl LockFile {
    fn acquire(path: &Path) -> io::Result<Self> {
        let lock = lock_path(path);
        for _ in 0..40 {
            match OpenOptions::new().write(true).create_new(true).open(&lock) {
                Ok(mut file) => {
                    let _ = writeln!(file, "{}", std::process::id());
                    return Ok(Self(lock));
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    if lock_is_stale(&lock) {
                        let _ = fs::remove_file(&lock);
                        continue;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::other("file is being saved by another subtext"))
    }
}

impl Drop for LockFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn lock_is_stale(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    meta.modified()
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age > LOCK_STALE)
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
            follow_cursor: true,
            output_name: "DP-2".into(),
        };
        write(&paths, &snapshot).unwrap();
        let loaded = read(&paths).unwrap().unwrap();
        assert_eq!(loaded, snapshot);
        assert!(paths.state.is_file());
        assert!(dir_has_no_subtext_temp(&paths.root));
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
        assert!(!snapshot.follow_cursor);
        assert!(snapshot.output_name.is_empty());
        assert_eq!(snapshot.text_size, 16.0);
        assert_eq!(snapshot.text_unit, TextUnit::Pt);
    }

    #[test]
    fn atomic_write_replaces_the_file_and_leaves_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.txt");
        fs::write(&path, "old").unwrap();
        atomic_write(&path, b"new").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        assert!(dir_has_no_subtext_temp(dir.path()));
    }

    #[test]
    fn a_write_leaves_a_file_that_changed_underneath() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.txt");
        fs::write(&path, "other").unwrap();
        let error = atomic_write_if_matches(&path, b"mine", b"base").unwrap_err();
        assert!(is_file_changed(&error));
        assert_eq!(fs::read_to_string(&path).unwrap(), "other");
        atomic_write_if_matches(&path, b"mine", b"other").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "mine");
    }

    #[test]
    fn an_absent_write_does_not_replace_a_file_that_appeared() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.txt");
        atomic_write_if_absent(&path, b"created").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "created");
        let error = atomic_write_if_absent(&path, b"again").unwrap_err();
        assert!(is_file_changed(&error));
        assert_eq!(fs::read_to_string(&path).unwrap(), "created");
    }

    #[test]
    fn simultaneous_writes_keep_one_complete_copy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.txt");
        fs::write(&path, "base").unwrap();
        let left = path.clone();
        let right = path.clone();
        let first = std::thread::spawn(move || atomic_write(&left, b"aaa"));
        let second = std::thread::spawn(move || atomic_write(&right, b"bbb"));
        first.join().unwrap().unwrap();
        second.join().unwrap().unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text == "aaa" || text == "bbb", "{text}");
        assert!(dir_has_no_subtext_temp(dir.path()));
    }

    fn dir_has_no_subtext_temp(dir: &Path) -> bool {
        fs::read_dir(dir).unwrap().all(|entry| {
            let name = entry.unwrap().file_name();
            let name = name.to_string_lossy();
            !name.contains(".subtext-")
        })
    }

    #[test]
    fn atomic_write_follows_a_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.txt");
        let link = dir.path().join("link.txt");
        fs::write(&target, "old").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        atomic_write(&link, b"new").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn a_failed_atomic_write_keeps_the_original() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blocked");
        fs::create_dir(&path).unwrap();
        assert!(atomic_write(&path, b"new").is_err());
        assert!(path.is_dir());
    }
}
