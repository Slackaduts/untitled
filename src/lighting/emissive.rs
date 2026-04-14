use bevy::prelude::*;

use super::components::LightSource;
use crate::particles::emitter::ParticleEmitter;

/// Links a particle emitter to a light source, scaling intensity with emitter activity.
#[derive(Component)]
pub struct EmissiveLink {
    pub light_entity: Entity,
    pub intensity_scale: f32,
}

pub fn sync_emissive_links(
    emitters: Query<(&EmissiveLink, &ParticleEmitter)>,
    mut lights: Query<&mut LightSource>,
) {
    for (link, emitter) in &emitters {
        if let Ok(mut light) = lights.get_mut(link.light_entity) {
            light.intensity = if emitter.active {
                link.intensity_scale
            } else {
                0.0
            };
        }
    }
}
