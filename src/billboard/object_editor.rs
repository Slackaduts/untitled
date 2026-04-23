use bevy::prelude::*;

// Re-export shared types so existing `crate::billboard::object_editor::X` paths still work.
pub use super::object_types::{
    ObjectProperties, ObjectLight, ObjectEmitter,
    ObjectSpriteLight, ObjectSpriteEmitter,
};

/// Repositions object lights and emitters each frame so they follow billboard tilt.
/// Runs after billboard_system to use the current frame's rotation.
pub fn update_object_light_positions(
    billboards: Query<
        (&crate::camera::combat::BillboardSpriteKey, &Transform, &crate::camera::combat::BillboardHeight),
        With<crate::camera::combat::Billboard>,
    >,
    mut lights: Query<
        (&ObjectSpriteLight, &mut Transform),
        (Without<crate::camera::combat::Billboard>, Without<ObjectSpriteEmitter>),
    >,
    mut emitters: Query<
        (&ObjectSpriteEmitter, &mut Transform),
        (Without<crate::camera::combat::Billboard>, Without<ObjectSpriteLight>),
    >,
) {
    let mut bb_map: std::collections::HashMap<&str, Vec<(Vec3, Quat, f32)>> =
        std::collections::HashMap::new();
    for (key, tf, bb_h) in &billboards {
        bb_map.entry(key.0.as_str())
            .or_default()
            .push((tf.translation, tf.rotation, bb_h.height));
    }

    for (osl, mut tf) in &mut lights {
        if let Some(bb_pos) = find_nearest_billboard(&bb_map, &osl.sprite_key, tf.translation) {
            tf.translation = light_world_pos(bb_pos.0, bb_pos.1, bb_pos.2, osl.sprite_width, osl.offset_x, osl.offset_y, osl.offset_z);
        }
    }

    for (ose, mut tf) in &mut emitters {
        if let Some(bb_pos) = find_nearest_billboard(&bb_map, &ose.sprite_key, tf.translation) {
            tf.translation = light_world_pos(bb_pos.0, bb_pos.1, bb_pos.2, ose.sprite_width, ose.offset_x, ose.offset_y, ose.offset_z);
        }
    }
}

fn light_world_pos(bb_pos: Vec3, bb_rot: Quat, bb_h: f32, bb_w: f32, offset_x: f32, offset_y: f32, offset_z: f32) -> Vec3 {
    let tile = crate::map::DEFAULT_TILE_SIZE;
    let local = Vec3::new(
        (offset_x - 0.5) * bb_w,
        (1.0 - offset_y) * bb_h - tile * 0.5,
        offset_z * bb_h,
    );
    bb_pos + bb_rot * local
}

fn find_nearest_billboard<'a>(
    bb_map: &'a std::collections::HashMap<&str, Vec<(Vec3, Quat, f32)>>,
    key: &str,
    current_pos: Vec3,
) -> Option<(Vec3, Quat, f32)> {
    let bbs = bb_map.get(key)?;
    let &(pos, rot, h) = if bbs.len() == 1 {
        &bbs[0]
    } else {
        bbs.iter()
            .min_by(|a, b| {
                let da = a.0.distance_squared(current_pos);
                let db = b.0.distance_squared(current_pos);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap()
    };
    Some((pos, rot, h))
}
