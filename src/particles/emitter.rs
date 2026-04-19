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
