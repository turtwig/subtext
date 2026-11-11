//! Pixel sizing for the drawer.
//!
//! The surface size is in pixels and follows the pointer. Columns and rows are
//! the readout of how many monospace cells fit, not the drag step. The tab
//! bar, padding, scrollbar, and resize handles sit outside that grid.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Editor font size in pixels. The cell below is the snapped advance and line
/// height of the default monospace face at this size.
pub const TEXT_SIZE: f32 = 16.0;

/// Whole-pixel width of one column.
pub const CELL_W: u32 = 10;
/// Whole-pixel height of one row.
pub const CELL_H: u32 = 22;

/// Hit target for a resize edge. Drawn as a hairline so it does not read as a frame.
pub const HANDLE: u32 = 4;
/// Shared tab and status row.
pub const BAR: u32 = 40;
/// Find bar between the editor and the tab row.
pub const FIND_BAR: u32 = 36;
/// Padding inside the editor, around the glyphs.
pub const EDITOR_PAD: u32 = 16;
/// Padding when the compact layout is on.
pub const COMPACT_PAD: u32 = 8;
/// Vertical scrollbar reserved beside the editor.
pub const SCROLLBAR: u32 = 12;

/// Editor padding for the saved layout choice.
pub fn editor_pad(compact: bool) -> u32 {
    if compact { COMPACT_PAD } else { EDITOR_PAD }
}

pub const MIN_COLS: u32 = 40;
pub const MIN_ROWS: u32 = 6;
pub const DEFAULT_COLS: u32 = 120;

/// Used until a stretched surface reports the real output width.
pub const FALLBACK_OUTPUT: (u32, u32) = (1920, 1080);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    Bottom,
    Top,
    Left,
    Right,
}

impl Default for Edge {
    fn default() -> Self {
        Self::Bottom
    }
}

impl Edge {
    /// Top and bottom share one saved size. Left and right share the other.
    pub fn horizontal(self) -> bool {
        matches!(self, Self::Top | Self::Bottom)
    }
}

impl fmt::Display for Edge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Bottom => "Bottom",
            Self::Top => "Top",
            Self::Left => "Left",
            Self::Right => "Right",
        })
    }
}

/// Advance and line height for a text size, scaled from the 16px cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub width: u32,
    pub height: u32,
}

impl Cell {
    pub fn for_text_size(text_size: f32) -> Self {
        let scale = if text_size.is_finite() && text_size > 0.0 {
            text_size / TEXT_SIZE
        } else {
            1.0
        };
        Self {
            width: ((CELL_W as f32) * scale).round().clamp(6.0, 40.0) as u32,
            height: ((CELL_H as f32) * scale).round().clamp(12.0, 64.0) as u32,
        }
    }
}

/// Pixels outside the character grid on the horizontal axis.
pub fn horizontal_chrome(edge: Edge, pad: u32) -> u32 {
    let handles = match edge {
        Edge::Top | Edge::Bottom => HANDLE * 2,
        Edge::Left | Edge::Right => HANDLE,
    };
    handles + pad * 2 + SCROLLBAR
}

/// Fixed gutter: digit-width columns for `digits`, plus the gap before the text.
pub fn gutter_width(cell: Cell, digits: u32, pad: u32) -> u32 {
    digits * cell.width + pad
}

/// Pixels outside the character grid on the vertical axis.
pub fn vertical_chrome(edge: Edge, pad: u32) -> u32 {
    match edge {
        Edge::Top | Edge::Bottom => HANDLE + BAR + pad * 2,
        Edge::Left | Edge::Right => HANDLE * 2 + BAR + pad * 2,
    }
}

pub fn pixel_size(
    edge: Edge,
    columns: u32,
    rows: u32,
    cell: Cell,
    gutter: u32,
    pad: u32,
) -> (u32, u32) {
    (
        horizontal_chrome(edge, pad) + gutter + columns * cell.width,
        vertical_chrome(edge, pad) + rows * cell.height,
    )
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn max_columns(edge: Edge, output_width: u32, cell: Cell, gutter: u32, pad: u32) -> u32 {
    output_width.saturating_sub(horizontal_chrome(edge, pad) + gutter) / cell.width.max(1)
}

pub fn max_rows(edge: Edge, output_height: u32, cell: Cell, pad: u32) -> u32 {
    output_height.saturating_sub(vertical_chrome(edge, pad)) / cell.height.max(1)
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn clamp_columns(
    edge: Edge,
    columns: u32,
    output_width: u32,
    cell: Cell,
    gutter: u32,
    pad: u32,
) -> u32 {
    let max = max_columns(edge, output_width, cell, gutter, pad).max(MIN_COLS);
    columns.clamp(MIN_COLS, max)
}

pub fn clamp_rows(edge: Edge, rows: u32, output_height: u32, cell: Cell, pad: u32) -> u32 {
    let max = max_rows(edge, output_height, cell, pad).max(MIN_ROWS);
    rows.clamp(MIN_ROWS, max)
}

/// Default row count: 40% of the output, rounded down to a whole row.
pub fn default_rows(edge: Edge, output_height: u32, cell: Cell, pad: u32) -> u32 {
    let target = output_height.saturating_mul(2) / 5;
    let rows = target.saturating_sub(vertical_chrome(edge, pad)) / cell.height.max(1);
    clamp_rows(edge, rows.max(MIN_ROWS), output_height, cell, pad)
}

/// Top of the editor band and its height, in window pixels.
///
/// The scrollbar uses this same band, so a click lines up with the thumb.
pub fn editor_track(edge: Edge, window_height: u32, extra: u32) -> (f32, f32) {
    let (top, outside) = match edge {
        Edge::Bottom => (HANDLE, HANDLE + BAR + extra),
        Edge::Top => (BAR + extra, HANDLE + BAR + extra),
        Edge::Left | Edge::Right => (HANDLE + BAR + extra, HANDLE * 2 + BAR + extra),
    };
    (top as f32, window_height.saturating_sub(outside) as f32)
}

/// The size to remember when the current one may have been clamped to the output.
///
/// A smaller display must not replace a larger preference. A size the user
/// actually set, below the output, replaces it.
pub fn preferred_size(memory: u32, current: u32, output: Option<u32>) -> u32 {
    if current == 0 {
        return memory;
    }
    // Until a configure arrives, the live size may already be the clamped one
    // from the last session. Keep the larger stored preference.
    if memory > current && output.is_none_or(|limit| limit > 0 && current >= limit) {
        return memory;
    }
    current
}

/// Thumb height and its offset from the top of the track.
pub fn thumb_span(track_h: f32, visible: u32, total: u32, scroll: i32) -> (f32, f32) {
    if track_h <= 1.0 {
        return (track_h.max(0.0), 0.0);
    }
    let total = total.max(1);
    let visible = visible.max(1).min(total);
    let thumb = if track_h <= 18.0 {
        track_h
    } else {
        (track_h * visible as f32 / total as f32).clamp(18.0, track_h)
    };
    let max_scroll = total.saturating_sub(visible) as f32;
    let travel = (track_h - thumb).max(0.0);
    let offset = if max_scroll <= 0.0 {
        0.0
    } else {
        travel * (scroll.max(0) as f32 / max_scroll)
    };
    (thumb, offset.clamp(0.0, travel))
}

/// How many screen pixels of pointer travel become one pixel of window growth.
///
/// A top or bottom drawer is centered, so the width grows on both sides and the
/// dragged edge only moves half as far as the width change. A left or right
/// drawer is centered vertically, so the same applies to the height. The
/// anchored axis stays put and tracks the pointer one pixel at a time.
#[cfg_attr(not(test), allow(dead_code))]
pub fn resize_scale(edge: Edge) -> (i32, i32) {
    match edge {
        Edge::Top | Edge::Bottom => (2, 1),
        Edge::Left | Edge::Right => (1, 2),
    }
}

/// How far the window's top-left has moved since the drag started.
///
/// `confirmed` is the size the compositor has actually applied. The pointer's
/// window-local position is measured in that frame, so the shift has to use
/// the same size. A positive shift means the top-left moved right or down.
#[cfg_attr(not(test), allow(dead_code))]
pub fn window_origin_shift(edge: Edge, start: (u32, u32), confirmed: (u32, u32)) -> (f32, f32) {
    let grown_w = confirmed.0 as i32 - start.0 as i32;
    let grown_h = confirmed.1 as i32 - start.1 as i32;
    let shift_x = match edge {
        Edge::Top | Edge::Bottom => -(grown_w as f32) / 2.0,
        Edge::Left => 0.0,
        Edge::Right => -(grown_w as f32),
    };
    let shift_y = match edge {
        Edge::Left | Edge::Right => -(grown_h as f32) / 2.0,
        Edge::Top => 0.0,
        Edge::Bottom => -(grown_h as f32),
    };
    (shift_x, shift_y)
}

/// Pointer movement in screen pixels since the drag started.
///
/// `local` and `origin` are window-local. `shift` is the top-left movement
/// from [`window_origin_shift`].
#[cfg_attr(not(test), allow(dead_code))]
pub fn screen_delta(origin: (f32, f32), local: (f32, f32), shift: (f32, f32)) -> (f32, f32) {
    (local.0 - origin.0 + shift.0, local.1 - origin.1 + shift.1)
}

/// How long the drawer takes to slide on or off its edge.
pub const SLIDE_MS: u64 = 150;

/// Ease-out position between `from` and `to`. The second value is true once
/// the slide has finished.
pub fn slide_progress(from: f32, to: f32, elapsed: std::time::Duration) -> (f32, bool) {
    let duration = std::time::Duration::from_millis(SLIDE_MS);
    let t = if elapsed >= duration {
        1.0
    } else {
        elapsed.as_secs_f32() / duration.as_secs_f32()
    };
    let eased = 1.0 - (1.0 - t).powi(3);
    let done = t >= 1.0;
    let value = if done { to } else { from + (to - from) * eased };
    (value.clamp(0.0, 1.0), done)
}

/// Top-left of the drawer on an output-sized surface.
///
/// The surface itself stays anchored to every edge. The drawer is a rectangle
/// on one of them, centered on the free axis.
pub fn drawer_origin(edge: Edge, output: (u32, u32), drawer: (u32, u32)) -> (i32, i32) {
    let (output_w, output_h) = (output.0 as i32, output.1 as i32);
    let (drawer_w, drawer_h) = (drawer.0 as i32, drawer.1 as i32);
    match edge {
        Edge::Bottom => ((output_w - drawer_w) / 2, output_h - drawer_h),
        Edge::Top => ((output_w - drawer_w) / 2, 0),
        Edge::Left => (0, (output_h - drawer_h) / 2),
        Edge::Right => (output_w - drawer_w, (output_h - drawer_h) / 2),
    }
}

/// Top-left while `slide` moves from 0 (fully past the anchor edge) to 1
/// (resting). The distance is the drawer itself, so a hidden drawer, its
/// blur, and its hit target are all off the output.
pub fn slide_origin(edge: Edge, output: (u32, u32), drawer: (u32, u32), slide: f32) -> (i32, i32) {
    let (x, y) = drawer_origin(edge, output, drawer);
    let span = match edge {
        Edge::Top | Edge::Bottom => drawer.1,
        Edge::Left | Edge::Right => drawer.0,
    };
    let along = (span as f32 * (1.0 - slide.clamp(0.0, 1.0))).round() as i32;
    let (dx, dy) = match edge {
        Edge::Bottom => (0, along),
        Edge::Top => (0, -along),
        Edge::Left => (-along, 0),
        Edge::Right => (along, 0),
    };
    (x + dx, y + dy)
}

/// The part of `origin`/`size` that lies on the output. `None` when the drawer
/// is fully past the edge, which is an empty input region.
pub fn output_intersection(
    origin: (i32, i32),
    size: (u32, u32),
    output: (u32, u32),
) -> Option<(i32, i32, i32, i32)> {
    let (x, y) = origin;
    let (w, h) = (size.0 as i32, size.1 as i32);
    let (ow, oh) = (output.0 as i32, output.1 as i32);
    let x1 = x.max(0);
    let y1 = y.max(0);
    let x2 = (x + w).min(ow);
    let y2 = (y + h).min(oh);
    let width = x2 - x1;
    let height = y2 - y1;
    if width <= 0 || height <= 0 {
        None
    } else {
        Some((x1, y1, width, height))
    }
}

/// Drawer size while the surface covers the whole output.
///
/// `pointer` is in output coordinates. `origin` is the press position in the
/// drawer. The grip's offset from the moving edge is kept so the edge stays
/// under the cursor. A centered axis grows by twice the pointer travel. An
/// anchored axis grows one to one.
pub fn capture_size(
    edge: Edge,
    output: (u32, u32),
    start: (u32, u32),
    columns_sign: i32,
    rows_sign: i32,
    origin: (f32, f32),
    pointer: (f32, f32),
) -> (u32, u32) {
    let offset_x = moving_edge_offset(columns_sign, origin.0, start.0);
    let offset_y = moving_edge_offset(rows_sign, origin.1, start.1);
    let width = match edge {
        Edge::Top | Edge::Bottom => {
            centered_span(columns_sign, output.0, pointer.0, offset_x, start.0)
        }
        Edge::Left => anchored_start(columns_sign, pointer.0, offset_x, start.0),
        Edge::Right => anchored_end(columns_sign, output.0, pointer.0, offset_x, start.0),
    };
    let height = match edge {
        Edge::Left | Edge::Right => {
            centered_span(rows_sign, output.1, pointer.1, offset_y, start.1)
        }
        Edge::Top => anchored_start(rows_sign, pointer.1, offset_y, start.1),
        Edge::Bottom => anchored_end(rows_sign, output.1, pointer.1, offset_y, start.1),
    };
    (width, height)
}

/// Pointer offset from the edge this grip moves. The near edge is local 0.
fn moving_edge_offset(sign: i32, origin: f32, start: u32) -> f32 {
    if sign > 0 {
        origin - start as f32
    } else {
        origin
    }
}

fn centered_span(sign: i32, output: u32, pointer: f32, offset: f32, start: u32) -> u32 {
    if sign == 0 {
        return start;
    }
    let edge = pointer - offset;
    let size = if sign > 0 {
        2.0 * edge - output as f32
    } else {
        output as f32 - 2.0 * edge
    };
    size.round().max(1.0) as u32
}

fn anchored_start(sign: i32, pointer: f32, offset: f32, start: u32) -> u32 {
    if sign == 0 {
        return start;
    }
    (pointer - offset).round().max(1.0) as u32
}

fn anchored_end(sign: i32, output: u32, pointer: f32, offset: f32, start: u32) -> u32 {
    if sign == 0 {
        return start;
    }
    (output as f32 - (pointer - offset)).round().max(1.0) as u32
}

/// Size change along one axis. `sign` is the grip direction, `scale` comes
/// from [`resize_scale`], and `screen_px` is the pointer movement on that axis.
#[cfg_attr(not(test), allow(dead_code))]
pub fn resize_delta(sign: i32, scale: i32, screen_px: f32) -> i32 {
    if sign == 0 || scale == 0 {
        return 0;
    }
    let pixels = screen_px.round() as i32;
    sign.saturating_mul(scale).saturating_mul(pixels)
}

/// Buffer line for a pointer sitting `local_y` pixels into the track.
///
/// `grab` is how far into the thumb the pointer grabbed it.
pub fn scroll_line(local_y: f32, grab: f32, track_h: f32, thumb: f32, max_scroll: i32) -> i32 {
    if max_scroll <= 0 {
        return 0;
    }
    let travel = (track_h - thumb).max(1.0);
    let pos = (local_y - grab).clamp(0.0, travel);
    let line = (pos / travel) * max_scroll as f32;
    line.round().clamp(0.0, max_scroll as f32) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_size_is_chrome_plus_whole_cells() {
        let cell = Cell::for_text_size(TEXT_SIZE);
        let pad = EDITOR_PAD;
        let (w, h) = pixel_size(Edge::Bottom, 120, 10, cell, 0, pad);
        assert_eq!(w, horizontal_chrome(Edge::Bottom, pad) + 120 * CELL_W);
        assert_eq!(h, vertical_chrome(Edge::Bottom, pad) + 10 * CELL_H);
        assert_eq!(w % 2, horizontal_chrome(Edge::Bottom, pad) % 2);
    }

    #[test]
    fn compact_padding_reclaims_room_from_every_side() {
        let cell = Cell::for_text_size(TEXT_SIZE);
        let breezy = pixel_size(Edge::Bottom, 120, 10, cell, 0, editor_pad(false));
        let compact = pixel_size(Edge::Bottom, 120, 10, cell, 0, editor_pad(true));
        assert_eq!(breezy.0 - compact.0, (EDITOR_PAD - COMPACT_PAD) * 2);
        assert_eq!(breezy.1 - compact.1, (EDITOR_PAD - COMPACT_PAD) * 2);
        assert_eq!(
            gutter_width(cell, 1, editor_pad(true)),
            gutter_width(cell, 1, editor_pad(false)) - 8
        );
        // 5-digit gutter is wider than a 1-digit one by 4 cells.
        assert_eq!(
            gutter_width(cell, 5, editor_pad(true)) - gutter_width(cell, 1, editor_pad(true)),
            4 * cell.width
        );
    }

    #[test]
    fn clamps_to_cells_that_fit_the_output() {
        let edge = Edge::Bottom;
        let cell = Cell::for_text_size(TEXT_SIZE);
        let pad = EDITOR_PAD;
        let output_w = horizontal_chrome(edge, pad) + 50 * CELL_W + 7;
        assert_eq!(clamp_columns(edge, 120, output_w, cell, 0, pad), 50);
        assert_eq!(clamp_columns(edge, 10, output_w, cell, 0, pad), MIN_COLS);
    }

    #[test]
    fn default_rows_are_about_forty_percent() {
        let cell = Cell::for_text_size(TEXT_SIZE);
        let pad = EDITOR_PAD;
        let rows = default_rows(Edge::Bottom, 1080, cell, pad);
        let pixels = pixel_size(Edge::Bottom, DEFAULT_COLS, rows, cell, 0, pad).1;
        assert!(pixels <= 1080 * 2 / 5 + CELL_H);
        assert!(rows >= MIN_ROWS);
    }

    #[test]
    fn editor_track_sits_under_the_top_grip() {
        let (top, height) = editor_track(Edge::Bottom, 400, 0);
        assert_eq!(top, HANDLE as f32);
        assert_eq!(height, (400 - HANDLE - BAR) as f32);
        let (top, height) = editor_track(Edge::Top, 400, FIND_BAR);
        assert_eq!(top, (BAR + FIND_BAR) as f32);
        assert_eq!(height, (400 - HANDLE - BAR - FIND_BAR) as f32);
    }

    #[test]
    fn thumb_shrinks_and_tracks_the_scroll() {
        let (thumb, offset) = thumb_span(200.0, 10, 40, 0);
        assert!((thumb - 50.0).abs() < 0.1);
        assert_eq!(offset, 0.0);
        let (thumb, offset) = thumb_span(200.0, 10, 40, 30);
        assert!((thumb - 50.0).abs() < 0.1);
        assert!((offset - 150.0).abs() < 0.1);
    }

    #[test]
    fn centered_width_stays_under_the_pointer_after_the_window_moves() {
        let edge = Edge::Bottom;
        let start = (1000, 400);
        let origin = (1000.0, 2.0);
        // Pointer moved 10px right. Width grew by 20 and the compositor
        // recentered, so the local x jumped by 20 while the screen move is 10.
        let confirmed = (1020, 400);
        let shift = window_origin_shift(edge, start, confirmed);
        assert_eq!(shift, (-10.0, 0.0));
        let (screen_x, _) = screen_delta(origin, (1020.0, 2.0), shift);
        assert_eq!(screen_x, 10.0);
        let (width_scale, height_scale) = resize_scale(edge);
        assert_eq!((width_scale, height_scale), (2, 1));
        assert_eq!(resize_delta(1, width_scale, screen_x), 20);
    }

    #[test]
    fn width_drag_uses_screen_movement_before_the_compositor_catches_up() {
        let edge = Edge::Bottom;
        let start = (1000, 400);
        let shift = window_origin_shift(edge, start, start);
        assert_eq!(shift, (0.0, 0.0));
        let (screen_x, _) = screen_delta((1000.0, 2.0), (1010.0, 2.0), shift);
        assert_eq!(resize_delta(1, resize_scale(edge).0, screen_x), 20);
    }

    #[test]
    fn bottom_edge_height_tracks_one_to_one_after_the_top_moves() {
        let edge = Edge::Bottom;
        let start = (1000, 400);
        let origin = (10.0, 2.0);
        let confirmed = (1000, 410);
        let shift = window_origin_shift(edge, start, confirmed);
        assert_eq!(shift, (0.0, -10.0));
        // The top moved up with the pointer, so the local y is unchanged.
        let (_, screen_y) = screen_delta(origin, origin, shift);
        assert_eq!(screen_y, -10.0);
        assert_eq!(resize_delta(-1, resize_scale(edge).1, screen_y), 10);
    }

    #[test]
    fn anchored_left_edge_does_not_double_the_width() {
        let edge = Edge::Left;
        let start = (800, 500);
        let confirmed = (820, 500);
        let shift = window_origin_shift(edge, start, confirmed);
        assert_eq!(shift, (0.0, 0.0));
        assert_eq!(resize_scale(edge), (1, 2));
        let (screen_x, _) = screen_delta((800.0, 4.0), (810.0, 4.0), shift);
        assert_eq!(resize_delta(1, resize_scale(edge).0, screen_x), 10);
    }

    #[test]
    fn drawer_origin_pins_each_edge() {
        assert_eq!(
            drawer_origin(Edge::Bottom, (1920, 1080), (1000, 400)),
            (460, 680)
        );
        assert_eq!(
            drawer_origin(Edge::Top, (1920, 1080), (1000, 400)),
            (460, 0)
        );
        assert_eq!(
            drawer_origin(Edge::Left, (1920, 1080), (400, 400)),
            (0, 340)
        );
        assert_eq!(
            drawer_origin(Edge::Right, (1920, 1080), (400, 400)),
            (1520, 340)
        );
        assert_eq!(
            drawer_origin(Edge::Bottom, (1920, 1080), (1920, 400)),
            (0, 680)
        );
    }

    #[test]
    fn capture_keeps_a_centered_edge_under_the_pointer() {
        let output = (1920, 1080);
        let start = (1000, 400);
        // Press 2px inside the right edge. The drawer is centered.
        let origin = (998.0, 2.0);
        let screen_x = (output.0 - start.0) as f32 / 2.0 + origin.0;
        let (width, height) =
            capture_size(Edge::Bottom, output, start, 1, 0, origin, (screen_x, 682.0));
        assert_eq!((width, height), start);
        let (width, _) = capture_size(
            Edge::Bottom,
            output,
            start,
            1,
            0,
            origin,
            (screen_x + 10.0, 682.0),
        );
        assert_eq!(width, start.0 + 20);
    }

    #[test]
    fn capture_grows_a_bottom_drawer_one_to_one() {
        let output = (1920, 1080);
        let start = (1000, 400);
        let origin = (10.0, 2.0);
        let screen_y = (output.1 - start.1) as f32 + origin.1;
        let (_, height) = capture_size(
            Edge::Bottom,
            output,
            start,
            0,
            -1,
            origin,
            (500.0, screen_y - 10.0),
        );
        assert_eq!(height, start.1 + 10);
    }

    #[test]
    fn capture_does_not_double_an_anchored_width() {
        let output = (1920, 1080);
        let start = (400, 400);
        let origin = (398.0, 10.0);
        let (width, _) = capture_size(Edge::Left, output, start, 1, 0, origin, (408.0, 400.0));
        assert_eq!(width, start.0 + 10);
    }

    #[test]
    fn capture_doubles_centered_height_on_a_side_drawer() {
        let output = (1920, 1080);
        let start = (400, 400);
        let origin = (10.0, 398.0);
        let screen_y = (output.1 - start.1) as f32 / 2.0 + origin.1;
        let (_, height) = capture_size(
            Edge::Left,
            output,
            start,
            0,
            1,
            origin,
            (10.0, screen_y + 10.0),
        );
        assert_eq!(height, start.1 + 20);
    }

    #[test]
    fn a_clamped_drawer_keeps_the_larger_preference() {
        assert_eq!(preferred_size(1400, 1000, Some(1000)), 1400);
        assert_eq!(preferred_size(1400, 900, Some(1000)), 900);
        assert_eq!(preferred_size(800, 1000, Some(1920)), 1000);
        assert_eq!(preferred_size(800, 0, Some(1000)), 800);
        assert_eq!(preferred_size(1400, 1000, None), 1400);
        assert_eq!(preferred_size(0, 1000, None), 1000);
    }

    #[test]
    fn scroll_line_maps_the_thumb_travel() {
        assert_eq!(scroll_line(0.0, 0.0, 200.0, 50.0, 30), 0);
        assert_eq!(scroll_line(150.0, 0.0, 200.0, 50.0, 30), 30);
        assert_eq!(scroll_line(75.0, 0.0, 200.0, 50.0, 30), 15);
        assert_eq!(scroll_line(10.0, 0.0, 200.0, 50.0, 0), 0);
    }

    #[test]
    fn slide_eases_out_and_lands_on_the_target() {
        let (at, done) = slide_progress(0.0, 1.0, std::time::Duration::from_millis(0));
        assert_eq!(at, 0.0);
        assert!(!done);
        let (at, done) = slide_progress(0.0, 1.0, std::time::Duration::from_millis(SLIDE_MS / 2));
        assert!((at - 0.875).abs() < 0.02);
        assert!(!done);
        let (at, done) = slide_progress(1.0, 0.0, std::time::Duration::from_millis(SLIDE_MS));
        assert_eq!(at, 0.0);
        assert!(done);
    }

    #[test]
    fn a_hidden_drawer_sits_fully_past_its_edge() {
        let output = (1920, 1080);
        let drawer = (1000, 400);
        assert_eq!(
            slide_origin(Edge::Bottom, output, drawer, 1.0),
            drawer_origin(Edge::Bottom, output, drawer)
        );
        assert_eq!(slide_origin(Edge::Bottom, output, drawer, 0.5), (460, 880));
        assert_eq!(slide_origin(Edge::Bottom, output, drawer, 0.0), (460, 1080));
        assert_eq!(slide_origin(Edge::Top, output, drawer, 0.0), (460, -400));
        assert_eq!(
            slide_origin(Edge::Left, output, (400, 400), 0.0),
            (-400, 340)
        );
        assert_eq!(
            slide_origin(Edge::Right, output, (400, 400), 0.0),
            (1920, 340)
        );
        assert_eq!(
            output_intersection((460, 880), drawer, output),
            Some((460, 880, 1000, 200))
        );
        assert!(
            output_intersection(
                slide_origin(Edge::Bottom, output, drawer, 0.0),
                drawer,
                output
            )
            .is_none()
        );
    }
}
