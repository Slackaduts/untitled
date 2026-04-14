use bevy::prelude::*;

/// Particle definition loaded from Lua data files.
#[derive(Debug, Clone)]
pub struct ParticleDef {
    pub id: String,
    pub lifetime: f32,
    pub speed_range: (f32, f32),
    pub color_start: Color,
    pub color_end: Color,
    pub size_start: f32,
    pub size_end: f32,
    pub gravity: f32,
}

/// Registry of all loaded particle definitions.
#[derive(Resource, Default)]
pub struct ParticleRegistry {
    pub defs: Vec<ParticleDef>,
}
