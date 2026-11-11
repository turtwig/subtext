//! A copy of a buffer's wrap, kept so a gutter can follow the editor.
//!
//! The editor lays each line out while it draws. Publishing that result
//! avoids shaping the same file again on the UI thread during a resize.

use crate::buffer::Buffer;
use crate::layout::LayoutLine;

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, LazyLock, Mutex};

/// Exclusive byte end of each visual row, one buffer line at a time.
#[derive(Clone, Debug)]
pub struct LayoutSnap {
    pub line_height: f32,
    pub scroll_line: usize,
    pub scroll_vertical: f32,
    /// Text box height from the last layout. Zero means it is not known yet.
    /// The gutter uses it to ignore a scroll the editor is about to clamp,
    /// and to number a row that only partly fits the box.
    pub viewport_height: f32,
    pub ends: Vec<Vec<usize>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct SnapKey {
    lines: usize,
    bytes: usize,
    first: u64,
    last: u64,
}

struct Store {
    map: HashMap<SnapKey, Arc<LayoutSnap>>,
    order: VecDeque<SnapKey>,
}

const CAP: usize = 128;

fn store() -> &'static Mutex<Store> {
    static STORE: LazyLock<Mutex<Store>> = LazyLock::new(|| {
        Mutex::new(Store {
            map: HashMap::new(),
            order: VecDeque::new(),
        })
    });
    &STORE
}

/// FNV-1a of one buffer line, without its line ending.
pub fn hash_line(text: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

pub fn publish(buffer: &Buffer) {
    if buffer.lines.is_empty() {
        return;
    }
    let mut bytes = 0usize;
    let mut ends = Vec::with_capacity(buffer.lines.len());
    for line in &buffer.lines {
        let text = line.text();
        bytes += text.len();
        ends.push(match line.layout_opt() {
            Some(layout) if !layout.is_empty() => row_ends(text, layout),
            _ => vec![text.len()],
        });
    }
    let key = SnapKey {
        lines: buffer.lines.len(),
        bytes,
        first: hash_line(buffer.lines[0].text()),
        last: hash_line(buffer.lines[buffer.lines.len() - 1].text()),
    };
    let snap = Arc::new(LayoutSnap {
        line_height: buffer.metrics().line_height,
        scroll_line: buffer.scroll().line,
        scroll_vertical: buffer.scroll().vertical,
        viewport_height: buffer.size().1.unwrap_or(0.0),
        ends,
    });
    let Ok(mut store) = store().lock() else {
        return;
    };
    if let Some(index) = store.order.iter().position(|stored| *stored == key) {
        store.order.remove(index);
    }
    store.map.insert(key, snap);
    store.order.push_back(key);
    while store.order.len() > CAP {
        let Some(old) = store.order.pop_front() else {
            break;
        };
        if !store.order.contains(&old) {
            store.map.remove(&old);
        }
    }
}

/// The layout last published for a buffer with this shape.
pub fn layout_snap(lines: usize, bytes: usize, first: u64, last: u64) -> Option<Arc<LayoutSnap>> {
    let key = SnapKey {
        lines,
        bytes,
        first,
        last,
    };
    store().lock().ok()?.map.get(&key).cloned()
}

fn row_ends(text: &str, layout: &[LayoutLine]) -> Vec<usize> {
    let len = text.len();
    let mut ends = Vec::with_capacity(layout.len());
    let mut cursor = 0usize;
    for index in 0..layout.len() {
        let end = if index + 1 == layout.len() {
            len
        } else {
            layout[index + 1]
                .glyphs
                .first()
                .map(|glyph| glyph.start)
                .filter(|start| *start > cursor)
                .unwrap_or(len)
        };
        let end = end.max(cursor);
        ends.push(end);
        cursor = end;
    }
    if ends.is_empty() {
        ends.push(len);
    }
    ends
}
