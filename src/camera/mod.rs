pub mod combat;
pub mod cutscene;
pub mod follow;
pub mod visible_rect;

use bevy::audio::SpatialListener;
use bevy::prelude::*;
use bevy::light::ShadowFilteringMethod;
use crate::app_state::GameState;
use crate::sound::spatial::GameListener;

pub struct CameraPlugin;

/// Marker for the 3D main camera (always active).
#[derive(Component)]
pub struct CombatCamera3d;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<combat::CombatCamera>()
            .init_resource::<combat::BillboardTilesReady>()
            .init_resource::<combat::BillboardCameraState>()
            .init_resource::<visible_rect::CameraVisibleRect>()
            .add_systems(Startup, spawn_camera)
            .add_systems(
                Update,
                (
                    visible_rect::update_camera_visible_rect,
                    follow::camera_follow,
                    combat::setup_billboard_tiles,
                    combat::combat_camera_system,
                    combat::billboard_system,
                    combat::combat_grid_fade,
                    combat::spawn_object_lights
                        .after(combat::setup_billboard_tiles),
                    #[cfg(feature = "dev_tools")]
                    crate::billboard::object_editor::update_object_light_positions
                        .after(combat::billboard_system),
                )
                    .run_if(in_state(GameState::Overworld)),
            );

    }
}

fn spawn_camera(mut commands: Commands) {
    // Main camera: Camera3d with perspective.
    // During overworld it looks straight down at z=0 (XY plane) — looks like 2D.
    // During combat it tilts for 2.5D effect.
    let mut cam = commands.spawn((
        Camera3d {
            // No transmissive materials in the scene — skip the transmissive pass.
            screen_space_specular_transmission_steps: 0,
            ..default()
        },
        Camera {
            order: 0,
            ..default()
        },
        // Far plane must cover the full camera-to-scene distance (~900+ units)
        // plus visible area beyond the look-at point.
        Projection::Perspective(PerspectiveProjection {
            far: 3000.0,
            ..default()
        }),
        // Start at the same pitch used everywhere (overworld + combat)
        Transform::from_xyz(0.0, -900.0 * follow::OVERWORLD_TILT_OFFSET, 900.0)
            .looking_at(Vec3::ZERO, Vec3::Y),
        SpatialListener::new(400.0),
        GameListener,
        CombatCamera3d,
        // Gaussian PCF: 5x5 filter kernel for soft shadow edges
        ShadowFilteringMethod::Gaussian,
    ));

    // MSAA 2x: hardware-native on all platforms, near-zero cost.
    // 4x doubles bandwidth with minimal visual benefit.
    cam.insert(Msaa::Sample2);
}
