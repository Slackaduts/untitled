use bevy::prelude::*;
use bevy_hanabi::prelude::*;
use rand::Rng;

use std::collections::HashMap;

use super::definitions::{EmissionDirection, EmissionShape, ParticleBlend, ParticleRegistry};
use super::emitter::ParticleEmitter;
use super::particle::{LightParticle, ParticleLight};
use super::render::{ParticleLightBudget, ParticleMeshes};

/// Shared materials for CPU-rendered emissive particles (one per def, never mutated).
#[derive(Resource, Default)]
pub struct EmissiveParticleMaterials {
    pub by_def: HashMap<String, Handle<StandardMaterial>>,
}

// ── Hanabi effect attachment (non-emissive particles only) ────────────────────

/// All particles use CPU rendering: non-light particles get `unlit: false`
/// (scene-lit), light particles get `unlit: true` (self-luminous).
/// Hanabi is not used because it has no PBR lighting support.
pub fn attach_hanabi_effects(
    mut emitters: Query<&mut ParticleEmitter, Without<ParticleEffect>>,
) {
    for mut emitter in &mut emitters {
        if !emitter.effect_spawned {
            emitter.effect_spawned = true;
        }
    }
}

pub fn sync_emitter_to_hanabi(
    mut emitters: Query<(&ParticleEmitter, &mut EffectSpawner), Changed<ParticleEmitter>>,
) {
    for (emitter, mut spawner) in &mut emitters {
        spawner.active = emitter.active;
    }
}

// ── CPU emissive particle spawning ───────────────────────────────────────────

/// Spawns CPU-rendered emissive particles with a shared material per def.
/// The same entity serves as both the visible sprite and the light position tracker.
pub fn spawn_emissive_particles(
    mut commands: Commands,
    time: Res<Time>,
    registry: Res<ParticleRegistry>,
    meshes: Res<ParticleMeshes>,
    mut budget: ResMut<ParticleLightBudget>,
    mut shared_mats: ResMut<EmissiveParticleMaterials>,
    mut materials: ResMut<Assets<StandardMaterial>>,
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

        emitter.light_timer.tick(time.delta());
        let to_spawn = emitter.light_timer.times_finished_this_tick() as usize;

        if to_spawn == 0 {
            continue;
        }

        let emitter_pos = emitter_gtf.translation();

        let color_start = LinearRgba::new(
            def.color_start[0],
            def.color_start[1],
            def.color_start[2],
            def.color_start[3],
        );
        let color_end = LinearRgba::new(
            def.color_end[0],
            def.color_end[1],
            def.color_end[2],
            def.color_end[3],
        );

        // Get or create shared material for this particle def (never mutated per-frame).
        // Self-luminous (has light) → unlit: true. Scene-lit (no light) → unlit: false.
        let mat_handle = shared_mats
            .by_def
            .entry(emitter.definition_id.clone())
            .or_insert_with(|| {
                let alpha_mode = match def.blend_mode {
                    ParticleBlend::Additive => bevy::prelude::AlphaMode::Add,
                    ParticleBlend::Alpha => bevy::prelude::AlphaMode::Blend,
                };
                materials.add(StandardMaterial {
                    base_color: Color::linear_rgba(
                        color_start.red,
                        color_start.green,
                        color_start.blue,
                        color_start.alpha,
                    ),
                    unlit: is_self_luminous,
                    alpha_mode,
                    double_sided: true,
                    cull_mode: None,
                    ..default()
                })
            })
            .clone();

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

            let mut entity_cmds = commands.spawn((
                Mesh3d(meshes.quad.clone()),
                MeshMaterial3d(mat_handle.clone()),
                Transform::from_translation(spawn_pos)
                    .with_scale(Vec3::splat(def.size_start)),
                Visibility::default(),
                LightParticle {
                    age: 0.0,
                    lifetime,
                    velocity,
                    gravity: def.gravity,
                    drag: def.drag,
                    color_start,
                    color_end,
                    size_start: def.size_start,
                    size_end: def.size_end,
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

/// Updates emissive particles: physics + size interpolation.
/// No material mutation — shared material is never touched per-frame.
pub fn update_emissive_particles(
    time: Res<Time>,
    mut particles: Query<(&mut LightParticle, &mut Transform)>,
) {
    let dt = time.delta_secs();

    for (mut particle, mut tf) in &mut particles {
        particle.age += dt;

        let t = (particle.age / particle.lifetime).clamp(0.0, 1.0);
        let size = lerp(particle.size_start, particle.size_end, t);

        // Physics.
        particle.velocity.z -= particle.gravity * dt;
        if particle.drag > 0.0 {
            let factor = (1.0 - particle.drag * dt).max(0.0);
            particle.velocity *= factor;
        }
        tf.translation += particle.velocity * dt;
        tf.scale = Vec3::splat(size);
    }
}

/// Billboard CPU particles toward the camera.
pub fn orient_emissive_particles(
    cameras: Query<&GlobalTransform, With<crate::camera::CombatCamera3d>>,
    mut particles: Query<&mut Transform, With<LightParticle>>,
) {
    let Ok(cam_gtf) = cameras.single() else {
        return;
    };
    let cam_pos = cam_gtf.translation();

    for mut tf in &mut particles {
        let dir = cam_pos - tf.translation;
        if dir.length_squared() > 0.001 {
            let look_rot = Transform::from_translation(tf.translation)
                .looking_at(cam_pos, Vec3::Z)
                .rotation;
            tf.rotation = look_rot;
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

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
