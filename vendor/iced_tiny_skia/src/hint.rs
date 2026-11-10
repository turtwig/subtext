//! Whether glyph outlines are hinted when they are rasterized.
//!
//! The flag is read while the frame is painted. Layout has already chosen
//! the advances, so this does not change character spacing.

use std::cell::Cell;

thread_local! {
    static ENABLED: Cell<bool> = const { Cell::new(true) };
}

pub fn set_hinting(enabled: bool) {
    ENABLED.with(|cell| cell.set(enabled));
}

pub fn hinting() -> bool {
    ENABLED.with(|cell| cell.get())
}
