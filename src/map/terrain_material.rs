use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;
use bevy_ecs_tilemap::prelude::*;
use bevy_ecs_tiled::prelude::*;

use super::elevation::{parse_elevation_from_layer_name, LayerKind, SlopeLayer, TileElevation};

// ── Terrain state IDs ───────────────────────────────────────────────────────
//
// Each terrain type is a state. The terrain type map stores these as the R
// channel value (0-255). Transitions between states define the edge shader.
//
// ID 0 = empty (no terrain). IDs 1+ match tile indices in terrain_surfaces.tsx.

pub mod terrain_id {
    pub const EMPTY: u8 = 0;
    pub const RIVER: u8 = 1;    // tile 0
    pub const GRASS: u8 = 2;    // tile 1
    pub const SAND: u8 = 3;     // tile 2
    pub const DIRT: u8 = 4;     // tile 3
    pub const SNOW: u8 = 5;     // tile 4
    pub const LAVA: u8 = 6;     // tile 5
    pub const MUD: u8 = 7;      // tile 6
    pub const STONE: u8 = 8;    // tile 7
    pub const SHALLOWS: u8 = 9; // tile 8
    // Tiles 9-16 (IDs 10-17) are slope/depth tiles — see slope.rs
    pub const BSAND: u8 = 18;   // tile 17 — black volcanic sand
    pub const YGRASS: u8 = 19;  // tile 18 — yellow grass
    pub const BASALT: u8 = 20;  // tile 19 — basalt stone
}

/// Name of the dedicated terrain surfaces tileset.
const TERRAIN_TILESET: &str = "terrain_surfaces";

/// Maps a tileset name to a terrain state ID.
/// Returns `None` if this tileset doesn't represent a single terrain type.
/// Returns `Some(id)` for legacy tilesets (A1=water, A2=grass).
///
/// For the `terrain_surfaces` tileset, returns `None` — those are handled
/// per-tile via `TileTextureIndex`.
pub fn classify_tileset(name: &str) -> Option<u8> {
    if name == TERRAIN_TILESET {
        None // per-tile classification
    } else if name.contains("TileA1") {
        Some(terrain_id::RIVER)
    } else if name.contains("TileA2") {
        Some(terrain_id::GRASS)
    } else {
        None
    }
}

/// Maps a tile texture index from `terrain_surfaces` to a terrain state ID.
/// Tiles 0-8 and 9-16 map directly via index+1 (terrain + slope tiles).
/// Tiles on the second row (17+) use explicit mapping.
fn terrain_surface_tile_to_id(texture_index: u32) -> u8 {
    match texture_index {
        17 => terrain_id::BSAND,
        18 => terrain_id::YGRASS,
        19 => terrain_id::BASALT,
        // Tiles 0-16: terrain IDs are 1-based (index + 1)
        i => (i as u8).saturating_add(1),
    }
}

// ── Transition types ────────────────────────────────────────────────────────

pub mod transition_id {
    pub const ROCK_LEDGE: u32 = 1;
}

// ── Material ────────────────────────────────────────────────────────────────

#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
pub struct TerrainMaterial {
    /// Terrain type map: 1 pixel per map tile. R = terrain state ID.
    #[texture(0)]
    #[sampler(1)]
    pub terrain_map: Handle<Image>,

    /// Diffuse textures per terrain type.
    #[texture(2)]
    #[sampler(3)]
    pub grass: Handle<Image>,

    #[texture(4)]
    #[sampler(5)]
    pub dirt: Handle<Image>,

    #[texture(6)]
    #[sampler(7)]
    pub stone: Handle<Image>,

    #[texture(9)]
    #[sampler(10)]
    pub volcanic_sand: Handle<Image>,

    #[texture(11)]
    #[sampler(12)]
    pub grass_alt: Handle<Image>,

    #[texture(13)]
    #[sampler(14)]
    pub stone_alt: Handle<Image>,

    /// Normal maps per terrain type (tangent-space, RGB encoded).
    #[texture(15)]
    #[sampler(16)]
    pub grass_normal: Handle<Image>,

    #[texture(17)]
    #[sampler(18)]
    pub dirt_normal: Handle<Image>,

    #[texture(19)]
    #[sampler(20)]
    pub stone_normal: Handle<Image>,

    #[texture(21)]
    #[sampler(22)]
    pub volcanic_sand_normal: Handle<Image>,

    #[texture(23)]
    #[sampler(24)]
    pub grass_alt_normal: Handle<Image>,

    #[texture(25)]
    #[sampler(26)]
    pub stone_alt_normal: Handle<Image>,

    #[uniform(8)]
    pub params: TerrainParams,
}

#[derive(Clone, Debug, ShaderType)]
pub struct TerrainParams {
    pub water_deep: LinearRgba,
    pub water_mid: LinearRgba,
    pub water_surface: LinearRgba,
    pub water_highlight: LinearRgba,
    pub water_flow_dir: Vec2,
    pub water_flow_speed: f32,
    pub _pad0: f32,
    pub grass_scale: f32,
    pub stochastic_cell: f32,
    pub map_size: Vec2,
    pub transition_type: u32,
    pub ledge_half_width: f32,
    pub normal_strength: f32,
    pub sun_intensity: f32,
    /// xyz = toward-light direction (world space), w = unused.
    pub sun_direction: Vec4,
}

impl Default for TerrainParams {
    fn default() -> Self {
        Self {
            water_deep:      LinearRgba::new(0.01, 0.02, 0.10, 1.0),
            water_mid:       LinearRgba::new(0.02, 0.06, 0.22, 1.0),
            water_surface:   LinearRgba::new(0.04, 0.12, 0.35, 1.0),
            water_highlight: LinearRgba::new(0.12, 0.25, 0.50, 1.0),
            water_flow_dir:  Vec2::new(0.35, -0.06),
            water_flow_speed: 1.0,
            _pad0: 0.0,
            grass_scale: 0.02,
            stochastic_cell: 96.0,
            map_size: Vec2::new(30.0, 20.0),
            transition_type: transition_id::ROCK_LEDGE,
            ledge_half_width: 0.25,
            normal_strength: 0.6,
            sun_intensity: 1.0,
            sun_direction: Vec4::new(0.0, 0.0, 1.0, 0.0),
        }
    }
}

impl MaterialTilemap for TerrainMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/terrain_fill.wgsl".into()
    }
}

// ── Precomputation helpers ─────────────────────────────────────────────────

const DIRS: [(i32, i32); 8] = [
    (0, 1), (0, -1), (1, 0), (-1, 0),  // N, S, E, W
    (1, 1), (-1, 1), (1, -1), (-1, -1), // NE, NW, SE, SW
];

fn pixel_id(pixels: &[u8], x: i32, y: i32, w: usize, h: usize) -> u8 {
    if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
        terrain_id::EMPTY
    } else {
        pixels[(y as usize * w + x as usize) * 4]
    }
}

fn is_watery(id: u8) -> bool {
    id == terrain_id::RIVER || id == terrain_id::SHALLOWS
}

fn river_depth_at_tile(pixels: &[u8], tx: i32, ty: i32, w: usize, h: usize) -> f32 {
    let mut min_dist: f32 = 6.0;
    for dir_idx in 0..4 {
        let (dx, dy) = DIRS[dir_idx];
        let mut dist: f32 = 0.5;
        for i in 1..=6i32 {
            let neighbor = pixel_id(pixels, tx + dx * i, ty + dy * i, w, h);
            if neighbor == terrain_id::EMPTY || !is_watery(neighbor) {
                break;
            }
            dist += 1.0;
        }
        min_dist = min_dist.min(dist);
    }
    (min_dist / 4.0).clamp(0.0, 1.0)
}

fn precompute_terrain_channels(pixels: &mut [u8], w: usize, h: usize) {
    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) * 4;
            let here = pixels[idx];
            if here == terrain_id::EMPTY {
                pixels[idx + 3] = 0;
                continue;
            }

            // G: neighbor bitmask
            let mut mask = 0u8;
            for (bit, &(dx, dy)) in DIRS.iter().enumerate() {
                let neighbor = pixel_id(pixels, x as i32 + dx, y as i32 + dy, w, h);
                if neighbor != terrain_id::EMPTY && neighbor != here {
                    mask |= 1 << bit;
                }
            }
            pixels[idx + 1] = mask;

            // B: river depth (only for watery tiles)
            if is_watery(here) {
                let depth = river_depth_at_tile(pixels, x as i32, y as i32, w, h);
                pixels[idx + 2] = (depth * 255.0) as u8;
            } else {
                pixels[idx + 2] = 0;
            }

            // A: shallows stack info, or 255 for non-shallows
            if here == terrain_id::SHALLOWS {
                let mut top = y as i32;
                for i in 1..8i32 {
                    if pixel_id(pixels, x as i32, y as i32 + i, w, h) == terrain_id::SHALLOWS {
                        top = y as i32 + i;
                    } else {
                        break;
                    }
                }
                let mut bottom = y as i32;
                for i in 1..8i32 {
                    if pixel_id(pixels, x as i32, y as i32 - i, w, h) == terrain_id::SHALLOWS {
                        bottom = y as i32 - i;
                    } else {
                        break;
                    }
                }
                let stack_height = ((top - bottom + 1) as u8).min(15);
                let depth_index = ((top - y as i32) as u8).min(15);
                pixels[idx + 3] = (depth_index << 4) | stack_height;
            } else {
                pixels[idx + 3] = 255;
            }
        }
    }
}

// ── Terrain type map generation ─────────────────────────────────────────────

#[derive(Component)]
pub(crate) struct TerrainTypeMapReady;

pub fn build_terrain_and_attach_material(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    new_layers: Query<
        (Entity, &Name, &TileStorage, &TilemapSize, &ChildOf),
        (With<TiledTilemap>, Without<TerrainTypeMapReady>),
    >,
    tile_indices: Query<&TileTextureIndex>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<TerrainMaterial>>,
) {
    // Group layers by (parent, elevation_level). Each elevation gets its own
    // terrain type map so transitions are independent per elevation plane.
    let mut layer_groups: std::collections::HashMap<(Entity, u8), Vec<(Entity, &Name, &TileStorage, &TilemapSize)>> =
        std::collections::HashMap::new();

    for (entity, name, storage, size, parent) in &new_layers {
        if storage.size.x == 0 || storage.size.y == 0 {
            continue;
        }
        let has_tiles = (0..storage.size.x)
            .any(|x| (0..storage.size.y).any(|y| storage.checked_get(&TilePos::new(x, y)).is_some()));
        if !has_tiles {
            continue;
        }

        // Parse elevation level + kind from layer name
        let parsed = parse_elevation_from_layer_name(name.as_str());
        let (elev_level, kind) = parsed.unwrap_or((0, LayerKind::Terrain { height_tiles: 1 }));

        info!("Layer '{}' → level={}, kind={}", name.as_str(), elev_level,
            match &kind { LayerKind::Terrain { height_tiles } => format!("Terrain(h={})", height_tiles),
                          LayerKind::Slope => "Slope".to_string() });

        match kind {
            LayerKind::Slope => {
                commands.entity(entity).insert((
                    TileElevation { level: elev_level, height_tiles: 1 },
                    SlopeLayer { level: elev_level },
                    TerrainTypeMapReady,
                    Visibility::Hidden,
                ));
                continue;
            }
            LayerKind::Terrain { height_tiles } => {
                commands.entity(entity).insert(
                    TileElevation { level: elev_level, height_tiles },
                );
            }
        }

        layer_groups
            .entry((parent.parent(), elev_level))
            .or_default()
            .push((entity, name, storage, size));
    }

    for ((_parent, _elevation), siblings) in &layer_groups {
        let map_size = siblings[0].3;
        let w = map_size.x as usize;
        let h = map_size.y as usize;

        let mut pixels = vec![0u8; w * h * 4];

        for &(_, name, storage, _) in siblings {
            let name_str = name.as_str();

            let tileset_name = name_str
                .strip_prefix("TiledTilemap(")
                .and_then(|s| s.strip_suffix(')'))
                .and_then(|s| s.rsplit_once(", "))
                .map(|(_, ts)| ts)
                .unwrap_or(name_str);

            let is_terrain_surfaces = tileset_name == TERRAIN_TILESET;
            let uniform_id = classify_tileset(tileset_name);

            // Skip tilesets that aren't terrain
            if !is_terrain_surfaces && uniform_id.is_none() {
                continue;
            }

            for y in 0..h {
                for x in 0..w {
                    let pos = TilePos::new(x as u32, y as u32);
                    let Some(tile_entity) = storage.checked_get(&pos) else {
                        continue;
                    };

                    let state_id = if is_terrain_surfaces {
                        // Per-tile classification: tile texture index → terrain ID
                        if let Ok(tex_idx) = tile_indices.get(tile_entity) {
                            terrain_surface_tile_to_id(tex_idx.0)
                        } else {
                            continue;
                        }
                    } else {
                        // Uniform classification: entire tileset = one terrain type
                        uniform_id.unwrap()
                    };

                    let idx = (y * w + x) * 4;
                    pixels[idx] = state_id;
                    pixels[idx + 1] = 0;
                    pixels[idx + 2] = 0;
                    pixels[idx + 3] = 255;
                }
            }
        }

        // ── Second pass: precompute G/B/A channels ───────────────────────
        // G = neighbor-differs bitmask (8 bits: N,S,E,W,NE,NW,SE,SW)
        // B = river depth quantized to 0-255
        // A = shallows stack info (high nibble=depth_idx, low=stack_height), 255 otherwise
        //
        // Bit ordering for neighbor bitmask (matches shader expectations):
        //   0=N  1=S  2=E  3=W  4=NE  5=NW  6=SE  7=SW
        precompute_terrain_channels(&mut pixels, w, h);

        let mut image = Image::new(
            Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 },
            TextureDimension::D2,
            pixels,
            TextureFormat::Rgba8Unorm,
            default(),
        );
        image.sampler = ImageSampler::nearest();
        let terrain_map_handle = images.add(image);

        // Diffuse textures
        let grass_handle: Handle<Image> = asset_server.load("textures/grass/diffuse.qoi");
        let dirt_handle: Handle<Image> = asset_server.load("textures/dirt/diffuse.qoi");
        let stone_handle: Handle<Image> = asset_server.load("textures/stone/diffuse.qoi");
        let volcanic_sand_handle: Handle<Image> = asset_server.load("textures/volcanic_sand/diffuse.qoi");
        let grass_alt_handle: Handle<Image> = asset_server.load("textures/grass_alt/diffuse.qoi");
        let stone_alt_handle: Handle<Image> = asset_server.load("textures/stone_alt/diffuse.qoi");

        // Normal maps (tangent-space)
        let grass_normal: Handle<Image> = asset_server.load("textures/grass/normal.qoi");
        let dirt_normal: Handle<Image> = asset_server.load("textures/dirt/normal.qoi");
        let stone_normal: Handle<Image> = asset_server.load("textures/stone/normal.qoi");
        let volcanic_sand_normal: Handle<Image> = asset_server.load("textures/volcanic_sand/normal.qoi");
        let grass_alt_normal: Handle<Image> = asset_server.load("textures/grass_alt/normal.qoi");
        let stone_alt_normal: Handle<Image> = asset_server.load("textures/stone_alt/normal.qoi");

        let material = materials.add(TerrainMaterial {
            terrain_map: terrain_map_handle,
            grass: grass_handle,
            dirt: dirt_handle,
            stone: stone_handle,
            volcanic_sand: volcanic_sand_handle,
            grass_alt: grass_alt_handle,
            stone_alt: stone_alt_handle,
            grass_normal,
            dirt_normal,
            stone_normal,
            volcanic_sand_normal,
            grass_alt_normal,
            stone_alt_normal,
            params: TerrainParams {
                map_size: Vec2::new(w as f32, h as f32),
                ..default()
            },
        });

        for &(entity, name, _, _) in siblings {
            // Only apply terrain material to terrain-related tileset layers
            let name_str = name.as_str();
            let tileset_name = name_str
                .strip_prefix("TiledTilemap(")
                .and_then(|s| s.strip_suffix(')'))
                .and_then(|s| s.rsplit_once(", "))
                .map(|(_, ts)| ts)
                .unwrap_or(name_str);

            let is_terrain = tileset_name == TERRAIN_TILESET
                || classify_tileset(tileset_name).is_some();

            if is_terrain {
                commands
                    .entity(entity)
                    .remove::<MaterialTilemapHandle<StandardTilemapMaterial>>()
                    .insert(MaterialTilemapHandle(material.clone()));
            }

            commands.entity(entity).insert(TerrainTypeMapReady);
        }

        info!("Built terrain type map ({}x{}) and attached TerrainMaterial", w, h);
    }
}

/// Syncs the sun direction from the DirectionalLight into all TerrainMaterial
/// instances each frame so the terrain normal map responds to sun movement.
pub fn update_terrain_sun(
    sun: Query<(&DirectionalLight, &Transform), With<crate::lighting::components::SunLight>>,
    mut materials: ResMut<Assets<TerrainMaterial>>,
) {
    let Ok((sun_light, sun_tf)) = sun.single() else {
        return;
    };

    // DirectionalLight shines along -Z local; direction toward the light is +Z = back().
    let toward_light = sun_tf.back();
    let intensity = sun_light.illuminance / 8_000.0; // normalize against peak

    for (_, mat) in materials.iter_mut() {
        mat.params.sun_direction = Vec4::new(toward_light.x, toward_light.y, toward_light.z, 0.0);
        mat.params.sun_intensity = intensity.clamp(0.0, 1.0);
    }
}
