use avian3d::prelude::*;
use bevy::prelude::*;
use bevy_ecs_tilemap::prelude::*;
use bevy_ecs_tiled::prelude::*;

use super::elevation::{ElevationConfig, SlopeLayer};
use super::terrain_material::TerrainTypeMapReady;
use super::DEFAULT_TILE_SIZE;

// ── Slope tile IDs (terrain_surfaces tileset) ──────────────────────────────

/// Explicit depth tile IDs. Tile index = ID - 1.
/// Each tile sets all 4 corners to `level * delta`.
/// Empty tiles (id=0) = sea level (height 0).
pub mod slope_id {
    // Tile 9 (ID 10) through tile 16 (ID 17)
    pub const NEG3: u8 = 10;  // -3
    pub const NEG2: u8 = 11;  // -2
    pub const NEG1: u8 = 12;  // -1
    pub const POS1: u8 = 13;  // +1
    pub const POS2: u8 = 14;  // +2
    pub const POS3: u8 = 15;  // +3
    pub const POS4: u8 = 16;  // +4
    pub const POS5: u8 = 17;  // +5

    /// Returns the depth level for a tile ID, or None if not a depth tile.
    pub fn depth_level(id: u8) -> Option<i32> {
        match id {
            NEG3 => Some(-3),
            NEG2 => Some(-2),
            NEG1 => Some(-1),
            POS1 => Some(1),
            POS2 => Some(2),
            POS3 => Some(3),
            POS4 => Some(4),
            POS5 => Some(5),
            _ => None,
        }
    }
}

// ── Data structures ────────────────────────────────────────────────────────

/// Per-corner height map for an elevation level.
///
/// Grid is `(map_width + 1) × (map_height + 1)` corners.
/// Corner `(cx, cy)` is the SW corner of tile `(cx, cy)`.
/// Tile `(tx, ty)` has corners:
///   SW = `(tx, ty)`, SE = `(tx+1, ty)`, NW = `(tx, ty+1)`, NE = `(tx+1, ty+1)`
pub struct CornerHeightMap {
    pub width: usize,
    pub height: usize,
    pub heights: Vec<f32>,
    /// Tiles marked as too steep to walk on (indexed by tile y * (width-1) + x).
    pub steep_tiles: Vec<bool>,
    /// Tiles that are slope/inherit plateaus (always walkable).
    pub plateau_tiles: Vec<bool>,
}

impl CornerHeightMap {
    pub fn new(map_w: usize, map_h: usize) -> Self {
        let width = map_w + 1;
        let height = map_h + 1;
        Self {
            width,
            height,
            heights: vec![0.0; width * height],
            steep_tiles: vec![false; map_w * map_h],
            plateau_tiles: vec![false; map_w * map_h],
        }
    }

    pub fn get(&self, cx: usize, cy: usize) -> f32 {
        self.heights[cy * self.width + cx]
    }

    pub fn set(&mut self, cx: usize, cy: usize, val: f32) {
        self.heights[cy * self.width + cx] = val;
    }

    /// Returns true if any corner has a non-zero height.
    pub fn has_slopes(&self) -> bool {
        self.heights.iter().any(|&h| h != 0.0)
    }
}

/// Per-level corner height maps.
#[derive(Resource, Default)]
pub struct SlopeHeightMaps {
    pub by_level: std::collections::HashMap<u8, CornerHeightMap>,
}

// ── Height propagation system ──────────────────────────────────────────────

/// Reads slope overlay layers and computes per-corner height maps.
/// Each depth tile explicitly sets height = `level * delta`.
/// No propagation needed — heights are direct from tile data.
pub fn compute_slope_height_maps(
    mut slope_maps: ResMut<SlopeHeightMaps>,
    config: Res<ElevationConfig>,
    slope_layers: Query<
        (&SlopeLayer, &TileStorage, &TilemapSize, &TilemapTileSize),
        (With<TerrainTypeMapReady>, With<TiledTilemap>,
         Added<SlopeLayer>),
    >,
    tile_indices: Query<&TileTextureIndex>,
) {
    for (slope_layer, storage, size, tile_size) in &slope_layers {
        let w = size.x as usize;
        let h = size.y as usize;
        let delta = tile_size.y * config.slope_angle_deg.to_radians().sin();

        let height_map = slope_maps.by_level
            .entry(slope_layer.level)
            .or_insert_with(|| CornerHeightMap::new(w, h));

        // For each tile, read the depth level and set all 4 corners.
        // Empty tiles stay at 0 (sea level).
        for y in 0..h {
            for x in 0..w {
                let pos = TilePos::new(x as u32, y as u32);
                let Some(tile_e) = storage.checked_get(&pos) else { continue };
                let Ok(tex_idx) = tile_indices.get(tile_e) else { continue };
                let id = (tex_idx.0 as u8).saturating_add(1);

                let Some(level) = slope_id::depth_level(id) else { continue };
                let tile_h = level as f32 * delta;

                height_map.set(x, y, tile_h);
                height_map.set(x + 1, y, tile_h);
                height_map.set(x, y + 1, tile_h);
                height_map.set(x + 1, y + 1, tile_h);

                // All depth tiles are plateaus (walkable surfaces)
                height_map.plateau_tiles[y * w + x] = true;
            }
        }

        let nonzero = height_map.heights.iter().filter(|&&h| h != 0.0).count();
        info!(
            "Slope height map for level {}: {}x{} grid, {} non-zero corners, delta={:.1}",
            slope_layer.level, w, h, nonzero, delta
        );
    }
}

/// Returns true if the tile ID is a depth tile (any explicit height level).
fn is_depth_tile(id: u8) -> bool {
    slope_id::depth_level(id).is_some()
}

// ── Public height sampling ────────────────────────────────────────────────

/// Sample the slope height offset at a world-space position for a given elevation level.
/// Returns 0.0 if no slope data exists for that level.
pub fn sample_slope_height(
    slope_maps: &SlopeHeightMaps,
    level: u8,
    world_x: f32,
    world_y: f32,
) -> f32 {
    let Some(hm) = slope_maps.by_level.get(&level) else { return 0.0 };
    let tile_size = super::DEFAULT_TILE_SIZE;

    let gx = world_x / tile_size;
    let gy = world_y / tile_size;

    let ix = (gx.floor() as usize).min(hm.width.saturating_sub(2));
    let iy = (gy.floor() as usize).min(hm.height.saturating_sub(2));

    let fx = (gx - ix as f32).clamp(0.0, 1.0);
    let fy = (gy - iy as f32).clamp(0.0, 1.0);

    let h00 = hm.get(ix, iy);
    let h10 = hm.get(ix + 1, iy);
    let h01 = hm.get(ix, iy + 1);
    let h11 = hm.get(ix + 1, iy + 1);

    let h_bot = h00 + (h10 - h00) * fx;
    let h_top = h01 + (h11 - h01) * fx;
    h_bot + (h_top - h_bot) * fy
}

/// Compute the full ground Z at a world position (base elevation + slope offset).
/// This is what gizmos/debug draws should use for Z placement.
pub fn ground_z(
    elev_heights: &crate::map::elevation::ElevationHeights,
    slope_maps: &SlopeHeightMaps,
    level: u8,
    world_x: f32,
    world_y: f32,
) -> f32 {
    let base = elev_heights.z_by_level.get(&level).copied().unwrap_or(-1.0);
    base + sample_slope_height(slope_maps, level, world_x, world_y)
}

// ── Steep slope colliders ──────────────────────────────────────────────────

#[derive(Component)]
pub struct SteepSlopeCollider;

#[derive(Component)]
pub struct SteepCollidersGenerated;

/// For each steep tile edge, compute the ground-plane width of the slope
/// from the height difference and slope angle, then place a collider of
/// that width straddling the edge. Adjacent colliders on the same axis
/// are merged to prevent seams.
pub fn generate_steep_colliders(
    mut commands: Commands,
    slope_maps: Res<SlopeHeightMaps>,
    config: Res<ElevationConfig>,
    mut generated: Local<bool>,
) {
    if *generated { return; }
    if slope_maps.by_level.is_empty() { return; }

    let tile = DEFAULT_TILE_SIZE;
    let half_delta = tile * config.slope_angle_deg.to_radians().sin() * 0.5;
    let max_step = half_delta + 0.01;
    let collider_width = tile;
    let mut count = 0u32;

    for (&_level, hm) in &slope_maps.by_level {
        let map_w = hm.width - 1;
        let map_h = hm.height - 1;

        let avg_h = |tx: usize, ty: usize| -> f32 {
            (hm.get(tx, ty) + hm.get(tx + 1, ty)
                + hm.get(tx, ty + 1) + hm.get(tx + 1, ty + 1)) * 0.25
        };

        // Step 1: Mark which tiles need colliders (non-plateau tiles adjacent
        // to plateau tiles with a steep height difference).
        let mut blocked = vec![false; map_w * map_h];

        for ty in 0..map_h {
            for tx in 0..map_w {
                if hm.plateau_tiles[ty * map_w + tx] { continue; } // never block plateaus
                let h = avg_h(tx, ty);

                // Check all 8 neighbors for a STEEP plateau (++/--) with steep diff.
                // Single +/- plateaus are walkable and don't generate colliders.
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        if dx == 0 && dy == 0 { continue; }
                        let nx = tx as i32 + dx;
                        let ny = ty as i32 + dy;
                        if nx < 0 || ny < 0 || nx as usize >= map_w || ny as usize >= map_h {
                            continue;
                        }
                        let ni = ny as usize * map_w + nx as usize;
                        // Only steep tiles (++/--) trigger blocking, not regular +/-/~
                        if !hm.steep_tiles[ni] { continue; }
                        let nh = avg_h(nx as usize, ny as usize);
                        if (h - nh).abs() >= max_step {
                            blocked[ty * map_w + tx] = true;
                        }
                    }
                }
            }
        }

        // Step 2: Greedy merge blocked tiles and spawn colliders
        let mut merged = vec![false; map_w * map_h];
        for ty in 0..map_h {
            for tx in 0..map_w {
                if !blocked[ty * map_w + tx] || merged[ty * map_w + tx] { continue; }

                // Expand right
                let mut ex = tx + 1;
                while ex < map_w && blocked[ty * map_w + ex] && !merged[ty * map_w + ex] {
                    ex += 1;
                }
                let rw = ex - tx;

                // Expand down
                let mut ey = ty + 1;
                'outer: while ey < map_h {
                    for x in tx..ex {
                        if !blocked[ey * map_w + x] || merged[ey * map_w + x] {
                            break 'outer;
                        }
                    }
                    ey += 1;
                }
                let rh = ey - ty;

                for y in ty..ey {
                    for x in tx..ex {
                        merged[y * map_w + x] = true;
                    }
                }

                // Compute Z from the highest adjacent plateau
                let mut max_neighbor_h = 0.0f32;
                for y in ty..ey {
                    for x in tx..ex {
                        let h = avg_h(x, y);
                        max_neighbor_h = max_neighbor_h.max(h);
                        for dy in -1i32..=1 {
                            for dx in -1i32..=1 {
                                let nx = x as i32 + dx;
                                let ny = y as i32 + dy;
                                if nx >= 0 && ny >= 0 && (nx as usize) < map_w && (ny as usize) < map_h {
                                    max_neighbor_h = max_neighbor_h.max(avg_h(nx as usize, ny as usize));
                                }
                            }
                        }
                    }
                }
                let cz = max_neighbor_h * 0.5;
                let c_height = max_neighbor_h + tile;

                let cx = (tx as f32 + rw as f32 * 0.5) * tile;
                let cy = (ty as f32 + rh as f32 * 0.5) * tile;
                commands.spawn((
                    RigidBody::Static,
                    Collider::cuboid(rw as f32 * tile, rh as f32 * tile, c_height),
                    Transform::from_xyz(cx, cy, cz),
                    SteepSlopeCollider,
                ));
                count += 1;
            }
        }
    }

    if count > 0 {
        info!("Generated {} steep slope colliders", count);
    }
    *generated = true;
}

/// Post-processing: despawn any SteepSlopeCollider whose center overlaps
/// a plateau tile (+/-/~/++/--). Plateau tiles are always walkable —
/// terrain-type colliders (water etc.) handle blocking on plateaus instead.
pub fn chop_plateau_colliders(
    mut commands: Commands,
    slope_maps: Res<SlopeHeightMaps>,
    colliders: Query<(Entity, &Transform, &Collider), With<SteepSlopeCollider>>,
    mut processed: Local<bool>,
) {
    if *processed { return; }
    if slope_maps.by_level.is_empty() { return; }
    if colliders.is_empty() { return; }

    let tile = DEFAULT_TILE_SIZE;
    let mut removed = 0u32;

    for (entity, tf, collider) in &colliders {
        let pos = tf.translation;

        // Get the collider's AABB in tile coordinates
        let aabb = collider.shape_scaled().compute_aabb(&avian3d::parry::math::Pose::identity());
        let min_tx = ((pos.x + aabb.mins.x) / tile).floor() as i32;
        let max_tx = ((pos.x + aabb.maxs.x) / tile).ceil() as i32;
        let min_ty = ((pos.y + aabb.mins.y) / tile).floor() as i32;
        let max_ty = ((pos.y + aabb.maxs.y) / tile).ceil() as i32;

        // Check if ANY tile covered by this collider is a plateau
        let mut overlaps_plateau = false;
        'check: for (&_level, hm) in &slope_maps.by_level {
            let map_w = hm.width - 1;
            let map_h = hm.height - 1;
            for ty in min_ty.max(0)..max_ty.min(map_h as i32) {
                for tx in min_tx.max(0)..max_tx.min(map_w as i32) {
                    if hm.plateau_tiles[ty as usize * map_w + tx as usize] {
                        overlaps_plateau = true;
                        break 'check;
                    }
                }
            }
        }

        if overlaps_plateau {
            commands.entity(entity).despawn();
            removed += 1;
        }
    }

    if removed > 0 {
        info!("Chopped {} colliders overlapping plateau tiles", removed);
    }
    *processed = true;
}

// ── Mesh-based slope colliders ─────────────────────────────────────────────

#[derive(Component)]
pub struct MeshSlopeCollider;

/// Analyze the terrain mesh triangles directly. For each tile, compute the
/// surface angle of its two triangles. If either exceeds the steepness
/// threshold, place a collider precisely where the slope begins — at the
/// edge between the steep tile and the lower adjacent tile, covering only
/// the lower tile's side up to the edge.
///
/// Height deviations are measured from the nearest multiple of tile_size
/// (0, ±48, ±96, etc.) so slopes between elevation levels work correctly.
pub fn generate_mesh_slope_colliders(
    mut commands: Commands,
    slope_maps: Res<SlopeHeightMaps>,
    config: Res<ElevationConfig>,
    mut generated: Local<bool>,
) {
    if *generated { return; }
    if slope_maps.by_level.is_empty() { return; }

    let tile = DEFAULT_TILE_SIZE;
    let max_angle_rad = config.slope_angle_deg.to_radians();
    let min_normal_z = max_angle_rad.cos();
    let mut count = 0u32;

    for (&_level, hm) in &slope_maps.by_level {
        let map_w = hm.width - 1;
        let map_h = hm.height - 1;

        // Step 1: Mark tiles with steep triangles and record their Z bounds.
        let mut blocked = vec![false; map_w * map_h];
        let mut z_min_grid = vec![0.0f32; map_w * map_h];
        let mut z_max_grid = vec![0.0f32; map_w * map_h];

        for ty in 0..map_h {
            for tx in 0..map_w {
                let h_sw = hm.get(tx, ty);
                let h_se = hm.get(tx + 1, ty);
                let h_nw = hm.get(tx, ty + 1);
                let h_ne = hm.get(tx + 1, ty + 1);

                // Skip truly flat tiles
                let all_same = (h_sw - h_se).abs() < 0.01
                    && (h_sw - h_nw).abs() < 0.01
                    && (h_sw - h_ne).abs() < 0.01;
                if all_same { continue; }

                let sw = Vec3::new(0.0, 0.0, h_sw);
                let se = Vec3::new(tile, 0.0, h_se);
                let nw = Vec3::new(0.0, tile, h_nw);
                let ne = Vec3::new(tile, tile, h_ne);

                let n1 = (se - sw).cross(ne - sw);
                let n2 = (ne - sw).cross(nw - sw);

                let steep1 = n1.length() > 0.001 && (n1.z / n1.length()).abs() < min_normal_z;
                let steep2 = n2.length() > 0.001 && (n2.z / n2.length()).abs() < min_normal_z;

                if !steep1 && !steep2 { continue; }

                let idx = ty * map_w + tx;
                blocked[idx] = true;
                z_min_grid[idx] = h_sw.min(h_se).min(h_nw).min(h_ne);
                z_max_grid[idx] = h_sw.max(h_se).max(h_nw).max(h_ne);
            }
        }

        // Step 2: Greedy merge blocked tiles into rectangles
        let mut merged = vec![false; map_w * map_h];
        for ty in 0..map_h {
            for tx in 0..map_w {
                if !blocked[ty * map_w + tx] || merged[ty * map_w + tx] { continue; }

                // Expand right
                let mut ex = tx + 1;
                while ex < map_w && blocked[ty * map_w + ex] && !merged[ty * map_w + ex] {
                    ex += 1;
                }
                let rw = ex - tx;

                // Expand down
                let mut ey = ty + 1;
                'outer: while ey < map_h {
                    for x in tx..ex {
                        if !blocked[ey * map_w + x] || merged[ey * map_w + x] {
                            break 'outer;
                        }
                    }
                    ey += 1;
                }
                let rh = ey - ty;

                // Mark merged
                let mut region_z_min = f32::MAX;
                let mut region_z_max = f32::MIN;
                for y in ty..ey {
                    for x in tx..ex {
                        let idx = y * map_w + x;
                        merged[idx] = true;
                        region_z_min = region_z_min.min(z_min_grid[idx]);
                        region_z_max = region_z_max.max(z_max_grid[idx]);
                    }
                }

                let cx = (tx as f32 + rw as f32 * 0.5) * tile;
                let cy = (ty as f32 + rh as f32 * 0.5) * tile;
                let cz = (region_z_min + region_z_max) * 0.5;
                let c_height = (region_z_max - region_z_min) + tile;

                commands.spawn((
                    RigidBody::Static,
                    Collider::cuboid(rw as f32 * tile, rh as f32 * tile, c_height),
                    Transform::from_xyz(cx, cy, cz),
                    MeshSlopeCollider,
                ));
                count += 1;
            }
        }
    }

    if count > 0 {
        info!("Generated {} mesh-based slope colliders", count);
    }
    *generated = true;
}
