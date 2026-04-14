pub mod combat;
pub mod cutscene;
pub mod follow;

use bevy::audio::SpatialListener;
use bevy::anti_alias::fxaa::Fxaa;
use bevy::prelude::*;

use crate::app_state::GameState;
use crate::lighting::uniforms::LightingPostProcess;
use crate::sound::spatial::GameListener;

pub struct CameraPlugin;

/// Marker for the 3D main camera (always active).
#[derive(Component)]
pub struct CombatCamera3d;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<combat::BillboardMaterial>::default())
            .init_resource::<combat::CombatCamera>()
            .init_resource::<combat::BillboardTilesReady>()
            .add_systems(Startup, spawn_camera)
            .add_systems(
                Update,
                (
                    follow::camera_follow,
                    combat::setup_billboard_tiles,
                    combat::combat_camera_system,
                    combat::billboard_system,
                    combat::combat_grid_fade,
                )
                    .run_if(in_state(GameState::Overworld)),
            );
    }
}

fn spawn_camera(mut commands: Commands) {
    // Main camera: Camera3d with perspective.
    // During overworld it looks straight down at z=0 (XY plane) — looks like 2D.
    // During combat it tilts for 2.5D effect.
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: 0,
            ..default()
        },
        // Start at the same pitch used everywhere (overworld + combat)
        Transform::from_xyz(0.0, -900.0 * follow::OVERWORLD_TILT_OFFSET, 900.0)
            .looking_at(Vec3::ZERO, Vec3::Y),
        SpatialListener::new(400.0),
        GameListener,
        CombatCamera3d,
        LightingPostProcess,
        Fxaa::default(),
    ));
}
