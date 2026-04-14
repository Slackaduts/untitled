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
/// Tile index 0 = RIVER (ID 1), tile index 1 = GRASS (ID 2), etc.
fn terrain_surface_tile_to_id(texture_index: u32) -> u8 {
    // Tile indices in terrain_surfaces.tsx are 0-based.
    // Terrain IDs are 1-based (0 = empty).
    (texture_index as u8).saturating_add(1)
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

    /// Grass source texture for stochastic tiling.
    #[texture(2)]
    #[sampler(3)]
    pub grass: Handle<Image>,

    /// Dirt source texture for tiling.
    #[texture(4)]
    #[sampler(5)]
    pub dirt: Handle<Image>,

    /// Stone source texture for shallows.
    #[texture(6)]
    #[sampler(7)]
    pub stone: Handle<Image>,

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
    pub _pad1: Vec2,
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
            _pad1: Vec2::ZERO,
        }
    }
}

impl MaterialTilemap for TerrainMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/terrain_fill.wgsl".into()
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

        let mut image = Image::new(
            Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 },
            TextureDimension::D2,
            pixels,
            TextureFormat::Rgba8Unorm,
            default(),
        );
        image.sampler = ImageSampler::nearest();
        let terrain_map_handle = images.add(image);

        let grass_handle: Handle<Image> = asset_server.load("textures/grass.qoi");
        let dirt_handle: Handle<Image> = asset_server.load("textures/dirt.qoi");
        let stone_handle: Handle<Image> = asset_server.load("textures/stone.qoi");

        let material = materials.add(TerrainMaterial {
            terrain_map: terrain_map_handle,
            grass: grass_handle,
            dirt: dirt_handle,
            stone: stone_handle,
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
