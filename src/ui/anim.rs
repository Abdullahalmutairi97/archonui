//! Motion: CSS-style easing curves, the transition constants from layout.md §6, an `Animator` that
//! retargets from the value on screen, and the reduced-motion setting. Driven by
//! `request_animation_frame`, so an idle window does no work. See docs/design/layout.md §6 and
//! docs/UI_PORT_PLAN.md ("CSS transition on any property").

use std::{
    io::Read,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicU8, Ordering},
        OnceLock,
    },
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement, LayoutId,
    Pixels, Window,
};

use crate::ui::theme;

/// A CSS timing function over progress `0..=1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Easing {
    Linear,
    /// `cubic-bezier(x1, y1, x2, y2)`. The curve runs from (0, 0) to (1, 1) like CSS; `x1` and `x2` must be in `0..=1`.
    CubicBezier(f32, f32, f32, f32),
}

impl Easing {
    /// CSS `ease`.
    pub const EASE: Self = Self::CubicBezier(0.25, 0.1, 0.25, 1.0);
    /// CSS `ease-in-out`.
    pub const EASE_IN_OUT: Self = Self::CubicBezier(0.42, 0.0, 0.58, 1.0);

    /// The eased value at progress `x`. `at(0.0)` is exactly 0 and `at(1.0)` exactly 1.
    pub fn at(self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Self::Linear => x,
            Self::CubicBezier(_, _, _, _) if x == 0.0 => 0.0,
            Self::CubicBezier(_, _, _, _) if x == 1.0 => 1.0,
            Self::CubicBezier(x1, y1, x2, y2) => sample(y1, y2, solve_t(x1, x2, x)),
        }
    }
}

/// Coefficients `(a, b, c)` of one axis of a cubic Bézier with end points 0 and 1: `((a·t + b)·t + c)·t`.
fn coefficients(p1: f32, p2: f32) -> (f32, f32, f32) {
    let c = 3.0 * p1;
    let b = 3.0 * (p2 - p1) - c;
    (1.0 - c - b, b, c)
}

/// One axis of the curve at parameter `t`.
fn sample(p1: f32, p2: f32, t: f32) -> f32 {
    let (a, b, c) = coefficients(p1, p2);
    ((a * t + b) * t + c) * t
}

/// The derivative of [`sample`] with respect to `t`.
fn slope(p1: f32, p2: f32, t: f32) -> f32 {
    let (a, b, c) = coefficients(p1, p2);
    (3.0 * a * t + 2.0 * b) * t + c
}

/// The curve parameter where the x axis equals `x`. Newton's method first, as browsers do; bisection if it stalls.
fn solve_t(x1: f32, x2: f32, x: f32) -> f32 {
    const TOLERANCE: f32 = 1e-6;
    let mut t = x;
    for _ in 0..8 {
        let error = sample(x1, x2, t) - x;
        if error.abs() < TOLERANCE {
            return t;
        }
        let d = slope(x1, x2, t);
        if d.abs() < TOLERANCE {
            break;
        }
        t = (t - error / d).clamp(0.0, 1.0);
    }
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    t = x;
    for _ in 0..60 {
        let error = sample(x1, x2, t) - x;
        if error.abs() < TOLERANCE {
            break;
        }
        if error > 0.0 {
            hi = t;
        } else {
            lo = t;
        }
        t = (lo + hi) / 2.0;
    }
    t
}

/// One CSS transition: how long it runs and along which curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transition {
    pub secs: f32,
    pub easing: Easing,
}

impl Transition {
    pub const fn new(secs: f32, easing: Easing) -> Self {
        Self { secs, easing }
    }

    /// The run time, or zero when reduced motion is on.
    pub fn duration(self, reduced: bool) -> Duration {
        if reduced {
            Duration::ZERO
        } else {
            Duration::from_secs_f32(self.secs)
        }
    }
}

/// Wallpaper layer opacity when the background style changes (layout.md §6 #1).
pub const WALLPAPER_OPACITY: Transition = Transition::new(0.4, Easing::EASE);
/// Wallpaper layer `left` and `width` (layout.md §6 #1).
pub const WALLPAPER_EXTENT: Transition = Transition::new(0.3, Easing::EASE);
/// Left sidebar `width` and `background` on collapse or expand (layout.md §6 #2).
pub const SIDEBAR_COLLAPSE: Transition = Transition::new(0.3, Easing::EASE);
/// Right panel `width`, `left` and `background` on collapse or expand (layout.md §6 #3).
pub const PANEL_COLLAPSE: Transition = Transition::new(0.3, Easing::EASE);
/// Right panel `background` only, while its edge is being dragged (layout.md §6 #3b).
pub const PANEL_DRAG: Transition = Transition::new(0.3, Easing::EASE);
/// Session tile `left`, `top`, `width` and `height` on split, close and reflow (layout.md §6 #4).
pub const TILE_REFLOW: Transition =
    Transition::new(0.32, Easing::CubicBezier(0.2, 0.8, 0.2, 1.0));
/// Resize-handle grip `background` (layout.md §6 #5).
pub const GRIP: Transition = Transition::new(0.15, Easing::EASE);
/// Effort slider fill `width` (layout.md §6 #6).
pub const EFFORT_FILL: Transition = Transition::new(0.25, Easing::EASE);
/// Effort node `filter` and `color` (layout.md §6 #7).
pub const EFFORT_NODE: Transition = Transition::new(0.2, Easing::EASE);
/// Toggle track `background` and knob `left` (layout.md §6 #8).
pub const TOGGLE: Transition = Transition::new(0.2, Easing::EASE);
/// No transition: the value changes on the same frame (layout.md §6 #4b, #9).
pub const INSTANT: Transition = Transition::new(0.0, Easing::Linear);
/// Hover states are instant (layout.md §6 #9).
pub const HOVER: Transition = INSTANT;
/// Popovers mount without a transition (layout.md §6 #10).
pub const POPOVER: Transition = INSTANT;

/// Values an [`Animator`] can interpolate between.
pub trait Lerp: Clone + PartialEq {
    /// The value `t` of the way from `from` to `to`. `t` is already eased and may leave `0..=1` on overshooting curves.
    fn lerp(from: &Self, to: &Self, t: f32) -> Self;
}

impl Lerp for f32 {
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        from + (to - from) * t
    }
}

impl Lerp for (f32, f32) {
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        (f32::lerp(&from.0, &to.0, t), f32::lerp(&from.1, &to.1, t))
    }
}

/// Hue is blended linearly, not around the colour wheel.
impl Lerp for gpui::Hsla {
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        gpui::Hsla {
            h: f32::lerp(&from.h, &to.h, t),
            s: f32::lerp(&from.s, &to.s, t),
            l: f32::lerp(&from.l, &to.l, t),
            a: f32::lerp(&from.a, &to.a, t),
        }
    }
}

impl Lerp for gpui::Rgba {
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        gpui::Rgba {
            r: f32::lerp(&from.r, &to.r, t),
            g: f32::lerp(&from.g, &to.g, t),
            b: f32::lerp(&from.b, &to.b, t),
            a: f32::lerp(&from.a, &to.a, t),
        }
    }
}

/// Channels are rounded to whole values, as the prototype's colour mixing does.
impl Lerp for theme::Rgba {
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        let channel = |a: u8, b: u8| f32::lerp(&f32::from(a), &f32::from(b), t).round().clamp(0.0, 255.0) as u8;
        theme::Rgba {
            r: channel(from.r, to.r),
            g: channel(from.g, to.g),
            b: channel(from.b, to.b),
            a: f32::lerp(&from.a, &to.a, t),
        }
    }
}

/// Animates a value like a CSS transition: a retarget starts from the value on screen, so motion
/// never jumps.
#[derive(Debug, Clone)]
pub struct Animator<T: Lerp> {
    transition: Transition,
    from: T,
    to: T,
    start: Instant,
    duration: Duration,
}

impl<T: Lerp> Animator<T> {
    /// A settled animator at `value`.
    pub fn new(value: T, transition: Transition) -> Self {
        Self {
            transition,
            from: value.clone(),
            to: value,
            start: Instant::now(),
            duration: Duration::ZERO,
        }
    }

    /// Starts moving towards `target` at `now`. Setting the current target again changes nothing,
    /// so a view can call this on every render.
    pub fn set_target(&mut self, target: T, now: Instant) {
        self.set_target_at(target, now, reduced_motion());
    }

    fn set_target_at(&mut self, target: T, now: Instant, reduced: bool) {
        if target == self.to {
            return;
        }
        self.from = self.value(now);
        self.to = target;
        self.start = now;
        self.duration = self.transition.duration(reduced);
    }

    /// The value at `now`.
    pub fn value(&self, now: Instant) -> T {
        let elapsed = now.saturating_duration_since(self.start);
        if elapsed >= self.duration {
            return self.to.clone();
        }
        let progress = elapsed.as_secs_f32() / self.duration.as_secs_f32();
        T::lerp(&self.from, &self.to, self.transition.easing.at(progress))
    }

    /// Whether the value is still moving at `now`. Use it with [`tick`].
    pub fn is_running(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.start) < self.duration
    }

    /// The value this animator is moving towards.
    pub fn target(&self) -> &T {
        &self.to
    }
}

const FOLLOW_OS: u8 = 0;
const FORCE_ANIMATE: u8 = 1;
const FORCE_REDUCED: u8 = 2;

/// The settings page's choice, read by [`reduced_motion`].
static MOTION_OVERRIDE: AtomicU8 = AtomicU8::new(FOLLOW_OS);

/// Sets the reduced-motion choice from settings: `Some(true)` turns motion off, `Some(false)` keeps
/// it on, and `None` follows the environment and the OS again.
pub fn set_reduced_motion(setting: Option<bool>) {
    let value = match setting {
        None => FOLLOW_OS,
        Some(false) => FORCE_ANIMATE,
        Some(true) => FORCE_REDUCED,
    };
    MOTION_OVERRIDE.store(value, Ordering::Relaxed);
}

/// Whether transitions should be instant. Settings override the environment, which overrides the OS.
pub fn reduced_motion() -> bool {
    match MOTION_OVERRIDE.load(Ordering::Relaxed) {
        FORCE_ANIMATE => false,
        FORCE_REDUCED => true,
        _ => system_reduced_motion(),
    }
}

/// `ARCHONUI_REDUCED_MOTION=1`, or the OS setting. Read once; the OS probe runs at most once.
fn system_reduced_motion() -> bool {
    static SYSTEM: OnceLock<bool> = OnceLock::new();
    *SYSTEM.get_or_init(|| {
        std::env::var("ARCHONUI_REDUCED_MOTION").is_ok_and(|v| v == "1") || gnome_reduced_motion()
    })
}

/// GNOME's `enable-animations`, read with a 250 ms limit so the UI never waits on it for long.
/// Other platforms have no probe yet: they use the environment variable and the settings page.
#[cfg(target_os = "linux")]
fn gnome_reduced_motion() -> bool {
    let Ok(mut child) = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "enable-animations"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let deadline = Instant::now() + Duration::from_millis(250);
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            let mut out = String::new();
            if let (true, Some(mut stdout)) = (status.success(), child.stdout.take()) {
                let _ = stdout.read_to_string(&mut out);
            }
            return out.trim() == "false";
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(not(target_os = "linux"))]
fn gnome_reduced_motion() -> bool {
    false
}

/// Asks for the next frame while something is animating. Call it at the end of a view's render
/// with whether any animator is running, so an idle window draws nothing.
pub fn tick(window: &Window, any_running: bool) {
    if any_running {
        window.request_animation_frame();
    }
}

/// An element that follows a [`Transition`], keeping its [`Animator`] in GPUI element state. A
/// `RenderOnce` widget can use it for a property that changes with its own state without owning an
/// animator. `build` runs every frame with the value on screen; `id` must be stable across frames.
/// Reduced motion makes the change instant, like every other transition.
pub fn animated<T, F>(id: impl Into<ElementId>, transition: Transition, target: T, build: F) -> AnyElement
where
    T: Lerp + 'static,
    F: Fn(T) -> AnyElement + 'static,
{
    Animated { id: id.into(), transition, target, build }.into_any_element()
}

struct Animated<T, F> {
    id: ElementId,
    transition: Transition,
    target: T,
    build: F,
}

impl<T: Lerp + 'static, F: Fn(T) -> AnyElement + 'static> IntoElement for Animated<T, F> {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<T: Lerp + 'static, F: Fn(T) -> AnyElement + 'static> Element for Animated<T, F> {
    type RequestLayoutState = (LayoutId, AnyElement);
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let (target, transition, build) = (self.target.clone(), self.transition, &self.build);
        let built = window.with_element_state::<Animator<T>, _>(global_id.unwrap(), |state, window| {
            let now = Instant::now();
            let mut animator = state.unwrap_or_else(|| Animator::new(target.clone(), transition));
            animator.set_target(target, now);
            let mut element = build(animator.value(now)).into_any_element();
            if animator.is_running(now) {
                window.request_animation_frame();
            }
            ((element.request_layout(window, cx), element), animator)
        });
        (built.0, built)
    }

    fn prepaint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        element: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        element.1.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        element: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        element.1.paint(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn ease_matches_css_at_midpoint() {
        // CSS `ease` at progress 0.5 is 0.8024 in every browser.
        assert!(close(Easing::EASE.at(0.5), 0.8024, 5e-4), "{}", Easing::EASE.at(0.5));
    }

    #[test]
    fn ease_in_out_is_symmetric() {
        assert!(close(Easing::EASE_IN_OUT.at(0.5), 0.5, 1e-4));
        assert!(close(Easing::EASE_IN_OUT.at(0.25) + Easing::EASE_IN_OUT.at(0.75), 1.0, 1e-3));
    }

    #[test]
    fn linear_bezier_is_the_identity() {
        let curve = Easing::CubicBezier(0.0, 0.0, 1.0, 1.0);
        for x in [0.1_f32, 0.3, 0.5, 0.9] {
            assert!(close(curve.at(x), x, 1e-4), "x={x} got {}", curve.at(x));
        }
    }

    #[test]
    fn endpoints_are_exact_for_every_curve() {
        for easing in [Easing::Linear, Easing::EASE, Easing::EASE_IN_OUT, TILE_REFLOW.easing] {
            assert_eq!(easing.at(0.0), 0.0);
            assert_eq!(easing.at(1.0), 1.0);
        }
    }

    #[test]
    fn tile_reflow_curve_is_monotonic() {
        let mut last = 0.0;
        for step in 1..=100 {
            let y = TILE_REFLOW.easing.at(step as f32 / 100.0);
            assert!(y >= last, "dips at step {step}");
            last = y;
        }
    }

    #[test]
    fn animator_moves_then_settles() {
        let t0 = Instant::now();
        let mut anim = Animator::new(0.0_f32, TILE_REFLOW);
        anim.set_target_at(1.0, t0, false);
        let halfway = anim.value(t0 + Duration::from_millis(160));
        assert!(halfway > 0.5 && halfway < 1.0, "halfway {halfway}");
        assert!(anim.is_running(t0 + Duration::from_millis(100)));
        assert_eq!(anim.value(t0 + Duration::from_millis(320)), 1.0);
        assert!(!anim.is_running(t0 + Duration::from_millis(320)));
    }

    #[test]
    fn retarget_starts_from_the_value_on_screen() {
        let t0 = Instant::now();
        let mut anim = Animator::new(0.0_f32, SIDEBAR_COLLAPSE);
        anim.set_target_at(1.0, t0, false);
        let mid = t0 + Duration::from_millis(150);
        let on_screen = anim.value(mid);
        anim.set_target_at(0.0, mid, false);
        assert_eq!(anim.value(mid), on_screen);
        assert_eq!(anim.value(mid + Duration::from_millis(300)), 0.0);
    }

    #[test]
    fn same_target_does_not_restart() {
        let t0 = Instant::now();
        let mut anim = Animator::new(0.0_f32, TILE_REFLOW);
        anim.set_target_at(1.0, t0, false);
        anim.set_target_at(1.0, t0 + Duration::from_millis(100), false);
        assert_eq!(anim.value(t0 + Duration::from_millis(320)), 1.0);
    }

    #[test]
    fn reduced_motion_is_instant() {
        let t0 = Instant::now();
        let mut anim = Animator::new(0.0_f32, TILE_REFLOW);
        anim.set_target_at(1.0, t0, true);
        assert_eq!(anim.value(t0), 1.0);
        assert!(!anim.is_running(t0));
        assert_eq!(TILE_REFLOW.duration(true), Duration::ZERO);
    }

    #[test]
    fn override_beats_environment_and_os() {
        set_reduced_motion(Some(true));
        assert!(reduced_motion());
        set_reduced_motion(Some(false));
        assert!(!reduced_motion());
        set_reduced_motion(None);
    }

    #[test]
    fn theme_colours_lerp_per_channel_with_rounding() {
        let black = theme::Rgba::hex(0x000000);
        let white = theme::Rgba::hex(0xffffff);
        assert_eq!(theme::Rgba::lerp(&black, &white, 0.5), theme::Rgba::hex(0x808080));
        let clear = theme::Rgba::new(0, 0, 0, 0.0);
        assert!(close(theme::Rgba::lerp(&clear, &white.alpha(1.0), 0.25).a, 0.25, 1e-6));
    }
}
