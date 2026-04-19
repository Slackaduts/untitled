use bevy::prelude::*;

use super::CombatCamera3d;
use super::follow::{DEFAULT_HEIGHT, OVERWORLD_TILT_OFFSET};

/// Axis-aligned rectangle on the XY plane representing the camera's visible area.
/// Computed once per frame from the camera transform.
#[derive(Resource, Default)]
pub struct CameraVisibleRect {
    pub center: Vec2,
    pub half_extents: Vec2,
}

impl CameraVisibleRect {
    /// Returns true if a circle (center, radius) overlaps this rect expanded by `margin`.
    pub fn overlaps_circle(&self, pos: Vec2, radius: f32, margin: f32) -> bool {
        let dx = (pos.x - self.center.x).abs() - (self.half_extents.x + margin);
        let dy = (pos.y - self.center.y).abs() - (self.half_extents.y + margin);
        let dx = dx.max(0.0);
        let dy = dy.max(0.0);
        dx * dx + dy * dy <= radius * radius
    }

    /// Simple point-in-expanded-rect check.
    pub fn contains_point(&self, pos: Vec2, margin: f32) -> bool {
        let dx = (pos.x - self.center.x).abs();
        let dy = (pos.y - self.center.y).abs();
        dx <= self.half_extents.x + margin && dy <= self.half_extents.y + margin
    }
}

/// Derives the visible rect from the camera transform each frame.
/// The camera sits at (target_x, target_y - height*tilt_offset, height)
/// looking north-ish. The visible area on the ground is a predictable
/// rectangle centered on the look-at point.
pub fn update_camera_visible_rect(
    camera_q: Query<&Transform, With<CombatCamera3d>>,
    mut rect: ResMut<CameraVisibleRect>,
) {
    let Ok(cam_tf) = camera_q.single() else { return };

    // Camera is at (x, target_y - h*tilt, h), looking at (x, target_y, 0).
    // Recover the look-at center on the ground plane:
    let center = Vec2::new(
        cam_tf.translation.x,
        cam_tf.translation.y + DEFAULT_HEIGHT * OVERWORLD_TILT_OFFSET,
    );
    rect.center = center;

    // Conservative half-extents for the visible area on the XY plane.
    // At 900 unit height with ~45° FOV and ~42° tilt, the visible footprint
    // is roughly 1000x900 world units. Use generous values to prevent pop-in.
    rect.half_extents = Vec2::new(550.0, 500.0);
}
