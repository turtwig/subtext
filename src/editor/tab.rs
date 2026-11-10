//! One open scratch or file, backed by iced's text editor buffer.

use super::indent::{self, Caret};
use super::syntax::Language;
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

impl Tab {
    pub fn new_scratch(dir: &Path, index: u32) -> Self {
        let scratch_path = dir.join(format!("scratch_{index}.txt"));
        let text = fs::read_to_string(&scratch_path).unwrap_or_default();
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
        ))
    }

    pub fn from_record(record: Record) -> Self {
        let path = record
            .file_path
            .clone()
            .or_else(|| record.scratch_path.clone());
        let text = path
            .as_ref()
            .and_then(|path| fs::read_to_string(path).ok())
            .unwrap_or_default();
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
        Self {
            id,
            title,
            file_path,
            scratch_path,
            language,
            content,
            dirty: false,
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

    /// Applies `action`. Returns whether it was a wheel scroll, which should
    /// not be pulled back to the cursor.
    pub fn edit(&mut self, action: Action) -> bool {
        let wheel = matches!(action, Action::Scroll { .. });
        if let Action::Scroll { lines } = &action {
            self.scroll_lines = self.scroll_lines.saturating_add(*lines);
        }
        let edits = action.is_edit();
        let before = edits.then(|| self.mark());
        self.content.perform(action);
        self.note_cursor();
        if let Some(before) = before
            && (before.text != self.text()
                || before.line != self.cursor_line
                || before.column != self.cursor_column)
        {
            self.push_history(before);
            self.dirty = true;
        }
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
        self.content = text_editor::Content::with_text(text);
        self.cursor_line = line;
        self.cursor_column = column;
        // The new buffer is scrolled to the top. `sync_scroll` measures from here.
        self.scroll_lines = 0;
        self.content.move_to(text_editor::Cursor {
            position: text_editor::Position { line, column },
            selection: None,
        });
        self.dirty = true;
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

    /// Keep the tracked scroll inside the buffer, and on screen with the caret
    /// after a move or an edit. Wheel scrolls are only clamped.
    ///
    /// `scroll_lines` counts visual rows. A wrapped line adds rows that are
    /// not new buffer lines, and the editor scrolls one of those per tick.
    pub fn sync_scroll(&mut self, columns: u32, wrap: bool, visible_rows: u32, wheel: bool) {
        let view = self.viewport(columns, wrap, 0, 1);
        let visible = visible_rows.max(1) as i32;
        let max_scroll = (view.total as i32 - visible).max(0);
        let mut scroll = self.scroll_lines;
        if !wheel {
            let line = view.caret as i32;
            if line < scroll {
                scroll = line;
            } else if line >= scroll + visible {
                scroll = line - visible + 1;
            }
        }
        scroll = scroll.clamp(0, max_scroll);
        self.scroll_by(scroll - self.scroll_lines);
    }

    /// Move the viewport so the caret's visual row is the first row.
    /// Used when wrap changes, because the editor's pixel scroll is not a
    /// logical line index.
    pub fn show_caret(&mut self, columns: u32, wrap: bool, visible_rows: u32) {
        let view = self.viewport(columns, wrap, 0, 1);
        let visible = visible_rows.max(1) as i32;
        let max_scroll = (view.total as i32 - visible).max(0);
        let scroll = (view.caret as i32).clamp(0, max_scroll);
        self.scroll_by(scroll - self.scroll_lines);
    }

    pub fn viewport(
        &self,
        columns: u32,
        wrap: bool,
        scroll: i32,
        visible: u32,
    ) -> super::wrap::Viewport {
        let lines: Vec<String> = self
            .content
            .lines()
            .map(|line| line.text.into_owned())
            .collect();
        super::wrap::viewport(
            lines.iter().map(String::as_str),
            columns,
            wrap,
            scroll,
            visible,
            self.cursor_line,
            self.cursor_column,
        )
    }

    fn scroll_by(&mut self, delta: i32) {
        if delta == 0 {
            return;
        }
        self.scroll_lines += delta;
        self.content.perform(Action::Scroll { lines: delta });
    }

    pub fn text(&self) -> String {
        self.content.text()
    }

    /// A scratch tab with something in the buffer. Closing it deletes the file.
    pub fn needs_close_confirm(&self) -> bool {
        self.file_path.is_none() && !self.content.is_empty()
    }

    /// Path the buffer is written to. Real files win over the scratch copy.
    pub fn store_path(&self) -> Option<&Path> {
        self.file_path.as_deref().or(self.scratch_path.as_deref())
    }

    pub fn write_buffer(&mut self) -> Result<(), std::io::Error> {
        let Some(path) = self.store_path().map(|path| path.to_path_buf()) else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, self.text())?;
        self.dirty = false;
        Ok(())
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
}
