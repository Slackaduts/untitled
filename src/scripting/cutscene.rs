use bevy::prelude::*;

/// A running cutscene backed by a Lua coroutine.
/// The cutscene system resumes one step per frame.
#[derive(Resource)]
pub struct ActiveCutscene {
    pub script_id: String,
    pub waiting: bool,
}
