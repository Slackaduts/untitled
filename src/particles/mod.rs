pub mod definitions;
pub mod emitter;
pub mod gpu_lights;
pub mod particle;
pub mod render;
pub mod systems;
#[cfg(feature = "dev_tools")]
pub mod editor;

use bevy::prelude::*;
use bevy_hanabi::prelude::*;

use definitions::ParticleRegistry;
use gpu_lights::ParticleLitMaterial;
use render::ParticleLightBudget;

pub struct ParticlePlugin;

impl Plugin for ParticlePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(HanabiPlugin)
            .add_plugins(MaterialPlugin::<ParticleLitMaterial>::default())
            .init_resource::<ParticleRegistry>()
            .init_resource::<ParticleLightBudget>()
            .init_resource::<systems::EmissiveParticleMaterials>()
            .add_systems(
                Startup,
                (
                    definitions::load_particle_defs,
                    render::setup_particle_meshes,
                    gpu_lights::setup_particle_light_buffer,
                ),
            )
            .add_systems(
                Update,
                (
                    // Hanabi path (non-emissive particles).
                    systems::attach_hanabi_effects,
                    systems::sync_emitter_to_hanabi
                        .after(systems::attach_hanabi_effects),
                    // Cull off-screen emitters before spawning.
                    emitter::cull_particle_emitters,
                    // CPU emissive particles (visible + light tracking).
                    systems::spawn_emissive_particles
                        .after(systems::attach_hanabi_effects)
                        .after(emitter::cull_particle_emitters),
                    systems::update_emissive_particles
                        .after(systems::spawn_emissive_particles),
                    systems::orient_emissive_particles
                        .after(systems::update_emissive_particles),
                    systems::despawn_emissive_particles
                        .after(systems::update_emissive_particles),
                    // GPU buffer upload — packs all particle positions for shaders.
                    gpu_lights::upload_particle_lights
                        .after(systems::update_emissive_particles),
                ),
            );

        // Register render-world systems for direct GPU texture writes.
        gpu_lights::register_render_systems(app);

        #[cfg(feature = "dev_tools")]
        {
            app.init_resource::<editor::ParticleEditorState>()
                .add_systems(
                    Update,
                    (
                        editor::toggle_particle_editor,
                        editor::particle_editor_ui
                            .after(editor::toggle_particle_editor)
                            .run_if(|state: Res<editor::ParticleEditorState>| state.open),
                        editor::place_emitter_on_click
                            .after(editor::particle_editor_ui)
                            .run_if(|state: Res<editor::ParticleEditorState>| state.open),
                        editor::draw_emitter_gizmos
                            .after(editor::particle_editor_ui)
                            .run_if(|state: Res<editor::ParticleEditorState>| state.open),
                        editor::particle_editor_preview
                            .after(editor::particle_editor_ui)
                            .run_if(|state: Res<editor::ParticleEditorState>| state.open),
                    ),
                );
        }
    }
}
