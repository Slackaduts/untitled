pub mod discovery;
pub mod loader;
pub mod serializer;
pub mod slots;

use bevy::prelude::*;

pub struct SavePlugin;

impl Plugin for SavePlugin {
    fn build(&self, _app: &mut App) {
        // Save/load systems will be registered here
    }
}
