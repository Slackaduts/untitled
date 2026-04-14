use bevy::prelude::*;
use bevy::render::camera::RenderTarget;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::render::view::RenderLayers;
use bevy_ecs_tilemap::prelude::*;
use bevy_ecs_tiled::prelude::*;

use super::terrain_material::TerrainTypeMapReady;

#[derive(Resource)]
pub struct ElevationConfig {
    pub height_per_level: f32,
    /// Slope angle in degrees. The height change per slope tile is
    /// `tile_size * sin(slope_angle)`. At 45° with 48px tiles this is ~34 units.
    pub slope_angle_deg: f32,
}

impl Default for ElevationConfig {
    fn default() -> Self {
        Self {
            height_per_level: 48.0,
            slope_angle_deg: 45.0,
        }
    }
}

#[derive(Component)]
pub struct TileElevation {
    pub level: u8,
    /// Custom height in tile units. Default 1 per level.
    pub height_tiles: u8,
}

#[derive(Component)]
pub struct ElevationMeshReady;

/// Marker for `terrain_X_slopes` layers that define slope height deltas.
#[derive(Component)]
pub struct SlopeLayer {
    pub level: u8,
}

#[derive(Component)]
pub struct ElevationQuad {
    pub level: u8,
}

#[derive(Component)]
pub struct ElevationRenderCam {
    pub level: u8,
}

#[derive(Resource, Default)]
pub struct ElevationMaterials {
    pub by_level: std::collections::HashMap<u8, Handle<StandardMaterial>>,
}

/// Stores the computed Z height for each elevation level's floor.
#[derive(Resource, Default)]
pub struct ElevationHeights {
    pub z_by_level: std::collections::HashMap<u8, f32>,
}

/// What kind of terrain layer this is.
pub enum LayerKind {
    /// Regular terrain: participates in procedural fill + elevation quads.
    Terrain { height_tiles: u8 },
    /// Slope overlay: tiles define height deltas for sloped terrain.
    Slope,
}

/// Parse elevation level and layer kind from a Tiled layer name.
///
/// Formats:
///   `terrain_N`          → level N, Terrain(height=1)
///   `terrain_N_hH`       → level N, Terrain(height=H)
///   `terrain_N_customY`  → level N, CustomWall(wall_height=Y)
pub fn parse_elevation_from_layer_name(name: &str) -> Option<(u8, LayerKind)> {
    let tiled_layer_name = name
        .strip_prefix("TiledMapTileLayerForTileset(")
        .and_then(|s| s.strip_suffix(')'))
        .and_then(|s| s.rsplit_once(", "))
        .map(|(layer, _tileset)| layer)
        .unwrap_or(name);

    let remainder = tiled_layer_name.strip_prefix("terrain_")?;

    // Check for _slopes suffix first
    if let Some(level_str) = remainder.strip_suffix("_slopes") {
        let level = level_str.parse::<u8>().ok().filter(|&n| n <= 15)?;
        return Some((level, LayerKind::Slope));
    }

    // Check for _hH suffix
    if let Some((level_str, h_str)) = remainder.rsplit_once("_h") {
        let level = level_str.parse::<u8>().ok().filter(|&n| n <= 15)?;
        let height = h_str.parse::<u8>().ok().filter(|&h| h >= 1)?;
        return Some((level, LayerKind::Terrain { height_tiles: height }));
    }

    // Plain terrain_N
    let level = remainder.parse::<u8>().ok().filter(|&n| n <= 15)?;
    Some((level, LayerKind::Terrain { height_tiles: 1 }))
}

fn render_layer_for_elevation(level: u8) -> usize {
    (level as usize) + 1
}

pub fn setup_elevation_meshes(
    mut commands: Commands,
    config: Res<ElevationConfig>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut elev_materials: ResMut<ElevationMaterials>,
    mut elev_heights: ResMut<ElevationHeights>,
    slope_maps: Res<super::slope::SlopeHeightMaps>,
    layers: Query<
        (Entity, &TileElevation, &TileStorage, &TilemapSize, &TilemapTileSize),
        (With<TerrainTypeMapReady>, With<TiledMapTileLayerForTileset>,
         Without<ElevationMeshReady>, Without<SlopeLayer>),
    >,
) {
    let mut elevation_groups: std::collections::HashMap<u8, Vec<Entity>> =
        std::collections::HashMap::new();
    let mut height_per_level: std::collections::HashMap<u8, u8> = std::collections::HashMap::new();
    let mut map_dims: Option<(f32, f32, f32, f32)> = None; // (map_w, map_h, tw, th)

    for (entity, elevation, _storage, size, tile_size) in &layers {
        elevation_groups
            .entry(elevation.level)
            .or_default()
            .push(entity);
        let h = height_per_level.entry(elevation.level).or_insert(1);
        *h = (*h).max(elevation.height_tiles);
        if map_dims.is_none() {
            map_dims = Some((
                size.x as f32 * tile_size.x,
                size.y as f32 * tile_size.y,
                tile_size.x,
                tile_size.y,
            ));
        }
    }

    if elevation_groups.is_empty() {
        return;
    }

    let Some((map_w, map_h, _tw, _th)) = map_dims else {
        return;
    };
    let map_center = Vec2::new(map_w * 0.5, map_h * 0.5);

    // Compute tile bounding boxes per level
    let mut level_bounds: std::collections::HashMap<u8, (f32, f32, f32, f32)> = std::collections::HashMap::new();
    for (_, elevation, storage, size, tile_size) in &layers {
        let lv = elevation.level;
        let tw = tile_size.x;
        let th = tile_size.y;
        for y in 0..size.y {
            for x in 0..size.x {
                if storage.checked_get(&TilePos::new(x, y)).is_some() {
                    let wx = x as f32 * tw;
                    let wy = y as f32 * th;
                    let bounds = level_bounds.entry(lv).or_insert((wx, wy, wx + tw, wy + th));
                    bounds.0 = bounds.0.min(wx);
                    bounds.1 = bounds.1.min(wy);
                    bounds.2 = bounds.2.max(wx + tw);
                    bounds.3 = bounds.3.max(wy + th);
                }
            }
        }
    }

    for (&level, entities) in &elevation_groups {
        let rl = render_layer_for_elevation(level);

        let extent = Extent3d {
            width: map_w as u32,
            height: map_h as u32,
            depth_or_array_layers: 1,
        };
        let mut rt_image = Image {
            texture_descriptor: TextureDescriptor {
                label: Some("elevation_rt"),
                size: extent,
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba8UnormSrgb,
                usage: TextureUsages::RENDER_ATTACHMENT
                    | TextureUsages::TEXTURE_BINDING
                    | TextureUsages::COPY_DST,
                view_formats: &[],
            },
            ..default()
        };
        rt_image.resize(extent);
        let rt_handle = images.add(rt_image);

        commands.spawn((
            Camera2d,
            Camera {
                order: -(level as isize) - 10,
                target: RenderTarget::Image(rt_handle.clone()),
                clear_color: ClearColorConfig::Custom(Color::srgba(0.0, 0.0, 0.0, 0.0)),
                ..default()
            },
            OrthographicProjection {
                near: -1000.0,
                far: 1000.0,
                scaling_mode: bevy::render::camera::ScalingMode::Fixed {
                    width: map_w,
                    height: map_h,
                },
                ..OrthographicProjection::default_2d()
            },
            Transform::from_xyz(map_center.x, map_center.y, 0.0),
            RenderLayers::layer(rl),
            ElevationRenderCam { level },
        ));

        for &entity in entities {
            commands
                .entity(entity)
                .insert(RenderLayers::layer(rl))
                .insert(ElevationMeshReady);
        }

        // Level 0 is always at ground (Z = -1).
        // Higher levels: Z = sum of height_tiles * hpl for all levels 1..=level.
        // This way level 0 = -1, level 1 = 1*48-1 = 47, level 2 = 2*48-1 = 95 (defaults).
        let z: f32 = if level == 0 {
            -1.0
        } else {
            (1..=level)
                .map(|l| *height_per_level.get(&l).unwrap_or(&1) as f32 * config.height_per_level)
                .sum::<f32>() - 1.0
        };
        elev_heights.z_by_level.insert(level, z);
        let mat = materials.add(StandardMaterial {
            base_color_texture: Some(rt_handle),
            unlit: true,
            alpha_mode: AlphaMode::Mask(0.9),
            double_sided: true,
            cull_mode: None,
            ..default()
        });

        elev_materials.by_level.insert(level, mat.clone());

        // Determine bounding box for the mesh.
        let (min_x, min_y, max_x, max_y) = if level == 0 {
            (0.0, 0.0, map_w, map_h)
        } else if let Some(&b) = level_bounds.get(&level) {
            b
        } else {
            (0.0, 0.0, map_w, map_h)
        };

        let slope_hm = slope_maps.by_level.get(&level);
        let has_slopes = slope_hm.is_some_and(|hm| hm.has_slopes());

        let mesh = if has_slopes {
            // ── Gridded mesh: one quad per tile with per-corner Z offsets ──
            let hm = slope_hm.unwrap();
            let tw = _tw;
            let th = _th;
            let tile_x0 = (min_x / tw).floor() as usize;
            let tile_y0 = (min_y / th).floor() as usize;
            let tile_x1 = (max_x / tw).ceil() as usize;
            let tile_y1 = (max_y / th).ceil() as usize;
            let tiles_w = tile_x1 - tile_x0;
            let tiles_h = tile_y1 - tile_y0;

            let verts_w = tiles_w + 1;
            let verts_h = tiles_h + 1;
            let num_verts = verts_w * verts_h;

            // Positions: world XY relative to quad center, Z from height map.
            // UVs: based on world XY (ground-plane projection) so textures
            // don't pinch on slopes.
            let cx = (min_x + max_x) * 0.5;
            let cy = (min_y + max_y) * 0.5;

            let mut positions = Vec::with_capacity(num_verts);
            let mut uvs = Vec::with_capacity(num_verts);

            for vy in 0..verts_h {
                for vx in 0..verts_w {
                    let world_x = (tile_x0 + vx) as f32 * tw;
                    let world_y = (tile_y0 + vy) as f32 * th;
                    let corner_z = hm.get(tile_x0 + vx, tile_y0 + vy);

                    positions.push([world_x - cx, world_y - cy, corner_z]);

                    let u = world_x / map_w;
                    let v = 1.0 - world_y / map_h; // RTT Y flip
                    uvs.push([u, v]);
                }
            }

            // Normals: compute per-vertex from adjacent face normals
            let mut normals = vec![[0.0f32, 0.0, 1.0]; num_verts];
            for ty in 0..tiles_h {
                for tx in 0..tiles_w {
                    let sw = ty * verts_w + tx;
                    let se = sw + 1;
                    let nw = sw + verts_w;
                    let ne = nw + 1;

                    let p_sw = Vec3::from(positions[sw]);
                    let p_se = Vec3::from(positions[se]);
                    let p_nw = Vec3::from(positions[nw]);
                    let p_ne = Vec3::from(positions[ne]);

                    // Two triangles: SW→SE→NE and SW→NE→NW
                    let n1 = (p_se - p_sw).cross(p_ne - p_sw);
                    let n2 = (p_ne - p_sw).cross(p_nw - p_sw);

                    for &vi in &[sw, se, ne] {
                        normals[vi][0] += n1.x;
                        normals[vi][1] += n1.y;
                        normals[vi][2] += n1.z;
                    }
                    for &vi in &[sw, ne, nw] {
                        normals[vi][0] += n2.x;
                        normals[vi][1] += n2.y;
                        normals[vi][2] += n2.z;
                    }
                }
            }
            // Normalize
            for n in &mut normals {
                let len = (n[0]*n[0] + n[1]*n[1] + n[2]*n[2]).sqrt();
                if len > 0.0 {
                    n[0] /= len; n[1] /= len; n[2] /= len;
                }
            }

            // Indices: two triangles per tile
            let mut indices = Vec::with_capacity(tiles_w * tiles_h * 6);
            for ty in 0..tiles_h {
                for tx in 0..tiles_w {
                    let sw = (ty * verts_w + tx) as u32;
                    let se = sw + 1;
                    let nw = sw + verts_w as u32;
                    let ne = nw + 1;

                    indices.extend_from_slice(&[sw, se, ne, sw, ne, nw]);
                }
            }

            info!(
                "Elevation {level}: gridded mesh {}x{} tiles, {} verts, {} tris",
                tiles_w, tiles_h, num_verts, indices.len() / 3
            );

            meshes.add(
                Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
                    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
                    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
                    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
                    .with_inserted_indices(Indices::U32(indices))
            )
        } else if level > 0 && level_bounds.contains_key(&level) {
            // ── Flat quad clipped to tile bounding box ──
            let hw = (max_x - min_x) * 0.5;
            let hh = (max_y - min_y) * 0.5;
            let u0 = min_x / map_w;
            let u1 = max_x / map_w;
            let v0 = 1.0 - max_y / map_h;
            let v1 = 1.0 - min_y / map_h;

            meshes.add(
                Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
                    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![
                        [-hw, -hh, 0.0], [hw, -hh, 0.0], [hw, hh, 0.0], [-hw, hh, 0.0],
                    ])
                    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4])
                    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![
                        [u0, v1], [u1, v1], [u1, v0], [u0, v0],
                    ])
                    .with_inserted_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]))
            )
        } else {
            // ── Full map rectangle (level 0 without slopes) ──
            meshes.add(Rectangle::new(map_w, map_h))
        };

        let qcx = (min_x + max_x) * 0.5;
        let qcy = (min_y + max_y) * 0.5;

        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(mat),
            Transform::from_xyz(qcx, qcy, z),
            ElevationQuad { level },
        ));

        info!(
            "Elevation {level}: render layer {rl}, {} tilemap layer(s), quad at Z={z}",
            entities.len()
        );
    }
}
