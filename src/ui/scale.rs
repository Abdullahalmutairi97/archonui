//! Interface scale. The UI is authored in design px (1 design px = 1 logical px at 100 %); the
//! user's "Interface size" setting scales every design size at once, like zooming a browser page.
//!
//! Layout code imports [`px`] from here instead of from gpui, so its constants scale. Code that
//! works with real window coordinates (mouse positions, painted bounds, the viewport) uses
//! `gpui::px` and converts with [`design`] / [`real`] where it meets design-unit values.

use gpui::Pixels;
use std::sync::atomic::{AtomicU32, Ordering};

/// The default interface scale.
pub const DEFAULT: f32 = 0.85;
/// The range the setting allows.
pub const RANGE: (f32, f32) = (0.6, 1.5);

static ZOOM: AtomicU32 = AtomicU32::new(0x3F59_999A); // 0.85_f32

/// The current interface scale.
pub fn zoom() -> f32 {
    f32::from_bits(ZOOM.load(Ordering::Relaxed))
}

/// Sets the interface scale, clamped to [`RANGE`]. Callers refresh the window afterwards.
pub fn set_zoom(z: f32) {
    ZOOM.store(clamp_zoom(z).to_bits(), Ordering::Relaxed);
}

/// `z` within [`RANGE`]; the default when it isn't a number.
fn clamp_zoom(z: f32) -> f32 {
    if z.is_finite() { z.clamp(RANGE.0, RANGE.1) } else { DEFAULT }
}

/// A design size as window pixels.
pub fn px(design: f32) -> Pixels {
    gpui::px(design * zoom())
}

/// Window pixels as design units.
pub fn design(real: Pixels) -> f32 {
    f32::from(real) / zoom()
}

/// Design units as window pixels, for values already in f32.
pub fn real(design: f32) -> f32 {
    design * zoom()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_bits_are_the_default() {
        assert_eq!(f32::from_bits(0x3F59_999A), DEFAULT);
    }

    #[test]
    fn zoom_is_clamped() {
        assert_eq!(clamp_zoom(9.), RANGE.1);
        assert_eq!(clamp_zoom(0.1), RANGE.0);
        assert_eq!(clamp_zoom(f32::NAN), DEFAULT);
        assert_eq!(clamp_zoom(0.9), 0.9);
    }
}
