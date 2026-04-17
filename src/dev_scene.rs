use avian3d::prelude::*;
use bevy::prelude::*;
use bevy_ecs_tiled::prelude::*;

use crate::app_state::GameState;
use crate::camera::combat::{
    Billboard, CombatCamera, CombatGridVisual, CombatTestEntity,
    compute_combat_view, compute_combat_grid, spawn_combat_grid,
    BILLBOARD_COLLIDER_Y_SCALE,
};
use crate::camera::follow::CameraTarget;
use crate::map::collision::CornerSlip;
use crate::map::DEFAULT_TILE_SIZE;

pub struct DevScenePlugin;

impl Plugin for DevScenePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, boot_into_overworld)
            .add_systems(OnEnter(GameState::Overworld), spawn_dev_scene)
            .add_systems(
                Update,
                (
                    player_movement,
                    patch_map_colliders,
                    debug_colliders,
                    toggle_combat_camera,
                )
                    .run_if(in_state(GameState::Overworld)),
            );
    }
}

/// Skip loading/menu for dev — go straight to Overworld.
fn boot_into_overworld(mut next_state: ResMut<NextState<GameState>>) {
    next_state.set(GameState::Overworld);
}

/// Spawn the Tiled map and the player.
fn spawn_dev_scene(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut std_materials: ResMut<Assets<StandardMaterial>>,
) {
    // ── Tiled map ────────────────────────────────────────────────────
    commands.spawn((
        TiledMap(asset_server.load("maps/test.tmx")),
        TiledPhysicsSettings::<crate::map::tiled_physics_3d::TiledPhysics3dBackend>::default(),
        // Shift terrain east by half a tile for visual alignment.
        Transform::from_xyz(crate::map::DEFAULT_TILE_SIZE * 0.5, 0.0, 0.0),
    ));

    // ── Player (blue square with physics) ────────────────────────────
    let player_mat = std_materials.add(StandardMaterial {
        base_color: Color::srgb(0.2, 0.4, 0.9),
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Rectangle::new(32.0, 32.0))),
        MeshMaterial3d(player_mat),
        Transform::from_xyz(0.0, 0.0, 16.0),
        CameraTarget,
        Player,
        Billboard,
        // Physics: dynamic body, no gravity, no bounce, no spin
        RigidBody::Dynamic,
        Collider::cuboid(28.0, 28.0, 28.0),
        LockedAxes::ROTATION_LOCKED.lock_translation_z(),
        Friction::ZERO,
        Restitution::ZERO,
        LinearDamping(0.0),
        CornerSlip::default(),
    ));

    info!("Dev scene spawned — WASD to move, map loaded from maps/test.tmx");
}

/// Marker for the player entity.
#[derive(Component)]
pub struct Player;

/// Player movement speed in pixels/second.
const PLAYER_SPEED: f32 = 200.0;

/// Set player velocity from WASD input. The physics engine handles collision
/// response and wall sliding automatically.
fn player_movement(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut query: Query<&mut LinearVelocity, With<Player>>,
) {
    let Some(mut vel) = query.iter_mut().next() else {
        return;
    };

    let mut direction = Vec2::ZERO;
    if keyboard.pressed(KeyCode::KeyW) {
        direction.y += 1.0;
    }
    if keyboard.pressed(KeyCode::KeyS) {
        direction.y -= 1.0;
    }
    if keyboard.pressed(KeyCode::KeyA) {
        direction.x -= 1.0;
    }
    if keyboard.pressed(KeyCode::KeyD) {
        direction.x += 1.0;
    }

    let dir = direction.normalize_or_zero() * PLAYER_SPEED;
    vel.0 = Vec3::new(dir.x, dir.y, 0.0);
}

/// Patch colliders spawned by bevy_ecs_tiled that are missing a RigidBody.
fn patch_map_colliders(
    mut commands: Commands,
    colliders_without_rb: Query<Entity, (Added<Collider>, Without<RigidBody>, Without<Player>)>,
) {
    for entity in colliders_without_rb.iter() {
        commands.entity(entity).insert(RigidBody::Static);
    }
}

/// Toggle combat camera with spacebar for testing.
fn toggle_combat_camera(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut combat_camera: ResMut<CombatCamera>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut std_materials: ResMut<Assets<StandardMaterial>>,
    player_q: Query<&Transform, With<Player>>,
    test_entities: Query<Entity, With<CombatTestEntity>>,
    grid_visuals: Query<Entity, With<CombatGridVisual>>,
    tilemap_layers: Query<
        (Entity, &Name, &bevy_ecs_tilemap::prelude::TileStorage,
         &bevy_ecs_tilemap::prelude::TilemapSize, &bevy_ecs_tilemap::prelude::TilemapTileSize),
        With<bevy_ecs_tiled::prelude::TiledTilemap>,
    >,
    tile_data: Query<(&bevy_ecs_tilemap::prelude::TilePos, &bevy_ecs_tilemap::prelude::TileTextureIndex)>,
    slope_maps: Res<crate::map::slope::SlopeHeightMaps>,
    elev_heights: Res<crate::map::elevation::ElevationHeights>,
    elev_config: Res<crate::map::elevation::ElevationConfig>,
) {
    if !keyboard.just_pressed(KeyCode::Space) {
        return;
    }

    if combat_camera.active {
        // ── Deactivate ──────────────────────────────────────────────
        combat_camera.active = false;

        // Despawn test entities and grid
        for e in test_entities.iter() {
            commands.entity(e).despawn();
        }
        for e in grid_visuals.iter() {
            commands.entity(e).despawn();
        }
        info!("Combat camera OFF");
    } else {
        // ── Activate ────────────────────────────────────────────────
        let Some(player_tf) = player_q.iter().next() else { return };
        let player_pos = player_tf.translation.truncate();

        // Spawn dummy enemies around the player
        let offsets = [
            Vec2::new(3.0, 2.0),
            Vec2::new(-2.0, 3.0),
            Vec2::new(4.0, -1.0),
        ];
        let enemy_mat = std_materials.add(StandardMaterial {
            base_color: Color::srgb(0.9, 0.2, 0.2),
            unlit: true,
            double_sided: true,
            cull_mode: None,
            ..default()
        });
        let mut positions = vec![player_pos];

        for offset in &offsets {
            let pos = player_pos + *offset * DEFAULT_TILE_SIZE;
            positions.push(pos);
            commands.spawn((
                Mesh3d(meshes.add(Rectangle::new(32.0, 32.0))),
                MeshMaterial3d(enemy_mat.clone()),
                Transform::from_xyz(pos.x, pos.y, 16.0),
                Billboard,
                CombatTestEntity,
            ));
        }

        // Compute camera view from all actor positions
        let (center, capture_size) = compute_combat_view(&positions);

        // Compute walkable grid, expanding from actors until hitting obstacles
        let (grid_origin, grid_size) = compute_combat_grid(
            &positions,
            &tilemap_layers,
            &tile_data,
        );

        combat_camera.active = true;
        combat_camera.activate_time = 0.0;
        combat_camera.target_center = center;
        combat_camera.capture_size = capture_size;
        combat_camera.camera_height = capture_size.y.max(capture_size.x) * 1.2;
        combat_camera.grid_origin = grid_origin;
        combat_camera.grid_size = grid_size;

        // Spawn grid (starts invisible, fades in via combat_grid_fade)
        spawn_combat_grid(
            &mut commands, &mut meshes, &mut std_materials,
            grid_origin, grid_size, 0.0,
            &slope_maps, &elev_heights, elev_config.slope_angle_deg,
        );

        info!(
            "Combat camera ON — grid {}x{} at tile ({}, {})",
            grid_size.x, grid_size.y, grid_origin.x, grid_origin.y
        );
    }
}

/// Temporary debug: log collider info once after map loads.
fn debug_colliders(
    colliders: Query<(Entity, &Collider, Option<&RigidBody>, Option<&Transform>)>,
    mut logged: Local<bool>,
    player: Query<Entity, With<Player>>,
) {
    if *logged {
        return;
    }
    let total = colliders.iter().count();
    if total > 1 {
        let player_e = player.iter().next();
        for (e, _collider, rb, tf) in colliders.iter() {
            let is_player = Some(e) == player_e;
            info!(
                "Collider entity {:?}: is_player={}, rigid_body={:?}, pos={:?}",
                e,
                is_player,
                rb,
                tf.map(|t| t.translation),
            );
        }
        *logged = true;
    }
}
