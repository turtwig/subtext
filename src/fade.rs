//! Saves and restores the renderer opacity around a recorded subtree.

pub struct Reset(f32);

impl Reset {
    pub fn push(opacity: f32) -> Self {
        Self(iced_tiny_skia::set_draw_opacity(opacity))
    }
}

impl Drop for Reset {
    fn drop(&mut self) {
        iced_tiny_skia::set_draw_opacity(self.0);
    }
}
