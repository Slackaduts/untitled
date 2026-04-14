pub mod actor;
pub mod inventory;

use bevy::prelude::*;

pub struct EntityPlugin;

impl Plugin for EntityPlugin {
    fn build(&self, _app: &mut App) {
        // Entity-related systems will be registered here
    }
}
