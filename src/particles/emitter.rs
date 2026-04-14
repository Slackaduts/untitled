use bevy::prelude::*;

/// A particle emitter that spawns particles from a pool.
#[derive(Component)]
pub struct ParticleEmitter {
    pub definition_id: String,
    pub rate: f32,
    pub timer: Timer,
    pub active: bool,
}
