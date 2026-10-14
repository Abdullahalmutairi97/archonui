//! Window state saved as `workspace.json`: the tile layout, the sidebar and tools panel, and the
//! current project. Projects are saved next to it, as `projects.json`.

use crate::ui::tiling::Layout;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Left sidebar width range when expanded, px.
pub const LEFT_WIDTH: (f32, f32) = (220., 520.);
/// Left sidebar width when none was saved, px.
pub const LEFT_WIDTH_DEFAULT: f32 = 300.;
/// Right tools panel width range, px.
pub const RIGHT_WIDTH: (f32, f32) = (360., 820.);
/// Right tools panel width when none was saved, px.
pub const RIGHT_WIDTH_DEFAULT: f32 = 480.;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Workspace {
    pub layout: Layout,
    /// Sidebar expanded rather than the collapsed rail.
    pub left_open: bool,
    /// Expanded sidebar width, px.
    pub left_width: f32,
    pub left_tab: LeftTab,
    /// Tools panel shown rather than its rail.
    pub right_open: bool,
    pub right_tab: RightTab,
    pub right_width: f32,
    /// Project shown in the Projects view and the Files and Git tools.
    pub current_project: Option<String>,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            layout: Layout::default(),
            left_open: true,
            left_width: LEFT_WIDTH_DEFAULT,
            left_tab: LeftTab::default(),
            right_open: true,
            right_tab: RightTab::default(),
            right_width: RIGHT_WIDTH_DEFAULT,
            current_project: None,
        }
    }
}

impl Workspace {
    /// Keeps the sidebar and panel widths in their ranges.
    pub fn normalize(&mut self) {
        self.left_width = self.left_width.clamp(LEFT_WIDTH.0, LEFT_WIDTH.1);
        self.right_width = self.right_width.clamp(RIGHT_WIDTH.0, RIGHT_WIDTH.1);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LeftTab {
    #[default]
    Sessions,
    Projects,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RightTab {
    Browser,
    #[default]
    Ide,
    Files,
    Git,
    Terminal,
}

/// A folder sessions can be grouped under.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    /// Seconds since the epoch.
    pub created: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_with_a_layout() {
        let mut w = Workspace::default();
        w.layout.tiles = vec!["a".into(), "b".into()];
        w.layout.focus = Some("b".into());
        w.layout.ratios.insert(0, 0.4);
        w.right_tab = RightTab::Terminal;
        w.left_tab = LeftTab::Projects;
        w.current_project = Some("p1".into());
        let json = serde_json::to_string(&w).unwrap();
        let back: Workspace = serde_json::from_str(&json).unwrap();
        assert_eq!(back, w);
    }

    #[test]
    fn partial_files_load_with_defaults() {
        let w: Workspace = serde_json::from_str(r#"{"right_open": false, "right_tab": "git"}"#).unwrap();
        assert!(!w.right_open && w.left_open);
        assert_eq!(w.right_tab, RightTab::Git);
        assert_eq!(w.right_width, RIGHT_WIDTH_DEFAULT);
        assert!(w.layout.tiles.is_empty());
    }

    #[test]
    fn left_width_defaults_and_is_clamped() {
        let old: Workspace = serde_json::from_str("{}").unwrap();
        assert_eq!(old.left_width, LEFT_WIDTH_DEFAULT);
        let mut w = Workspace { left_width: 9000., ..Default::default() };
        w.normalize();
        assert_eq!(w.left_width, LEFT_WIDTH.1);
        w.left_width = 10.;
        w.normalize();
        assert_eq!(w.left_width, LEFT_WIDTH.0);
    }

    #[test]
    fn right_width_is_clamped() {
        let mut w = Workspace { right_width: 2000., ..Default::default() };
        w.normalize();
        assert_eq!(w.right_width, RIGHT_WIDTH.1);
        w.right_width = 100.;
        w.normalize();
        assert_eq!(w.right_width, RIGHT_WIDTH.0);
    }
}
