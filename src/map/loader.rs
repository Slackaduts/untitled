use bevy::prelude::*;

/// Marker for the currently loaded map entity.
#[derive(Component)]
pub struct ActiveMap;

/// Resource tracking the current map path.
#[derive(Resource, Default)]
pub struct CurrentMap {
    pub path: Option<String>,
}
