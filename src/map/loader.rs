use bevy::prelude::*;

/// Marker for the currently loaded map entity.
#[derive(Component)]
pub struct ActiveMap;

/// Resource tracking the current map path.
#[derive(Resource, Default)]
pub struct CurrentMap {
    /// Path to the TMX file (e.g. "assets/maps/test.tmx").
    pub path: Option<String>,
    /// Root entity of the spawned TiledMap (for despawning).
    pub entity: Option<Entity>,
}

/// Resource inserted to request a map transition.
/// Systems watching for this resource will perform the transition.
#[derive(Resource)]
pub struct MapTransitionRequest {
    /// Target map path (e.g. "assets/maps/interior.tmx").
    pub target_map: String,
    /// Spawn position in the target map (tile grid coordinates).
    pub spawn_point: IVec2,
    /// Optional Lua script path for custom transition effects.
    pub script: Option<String>,
}
