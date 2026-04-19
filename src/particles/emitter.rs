use bevy::prelude::*;

/// A particle emitter that drives both a hanabi GPU effect and CPU light particles.
#[derive(Component)]
pub struct ParticleEmitter {
    /// Which `ParticleDef` to use (key into `ParticleRegistry`).
    pub definition_id: String,
    /// Particles spawned per second (visual rate for hanabi).
    pub rate: f32,
    /// If set, spawn this many particles immediately then deactivate (one-shot burst).
    pub burst: Option<u32>,
    /// Maximum live particles for the GPU effect.
    pub max_particles: u32,
    /// Spawn timer — ticks at the same rate as the visual particles (1:1 mapping).
    pub light_timer: Timer,
    /// Whether the emitter is currently spawning.
    pub active: bool,
    /// Current number of live CPU light particles from this emitter.
    pub active_count: u32,
    /// If true, particles live in world space; if false, they follow the emitter's transform.
    pub world_space: bool,
    /// Whether the hanabi EffectAsset has been created and attached to this entity.
    pub effect_spawned: bool,
}

impl Default for ParticleEmitter {
    fn default() -> Self {
        Self {
            definition_id: String::new(),
            rate: 10.0,
            burst: None,
            max_particles: 256,
            light_timer: Timer::from_seconds(0.1, TimerMode::Repeating),
            active: true,
            active_count: 0,
            world_space: true,
            effect_spawned: false,
        }
    }
}

impl ParticleEmitter {
    pub fn new(definition_id: impl Into<String>, rate: f32) -> Self {
        // Light particles spawn at the SAME rate as visual particles (1:1).
        let interval = if rate > 0.0 { 1.0 / rate } else { 1.0 };
        Self {
            definition_id: definition_id.into(),
            rate,
            light_timer: Timer::from_seconds(interval, TimerMode::Repeating),
            ..default()
        }
    }

    pub fn with_burst(mut self, count: u32) -> Self {
        self.burst = Some(count);
        self
    }

    pub fn with_max_particles(mut self, max: u32) -> Self {
        self.max_particles = max;
        self
    }
}

/// Marker: this emitter was deactivated by the culling system, not by game logic.
/// Used to distinguish culling-off from intentionally-off emitters.
#[derive(Component)]
pub struct CulledEmitter;

const PARTICLE_CULL_MARGIN: f32 = 100.0;

/// Deactivates emitters outside the camera's visible area and reactivates
/// them when they come back into range.
pub fn cull_particle_emitters(
    mut commands: Commands,
    rect: Res<crate::camera::visible_rect::CameraVisibleRect>,
    mut emitters: Query<(Entity, &GlobalTransform, &mut ParticleEmitter, Option<&CulledEmitter>)>,
) {
    for (entity, gtf, mut emitter, culled) in &mut emitters {
        let pos = gtf.translation().truncate();
        let in_range = rect.contains_point(pos, PARTICLE_CULL_MARGIN);

        if in_range {
            if culled.is_some() {
                emitter.active = true;
                commands.entity(entity).remove::<CulledEmitter>();
            }
        } else if emitter.active && culled.is_none() {
            emitter.active = false;
            commands.entity(entity).insert(CulledEmitter);
        }
    }
}
