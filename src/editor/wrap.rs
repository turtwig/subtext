//! Visual rows for the gutter.
//!
//! The editor wraps with cosmic-text `Wrap::Word`: a word that does not fit
//! moves to the next row, and a word longer than the row stays on one row.
//! Continuation rows are not real lines, so they do not get a number.

/// One visible editor row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    /// 1-based logical line. Empty on a wrapped continuation.
    pub number: Option<u32>,
    /// The caret sits on this visual row.
    pub current: bool,
}

/// Visible rows, plus the total number of visual rows in the buffer.
pub struct Viewport {
    pub slots: Vec<Slot>,
    pub total: u32,
    /// Visual row of the caret, from the start of the buffer.
    pub caret: u32,
}

pub fn viewport<'a>(
    lines: impl Iterator<Item = &'a str>,
    columns: u32,
    wrap: bool,
    scroll: i32,
    visible: u32,
    caret_line: usize,
    caret_byte: usize,
) -> Viewport {
    let visible = visible.max(1);
    let scroll = scroll.max(0) as u32;
    let mut slots = Vec::with_capacity(visible as usize);
    let mut total = 0u32;
    let mut caret = 0u32;
    let mut saw_caret = false;

    for (index, line) in lines.enumerate() {
        let ends = row_ends(line, if wrap { columns } else { 0 });
        let sub = subrow(&ends, caret_byte);
        for (row, _) in ends.iter().enumerate() {
            if index == caret_line && row == sub {
                caret = total;
                saw_caret = true;
            }
            if total >= scroll && slots.len() < visible as usize {
                let on_caret = index == caret_line && row == sub;
                slots.push(Slot {
                    number: (row == 0).then_some(index as u32 + 1),
                    current: on_caret,
                });
            }
            total += 1;
        }
    }

    if total == 0 {
        total = 1;
        if slots.is_empty() {
            slots.push(Slot {
                number: Some(1),
                current: true,
            });
        }
    }
    if !saw_caret {
        caret = caret.min(total.saturating_sub(1));
    }
    while slots.len() < visible as usize {
        slots.push(Slot {
            number: None,
            current: false,
        });
    }
    Viewport {
        slots,
        total,
        caret,
    }
}

/// Exclusive byte ends of each visual row. `columns == 0` means no wrapping.
fn row_ends(text: &str, columns: u32) -> Vec<usize> {
    let columns = columns as usize;
    if columns == 0 || text.is_empty() {
        return vec![text.len()];
    }
    let mut ends = Vec::new();
    let mut column = 0usize;
    let mut word_start = 0usize;
    let mut word_column = 0usize;
    let mut in_word = false;
    let mut byte = 0usize;
    for ch in text.chars() {
        let next = byte + ch.len_utf8();
        if ch.is_whitespace() {
            in_word = false;
            let width = if ch == '\t' { 8 - (column % 8) } else { 1 };
            if column > 0 && column + width > columns {
                ends.push(byte);
                column = 0;
                word_column = 0;
            } else {
                column += width;
            }
        } else {
            if !in_word {
                in_word = true;
                word_start = byte;
                word_column = column;
            }
            if column + 1 > columns {
                if word_column == 0 {
                    column += 1;
                } else {
                    ends.push(word_start);
                    column = column - word_column + 1;
                    word_column = 0;
                }
            } else {
                column += 1;
            }
        }
        byte = next;
    }
    ends.push(text.len());
    if ends.is_empty() {
        ends.push(text.len());
    }
    ends
}

fn subrow(ends: &[usize], index: usize) -> usize {
    ends.iter()
        .position(|end| index < *end)
        .unwrap_or(ends.len().saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbers(text: &str, columns: u32, scroll: i32, visible: u32) -> Vec<Option<u32>> {
        viewport(text.lines(), columns, true, scroll, visible, 0, 0)
            .slots
            .into_iter()
            .map(|slot| slot.number)
            .collect()
    }

    #[test]
    fn unwrapped_lines_keep_their_numbers() {
        let view = viewport(["one", "two"].into_iter(), 2, false, 0, 2, 1, 0);
        assert_eq!(view.total, 2);
        assert_eq!(view.slots[0].number, Some(1));
        assert!(!view.slots[0].current);
        assert_eq!(view.slots[1].number, Some(2));
        assert!(view.slots[1].current);
    }

    #[test]
    fn a_long_word_stays_on_one_row() {
        let view = viewport(["abcdef"].into_iter(), 4, true, 0, 3, 0, 0);
        assert_eq!(view.total, 1);
        assert_eq!(view.slots[0].number, Some(1));
        assert!(view.slots[1].number.is_none());
    }

    #[test]
    fn wrapped_continuations_have_no_number() {
        assert_eq!(numbers("hello world", 8, 0, 3), vec![Some(1), None, None]);
    }

    #[test]
    fn the_caret_marks_the_wrapped_row_it_sits_on() {
        let view = viewport(["hello world"].into_iter(), 8, true, 0, 2, 0, 6);
        assert!(view.slots[0].number.is_some());
        assert!(!view.slots[0].current);
        assert!(view.slots[1].number.is_none());
        assert!(view.slots[1].current);
        assert_eq!(view.caret, 1);
    }

    #[test]
    fn scrolling_is_in_visual_rows() {
        let text = ["hello world", "next"];
        let view = viewport(text.into_iter(), 8, true, 1, 2, 1, 0);
        assert_eq!(
            view.slots
                .iter()
                .map(|slot| slot.number)
                .collect::<Vec<_>>(),
            vec![None, Some(2)]
        );
        assert!(view.slots[1].current);
    }
}
