pub mod animation;
pub mod lpc;
pub mod splitter;
pub mod traits;

use bevy::prelude::*;

pub struct SpritePlugin;

impl Plugin for SpritePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                animation::tick_animations,
                animation::update_sprite_uvs
                    .after(animation::tick_animations),
            ),
        );
    }
}
