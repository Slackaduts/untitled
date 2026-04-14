//! Custom 3D physics backend for bevy_ecs_tiled.
//!
//! Converts Tiled 2D collision polygons into Avian3D colliders by extruding
//! the polygon triangles into 3D cuboids with a fixed Z extent of one tile
//! height (48 units).

use avian3d::{
    parry::{
        math::{Pose, Real},
        shape::SharedShape,
    },
    prelude::*,
};
use bevy::prelude::*;

use bevy_ecs_tiled::prelude::*;

use super::DEFAULT_TILE_SIZE;

const COLLIDER_HALF_HEIGHT: f32 = DEFAULT_TILE_SIZE / 2.0;

/// 3D physics backend for bevy_ecs_tiled.
///
/// The new bevy_ecs_tiled 0.11 API provides pre-computed `geo::MultiPolygon`
/// geometry. We triangulate it and extrude each triangle into a 3D prism
/// (approximated as a compound of cuboids) so the resulting colliders work
/// with Avian3D.
#[derive(Default, Reflect, Copy, Clone, Debug)]
#[reflect(Default, Debug)]
pub struct TiledPhysics3dBackend;

impl TiledPhysicsBackend for TiledPhysics3dBackend {
    fn spawn_colliders(
        &self,
        commands: &mut Commands,
        source: &TiledEvent<ColliderCreated>,
        multi_polygon: &geo::MultiPolygon<f32>,
    ) -> Vec<Entity> {
        let mut out = vec![];

        // Triangulate the 2D polygon and extrude each triangle into a 3D
        // compound collider (cuboid approximation with COLLIDER_HALF_HEIGHT).
        let triangles = multi_polygon_as_triangles(multi_polygon);
        if triangles.is_empty() {
            return out;
        }

        let shared_shapes: Vec<(Pose, SharedShape)> = triangles
            .iter()
            .map(|([a, b, c], centroid)| {
                // Compute an axis-aligned bounding box for the triangle and
                // use a cuboid as a conservative approximation.
                let min_x = a.x.min(b.x).min(c.x);
                let max_x = a.x.max(b.x).max(c.x);
                let min_y = a.y.min(b.y).min(c.y);
                let max_y = a.y.max(b.y).max(c.y);
                let half_w = ((max_x - min_x) / 2.0).max(0.5) as Real;
                let half_h = ((max_y - min_y) / 2.0).max(0.5) as Real;

                (
                    Pose::from_translation(avian3d::parry::math::Vector::new(
                        centroid.x as Real,
                        centroid.y as Real,
                        0.0,
                    )),
                    SharedShape::cuboid(half_w, half_h, COLLIDER_HALF_HEIGHT as Real),
                )
            })
            .collect();

        if !shared_shapes.is_empty() {
            let collider: Collider = SharedShape::compound(shared_shapes).into();
            out.push(
                commands
                    .spawn((
                        Name::from("Avian3D[Compound3D]"),
                        ChildOf(*source.event.collider_of),
                        collider,
                    ))
                    .id(),
            );
        }

        out
    }
}
