pub mod custom;
#[cfg(feature = "dev_tools")]
pub mod editor;
pub mod shockwave;
pub mod transition;

use bevy::prelude::*;
use bevy::core_pipeline::fullscreen_material::FullscreenMaterialPlugin;
use bevy::render::view::ColorGrading;
use custom::CustomPostProcess;

pub struct PostProcessPlugin;

impl Plugin for PostProcessPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FullscreenMaterialPlugin::<CustomPostProcess>::default());
        app.init_resource::<transition::FxTransitions>();
        app.add_systems(
            Update,
            (
                warm_color_grading_pipeline,
                update_custom_post_process_uniforms,
                transition::tick_fx_transitions,
                shockwave::tick_shockwaves,
            ),
        );

        #[cfg(feature = "dev_tools")]
        {
            app.init_resource::<editor::PostProcessEditorState>();
            app.add_systems(
                Update,
                (
                    editor::toggle_editor,
                    editor::post_process_editor_ui.after(editor::toggle_editor),
                ),
            );
        }
    }
}

/// Bevy's tonemapping shader uses `#ifdef` specialization to skip unused color
/// grading features (white balance, hue rotation, sectional grading). When the
/// camera starts with all-default values, those features are compiled out. The
/// first time a user changes temperature/hue/sections, the shader must be
/// recompiled — which takes multiple frames of broken rendering.
///
/// Fix: on camera creation, set imperceptibly-small non-default values so the
/// full shader variant is compiled from the start.
fn warm_color_grading_pipeline(mut q: Query<&mut ColorGrading, Added<ColorGrading>>) {
    for mut cg in q.iter_mut() {
        cg.global.hue = f32::MIN_POSITIVE;
        cg.global.temperature = f32::MIN_POSITIVE;
        cg.shadows.saturation = 1.0 + f32::EPSILON;
    }
}

/// Pump elapsed time and window resolution into the custom shader uniform each frame.
fn update_custom_post_process_uniforms(
    time: Res<Time>,
    windows: Query<&Window>,
    mut query: Query<&mut CustomPostProcess>,
) {
    let Ok(window) = windows.single() else { return };
    let res_x = window.physical_width() as f32;
    let res_y = window.physical_height() as f32;
    let t = time.elapsed_secs();

    for mut pp in query.iter_mut() {
        pp.time_resolution.x = t;
        pp.time_resolution.y = res_x;
        pp.time_resolution.z = res_y;
    }
}
