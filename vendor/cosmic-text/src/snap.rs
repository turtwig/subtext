// SPDX-License-Identifier: MIT OR Apache-2.0

//! Snap a monospace advance to the hinted pixel cell.
//!
//! Shaping uses the font's linear advance. A terminal's slight hinter rounds
//! that cell onto a whole pixel, which is narrower for some faces (3270 at
//! 16pt is 11px hinted, 11.52px linear). The snap is the hinted advance of
//! the space glyph. Proportional faces are left alone.

use crate::{FontSystem, ShapeGlyph, ShapeLine};
use core::cell::RefCell;
use skrifa::instance::{LocationRef, NormalizedCoord, Size};
use skrifa::outline::pen::NullPen;
use skrifa::outline::{DrawSettings, HintingInstance, SmoothMode, Target};
use skrifa::raw::FontRef;
use skrifa::MetadataProvider;
use std::collections::HashMap;

#[derive(Clone, Copy)]
struct CellSnap {
    linear: f32,
    hinted: f32,
}

impl CellSnap {
    fn apply(self, linear_px: f32) -> f32 {
        if self.linear <= f32::EPSILON {
            return linear_px;
        }
        let cells = linear_px / self.linear;
        let nearest = cells.round();
        // Only whole cells. A mark or a fractional cluster keeps its own advance.
        if (cells - nearest).abs() > 0.2 {
            return linear_px;
        }
        nearest * self.hinted
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct SnapKey {
    id: fontdb::ID,
    weight: u16,
    size_bits: u32,
}

struct Frame {
    map: HashMap<SnapKey, CellSnap>,
}

thread_local! {
    static CACHE: RefCell<HashMap<SnapKey, Option<CellSnap>>> = RefCell::new(HashMap::new());
    static FRAME: RefCell<Option<Frame>> = const { RefCell::new(None) };
}

pub struct Guard;

impl Drop for Guard {
    fn drop(&mut self) {
        FRAME.with(|slot| slot.borrow_mut().take());
    }
}

/// Hinted cell widths for the monospace faces in this line.
pub fn install(font_system: &mut FontSystem, line: &ShapeLine, font_size: f32) -> Guard {
    let mut map = HashMap::new();
    for span in &line.spans {
        for word in &span.words {
            for glyph in &word.glyphs {
                let size = glyph_size(glyph, font_size);
                let key = SnapKey {
                    id: glyph.font_id,
                    weight: glyph.font_weight.0,
                    size_bits: size.to_bits(),
                };
                if map.contains_key(&key) {
                    continue;
                }
                let cached = CACHE.with(|slot| slot.borrow().get(&key).copied());
                let snap = match cached {
                    Some(snap) => snap,
                    None => {
                        let snap = cell_snap(font_system, glyph, size);
                        CACHE.with(|slot| slot.borrow_mut().insert(key, snap));
                        snap
                    }
                };
                if let Some(snap) = snap {
                    map.insert(key, snap);
                }
            }
        }
    }
    FRAME.with(|slot| *slot.borrow_mut() = Some(Frame { map }));
    Guard
}

pub fn advance(glyph: &ShapeGlyph, size: f32) -> f32 {
    let linear = size * glyph.x_advance;
    FRAME.with(|slot| {
        let frame = slot.borrow();
        let Some(frame) = frame.as_ref() else {
            return linear;
        };
        let key = SnapKey {
            id: glyph.font_id,
            weight: glyph.font_weight.0,
            size_bits: size.to_bits(),
        };
        match frame.map.get(&key) {
            Some(snap) => snap.apply(linear),
            None => linear,
        }
    })
}

fn glyph_size(glyph: &ShapeGlyph, font_size: f32) -> f32 {
    glyph.metrics_opt.map_or(font_size, |metrics| metrics.font_size)
}

fn cell_snap(font_system: &mut FontSystem, glyph: &ShapeGlyph, size: f32) -> Option<CellSnap> {
    if size <= 0.0 {
        return None;
    }
    let mono = font_system
        .db()
        .face(glyph.font_id)
        .is_some_and(|face| face.monospaced);
    if !mono {
        return None;
    }
    let font = font_system.get_font(glyph.font_id, glyph.font_weight)?;
    let font_ref = FontRef::from_index(font.data(), font.face_index()).ok()?;
    let outlines = font_ref.outline_glyphs();
    let gid = font_ref.charmap().map(' ')?;
    let outline = outlines.get(gid)?;
    let coords: &[NormalizedCoord] = &[];
    let px = Size::new(size);
    let linear = outline
        .draw(DrawSettings::unhinted(px, LocationRef::new(coords)), &mut NullPen)
        .ok()?
        .advance_width?;
    // Konsole uses slight hinting, which is the native hinter in light mode.
    let target = Target::Smooth {
        mode: SmoothMode::Light,
        symmetric_rendering: true,
        preserve_linear_metrics: false,
    };
    let hinter = HintingInstance::new(&outlines, px, LocationRef::new(coords), target).ok()?;
    let hinted = outline
        .draw(DrawSettings::hinted(&hinter, false), &mut NullPen)
        .ok()?
        .advance_width?;
    if hinted <= 0.0 || linear <= 0.0 {
        return None;
    }
    Some(CellSnap { linear, hinted })
}

#[cfg(test)]
mod tests {
    use super::CellSnap;

    #[test]
    fn a_fractional_cell_lands_on_the_hinted_pixel() {
        let snap = CellSnap {
            linear: 11.52,
            hinted: 11.0,
        };
        assert_eq!(snap.apply(11.52), 11.0);
        assert_eq!(snap.apply(46.08), 44.0);
        assert_eq!(snap.apply(0.0), 0.0);
    }

    #[test]
    fn a_partial_cluster_keeps_its_advance() {
        let snap = CellSnap {
            linear: 11.52,
            hinted: 11.0,
        };
        assert_eq!(snap.apply(3.0), 3.0);
    }

    #[test]
    fn nerd_font_mono_at_sixteen_points_uses_the_hinted_cell() {
        let path = "/home/deck/.local/share/fonts/3/3270NerdFontMono_Regular.ttf";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let mut font_system = crate::FontSystem::new();
        font_system.db_mut().load_font_file(path).unwrap();
        let px = 16.0 * 96.0 / 72.0;
        let mut buffer = crate::Buffer::new(&mut font_system, crate::Metrics::new(px, px * 1.4));
        let attrs = crate::Attrs::new().family(crate::Family::Name("3270 Nerd Font Mono"));
        buffer.set_text(
            &mut font_system,
            "MMMMMMMMMM",
            &attrs,
            crate::Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut font_system, false);
        let width = buffer.layout_runs().next().unwrap().line_w;
        assert!(
            (width - 110.0).abs() < 0.01,
            "expected 10 hinted cells of 11px, got {width}"
        );
    }
}
