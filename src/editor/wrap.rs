//! Visual rows for the gutter.
//!
//! The editor wraps with cosmic-text and publishes that layout. Continuation
//! rows are not buffer lines, so they do not get a number. The gutter reads
//! the published rows instead of wrapping the file a second time.

use cosmic_text::LayoutSnap;

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
    #[cfg_attr(not(test), allow(dead_code))]
    pub caret: u32,
    /// Visual row of the first slot.
    pub scroll: i32,
    /// Buffer line of the first visible row.
    pub top_line: u32,
    /// Pixel offset of the first slot from the editor padding.
    /// Negative when that row is partly above the box.
    pub lead: f32,
}

/// Rows the editor last laid out for this buffer.
pub fn from_snap(
    snap: &LayoutSnap,
    visible: u32,
    caret_line: usize,
    caret_byte: usize,
) -> Viewport {
    let visible = visible.max(1);
    let height = if snap.line_height.is_finite() && snap.line_height > 0.0 {
        snap.line_height
    } else {
        1.0
    };
    let vertical = if snap.scroll_vertical.is_finite() {
        snap.scroll_vertical
    } else {
        0.0
    };
    if snap.ends.is_empty() {
        return blank(visible);
    }
    let known_box = snap.viewport_height.is_finite() && snap.viewport_height > 1.0;
    let box_h = if known_box {
        snap.viewport_height
    } else {
        visible as f32 * height
    };
    // A missing text-box height must not pull the scroll. The fallback box
    // only decides how many numbers to draw.
    let (scroll_line, vertical) = normalize(
        &snap.ends,
        height,
        snap.scroll_line,
        vertical,
        known_box.then_some(snap.viewport_height),
    );

    let mut total = 0u32;
    let mut caret = 0u32;
    let mut saw_caret = false;
    for (index, rows) in snap.ends.iter().enumerate() {
        let sub = subrow(rows, caret_byte);
        for row in 0..rows.len() {
            if index == caret_line && row == sub {
                caret = total;
                saw_caret = true;
            }
            total += 1;
        }
    }
    if total == 0 {
        return blank(visible);
    }
    if !saw_caret {
        caret = caret.min(total - 1);
    }

    let scroll_line = scroll_line.min(snap.ends.len() - 1);
    let mut scroll = 0i32;
    for rows in snap.ends.iter().take(scroll_line) {
        scroll += rows.len() as i32;
    }

    let mut slots = Vec::with_capacity(visible as usize);
    let mut top_line = scroll_line as u32;
    let mut lead = 0.0;
    let mut y = 0.0;
    let mut line_i = scroll_line;
    let mut row_i = 0usize;
    while line_i < snap.ends.len() {
        let rows = &snap.ends[line_i];
        if rows.is_empty() || row_i >= rows.len() {
            line_i += 1;
            row_i = 0;
            continue;
        }
        let top = y - vertical;
        y += height;
        if top + height <= 0.0 {
            scroll += 1;
            row_i += 1;
            continue;
        }
        // The text box is taller than a whole number of rows. A line that
        // still intersects that box gets a number, even when `visible` was
        // rounded down.
        if top >= box_h {
            break;
        }
        if slots.is_empty() {
            lead = top;
            top_line = line_i as u32;
        }
        let sub = subrow(rows, caret_byte);
        slots.push(Slot {
            number: (row_i == 0).then_some(line_i as u32 + 1),
            current: line_i == caret_line && row_i == sub,
        });
        row_i += 1;
    }

    pad(&mut slots, visible);
    Viewport {
        slots,
        total,
        caret,
        scroll,
        top_line,
        lead,
    }
}

pub fn from_ends(
    ends: &[Vec<usize>],
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
    let mut top_line = ends.len().saturating_sub(1) as u32;

    for (index, rows) in ends.iter().enumerate() {
        let rows = if rows.is_empty() {
            &[0][..]
        } else {
            rows.as_slice()
        };
        let sub = subrow(rows, caret_byte);
        for (row, _) in rows.iter().enumerate() {
            if index == caret_line && row == sub {
                caret = total;
                saw_caret = true;
            }
            if total >= scroll && slots.len() < visible as usize {
                if slots.is_empty() {
                    top_line = index as u32;
                }
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
        return blank(visible);
    }
    if !saw_caret {
        caret = caret.min(total - 1);
    }
    pad(&mut slots, visible);
    Viewport {
        slots,
        total,
        caret,
        scroll: scroll as i32,
        top_line,
        lead: 0.0,
    }
}

fn blank(visible: u32) -> Viewport {
    let mut slots = Vec::new();
    pad(&mut slots, visible.max(1));
    if let Some(slot) = slots.first_mut() {
        slot.number = Some(1);
        slot.current = true;
    }
    Viewport {
        slots,
        total: 1,
        caret: 0,
        scroll: 0,
        top_line: 0,
        lead: 0.0,
    }
}

fn pad(slots: &mut Vec<Slot>, visible: u32) {
    while slots.len() < visible as usize {
        slots.push(Slot {
            number: None,
            current: false,
        });
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn viewport<'a>(
    lines: impl Iterator<Item = &'a str>,
    columns: u32,
    wrap: bool,
    scroll: i32,
    visible: u32,
    caret_line: usize,
    caret_byte: usize,
) -> Viewport {
    let ends: Vec<Vec<usize>> = lines
        .map(|line| row_ends(line, if wrap { columns } else { 0 }))
        .collect();
    from_ends(&ends, scroll, visible, caret_line, caret_byte)
}

/// Exclusive byte ends of each visual row. `columns == 0` means no wrapping.
#[cfg_attr(not(test), allow(dead_code))]
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

/// The scroll `shape_until_scroll` will settle on.
///
/// `set_scroll` publishes the raw value first. At the top that value is
/// negative for a frame, and at the bottom it runs past the last line.
/// The glyphs clamp on the following layout. Matching that clamp here keeps
/// the numbers from jumping for that one frame.
fn normalize(
    ends: &[Vec<usize>],
    line_h: f32,
    mut line: usize,
    mut vertical: f32,
    view_h: Option<f32>,
) -> (usize, f32) {
    if ends.is_empty() || line_h <= 0.0 {
        return (0, 0.0);
    }
    if !vertical.is_finite() {
        vertical = 0.0;
    }
    line = line.min(ends.len());
    let guard = ends.len().saturating_mul(4).max(4);
    for _ in 0..guard {
        while vertical < 0.0 {
            if line > 0 {
                line -= 1;
                vertical += line_px(ends, line, line_h);
            } else {
                vertical = 0.0;
                break;
            }
        }
        let Some(view_h) = view_h else {
            while line + 1 < ends.len() {
                let height = line_px(ends, line, line_h);
                if height <= vertical {
                    vertical -= height;
                    line += 1;
                } else {
                    break;
                }
            }
            break;
        };

        let scroll_end = vertical + view_h;
        let mut total = 0.0;
        let start = line.min(ends.len());
        for index in start..ends.len() {
            if total > scroll_end {
                break;
            }
            let height = line_px(ends, index, line_h);
            total += height;
            if index == line && height <= vertical {
                line += 1;
                vertical -= height;
            }
        }
        if total < scroll_end && line > 0 {
            vertical -= scroll_end - total;
        } else if total < scroll_end {
            // Same clamp as `shape_until_scroll`: a short note must not keep
            // a sub-line offset that hides the first line above the box.
            let max_vertical = (total - view_h).max(0.0);
            if vertical > max_vertical {
                vertical = max_vertical;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    if line >= ends.len() {
        line = ends.len() - 1;
    }
    if line == 0 && vertical < 0.0 {
        vertical = 0.0;
    }
    (line, vertical)
}

fn line_px(ends: &[Vec<usize>], line: usize, line_h: f32) -> f32 {
    let rows = ends.get(line).map(|rows| rows.len()).unwrap_or(1).max(1);
    rows as f32 * line_h
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

    #[test]
    fn a_published_layout_numbers_the_editor_rows() {
        let snap = LayoutSnap {
            line_height: 10.0,
            scroll_line: 0,
            scroll_vertical: 10.0,
            viewport_height: 0.0,
            ends: vec![vec![4, 10], vec![3]],
        };
        let view = from_snap(&snap, 3, 0, 6);
        assert_eq!(view.total, 3);
        assert_eq!(
            view.slots
                .iter()
                .map(|slot| slot.number)
                .collect::<Vec<_>>(),
            vec![None, Some(2), None]
        );
        assert!(view.slots[0].current);
        assert_eq!(view.scroll, 1);
        assert_eq!(view.top_line, 0);
        assert!(view.lead.abs() < 0.01);
    }

    #[test]
    fn a_partial_row_shifts_the_gutter_by_the_same_pixels() {
        let snap = LayoutSnap {
            line_height: 29.0,
            scroll_line: 1,
            scroll_vertical: 8.0,
            viewport_height: 0.0,
            ends: vec![vec![1], vec![4, 8]],
        };
        let view = from_snap(&snap, 2, 1, 0);
        assert_eq!(view.slots[0].number, Some(2));
        assert!(view.slots[0].current);
        assert!((view.lead - -8.0).abs() < 0.01);
        assert_eq!(view.top_line, 1);
        assert_eq!(view.scroll, 1);
    }

    fn lines(count: usize) -> Vec<Vec<usize>> {
        vec![vec![1]; count]
    }

    #[test]
    fn scrolling_up_at_the_top_does_not_move_the_numbers() {
        let snap = LayoutSnap {
            line_height: 10.0,
            scroll_line: 0,
            scroll_vertical: -10.0,
            viewport_height: 40.0,
            ends: lines(6),
        };
        let view = from_snap(&snap, 4, 0, 0);
        assert_eq!(view.slots[0].number, Some(1));
        assert!(view.lead.abs() < 0.01);
        assert_eq!(view.scroll, 0);
    }

    #[test]
    fn scrolling_down_at_the_bottom_does_not_move_the_numbers() {
        let snap = LayoutSnap {
            line_height: 10.0,
            scroll_line: 7,
            scroll_vertical: 10.0,
            viewport_height: 30.0,
            ends: lines(10),
        };
        let view = from_snap(&snap, 3, 9, 0);
        assert_eq!(view.slots[0].number, Some(8));
        assert!(view.lead.abs() < 0.01);
        assert_eq!(view.slots[2].number, Some(10));
    }

    #[test]
    fn a_short_note_scrolled_past_the_top_comes_back() {
        // Eleven lines at 22px fit in a 400px box. A leftover one-line scroll
        // must not hide line 1, or the gutter and the glyphs disagree.
        let snap = LayoutSnap {
            line_height: 22.0,
            scroll_line: 0,
            scroll_vertical: 18.0,
            viewport_height: 400.0,
            ends: lines(11),
        };
        let view = from_snap(&snap, 12, 10, 0);
        assert_eq!(view.slots[0].number, Some(1));
        assert!(view.lead.abs() < 0.01, "lead {}", view.lead);
        assert_eq!(view.top_line, 0);
    }

    #[test]
    fn a_remainder_row_still_gets_a_number() {
        let snap = LayoutSnap {
            line_height: 10.0,
            scroll_line: 0,
            scroll_vertical: 0.0,
            viewport_height: 25.0,
            ends: lines(5),
        };
        let view = from_snap(&snap, 2, 0, 0);
        assert_eq!(
            view.slots
                .iter()
                .map(|slot| slot.number)
                .collect::<Vec<_>>(),
            vec![Some(1), Some(2), Some(3)]
        );
    }
}
