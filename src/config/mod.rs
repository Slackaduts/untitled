pub mod display;
pub mod keybinds;

use bevy::prelude::*;

pub struct ConfigPlugin;

impl Plugin for ConfigPlugin {
    fn build(&self, _app: &mut App) {
        // Config loading systems will be registered here
    }
}
