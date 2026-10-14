//! Range slider from settings §1.6: a 240px track, a readout, and `on_change` on each value change.
//! Drag and track click both set the value. The pure math is in [`SliderRange`] and [`format_value`].
//! Spec: settings §1.6 (`accent-color` acc, a light track, readout `alt`).

use std::rc::Rc;

use gpui::{
    canvas, div, prelude::*, relative, App, Bounds, Context, DispatchPhase, Entity, Hsla,
    IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, RenderOnce,
    SharedString, Window,
};
use crate::ui::scale::px;

use crate::ui::theme::Theme;

/// Track width in design px. Matches settings §1.6.
const TRACK_WIDTH: f32 = 240.;

/// Minimum, maximum and step of a slider.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliderRange {
    pub min: f32,
    pub max: f32,
    /// Values snap to `min + n * step`. A step of zero or less leaves values unsnapped.
    pub step: f32,
}

impl SliderRange {
    /// Clamps `value` to the range, then snaps it to the nearest step.
    pub fn snap(&self, value: f32) -> f32 {
        // `f32::clamp` panics when min > max, and passes NaN through; a bad range or value falls back to `min`.
        let (min, max) = if self.max >= self.min { (self.min, self.max) } else { (self.min, self.min) };
        let clamped = if value.is_nan() { min } else { value.clamp(min, max) };
        if !(self.step > 0.) || !self.step.is_finite() {
            return clamped;
        }
        let steps = ((clamped - min) / self.step).round();
        (min + steps * self.step).min(max)
    }

    /// Position of `value` along the track, from 0 at `min` to 1 at `max`.
    pub fn fraction(&self, value: f32) -> f32 {
        if self.max <= self.min {
            return 0.;
        }
        ((value - self.min) / (self.max - self.min)).clamp(0., 1.)
    }

    /// The snapped value at track position `fraction` (0 to 1).
    pub fn value_at(&self, fraction: f32) -> f32 {
        self.snap(self.min + fraction.clamp(0., 1.) * (self.max - self.min))
    }
}

/// The track position under `x`, given the track's left edge and width. Clamped to 0 to 1.
pub fn fraction_at(x: f32, left: f32, width: f32) -> f32 {
    if !(width > 0.) || x.is_nan() || left.is_nan() {
        return 0.;
    }
    ((x - left) / width).clamp(0., 1.)
}

/// The readout text: `value` with as many decimals as `step` has, then `unit`.
pub fn format_value(value: f32, step: f32, unit: &str) -> String {
    let decimals = step_decimals(step);
    format!("{value:.decimals$}{unit}")
}

fn step_decimals(step: f32) -> usize {
    let text = format!("{}", step.abs());
    text.split_once('.').map_or(0, |(_, fraction)| fraction.len())
}

/// The slider's state. Create it with `cx.new(|_| Slider::new(...))` and show it with [`slider`].
pub struct Slider {
    value: f32,
    range: SliderRange,
    unit: SharedString,
    /// The track's last painted bounds, for mapping mouse positions to values.
    track: Option<Bounds<Pixels>>,
    dragging: bool,
    /// When set, a drag only previews the value; `on_change` runs once when the mouse is released.
    commit_on_release: bool,
    /// A previewed value that `on_change` has not seen yet.
    pending: bool,
    on_change: Rc<dyn Fn(f32, &mut Window, &mut App)>,
}

impl Slider {
    /// A slider at `value`. `on_change` gets the new value when it changes by a drag or a track click.
    pub fn new(
        value: f32,
        range: SliderRange,
        unit: impl Into<SharedString>,
        on_change: impl Fn(f32, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            value: range.snap(value),
            range,
            unit: unit.into(),
            track: None,
            dragging: false,
            commit_on_release: false,
            pending: false,
            on_change: Rc::new(on_change),
        }
    }

    /// For settings that are expensive to apply live: a drag previews the value, and `on_change` runs
    /// once on mouse release (a track click commits the same way).
    pub fn commit_on_release(mut self) -> Self {
        self.commit_on_release = true;
        self
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    /// True while a previewed value has not been committed through `on_change`.
    pub fn is_pending(&self) -> bool {
        self.pending
    }

    /// Sets the value without calling `on_change`.
    pub fn set_value(&mut self, value: f32, cx: &mut Context<Self>) {
        self.value = self.range.snap(value);
        self.pending = false;
        cx.notify();
    }
}

/// Sets the slider to the value at track position `fraction`. Calls `on_change` if that changed it,
/// unless the slider commits on release, in which case only the preview moves.
/// The handler is cloned out before it runs, so it can update this entity without a borrow conflict.
fn set_fraction(entity: &Entity<Slider>, fraction: f32, window: &mut Window, cx: &mut App) {
    let change = entity.update(cx, |slider, cx| {
        let value = slider.range.value_at(fraction);
        if value == slider.value {
            return None;
        }
        slider.value = value;
        if slider.commit_on_release {
            slider.pending = true;
            cx.notify();
            return None;
        }
        Some((value, slider.on_change.clone()))
    });
    if let Some((value, on_change)) = change {
        on_change(value, window, cx);
    }
}

/// Calls `on_change` with the previewed value, if a preview is waiting to be committed.
fn commit(entity: &Entity<Slider>, window: &mut Window, cx: &mut App) {
    let change = entity.update(cx, |slider, _| {
        if !slider.pending {
            return None;
        }
        slider.pending = false;
        Some((slider.value, slider.on_change.clone()))
    });
    if let Some((value, on_change)) = change {
        on_change(value, window, cx);
    }
}

/// Shows `slider` in the theme's colours.
pub fn slider(entity: &Entity<Slider>, theme: &Theme) -> SliderView {
    SliderView {
        entity: entity.clone(),
        colors: SliderColors {
            acc: theme.acc.to_hsla(),
            track: theme.fg.to_hsla(),
            alt: theme.alt.to_hsla(),
        },
    }
}

#[derive(Clone, Copy)]
struct SliderColors {
    acc: Hsla,
    /// The unfilled track: light, as a native range input draws it (settings §1.6 rendered values).
    track: Hsla,
    alt: Hsla,
}

#[derive(IntoElement)]
pub struct SliderView {
    entity: Entity<Slider>,
    colors: SliderColors,
}

impl RenderOnce for SliderView {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let c = self.colors;
        let state = self.entity.read(cx);
        let fraction = state.range.fraction(state.value);
        let readout = format_value(state.value, state.range.step, &state.unit);

        let press = self.entity.clone();
        let track = div()
            .relative()
            .w(px(TRACK_WIDTH))
            .h(px(18.))
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, window, cx| {
                press.update(cx, |slider, _| slider.dragging = true);
                if let Some(bounds) = press.read(cx).track {
                    let x = f32::from(event.position.x);
                    let fraction = fraction_at(x, f32::from(bounds.left()), f32::from(bounds.size.width));
                    set_fraction(&press, fraction, window, cx);
                }
            })
            .child(canvas(
                {
                    let entity = self.entity.clone();
                    move |bounds, _, cx| entity.update(cx, |slider, _| slider.track = Some(bounds))
                },
                {
                    let entity = self.entity.clone();
                    move |bounds, _, window, _| {
                        // Drag can leave the track, so the move and release listeners are window-wide.
                        let drag = entity.clone();
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                            if phase != DispatchPhase::Bubble || !drag.read(cx).dragging {
                                return;
                            }
                            let fraction = fraction_at(
                                f32::from(event.position.x),
                                f32::from(bounds.left()),
                                f32::from(bounds.size.width),
                            );
                            set_fraction(&drag, fraction, window, cx);
                        });
                        let release = entity.clone();
                        window.on_mouse_event(move |_: &MouseUpEvent, phase, window, cx| {
                            if phase != DispatchPhase::Bubble {
                                return;
                            }
                            let was_dragging = release.update(cx, |slider, _| std::mem::replace(&mut slider.dragging, false));
                            if was_dragging {
                                commit(&release, window, cx);
                            }
                        });
                    }
                },
            )
            .absolute()
            .size_full())
            .child(
                div()
                    .absolute()
                    .top(px(7.))
                    .left_0()
                    .w_full()
                    .h(px(4.))
                    .rounded(px(2.))
                    .bg(c.track),
            )
            .child(
                div()
                    .absolute()
                    .top(px(7.))
                    .left_0()
                    .w(relative(fraction))
                    .h(px(4.))
                    .rounded(px(2.))
                    .bg(c.acc),
            )
            .child(
                div()
                    .absolute()
                    .top(px(2.))
                    .left(relative(fraction))
                    .ml(px(-7.))
                    .size(px(14.))
                    .rounded(px(7.))
                    .bg(c.acc),
            );

        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .flex_none()
            .child(track)
            .child(
                div()
                    .w(px(48.))
                    .flex()
                    .justify_end()
                    .text_size(px(12.))
                    .text_color(c.alt)
                    .child(readout),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAX_SETTING: SliderRange = SliderRange { min: 10., max: 100., step: 5. };

    #[test]
    fn snap_rounds_to_the_nearest_step_from_min() {
        assert_eq!(MAX_SETTING.snap(42.), 40.);
        assert_eq!(MAX_SETTING.snap(43.), 45.);
        assert_eq!(MAX_SETTING.snap(12.4), 10.);
    }

    #[test]
    fn snap_clamps_to_the_range() {
        assert_eq!(MAX_SETTING.snap(3.), 10.);
        assert_eq!(MAX_SETTING.snap(500.), 100.);
    }

    #[test]
    fn snap_never_exceeds_max_when_it_is_not_a_step_multiple() {
        let range = SliderRange { min: 0., max: 10., step: 4. };
        assert_eq!(range.snap(9.), 8.);
        assert_eq!(range.snap(10.), 10.);
    }

    #[test]
    fn degenerate_ranges_and_nan_do_not_panic() {
        let flat = SliderRange { min: 5., max: 5., step: 1. };
        assert_eq!(flat.snap(9.), 5.);
        assert_eq!(flat.fraction(5.), 0.);
        assert_eq!(flat.value_at(0.7), 5.);
        let inverted = SliderRange { min: 5., max: 1., step: 1. };
        assert_eq!(inverted.snap(3.), 5.);
        assert_eq!(MAX_SETTING.snap(f32::NAN), 10.);
        let nan_step = SliderRange { min: 0., max: 1., step: f32::NAN };
        assert_eq!(nan_step.snap(0.4), 0.4);
        assert_eq!(fraction_at(f32::NAN, 0., 10.), 0.);
        assert_eq!(fraction_at(5., 0., f32::NAN), 0.);
    }

    #[test]
    fn zero_step_keeps_the_value() {
        let range = SliderRange { min: 0., max: 1., step: 0. };
        assert_eq!(range.snap(0.37), 0.37);
    }

    #[test]
    fn fraction_maps_min_and_max_to_the_ends() {
        assert_eq!(MAX_SETTING.fraction(10.), 0.);
        assert_eq!(MAX_SETTING.fraction(100.), 1.);
        assert!((MAX_SETTING.fraction(55.) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn value_at_inverts_fraction_and_snaps() {
        assert_eq!(MAX_SETTING.value_at(0.5), 55.);
        assert_eq!(MAX_SETTING.value_at(-1.), 10.);
        assert_eq!(MAX_SETTING.value_at(2.), 100.);
    }

    #[test]
    fn track_click_position_maps_to_a_fraction() {
        assert_eq!(fraction_at(150., 100., 200.), 0.25);
        assert_eq!(fraction_at(50., 100., 200.), 0.);
        assert_eq!(fraction_at(400., 100., 200.), 1.);
        assert_eq!(fraction_at(150., 100., 0.), 0.);
    }

    #[test]
    fn readout_uses_the_step_precision_and_unit() {
        assert_eq!(format_value(40., 5., ""), "40");
        assert_eq!(format_value(0.5, 0.1, "s"), "0.5s");
        assert_eq!(format_value(1.25, 0.25, "x"), "1.25x");
    }
}
