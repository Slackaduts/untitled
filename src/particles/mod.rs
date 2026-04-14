pub mod definitions;
pub mod emitter;
pub mod systems;

use bevy::prelude::*;

pub struct ParticlePlugin;

impl Plugin for ParticlePlugin {
    fn build(&self, _app: &mut App) {
        // Particle systems will be registered here
    }
}
