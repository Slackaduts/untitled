use bevy::prelude::*;
use std::collections::HashMap;
use bevy_ecs_tiled::prelude::*;
use bevy_ecs_tilemap::prelude::*;

use super::components::{FlickerConfig, LightShape, LightSource, PulseConfig};
use crate::camera::combat::{BillboardTileQuad, BillboardTilesReady};

/// Light definition parsed from a Tiled tileset tile's custom properties.
///
/// In Tiled: select a tile in the tileset editor, set its **Class** (or Type)
/// to `TileLight`, then add custom properties:
///   - `radius` (float) — outer falloff radius in world units
///   - `intensity` (float) — light intensity
///   - `color_r`, `color_g`, `color_b` (float) — linear RGB, 0–1
///   - `pulse` (bool) — enable sine-wave oscillation
///   - `flicker` (bool) — enable occasional intensity dip
#[derive(Debug, Clone)]
pub struct TileLightDef {
    pub radius: f32,
    pub intensity: f32,
    pub color_r: f32,
    pub color_g: f32,
    pub color_b: f32,
    pub pulse: bool,
    pub flicker: bool,
    pub offset_x: f32,
    pub offset_y: f32,
    /// Shape: "point", "cone", "line", "capsule"
    pub shape: String,
    /// Direction angle in degrees (cone, capsule). 0=right, 90=up.
    pub direction: f32,
    /// Full cone spread in degrees (cone only).
    pub angle: f32,
    /// Half-length in world units (capsule) or unused.
    pub length: f32,
    /// Line endpoint2 offset as tile fraction (line only).
    pub end_offset_x: f32,
    pub end_offset_y: f32,
}

/// Marker so we only process each tile layer once.
#[derive(Component)]
pub struct TileLightsProcessed;

/// Marks a light as spawned from a tile, storing its world position
/// so we can parent it to a billboard quad later.
#[derive(Component)]
pub struct TileLightSource {
    pub world_pos: Vec2,
}

/// Marker: this light has been parented to a billboard quad.
#[derive(Component)]
pub struct TileLightParented;

/// Scans newly loaded tile layers for tiles whose tileset entry has
/// `user_type == "TileLight"`, and spawns `LightSource` entities at their
/// world positions.
pub fn spawn_lights_from_tile_properties(
    mut commands: Commands,
    map_assets: Res<Assets<TiledMap>>,
    map_handles: Query<&TiledMapHandle>,
    new_layers: Query<
        (Entity, &Name, &TileStorage, &TilemapSize, &TilemapTileSize),
        (With<TiledMapTileLayerForTileset>, Without<TileLightsProcessed>),
    >,
    tile_indices: Query<&TileTextureIndex>,
) {
    let Some(tiled_map) = map_handles
        .iter()
        .find_map(|h| map_assets.get(&h.0))
    else {
        return;
    };

    let light_defs = collect_tile_light_defs(&tiled_map.map);
    if light_defs.is_empty() {
        for (entity, _, _, _, _) in &new_layers {
            commands.entity(entity).insert(TileLightsProcessed);
        }
        return;
    }

    for (layer_entity, name, storage, size, tile_size) in &new_layers {
        let tileset_name = name
            .as_str()
            .strip_prefix("TiledMapTileLayerForTileset(")
            .and_then(|s| s.strip_suffix(')'))
            .and_then(|s| s.rsplit_once(", "))
            .map(|(_, ts)| ts);

        let Some(tileset_name) = tileset_name else {
            commands.entity(layer_entity).insert(TileLightsProcessed);
            continue;
        };

        for y in 0..size.y {
            for x in 0..size.x {
                let pos = TilePos::new(x, y);
                let Some(tile_entity) = storage.checked_get(&pos) else {
                    continue;
                };
                let Ok(tex_idx) = tile_indices.get(tile_entity) else {
                    continue;
                };

                let key = (tileset_name, tex_idx.0);
                let Some(def) = light_defs.get(&key) else {
                    continue;
                };

                let world_x = (x as f32 + 0.5 + def.offset_x) * tile_size.x;
                let world_y = (y as f32 + 0.5 + def.offset_y) * tile_size.y;

                info!(
                    "Spawning tile light at ({:.0}, {:.0}) from tileset '{}' tile {}",
                    world_x, world_y, tileset_name, tex_idx.0
                );

                let shape = match def.shape.as_str() {
                    "cone" => LightShape::Cone {
                        direction: def.direction.to_radians(),
                        angle: def.angle.to_radians(),
                    },
                    "line" => LightShape::Line {
                        end_offset: Vec2::new(
                            def.end_offset_x * tile_size.x,
                            def.end_offset_y * tile_size.y,
                        ),
                    },
                    "capsule" => LightShape::Capsule {
                        direction: def.direction.to_radians(),
                        half_length: def.length / 2.0,
                    },
                    _ => LightShape::Point,
                };

                commands.spawn((
                    Transform::from_xyz(world_x, world_y, 0.0),
                    GlobalTransform::default(),
                    LightSource {
                        color: Color::linear_rgb(def.color_r, def.color_g, def.color_b),
                        base_intensity: def.intensity,
                        intensity: def.intensity,
                        inner_radius: def.radius * 0.3,
                        outer_radius: def.radius,
                        shape,
                        pulse: if def.pulse {
                            Some(PulseConfig::default())
                        } else {
                            None
                        },
                        flicker: if def.flicker {
                            Some(FlickerConfig::default())
                        } else {
                            None
                        },
                        anim_seed: rand::random::<f32>() * 100.0,
                        ..default()
                    },
                    TileLightSource {
                        world_pos: Vec2::new(world_x, world_y),
                    },
                ));
            }
        }

        commands.entity(layer_entity).insert(TileLightsProcessed);
    }
}

/// After billboard quads are created, parent tile lights to the nearest
/// billboard quad so they rotate together during combat camera tilt.
pub fn parent_tile_lights_to_billboards(
    mut commands: Commands,
    billboard_ready: Res<BillboardTilesReady>,
    unparented_lights: Query<
        (Entity, &TileLightSource),
        Without<TileLightParented>,
    >,
    billboard_quads: Query<(Entity, &Transform), With<BillboardTileQuad>>,
) {
    if !billboard_ready.0 || unparented_lights.is_empty() {
        return;
    }

    for (light_entity, tile_light) in &unparented_lights {
        // Find the billboard quad whose origin is closest to this light.
        // Billboard quads are positioned at their center-x, bottom-y.
        let mut best: Option<(Entity, f32, Vec3)> = None;

        for (quad_entity, quad_tf) in &billboard_quads {
            let quad_pos = quad_tf.translation.truncate();
            let dist = quad_pos.distance(tile_light.world_pos);
            if best.is_none() || dist < best.unwrap().1 {
                best = Some((quad_entity, dist, quad_tf.translation));
            }
        }

        if let Some((quad_entity, _, quad_translation)) = best {
            // Compute local offset from quad's transform origin
            let local_x = tile_light.world_pos.x - quad_translation.x;
            let local_y = tile_light.world_pos.y - quad_translation.y;

            commands.entity(light_entity).insert(TileLightParented);
            commands.entity(light_entity).set_parent(quad_entity);
            // Set local transform relative to the quad
            commands.entity(light_entity).insert(
                Transform::from_xyz(local_x, local_y, 0.0),
            );
        } else {
            // No billboard quads exist — mark as parented to skip future checks
            commands.entity(light_entity).insert(TileLightParented);
        }
    }
}


/// Parse tileset tiles that have `user_type == "TileLight"` and extract
/// their custom properties into `TileLightDef`s.
pub fn collect_tile_light_defs<'a>(
    map: &'a tiled::Map,
) -> HashMap<(&'a str, u32), TileLightDef> {
    let mut defs = HashMap::new();

    for tileset in map.tilesets().iter() {
        for (tile_id, tile_data) in tileset.tiles() {
            let Some(ref user_type) = tile_data.user_type else {
                continue;
            };
            if user_type != "TileLight" {
                continue;
            }

            let mut def = TileLightDef {
                radius: 100.0,
                intensity: 1.0,
                color_r: 1.0,
                color_g: 0.85,
                color_b: 0.6,
                pulse: false,
                flicker: false,
                offset_x: 0.0,
                offset_y: 0.0,
                shape: "point".to_string(),
                direction: 0.0,
                angle: 90.0,
                length: 0.0,
                end_offset_x: 0.0,
                end_offset_y: 0.0,
            };

            for (name, value) in &tile_data.properties {
                use tiled::PropertyValue::*;
                match (name.as_str(), value) {
                    ("radius", FloatValue(v)) => def.radius = *v,
                    ("radius", IntValue(v)) => def.radius = *v as f32,
                    ("intensity", FloatValue(v)) => def.intensity = *v,
                    ("intensity", IntValue(v)) => def.intensity = *v as f32,
                    ("color_r", FloatValue(v)) => def.color_r = *v,
                    ("color_g", FloatValue(v)) => def.color_g = *v,
                    ("color_b", FloatValue(v)) => def.color_b = *v,
                    ("pulse", BoolValue(v)) => def.pulse = *v,
                    ("flicker", BoolValue(v)) => def.flicker = *v,
                    ("offset_x", FloatValue(v)) => def.offset_x = *v,
                    ("offset_y", FloatValue(v)) => def.offset_y = *v,
                    ("shape", StringValue(v)) => def.shape = v.clone(),
                    ("direction", FloatValue(v)) => def.direction = *v,
                    ("direction", IntValue(v)) => def.direction = *v as f32,
                    ("angle", FloatValue(v)) => def.angle = *v,
                    ("angle", IntValue(v)) => def.angle = *v as f32,
                    ("length", FloatValue(v)) => def.length = *v,
                    ("length", IntValue(v)) => def.length = *v as f32,
                    ("end_offset_x", FloatValue(v)) => def.end_offset_x = *v,
                    ("end_offset_y", FloatValue(v)) => def.end_offset_y = *v,
                    _ => {}
                }
            }

            defs.insert((tileset.name.as_str(), tile_id), def);
        }
    }

    defs
}
