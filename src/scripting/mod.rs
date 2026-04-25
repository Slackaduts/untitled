pub mod api;
pub mod cutscene;
pub mod data;
pub mod event_bridge;
pub mod runner;
pub mod scene_action;
pub mod scene_event;

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
            .init_resource::<scene_action::SceneActionRegistry>()
            .init_resource::<runner::SceneRunner>()
            .add_message::<event_bridge::LuaCommand>()
            .add_systems(Startup, scene_action::register_builtin_actions)
            .add_systems(
                Update,
                (
                    runner::detect_interactions,
                    runner::start_pending_events
                        .after(runner::detect_interactions),
                    runner::tick_coroutines
                        .after(runner::start_pending_events),
                    runner::start_pending_yarn_nodes
                        .after(runner::tick_coroutines)
                        .run_if(resource_exists::<bevy_yarnspinner::prelude::YarnProject>),
                    runner::clear_dialogue_wait
                        .after(runner::tick_coroutines),
                    event_bridge::process_lua_commands
                        .after(runner::tick_coroutines),
                ),
            );
    }
}
