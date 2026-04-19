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
            if !emitter.active || emitter.active_count == 0 {
                light.intensity = 0.0;
            } else {
                // Modulate intensity by particle count ratio for natural
                // glow behavior (starting fire = dim, full blaze = bright).
                let count_factor = (emitter.active_count as f32
                    / emitter.max_particles.max(1) as f32)
                    .clamp(0.0, 1.0);
                light.base_intensity = link.intensity_scale * count_factor;
            }
        }
    }
}
