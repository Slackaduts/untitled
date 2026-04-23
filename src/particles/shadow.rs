//! Shadow particles: lightweight CPU-side position trackers that mirror hanabi's
//! GPU particles for the per-particle lighting system.
//!
//! These have NO ECS entities — just a flat `Vec<ShadowParticle>` in a resource.
//! The `upload_particle_lights` system reads from this pool instead of querying
//! `LightParticle` entities.

use bevy::prelude::*;

use super::definitions::{
    ColorStop, EmissionDirection, EmissionShape, ParticleRegistry, SizeStop,
    sample_gradient_color, sample_gradient_size,
};
use super::emitter::ParticleEmitter;
use super::render::ParticleLightBudget;

// ── PCG32 RNG (matches hanabi's vfx_common.wgsl) ─────────────────────────────

/// PCG hash matching hanabi's GPU implementation exactly.
fn pcg_hash(input: u32) -> u32 {
    let state = input.wrapping_mul(747796405).wrapping_add(2891336453);
    let word = ((state >> ((state >> 28).wrapping_add(4))) ^ state).wrapping_mul(277803737);
    (word >> 22) ^ word
}

/// Matches hanabi's `to_float01`: extracts mantissa bits to produce [0, 1).
fn to_float01(u: u32) -> f32 {
    f32::from_bits((u & 0x007fffff) | 0x3f800000) - 1.0
}

/// CPU-side PRNG matching hanabi's `frand()`.
struct PcgRng {
    seed: u32,
}

impl PcgRng {
    fn new(seed: u32) -> Self {
        Self { seed }
    }

    /// Single random float in [0, 1). Matches hanabi's frand().
    fn frand(&mut self) -> f32 {
        self.seed = pcg_hash(self.seed);
        to_float01(pcg_hash(self.seed))
    }

    /// Random Vec3 in [0, 1)^3. Matches hanabi's frand3().
    fn frand3(&mut self) -> Vec3 {
        self.seed = pcg_hash(self.seed);
        let x = to_float01(self.seed);
        self.seed = pcg_hash(self.seed);
        let y = to_float01(self.seed);
        self.seed = pcg_hash(self.seed);
        let z = to_float01(self.seed);
        Vec3::new(x, y, z)
    }

    /// Uniform float in [a, b]. Matches hanabi's rand_uniform_f.
    fn uniform_f(&mut self, a: f32, b: f32) -> f32 {
        a + self.frand() * (b - a)
    }
}

// ── Shadow particle data ─────────────────────────────────────────────────────

/// A lightweight particle tracked purely for lighting (no ECS entity).
pub struct ShadowParticle {
    pub position: Vec3,
    pub velocity: Vec3,
    pub age: f32,
    pub lifetime: f32,
    pub gravity: f32,
    pub drag: f32,
    pub color_stops: Vec<ColorStop>,
    pub size_stops: Vec<SizeStop>,
    pub intensity: f32,
    pub light_radius: f32,
    /// The emitter's ground-plane Z at spawn time. Used for lighting Z so
    /// particle lights illuminate billboards at the correct depth regardless
    /// of the particle's actual Z trajectory.
    pub ground_z: f32,
}

/// Pool of all active shadow particles. Read by `upload_particle_lights`.
#[derive(Resource, Default)]
pub struct ShadowParticlePool {
    pub particles: Vec<ShadowParticle>,
    /// Running particle index counter per emitter (for PCG seeding).
    /// Key = emitter entity index.
    spawn_counters: std::collections::HashMap<Entity, u32>,
    /// Per-emitter seed state (advances each frame like hanabi's prng_seed).
    emitter_seeds: std::collections::HashMap<Entity, u32>,
}

// ── Systems ──────────────────────────────────────────────────────────────────

/// Spawns shadow particles for emitters with per-particle light.
/// Replicates hanabi's exact RNG sequence so shadow positions match GPU particles.
///
/// The RNG call order MUST match `to_effect_asset`'s init modifier chain:
///   1. Position init (SetPositionSphereModifier or SetAttributeModifier)
///   2. Lifetime (SetAttributeModifier with uniform)
///   3. Age (constant, no RNG)
///   4. Velocity (SetVelocitySphereModifier or SetAttributeModifier)
///
/// Hanabi advances its spawner seed each frame via:
///   `StdRng::seed_from_u64(old as u64).random::<u32>()`
/// Per particle: `seed = pcg_hash(particle_index ^ spawner_seed)`
pub fn spawn_shadow_particles(
    registry: Res<ParticleRegistry>,
    mut budget: ResMut<ParticleLightBudget>,
    mut pool: ResMut<ShadowParticlePool>,
    emitters: Query<(Entity, &ParticleEmitter, &GlobalTransform, Option<&bevy_hanabi::EffectSpawner>)>,
) {
    for (emitter_entity, emitter, emitter_gtf, spawner) in &emitters {
        if !emitter.active {
            continue;
        }

        let Some(def) = registry.defs.get(&emitter.definition_id) else {
            continue;
        };

        let Some(light_def) = &def.light else {
            continue;
        };

        // Advance seed EVERY frame unconditionally — must match hanabi's
        // tick_spawners which advances compiled_effect.prng_seed every frame
        // regardless of whether particles spawn.
        let bits = emitter_entity.to_bits();
        let default_seed = (bits as u32) ^ ((bits >> 32) as u32);
        let mut frame_seed = *pool.emitter_seeds.entry(emitter_entity).or_insert(default_seed);
        {
            use rand::SeedableRng;
            let mut std_rng = rand::rngs::StdRng::seed_from_u64(frame_seed as u64);
            frame_seed = rand::Rng::r#gen::<u32>(&mut std_rng);
        }
        pool.emitter_seeds.insert(emitter_entity, frame_seed);

        // Read spawn count directly from hanabi's EffectSpawner — this is
        // the exact number of particles hanabi spawns this frame, keeping
        // our particle indices in perfect lockstep.
        let to_spawn = spawner.map_or(0, |s| s.spawn_count) as usize;
        if to_spawn == 0 {
            continue;
        }

        let emitter_pos = emitter_gtf.translation();
        let color_gradient = def.color_gradient();
        let size_gradient = def.size_gradient();
        let is_point_emission = matches!(def.emission_shape, EmissionShape::Point);

        let mut counter = *pool.spawn_counters.entry(emitter_entity).or_insert(0);

        for _ in 0..to_spawn {
            if !budget.try_allocate() {
                break;
            }

            // Per-particle seed: matches hanabi's vfx_init.wgsl line 100.
            let particle_seed = pcg_hash(counter ^ frame_seed);
            counter = counter.wrapping_add(1);
            let mut rng = PcgRng::new(particle_seed);

            // ── RNG calls must match to_effect_asset init order exactly ──

            // 1. Position init (first in .init() chain).
            let offset = match &def.emission_shape {
                EmissionShape::Point => Vec3::ZERO, // lit(ZERO), no RNG
                EmissionShape::Sphere { radius } => {
                    // SetPositionSphereModifier(Volume): frand3() for direction + frand() for radius
                    let raw = rng.frand3() * 2.0 - Vec3::ONE;
                    let len = raw.length().max(0.001);
                    let r = rng.frand() * radius;
                    raw / len * r
                }
                EmissionShape::Box { half_extents } => {
                    // SetAttributeModifier with 3x uniform: 3x frand()
                    let he = *half_extents;
                    Vec3::new(
                        rng.uniform_f(-he[0], he[0]),
                        rng.uniform_f(-he[1], he[1]),
                        rng.uniform_f(-he[2], he[2]),
                    )
                }
                EmissionShape::Ring { radius, width } => {
                    // SetPositionCircleModifier: uses its own RNG calls
                    let theta = rng.frand() * std::f32::consts::TAU;
                    let r = radius + rng.uniform_f(-width * 0.5, *width * 0.5);
                    Vec3::new(r * theta.cos(), r * theta.sin(), 0.0)
                }
            };

            // 2. Lifetime: rand_uniform_f(min, max) → 1 frand().
            let lt_min = def.lifetime.0.min(def.lifetime.1);
            let lt_max = def.lifetime.0.max(def.lifetime.1);
            let lifetime = rng.uniform_f(lt_min, lt_max);

            // 3. Age: lit(0), no RNG.

            // 4. Velocity init (last in .init() chain).
            let velocity = match &def.direction {
                EmissionDirection::Up => {
                    // SetAttributeModifier: vec3(0, 0, uniform(min,max)) → 1 frand()
                    let speed = rng.uniform_f(
                        def.speed_range.0.min(def.speed_range.1),
                        def.speed_range.0.max(def.speed_range.1),
                    );
                    Vec3::new(0.0, 0.0, speed)
                }
                _ if is_point_emission => {
                    // SetAttributeModifier: (rand(VEC3F)*2-1).normalized() * uniform(min,max)
                    // → frand3() (3 advances) then frand() (1 advance)
                    let raw = rng.frand3() * 2.0 - Vec3::ONE;
                    let speed = rng.uniform_f(
                        def.speed_range.0.min(def.speed_range.1),
                        def.speed_range.0.max(def.speed_range.1),
                    );
                    let len = raw.length();
                    if len > 0.001 { raw / len * speed } else { Vec3::Z * speed }
                }
                _ => {
                    // SetVelocitySphereModifier: normalize(POSITION - center) * speed
                    // RNG: just 1 frand() for the speed uniform
                    let speed = rng.uniform_f(
                        def.speed_range.0.min(def.speed_range.1),
                        def.speed_range.0.max(def.speed_range.1),
                    );
                    // Direction from position offset (same as hanabi)
                    let dir = if offset.length_squared() > 0.001 {
                        offset.normalize()
                    } else {
                        Vec3::Z
                    };
                    dir * speed
                }
            };

            pool.particles.push(ShadowParticle {
                position: emitter_pos + offset,
                velocity,
                age: 0.0,
                lifetime,
                gravity: def.gravity,
                drag: def.drag,
                color_stops: color_gradient.clone(),
                size_stops: size_gradient.clone(),
                intensity: light_def.intensity,
                light_radius: light_def.radius,
                ground_z: emitter_pos.z,
            });
        }

        pool.spawn_counters.insert(emitter_entity, counter);
    }
}

/// Updates shadow particle physics and removes expired particles.
pub fn update_shadow_particles(
    time: Res<Time>,
    mut pool: ResMut<ShadowParticlePool>,
    mut budget: ResMut<ParticleLightBudget>,
) {
    let dt = time.delta_secs();

    pool.particles.retain_mut(|p| {
        p.age += dt;
        if p.age >= p.lifetime {
            budget.release();
            return false;
        }

        // Physics (matches hanabi's AccelModifier + LinearDragModifier).
        p.velocity.z -= p.gravity * dt;
        if p.drag > 0.0 {
            let factor = (1.0 - p.drag * dt).max(0.0);
            p.velocity *= factor;
        }
        p.position += p.velocity * dt;
        true
    });
}

/// Uploads shadow particle positions/colors to the GPU light texture.
/// Replaces the old `upload_particle_lights` query over `LightParticle` entities.
pub fn upload_shadow_particle_lights(
    pool: Res<ShadowParticlePool>,
    mut data: ResMut<super::gpu_lights::ParticleLightData>,
) {
    const PARTICLE_TEX_WIDTH: usize = 256;
    let w = PARTICLE_TEX_WIDTH;
    let total_halfs = w * 2 * 4;
    let mut halfs = vec![0u16; total_halfs];
    let max_particles = w - 1;
    let mut count = 0usize;

    for p in &pool.particles {
        if count >= max_particles {
            break;
        }

        let col = count + 1;
        let t = (p.age / p.lifetime).clamp(0.0, 1.0);

        // Scale radius with size gradient.
        let size_scale = sample_gradient_size(&p.size_stops, t);
        let initial_size = p.size_stops.first().map_or(1.0, |s| s.size).max(0.01);
        let radius = p.light_radius * (size_scale / initial_size);

        // Row 0: position + radius.
        // XY follows the particle's actual trajectory; Z uses the emitter's
        // ground plane so lights illuminate billboards at the correct depth
        // (eliminates parallax from the tilted camera).
        let base0 = col * 4;
        halfs[base0]     = half::f16::from_f32(p.position.x).to_bits();
        halfs[base0 + 1] = half::f16::from_f32(p.position.y).to_bits();
        halfs[base0 + 2] = half::f16::from_f32(p.ground_z).to_bits();
        halfs[base0 + 3] = half::f16::from_f32(radius).to_bits();

        // Row 1: color + alpha.
        let c = sample_gradient_color(&p.color_stops, t);
        let base1 = w * 4 + col * 4;
        halfs[base1]     = half::f16::from_f32(c[0] * p.intensity).to_bits();
        halfs[base1 + 1] = half::f16::from_f32(c[1] * p.intensity).to_bits();
        halfs[base1 + 2] = half::f16::from_f32(c[2] * p.intensity).to_bits();
        halfs[base1 + 3] = half::f16::from_f32(c[3]).to_bits();

        count += 1;
    }

    halfs[0] = half::f16::from_f32(count as f32).to_bits();
    data.bytes = bytemuck::cast_slice(&halfs).to_vec();
}
