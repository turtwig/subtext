//! What the Tab key inserts, and the pure edits behind indent and outdent.

use serde::{Deserialize, Serialize};

/// The characters produced by Tab. Shift+Tab removes one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TabStop {
    Tab,
    Spaces2,
    #[default]
    Spaces4,
}

impl TabStop {
    pub const ALL: &'static [Self] = &[Self::Tab, Self::Spaces2, Self::Spaces4];

    pub fn label(self) -> &'static str {
        match self {
            Self::Tab => "Tab",
            Self::Spaces2 => "2 spaces",
            Self::Spaces4 => "4 spaces",
        }
    }

    pub fn unit(self) -> &'static str {
        match self {
            Self::Tab => "\t",
            Self::Spaces2 => "  ",
            Self::Spaces4 => "    ",
        }
    }
}

impl std::fmt::Display for TabStop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// A caret in the same byte columns the editor stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caret {
    pub line: usize,
    pub column: usize,
}

/// Indent or outdent. `anchor` is the other end of a selection.
///
/// A bare caret inserts `unit` at the caret. A selection indents each covered
/// line instead of replacing the selection. Outdent always removes one unit
/// from the start of the caret line, or of each selected line.
pub fn retab(
    text: &str,
    caret: Caret,
    anchor: Option<Caret>,
    unit: &str,
    outdent: bool,
) -> Option<(String, Caret)> {
    if unit.is_empty() {
        return None;
    }
    let mut lines: Vec<String> = if text.is_empty() {
        vec![String::new()]
    } else {
        text.split('\n').map(str::to_string).collect()
    };
    if lines.is_empty() {
        lines.push(String::new());
    }
    let last = lines.len() - 1;
    let caret_line = caret.line.min(last);
    if !outdent && anchor.is_none() {
        return insert_at(&mut lines, caret_line, caret.column, unit);
    }
    let (start, end) = match anchor {
        Some(anchor) => (
            caret_line.min(anchor.line.min(last)),
            caret_line.max(anchor.line.min(last)),
        ),
        None => (caret_line, caret_line),
    };
    let mut changed = false;
    let mut column = caret.column;
    for index in start..=end {
        if outdent {
            let (next, removed) = strip_one(&lines[index], unit);
            if removed > 0 {
                changed = true;
                if index == caret_line {
                    column = caret.column.saturating_sub(removed);
                }
                lines[index] = next;
            }
        } else {
            lines[index] = format!("{unit}{}", lines[index]);
            changed = true;
            if index == caret_line {
                column = caret.column + unit.len();
            }
        }
    }
    if !changed {
        return None;
    }
    Some((
        lines.join("\n"),
        Caret {
            line: caret_line,
            column,
        },
    ))
}

fn insert_at(
    lines: &mut [String],
    line_index: usize,
    column: usize,
    unit: &str,
) -> Option<(String, Caret)> {
    let line = &lines[line_index];
    let column = column.min(line.len());
    let column = if line.is_char_boundary(column) {
        column
    } else {
        line.len()
    };
    let mut next = String::with_capacity(line.len() + unit.len());
    next.push_str(&line[..column]);
    next.push_str(unit);
    next.push_str(&line[column..]);
    lines[line_index] = next;
    Some((
        lines.join("\n"),
        Caret {
            line: line_index,
            column: column + unit.len(),
        },
    ))
}

/// Insert a newline at the caret. `unit` repeats the leading run of that
/// string from the line the break starts on. `None` is a plain newline.
///
/// A selection is replaced by that newline. The caret ends after the inserted
/// indent. This is one edit.
pub fn break_line(
    text: &str,
    caret: Caret,
    anchor: Option<Caret>,
    unit: Option<&str>,
) -> Option<(String, Caret)> {
    let lines: Vec<String> = if text.is_empty() {
        vec![String::new()]
    } else {
        text.split('\n').map(str::to_string).collect()
    };
    if lines.is_empty() {
        return None;
    }
    let joined = lines.join("\n");
    let caret_at = point_off(&lines, caret.line, caret.column);
    let anchor_at = anchor.map(|anchor| point_off(&lines, anchor.line, anchor.column));
    let (start, end) = match anchor_at {
        Some(anchor_at) if anchor_at < caret_at => (anchor_at, caret_at),
        Some(anchor_at) => (caret_at, anchor_at),
        None => (caret_at, caret_at),
    };
    let indent_line = line_at(&joined, start).min(lines.len() - 1);
    let prefix = unit
        .map(|unit| leading_unit(&lines[indent_line], unit))
        .unwrap_or("");
    let insert = format!("\n{prefix}");
    let mut next = String::with_capacity(joined.len() + insert.len());
    next.push_str(&joined[..start]);
    next.push_str(&insert);
    next.push_str(&joined[end..]);
    let (line, column) = line_column(&next, start + insert.len());
    Some((next, Caret { line, column }))
}

fn leading_unit<'a>(line: &'a str, unit: &str) -> &'a str {
    if unit.is_empty() {
        return "";
    }
    let mut len = 0;
    while line[len..].starts_with(unit) {
        len += unit.len();
        if !line.is_char_boundary(len) {
            break;
        }
    }
    &line[..len]
}

fn point_off(lines: &[String], line: usize, column: usize) -> usize {
    let last = lines.len() - 1;
    let line = line.min(last);
    let mut off = 0;
    for (index, text) in lines.iter().enumerate() {
        if index == line {
            return off + byte_column(text, column);
        }
        off += text.len() + 1;
    }
    off
}

fn byte_column(line: &str, column: usize) -> usize {
    let column = column.min(line.len());
    if line.is_char_boundary(column) {
        return column;
    }
    line.char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index < column)
        .last()
        .unwrap_or(0)
}

fn line_at(text: &str, offset: usize) -> usize {
    text[..offset.min(text.len())]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
}

fn line_column(text: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(text.len());
    let line = line_at(text, offset);
    let start = text[..offset]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    (line, offset - start)
}

/// Delete one tab stop before the caret when the whole line is that indent.
///
/// A partial stop is removed first, so 15 spaces with a 4-space stop become
/// 12. A selection, an empty line, and any other character keep the normal
/// backspace. `None` means the caller should do that instead.
pub fn eat_indent(
    text: &str,
    caret: Caret,
    anchor: Option<Caret>,
    unit: &str,
) -> Option<(String, Caret)> {
    if anchor.is_some() || unit.is_empty() {
        return None;
    }
    let mut lines: Vec<String> = if text.is_empty() {
        vec![String::new()]
    } else {
        text.split('\n').map(str::to_string).collect()
    };
    if lines.is_empty() {
        return None;
    }
    let last = lines.len() - 1;
    let line_index = caret.line.min(last);
    let line = &lines[line_index];
    if !indent_only(line, unit) {
        return None;
    }
    let column = byte_column(line, caret.column);
    if column == 0 {
        return None;
    }
    let width = unit.len();
    let extra = column % width;
    let remove = if extra == 0 { width } else { extra };
    if remove == 0 || remove > column {
        return None;
    }
    let mut next = String::with_capacity(line.len() - remove);
    next.push_str(&line[..column - remove]);
    next.push_str(&line[column..]);
    lines[line_index] = next;
    Some((
        lines.join("\n"),
        Caret {
            line: line_index,
            column: column - remove,
        },
    ))
}

/// The line is a run of the indent character and nothing else.
fn indent_only(line: &str, unit: &str) -> bool {
    if line.is_empty() {
        return false;
    }
    let mark = if unit == "\t" { b'\t' } else { b' ' };
    line.bytes().all(|byte| byte == mark)
}

fn strip_one(line: &str, unit: &str) -> (String, usize) {
    if unit == "\t" {
        return match line.strip_prefix('\t') {
            Some(rest) => (rest.to_string(), 1),
            None => (line.to_string(), 0),
        };
    }
    let width = unit.chars().count();
    let leading = line.chars().take_while(|c| *c == ' ').count();
    let remove = leading.min(width);
    if remove == 0 {
        return (line.to_string(), 0);
    }
    let byte = line
        .char_indices()
        .nth(remove)
        .map(|(index, _)| index)
        .unwrap_or(line.len());
    (line[byte..].to_string(), byte)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_caret_inserts_the_unit_where_it_sits() {
        let (text, caret) =
            retab("hello", Caret { line: 0, column: 2 }, None, "  ", false).unwrap();
        assert_eq!(text, "he  llo");
        assert_eq!(caret, Caret { line: 0, column: 4 });
    }

    #[test]
    fn a_selection_indents_each_line() {
        let (text, caret) = retab(
            "one\ntwo",
            Caret { line: 1, column: 1 },
            Some(Caret { line: 0, column: 0 }),
            "\t",
            false,
        )
        .unwrap();
        assert_eq!(text, "\tone\n\ttwo");
        assert_eq!(caret, Caret { line: 1, column: 2 });
    }

    #[test]
    fn outdent_removes_one_unit_and_leaves_a_shorter_prefix() {
        let (text, _) = retab(
            "    one\n  two",
            Caret { line: 0, column: 4 },
            Some(Caret { line: 1, column: 0 }),
            "    ",
            true,
        )
        .unwrap();
        assert_eq!(text, "one\ntwo");
        assert!(retab("one", Caret { line: 0, column: 0 }, None, "    ", true).is_none());
    }

    #[test]
    fn enter_repeats_whole_units_only() {
        let (text, caret) = break_line(
            "    hello",
            Caret { line: 0, column: 9 },
            None,
            Some("    "),
        )
        .unwrap();
        assert_eq!(text, "    hello\n    ");
        assert_eq!(caret, Caret { line: 1, column: 4 });
        let (text, _) =
            break_line("  hello", Caret { line: 0, column: 7 }, None, Some("    ")).unwrap();
        assert_eq!(text, "  hello\n");
        let (text, caret) =
            break_line("\t\tone", Caret { line: 0, column: 5 }, None, Some("\t")).unwrap();
        assert_eq!(text, "\t\tone\n\t\t");
        assert_eq!(caret, Caret { line: 1, column: 2 });
    }

    #[test]
    fn enter_without_a_unit_is_a_plain_newline() {
        let (text, caret) =
            break_line("    hello", Caret { line: 0, column: 4 }, None, None).unwrap();
        assert_eq!(text, "    \nhello");
        assert_eq!(caret, Caret { line: 1, column: 0 });
    }

    #[test]
    fn backspace_on_a_blank_indent_removes_one_stop() {
        let (text, caret) =
            eat_indent("        ", Caret { line: 0, column: 8 }, None, "    ").unwrap();
        assert_eq!(text, "    ");
        assert_eq!(caret, Caret { line: 0, column: 4 });
        let (text, caret) = eat_indent(
            "               ",
            Caret {
                line: 0,
                column: 15,
            },
            None,
            "    ",
        )
        .unwrap();
        assert_eq!(text, "            ");
        assert_eq!(caret.column, 12);
        let (text, caret) =
            eat_indent("      ", Caret { line: 0, column: 6 }, None, "    ").unwrap();
        assert_eq!(text, "    ");
        assert_eq!(caret.column, 4);
    }

    #[test]
    fn backspace_leaves_a_normal_line_and_a_selection_alone() {
        assert!(eat_indent("    hello", Caret { line: 0, column: 4 }, None, "    ").is_none());
        assert!(eat_indent("", Caret { line: 0, column: 0 }, None, "    ").is_none());
        assert!(
            eat_indent(
                "        ",
                Caret { line: 0, column: 8 },
                Some(Caret { line: 0, column: 4 }),
                "    ",
            )
            .is_none()
        );
        let (text, caret) = eat_indent("\t\t", Caret { line: 0, column: 2 }, None, "\t").unwrap();
        assert_eq!(text, "\t");
        assert_eq!(caret.column, 1);
    }

    #[test]
    fn enter_replaces_a_selection_and_keeps_the_first_line_indent() {
        let (text, caret) = break_line(
            "    ab\n    cd",
            Caret { line: 1, column: 6 },
            Some(Caret { line: 0, column: 4 }),
            Some("    "),
        )
        .unwrap();
        assert_eq!(text, "    \n    ");
        assert_eq!(caret, Caret { line: 1, column: 4 });
    }
}
