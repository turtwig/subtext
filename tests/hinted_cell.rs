//! 3270 Nerd Font Mono at 16pt. Konsole's slight hinter uses an 11px cell.
//! The linear advance is 11.52px.

use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping};

#[test]
fn sixteen_points_snaps_to_the_hinted_cell() {
    let path = "/home/deck/.local/share/fonts/3/3270NerdFontMono_Regular.ttf";
    if !std::path::Path::new(path).exists() {
        return;
    }
    let mut font_system = FontSystem::new();
    font_system.db_mut().load_font_file(path).unwrap();
    let px = 16.0 * 96.0 / 72.0;
    let mut buffer = Buffer::new(&mut font_system, Metrics::new(px, px * 1.4));
    let attrs = Attrs::new().family(Family::Name("3270 Nerd Font Mono"));
    buffer.set_text(
        &mut font_system,
        "MMMMMMMMMM",
        &attrs,
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(&mut font_system, false);
    let width = buffer.layout_runs().next().expect("a line").line_w;
    assert!(
        (width - 110.0).abs() < 0.01,
        "expected 10 hinted cells of 11px, got {width}"
    );
}
