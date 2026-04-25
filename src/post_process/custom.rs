use bevy::prelude::*;
use bevy::core_pipeline::core_3d::graph::{Core3d, Node3d};
use bevy::core_pipeline::fullscreen_material::FullscreenMaterial;
use bevy::render::extract_component::ExtractComponent;
use bevy::render::render_graph::{InternedRenderLabel, InternedRenderSubGraph, RenderLabel, RenderSubGraph};
use bevy::render::render_resource::ShaderType;
use bevy::shader::ShaderRef;

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub struct CustomPostProcessLabel;

/// Uniform data for custom post-processing effects that Bevy doesn't provide natively.
///
/// Each `Vec4` packs related parameters together for GPU-friendly alignment.
/// Attach this component to a camera entity to enable custom post-processing.
#[derive(Component, ExtractComponent, Clone, Copy, ShaderType, Default)]
#[extract_component_filter(With<Camera>)]
pub struct CustomPostProcess {
    /// Vignette: rgb = edge color, w = intensity (0 = off)
    pub vignette_color: Vec4,
    /// Vignette: x = smoothness, y = roundness (aspect correction)
    pub vignette_params: Vec4,
    /// Pixelation: x = cell_width, y = cell_height, z = enabled (>0.5)
    pub pixelation_params: Vec4,
    /// Scanlines: x = intensity, y = line count, z = scroll speed
    pub scanline_params: Vec4,
    /// Film grain: x = intensity, y = speed multiplier
    pub grain_params: Vec4,
    /// Fade overlay: rgb = color, w = intensity (0 = off, 1 = solid)
    pub fade_color: Vec4,
    /// Color tint: rgb = tint color, w = mix intensity
    pub color_tint: Vec4,
    /// x = invert (0-1 mix), y = brightness (additive), z = contrast (1=neutral), w = saturation (1=neutral)
    pub misc_params: Vec4,
    /// x = elapsed time, y = resolution_x, z = resolution_y, w = master enable (>0.5)
    pub time_resolution: Vec4,
    /// Sine wave: x = amplitude_x, y = amplitude_y, z = frequency, w = speed
    pub sine_wave: Vec4,
    /// Swirl: x = angle (radians), y = radius (0-1 UV), z = center_x, w = center_y
    pub swirl: Vec4,
    /// x = lens_distortion_intensity, y = lens_distortion_zoom, z = shake_intensity, w = shake_speed
    pub distortion_shake: Vec4,
    /// x = zoom (1.0=none), y = rotation (radians), z = posterize_levels (0=off), w = cinema_bar_size (0=off)
    pub zoom_rotation: Vec4,
    /// Cinema bar color: rgb, w = unused
    pub cinema_bar_color: Vec4,
    /// Shockwave 0: x = center_u, y = center_v, z = radius (UV), w = intensity
    pub shockwave_0: Vec4,
    /// Shockwave 0 extra: x = thickness (UV), y = chromatic_split
    pub shockwave_0_extra: Vec4,
    /// Shockwave 1
    pub shockwave_1: Vec4,
    pub shockwave_1_extra: Vec4,
    /// Shockwave 2
    pub shockwave_2: Vec4,
    pub shockwave_2_extra: Vec4,
    /// Shockwave 3
    pub shockwave_3: Vec4,
    pub shockwave_3_extra: Vec4,
}

impl FullscreenMaterial for CustomPostProcess {
    fn fragment_shader() -> ShaderRef {
        "shaders/post_process_custom.wgsl".into()
    }

    fn node_edges() -> Vec<InternedRenderLabel> {
        // Run after all native post-processing (tonemapping → FXAA → CAS → us)
        vec![
            Node3d::ContrastAdaptiveSharpening.intern(),
            CustomPostProcessLabel.intern(),
            Node3d::EndMainPassPostProcessing.intern(),
        ]
    }

    fn sub_graph() -> Option<InternedRenderSubGraph> {
        Some(Core3d.intern())
    }

    fn node_label() -> impl RenderLabel {
        CustomPostProcessLabel
    }
}

impl CustomPostProcess {
    /// Create with master enable on and neutral defaults (no visible effects).
    pub fn enabled() -> Self {
        Self {
            vignette_params: Vec4::new(0.5, 1.0, 0.0, 0.0),
            pixelation_params: Vec4::new(4.0, 4.0, 0.0, 0.0),
            color_tint: Vec4::new(1.0, 1.0, 1.0, 0.0),
            misc_params: Vec4::new(0.0, 0.0, 1.0, 1.0),
            time_resolution: Vec4::new(0.0, 0.0, 0.0, 1.0),
            distortion_shake: Vec4::new(0.0, 1.0, 0.0, 0.0), // lens_zoom=1.0
            zoom_rotation: Vec4::new(1.0, 0.0, 0.0, 0.0),    // zoom=1.0
            ..default()
        }
    }
}
