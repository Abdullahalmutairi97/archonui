//! Shared controls styled from the theme tokens (docs/design/settings.md §1.6, chat.md §4).

pub mod agents;
pub mod badge;
pub mod button;
pub mod demo;
pub mod kbd;
pub mod popover;
pub mod section;
pub mod segmented;
pub mod slider;
pub mod text_input;
pub mod toast;
pub mod toggle;

// Nothing in the app uses these yet and `demo` isn't wired into `main.rs`, so the re-exports are unused.
#[allow(unused_imports)]
pub use badge::{AttachmentCard, Badge, CardVariant, Tone};
#[allow(unused_imports)]
pub use button::{Button, ButtonStyle};
#[allow(unused_imports)]
pub use kbd::Kbd;
#[allow(unused_imports)]
pub use popover::{Placement, Popover};
#[allow(unused_imports)]
pub use section::{Section, SettingRow};
#[allow(unused_imports)]
pub use segmented::Segmented;
#[allow(unused_imports)]
pub use slider::{slider, Slider, SliderRange};
#[allow(unused_imports)]
pub use text_input::{text_input, TextInput, TextInputConfig, TextInputEvent};
#[allow(unused_imports)]
pub use toast::Toasts;
#[allow(unused_imports)]
pub use toggle::Toggle;
