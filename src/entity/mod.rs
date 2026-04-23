pub mod actor;
pub mod inventory;
pub mod movement;

use bevy::prelude::*;

pub struct EntityPlugin;

impl Plugin for EntityPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, movement::overworld_movement_system);
    }
}
