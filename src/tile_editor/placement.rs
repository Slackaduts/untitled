//! Grid-snapped placement mode: ghost preview, click to place/remove.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::camera::CombatCamera3d;
use crate::map::loader::CurrentMap;
use crate::map::DEFAULT_TILE_SIZE;

use super::sidecar::{self, PlacedObjectDef};
use super::spawner;
use super::state::{EditorMode, PlacedObject, TileEditorState};

/// Component marking the ghost preview entity.
#[derive(Component)]
pub struct PlacementGhost;

/// System: manages the ghost preview and handles placement clicks.
pub fn placement_system(
    mut commands: Commands,
    state: Res<TileEditorState>,
    mut ghost_q: Query<(Entity, &mut Transform, &mut Visibility), With<PlacementGhost>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<CombatCamera3d>>,
    mut contexts: bevy_egui::EguiContexts,
    mouse: Res<ButtonInput<MouseButton>>,
) {
    // Despawn ghost if not in Place mode or no object composed
    if !state.open || state.mode != EditorMode::Place || state.current_object.is_none() {
        for (entity, _, _) in &ghost_q {
            commands.entity(entity).despawn();
        }
        return;
    }

    let Ok(ctx) = contexts.ctx_mut() else { return };
    let egui_wants = ctx.is_pointer_over_area();

    let Ok(window) = windows.single() else { return };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };
    let Ok((camera, cam_tf)) = cameras.single() else {
        return;
    };

    // Project cursor to world
    let Ok(ray) = camera.viewport_to_world(cam_tf, cursor_pos) else {
        return;
    };
    let Some(distance) = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Z)) else {
        return;
    };
    let world_pos = ray.get_point(distance);

    // Snap to grid
    let grid_x = (world_pos.x / DEFAULT_TILE_SIZE).floor() as i32;
    let grid_y = (world_pos.y / DEFAULT_TILE_SIZE).floor() as i32;
    let snapped = Vec3::new(
        (grid_x as f32 + 0.5) * DEFAULT_TILE_SIZE,
        (grid_y as f32 + 0.5) * DEFAULT_TILE_SIZE,
        1.0, // Slight Z offset for visibility
    );

    // Update or spawn ghost
    if let Some((_, mut tf, mut vis)) = ghost_q.iter_mut().next() {
        tf.translation = snapped;
        *vis = Visibility::Visible;
    }
    // Ghost spawning is handled by the editor UI (placeholder for now)

    // Handle clicks (only when egui doesn't want the pointer)
    if egui_wants {
        return;
    }

    // Left click = place
    if mouse.just_pressed(MouseButton::Left) {
        // This is handled by place_object_system which has write access to state
    }

    // Right click = remove nearest
    if mouse.just_pressed(MouseButton::Right) {
        // Handled by remove_object_system
    }
}

/// System: places objects on left-click when in Place mode.
pub fn place_object_on_click(
    mut commands: Commands,
    mut state: ResMut<TileEditorState>,
    current_map: Res<CurrentMap>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<CombatCamera3d>>,
    mut contexts: bevy_egui::EguiContexts,
    mouse: Res<ButtonInput<MouseButton>>,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut bb_materials: ResMut<Assets<bevy::pbr::ExtendedMaterial<StandardMaterial, crate::particles::gpu_lights::ParticleLightExt>>>,
    particle_buf: Res<crate::particles::gpu_lights::ParticleLightBuffer>,
    mut images: ResMut<Assets<Image>>,
) {
    if !state.open || state.mode != EditorMode::Place {
        return;
    }

    // Clone what we need from current_object before mutably borrowing state
    let obj_data = match &state.current_object {
        Some(o) => (
            o.sprite_key.clone(),
            o.tileset_name.clone(),
            o.tile_ids.clone(),
            o.properties.clone(),
            o.collision_rects.clone(),
        ),
        None => return,
    };

    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }

    let Ok(ctx) = contexts.ctx_mut() else { return };
    if ctx.is_pointer_over_area() {
        return;
    }

    let Ok(window) = windows.single() else { return };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };
    let Ok((camera, cam_tf)) = cameras.single() else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(cam_tf, cursor_pos) else {
        return;
    };
    let Some(distance) = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Z)) else {
        return;
    };
    let world_pos = ray.get_point(distance);

    let grid_x = (world_pos.x / DEFAULT_TILE_SIZE).floor() as i32;
    let grid_y = (world_pos.y / DEFAULT_TILE_SIZE).floor() as i32;

    // Generate next ID
    let max_id = state
        .placed_objects
        .iter()
        .filter_map(|o| o.id.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    let id = (max_id + 1).to_string();

    let (sprite_key, tileset, tile_ids, properties, collision_rects) = obj_data;

    let placed_def = PlacedObjectDef {
        id: id.clone(),
        name: None,
        sprite_key: sprite_key.clone(),
        tileset,
        tile_ids,
        grid_pos: [grid_x, grid_y],
        elevation: 0,
        properties,
        collision_rects,
        door: None,
    };

    // Spawn the billboard entity
    spawner::spawn_sidecar_object_immediate(
        &mut commands,
        &asset_server,
        &mut meshes,
        &mut bb_materials,
        &particle_buf,
        &mut images,
        &placed_def,
    );

    // Add to state and auto-save
    state.placed_objects.push(placed_def);

    if let Some(map_path) = &current_map.path {
        let file = sidecar::MapObjectsFile {
            version: 1,
            objects: state.placed_objects.clone(),
        };
        if let Err(e) = sidecar::save_sidecar(map_path, &file) {
            error!("Failed to save sidecar: {e}");
        }
    }

    info!("Placed object {sprite_key} at grid ({grid_x}, {grid_y})");
}

/// System: select placed objects by right-clicking them (works in any mode).
/// Also resolves pending selections from the UI list.
pub fn select_placed_object(
    mut state: ResMut<TileEditorState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<CombatCamera3d>>,
    mut contexts: bevy_egui::EguiContexts,
    mouse: Res<ButtonInput<MouseButton>>,
    placed_q: Query<(Entity, &PlacedObject, &Transform, &crate::camera::combat::BillboardHeight)>,
) {
    if !state.open {
        return;
    }

    // Resolve pending selection from the UI list
    if let Some(sidecar_id) = state.pending_select_sidecar_id.take() {
        if let Some((entity, _, _, _)) = placed_q.iter().find(|(_, po, _, _)| po.sidecar_id == sidecar_id) {
            state.selected_placed = Some(entity);
            state.selected_sidecar_id = Some(sidecar_id);
        }
        return;
    }

    if !mouse.just_pressed(MouseButton::Right) {
        return;
    }

    let Ok(ctx) = contexts.ctx_mut() else { return };
    if ctx.is_pointer_over_area() {
        return;
    }

    let Some(world_pos) = cursor_to_world(&windows, &cameras) else {
        return;
    };

    // Find placed object whose billboard bounding box contains the click point.
    // Billboard origin is at the center of the bottom tile. The quad extends:
    //   X: [-width/2, +width/2]
    //   Y: [-TILE_SIZE/2, height - TILE_SIZE/2]
    let mut closest: Option<(Entity, f32, String)> = None;
    for (entity, po, tf, bh) in &placed_q {
        let pos = tf.translation;
        let half_w = bh.height * 0.5; // approximate width with height
        let bottom = pos.y - DEFAULT_TILE_SIZE * 0.5;
        let top = pos.y + bh.height - DEFAULT_TILE_SIZE * 0.5;
        let left = pos.x - half_w;
        let right = pos.x + half_w;

        if world_pos.x >= left && world_pos.x <= right
            && world_pos.y >= bottom && world_pos.y <= top
        {
            let dist = tf.translation.truncate().distance(world_pos.truncate());
            if closest.is_none() || dist < closest.as_ref().unwrap().1 {
                closest = Some((entity, dist, po.sidecar_id.clone()));
            }
        }
    }

    if let Some((entity, _, sidecar_id)) = closest {
        if state.selected_placed == Some(entity) {
            state.selected_placed = None;
            state.selected_sidecar_id = None;
        } else {
            state.selected_placed = Some(entity);
            state.selected_sidecar_id = Some(sidecar_id);
        }
    } else {
        state.selected_placed = None;
        state.selected_sidecar_id = None;
    }
}

/// System: delete selected placed object with Delete key.
pub fn delete_selected_object(
    mut commands: Commands,
    mut state: ResMut<TileEditorState>,
    current_map: Res<CurrentMap>,
    keyboard: Res<ButtonInput<KeyCode>>,
    placed_q: Query<(Entity, &PlacedObject)>,
    children_q: Query<(Entity, &super::state::SidecarChild)>,
) {
    if !state.open {
        return;
    }

    let Some(selected_entity) = state.selected_placed else {
        return;
    };

    if !keyboard.just_pressed(KeyCode::Delete) && !keyboard.just_pressed(KeyCode::Backspace) {
        return;
    }

    // Find the sidecar ID for this entity
    let Some((entity, po)) = placed_q.iter().find(|(e, _)| *e == selected_entity) else {
        state.selected_placed = None;
        return;
    };

    let sidecar_id = po.sidecar_id.clone();

    // Despawn the billboard entity
    commands.entity(entity).despawn();

    // Despawn all associated lights/emitters (SidecarChild entities)
    for (child_entity, child) in &children_q {
        if child.sidecar_id == sidecar_id {
            commands.entity(child_entity).despawn();
        }
    }

    state.selected_placed = None;
    state.selected_sidecar_id = None;
    state.placed_objects.retain(|o| o.id != sidecar_id);

    // Auto-save
    if let Some(map_path) = &current_map.path {
        let file = sidecar::MapObjectsFile {
            version: 1,
            objects: state.placed_objects.clone(),
        };
        let _ = sidecar::save_sidecar(map_path, &file);
    }

    info!("Deleted placed object {sidecar_id}");
}

fn cursor_to_world(
    windows: &Query<&Window, With<PrimaryWindow>>,
    cameras: &Query<(&Camera, &GlobalTransform), With<CombatCamera3d>>,
) -> Option<Vec3> {
    let window = windows.single().ok()?;
    let cursor_pos = window.cursor_position()?;
    let (camera, cam_tf) = cameras.single().ok()?;
    let ray = camera.viewport_to_world(cam_tf, cursor_pos).ok()?;
    let distance = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Z))?;
    Some(ray.get_point(distance))
}

/// Draw grid overlay gizmos when in placement mode.
pub fn draw_placement_grid(
    state: Res<TileEditorState>,
    cameras: Query<(&Camera, &GlobalTransform), With<CombatCamera3d>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut gizmos: Gizmos,
) {
    if !state.open || state.mode != EditorMode::Place {
        return;
    }

    let Ok((_camera, cam_tf)) = cameras.single() else {
        return;
    };

    // Draw grid around camera center
    let cam_pos = cam_tf.translation().truncate();
    let half_range = 15; // tiles in each direction
    let grid_color = Color::srgba(1.0, 1.0, 1.0, 0.08);

    let center_tile_x = (cam_pos.x / DEFAULT_TILE_SIZE).round() as i32;
    let center_tile_y = (cam_pos.y / DEFAULT_TILE_SIZE).round() as i32;

    for dx in -half_range..=half_range {
        let x = (center_tile_x + dx) as f32 * DEFAULT_TILE_SIZE;
        let y_min = (center_tile_y - half_range) as f32 * DEFAULT_TILE_SIZE;
        let y_max = (center_tile_y + half_range) as f32 * DEFAULT_TILE_SIZE;
        gizmos.line(
            Vec3::new(x, y_min, 0.5),
            Vec3::new(x, y_max, 0.5),
            grid_color,
        );
    }
    for dy in -half_range..=half_range {
        let y = (center_tile_y + dy) as f32 * DEFAULT_TILE_SIZE;
        let x_min = (center_tile_x - half_range) as f32 * DEFAULT_TILE_SIZE;
        let x_max = (center_tile_x + half_range) as f32 * DEFAULT_TILE_SIZE;
        gizmos.line(
            Vec3::new(x_min, y, 0.5),
            Vec3::new(x_max, y, 0.5),
            grid_color,
        );
    }

    // Highlight the tile under cursor
    let Ok(window) = windows.single() else { return };
    let Some(cursor_pos) = window.cursor_position() else { return };
    let Ok((camera, cam_tf)) = cameras.single() else { return };
    let Ok(ray) = camera.viewport_to_world(cam_tf, cursor_pos) else { return };
    let Some(distance) = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Z)) else { return };
    let world_pos = ray.get_point(distance);

    let gx = (world_pos.x / DEFAULT_TILE_SIZE).floor() as i32;
    let gy = (world_pos.y / DEFAULT_TILE_SIZE).floor() as i32;
    let cx = (gx as f32 + 0.5) * DEFAULT_TILE_SIZE;
    let cy = (gy as f32 + 0.5) * DEFAULT_TILE_SIZE;
    let half = DEFAULT_TILE_SIZE / 2.0;
    let z = 0.6;
    let highlight = Color::srgba(1.0, 1.0, 0.3, 0.3);

    // Draw highlighted square
    gizmos.line(Vec3::new(cx - half, cy - half, z), Vec3::new(cx + half, cy - half, z), highlight);
    gizmos.line(Vec3::new(cx + half, cy - half, z), Vec3::new(cx + half, cy + half, z), highlight);
    gizmos.line(Vec3::new(cx + half, cy + half, z), Vec3::new(cx - half, cy + half, z), highlight);
    gizmos.line(Vec3::new(cx - half, cy + half, z), Vec3::new(cx - half, cy - half, z), highlight);
}

/// Draw a highlight gizmo around the currently selected placed object.
pub fn draw_selection_gizmo(
    state: Res<TileEditorState>,
    placed_q: Query<(Entity, &Transform, &crate::camera::combat::BillboardHeight), With<PlacedObject>>,
    mut gizmos: Gizmos,
    time: Res<Time>,
) {
    let Some(selected) = state.selected_placed else {
        return;
    };

    let Some((_, tf, bh)) = placed_q.iter().find(|(e, _, _)| *e == selected) else {
        return;
    };

    let pos = tf.translation;
    let half_w = bh.height * 0.5; // approximate width with height
    let half_h = bh.height * 0.5;
    let z = pos.z + 1.0;

    // Pulsing yellow selection box
    let pulse = ((time.elapsed_secs() * 3.0).sin() * 0.3 + 0.7).clamp(0.4, 1.0);
    let color = Color::srgba(1.0, 1.0, 0.2, pulse);

    let corners = [
        Vec3::new(pos.x - half_w, pos.y - DEFAULT_TILE_SIZE * 0.5, z),
        Vec3::new(pos.x + half_w, pos.y - DEFAULT_TILE_SIZE * 0.5, z),
        Vec3::new(pos.x + half_w, pos.y + half_h, z),
        Vec3::new(pos.x - half_w, pos.y + half_h, z),
    ];
    for i in 0..4 {
        gizmos.line(corners[i], corners[(i + 1) % 4], color);
    }
}

/// System: despawn old lights/emitters for an edited placed object and respawn
/// them with updated properties. Triggered by `pending_respawn_sidecar_id`.
pub fn respawn_edited_children(
    mut commands: Commands,
    mut state: ResMut<TileEditorState>,
    children_q: Query<(Entity, &super::state::SidecarChild)>,
) {
    let Some(sidecar_id) = state.pending_respawn_sidecar_id.take() else {
        return;
    };

    // Despawn all old light/emitter children for this object
    for (entity, child) in &children_q {
        if child.sidecar_id == sidecar_id {
            commands.entity(entity).despawn();
        }
    }

    // Find the placed object definition to respawn from
    let Some(obj_def) = state.placed_objects.iter().find(|o| o.id == sidecar_id).cloned() else {
        return;
    };

    // Respawn lights
    for light_def in &obj_def.properties.lights {
        spawner::spawn_object_light_pub(&mut commands, &obj_def, light_def);
    }

    // Respawn emitters
    for emitter_def in &obj_def.properties.emitters {
        spawner::spawn_object_emitter_pub(&mut commands, &obj_def, emitter_def);
    }

    info!("Respawned lights/emitters for placed object #{sidecar_id}");
}

/// System: respawn lights/emitters for ALL placed objects matching a sprite key.
/// Triggered by `pending_respawn_sprite_key` (set when saving root object from Library mode).
pub fn respawn_by_sprite_key(
    mut commands: Commands,
    mut state: ResMut<TileEditorState>,
    children_q: Query<(Entity, &super::state::SidecarChild)>,
) {
    let Some(sprite_key) = state.pending_respawn_sprite_key.take() else {
        return;
    };

    // Find all placed objects with this sprite key
    let matching: Vec<sidecar::PlacedObjectDef> = state
        .placed_objects
        .iter()
        .filter(|o| o.sprite_key == sprite_key)
        .cloned()
        .collect();

    if matching.is_empty() {
        return;
    }

    // Collect sidecar IDs for bulk despawn
    let matching_ids: Vec<&str> = matching.iter().map(|o| o.id.as_str()).collect();

    // Despawn all existing lights/emitters for these objects
    for (entity, child) in &children_q {
        if matching_ids.contains(&child.sidecar_id.as_str()) {
            commands.entity(entity).despawn();
        }
    }

    // Respawn from updated definitions
    for obj_def in &matching {
        for light_def in &obj_def.properties.lights {
            spawner::spawn_object_light_pub(&mut commands, obj_def, light_def);
        }
        for emitter_def in &obj_def.properties.emitters {
            spawner::spawn_object_emitter_pub(&mut commands, obj_def, emitter_def);
        }
    }

    info!(
        "Respawned lights/emitters for {} instances of sprite_key '{sprite_key}'",
        matching.len()
    );
}

/// System: despawn a placed object and its children (lights/emitters) from ECS.
/// Triggered by `pending_delete_sidecar_id` (set by UI delete button).
pub fn delete_pending_object(
    mut commands: Commands,
    mut state: ResMut<TileEditorState>,
    placed_q: Query<(Entity, &PlacedObject)>,
    children_q: Query<(Entity, &super::state::SidecarChild)>,
) {
    let Some(sidecar_id) = state.pending_delete_sidecar_id.take() else {
        return;
    };

    // Despawn the billboard entity
    if let Some((entity, _)) = placed_q.iter().find(|(_, po)| po.sidecar_id == sidecar_id) {
        commands.entity(entity).despawn();
    }

    // Despawn all associated lights/emitters
    for (entity, child) in &children_q {
        if child.sidecar_id == sidecar_id {
            commands.entity(entity).despawn();
        }
    }

    info!("Deleted placed object #{sidecar_id} and its children from ECS");
}
