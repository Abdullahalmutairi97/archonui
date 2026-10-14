//! Agent engine: finds agent CLIs installed on this machine and talks to them.
//!
//! Each agent is one `Harness` in `harness/`: how to run a turn with it, what
//! it offers, and whether it is signed in. `detect` scans the machine for
//! those binaries; `start_turn` runs one prompt and streams normalized events back.

pub mod approve;
pub mod attach;
mod detect;
mod discover;
pub mod mcp;
pub mod plugins;
pub mod skills;
pub mod title;
mod harness;
mod parse;
mod session;

pub use detect::{detect, DetectedAgent};
pub use discover::{discover, Capabilities, ModelInfo};
pub use harness::{harness, Harness, Mode, ToolSettings};
pub use parse::Out;
pub use session::{start_turn, Event, Turn, TurnOptions};

/// Display name for an agent id, whether or not it is installed.
pub fn agent_name(id: &str) -> &str {
    harness(id).map_or(id, |h| h.name())
}
