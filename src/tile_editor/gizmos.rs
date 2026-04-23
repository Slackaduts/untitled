//! In-world gizmo visualization for collision rects and lights.

use bevy::prelude::*;

use crate::camera::combat::{Billboard, BillboardHeight, BillboardSpriteKey};
use crate::lighting::components::LightSource;
use crate::billboard::object_types::ObjectSpriteLight;
use super::state::{PlacedObject, TileEditorState};

/// Draw wireframe cuboids for collision rects of placed objects.
pub fn draw_placed_object_gizmos(
    state: Res<TileEditorState>,
    placed: Query<(&PlacedObject, &Transform, &BillboardHeight, &BillboardSpriteKey), With<Billboard>>,
    mut gizmos: Gizmos,
) {
    if !state.open {
        return;
    }

    let collision_color = Color::srgba(0.3, 0.9, 0.3, 0.5);

    for (po, tf, bh, _key) in &placed {
        // Find matching sidecar object to get collision rects
        let Some(obj_def) = state
            .placed_objects
            .iter()
            .find(|o| o.id == po.sidecar_id)
        else {
            continue;
        };

        for rect in &obj_def.collision_rects {
            // Convert sprite-local pixel coords to world coords
            let cx = rect.x + rect.w / 2.0 - bh.height * 0.5;
            let cy = rect.y + rect.h / 2.0;

            let world_pos = tf.translation
                + tf.rotation * Vec3::new(cx, cy, 0.0);

            // Draw a flat rect gizmo at the collision position
            gizmos.rect(
                Isometry3d::new(world_pos, Quat::IDENTITY),
                Vec2::new(rect.w, rect.h),
                collision_color,
            );
        }
    }
}

/// Draw light gizmo spheres for placed objects (reuses the same visual
/// style as the object editor's `draw_object_light_gizmos`).
pub fn draw_placed_light_gizmos(
    state: Res<TileEditorState>,
    lights: Query<(&ObjectSpriteLight, &Transform, &LightSource)>,
    mut gizmos: Gizmos,
) {
    if !state.open {
        return;
    }

    for (_osl, tf, ls) in &lights {
        let color = ls.color.with_alpha(0.4);
        gizmos.sphere(
            Isometry3d::from_translation(tf.translation),
            ls.inner_radius,
            color,
        );
        gizmos.sphere(
            Isometry3d::from_translation(tf.translation),
            ls.outer_radius,
            ls.color.with_alpha(0.12),
        );
    }
}
