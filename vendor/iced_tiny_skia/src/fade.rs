//! Optional alpha multiplier for primitives recorded on this thread.
//!
//! The drawer fades in and out. Widget colors are chosen before that fade, so
//! the multiplier is applied when a quad or glyph is recorded.

use crate::core::{Background, Color};
use std::cell::Cell;

thread_local! {
    static FACTOR: Cell<f32> = const { Cell::new(1.0) };
}

/// Sets the alpha multiplier and returns the previous one.
///
/// `1.0` records colors unchanged. The caller restores the previous value
/// after the faded subtree has been recorded.
pub fn set_draw_opacity(opacity: f32) -> f32 {
    let opacity = if opacity.is_finite() {
        opacity.clamp(0.0, 1.0)
    } else {
        1.0
    };
    FACTOR.with(|factor| factor.replace(opacity))
}

fn factor() -> f32 {
    FACTOR.with(Cell::get)
}

pub(crate) fn color(color: Color) -> Color {
    let factor = factor();
    if factor >= 1.0 {
        color
    } else {
        color.scale_alpha(factor)
    }
}

pub(crate) fn background(background: Background) -> Background {
    let factor = factor();
    if factor >= 1.0 {
        background
    } else {
        background.scale_alpha(factor)
    }
}
