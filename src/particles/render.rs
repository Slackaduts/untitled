use bevy::prelude::*;

/// Shared mesh handles for CPU-rendered emissive particles.
/// Only used for ParticleDefs that have lights (hanabi handles non-emissive).
#[derive(Resource)]
pub struct ParticleMeshes {
    /// A 1x1 unit quad centered at origin.
    pub quad: Handle<Mesh>,
}

impl Default for ParticleMeshes {
    fn default() -> Self {
        Self {
            quad: Handle::default(),
        }
    }
}

/// Global budget for CPU-side particle lights.
#[derive(Resource)]
pub struct ParticleLightBudget {
    pub max: u32,
    pub current: u32,
}

impl Default for ParticleLightBudget {
    fn default() -> Self {
        Self {
            max: 256,
            current: 0,
        }
    }
}

impl ParticleLightBudget {
    pub fn try_allocate(&mut self) -> bool {
        if self.current < self.max {
            self.current += 1;
            true
        } else {
            false
        }
    }

    pub fn release(&mut self) {
        self.current = self.current.saturating_sub(1);
    }
}

/// Creates the shared particle quad mesh at startup.
pub fn setup_particle_meshes(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    let quad = meshes.add(Rectangle::new(1.0, 1.0));
    commands.insert_resource(ParticleMeshes { quad });
}
