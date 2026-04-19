pub mod collision;
pub mod elevation;
pub mod height;
pub mod loader;
pub mod properties;
pub mod slope;
pub mod terrain_edges;
pub mod terrain_material;
pub mod tiled_physics_3d;

use avian3d::prelude::*;
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
#[cfg(feature = "dev_tools")]
use bevy::pbr::wireframe::WireframeConfig;
use bevy::prelude::*;
use bevy_ecs_tiled::prelude::*;
use bevy_ecs_tilemap::prelude::*;

use crate::app_state::GameState;
use crate::camera::follow::MapBounds;
use elevation::ElevationConfig;
use terrain_material::TerrainMaterial;

/// Default tile size in pixels. Used as the physics length unit.
/// The actual tile size is read from the map file at runtime for rendering.
pub const DEFAULT_TILE_SIZE: f32 = 48.0;

pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(TiledPlugin::default())
            .add_plugins(TiledPhysicsPlugin::<tiled_physics_3d::TiledPhysics3dBackend>::default())
            .add_plugins(PhysicsPlugins::default().with_length_unit(DEFAULT_TILE_SIZE))
            .add_plugins(MaterialTilemapPlugin::<TerrainMaterial>::default())
            .add_plugins(FrameTimeDiagnosticsPlugin::default());

        // PhysicsDebugPlugin registers per-frame systems that iterate every
        // physics entity even when gizmo rendering is disabled — keep it out
        // of release builds.
        #[cfg(feature = "dev_tools")]
        app.add_plugins(PhysicsDebugPlugin::default());

        app
            .insert_resource(Gravity(Vec3::ZERO))
            .init_resource::<MapBounds>()
            .init_resource::<ElevationConfig>()
            .init_resource::<elevation::ElevationMaterials>()
            .init_resource::<elevation::ElevationHeights>()
            .init_resource::<slope::SlopeHeightMaps>();

        app.add_systems(
            Update,
            (
                collision::corner_slip_system
                    .run_if(in_state(GameState::Overworld).or(in_state(GameState::Combat))),
                terrain_material::build_terrain_and_attach_material,
                slope::compute_slope_height_maps
                    .after(terrain_material::build_terrain_and_attach_material),
                elevation::setup_elevation_meshes
                    .after(slope::compute_slope_height_maps),
                slope::generate_mesh_slope_colliders
                    .after(slope::compute_slope_height_maps),
                terrain_edges::generate_edge_colliders,
                update_map_bounds,
                update_fps_display,
            ),
        );

        app.add_systems(Startup, spawn_fps_display);

        #[cfg(feature = "dev_tools")]
        app.add_systems(Startup, disable_physics_debug)
            .add_systems(Update, toggle_debug_overlay);
    }
}

/// Detect map bounds from any tilemap layer once available.
fn update_map_bounds(
    mut bounds: ResMut<MapBounds>,
    layers: Query<(&TilemapSize, &TilemapTileSize), Added<TilemapSize>>,
) {
    if bounds.valid { return; }
    for (map_size, tile_size) in &layers {
        let w = map_size.x as f32 * tile_size.x;
        let h = map_size.y as f32 * tile_size.y;
        bounds.min = Vec2::ZERO;
        bounds.max = Vec2::new(w, h);
        bounds.valid = true;
        info!("Map bounds set: {}x{} pixels", w, h);
        return;
    }
}

/// Start with physics debug gizmos disabled and rendered above terrain.
#[cfg(feature = "dev_tools")]
fn disable_physics_debug(mut config_store: ResMut<GizmoConfigStore>) {
    // Disable physics gizmos by default, set to render on top when enabled
    let (gizmo_config, physics_config) = config_store.config_mut::<PhysicsGizmos>();
    gizmo_config.enabled = false;
    gizmo_config.depth_bias = -1.0;
}

/// Marker for the FPS text entity.
#[derive(Component)]
struct FpsText;

/// Spawns the FPS counter at startup so it's always visible.
fn spawn_fps_display(mut commands: Commands) {
    commands.spawn((
        Text::new("FPS: --"),
        TextFont {
            font_size: 18.0,
            ..default()
        },
        TextColor(Color::srgb(1.0, 1.0, 0.0)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(5.0),
            left: Val::Px(5.0),
            ..default()
        },
        FpsText,
    ));
}

/// F3 toggles debug overlay: physics collider gizmos + wireframe.
#[cfg(feature = "dev_tools")]
fn toggle_debug_overlay(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut config_store: ResMut<GizmoConfigStore>,
    mut wireframe_config: ResMut<WireframeConfig>,
) {
    if !keyboard.just_pressed(KeyCode::F3) {
        return;
    }

    let (config, _) = config_store.config_mut::<PhysicsGizmos>();
    config.enabled = !config.enabled;
    wireframe_config.global = config.enabled;
}

/// Update FPS text each frame when visible.
fn update_fps_display(
    diagnostics: Res<DiagnosticsStore>,
    mut query: Query<&mut Text, With<FpsText>>,
) {
    for mut text in &mut query {
        if let Some(fps) = diagnostics
            .get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FPS)
            .and_then(|d| d.smoothed())
        {
            **text = format!("FPS: {:.0}", fps);
        }
    }
}
