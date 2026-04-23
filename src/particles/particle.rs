use bevy::prelude::*;

use super::definitions::{ColorStop, SizeStop};

/// A CPU-rendered emissive particle with a visual mesh.
/// Lighting is evaluated in shaders via a GPU storage buffer (see `gpu_lights`).
#[derive(Component)]
pub struct LightParticle {
    pub age: f32,
    pub lifetime: f32,
    pub velocity: Vec3,
    pub gravity: f32,
    pub drag: f32,
    pub color_stops: Vec<ColorStop>,
    pub size_stops: Vec<SizeStop>,
    pub intensity: f32,
    pub light_radius: f32,
    pub rotation: f32,
    pub angular_velocity: f32,
    pub emitter_entity: Entity,
}

/// Marker for light particles.
#[derive(Component)]
pub struct ParticleLight;
