//! Live preview system for the F6 tile editor.
//!
//! When editing object properties, this system updates matching billboard
//! lights in real-time so changes are visible immediately in the world.

use bevy::prelude::*;

use super::state::{TileEditorState, PlacedObject, SidecarChild};
use crate::billboard::object_types::ObjectSpriteLight;
use crate::camera::combat::{Billboard, BillboardSpriteKey, BillboardHeight};

/// Applies property changes in real-time to matching billboard entities.
/// Spawns/updates/despawns lights for the object currently being edited.
///
/// When editing a specific instance (`editing_placed_idx` is set), only that
/// instance's billboard and lights are affected. When editing the root object
/// (Library mode), all instances with the same sprite_key are updated.
pub fn live_preview_system(
    mut commands: Commands,
    state: Res<TileEditorState>,
    mut prev_selected: Local<Option<String>>,
    mut queries: ParamSet<(
        Query<(Entity, &BillboardSpriteKey, &Transform, &BillboardHeight, Option<&PlacedObject>), With<Billboard>>,
        Query<(Entity, &mut ObjectSpriteLight, &mut Transform, &mut crate::lighting::components::LightSource, Option<&SidecarChild>)>,
    )>,
) {
    let current_key = state.open
        .then(|| state.current_object.as_ref())
        .flatten()
        .map(|obj| obj.sprite_key.clone());

    if current_key != *prev_selected {
        *prev_selected = current_key.clone();
    }

    if !state.open || current_key.is_none() {
        return;
    }

    let obj = state.current_object.as_ref().unwrap();
    let sprite_key = &obj.sprite_key;

    // When editing a specific instance, only affect that one billboard/lights.
    let editing_sidecar_id: Option<String> = state.editing_placed_idx
        .and_then(|idx| state.placed_objects.get(idx))
        .map(|p| p.id.clone());

    // Read billboard transforms (filtered to the specific instance if per-instance editing)
    let mut billboard_data: Vec<(Vec3, Quat, f32, f32)> = Vec::new();
    {
        let bb_query = queries.p0();
        for (_bb_entity, key, bb_tf, bb_height, placed) in &bb_query {
            if key.0 != *sprite_key {
                continue;
            }
            // If editing a specific instance, only match that one billboard
            if let Some(ref target_id) = editing_sidecar_id {
                if placed.map(|p| p.sidecar_id.as_str()) != Some(target_id.as_str()) {
                    continue;
                }
            }
            let ts_name = key.0.rsplit_once('_').map(|(n,_)| n).unwrap_or(&key.0);
            let sprite_w = {
                let qoi = format!("assets/objects/{ts_name}/{}/sprite.qoi", key.0);
                let png = format!("assets/objects/{ts_name}/{}/sprite.png", key.0);
                let path = if std::path::Path::new(&qoi).exists() { qoi } else { png };
                image::image_dimensions(&path).map(|(w,_)| w as f32).unwrap_or(bb_height.height)
            };
            billboard_data.push((bb_tf.translation, bb_tf.rotation, bb_height.height, sprite_w));
        }
    }

    // Update existing lights in-place, or spawn/despawn as needed
    {
        let light_query = queries.p1();

        // Filter lights: by sprite_key, and by sidecar_id if per-instance
        let matches_filter = |osl: &ObjectSpriteLight, sc: Option<&SidecarChild>| -> bool {
            if osl.sprite_key != *sprite_key {
                return false;
            }
            if let Some(ref target_id) = editing_sidecar_id {
                return sc.map(|c| c.sidecar_id.as_str()) == Some(target_id.as_str());
            }
            true
        };

        let existing_count = light_query.iter()
            .filter(|(_, m, _, _, sc)| matches_filter(m, sc.as_deref()))
            .count();
        let desired_count = obj.properties.lights.len() * billboard_data.len();

        if existing_count != desired_count {
            let to_despawn: Vec<Entity> = light_query.iter()
                .filter(|(_, m, _, _, sc)| matches_filter(m, sc.as_deref()))
                .map(|(e, _, _, _, _)| e)
                .collect();
            drop(light_query);
            for entity in to_despawn {
                commands.entity(entity).despawn();
            }

            for &(bb_pos, bb_rot, bb_h, bb_w) in &billboard_data {
                for light_def in &obj.properties.lights {
                    use crate::lighting::components::*;

                    let shape = match light_def.shape.as_str() {
                        "cone" => LightShape::Cone { direction: 0.0, angle: std::f32::consts::FRAC_PI_2 },
                        "line" => LightShape::Line { end_offset: Vec2::new(48.0, 0.0) },
                        "capsule" => LightShape::Capsule { direction: 0.0, half_length: 24.0 },
                        _ => LightShape::Point,
                    };

                    let light_pos = light_world_pos(
                        bb_pos, bb_rot, bb_h, bb_w,
                        light_def.offset_x, light_def.offset_y, light_def.offset_z,
                    );

                    let mut ecmds = commands.spawn((
                        Transform::from_translation(light_pos),
                        Visibility::default(),
                        LightSource {
                            color: Color::linear_rgb(light_def.color[0], light_def.color[1], light_def.color[2]),
                            base_intensity: light_def.intensity,
                            intensity: light_def.intensity,
                            inner_radius: light_def.radius * 0.3,
                            outer_radius: light_def.radius,
                            shape,
                            pulse: if light_def.pulse { Some(PulseConfig::default()) } else { None },
                            flicker: if light_def.flicker { Some(FlickerConfig::default()) } else { None },
                            anim_seed: rand::random::<f32>() * 100.0,
                            ..default()
                        },
                        ObjectSpriteLight {
                            sprite_key: sprite_key.clone(),
                            ref_id: light_def.ref_id.clone(),
                            offset_x: light_def.offset_x,
                            offset_y: light_def.offset_y,
                            offset_z: light_def.offset_z,
                            sprite_width: bb_w,
                        },
                    ));
                    // Attach SidecarChild if editing a specific instance
                    if let Some(ref target_id) = editing_sidecar_id {
                        ecmds.insert(SidecarChild {
                            sidecar_id: target_id.clone(),
                            ref_id: light_def.ref_id.clone(),
                        });
                    }
                }
            }
        } else {
            drop(light_query);
            let mut light_query_mut = queries.p1();
            let mut light_iter: Vec<_> = light_query_mut.iter_mut()
                .filter(|(_, m, _, _, sc)| matches_filter(m, sc.as_deref()))
                .collect();

            let mut li = 0;
            for &(bb_pos, bb_rot, bb_h, bb_w) in &billboard_data {
                for light_def in &obj.properties.lights {
                    if li >= light_iter.len() { break; }
                    let (_, ref mut osl, ref mut tf, ref mut ls, _) = light_iter[li];

                    osl.offset_x = light_def.offset_x;
                    osl.offset_y = light_def.offset_y;
                    osl.offset_z = light_def.offset_z;
                    osl.sprite_width = bb_w;

                    tf.translation = light_world_pos(
                        bb_pos, bb_rot, bb_h, bb_w,
                        light_def.offset_x, light_def.offset_y, light_def.offset_z,
                    );

                    ls.color = Color::linear_rgb(light_def.color[0], light_def.color[1], light_def.color[2]);
                    ls.base_intensity = light_def.intensity;
                    ls.intensity = light_def.intensity;
                    ls.inner_radius = light_def.radius * 0.3;
                    ls.outer_radius = light_def.radius;

                    li += 1;
                }
            }
        }
    }
}

/// Computes the world position of a light on a billboard face.
pub(crate) fn light_world_pos(bb_pos: Vec3, bb_rot: Quat, bb_h: f32, bb_w: f32, offset_x: f32, offset_y: f32, offset_z: f32) -> Vec3 {
    let tile = crate::map::DEFAULT_TILE_SIZE;
    let local = Vec3::new(
        (offset_x - 0.5) * bb_w,
        (1.0 - offset_y) * bb_h - tile * 0.5,
        offset_z * bb_h,
    );
    bb_pos + bb_rot * local
}
