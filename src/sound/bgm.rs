use bevy::prelude::*;

/// Tracks the currently playing background music.
#[derive(Resource, Default)]
pub struct BgmState {
    pub current_track: Option<String>,
    pub entity: Option<Entity>,
}
