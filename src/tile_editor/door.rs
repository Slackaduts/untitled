//! Door/portal system: placement, trigger detection, map transitions.

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::camera::follow::CameraTarget;
use crate::map::loader::MapTransitionRequest;
use crate::map::DEFAULT_TILE_SIZE;

/// Component marking a placed object as a door/portal.
/// When the player walks into the door's sensor collider,
/// a map transition is triggered.
#[derive(Component, Clone)]
pub struct DoorPortal {
    /// Target map path relative to assets (e.g. "maps/interior.tmx").
    pub target_map: String,
    /// Spawn position in the target map (tile grid coordinates).
    pub spawn_point: IVec2,
    /// Optional Lua script path for custom transition effects.
    pub script: Option<String>,
}

/// Marker for the door's sensor collider entity (child of the door billboard).
#[derive(Component)]
pub struct DoorSensor;

/// Detect player collision with door sensors and trigger map transitions.
pub fn door_trigger_system(
    mut commands: Commands,
    doors: Query<(&DoorPortal, &CollidingEntities), With<DoorSensor>>,
    player: Query<Entity, With<CameraTarget>>,
    existing_request: Option<Res<MapTransitionRequest>>,
) {
    // Don't trigger if a transition is already in progress
    if existing_request.is_some() {
        return;
    }

    let Ok(player_entity) = player.single() else {
        return;
    };

    for (door, colliding) in &doors {
        if colliding.contains(&player_entity) {
            info!(
                "Door triggered: transitioning to {} at ({}, {})",
                door.target_map, door.spawn_point.x, door.spawn_point.y
            );

            commands.insert_resource(MapTransitionRequest {
                target_map: door.target_map.clone(),
                spawn_point: door.spawn_point,
                script: door.script.clone(),
            });
            return;
        }
    }
}

/// Spawn a sensor collider for a door entity.
/// Called by the spawner when a placed object has door data.
pub fn spawn_door_sensor(
    commands: &mut Commands,
    door_entity: Entity,
    door: &DoorPortal,
) {
    // Sensor collider covering one tile
    let sensor_size = DEFAULT_TILE_SIZE;
    commands.entity(door_entity).with_children(|parent| {
        parent.spawn((
            Collider::cuboid(sensor_size, sensor_size, sensor_size),
            Sensor,
            DoorSensor,
            door.clone(),
        ));
    });
}
