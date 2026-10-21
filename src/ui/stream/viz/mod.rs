//! Charts and diagrams in chat: ```` ```viz ```` blocks drawn natively from their JSON, and
//! ```` ```mermaid ```` blocks drawn by a pinned, bundled mermaid.js. See `spec` for the data,
//! `scene` for the geometry, `card` for the card and its actions, `mermaid` for the renderer.

pub mod card;
pub mod mermaid;
pub mod scene;
pub mod spec;
