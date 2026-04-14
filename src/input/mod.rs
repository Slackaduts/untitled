pub mod context;
pub mod mapping;

use bevy::prelude::*;

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<context::InputContext>();
    }
}
