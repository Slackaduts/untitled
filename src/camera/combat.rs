use bevy::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowReceiver;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat,
};
use bevy_ecs_tilemap::prelude::*;
use bevy_ecs_tiled::prelude::TiledTilemap;

use super::CombatCamera3d;
use crate::billboard::properties::BillboardProperties;
use crate::map::DEFAULT_TILE_SIZE;

// ── Resources ──────────────────────────────────────────────────────────────

#[derive(Resource)]
pub struct CombatCamera {
    pub active: bool,
    pub target_center: Vec2,
    pub capture_size: Vec2,
    pub camera_height: f32,
    pub transition_speed: f32,
    pub target_tilt: f32,
    /// Time since combat activated (for delayed grid fade-in).
    pub activate_time: f32,
    /// Grid origin and size in tile coords (computed on activation).
    pub grid_origin: IVec2,
    pub grid_size: UVec2,
}

impl Default for CombatCamera {
    fn default() -> Self {
        Self {
            active: false,
            target_center: Vec2::ZERO,
            capture_size: Vec2::new(800.0, 600.0),
            camera_height: 800.0,
            transition_speed: 3.0,
            target_tilt: 0.5,
            activate_time: 0.0,
            grid_origin: IVec2::ZERO,
            grid_size: UVec2::ZERO,
        }
    }
}

#[derive(Resource, Default)]
pub struct BillboardTilesReady(pub bool);

// ── Components ─────────────────────────────────────────────────────────────

#[derive(Component)]
pub struct Billboard;

#[derive(Component)]
pub struct CombatTestEntity;

#[derive(Component)]
pub struct CombatGridVisual;

#[derive(Component)]
pub struct TerrainRenderCam;

#[derive(Component)]
pub struct FloorQuad;

#[derive(Component)]
pub struct BillboardTileQuad;

/// Stores the billboard quad's height in world units and its original Y position
/// so the billboard system can compensate for tilt displacement.
#[derive(Component)]
pub struct BillboardHeight {
    pub height: f32,
    pub base_y: f32,
}

/// Dedup key for billboard sprite export. Hash of the sorted tile indices
/// used to build this billboard's composite texture.
#[derive(Component)]
pub struct BillboardSpriteKey(pub String);

/// Small Z offset for layer ordering within the same elevation level.
/// Prevents Z-fighting between overlapping billboards from different Tiled layers.
#[derive(Component)]
pub struct BillboardLayerOffset(pub f32);

/// Cached billboard state to avoid recomputing slope Z and tilt every frame
/// when neither the billboard's XY position nor the camera has moved.
#[derive(Component, Default)]
pub struct BillboardCache {
    pub last_xy: Vec2,
    pub cached_z: f32,
    pub cached_rotation: Quat,
}

/// Resource tracking the main camera's last position for billboard cache invalidation.
#[derive(Resource, Default)]
pub struct BillboardCameraState {
    pub last_cam_translation: Vec3,
}

// ── Map setup ──────────────────────────────────────────────────────────────

pub fn setup_billboard_tiles(
    mut commands: Commands,
    mut billboard_ready: ResMut<BillboardTilesReady>,
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut bb_materials: ResMut<Assets<crate::particles::gpu_lights::ParticleLitMaterial>>,
    particle_buf: Res<crate::particles::gpu_lights::ParticleLightBuffer>,
    billboard_defs: Res<crate::billboard::properties::BillboardPropertyDefs>,
    tilemap_layers: Query<
        (Entity, &Name, &TileStorage, &TilemapSize, &TilemapTileSize,
         Option<&crate::map::elevation::TileElevation>),
        With<TiledTilemap>,
    >,
    tile_data: Query<(&TilePos, &TileTextureIndex)>,
    tilemap_textures: Query<&TilemapTexture>,
) {
    if billboard_ready.0 || tilemap_layers.is_empty() {
        return;
    }

    let (_, _, _, map_size, tile_size, _) = tilemap_layers.iter().next().unwrap();
    let map_w_px = map_size.x as f32 * tile_size.x;
    let map_h_px = map_size.y as f32 * tile_size.y;
    let map_center = Vec2::new(map_w_px * 0.5, map_h_px * 0.5);

    // Terrain rendering is handled by the elevation system (map/elevation.rs)
    // which creates per-elevation render cameras and 3D quads. No terrain
    // render target or floor quad needed here.

    // ── Process each tilemap layer ────────────────────────────────
    let mut non_terrain_layer_idx = 0u32;
    for (entity, name, storage, tilemap_size, ts, tile_elev) in &tilemap_layers {
        let name_str = name.as_str();

        if is_terrain_layer(name_str) {
            // Skip: elevation system already assigned terrain layers to
            // per-elevation render layers. Don't override with layer 1.
        } else {
            // Non-terrain: greedy-merge tiles into larger quads
            if let Ok(tilemap_tex) = tilemap_textures.get(entity) {
                let texture_handle = match tilemap_tex {
                    TilemapTexture::Single(h) => h.clone(),
                    TilemapTexture::Vector(v) if !v.is_empty() => v[0].clone(),
                    _ => continue,
                };

                // Wait for texture to be loaded before processing
                let Some(atlas_img) = images.get(&texture_handle) else {
                    // Not loaded yet — skip this frame, try again next frame
                    return;
                };
                let tex_w = atlas_img.texture_descriptor.size.width as f32;
                let _tex_h = atlas_img.texture_descriptor.size.height as f32;

                let cols = (tex_w / ts.x).round() as u32;
                let tile_w = ts.x as u32;
                let tile_h = ts.y as u32;

                // Build grid: which tiles are occupied (any index)
                let w = tilemap_size.x as usize;
                let h = tilemap_size.y as usize;
                let mut grid: Vec<Option<u32>> = vec![None; w * h];

                for y in 0..tilemap_size.y {
                    for x in 0..tilemap_size.x {
                        let pos = TilePos::new(x, y);
                        if let Some(tile_e) = storage.checked_get(&pos) {
                            if let Ok((_, tex_idx)) = tile_data.get(tile_e) {
                                grid[y as usize * w + x as usize] = Some(tex_idx.0);
                            }
                        }
                    }
                }

                // Atlas pixels for compositing
                let atlas_pixels = images.get(&texture_handle).unwrap().data.clone().unwrap_or_default();
                let atlas_w = images.get(&texture_handle).unwrap().texture_descriptor.size.width;
                let atlas_bpp = 4u32;

                // ── Border-transparency flood fill ──────────────────────
                // Two adjacent tiles merge if they share opaque pixel content
                // at their mutual border (content visually crosses the seam).
                // This naturally groups multi-tile objects while keeping
                // separate objects apart (transparent gap = no merge).
                let border_px = 3u32; // pixels from edge to check
                let alpha_thresh = 32u8;

                // Check if two horizontally adjacent tiles share opaque content
                // at their vertical border.
                let shares_h_border = |tx1: usize, ty: usize, tx2: usize| -> bool {
                    let tile1 = grid[ty * w + tx1];
                    let tile2 = grid[ty * w + tx2];
                    if tile1.is_none() || tile2.is_none() { return false; }
                    let idx1 = tile1.unwrap();
                    let idx2 = tile2.unwrap();
                    let ac1 = idx1 % cols;
                    let ar1 = idx1 / cols;
                    let ac2 = idx2 % cols;
                    let ar2 = idx2 / cols;

                    for py in 0..tile_h {
                        let mut right_opaque = false;
                        let mut left_opaque = false;
                        for dx in 0..border_px {
                            // Right edge of tile1
                            let sx = (ac1 * tile_w + tile_w - 1 - dx) as usize;
                            let sy = (ar1 * tile_h + py) as usize;
                            let off = (sy * atlas_w as usize + sx) * 4 + 3;
                            if off < atlas_pixels.len() && atlas_pixels[off] > alpha_thresh {
                                right_opaque = true;
                            }
                            // Left edge of tile2
                            let sx = (ac2 * tile_w + dx) as usize;
                            let sy = (ar2 * tile_h + py) as usize;
                            let off = (sy * atlas_w as usize + sx) * 4 + 3;
                            if off < atlas_pixels.len() && atlas_pixels[off] > alpha_thresh {
                                left_opaque = true;
                            }
                            if right_opaque && left_opaque { return true; }
                        }
                    }
                    false
                };

                // Check if two vertically adjacent tiles share opaque content
                // at their horizontal border.
                let shares_v_border = |tx: usize, ty1: usize, ty2: usize| -> bool {
                    let tile1 = grid[ty1 * w + tx];
                    let tile2 = grid[ty2 * w + tx];
                    if tile1.is_none() || tile2.is_none() { return false; }
                    let idx1 = tile1.unwrap();
                    let idx2 = tile2.unwrap();
                    let ac1 = idx1 % cols;
                    let ar1 = idx1 / cols;
                    let ac2 = idx2 % cols;
                    let ar2 = idx2 / cols;

                    for px in 0..tile_w {
                        let mut top1_opaque = false;
                        let mut bot2_opaque = false;
                        for dy in 0..border_px {
                            // Top edge of tile1 (lower tile in bevy) = low pixel rows in atlas
                            let sx = (ac1 * tile_w + px) as usize;
                            let sy = (ar1 * tile_h + dy) as usize;
                            let off = (sy * atlas_w as usize + sx) * 4 + 3;
                            if off < atlas_pixels.len() && atlas_pixels[off] > alpha_thresh {
                                top1_opaque = true;
                            }
                            // Bottom edge of tile2 (upper tile in bevy) = high pixel rows in atlas
                            let sx = (ac2 * tile_w + px) as usize;
                            let sy = (ar2 * tile_h + tile_h - 1 - dy) as usize;
                            let off = (sy * atlas_w as usize + sx) * 4 + 3;
                            if off < atlas_pixels.len() && atlas_pixels[off] > alpha_thresh {
                                bot2_opaque = true;
                            }
                            if top1_opaque && bot2_opaque { return true; }
                        }
                    }
                    false
                };

                let mut visited = vec![false; w * h];
                for start in 0..(w * h) {
                    if visited[start] || grid[start].is_none() {
                        continue;
                    }

                    let mut stack = vec![start];
                    let mut component = Vec::new();

                    while let Some(idx) = stack.pop() {
                        if visited[idx] || grid[idx].is_none() { continue; }
                        visited[idx] = true;
                        component.push(idx);
                        let cx = idx % w;
                        let cy = idx / w;

                        // Left
                        if cx > 0 && !visited[cy * w + cx - 1] && grid[cy * w + cx - 1].is_some() {
                            if shares_h_border(cx - 1, cy, cx) {
                                stack.push(cy * w + cx - 1);
                            }
                        }
                        // Right
                        if cx + 1 < w && !visited[cy * w + cx + 1] && grid[cy * w + cx + 1].is_some() {
                            if shares_h_border(cx, cy, cx + 1) {
                                stack.push(cy * w + cx + 1);
                            }
                        }
                        // Down (lower y in bevy)
                        if cy > 0 && !visited[(cy - 1) * w + cx] && grid[(cy - 1) * w + cx].is_some() {
                            if shares_v_border(cx, cy - 1, cy) {
                                stack.push((cy - 1) * w + cx);
                            }
                        }
                        // Up (higher y in bevy)
                        if cy + 1 < h && !visited[(cy + 1) * w + cx] && grid[(cy + 1) * w + cx].is_some() {
                            if shares_v_border(cx, cy, cy + 1) {
                                stack.push((cy + 1) * w + cx);
                            }
                        }
                    }

                    if component.is_empty() { continue; }

                    // Bounding box
                    let (mut min_x, mut max_x) = (w, 0usize);
                    let (mut min_y, mut max_y) = (h, 0usize);
                    for &idx in &component {
                        let cx = idx % w;
                        let cy = idx / w;
                        min_x = min_x.min(cx); max_x = max_x.max(cx);
                        min_y = min_y.min(cy); max_y = max_y.max(cy);
                    }
                    let rect_w = max_x - min_x + 1;
                    let rect_h = max_y - min_y + 1;

                    // ── Composite bounding box into one texture ───────
                    let comp_w = rect_w as u32 * tile_w;
                    let comp_h = rect_h as u32 * tile_h;
                    let mut comp_pixels = vec![0u8; (comp_w * comp_h * atlas_bpp) as usize];

                    for ty in 0..rect_h {
                        for tx in 0..rect_w {
                            let Some(tile_idx) = grid[(min_y + ty) * w + (min_x + tx)] else {
                                continue; // gap — stays transparent
                            };
                            let ac = tile_idx % cols;
                            let ar = tile_idx / cols;
                            let src_x0 = ac * tile_w;
                            let src_y0 = ar * tile_h;
                            let dst_x0 = tx as u32 * tile_w;
                            let dst_y0 = (rect_h as u32 - 1 - ty as u32) * tile_h;

                            for py in 0..tile_h {
                                let s = ((src_y0 + py) * atlas_w + src_x0) * atlas_bpp;
                                let d = ((dst_y0 + py) * comp_w + dst_x0) * atlas_bpp;
                                let ss = s as usize;
                                let se = ss + (tile_w * atlas_bpp) as usize;
                                let dd = d as usize;
                                if se <= atlas_pixels.len()
                                    && dd + (tile_w * atlas_bpp) as usize <= comp_pixels.len()
                                {
                                    comp_pixels[dd..dd + (tile_w * atlas_bpp) as usize]
                                        .copy_from_slice(&atlas_pixels[ss..se]);
                                }
                            }
                        }
                    }

                    // Look up billboard properties for the origin tile
                    // (bottom-center of the mosaic)
                    let tileset_name = name_str
                        .strip_prefix("TiledTilemap(")
                        .and_then(|s| s.strip_suffix(')'))
                        .and_then(|s| s.rsplit_once(", "))
                        .map(|(_, ts_name)| ts_name)
                        .unwrap_or("");
                    // Find the TSX filename for this tileset
                    let tsx_key = billboard_defs.by_tileset.keys()
                        .find(|k| k.contains(tileset_name))
                        .cloned();
                    // Origin tile = bottom-center of the mosaic
                    let origin_tile_x = min_x + rect_w / 2;
                    let origin_tile_y = min_y; // bottom row
                    let origin_tile_idx = grid[origin_tile_y * w + origin_tile_x];
                    let bb_props = tsx_key.as_ref()
                        .and_then(|k| billboard_defs.by_tileset.get(k))
                        .and_then(|tiles| origin_tile_idx.and_then(|idx| tiles.get(&idx)));

                    // ── Fix premultiplied alpha fringe ──
                    // Unpremultiply RGB BEFORE any alpha modifications (blend etc.)
                    for chunk in comp_pixels.chunks_exact_mut(4) {
                        let a = chunk[3] as f32 / 255.0;
                        if a > 0.01 && a < 0.99 {
                            chunk[0] = (chunk[0] as f32 / a).min(255.0) as u8;
                            chunk[1] = (chunk[1] as f32 / a).min(255.0) as u8;
                            chunk[2] = (chunk[2] as f32 / a).min(255.0) as u8;
                        }
                    }

                    // Apply ground blend to composite texture (after unpremultiply).
                    // Check properties.json first (F7 editor), fall back to TSX property.
                    let mut blend_height = bb_props.map_or(0.0, |p| p.blend_height);
                    {
                        let ts_name_for_props = name_str
                            .strip_prefix("TiledTilemap(")
                            .and_then(|s| s.strip_suffix(')'))
                            .and_then(|s| s.rsplit_once(", "))
                            .map(|(_, n)| n)
                            .unwrap_or(name_str);
                        // sprite_key isn't computed yet, but we can construct the same hash
                        let mut tile_indices_for_blend: Vec<u32> = component.iter()
                            .filter_map(|&idx| grid[idx].map(|t| t as u32))
                            .collect();
                        tile_indices_for_blend.sort();
                        tile_indices_for_blend.dedup();
                        let key_str_for_blend = tile_indices_for_blend.iter()
                            .map(|i| i.to_string())
                            .collect::<Vec<_>>()
                            .join("_");
                        use std::hash::{Hash, Hasher};
                        let mut hasher = std::collections::hash_map::DefaultHasher::new();
                        key_str_for_blend.hash(&mut hasher);
                        let sprite_key_for_blend = format!("{ts_name_for_props}_{:08x}", hasher.finish() as u32);
                        let props_path = format!("assets/objects/{ts_name_for_props}/{sprite_key_for_blend}/properties.json");
                        if let Ok(json_str) = std::fs::read_to_string(&props_path) {
                            if let Ok(props) = serde_json::from_str::<crate::billboard::object_types::ObjectProperties>(&json_str) {
                                if props.blend_height > 0.0 {
                                    blend_height = props.blend_height;
                                }
                            }
                        }
                    }
                    if blend_height > 0.0 {
                        let blend_rows = (blend_height as u32).min(comp_h);
                        for py in 0..blend_rows {
                            let alpha = py as f32 / blend_height;
                            for px in 0..comp_w {
                                let base = ((comp_h - 1 - py) * comp_w + px) as usize * 4;
                                if base + 3 < comp_pixels.len() {
                                    // Fade both alpha AND RGB to avoid bright fringe
                                    comp_pixels[base] = (comp_pixels[base] as f32 * alpha) as u8;
                                    comp_pixels[base + 1] = (comp_pixels[base + 1] as f32 * alpha) as u8;
                                    comp_pixels[base + 2] = (comp_pixels[base + 2] as f32 * alpha) as u8;
                                    comp_pixels[base + 3] = (comp_pixels[base + 3] as f32 * alpha) as u8;
                                }
                            }
                        }
                    }

                    // ── Trim empty rows from the bottom of the composite ──
                    // Scan from the bottom row upward to find the first row
                    // with any non-transparent pixel.
                    let mut trim_rows = 0u32;
                    'trim: for row in (0..comp_h).rev() {
                        for col in 0..comp_w {
                            let idx = (row * comp_w + col) as usize * 4 + 3; // alpha
                            if idx < comp_pixels.len() && comp_pixels[idx] > 0 {
                                break 'trim;
                            }
                        }
                        trim_rows += 1;
                    }

                    // Crop the pixel buffer and adjust dimensions
                    let trimmed_h = comp_h - trim_rows;
                    if trimmed_h == 0 { continue; } // entirely empty
                    if trim_rows > 0 {
                        comp_pixels.truncate((trimmed_h * comp_w * 4) as usize);
                    }

                    let quad_h = rect_h as f32 * ts.y - trim_rows as f32;
                    let quad_w = rect_w as f32 * ts.x;

                    // Anchor at center-bottom of the origin tile
                    let world_x = (min_x as f32 + rect_w as f32 * 0.5) * ts.x;
                    let world_y = (min_y as f32 + 0.5) * ts.y;

                    // Mesh offset: center X, bottom at anchor
                    let origin_px_x = quad_w * 0.5;
                    let origin_px_y = DEFAULT_TILE_SIZE * 0.5;

                    // Build dedup key from sorted tile indices
                    let sprite_key = {
                        let mut tile_indices: Vec<u32> = component.iter()
                            .filter_map(|&idx| grid[idx].map(|t| t as u32))
                            .collect();
                        tile_indices.sort();
                        tile_indices.dedup();
                        let key_str = tile_indices.iter()
                            .map(|i| i.to_string())
                            .collect::<Vec<_>>()
                            .join("_");
                        use std::hash::{Hash, Hasher};
                        let mut h = std::collections::hash_map::DefaultHasher::new();
                        key_str.hash(&mut h);
                        let ts_name = name_str
                            .strip_prefix("TiledTilemap(")
                            .and_then(|s| s.strip_suffix(')'))
                            .and_then(|s| s.rsplit_once(", "))
                            .map(|(_, n)| n)
                            .unwrap_or(name_str);
                        format!("{ts_name}_{:08x}", h.finish() as u32)
                    };

                    // Export billboard sprite to disk (debug builds only).
                    // Done here because Image data is dropped after GPU upload.
                    // Cleans up shadow/artifact pixels for better AI mesh generation
                    // without modifying the in-game billboard.
                    #[cfg(feature = "dev_tools")]
                    {
                        let ts_name = sprite_key.rsplit_once('_')
                            .map(|(name, _)| name)
                            .unwrap_or(&sprite_key);
                        // Each sprite gets its own subfolder: <tileset>/<sprite_key>/sprite.png
                        let dir = std::path::Path::new("assets/objects")
                            .join(ts_name)
                            .join(&sprite_key);
                        let path = dir.join("sprite.png");
                        if !path.exists() {
                            let _ = std::fs::create_dir_all(&dir);
                            let mut clean = comp_pixels.clone();
                            clean_sprite_for_export(&mut clean, comp_w, trimmed_h);
                            if let Some(img) = image::RgbaImage::from_raw(comp_w, trimmed_h, clean) {
                                match img.save(&path) {
                                    Ok(()) => info!("Exported: {}", path.display()),
                                    Err(e) => warn!("Failed to export sprite: {e}"),
                                }
                            }
                            // Save blend_height so the mesh pipeline can trim shadow bottoms
                            if blend_height > 0.0 {
                                let _ = std::fs::write(
                                    dir.join("blend_height.txt"),
                                    format!("{blend_height}"),
                                );
                            }
                        }
                    }

                    let comp_image = Image::new(
                        Extent3d { width: comp_w, height: trimmed_h, depth_or_array_layers: 1 },
                        TextureDimension::D2, comp_pixels,
                        TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default(),
                    );

                    let quad_mesh = create_billboard_quad(
                        quad_w, quad_h, origin_px_x, origin_px_y,
                    );

                    // Try to load normal and depth maps for this tileset
                    let tileset_name = name_str
                        .strip_prefix("TiledTilemap(")
                        .and_then(|s| s.strip_suffix(')'))
                        .and_then(|s| s.rsplit_once(", "))
                        .map(|(_, ts_name)| ts_name)
                        .unwrap_or("");
                    let normal_path = format!("tilesets/{}_normal.png", tileset_name);
                    // Check if normal map file exists
                    let has_normal = std::path::Path::new(&format!("assets/{}", normal_path)).exists();

                    // Composite normal and depth maps using the same tile layout
                    let normal_handle = if has_normal {
                        let normal_atlas: Handle<Image> = asset_server.load(&normal_path);
                        if let Some(normal_img) = images.get(&normal_atlas) {
                            let normal_data = normal_img.data.clone().unwrap_or_default();
                            let normal_w = normal_img.texture_descriptor.size.width;
                            let normal_bpp = 4u32; // RGBA
                            let normal_cols = (normal_w as f32 / ts.x).round() as u32;

                            // Composite normal map using same tile grid
                            let mut norm_pixels = vec![128u8; (comp_w * trimmed_h * 4) as usize];
                            // Set default flat normal (128, 128, 255, 255)
                            for chunk in norm_pixels.chunks_exact_mut(4) {
                                chunk[0] = 128; chunk[1] = 128; chunk[2] = 255; chunk[3] = 255;
                            }
                            for ty in 0..rect_h {
                                for tx in 0..rect_w {
                                    let Some(tile_idx) = grid[(min_y + ty) * w + (min_x + tx)] else { continue };
                                    let ac = tile_idx % normal_cols;
                                    let ar = tile_idx / normal_cols;
                                    let src_x0 = ac * tile_w;
                                    let src_y0 = ar * tile_h;
                                    let dst_y0 = (rect_h as u32 - 1 - ty as u32) * tile_h;
                                    // Only copy rows within trimmed height
                                    for py in 0..tile_h {
                                        let dst_row = dst_y0 + py;
                                        if dst_row >= trimmed_h { continue; }
                                        let s = ((src_y0 + py) * normal_w + src_x0) * normal_bpp;
                                        let d = (dst_row * comp_w + tx as u32 * tile_w) * 4;
                                        let ss = s as usize;
                                        let se = ss + (tile_w * normal_bpp) as usize;
                                        let dd = d as usize;
                                        let de = dd + (tile_w * 4) as usize;
                                        if se <= normal_data.len() && de <= norm_pixels.len() {
                                            // Normal maps are RGB, copy as RGBA
                                            for px in 0..tile_w as usize {
                                                let si = ss + px * normal_bpp as usize;
                                                let di = dd + px * 4;
                                                if si + 2 < normal_data.len() && di + 3 < norm_pixels.len() {
                                                    norm_pixels[di] = normal_data[si];
                                                    norm_pixels[di + 1] = normal_data[si + 1];
                                                    norm_pixels[di + 2] = normal_data[si + 2];
                                                    norm_pixels[di + 3] = 255;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            let norm_img = Image::new(
                                Extent3d { width: comp_w, height: trimmed_h, depth_or_array_layers: 1 },
                                TextureDimension::D2, norm_pixels,
                                TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default(),
                            );
                            images.add(norm_img)
                        } else {
                            // Normal atlas not loaded yet — use flat normal
                            create_flat_texture(&mut images, comp_w, trimmed_h, [128, 128, 255, 255])
                        }
                    } else {
                        create_flat_texture(&mut images, comp_w, trimmed_h, [128, 128, 255, 255])
                    };

                    let comp_tex_handle = images.add(comp_image);

                    let mat = bb_materials.add(bevy::pbr::ExtendedMaterial {
                        base: StandardMaterial {
                            base_color_texture: Some(comp_tex_handle.clone()),
                            alpha_mode: AlphaMode::Mask(0.5),
                            unlit: false,
                            perceptual_roughness: 1.0,
                            metallic: 0.0,
                            reflectance: 0.0,
                            double_sided: true,
                            cull_mode: None,
                            ..default()
                        },
                        extension: crate::particles::gpu_lights::ParticleLightExt {
                            particle_data: particle_buf.handle.clone(),
                        },
                    });

                    let bb_level = tile_elev.map_or(0, |e| e.level);
                    let layer_z_offset = non_terrain_layer_idx as f32 * 0.5;

                    let mut entity_cmds = commands.spawn((
                        Mesh3d(meshes.add(quad_mesh)),
                        MeshMaterial3d(mat),
                        Transform::from_xyz(world_x, world_y, 0.0),
                        Billboard,
                        BillboardTileQuad,
                        BillboardHeight { height: quad_h, base_y: world_y },
                        BillboardElevation { level: bb_level },
                        BillboardLayerOffset(layer_z_offset),
                        BillboardSpriteKey(sprite_key.clone()),
                        BillboardCache::default(),
                        // Billboard's own shader handles self-shadowing via
                        // depth-map tracing; receiving Bevy cast-shadows would
                        // darken the sprite under its own shadow mesh.
                        NotShadowReceiver,
                    ));

                    // Attach billboard properties component if customized
                    if let Some(props) = bb_props {
                        if !props.is_default() {
                            entity_cmds.insert(BillboardProperties {
                                origin: Vec2::new(props.origin_x, props.origin_y),
                                blend_height: props.blend_height,
                                tilt_override: props.tilt_override,
                                z_offset: props.z_offset,
                                collider_depth: props.collider_depth,
                                no_shadows: props.no_shadows,
                            });
                        }
                    }

                    // Bevy's 3D shadow mapping handles billboard shadows natively
                }
            }

            commands.entity(entity).insert(Visibility::Hidden);
            non_terrain_layer_idx += 1;
        }
    }

    billboard_ready.0 = true;
    info!("Billboard setup: non-terrain as greedy-merged 3D quads (terrain handled by elevation system)");
}


/// Marker for billboards that have had their lights spawned from properties.json.
#[derive(Component)]
pub struct ObjectLightsSpawned;

/// Spawns point lights from properties.json for billboard tile objects.
/// Runs after billboards are ready; the ObjectLightsSpawned marker prevents re-processing.
pub fn spawn_object_lights(
    mut commands: Commands,
    billboard_ready: Res<BillboardTilesReady>,
    billboards: Query<
        (Entity, &BillboardSpriteKey, &Transform, &BillboardHeight),
        (With<Billboard>, Without<ObjectLightsSpawned>),
    >,
) {
    if !billboard_ready.0 {
        return;
    }
    let bb_count = billboards.iter().count();
    if bb_count == 0 {
        return;
    }

    let mut light_count = 0;

    for (bb_entity, key, bb_tf, bb_height) in &billboards {
        let ts_name = key.0.rsplit_once('_')
            .map(|(name, _)| name)
            .unwrap_or(&key.0);
        let sprite_dir = format!("assets/objects/{ts_name}/{}", key.0);
        let props_path = format!("{sprite_dir}/properties.json");

        let obj_props: Option<crate::billboard::object_types::ObjectProperties> =
            std::fs::read_to_string(&props_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok());

        commands.entity(bb_entity).insert(ObjectLightsSpawned);

        let Some(props) = obj_props else { continue };
        if props.lights.is_empty() { continue; }

        for light_def in &props.lights {
            use crate::lighting::components::*;

            let shape = match light_def.shape.as_str() {
                "cone" => LightShape::Cone {
                    direction: 0.0,
                    angle: std::f32::consts::FRAC_PI_2,
                },
                "line" => LightShape::Line {
                    end_offset: Vec2::new(48.0, 0.0),
                },
                "capsule" => LightShape::Capsule {
                    direction: 0.0,
                    half_length: 24.0,
                },
                _ => LightShape::Point,
            };

            let bb_h = bb_height.height;
            let local = Vec3::new(
                (light_def.offset_x - 0.5) * bb_h,
                light_def.offset_y * bb_h,
                // Push forward past the depth-displaced shadow volume (max_depth
                // is up to bb_h * 0.5) so the point light doesn't self-occlude.
                bb_h * 0.55,
            );
            let light_pos = bb_tf.translation + bb_tf.rotation * local;

            commands.spawn((
                Transform::from_translation(light_pos),
                LightSource {
                    color: Color::linear_rgb(
                        light_def.color[0],
                        light_def.color[1],
                        light_def.color[2],
                    ),
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
                crate::billboard::object_types::ObjectSpriteLight {
                    sprite_key: key.0.clone(),
                    offset_x: light_def.offset_x,
                    offset_y: light_def.offset_y,
                },
            ));
            light_count += 1;
        }
    }

    if light_count > 0 {
        info!("Spawned {light_count} object lights");
    }
}

/// Clean up a billboard sprite for AI mesh generation export.
/// - Smoothly fades out dark semi-transparent shadow pixels
/// - Zeroes isolated near-transparent stray pixels (1px artifacts)
/// Does NOT modify the in-game billboard — only the exported PNG.
#[cfg(feature = "dev_tools")]
fn clean_sprite_for_export(pixels: &mut [u8], w: u32, h: u32) {
    let w = w as usize;
    let h = h as usize;

    // Pass 1: Smoothly fade dark semi-transparent pixels.
    // Shadow pixels are dark (low brightness) with low alpha.
    // Scale their alpha based on brightness — darker = more transparent.
    const ALPHA_CUTOFF: f32 = 100.0;   // only affect pixels with alpha below this
    const BRIGHT_THRESH: f32 = 60.0;   // pixels darker than this (avg RGB) get faded

    for chunk in pixels.chunks_exact_mut(4) {
        let a = chunk[3] as f32;
        if a < 1.0 || a >= ALPHA_CUTOFF {
            continue; // fully transparent or solid enough to keep
        }
        let brightness = (chunk[0] as f32 + chunk[1] as f32 + chunk[2] as f32) / 3.0;
        if brightness < BRIGHT_THRESH {
            // Fade alpha based on brightness: darker = more transparent
            // At brightness=0: alpha → 0. At brightness=BRIGHT_THRESH: alpha unchanged.
            let factor = (brightness / BRIGHT_THRESH).powi(2); // quadratic for smooth falloff
            chunk[3] = (a * factor) as u8;
        }
    }

    // Pass 2: Zero out isolated near-transparent stray pixels.
    // A pixel with alpha < 15 that has no neighbor with alpha > 64 is a stray artifact.
    const STRAY_THRESH: u8 = 15;
    const NEIGHBOR_THRESH: u8 = 64;

    let mut to_zero = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) * 4;
            if pixels[idx + 3] == 0 || pixels[idx + 3] >= STRAY_THRESH {
                continue;
            }
            // Check 8 neighbors for substantial content
            let mut has_neighbor = false;
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    if dx == 0 && dy == 0 { continue; }
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx < 0 || nx >= w as i32 || ny < 0 || ny >= h as i32 {
                        continue;
                    }
                    let ni = (ny as usize * w + nx as usize) * 4;
                    if pixels[ni + 3] >= NEIGHBOR_THRESH {
                        has_neighbor = true;
                        break;
                    }
                }
                if has_neighbor { break; }
            }
            if !has_neighbor {
                to_zero.push(idx);
            }
        }
    }
    for idx in to_zero {
        pixels[idx] = 0;
        pixels[idx + 1] = 0;
        pixels[idx + 2] = 0;
        pixels[idx + 3] = 0;
    }
}

/// Create a quad mesh with tiling UVs, pivoted at the BOTTOM edge.
/// Bottom at local y=0, extends to y=h. This way when the quad billboards
/// in combat mode, it "stands up" from the floor rather than rotating
/// through it.
fn create_tiled_uv_quad(
    w: f32, h: f32,
    u_min: f32, v_min: f32, u_max: f32, v_max: f32,
    tiles_x: f32, tiles_y: f32,
) -> Mesh {
    let hw = w * 0.5;

    let u_span = u_max - u_min;
    let v_span = v_max - v_min;

    // Pivot at center of bottom tile row (half a tile up from the bottom edge).
    let offset = DEFAULT_TILE_SIZE * 0.5;
    let vertices = vec![
        [-hw, -offset, 0.0],       // bottom-left
        [hw, -offset, 0.0],        // bottom-right
        [hw, h - offset, 0.0],     // top-right
        [-hw, h - offset, 0.0],    // top-left
    ];
    let normals = vec![[0.0, 0.0, 1.0]; 4];
    let uvs = vec![
        [u_min, v_min + v_span * tiles_y],                     // bottom-left
        [u_min + u_span * tiles_x, v_min + v_span * tiles_y],  // bottom-right
        [u_min + u_span * tiles_x, v_min],                     // top-right
        [u_min, v_min],                                        // top-left
    ];
    let indices = vec![0u32, 1, 2, 0, 2, 3];

    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vertices)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}

/// Create a tessellated billboard mesh with a custom origin offset.
///
/// The mesh is a flat quad tessellated into an NxN grid of vertices so the
/// shadow prepass vertex shader has enough sample points to reconstruct a
/// full 3D heightfield from the sprite's depth map. From the sun's view,
/// this extruded geometry casts a proper silhouette regardless of the sun's
/// angle relative to the billboard face — a flat quad would rasterize to
/// zero area when viewed edge-on, producing no shadow at all.
///
/// The main render pass uses StandardMaterial's default vertex shader which
/// does not displace vertices, so the main view still sees a flat sprite.
fn create_billboard_quad(w: f32, h: f32, offset_x: f32, offset_y: f32) -> Mesh {
    let left = -offset_x;
    let right = w - offset_x;
    let bottom = -offset_y;
    let top = h - offset_y;

    let vertices = vec![
        [left, bottom, 0.0],
        [right, bottom, 0.0],
        [right, top, 0.0],
        [left, top, 0.0],
    ];
    let normals = vec![[0.0, 0.0, 1.0]; 4];
    let uvs = vec![
        [0.0, 1.0],
        [1.0, 1.0],
        [1.0, 0.0],
        [0.0, 0.0],
    ];
    let indices = vec![0u32, 1, 2, 0, 2, 3];

    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vertices)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}

/// Create a small flat-color texture for use as a placeholder.
fn create_flat_texture(images: &mut Assets<Image>, w: u32, h: u32, color: [u8; 4]) -> Handle<Image> {
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    for chunk in pixels.chunks_exact_mut(4) {
        chunk.copy_from_slice(&color);
    }
    images.add(Image::new(
        Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        TextureDimension::D2, pixels,
        TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default(),
    ))
}

fn is_terrain_layer(name: &str) -> bool {
    if let Some(inner) = name
        .strip_prefix("TiledTilemap(")
        .and_then(|s| s.strip_suffix(')'))
    {
        if let Some((layer_name, _)) = inner.rsplit_once(", ") {
            // Match "terrain" (legacy) and "terrain_0", "terrain_1", etc.
            return layer_name == "terrain" || layer_name.starts_with("terrain_");
        }
    }
    name.contains("TileA1") || name.contains("TileA2")
}

// ── Combat camera ──────────────────────────────────────────────────────────

pub fn combat_camera_system(
    combat_camera: Res<CombatCamera>,
    mut cam_3d: Query<&mut Transform, With<CombatCamera3d>>,
    time: Res<Time>,
) {
    if !combat_camera.active { return; }
    let Some(mut tf) = cam_3d.iter_mut().next() else { return };

    let dt = time.delta_secs();
    let t = (combat_camera.transition_speed * dt).min(1.0);

    let target_pos = Vec3::new(
        combat_camera.target_center.x,
        combat_camera.target_center.y - combat_camera.camera_height * 0.9,
        combat_camera.camera_height,
    );

    tf.translation = tf.translation.lerp(target_pos, t);

    // Pure pitch rotation only — no yaw or roll ever.
    // Compute the pitch angle from the camera's offset to the look target.
    let dy = combat_camera.target_center.y - tf.translation.y;
    let dz = tf.translation.z;
    let pitch = dy.atan2(dz); // angle from straight-down toward the target
    let target_rot = Quat::from_rotation_x(pitch);
    tf.rotation = tf.rotation.slerp(target_rot, t);
}

/// Billboard tilt angle from the ground plane, in degrees.
/// 90° = fully upright, 0° = flat on ground.
/// At our ~42° camera angle, 65° gives a natural look: the billboard appears
/// to stand up while the sprite is only viewed ~23° off face-on.
pub const BILLBOARD_TILT_DEG: f32 = 35.0;

/// Y-axis scale factor for colliders on billboard entities.
/// Matches the visual foreshortening so the collision footprint agrees with
/// what the player sees on screen.
///
/// Derived from: `cos(camera_tilt - (90° - billboard_tilt))`
///   = cos(42° - 25°) = cos(17°) ≈ 0.956
///
/// Avian2D ignores 3D X-rotation, so this must be applied at spawn time:
///   `Collider::rectangle(w, h * BILLBOARD_COLLIDER_Y_SCALE)`
pub const BILLBOARD_COLLIDER_Y_SCALE: f32 = 0.956;

pub fn billboard_system(
    camera_q: Query<(&Camera, &GlobalTransform), With<CombatCamera3d>>,
    mut billboards: Query<
        (&mut Transform, Option<&BillboardHeight>, Option<&BillboardElevation>,
         Option<&BillboardLayerOffset>, Option<&BillboardProperties>,
         Option<&mut BillboardCache>),
        (With<Billboard>, Without<CombatCamera3d>),
    >,
    windows: Query<&Window>,
    slope_maps: Res<crate::map::slope::SlopeHeightMaps>,
    elev_heights: Res<crate::map::elevation::ElevationHeights>,
    mut cam_state: ResMut<BillboardCameraState>,
) {
    let Some((camera, cam_gt)) = camera_q.iter().next() else { return };
    let window_height = windows.iter().next()
        .map(|w| w.height())
        .unwrap_or(1080.0);

    let default_tilt = BILLBOARD_TILT_DEG.to_radians();
    let camera_tilt = super::follow::OVERWORLD_TILT_OFFSET.atan();
    let min_tilt = std::f32::consts::FRAC_PI_2 - camera_tilt;

    let fade_start_y = window_height * 0.7;
    let fade_range = window_height * 0.6;

    // Check if camera has moved since last frame
    let cam_translation = cam_gt.translation();
    let camera_moved = cam_translation != cam_state.last_cam_translation;
    cam_state.last_cam_translation = cam_translation;

    for (mut tf, _bh, elev, layer_offset, bb_props, cache) in &mut billboards {
        let xy = Vec2::new(tf.translation.x, tf.translation.y);

        // If neither the billboard nor the camera moved, use cached values
        if let Some(ref cache) = cache {
            let billboard_moved = xy != cache.last_xy;
            if !billboard_moved && !camera_moved && cache.last_xy != Vec2::ZERO {
                tf.translation.z = cache.cached_z;
                tf.rotation = cache.cached_rotation;
                continue;
            }
        }

        // ── Slope Z adjustment ──
        let level = elev.map_or(0, |e| e.level);
        let base_z = elev_heights.z_by_level.get(&level).copied().unwrap_or(-1.0);
        let slope_z = sample_slope_height(&slope_maps, level, tf.translation.x, tf.translation.y);
        let ground_z = base_z + slope_z;

        // Z offset: billboard properties override, or default based on type
        let extra_z = bb_props.map_or(0.0, |p| p.z_offset);
        let z_offset = if _bh.is_some() {
            let tilt_rad = BILLBOARD_TILT_DEG.to_radians();
            DEFAULT_TILE_SIZE * 0.5 * tilt_rad.sin() + extra_z
        } else {
            16.0
        };
        let layer_z = layer_offset.map_or(0.0, |o| o.0);
        let final_z = ground_z + z_offset + layer_z;
        tf.translation.z = final_z;

        // ── Tilt ──
        // Priority: per-billboard override > height-based > global default
        let base_tilt = if let Some(props) = bb_props.filter(|p| p.tilt_override >= 0.0) {
            props.tilt_override.to_radians()
        } else if let Some(bh) = _bh {
            // Height-based tilt: taller billboards stand more upright.
            // 1 tile high = default tilt, scales toward 75° for tall sprites.
            let tiles_tall = bh.height / DEFAULT_TILE_SIZE;
            let max_upright = 55.0_f32.to_radians();
            let t = ((tiles_tall - 1.0) / 4.0).clamp(0.0, 1.0); // 1-5 tiles range
            default_tilt + (max_upright - default_tilt) * t
        } else {
            default_tilt
        };

        let screen_y = camera
            .world_to_viewport(cam_gt, tf.translation)
            .map_or(window_height + fade_range, |vp| vp.y);

        let tilt = if screen_y <= fade_start_y {
            base_tilt
        } else {
            let t = ((screen_y - fade_start_y) / fade_range).clamp(0.0, 1.0);
            let t = t * t * (3.0 - 2.0 * t);
            base_tilt + (min_tilt - base_tilt) * t
        };

        let rotation = Quat::from_rotation_x(tilt);
        tf.rotation = rotation;

        // Update cache if present
        if let Some(mut cache) = cache {
            cache.last_xy = xy;
            cache.cached_z = final_z;
            cache.cached_rotation = rotation;
        }
    }
}

// Billboard lighting and shadows are now handled by Bevy's built-in 3D lighting pipeline.

/// Component that tracks which elevation level a billboard belongs to.
#[derive(Component)]
pub struct BillboardElevation {
    pub level: u8,
}

/// Sample the slope height map at a world XY position using bilinear interpolation.
fn sample_slope_height(
    slope_maps: &crate::map::slope::SlopeHeightMaps,
    level: u8,
    world_x: f32,
    world_y: f32,
) -> f32 {
    let Some(hm) = slope_maps.by_level.get(&level) else { return 0.0 };
    let tile_size = 48.0; // DEFAULT_TILE_SIZE

    // Convert world position to corner-grid coordinates
    let gx = world_x / tile_size;
    let gy = world_y / tile_size;

    let ix = (gx.floor() as usize).min(hm.width.saturating_sub(2));
    let iy = (gy.floor() as usize).min(hm.height.saturating_sub(2));

    let fx = (gx - ix as f32).clamp(0.0, 1.0);
    let fy = (gy - iy as f32).clamp(0.0, 1.0);

    // Bilinear interpolation of the 4 surrounding corners
    let h00 = hm.get(ix, iy);
    let h10 = hm.get(ix + 1, iy);
    let h01 = hm.get(ix, iy + 1);
    let h11 = hm.get(ix + 1, iy + 1);

    let h_bot = h00 + (h10 - h00) * fx;
    let h_top = h01 + (h11 - h01) * fx;
    h_bot + (h_top - h_bot) * fy
}

// ── Combat grid ────────────────────────────────────────────────────────────

use crate::map::terrain_material::{classify_tileset, terrain_id};

/// Compute the largest walkable rectangle centered on the actors.
/// Expands outward from the actor AABB until hitting map edge or impassable terrain.
pub fn compute_combat_grid(
    actor_positions: &[Vec2],
    tilemap_layers: &Query<
        (Entity, &Name, &TileStorage, &TilemapSize, &TilemapTileSize),
        With<TiledTilemap>,
    >,
    tile_data: &Query<(&TilePos, &TileTextureIndex)>,
) -> (IVec2, UVec2) {
    let tile = DEFAULT_TILE_SIZE;

    // Actor bounding box in tile coords
    let mut min_t = IVec2::new(i32::MAX, i32::MAX);
    let mut max_t = IVec2::new(i32::MIN, i32::MIN);
    for &pos in actor_positions {
        let tx = (pos.x / tile).floor() as i32;
        let ty = (pos.y / tile).floor() as i32;
        min_t = min_t.min(IVec2::new(tx, ty));
        max_t = max_t.max(IVec2::new(tx, ty));
    }
    // Pad by 1 tile
    min_t -= IVec2::ONE;
    max_t += IVec2::ONE;

    // Build walkability grid from terrain layers
    let mut map_w = 0u32;
    let mut map_h = 0u32;
    let mut walkable: Vec<bool> = Vec::new();

    for (entity, name, storage, tilemap_size, _ts) in tilemap_layers.iter() {
        let name_str = name.as_str();
        if !is_terrain_layer(name_str) {
            continue;
        }

        map_w = tilemap_size.x;
        map_h = tilemap_size.y;
        walkable.resize((map_w * map_h) as usize, true);

        // Parse tileset name to classify tiles
        let tileset_name = name_str
            .strip_prefix("TiledTilemap(")
            .and_then(|s| s.strip_suffix(')'))
            .and_then(|s| s.rsplit_once(", "))
            .map(|(_, ts)| ts)
            .unwrap_or(name_str);

        let is_terrain_surfaces = tileset_name == "terrain_surfaces";
        let uniform_id = classify_tileset(tileset_name);

        for y in 0..map_h {
            for x in 0..map_w {
                let pos = TilePos::new(x, y);
                let Some(tile_entity) = storage.checked_get(&pos) else { continue };

                let state_id = if is_terrain_surfaces {
                    if let Ok((_, tex_idx)) = tile_data.get(tile_entity) {
                        (tex_idx.0 as u8).saturating_add(1)
                    } else {
                        continue;
                    }
                } else if let Some(id) = uniform_id {
                    id
                } else {
                    continue;
                };

                // Mark impassable terrain
                if state_id == terrain_id::RIVER || state_id == terrain_id::SHALLOWS {
                    walkable[(y * map_w + x) as usize] = false;
                }
            }
        }
    }

    if walkable.is_empty() {
        return (min_t, (max_t - min_t + IVec2::ONE).as_uvec2());
    }

    // Expand the rectangle outward from actor AABB, stopping at map edge or obstacle
    // Expand each side independently
    let is_walkable_col = |x: i32, y_min: i32, y_max: i32| -> bool {
        if x < 0 || x >= map_w as i32 { return false; }
        for y in y_min..=y_max {
            if y < 0 || y >= map_h as i32 { return false; }
            if !walkable[(y as u32 * map_w + x as u32) as usize] { return false; }
        }
        true
    };
    let is_walkable_row = |y: i32, x_min: i32, x_max: i32| -> bool {
        if y < 0 || y >= map_h as i32 { return false; }
        for x in x_min..=x_max {
            if x < 0 || x >= map_w as i32 { return false; }
            if !walkable[(y as u32 * map_w + x as u32) as usize] { return false; }
        }
        true
    };

    // Expand left
    while min_t.x > 0 && is_walkable_col(min_t.x - 1, min_t.y, max_t.y) {
        min_t.x -= 1;
    }
    // Expand right
    while max_t.x < map_w as i32 - 1 && is_walkable_col(max_t.x + 1, min_t.y, max_t.y) {
        max_t.x += 1;
    }
    // Expand down (lower y)
    while min_t.y > 0 && is_walkable_row(min_t.y - 1, min_t.x, max_t.x) {
        min_t.y -= 1;
    }
    // Expand up (higher y)
    while max_t.y < map_h as i32 - 1 && is_walkable_row(max_t.y + 1, min_t.x, max_t.x) {
        max_t.y += 1;
    }

    // Clamp to map bounds
    min_t = min_t.max(IVec2::ZERO);
    max_t = max_t.min(IVec2::new(map_w as i32 - 1, map_h as i32 - 1));

    let size = (max_t - min_t + IVec2::ONE).as_uvec2();
    (min_t, size)
}

/// Spawn per-tile grid squares on the terrain, adjusted for slope elevation.
/// Each square floats at the maximum corner height of its tile (never intersects slopes).
pub fn spawn_combat_grid(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    origin: IVec2,
    size: UVec2,
    alpha: f32,
    slope_maps: &crate::map::slope::SlopeHeightMaps,
    elev_heights: &crate::map::elevation::ElevationHeights,
    slope_angle: f32,
) {
    let tile = DEFAULT_TILE_SIZE;
    let line_thickness = 1.5;
    let base_z = elev_heights.z_by_level.get(&0).copied().unwrap_or(-1.0);

    // Find the height range across the grid for normalization
    let mut min_h = 0.0f32;
    let mut max_h = 0.0f32;
    if let Some(hm) = slope_maps.by_level.get(&0) {
        for ty in 0..size.y {
            for tx in 0..size.x {
                let ux = (origin.x + tx as i32) as usize;
                let uy = (origin.y + ty as i32) as usize;
                if ux + 1 < hm.width && uy + 1 < hm.height {
                    let h = hm.get(ux, uy).max(hm.get(ux+1, uy))
                        .max(hm.get(ux, uy+1)).max(hm.get(ux+1, uy+1));
                    min_h = min_h.min(h);
                    max_h = max_h.max(h);
                }
            }
        }
    }
    // Ensure range is at least 1 delta to avoid division by zero
    let range = (max_h - min_h).max(1.0);

    // Cache shared meshes
    let h_mesh = meshes.add(Rectangle::new(tile, line_thickness));
    let v_mesh = meshes.add(Rectangle::new(line_thickness, tile));

    for ty in 0..size.y {
        for tx in 0..size.x {
            let gx = origin.x + tx as i32;
            let gy = origin.y + ty as i32;

            let tile_z = if let Some(hm) = slope_maps.by_level.get(&0) {
                let ux = gx as usize;
                let uy = gy as usize;
                if ux + 1 < hm.width && uy + 1 < hm.height {
                    hm.get(ux, uy).max(hm.get(ux+1, uy))
                        .max(hm.get(ux, uy+1)).max(hm.get(ux+1, uy+1))
                } else { 0.0 }
            } else { 0.0 };

            // Map height to depth level, then to tile colors matching the tileset.
            // Sea level = white, negative = reds/oranges, positive = greens/blues.
            let delta = tile * slope_angle.to_radians().sin();
            let level = if delta > 0.001 { (tile_z / delta).round() as i32 } else { 0 };
            let color = match level {
                i if i <= -3 => Color::srgb(220.0/255.0, 40.0/255.0, 40.0/255.0),   // -3 red
                -2           => Color::srgb(240.0/255.0, 100.0/255.0, 30.0/255.0),   // -2 orange
                -1           => Color::srgb(250.0/255.0, 150.0/255.0, 50.0/255.0),   // -1 yellow-orange
                0            => Color::srgb(1.0, 1.0, 1.0),                           // sea level white
                1            => Color::srgb(40.0/255.0, 220.0/255.0, 80.0/255.0),     // 1 green
                2            => Color::srgb(40.0/255.0, 240.0/255.0, 140.0/255.0),    // 2 teal
                3            => Color::srgb(40.0/255.0, 180.0/255.0, 240.0/255.0),    // 3 light blue
                4            => Color::srgb(60.0/255.0, 120.0/255.0, 250.0/255.0),    // 4 blue
                i if i >= 5  => Color::srgb(100.0/255.0, 60.0/255.0, 250.0/255.0),   // 5+ purple
                _            => Color::srgb(1.0, 1.0, 1.0),
            }.with_alpha(0.75 * alpha);
            let mat = materials.add(StandardMaterial {
                base_color: color,
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                double_sided: true,
                cull_mode: None,
                ..default()
            });

            let z = base_z + tile_z + 0.5;
            let cx = (gx as f32 + 0.5) * tile;
            let cy = (gy as f32 + 0.5) * tile;
            let inner = tile - line_thickness;

            // Hollow square: 4 edges
            let edges: [(Handle<Mesh>, f32, f32); 4] = [
                (h_mesh.clone(), cx, cy + inner * 0.5),
                (h_mesh.clone(), cx, cy - inner * 0.5),
                (v_mesh.clone(), cx - inner * 0.5, cy),
                (v_mesh.clone(), cx + inner * 0.5, cy),
            ];
            for (mesh, px, py) in edges {
                commands.spawn((
                    Mesh3d(mesh),
                    MeshMaterial3d(mat.clone()),
                    Transform::from_xyz(px, py, z),
                    CombatGridVisual,
                ));
            }
        }
    }
}

/// Updates grid line alpha for fade-in effect after camera transition.
pub fn combat_grid_fade(
    combat_camera: Res<CombatCamera>,
    grid_visuals: Query<&MeshMaterial3d<StandardMaterial>, With<CombatGridVisual>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    time: Res<Time>,
    mut elapsed: Local<f32>,
) {
    if !combat_camera.active {
        *elapsed = 0.0;
        return;
    }

    *elapsed += time.delta_secs();

    // Delay: wait 0.6s for camera transition, then fade in over 0.4s
    let fade_delay = 0.6;
    let fade_duration = 0.4;
    let alpha = ((*elapsed - fade_delay) / fade_duration).clamp(0.0, 1.0);

    for mat_handle in &grid_visuals {
        if let Some(mat) = materials.get_mut(&mat_handle.0) {
            // Preserve per-tile RGB brightness, only animate the alpha fade-in
            let Srgba { red, green, blue, .. } = mat.base_color.to_srgba();
            mat.base_color = Color::srgba(red, green, blue, 0.75 * alpha);
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

pub fn compute_combat_view(positions: &[Vec2]) -> (Vec2, Vec2) {
    if positions.is_empty() {
        return (Vec2::ZERO, Vec2::new(800.0, 600.0));
    }

    let mut min = positions[0];
    let mut max = positions[0];
    for &p in positions.iter().skip(1) {
        min = min.min(p);
        max = max.max(p);
    }

    let padding = DEFAULT_TILE_SIZE * 3.0;
    min -= Vec2::splat(padding);
    max += Vec2::splat(padding);

    ((min + max) * 0.5, (max - min).max(Vec2::splat(400.0)))
}

pub fn spawn_grid_visuals(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    origin: Vec2,
    width: u32,
    height: u32,
) {
    let tile = DEFAULT_TILE_SIZE;
    let line_thickness = 1.5;
    let color = Color::srgba(1.0, 1.0, 1.0, 0.2);
    let mat = materials.add(ColorMaterial::from(color));

    let total_w = width as f32 * tile;
    let total_h = height as f32 * tile;

    for x in 0..=width {
        let px = origin.x + x as f32 * tile;
        let cy = origin.y + total_h * 0.5;
        commands.spawn((
            Mesh2d(meshes.add(Rectangle::new(line_thickness, total_h))),
            MeshMaterial2d(mat.clone()),
            Transform::from_xyz(px, cy, 5.0),
            CombatGridVisual,
        ));
    }

    for y in 0..=height {
        let py = origin.y + y as f32 * tile;
        let cx = origin.x + total_w * 0.5;
        commands.spawn((
            Mesh2d(meshes.add(Rectangle::new(total_w, line_thickness))),
            MeshMaterial2d(mat.clone()),
            Transform::from_xyz(cx, py, 5.0),
            CombatGridVisual,
        ));
    }
}
