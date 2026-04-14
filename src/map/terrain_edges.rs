use avian3d::prelude::*;
use bevy::prelude::*;

use super::terrain_material::{terrain_id, TerrainTypeMapReady};
use bevy_ecs_tilemap::prelude::*;
use bevy_ecs_tiled::prelude::*;

#[derive(Component)]
pub(crate) struct EdgeCollidersGenerated;

/// After the terrain type map is built, fill all water tiles with merged
/// rectangular colliders using a greedy meshing algorithm.
///
/// Water is impassable. All other terrain types are walkable.
pub fn generate_edge_colliders(
    mut commands: Commands,
    layers: Query<
        (Entity, &TileStorage, &TilemapSize, &TilemapTileSize, &Name, &Parent),
        (With<TerrainTypeMapReady>, With<TiledMapTileLayerForTileset>, Without<EdgeCollidersGenerated>),
    >,
    tile_indices: Query<&TileTextureIndex>,
) {
    let mut parent_data: bevy::utils::HashMap<Entity, (u32, u32, f32, f32, Vec<(Entity, String)>)> =
        bevy::utils::HashMap::new();

    for (entity, _storage, map_size, tile_size, name, parent) in &layers {
        let entry = parent_data
            .entry(parent.get())
            .or_insert((map_size.x, map_size.y, tile_size.x, tile_size.y, Vec::new()));
        entry.4.push((entity, name.as_str().to_string()));
    }

    let mut seen_parents: bevy::utils::HashSet<Entity> = bevy::utils::HashSet::new();

    for (parent_entity, (map_w, map_h, tile_w, tile_h, siblings)) in &parent_data {
        if !seen_parents.insert(*parent_entity) {
            continue;
        }

        let w = *map_w as usize;
        let h = *map_h as usize;

        // Build terrain ID grid from all sibling layers.
        let mut grid = vec![terrain_id::EMPTY; w * h];

        for (entity, name_str) in siblings {
            let Ok((_, storage, _, _, _, _)) = layers.get(*entity) else { continue };

            let tileset_name = name_str
                .strip_prefix("TiledMapTileLayerForTileset(")
                .and_then(|s| s.strip_suffix(')'))
                .and_then(|s| s.rsplit_once(", "))
                .map(|(_, ts)| ts)
                .unwrap_or(name_str);

            let is_terrain_surfaces = tileset_name == "terrain_surfaces";
            let uniform_id = super::terrain_material::classify_tileset(tileset_name);

            if !is_terrain_surfaces && uniform_id.is_none() {
                continue;
            }

            for y in 0..h {
                for x in 0..w {
                    let pos = TilePos::new(x as u32, y as u32);
                    let Some(tile_entity) = storage.checked_get(&pos) else { continue };

                    let state_id = if is_terrain_surfaces {
                        if let Ok(tex_idx) = tile_indices.get(tile_entity) {
                            (tex_idx.0 as u8).saturating_add(1)
                        } else {
                            continue;
                        }
                    } else {
                        uniform_id.unwrap()
                    };

                    grid[y * w + x] = state_id;
                }
            }
        }

        // ── Greedy rectangle merge over impassable tiles ───────────────
        // River and shallows are impassable → fill with colliders.
        let is_impassable = |id: u8| id == terrain_id::RIVER || id == terrain_id::SHALLOWS;

        let mut used = vec![false; w * h];
        let mut collider_count = 0u32;

        for start_y in 0..h {
            for start_x in 0..w {
                if used[start_y * w + start_x] || !is_impassable(grid[start_y * w + start_x]) {
                    continue;
                }

                // Extend right as far as possible
                let mut end_x = start_x;
                while end_x < w
                    && is_impassable(grid[start_y * w + end_x])
                    && !used[start_y * w + end_x]
                {
                    end_x += 1;
                }
                let rect_w = end_x - start_x;

                // Extend down: check each subsequent row has the full width available
                let mut end_y = start_y + 1;
                'outer: while end_y < h {
                    for x in start_x..end_x {
                        if !is_impassable(grid[end_y * w + x]) || used[end_y * w + x] {
                            break 'outer;
                        }
                    }
                    end_y += 1;
                }
                let rect_h = end_y - start_y;

                // Mark used
                for y in start_y..end_y {
                    for x in start_x..end_x {
                        used[y * w + x] = true;
                    }
                }

                // Spawn collider centered on the merged rectangle
                let cx = (start_x as f32 + rect_w as f32 * 0.5) * tile_w;
                let cy = (start_y as f32 + rect_h as f32 * 0.5) * tile_h;
                commands.spawn((
                    RigidBody::Static,
                    Collider::cuboid(
                        rect_w as f32 * tile_w,
                        rect_h as f32 * tile_h,
                        48.0,
                    ),
                    Transform::from_xyz(cx, cy, 0.0),
                ));
                collider_count += 1;
            }
        }

        // Mark all siblings as processed
        for (entity, _) in siblings {
            commands.entity(*entity).insert(EdgeCollidersGenerated);
        }

        info!(
            "Generated {} water colliders for terrain layer ({}x{} grid)",
            collider_count, w, h
        );
    }
}
