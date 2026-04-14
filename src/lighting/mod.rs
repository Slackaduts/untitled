pub mod ambient;
pub mod animation;
pub mod components;
#[cfg(feature = "dev_tools")]
pub mod debug_panel;
pub mod emissive;
pub mod pipeline;
pub mod render_node;
pub mod tile_light;
pub mod tiled_spawn;
#[cfg(feature = "dev_tools")]
pub mod tileset_editor;
pub mod time_of_day;
pub mod uniforms;

use bevy::core_pipeline::core_3d::graph::{Core3d, Node3d};
use bevy::prelude::*;
use bevy::render::extract_component::ExtractComponentPlugin;
use bevy::render::render_graph::{RenderGraphExt, ViewNodeRunner};
use bevy::render::render_resource::StorageBuffer;
use bevy::render::{Render, RenderApp, RenderSystems};

use ambient::AmbientConfig;
use components::LightSource;
use render_node::{LightingNode, LightingNodeLabel};
use time_of_day::TimeOfDay;
use uniforms::{ExtractedLightData, LightUniformBuffer, LightUniformData, LightingPostProcess};

pub struct LightingPlugin;

impl Plugin for LightingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AmbientConfig>()
            .init_resource::<TimeOfDay>()
            .register_type::<LightSource>()
            .add_plugins(ExtractComponentPlugin::<LightingPostProcess>::default())
            .add_systems(
                Update,
                (
                    time_of_day::advance_time_of_day,
                    time_of_day::compute_ambient_from_time
                        .after(time_of_day::advance_time_of_day),
                    animation::animate_lights,
                    emissive::sync_emissive_links,
                    tiled_spawn::spawn_lights_from_tiled,
                    tile_light::spawn_lights_from_tile_properties,
                    tile_light::parent_tile_lights_to_billboards
                        .after(tile_light::spawn_lights_from_tile_properties),
                ),
            );

        #[cfg(feature = "dev_tools")]
        {
            use debug_panel::LightingDebugPanel;
            app.init_resource::<LightingDebugPanel>()
                .init_resource::<tileset_editor::TilesetEditorState>()
                .add_systems(
                    Update,
                    (
                        debug_panel::toggle_debug_panel,
                        debug_panel::lighting_debug_ui
                            .after(debug_panel::toggle_debug_panel),
                        debug_panel::place_light_on_click
                            .after(debug_panel::lighting_debug_ui),
                        debug_panel::draw_light_gizmos
                            .after(debug_panel::lighting_debug_ui),
                        debug_panel::tileset_editor_system
                            .after(debug_panel::lighting_debug_ui),
                    ),
                );
        }
    }

    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app
            .init_resource::<ExtractedLightData>()
            .insert_resource(LightUniformBuffer {
                buffer: StorageBuffer::<LightUniformData>::default(),
            })
            .init_resource::<pipeline::LightingPipeline>()
            .add_systems(
                ExtractSchedule,
                uniforms::extract_light_data,
            )
            .add_systems(
                Render,
                uniforms::prepare_light_uniform
                    .in_set(RenderSystems::Prepare),
            )
            .add_render_graph_node::<ViewNodeRunner<LightingNode>>(Core3d, LightingNodeLabel)
            .add_render_graph_edges(
                Core3d,
                (Node3d::EndMainPass, LightingNodeLabel, Node3d::Tonemapping),
            );
    }
}
