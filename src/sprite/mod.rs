pub mod animation;
pub mod lpc;
pub mod splitter;
pub mod traits;

use bevy::prelude::*;

pub struct SpritePlugin;

impl Plugin for SpritePlugin {
    fn build(&self, _app: &mut App) {
        // Sprite animation systems will be registered here
    }
}
