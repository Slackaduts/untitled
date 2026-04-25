use bevy::prelude::*;

use super::CombatCamera3d;
use super::follow::{DEFAULT_HEIGHT, OVERWORLD_TILT_OFFSET};

/// Active camera pan: smoothly moves the camera to a world position.
#[derive(Resource)]
pub struct CameraPanState {
    pub target: Vec2,
    pub duration: f32,
    pub elapsed: f32,
    /// Captured on first tick.
    pub start: Option<Vec2>,
}

/// Active camera shake: offsets the camera randomly.
#[derive(Resource)]
pub struct CameraShakeState {
    pub intensity: f32,
    pub duration: f32,
    pub elapsed: f32,
}

/// System: smoothly pan the camera to a target world position.
/// Temporarily overrides camera_follow. Removes itself when done.
pub fn tick_camera_pan(
    time: Res<Time>,
    mut pan: Option<ResMut<CameraPanState>>,
    mut camera_q: Query<&mut Transform, With<CombatCamera3d>>,
    mut commands: Commands,
) {
    let Some(ref mut pan) = pan else { return };
    let Ok(mut cam_tf) = camera_q.single_mut() else { return };

    let start = *pan.start.get_or_insert_with(|| {
        Vec2::new(cam_tf.translation.x, cam_tf.translation.y + DEFAULT_HEIGHT * OVERWORLD_TILT_OFFSET)
    });

    pan.elapsed += time.delta_secs();
    let t = if pan.duration > 0.0 { (pan.elapsed / pan.duration).min(1.0) } else { 1.0 };
    // Smooth interpolation
    let t_smooth = t * t * (3.0 - 2.0 * t);

    let pos = start.lerp(pan.target, t_smooth);
    cam_tf.translation.x = pos.x;
    cam_tf.translation.y = pos.y - DEFAULT_HEIGHT * OVERWORLD_TILT_OFFSET;

    if t >= 1.0 {
        commands.remove_resource::<CameraPanState>();
    }
}

/// System: apply random offset to camera for screen shake.
/// Removes itself when done.
pub fn tick_camera_shake(
    time: Res<Time>,
    mut shake: Option<ResMut<CameraShakeState>>,
    mut camera_q: Query<&mut Transform, With<CombatCamera3d>>,
    mut commands: Commands,
) {
    let Some(ref mut shake) = shake else { return };
    let Ok(mut cam_tf) = camera_q.single_mut() else { return };

    shake.elapsed += time.delta_secs();
    let t = if shake.duration > 0.0 { (shake.elapsed / shake.duration).min(1.0) } else { 1.0 };

    if t >= 1.0 {
        commands.remove_resource::<CameraShakeState>();
        return;
    }

    // Intensity decays linearly over duration
    let strength = shake.intensity * (1.0 - t);
    let time_val = time.elapsed_secs();
    let offset_x = (time_val * 73.1).sin() * strength;
    let offset_y = (time_val * 97.3).cos() * strength;

    cam_tf.translation.x += offset_x;
    cam_tf.translation.y += offset_y;
}
