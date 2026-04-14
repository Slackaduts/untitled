pub mod app_state;
pub mod billboard;
pub mod camera;
pub mod dev_scene;
pub mod combat;
pub mod config;
pub mod entity;
pub mod input;
pub mod lighting;
pub mod map;
pub mod particles;
pub mod save;
pub mod scripting;
pub mod sound;
pub mod sprite;
pub mod ui;

use bevy::prelude::*;

use app_state::GameState;
use camera::CameraPlugin;
use combat::CombatPlugin;
use config::ConfigPlugin;
use entity::EntityPlugin;
use input::InputPlugin;
use lighting::LightingPlugin;
use map::MapPlugin;
use particles::ParticlePlugin;
use save::SavePlugin;
use scripting::ScriptingPlugin;
use sound::SoundPlugin;
use sprite::SpritePlugin;
use ui::UiPlugin;

pub struct UntitledPlugin;

impl Plugin for UntitledPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<GameState>()
            .add_plugins(ConfigPlugin)
            .add_plugins(InputPlugin)
            .add_plugins(CameraPlugin)
            .add_plugins(MapPlugin)
            .add_plugins(SpritePlugin)
            .add_plugins(LightingPlugin)
            .add_plugins(ParticlePlugin)
            .add_plugins(EntityPlugin)
            .add_plugins(CombatPlugin)
            .add_plugins(SoundPlugin)
            .add_plugins(ScriptingPlugin)
            .add_plugins(UiPlugin)
            .add_plugins(SavePlugin)
            .add_plugins(billboard::BillboardPropertiesPlugin)
            .add_plugins(crate::dev_scene::DevScenePlugin);
    }
}
