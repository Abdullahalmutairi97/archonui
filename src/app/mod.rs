//! App state store: sessions, projects, layout, settings and live turns. UI views read it and send it
//! intents; it talks to harness processes. See docs/UI_PORT_PLAN.md.
#![allow(dead_code)]

pub mod notify;
pub mod persist;
pub mod sections;
pub mod settings;
pub mod state;
pub mod update;
pub mod workspace;
