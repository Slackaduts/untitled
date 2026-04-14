//! Custom 3D physics backend for bevy_ecs_tiled.
//!
//! Converts Tiled collision shapes into Avian3D colliders with a fixed
//! Z extent of one tile height (48 units).

use avian3d::{
    parry::{
        math::{Isometry, Real, Vector},
        shape::SharedShape,
    },
    prelude::*,
};
use bevy::prelude::*;
use bevy_ecs_tilemap::map::TilemapGridSize;
use tiled::{ObjectLayerData, ObjectShape};

use bevy_ecs_tiled::prelude::*;

use super::DEFAULT_TILE_SIZE;

const COLLIDER_HALF_HEIGHT: f32 = DEFAULT_TILE_SIZE / 2.0;

/// 3D physics backend for bevy_ecs_tiled.
#[derive(Default, Reflect, Copy, Clone, Debug)]
#[reflect(Default, Debug)]
pub struct TiledPhysics3dBackend;

impl TiledPhysicsBackend for TiledPhysics3dBackend {
    fn spawn_colliders(
        &self,
        commands: &mut Commands,
        tiled_map: &TiledMap,
        filter: &TiledNameFilter,
        collider: &TiledCollider,
    ) -> Vec<TiledColliderSpawnInfos> {
        match collider {
            TiledCollider::Object {
                layer_id: _,
                object_id: _,
            } => {
                let Some(object) = collider.get_object(tiled_map) else {
                    return vec![];
                };

                match object.get_tile() {
                    Some(object_tile) => object_tile.get_tile().and_then(|tile| {
                        let Some(object_layer_data) = &tile.collision else {
                            return None;
                        };
                        let mut composables = vec![];
                        let mut spawn_infos = vec![];
                        compose_tiles(
                            commands,
                            filter,
                            object_layer_data,
                            Vec2::ZERO,
                            get_grid_size(&tiled_map.map),
                            &mut composables,
                            &mut spawn_infos,
                        );
                        if !composables.is_empty() {
                            let collider: Collider = SharedShape::compound(composables).into();
                            spawn_infos.push(TiledColliderSpawnInfos {
                                name: "Avian3D[ComposedTile]".to_string(),
                                entity: commands.spawn(collider).id(),
                                transform: Transform::default(),
                            });
                        }
                        Some(spawn_infos)
                    }),
                    None => get_position_and_shape(&object.shape).map(|(pos, shared_shape, _)| {
                        let collider: Collider = shared_shape.into();
                        let iso = Isometry3d::from_rotation(Quat::from_rotation_z(
                            f32::to_radians(-object.rotation),
                        )) * Isometry3d::from_xyz(pos.x, pos.y, 0.);
                        vec![TiledColliderSpawnInfos {
                            name: format!("Avian3D[Object={}]", object.name),
                            entity: commands.spawn(collider).id(),
                            transform: Transform::from_isometry(iso),
                        }]
                    }),
                }
                .unwrap_or_default()
            }
            TiledCollider::TilesLayer { layer_id: _ } => {
                let mut composables = vec![];
                let mut spawn_infos = vec![];
                for (tile_position, tile) in collider.get_tiles(tiled_map) {
                    if let Some(collision) = &tile.collision {
                        compose_tiles(
                            commands,
                            filter,
                            collision,
                            tile_position,
                            get_grid_size(&tiled_map.map),
                            &mut composables,
                            &mut spawn_infos,
                        );
                    }
                }
                if !composables.is_empty() {
                    let collider: Collider = SharedShape::compound(composables).into();
                    spawn_infos.push(TiledColliderSpawnInfos {
                        name: "Avian3D[ComposedTile]".to_string(),
                        entity: commands.spawn(collider).id(),
                        transform: Transform::default(),
                    });
                }
                spawn_infos
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn compose_tiles(
    commands: &mut Commands,
    filter: &TiledNameFilter,
    object_layer_data: &ObjectLayerData,
    tile_offset: Vec2,
    grid_size: TilemapGridSize,
    composables: &mut Vec<(Isometry<Real>, SharedShape)>,
    spawn_infos: &mut Vec<TiledColliderSpawnInfos>,
) {
    for object in object_layer_data.object_data() {
        if !filter.contains(&object.name) {
            continue;
        }
        let object_position = Vec2 {
            x: object.x - grid_size.x / 2.,
            y: (grid_size.y - object.y) - grid_size.y / 2.,
        };
        if let Some((shape_offset, shared_shape, is_composable)) =
            get_position_and_shape(&object.shape)
        {
            let mut position = tile_offset + object_position;
            position += Vec2 {
                x: grid_size.x / 2.,
                y: grid_size.y / 2.,
            };
            if is_composable {
                let pos_v = Vector::new(position.x as Real, position.y as Real, 0.0);
                let offset_v = Vector::new(shape_offset.x as Real, shape_offset.y as Real, 0.0);
                let rot = avian3d::parry::math::Rotation::from_euler_angles(
                    0.0, 0.0, f32::to_radians(-object.rotation) as Real,
                );
                composables.push((
                    Isometry::from_parts(pos_v.into(), rot)
                        * Isometry::from_parts(offset_v.into(), avian3d::parry::math::Rotation::identity()),
                    shared_shape,
                ));
            } else {
                let collider: Collider = shared_shape.into();
                let iso = Isometry3d::from_xyz(position.x, position.y, 0.)
                    * Isometry3d::from_rotation(Quat::from_rotation_z(f32::to_radians(
                        -object.rotation,
                    )));
                spawn_infos.push(TiledColliderSpawnInfos {
                    name: "Avian3D[ComplexTile]".to_string(),
                    entity: commands.spawn(collider).id(),
                    transform: Transform::from_isometry(iso),
                });
            }
        }
    }
}

fn get_position_and_shape(shape: &ObjectShape) -> Option<(Vec2, SharedShape, bool)> {
    match shape {
        ObjectShape::Rect { width, height } => {
            let shape = SharedShape::cuboid(
                *width as Real / 2.,
                *height as Real / 2.,
                COLLIDER_HALF_HEIGHT as Real,
            );
            let pos = Vec2::new(width / 2., -height / 2.);
            Some((pos, shape, true))
        }
        ObjectShape::Ellipse { width, height } => {
            // Approximate as cylinder
            let radius = ((width + height) / 4.0) as Real;
            let shape = SharedShape::cylinder(COLLIDER_HALF_HEIGHT as Real, radius);
            let pos = Vec2::new(width / 2., -height / 2.);
            Some((pos, shape, true))
        }
        ObjectShape::Polyline { points } | ObjectShape::Polygon { points } => {
            if points.len() < 2 {
                return None;
            }
            // Convert to compound of thin cuboid segments
            let pts: Vec<Vec2> = points.iter().map(|(x, y)| Vec2::new(*x, -*y)).collect();
            let is_polygon = matches!(shape, ObjectShape::Polygon { .. });
            let edge_count = if is_polygon { pts.len() } else { pts.len() - 1 };

            let mut shapes: Vec<(Isometry<Real>, SharedShape)> = Vec::new();
            for i in 0..edge_count {
                let p0 = pts[i];
                let p1 = pts[(i + 1) % pts.len()];
                let center = (p0 + p1) * 0.5;
                let diff = p1 - p0;
                let length = diff.length();
                let angle = diff.y.atan2(diff.x);
                let seg = SharedShape::cuboid(
                    length as Real / 2.0,
                    2.0,
                    COLLIDER_HALF_HEIGHT as Real,
                );
                let pos_v = Vector::new(center.x as Real, center.y as Real, 0.0);
                let rot = avian3d::parry::math::Rotation::from_euler_angles(0.0, 0.0, angle as Real);
                shapes.push((
                    Isometry::from_parts(pos_v.into(), rot),
                    seg,
                ));
            }
            let compound = SharedShape::compound(shapes);
            Some((Vec2::ZERO, compound, false))
        }
        _ => None,
    }
}

fn get_grid_size(map: &tiled::Map) -> TilemapGridSize {
    TilemapGridSize {
        x: map.tile_width as f32,
        y: map.tile_height as f32,
    }
}
