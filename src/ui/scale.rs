//! Interface scale. The UI is authored in design px (1 design px = 1 logical px at 100 %); the
//! user's "Interface size" setting scales every design size at once, like zooming a browser page.
//!
//! Layout code imports [`px`] from here instead of from gpui, so its constants scale. Code that
//! works with real window coordinates (mouse positions, painted bounds, the viewport) uses
//! `gpui::px` and converts with [`design`] / [`real`] where it meets design-unit values.

use gpui::{AnyElement, App, Bounds, Element, GlobalElementId, InspectorElementId, IntoElement, LayoutId, Pixels, Window};
use std::{
    cell::Cell,
    sync::atomic::{AtomicU32, Ordering},
};

/// The default interface scale.
pub const DEFAULT: f32 = 0.85;
/// The range the setting allows.
pub const RANGE: (f32, f32) = (0.6, 1.5);

static ZOOM: AtomicU32 = AtomicU32::new(0x3F59_999A); // 0.85_f32

thread_local! {
    /// A zoom that replaces the interface scale while one part of the window is built and drawn.
    static SCOPED: Cell<Option<f32>> = const { Cell::new(None) };
}

/// The current interface scale, or the zoom of the part being drawn.
pub fn zoom() -> f32 {
    SCOPED.with(Cell::get).unwrap_or_else(|| f32::from_bits(ZOOM.load(Ordering::Relaxed)))
}

/// The interface scale itself, whatever part is being drawn.
pub fn base_zoom() -> f32 {
    f32::from_bits(ZOOM.load(Ordering::Relaxed))
}

/// Runs `f` with design sizes drawn at `z` instead of the interface scale.
pub fn scoped<R>(z: f32, f: impl FnOnce() -> R) -> R {
    let before = SCOPED.with(|s| s.replace(Some(z)));
    let out = f();
    SCOPED.with(|s| s.set(before));
    out
}

/// An element drawn at its own zoom, like CSS `zoom`. Components and views inside it build their
/// elements during layout, so the zoom is held through every phase, not only while it is built.
pub struct Zoomed {
    zoom: f32,
    child: AnyElement,
}

/// `child`, laid out and painted at `zoom`. Build `child` inside [`scoped`] with the same zoom.
pub fn zoomed(zoom: f32, child: impl IntoElement) -> Zoomed {
    Zoomed { zoom, child: scoped(zoom, || child.into_any_element()) }
}

impl IntoElement for Zoomed {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for Zoomed {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let child = &mut self.child;
        (scoped(self.zoom, || child.request_layout(window, cx)), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let child = &mut self.child;
        scoped(self.zoom, || child.prepaint(window, cx));
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        _prepaint: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let child = &mut self.child;
        scoped(self.zoom, || child.paint(window, cx));
    }
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
    fn a_scoped_zoom_applies_only_inside_its_scope() {
        let base = zoom();
        assert_eq!(scoped(0.5, || (zoom(), scoped(0.25, zoom), zoom())), (0.5, 0.25, 0.5));
        assert_eq!(zoom(), base);
    }

    #[test]
    fn zoom_is_clamped() {
        assert_eq!(clamp_zoom(9.), RANGE.1);
        assert_eq!(clamp_zoom(0.1), RANGE.0);
        assert_eq!(clamp_zoom(f32::NAN), DEFAULT);
        assert_eq!(clamp_zoom(0.9), 0.9);
    }
}
