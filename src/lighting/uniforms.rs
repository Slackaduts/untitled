use bevy::prelude::*;
use bevy::render::extract_component::ExtractComponent;
use bevy::render::render_resource::{ShaderType, StorageBuffer};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::Extract;

use super::ambient::AmbientConfig;
use super::components::{LightShape, LightSource};

/// Max lights per frame.
pub const MAX_LIGHTS: usize = 128;

// ── GPU structs (must match WGSL layout) ────────────────────────────────────

/// Light type constants (mirrored in shader).
pub const LIGHT_TYPE_POINT: u32 = 0;
pub const LIGHT_TYPE_CONE: u32 = 1;
pub const LIGHT_TYPE_LINE: u32 = 2;
pub const LIGHT_TYPE_CAPSULE: u32 = 3;

#[derive(ShaderType, Clone, Copy, Default)]
pub struct GpuLight {
    pub position: Vec2,
    pub inner_radius: f32,
    pub outer_radius: f32,
    pub color: Vec3,
    pub intensity: f32,
    pub light_type: u32,
    pub shape_param: f32,
    pub direction_or_end2: Vec2,
}

#[derive(ShaderType, Clone, Default)]
pub struct LightUniformData {
    pub ambient_color: Vec3,
    pub ambient_intensity: f32,
    pub screen_size: Vec2,
    pub num_lights: u32,
    pub _pad: u32,
    #[size(runtime)]
    pub lights: Vec<GpuLight>,
}

// ── Extracted data (main world → render world) ──────────────────────────────

#[derive(Resource, Default)]
pub struct ExtractedLightData {
    pub ambient_color: Vec3,
    pub ambient_intensity: f32,
    pub screen_size: Vec2,
    pub lights: Vec<GpuLight>,
}

/// Marker component added to the camera so we can enable the lighting pass.
#[derive(Component, Clone, ExtractComponent)]
pub struct LightingPostProcess;

pub fn extract_light_data(
    mut extracted: ResMut<ExtractedLightData>,
    ambient: Extract<Res<AmbientConfig>>,
    lights: Extract<Query<(&LightSource, &GlobalTransform)>>,
    cameras: Extract<Query<(&Camera, &GlobalTransform), With<Camera3d>>>,
) {
    extracted.lights.clear();

    let Ok((camera, cam_tf)) = cameras.get_single() else {
        return;
    };

    let Some(viewport_size) = camera.physical_viewport_size() else {
        return;
    };
    let viewport_size: UVec2 = viewport_size;
    extracted.screen_size = Vec2::new(viewport_size.x as f32, viewport_size.y as f32);

    let ambient_linear = ambient.color.to_linear();
    extracted.ambient_color = Vec3::new(ambient_linear.red, ambient_linear.green, ambient_linear.blue);
    extracted.ambient_intensity = ambient.intensity;

    for (light, tf) in lights.iter() {
        if extracted.lights.len() >= MAX_LIGHTS {
            break;
        }

        let world_pos = tf.translation();
        let Ok(viewport_pos) = camera.world_to_viewport(cam_tf, world_pos) else {
            continue;
        };

        // Compute screen-space scale: pixels per world unit
        let offset_pos = world_pos + Vec3::X;
        let Ok(viewport_offset) = camera.world_to_viewport(cam_tf, offset_pos) else {
            continue;
        };
        let ppu = (viewport_offset.x - viewport_pos.x).abs();

        let color_linear = light.color.to_linear();

        // Compute shape-specific GPU fields
        let (light_type, shape_param, direction_or_end2) = match light.shape {
            LightShape::Point => (LIGHT_TYPE_POINT, 0.0, Vec2::ZERO),

            LightShape::Cone { direction, angle } => {
                // Screen-space direction: world angle maps directly since camera
                // looks straight down. Y is flipped in screen space.
                let dir = Vec2::new(direction.cos(), -direction.sin());
                (LIGHT_TYPE_CONE, angle * 0.5, dir)
            }

            LightShape::Line { end_offset } => {
                // Project endpoint2 to screen space
                let end2_world = world_pos + Vec3::new(end_offset.x, end_offset.y, 0.0);
                let Ok(end2_screen) = camera.world_to_viewport(cam_tf, end2_world) else {
                    continue;
                };
                (LIGHT_TYPE_LINE, 0.0, end2_screen)
            }

            LightShape::Capsule { direction, half_length } => {
                let dir = Vec2::new(direction.cos(), -direction.sin());
                (LIGHT_TYPE_CAPSULE, half_length * ppu, dir)
            }
        };

        extracted.lights.push(GpuLight {
            position: viewport_pos,
            inner_radius: light.inner_radius * ppu,
            outer_radius: light.outer_radius * ppu,
            color: Vec3::new(color_linear.red, color_linear.green, color_linear.blue),
            intensity: light.intensity,
            light_type,
            shape_param,
            direction_or_end2,
        });
    }
}

// ── GPU buffer ──────────────────────────────────────────────────────────────

#[derive(Resource)]
pub struct LightUniformBuffer {
    pub buffer: StorageBuffer<LightUniformData>,
}

pub fn prepare_light_uniform(
    extracted: Res<ExtractedLightData>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut uniform_buf: ResMut<LightUniformBuffer>,
) {
    let data = LightUniformData {
        ambient_color: extracted.ambient_color,
        ambient_intensity: extracted.ambient_intensity,
        screen_size: extracted.screen_size,
        num_lights: extracted.lights.len() as u32,
        _pad: 0,
        lights: extracted.lights.clone(),
    };

    uniform_buf.buffer.set(data);
    uniform_buf.buffer.write_buffer(&render_device, &render_queue);
}
