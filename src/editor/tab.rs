//! One open scratch or file, backed by iced's text editor buffer.

use super::indent::{self, Caret};
use super::merge::{self, Merge};
use super::syntax::Language;
use super::wrap;
use iced::widget::text_editor::{self, Action};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Record {
    pub id: String,
    pub title: String,
    pub file_path: Option<PathBuf>,
    pub scratch_path: Option<PathBuf>,
    pub language: Language,
    pub cursor_line: usize,
    pub cursor_column: usize,
    pub scroll_lines: i32,
}

pub struct Tab {
    pub id: String,
    pub title: String,
    pub file_path: Option<PathBuf>,
    pub scratch_path: Option<PathBuf>,
    pub language: Language,
    pub content: text_editor::Content,
    pub dirty: bool,
    /// The backing file exists but could not be read. Nothing may replace it.
    unreadable: bool,
    /// Last on-disk text this buffer was based on, in the editor's own form.
    /// A merge keeps this as the other copy, not the combined buffer, so a
    /// later open can still tell the two sides apart.
    synced: String,
    /// Both sides edited the same lines. Automatic saves leave the file alone.
    conflicted: bool,
    /// Set once a real conflict was installed, so undo/redo can put the flag
    /// back when the markers return. Typed notes that happen to contain the
    /// marker do not set this.
    saw_conflict: bool,
    pub cursor_line: usize,
    pub cursor_column: usize,
    pub scroll_lines: i32,
    undo: Vec<Mark>,
    redo: Vec<Mark>,
}

struct Mark {
    text: String,
    line: usize,
    column: usize,
}

const HISTORY: usize = 100;

/// What [`Tab::reconcile_disk`] did with the backing file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refresh {
    /// The file matches the baseline. Local edits, if any, are still in the buffer.
    Unchanged,
    /// This buffer had no edits, so it now shows the file.
    Reloaded,
    /// Both sides edited different lines. The buffer holds the combination.
    Merged,
    /// Both sides edited the same lines. The buffer holds markers and must not be saved automatically.
    Conflict,
    /// The file could not be reread. The buffer and the file were both left alone.
    Failed,
}

fn editor_text(text: &str) -> String {
    let content: text_editor::Content = text_editor::Content::with_text(text);
    content.text()
}

fn caret_in(text: &str, line: usize, column: usize) -> (usize, usize) {
    if text.is_empty() {
        return (0, 0);
    }
    let mut bodies = Vec::new();
    let mut start = 0;
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            bodies.push(&text[start..index]);
            start = index + 1;
        }
    }
    if start < text.len() {
        bodies.push(&text[start..]);
    } else if text.ends_with('\n') {
        // The editor keeps a trailing empty line after a final newline.
        bodies.push("");
    }
    if bodies.is_empty() {
        return (0, 0);
    }
    let line = line.min(bodies.len() - 1);
    let body = bodies[line].trim_end_matches('\r');
    let column = column.min(body.len());
    let column = if body.is_char_boundary(column) {
        column
    } else {
        let mut at = column;
        while at > 0 && !body.is_char_boundary(at) {
            at -= 1;
        }
        at
    };
    (line, column)
}

/// Read a backing file. A missing file is a new buffer. Any other failure
/// must not be saved back over the original.
fn load_text(path: &Path) -> (String, bool) {
    match fs::read_to_string(path) {
        Ok(text) => (text, false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (String::new(), false),
        Err(error) => {
            eprintln!("subtext: could not read {}: {error}", path.display());
            (String::new(), true)
        }
    }
}

impl Tab {
    pub fn new_scratch(dir: &Path, index: u32) -> Self {
        let scratch_path = dir.join(format!("scratch_{index}.txt"));
        let (text, unreadable) = load_text(&scratch_path);
        Self::from_parts(
            Uuid::new_v4().to_string(),
            format!("Scratch {index}"),
            None,
            Some(scratch_path),
            Language::Markdown,
            text,
            0,
            0,
            0,
            unreadable,
        )
    }

    pub fn open_file(path: PathBuf) -> Result<Self, std::io::Error> {
        let text = fs::read_to_string(&path)?;
        let title = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let language = Language::from_path(&path);
        Ok(Self::from_parts(
            Uuid::new_v4().to_string(),
            title,
            Some(path),
            None,
            language,
            text,
            0,
            0,
            0,
            false,
        ))
    }

    pub fn from_record(record: Record) -> Self {
        let path = record
            .file_path
            .clone()
            .or_else(|| record.scratch_path.clone());
        let (text, unreadable) = path
            .as_ref()
            .map(|path| load_text(path))
            .unwrap_or_else(|| (String::new(), false));
        Self::from_parts(
            record.id,
            record.title,
            record.file_path,
            record.scratch_path,
            record.language,
            text,
            record.cursor_line,
            record.cursor_column,
            record.scroll_lines,
            unreadable,
        )
    }

    fn from_parts(
        id: String,
        title: String,
        file_path: Option<PathBuf>,
        scratch_path: Option<PathBuf>,
        language: Language,
        text: String,
        cursor_line: usize,
        cursor_column: usize,
        scroll_lines: i32,
        unreadable: bool,
    ) -> Self {
        let mut content = text_editor::Content::with_text(&text);
        content.move_to(text_editor::Cursor {
            position: text_editor::Position {
                line: cursor_line,
                column: cursor_column,
            },
            selection: None,
        });
        if scroll_lines != 0 {
            content.perform(Action::Scroll {
                lines: scroll_lines,
            });
        }
        let synced = content.text();
        Self {
            id,
            title,
            file_path,
            scratch_path,
            language,
            content,
            dirty: false,
            unreadable,
            synced,
            conflicted: false,
            saw_conflict: false,
            cursor_line,
            cursor_column,
            scroll_lines,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    pub fn record(&self) -> Record {
        Record {
            id: self.id.clone(),
            title: self.title.clone(),
            file_path: self.file_path.clone(),
            scratch_path: self.scratch_path.clone(),
            language: self.language,
            cursor_line: self.cursor_line,
            cursor_column: self.cursor_column,
            scroll_lines: self.scroll_lines,
        }
    }

    /// Put the caret back on the stored line and column.
    pub fn recall_cursor(&mut self) {
        self.content.move_to(text_editor::Cursor {
            position: text_editor::Position {
                line: self.cursor_line,
                column: self.cursor_column,
            },
            selection: None,
        });
    }

    /// Applies `action`. Returns whether it was a wheel scroll.
    pub fn edit(&mut self, action: Action) -> bool {
        let wheel = matches!(action, Action::Scroll { .. });
        let edits = action.is_edit();
        let before = edits.then(|| self.mark());
        self.content.perform(action);
        self.note_cursor();
        if let Some(before) = before {
            if before.text != self.text()
                || before.line != self.cursor_line
                || before.column != self.cursor_column
            {
                self.push_history(before);
                self.dirty = true;
            }
        }
        self.refresh_conflict_flag(true);
        wheel
    }

    pub fn undo(&mut self) -> bool {
        self.step(true)
    }

    pub fn redo(&mut self) -> bool {
        self.step(false)
    }

    /// Insert the tab unit at the caret, or before each selected line.
    pub fn indent(&mut self, unit: &str) {
        self.retab(unit, false);
    }

    /// Remove one tab unit from the caret line, or from each selected line.
    pub fn unindent(&mut self, unit: &str) {
        self.retab(unit, true);
    }

    /// Remove one indent stop when this line is only that indent.
    ///
    /// Returns false when backspace should delete one character instead.
    pub fn eat_indent(&mut self, unit: &str) -> bool {
        let cursor = self.content.cursor();
        let Some((text, caret)) = indent::eat_indent(
            &self.text(),
            Caret {
                line: cursor.position.line,
                column: cursor.position.column,
            },
            cursor.selection.map(|position| Caret {
                line: position.line,
                column: position.column,
            }),
            unit,
        ) else {
            return false;
        };
        let before = self.mark();
        if before.text == text && before.line == caret.line && before.column == caret.column {
            return false;
        }
        self.restore(&text, caret.line, caret.column);
        self.push_history(before);
        true
    }

    /// Break the line. `unit` repeats that line's indent. `None` does not.
    pub fn break_line(&mut self, unit: Option<&str>) {
        let cursor = self.content.cursor();
        let Some((text, caret)) = indent::break_line(
            &self.text(),
            Caret {
                line: cursor.position.line,
                column: cursor.position.column,
            },
            cursor.selection.map(|position| Caret {
                line: position.line,
                column: position.column,
            }),
            unit,
        ) else {
            return;
        };
        let before = self.mark();
        if before.text == text && before.line == caret.line && before.column == caret.column {
            return;
        }
        self.restore(&text, caret.line, caret.column);
        self.push_history(before);
    }

    fn retab(&mut self, unit: &str, outdent: bool) {
        let cursor = self.content.cursor();
        let Some((text, caret)) = indent::retab(
            &self.text(),
            Caret {
                line: cursor.position.line,
                column: cursor.position.column,
            },
            cursor.selection.map(|position| Caret {
                line: position.line,
                column: position.column,
            }),
            unit,
            outdent,
        ) else {
            return;
        };
        let before = self.mark();
        self.restore(&text, caret.line, caret.column);
        self.push_history(before);
    }

    fn step(&mut self, undo: bool) -> bool {
        let mark = if undo {
            self.undo.pop()
        } else {
            self.redo.pop()
        };
        let Some(mark) = mark else {
            return false;
        };
        let current = self.mark();
        self.restore(&mark.text, mark.line, mark.column);
        if undo {
            self.redo.push(current);
        } else {
            self.undo.push(current);
        }
        self.refresh_conflict_flag(false);
        true
    }

    fn mark(&self) -> Mark {
        Mark {
            text: self.text(),
            line: self.cursor_line,
            column: self.cursor_column,
        }
    }

    fn push_history(&mut self, mark: Mark) {
        self.undo.push(mark);
        if self.undo.len() > HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn restore(&mut self, text: &str, line: usize, column: usize) {
        self.replace_text(text, line, column, true);
    }

    /// Swap the buffer for `text`. `dirty` stays under the caller's control so
    /// a clean reload does not look like an unsaved edit.
    fn replace_text(&mut self, text: &str, line: usize, column: usize, dirty: bool) {
        let (line, column) = caret_in(text, line, column);
        self.content = text_editor::Content::with_text(text);
        self.cursor_line = line;
        self.cursor_column = column;
        // The new buffer starts at the top, at a 1px line height. Scrolling
        // by the caret's buffer line parks that line at the top until the
        // editor lays the real metrics out and brings the caret on screen.
        self.scroll_lines = line as i32;
        if line != 0 {
            self.content.perform(Action::Scroll { lines: line as i32 });
        }
        self.content.move_to(text_editor::Cursor {
            position: text_editor::Position { line, column },
            selection: None,
        });
        self.dirty = dirty;
    }

    /// Ask the editor to scroll the caret on screen at the next layout.
    ///
    /// Used when wrap changes. The editor only reveals the caret after the
    /// cursor moves, so the caret is nudged and put back.
    pub fn reveal_caret(&mut self) {
        let line = self.cursor_line;
        let column = self.cursor_column;
        self.content.move_to(text_editor::Cursor {
            position: text_editor::Position {
                line,
                column: column.saturating_add(1),
            },
            selection: None,
        });
        self.content.move_to(text_editor::Cursor {
            position: text_editor::Position { line, column },
            selection: None,
        });
        self.note_cursor();
    }

    fn note_cursor(&mut self) {
        let cursor = self.content.cursor();
        self.cursor_line = cursor.position.line;
        self.cursor_column = cursor.position.column;
    }

    /// Select `start..end` and put the caret at `end`. This is not an edit.
    pub fn select_range(
        &mut self,
        start_line: usize,
        start_column: usize,
        end_line: usize,
        end_column: usize,
    ) {
        self.content.move_to(text_editor::Cursor {
            position: text_editor::Position {
                line: end_line,
                column: end_column,
            },
            selection: Some(text_editor::Position {
                line: start_line,
                column: start_column,
            }),
        });
        self.note_cursor();
    }

    /// Remember the buffer line at the top of the editor's layout.
    pub fn note_top_line(&mut self) {
        let top = self.viewport(1).top_line;
        self.scroll_lines = top as i32;
    }

    /// Visible rows from the editor's published layout.
    ///
    /// Before that layout exists, each buffer line is one row and
    /// `scroll_lines` is the buffer line at the top.
    pub fn viewport(&self, visible: u32) -> wrap::Viewport {
        if let Some(snap) = self.editor_snap() {
            return wrap::from_snap(&snap, visible, self.cursor_line, self.cursor_column);
        }
        let count = self.content.line_count().max(1);
        let ends = vec![vec![0usize]; count];
        wrap::from_ends(
            &ends,
            self.scroll_lines,
            visible,
            self.cursor_line,
            self.cursor_column,
        )
    }

    fn editor_snap(&self) -> Option<std::sync::Arc<cosmic_text::LayoutSnap>> {
        let lines = self.content.line_count();
        if lines == 0 {
            return None;
        }
        let first = self
            .content
            .line(0)
            .map(|line| cosmic_text::hash_line(&line.text))
            .unwrap_or(0);
        let last = self
            .content
            .line(lines - 1)
            .map(|line| cosmic_text::hash_line(&line.text))
            .unwrap_or(0);
        let mut bytes = 0usize;
        for index in 0..lines {
            if let Some(line) = self.content.line(index) {
                bytes += line.text.len();
            }
        }
        cosmic_text::layout_snap(lines, bytes, first, last)
    }

    /// Buffer lines, not a copy of the text. 1 for an empty buffer.
    pub fn line_count(&self) -> u32 {
        self.content.line_count().max(1) as u32
    }

    pub fn text(&self) -> String {
        self.content.text()
    }

    /// A scratch tab with something in the buffer. Closing it deletes the file.
    pub fn needs_close_confirm(&self) -> bool {
        self.file_path.is_none() && !self.content.is_empty()
    }

    /// The backing file could not be read, so it must not be replaced or deleted.
    pub(crate) fn unreadable(&self) -> bool {
        self.unreadable
    }

    #[cfg(test)]
    pub(crate) fn conflicted(&self) -> bool {
        self.conflicted
    }

    /// True when the file still matches the baseline, so a scratch close may
    /// delete it. A reload, merge, conflict, or unreadable file returns false
    /// and the caller leaves the file in place.
    pub(crate) fn disk_matches_baseline(&mut self) -> bool {
        matches!(self.reconcile_disk(), Refresh::Unchanged)
    }

    /// Path the buffer is written to. Real files win over the scratch copy.
    pub fn store_path(&self) -> Option<&Path> {
        self.file_path.as_deref().or(self.scratch_path.as_deref())
    }

    /// Reread the backing file. A clean buffer takes the file, including a
    /// full rewrite. Local edits stay. Both sides changing are merged, and
    /// overlapping lines become markers instead of a write.
    pub(crate) fn reconcile_disk(&mut self) -> Refresh {
        self.refresh_conflict_flag(false);
        if self.unreadable {
            return Refresh::Unchanged;
        }
        let Some(path) = self.store_path().map(|path| path.to_path_buf()) else {
            return Refresh::Unchanged;
        };
        let raw = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Refresh::Unchanged;
            }
            Err(error) => {
                eprintln!("subtext: could not read {}: {error}", path.display());
                return Refresh::Failed;
            }
        };
        // Compare in the form the editor will round-trip, so a trailing
        // newline the shaper keeps does not look like a disk change.
        let disk = editor_text(&raw);
        let buffer = self.text();
        if disk == buffer {
            self.synced = buffer;
            self.conflicted = false;
            self.saw_conflict = false;
            self.dirty = false;
            return Refresh::Unchanged;
        }
        if buffer == self.synced {
            self.adopt_disk(&disk);
            return Refresh::Reloaded;
        }
        if disk == self.synced {
            return Refresh::Unchanged;
        }
        let line = self.cursor_line;
        let column = self.cursor_column;
        let before = self.mark();
        match merge::merge(&self.synced, &buffer, &disk) {
            Merge::Clean(text) => {
                let changed = text != buffer;
                self.replace_text(&text, line, column, text != disk);
                if changed {
                    self.push_history(before);
                }
                // The baseline stays the other copy. Setting it to the merged
                // text would drop this computer's half the next time the file
                // changes.
                self.synced = disk;
                self.conflicted = false;
                self.saw_conflict = false;
                if changed {
                    eprintln!("subtext: merged changes into {}", path.display());
                }
                Refresh::Merged
            }
            Merge::Conflict(text) => {
                self.replace_text(&text, line, column, true);
                self.push_history(before);
                self.synced = disk;
                self.conflicted = true;
                self.saw_conflict = true;
                eprintln!(
                    "subtext: {} changed on both sides; resolve the marked lines before it can be saved",
                    path.display()
                );
                Refresh::Conflict
            }
        }
    }

    /// Take a file this buffer had not edited. Undo is cleared so a later save
    /// cannot put the stale text back.
    fn adopt_disk(&mut self, disk: &str) {
        self.replace_text(disk, self.cursor_line, self.cursor_column, false);
        self.synced = disk.to_string();
        self.conflicted = false;
        self.saw_conflict = false;
        self.undo.clear();
        self.redo.clear();
    }

    /// `edited` means a keystroke, not undo or a disk refresh. Resolving the
    /// markers by editing is what allows the next automatic save. Undoing back
    /// to this computer's text is not, or that save would replace the other copy.
    fn refresh_conflict_flag(&mut self, edited: bool) {
        let marked = self.text().contains(merge::OURS_MARK);
        if marked && self.saw_conflict {
            self.conflicted = true;
        } else if !marked {
            self.conflicted = false;
            if edited {
                self.saw_conflict = false;
            }
        }
    }

    /// Write this buffer back. `explicit` is Save or Save As: a conflict is
    /// the user's decision and may replace the file. An automatic save will
    /// not, and a file that could not be reread is left untouched either way.
    pub(crate) fn flush(&mut self, explicit: bool) -> Result<(), std::io::Error> {
        match self.reconcile_disk() {
            Refresh::Failed => {
                return Err(std::io::Error::other(
                    "could not reread the file, leaving it untouched",
                ));
            }
            // A conflict already in the buffer stays conflicted when the file
            // has not moved again. `reconcile_disk` reports that as unchanged,
            // and it still must not be written over the other copy.
            Refresh::Conflict if !explicit => {
                return Err(std::io::Error::other(
                    "refusing to overwrite a file that conflicts with a newer copy",
                ));
            }
            _ if !explicit && (self.conflicted || self.saw_conflict) => {
                return Err(std::io::Error::other(
                    "refusing to overwrite a file that conflicts with a newer copy",
                ));
            }
            _ => {}
        }
        if !self.dirty && !explicit {
            return Ok(());
        }
        self.write_buffer()?;
        if explicit {
            self.conflicted = false;
            self.saw_conflict = false;
        }
        Ok(())
    }

    pub(crate) fn note_saved(&mut self) {
        self.dirty = false;
        self.conflicted = false;
        self.saw_conflict = false;
        self.synced = self.text();
    }

    pub fn write_buffer(&mut self) -> Result<(), std::io::Error> {
        let Some(path) = self.store_path().map(|path| path.to_path_buf()) else {
            return Ok(());
        };
        self.write_to(&path)?;
        self.note_saved();
        Ok(())
    }

    /// Write the buffer to `path` without changing where this tab is stored.
    pub fn write_to(&self, path: &Path) -> Result<(), std::io::Error> {
        if self.unreadable {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "refusing to overwrite a file that could not be read",
            ));
        }
        crate::state::atomic_write(path, self.text().as_bytes())
    }

    #[cfg(test)]
    fn testing(text: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("subtext-tab-{}", Uuid::new_v4()));
        let _ = fs::create_dir_all(&dir);
        let mut tab = Self::new_scratch(&dir, 1);
        tab.content = text_editor::Content::with_text(text);
        tab.note_cursor();
        tab
    }

    pub fn location_label(&self) -> String {
        self.file_path
            .as_ref()
            .or(self.scratch_path.as_ref())
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| self.title.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_count_reads_the_buffer_length() {
        let tab = Tab::testing("one\ntwo\nthree");
        assert_eq!(tab.line_count(), 3);
        assert_eq!(Tab::testing("").line_count(), 1);
    }

    #[test]
    fn a_failed_read_is_never_written_back() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("note.txt");
        fs::create_dir(&file).unwrap();
        let mut tab = Tab::from_record(Record {
            id: "file".into(),
            title: "note.txt".into(),
            file_path: Some(file.clone()),
            scratch_path: None,
            language: Language::Plain,
            cursor_line: 0,
            cursor_column: 0,
            scroll_lines: 0,
        });
        assert!(tab.unreadable);
        tab.dirty = true;
        assert!(tab.write_buffer().is_err());
        assert!(file.is_dir());
    }

    #[test]
    fn a_missing_file_can_be_created() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("new.txt");
        let mut tab = Tab::from_record(Record {
            id: "file".into(),
            title: "new.txt".into(),
            file_path: Some(file.clone()),
            scratch_path: None,
            language: Language::Plain,
            cursor_line: 0,
            cursor_column: 0,
            scroll_lines: 0,
        });
        assert!(!tab.unreadable);
        tab.content = text_editor::Content::with_text("hello");
        tab.write_buffer().unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "hello");
    }

    #[test]
    fn the_gutter_uses_the_editor_buffer() {
        let mut tab = Tab::testing("gutter-snap alpha\ngutter-snap beta");
        tab.edit(Action::Scroll { lines: 1 });
        let view = tab.viewport(2);
        assert_eq!(view.slots[0].number, Some(2));
        assert_eq!(view.top_line, 1);
        assert!(view.slots[1].number.is_none());
    }

    #[test]
    fn undo_and_redo_walk_the_edits() {
        let mut tab = Tab::testing("");
        tab.edit(Action::Edit(text_editor::Edit::Insert('a')));
        tab.edit(Action::Edit(text_editor::Edit::Insert('b')));
        assert_eq!(tab.text(), "ab");
        assert!(tab.undo());
        assert_eq!(tab.text(), "a");
        assert!(tab.redo());
        assert_eq!(tab.text(), "ab");
        assert!(!tab.redo());
    }

    #[test]
    fn selecting_a_match_is_not_an_edit() {
        let mut tab = Tab::testing("hello hello");
        tab.select_range(0, 6, 0, 11);
        assert_eq!(tab.text(), "hello hello");
        assert!(!tab.undo());
        assert_eq!(tab.cursor_column, 11);
        assert_eq!(tab.content.selection().as_deref(), Some("hello"));
        let mut tab = Tab::testing("say Café");
        let end = 4 + "Café".len();
        tab.select_range(0, 4, 0, end);
        assert_eq!(tab.content.selection().as_deref(), Some("Café"));
        assert_eq!(tab.cursor_column, end);
    }

    #[test]
    fn tab_inserts_the_unit_and_undoes_as_one_edit() {
        let mut tab = Tab::testing("hi");
        tab.content.move_to(text_editor::Cursor {
            position: text_editor::Position { line: 0, column: 2 },
            selection: None,
        });
        tab.note_cursor();
        tab.indent("\t");
        assert_eq!(tab.text(), "hi\t");
        assert!(tab.undo());
        assert_eq!(tab.text(), "hi");
    }

    #[test]
    fn enter_repeats_indent_as_one_edit() {
        let mut tab = Tab::testing("    hello");
        tab.content.move_to(text_editor::Cursor {
            position: text_editor::Position { line: 0, column: 9 },
            selection: None,
        });
        tab.note_cursor();
        tab.break_line(Some("    "));
        assert_eq!(tab.text(), "    hello\n    ");
        assert_eq!((tab.cursor_line, tab.cursor_column), (1, 4));
        tab.break_line(Some("    "));
        assert!(tab.undo());
        assert_eq!(tab.text(), "    hello\n    ");
        assert!(tab.undo());
        assert_eq!(tab.text(), "    hello");
    }

    fn file_tab(dir: &Path, name: &str, text: &str) -> (PathBuf, Tab) {
        let path = dir.join(name);
        fs::write(&path, text).unwrap();
        let tab = Tab::from_record(Record {
            id: "file".into(),
            title: name.into(),
            file_path: Some(path.clone()),
            scratch_path: None,
            language: Language::Plain,
            cursor_line: 0,
            cursor_column: 0,
            scroll_lines: 0,
        });
        (path, tab)
    }

    #[test]
    fn the_editor_round_trips_newlines() {
        for sample in ["", "a", "a\n", "a\nb", "a\r\n", "a\n\n", "a\r\nb\r\n"] {
            let tab = Tab::from_parts(
                "id".into(),
                "t".into(),
                None,
                None,
                Language::Plain,
                sample.into(),
                0,
                0,
                0,
                false,
            );
            assert_eq!(tab.text(), sample, "{sample:?}");
            assert_eq!(tab.synced, sample);
        }
    }

    #[test]
    fn a_clean_buffer_reloads_the_file_and_does_not_write_it() {
        let dir = tempfile::tempdir().unwrap();
        let (path, mut tab) = file_tab(dir.path(), "note.txt", "hello\n");
        fs::write(&path, "from the other computer\n").unwrap();
        assert_eq!(tab.reconcile_disk(), Refresh::Reloaded);
        assert_eq!(tab.text(), "from the other computer\n");
        assert!(!tab.dirty);
        assert!(!tab.undo());
        fs::write(&path, "newer\n").unwrap();
        tab.dirty = true;
        tab.flush(false).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "newer\n");
        assert_eq!(tab.text(), "newer\n");
    }

    #[test]
    fn local_edits_stay_when_the_file_is_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let (path, mut tab) = file_tab(dir.path(), "note.txt", "hello\n");
        tab.content = text_editor::Content::with_text("hello\nlocal\n");
        tab.dirty = true;
        assert_eq!(tab.reconcile_disk(), Refresh::Unchanged);
        assert_eq!(tab.text(), "hello\nlocal\n");
        tab.flush(false).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello\nlocal\n");
    }

    #[test]
    fn apart_edits_merge_and_overlapping_ones_are_not_written() {
        let dir = tempfile::tempdir().unwrap();
        let (path, mut tab) = file_tab(dir.path(), "note.txt", "a\n");
        tab.content = text_editor::Content::with_text("a\nlocal\n");
        tab.dirty = true;
        fs::write(&path, "remote\na\n").unwrap();
        assert_eq!(tab.reconcile_disk(), Refresh::Merged);
        assert_eq!(tab.text(), "remote\na\nlocal\n");
        assert!(tab.dirty);
        assert!(tab.undo());
        assert_eq!(tab.text(), "a\nlocal\n");
        tab.redo();
        assert_eq!(tab.text(), "remote\na\nlocal\n");
        tab.flush(false).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "remote\na\nlocal\n");

        let (path, mut tab) = file_tab(dir.path(), "clash.txt", "a\n");
        tab.content = text_editor::Content::with_text("local\n");
        tab.dirty = true;
        fs::write(&path, "remote\n").unwrap();
        assert_eq!(tab.reconcile_disk(), Refresh::Conflict);
        assert!(tab.conflicted());
        let marked = tab.text();
        assert!(tab.flush(false).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "remote\n");
        assert!(tab.undo());
        assert!(tab.flush(false).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "remote\n");
        assert!(tab.redo());
        tab.flush(true).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), marked);
        assert!(!tab.conflicted());
    }

    #[test]
    fn a_failed_reread_does_not_replace_the_buffer_or_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let (path, mut tab) = file_tab(dir.path(), "note.txt", "hello\n");
        tab.content = text_editor::Content::with_text("local\n");
        tab.dirty = true;
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert_eq!(tab.reconcile_disk(), Refresh::Failed);
        assert_eq!(tab.text(), "local\n");
        assert!(tab.flush(false).is_err());
        assert!(path.is_dir());
    }

    #[test]
    fn resolving_the_markers_allows_the_next_save() {
        let dir = tempfile::tempdir().unwrap();
        let (path, mut tab) = file_tab(dir.path(), "note.txt", "a\n");
        tab.content = text_editor::Content::with_text("local\n");
        tab.dirty = true;
        fs::write(&path, "remote\n").unwrap();
        assert_eq!(tab.reconcile_disk(), Refresh::Conflict);
        tab.content = text_editor::Content::with_text("kept both\n");
        tab.refresh_conflict_flag(true);
        assert!(!tab.conflicted());
        tab.dirty = true;
        tab.flush(false).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "kept both\n");
    }
}
