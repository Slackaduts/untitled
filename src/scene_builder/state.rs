//! Scene builder editor state.

use bevy::prelude::*;
use crate::scripting::scene_event::SceneEvent;
use crate::scripting::scene_action::ActionCategory;

#[derive(Resource)]
pub struct SceneBuilderState {
    pub open: bool,
    pub events: Vec<SceneEvent>,
    pub selected_event: Option<usize>,
    pub dirty: bool,
    pub loaded_for_map: Option<String>,
    /// Category filter for the "+ Add Action" dropdown.
    pub add_action_category: ActionCategory,
    /// Search filter for event list.
    pub event_search: String,
    /// Active position pick request: (action_index, arg_name, grid_mode).
    /// When set, the next world click fills the position arg.
    pub picking_position: Option<(usize, String, bool)>,
    /// Active drag: (action_index, arg_name). Set when dragging a position node.
    pub dragging_node: Option<(usize, String)>,
    /// Right-clicked a line segment: show settings popup for this action index.
    pub line_popup_action: Option<usize>,
    /// Spline init pick: (action_index, arg_name, grid_mode, start_point).
    /// First click sets start_point (None→Some), second click finalizes with end point.
    pub spline_init: Option<SplineInitState>,
}

/// State for the two-click spline initialization.
pub struct SplineInitState {
    pub action_idx: usize,
    pub arg_name: String,
    pub grid_mode: bool,
    /// Set after the first click.
    pub start: Option<[f32; 2]>,
}

impl Default for SceneBuilderState {
    fn default() -> Self {
        Self {
            open: false,
            events: Vec::new(),
            selected_event: None,
            dirty: false,
            loaded_for_map: None,
            add_action_category: ActionCategory::Movement,
            event_search: String::new(),
            picking_position: None,
            dragging_node: None,
            line_popup_action: None,
            spline_init: None,
        }
    }
}
