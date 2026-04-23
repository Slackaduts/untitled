pub mod definitions;
pub mod emitter;
pub mod gpu_lights;
pub mod particle;
pub mod render;
pub mod shadow;
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
            .init_resource::<shadow::ShadowParticlePool>()
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
                    // Hanabi GPU rendering pipeline.
                    systems::attach_hanabi_effects,
                    systems::sync_emitter_to_hanabi
                        .after(systems::attach_hanabi_effects),
                    // Cull off-screen emitters before spawning.
                    emitter::cull_particle_emitters,
                    // Emitter-level persistent lights.
                    systems::spawn_emitter_lights,
                    systems::update_emitter_light_positions
                        .after(systems::spawn_emitter_lights),
                    systems::cleanup_emitter_lights,
                ),
            )
            .add_systems(
                PostUpdate,
                (
                    // Shadow particles run in PostUpdate AFTER hanabi's tick_spawners,
                    // so we can read EffectSpawner::spawn_count for exact spawn sync.
                    shadow::spawn_shadow_particles
                        .after(EffectSystems::TickSpawners),
                    shadow::update_shadow_particles
                        .after(shadow::spawn_shadow_particles),
                    shadow::upload_shadow_particle_lights
                        .after(shadow::update_shadow_particles),
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
