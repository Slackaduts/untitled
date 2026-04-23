use bevy::prelude::*;
use bevy_hanabi::prelude::*;
use rand::Rng;

use std::collections::HashMap;

use super::definitions::{EmissionDirection, EmissionShape, ParticleRegistry, EmitterLightDef};
use super::emitter::ParticleEmitter;
use super::particle::{LightParticle, ParticleLight};
use super::render::ParticleLightBudget;

/// Shared materials for CPU-rendered emissive particles (one per def, never mutated).
#[derive(Resource, Default)]
pub struct EmissiveParticleMaterials {
    pub by_def: HashMap<String, Handle<StandardMaterial>>,
}

// ── Hanabi GPU effect attachment ──────────────────────────────────────────────

/// Attaches a hanabi `ParticleEffect` to emitters that don't have one yet.
/// Builds an `EffectAsset` from the `ParticleDef` in the registry and inserts
/// it as a component. Hanabi's internal systems then create the GPU pipeline.
pub fn attach_hanabi_effects(
    mut commands: Commands,
    registry: Res<ParticleRegistry>,
    mut effects: ResMut<Assets<EffectAsset>>,
    elev_heights: Res<crate::map::elevation::ElevationHeights>,
    mut emitters: Query<(Entity, &mut ParticleEmitter), Without<ParticleEffect>>,
) {
    for (entity, mut emitter) in &mut emitters {
        if emitter.effect_spawned {
            continue;
        }

        let Some(def) = registry.defs.get(&emitter.definition_id) else {
            continue;
        };

        // Use the terrain's base Z for the kill plane so particles don't clip through ground.
        let ground_z = elev_heights.z_by_level.get(&0).copied().unwrap_or(-1.0);
        let asset = def.to_effect_asset(emitter.rate, emitter.max_particles, ground_z);
        let handle = effects.add(asset);
        // Set a deterministic seed derived from the entity so the shadow
        // particle system can replicate the same RNG sequence.
        let bits = entity.to_bits();
        let seed = (bits as u32) ^ ((bits >> 32) as u32);
        let mut effect = ParticleEffect::new(handle);
        effect.prng_seed = Some(seed);
        commands.entity(entity).insert(effect);
        emitter.effect_spawned = true;
    }
}

/// Syncs the `ParticleEmitter` active state to the hanabi `EffectSpawner`.
pub fn sync_emitter_to_hanabi(
    mut emitters: Query<(&ParticleEmitter, &mut EffectSpawner), Changed<ParticleEmitter>>,
) {
    for (emitter, mut spawner) in &mut emitters {
        spawner.active = emitter.active;
    }
}

// ── CPU emissive particle spawning ───────────────────────────────────────────

/// Spawns invisible light-tracking particles for defs with per-particle light.
/// These have no mesh — they just mirror particle physics for the light texture upload.
/// All visual rendering is handled by hanabi GPU.
pub fn spawn_emissive_particles(
    mut commands: Commands,
    time: Res<Time>,
    registry: Res<ParticleRegistry>,
    mut budget: ResMut<ParticleLightBudget>,
    mut emitters: Query<(Entity, &mut ParticleEmitter, &GlobalTransform)>,
) {
    let mut rng = rand::thread_rng();

    for (emitter_entity, mut emitter, emitter_gtf) in &mut emitters {
        if !emitter.active {
            continue;
        }

        let Some(def) = registry.defs.get(&emitter.definition_id) else {
            continue;
        };

        let light_def = def.light.as_ref();
        let is_self_luminous = light_def.is_some();

        // Only spawn CPU particles for light-emitting defs.
        // Non-light particles are rendered purely by hanabi GPU.
        if !is_self_luminous {
            continue;
        }

        emitter.light_timer.tick(time.delta());
        let to_spawn = emitter.light_timer.times_finished_this_tick() as usize;

        if to_spawn == 0 {
            continue;
        }

        let emitter_pos = emitter_gtf.translation();

        let color_gradient = def.color_gradient();
        let size_gradient = def.size_gradient();

        for _ in 0..to_spawn {
            if !budget.try_allocate() {
                break;
            }

            let (lt_min, lt_max) = (def.lifetime.0.min(def.lifetime.1), def.lifetime.0.max(def.lifetime.1));
            let lifetime = rng.gen_range(lt_min..=lt_max);
            let (sp_min, sp_max) = (def.speed_range.0.min(def.speed_range.1), def.speed_range.0.max(def.speed_range.1));
            let speed = rng.gen_range(sp_min..=sp_max);
            let velocity = emission_velocity(&def.direction, speed, &mut rng);
            let offset = emission_offset(&def.emission_shape, &mut rng);
            let rotation = def
                .rotation_range
                .map(|(a, b)| rng.gen_range(a.min(b)..=a.max(b)))
                .unwrap_or(0.0);
            let angular_velocity = def
                .angular_velocity
                .map(|(a, b)| rng.gen_range(a.min(b)..=a.max(b)))
                .unwrap_or(0.0);

            let spawn_pos = emitter_pos + offset;

            // Light-tracking particles are invisible — hanabi handles visuals.
            // We only need Transform + LightParticle for the light texture upload.
            let mut entity_cmds = commands.spawn((
                Transform::from_translation(spawn_pos),
                LightParticle {
                    age: 0.0,
                    lifetime,
                    velocity,
                    gravity: def.gravity,
                    drag: def.drag,
                    color_stops: color_gradient.clone(),
                    size_stops: size_gradient.clone(),
                    intensity: light_def.map_or(0.0, |l| l.intensity),
                    light_radius: light_def.map_or(0.0, |l| l.radius),
                    rotation,
                    angular_velocity,
                    emitter_entity,
                },
            ));

            // Only light-enabled particles get the ParticleLight marker
            // (tracked for GPU light texture upload).
            if is_self_luminous {
                entity_cmds.insert(ParticleLight);
            }

            emitter.active_count += 1;
        }
    }
}

// ── CPU emissive particle update ─────────────────────────────────────────────

/// Updates light-tracking particles: physics simulation.
/// These particles are invisible (no mesh) — they just track position for the
/// light texture upload. Hanabi handles all visual rendering.
pub fn update_emissive_particles(
    time: Res<Time>,
    mut particles: Query<(&mut LightParticle, &mut Transform)>,
) {
    let dt = time.delta_secs();

    for (mut particle, mut tf) in &mut particles {
        particle.age += dt;

        // Physics.
        particle.velocity.z -= particle.gravity * dt;
        if particle.drag > 0.0 {
            let factor = (1.0 - particle.drag * dt).max(0.0);
            particle.velocity *= factor;
        }
        tf.translation += particle.velocity * dt;
    }
}

/// No-op: light-tracking particles are invisible, hanabi handles visual billboarding.
/// Kept as a function to avoid breaking system registration.
pub fn orient_emissive_particles() {}


// ── CPU emissive particle despawn ────────────────────────────────────────────

pub fn despawn_emissive_particles(
    mut commands: Commands,
    mut budget: ResMut<ParticleLightBudget>,
    particles: Query<(Entity, &LightParticle)>,
    mut emitters: Query<&mut ParticleEmitter>,
) {
    for (entity, particle) in &particles {
        if particle.age >= particle.lifetime {
            commands.entity(entity).despawn();
            budget.release();

            if let Ok(mut emitter) = emitters.get_mut(particle.emitter_entity) {
                emitter.active_count = emitter.active_count.saturating_sub(1);
            }
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn emission_velocity(direction: &EmissionDirection, speed: f32, rng: &mut impl Rng) -> Vec3 {
    match direction {
        EmissionDirection::Sphere => {
            let theta = rng.gen_range(0.0..std::f32::consts::TAU);
            let phi = rng.gen_range(-1.0_f32..1.0).acos();
            Vec3::new(
                phi.sin() * theta.cos(),
                phi.sin() * theta.sin(),
                phi.cos(),
            ) * speed
        }
        EmissionDirection::Cone { angle, direction } => {
            let dir = Vec3::from_array(*direction).normalize_or(Vec3::Z);
            let half_angle = *angle * 0.5;
            let theta = rng.gen_range(0.0..std::f32::consts::TAU);
            let cos_a = rng.gen_range(half_angle.cos()..1.0_f32);
            let sin_a = (1.0 - cos_a * cos_a).sqrt();
            let local = Vec3::new(sin_a * theta.cos(), sin_a * theta.sin(), cos_a);
            let rot = Quat::from_rotation_arc(Vec3::Z, dir);
            rot * local * speed
        }
        EmissionDirection::Up => Vec3::Z * speed,
        EmissionDirection::Ring { .. } => {
            let theta = rng.gen_range(0.0..std::f32::consts::TAU);
            let outward = Vec3::new(theta.cos(), theta.sin(), 0.0);
            (outward + Vec3::Z * 0.3).normalize() * speed
        }
    }
}

fn emission_offset(shape: &EmissionShape, rng: &mut impl Rng) -> Vec3 {
    match shape {
        EmissionShape::Point => Vec3::ZERO,
        EmissionShape::Sphere { radius } => {
            let theta = rng.gen_range(0.0..std::f32::consts::TAU);
            let phi = rng.gen_range(-1.0_f32..1.0).acos();
            let r = rng.gen_range(0.0..*radius);
            Vec3::new(
                r * phi.sin() * theta.cos(),
                r * phi.sin() * theta.sin(),
                r * phi.cos(),
            )
        }
        EmissionShape::Box { half_extents } => Vec3::new(
            rng.gen_range(-half_extents[0]..half_extents[0]),
            rng.gen_range(-half_extents[1]..half_extents[1]),
            rng.gen_range(-half_extents[2]..half_extents[2]),
        ),
        EmissionShape::Ring { radius, width } => {
            let theta = rng.gen_range(0.0..std::f32::consts::TAU);
            let r = radius + rng.gen_range(-width * 0.5..*width * 0.5);
            Vec3::new(r * theta.cos(), r * theta.sin(), 0.0)
        }
    }
}

// ── Emitter lights ──────────────────────────────────────────────────────────

/// Marker for lights spawned from a ParticleDef's `emitter_lights` list.
/// Stores the emitter entity so we can despawn/update when the emitter changes.
#[derive(Component)]
pub struct EmitterLight {
    pub emitter_entity: Entity,
}

/// Marker indicating that emitter lights have already been spawned for this emitter.
#[derive(Component)]
pub struct EmitterLightsSpawned;

/// Spawns persistent LightSource entities for emitters that define `emitter_lights`.
/// Runs once per emitter (gated by `EmitterLightsSpawned` marker).
/// Also despawns old emitter lights when re-triggered (marker removed).
pub fn spawn_emitter_lights(
    mut commands: Commands,
    registry: Res<ParticleRegistry>,
    emitters: Query<(Entity, &ParticleEmitter, &GlobalTransform), Without<EmitterLightsSpawned>>,
    existing_lights: Query<(Entity, &EmitterLight)>,
) {
    for (emitter_entity, emitter, emitter_gtf) in &emitters {
        // Despawn any existing emitter lights for this emitter (handles re-trigger on edit).
        for (light_entity, el) in &existing_lights {
            if el.emitter_entity == emitter_entity {
                commands.entity(light_entity).despawn();
            }
        }

        let Some(def) = registry.defs.get(&emitter.definition_id) else {
            commands.entity(emitter_entity).insert(EmitterLightsSpawned);
            continue;
        };

        commands.entity(emitter_entity).insert(EmitterLightsSpawned);

        if def.emitter_lights.is_empty() {
            continue;
        }

        let pos = emitter_gtf.translation();

        for light_def in &def.emitter_lights {
            spawn_single_emitter_light(&mut commands, emitter_entity, pos, light_def);
        }
    }
}

fn spawn_single_emitter_light(
    commands: &mut Commands,
    emitter_entity: Entity,
    pos: Vec3,
    light_def: &EmitterLightDef,
) {
    use crate::lighting::components::*;

    commands.spawn((
        Transform::from_translation(pos),
        LightSource {
            color: Color::linear_rgb(
                light_def.color[0],
                light_def.color[1],
                light_def.color[2],
            ),
            base_intensity: light_def.intensity,
            intensity: light_def.intensity,
            inner_radius: light_def.radius * 0.3,
            outer_radius: light_def.radius,
            shape: LightShape::Point,
            pulse: if light_def.pulse { Some(PulseConfig::default()) } else { None },
            flicker: if light_def.flicker { Some(FlickerConfig::default()) } else { None },
            anim_seed: rand::random::<f32>() * 100.0,
            ..default()
        },
        EmitterLight { emitter_entity },
    ));
}

/// Keeps emitter light positions in sync with their parent emitter.
pub fn update_emitter_light_positions(
    emitters: Query<&GlobalTransform, With<ParticleEmitter>>,
    mut lights: Query<(&EmitterLight, &mut Transform)>,
) {
    for (el, mut tf) in &mut lights {
        if let Ok(emitter_gtf) = emitters.get(el.emitter_entity) {
            tf.translation = emitter_gtf.translation();
        }
    }
}

/// Despawn emitter lights when their parent emitter is despawned.
pub fn cleanup_emitter_lights(
    mut commands: Commands,
    lights: Query<(Entity, &EmitterLight)>,
    emitters: Query<Entity, With<ParticleEmitter>>,
) {
    for (light_entity, el) in &lights {
        if emitters.get(el.emitter_entity).is_err() {
            commands.entity(light_entity).despawn();
        }
    }
}
