//! Case-insensitive search over one note.
//!
//! Matches are byte ranges in the same columns the editor caret uses. The
//! comparison folds one source character against one query character, so a
//! lowercase form that changes length (ß and "ss") does not match.

/// A caret, in editor lines and byte columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub line: usize,
    pub column: usize,
}

/// One match. `index` is 1-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    pub start: Span,
    pub end: Span,
    pub index: usize,
    pub total: usize,
}

/// The match at or after `origin`, wrapping to the first.
///
/// A match that already covers `origin` wins, so extending a query stays put.
pub fn seek(text: &str, query: &str, origin: Span) -> Option<Place> {
    let found = found(text, query)?;
    let origin = byte_of(text, origin);
    let index = found
        .iter()
        .position(|hit| hit.start <= origin && origin < hit.end)
        .or_else(|| found.iter().position(|hit| hit.start >= origin))
        .unwrap_or(0);
    Some(place(&found, text, index))
}

/// The next or previous match from the caret.
///
/// When the selection is already a match, this steps off it. Otherwise the
/// next match starts at or after the caret, and the previous one starts
/// before it. Either direction wraps.
pub fn step(
    text: &str,
    query: &str,
    caret: Span,
    anchor: Option<Span>,
    backward: bool,
) -> Option<Place> {
    let found = found(text, query)?;
    let caret_byte = byte_of(text, caret);
    let selected = anchor.and_then(|anchor| {
        let anchor = byte_of(text, anchor);
        let (start, end) = if caret_byte <= anchor {
            (caret_byte, anchor)
        } else {
            (anchor, caret_byte)
        };
        found
            .iter()
            .position(|hit| hit.start == start && hit.end == end)
    });
    let index = if let Some(index) = selected {
        if backward {
            if index == 0 {
                found.len() - 1
            } else {
                index - 1
            }
        } else if index + 1 == found.len() {
            0
        } else {
            index + 1
        }
    } else if backward {
        found
            .iter()
            .rposition(|hit| hit.start < caret_byte)
            .unwrap_or(found.len() - 1)
    } else {
        found
            .iter()
            .position(|hit| hit.start >= caret_byte)
            .unwrap_or(0)
    };
    Some(place(&found, text, index))
}

/// `2/5` on a selected match, `0/5` when others exist, `None` when nothing
/// matches. An empty query has no readout.
pub fn readout(text: &str, query: &str, caret: Span, anchor: Option<Span>) -> Option<String> {
    if query.is_empty() {
        return None;
    }
    let found = found(text, query);
    let Some(found) = found else {
        return Some("None".to_string());
    };
    let caret_byte = byte_of(text, caret);
    let selected = anchor.and_then(|anchor| {
        let anchor = byte_of(text, anchor);
        let (start, end) = if caret_byte <= anchor {
            (caret_byte, anchor)
        } else {
            (anchor, caret_byte)
        };
        found
            .iter()
            .position(|hit| hit.start == start && hit.end == end)
    });
    Some(match selected {
        Some(index) => format!("{}/{}", index + 1, found.len()),
        None => format!("0/{}", found.len()),
    })
}

/// The earlier of two spans. A backwards selection still searches from its start.
pub fn earlier(left: Span, right: Span) -> Span {
    if left.line < right.line || (left.line == right.line && left.column <= right.column) {
        left
    } else {
        right
    }
}

struct Hit {
    start: usize,
    end: usize,
}

fn found(text: &str, query: &str) -> Option<Vec<Hit>> {
    let query: Vec<char> = query.chars().collect();
    if query.is_empty() {
        return None;
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut hits = Vec::new();
    for index in 0..chars.len() {
        if let Some(end) = match_at(&chars, text.len(), &query, index) {
            hits.push(Hit {
                start: chars[index].0,
                end,
            });
        }
    }
    if hits.is_empty() { None } else { Some(hits) }
}

fn match_at(chars: &[(usize, char)], len: usize, query: &[char], index: usize) -> Option<usize> {
    let mut query_index = 0;
    let mut text_index = index;
    while query_index < query.len() {
        let (_, text_char) = chars.get(text_index)?;
        if !same_letter(*text_char, query[query_index]) {
            return None;
        }
        query_index += 1;
        text_index += 1;
    }
    Some(chars.get(text_index).map(|(byte, _)| *byte).unwrap_or(len))
}

fn same_letter(left: char, right: char) -> bool {
    let left: String = left.to_lowercase().collect();
    let right: String = right.to_lowercase().collect();
    left == right
}

fn place(hits: &[Hit], text: &str, index: usize) -> Place {
    let hit = &hits[index];
    Place {
        start: span_at(text, hit.start),
        end: span_at(text, hit.end),
        index: index + 1,
        total: hits.len(),
    }
}

fn span_at(text: &str, byte: usize) -> Span {
    let byte = byte.min(text.len());
    let mut line = 0;
    let mut start = 0;
    for (index, ch) in text.char_indices() {
        if index >= byte {
            break;
        }
        if ch == '\n' {
            line += 1;
            start = index + 1;
        }
    }
    Span {
        line,
        column: byte.saturating_sub(start),
    }
}

fn byte_of(text: &str, span: Span) -> usize {
    let mut line = 0;
    let mut start = 0;
    for (index, ch) in text.char_indices() {
        if ch == '\n' {
            if line == span.line {
                break;
            }
            line += 1;
            start = index + 1;
        }
    }
    if line < span.line {
        return text.len();
    }
    let end = text[start..]
        .find('\n')
        .map(|index| start + index)
        .unwrap_or(text.len());
    start + span.column.min(end.saturating_sub(start))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_is_case_insensitive_and_keeps_byte_columns() {
        let place = seek("say Café", "café", Span { line: 0, column: 0 }).unwrap();
        assert_eq!(place.start, Span { line: 0, column: 4 });
        assert_eq!(place.end.column, 4 + "Café".len());
        assert_eq!(place.index, 1);
        assert_eq!(place.total, 1);
        assert!(seek("straße", "ss", Span { line: 0, column: 0 }).is_none());
        assert!(seek("straße", "ß", Span { line: 0, column: 0 }).is_some());
    }

    #[test]
    fn next_and_previous_wrap() {
        let text = "ab ab";
        let first = Span { line: 0, column: 0 };
        let on_first = step(text, "ab", Span { line: 0, column: 2 }, Some(first), false).unwrap();
        assert_eq!(on_first.start.column, 3);
        assert_eq!(on_first.index, 2);
        let wrapped = step(
            text,
            "ab",
            Span { line: 0, column: 5 },
            Some(Span { line: 0, column: 3 }),
            false,
        )
        .unwrap();
        assert_eq!(wrapped.index, 1);
        let back = step(text, "ab", Span { line: 0, column: 2 }, Some(first), true).unwrap();
        assert_eq!(back.index, 2);
    }

    #[test]
    fn an_empty_query_does_not_match() {
        assert!(seek("abc", "", Span { line: 0, column: 0 }).is_none());
        assert!(readout("abc", "", Span { line: 0, column: 0 }, None).is_none());
        assert_eq!(
            readout("abc", "z", Span { line: 0, column: 0 }, None).as_deref(),
            Some("None")
        );
    }

    #[test]
    fn a_match_on_the_next_line_uses_that_line() {
        let place = seek("one\ntwo", "two", Span { line: 0, column: 0 }).unwrap();
        assert_eq!(place.start, Span { line: 1, column: 0 });
        assert_eq!(place.end.column, 3);
    }

    #[test]
    fn readout_counts_the_selected_match() {
        let text = "say Café";
        let place = seek(text, "café", Span { line: 0, column: 0 }).unwrap();
        assert_eq!(
            readout(text, "café", place.end, Some(place.start)).as_deref(),
            Some("1/1")
        );
        assert_eq!(
            readout(text, "café", Span { line: 0, column: 0 }, None).as_deref(),
            Some("0/1")
        );
    }
}
