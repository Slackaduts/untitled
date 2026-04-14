pub mod api;
pub mod cutscene;
pub mod data;
pub mod event_bridge;

use bevy::prelude::*;
use mlua::Lua;

/// Resource wrapping the Lua VM.
#[derive(Resource)]
pub struct LuaVm {
    pub lua: Lua,
}

impl Default for LuaVm {
    fn default() -> Self {
        Self {
            lua: Lua::new(),
        }
    }
}

pub struct ScriptingPlugin;

impl Plugin for ScriptingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LuaVm>()
            .add_message::<event_bridge::LuaCommand>();
    }
}
