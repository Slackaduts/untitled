use bevy::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, ShaderType,
    TextureDimension, TextureFormat,
};
use bevy::shader::ShaderRef;
use bevy_ecs_tilemap::prelude::*;
use bevy_ecs_tiled::prelude::TiledTilemap;

use super::CombatCamera3d;
use crate::billboard::properties::BillboardProperties;
use crate::map::DEFAULT_TILE_SIZE;

// ── Billboard material with per-pixel depth ─────────────────────────────

#[derive(ShaderType, Clone, Default)]
pub struct BillboardParams {
    pub depth_range: f32,
    pub _pad: Vec3,
}

#[derive(Asset, AsBindGroup, TypePath, Clone)]
pub struct BillboardMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub base_texture: Handle<Image>,
    #[uniform(2)]
    pub params: BillboardParams,
}

impl Material for BillboardMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/billboard.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        // Mask mode writes depth (unlike Blend which doesn't).
        // Low threshold so semi-transparent edges still render.
        AlphaMode::Mask(0.05)
    }

    fn specialize(
        _pipeline: &bevy::pbr::MaterialPipeline,
        descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
        _layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        _key: bevy::pbr::MaterialPipelineKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

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

/// Small Z offset for layer ordering within the same elevation level.
/// Prevents Z-fighting between overlapping billboards from different Tiled layers.
#[derive(Component)]
pub struct BillboardLayerOffset(pub f32);

// ── Map setup ──────────────────────────────────────────────────────────────

pub fn setup_billboard_tiles(
    mut commands: Commands,
    mut billboard_ready: ResMut<BillboardTilesReady>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut std_materials: ResMut<Assets<StandardMaterial>>,
    mut bb_materials: ResMut<Assets<BillboardMaterial>>,
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

                    // Apply ground blend to composite texture (after unpremultiply)
                    let blend_height = bb_props.map_or(0.0, |p| p.blend_height);
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

                    let comp_image = Image::new(
                        Extent3d { width: comp_w, height: trimmed_h, depth_or_array_layers: 1 },
                        TextureDimension::D2, comp_pixels,
                        TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default(),
                    );
                    let quad_mesh = create_billboard_quad(
                        quad_w, quad_h, origin_px_x, origin_px_y,
                    );

                    let mat = std_materials.add(StandardMaterial {
                        base_color_texture: Some(images.add(comp_image)),
                        unlit: true,
                        alpha_mode: AlphaMode::Mask(0.1),
                        double_sided: true, cull_mode: None, ..default()
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
                            });
                        }
                    }
                }
            }

            commands.entity(entity).insert(Visibility::Hidden);
            non_terrain_layer_idx += 1;
        }
    }

    billboard_ready.0 = true;
    info!("Billboard setup: non-terrain as greedy-merged 3D quads (terrain handled by elevation system)");
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

/// Create a billboard quad with a custom origin offset.
/// `offset_x`: how far right the origin is from the left edge of the quad.
/// `offset_y`: how far up the origin is from the bottom edge.
/// The mesh is positioned so local (0,0) is at the origin point.
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
pub const BILLBOARD_TILT_DEG: f32 = 50.0;

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
         Option<&BillboardLayerOffset>, Option<&BillboardProperties>),
        (With<Billboard>, Without<CombatCamera3d>),
    >,
    windows: Query<&Window>,
    slope_maps: Res<crate::map::slope::SlopeHeightMaps>,
    elev_heights: Res<crate::map::elevation::ElevationHeights>,
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

    for (mut tf, _bh, elev, layer_offset, bb_props) in &mut billboards {
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
        tf.translation.z = ground_z + z_offset + layer_z;

        // ── Tilt ──
        // Priority: per-billboard override > height-based > global default
        let base_tilt = if let Some(props) = bb_props.filter(|p| p.tilt_override >= 0.0) {
            props.tilt_override.to_radians()
        } else if let Some(bh) = _bh {
            // Height-based tilt: taller billboards stand more upright.
            // 1 tile high = default tilt, scales toward 75° for tall sprites.
            let tiles_tall = bh.height / DEFAULT_TILE_SIZE;
            let max_upright = 75.0_f32.to_radians();
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

        tf.rotation = Quat::from_rotation_x(tilt);
    }
}

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
