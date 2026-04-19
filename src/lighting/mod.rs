pub mod ambient;
pub mod animation;
pub mod components;
#[cfg(feature = "dev_tools")]
pub mod debug_panel;
pub mod emissive;
pub mod tile_light;
pub mod tiled_spawn;
pub mod time_of_day;

use bevy::prelude::*;

use components::LightSource;
use time_of_day::TimeOfDay;

/// Condition: true when tile layers exist that haven't had lights spawned yet.
fn has_unprocessed_tile_lights(
    q: Query<(), (With<bevy_ecs_tiled::prelude::TiledTilemap>, Without<tile_light::TileLightsProcessed>)>,
) -> bool {
    !q.is_empty()
}

pub struct LightingPlugin;

impl Plugin for LightingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TimeOfDay>()
            .register_type::<LightSource>()
            .add_systems(Startup, time_of_day::spawn_sun_light)
            .add_systems(
                Update,
                (
                    time_of_day::advance_time_of_day,
                    time_of_day::compute_ambient_from_time
                        .after(time_of_day::advance_time_of_day),
                    time_of_day::update_sun_light
                        .after(time_of_day::compute_ambient_from_time),
                    animation::animate_lights,
                    components::sync_light_components
                        .after(animation::animate_lights),
                    components::cull_lights
                        .after(components::sync_light_components),
                    emissive::sync_emissive_links,
                    tiled_spawn::spawn_lights_from_tiled,
                    tile_light::spawn_lights_from_tile_properties
                        .run_if(has_unprocessed_tile_lights),
                    tile_light::parent_tile_lights_to_billboards
                        .after(tile_light::spawn_lights_from_tile_properties),
                ),
            );

        #[cfg(feature = "dev_tools")]
        {
            use debug_panel::LightingDebugPanel;
            app.init_resource::<LightingDebugPanel>()
                .add_systems(
                    Update,
                    (
                        debug_panel::toggle_debug_panel,
                        debug_panel::lighting_debug_ui
                            .after(debug_panel::toggle_debug_panel)
                            .run_if(|panel: Res<LightingDebugPanel>| panel.open),
                        debug_panel::place_light_on_click
                            .after(debug_panel::lighting_debug_ui)
                            .run_if(|panel: Res<LightingDebugPanel>| panel.open),
                        debug_panel::draw_light_gizmos
                            .after(debug_panel::lighting_debug_ui)
                            .run_if(|panel: Res<LightingDebugPanel>| panel.open),
                    ),
                );
        }
    }
}
