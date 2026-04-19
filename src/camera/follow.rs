use bevy::prelude::*;

use super::CombatCamera3d;

/// Marker for the entity the camera should follow.
#[derive(Component)]
pub struct CameraTarget;

/// Map pixel bounds, set when terrain is built.
#[derive(Resource, Default)]
pub struct MapBounds {
    /// Bottom-left corner in world pixels.
    pub min: Vec2,
    /// Top-right corner in world pixels.
    pub max: Vec2,
    pub valid: bool,
}

const DEFAULT_FOLLOW_SPEED: f32 = 5.0;
pub const DEFAULT_HEIGHT: f32 = 900.0;
/// Y offset as a fraction of camera height — positions the camera "south" of
/// the target so it looks slightly northward, giving a tilted 3/4 perspective.
pub const OVERWORLD_TILT_OFFSET: f32 = 0.9;

/// Moves the Camera3d to follow the target.
pub fn camera_follow(
    target_q: Query<&Transform, (With<CameraTarget>, Without<CombatCamera3d>)>,
    mut camera_q: Query<&mut Transform, With<CombatCamera3d>>,
    combat_camera: Option<Res<super::combat::CombatCamera>>,
    time: Res<Time>,
) {
    if combat_camera.is_some_and(|cc| cc.active) {
        return;
    }

    let Some(target_tf) = target_q.iter().next() else { return };
    let Some(mut cam_tf) = camera_q.iter_mut().next() else { return };

    let height = DEFAULT_HEIGHT;
    let dt = time.delta_secs();
    let t = (DEFAULT_FOLLOW_SPEED * dt).min(1.0);

    let target_x = target_tf.translation.x;
    let target_y = target_tf.translation.y;

    let target_pos = Vec3::new(
        target_x,
        target_y - height * OVERWORLD_TILT_OFFSET,
        height,
    );
    cam_tf.translation = cam_tf.translation.lerp(target_pos, t);

    // Tilt: pitch the camera to look at the target from slightly south
    let dy = target_y - cam_tf.translation.y;
    let dz = cam_tf.translation.z;
    let pitch = dy.atan2(dz);
    cam_tf.rotation = Quat::from_rotation_x(pitch);
}
