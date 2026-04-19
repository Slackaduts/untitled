//! Benchmark that loads game plugins one at a time to find the FPS killer.
//! Toggle the flags below and rerun to binary-search the bottleneck.

use bevy::prelude::*;
use bevy::window::PresentMode;
use bevy::diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin};

// ── Toggle these to find the bottleneck ─────────────────────────────────────
const ENABLE_MAP: bool = true;
const ENABLE_CAMERA: bool = true;
const ENABLE_LIGHTING: bool = true;
const ENABLE_PARTICLES: bool = true;
const ENABLE_BILLBOARD: bool = true;
const ENABLE_UI: bool = true;
const ENABLE_SCENE: bool = true;
// ────────────────────────────────────────────────────────────────────────────

fn main() {
    let mut app = App::new();

    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Plugin Bench".into(),
            present_mode: PresentMode::Immediate,
            ..default()
        }),
        ..default()
    }));

    app.add_plugins(FrameTimeDiagnosticsPlugin::default());
    app.add_plugins(LogDiagnosticsPlugin::default());

    // Always needed
    app.init_state::<untitled::app_state::GameState>();
    app.add_plugins(untitled::config::ConfigPlugin);
    app.add_plugins(untitled::input::InputPlugin);
    app.add_plugins(untitled::save::SavePlugin);
    app.add_plugins(untitled::scripting::ScriptingPlugin);
    app.add_plugins(untitled::sound::SoundPlugin);
    app.add_plugins(untitled::combat::CombatPlugin);
    app.add_plugins(untitled::entity::EntityPlugin);
    app.add_plugins(untitled::sprite::SpritePlugin);

    if ENABLE_CAMERA {
        app.add_plugins(untitled::camera::CameraPlugin);
    }
    if ENABLE_MAP {
        app.add_plugins(untitled::map::MapPlugin);
    }
    if ENABLE_LIGHTING {
        app.add_plugins(untitled::lighting::LightingPlugin);
    }
    if ENABLE_PARTICLES {
        app.add_plugins(untitled::particles::ParticlePlugin);
    }
    if ENABLE_BILLBOARD {
        app.add_plugins(untitled::billboard::BillboardPropertiesPlugin);
    }
    if ENABLE_UI {
        app.add_plugins(untitled::ui::UiPlugin);
    }
    if ENABLE_SCENE {
        app.add_plugins(untitled::dev_scene::DevScenePlugin);
    }

    app.run();
}
