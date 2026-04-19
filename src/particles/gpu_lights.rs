use bevy::asset::RenderAssetUsages;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat};
use bevy::render::render_asset::RenderAssets;
use bevy::render::renderer::RenderQueue;
use bevy::render::texture::GpuImage;
use bevy::render::{Extract, Render, RenderApp, RenderSystems};
use bevy::shader::ShaderRef;

use super::particle::LightParticle;

// ── GPU-side particle light data ────────────────────────────────────────────

/// Fixed texture width — max number of particle lights.
const PARTICLE_TEX_WIDTH: u32 = 256;
/// Bytes per row: width × 4 channels × 2 bytes (f16).
const ROW_BYTES: u32 = PARTICLE_TEX_WIDTH * 4 * 2;

/// Resource holding the data texture handle shared across all ParticleLitMaterials.
#[derive(Resource)]
pub struct ParticleLightBuffer {
    pub handle: Handle<Image>,
}

/// Main-world resource: packed f16 pixel data ready for GPU upload.
#[derive(Resource, Default)]
pub struct ParticleLightData {
    pub bytes: Vec<u8>,
}

/// Render-world mirror of ParticleLightData (extracted each frame).
#[derive(Resource, Default)]
struct ExtractedParticleLightData {
    bytes: Vec<u8>,
}

/// Render-world copy of the image handle so we can look up the GpuImage.
#[derive(Resource)]
struct ParticleLightImageId {
    id: AssetId<Image>,
}

// ── Material extension ──────────────────────────────────────────────────────

/// Material extension that adds GPU-evaluated particle lighting to StandardMaterial.
#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
pub struct ParticleLightExt {
    #[texture(32, sample_type = "float", filterable = false)]
    pub particle_data: Handle<Image>,
}

impl MaterialExtension for ParticleLightExt {
    fn fragment_shader() -> ShaderRef {
        "shaders/particle_lights.wgsl".into()
    }
}

/// Type alias for convenience.
pub type ParticleLitMaterial = ExtendedMaterial<StandardMaterial, ParticleLightExt>;

// ── Main-world systems ──────────────────────────────────────────────────────

/// Creates the fixed-size data texture at startup.
pub fn setup_particle_light_buffer(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
) {
    let total_pixels = PARTICLE_TEX_WIDTH * 2;
    let halfs = vec![0u16; (total_pixels * 4) as usize];
    let mut image = Image::new(
        Extent3d { width: PARTICLE_TEX_WIDTH, height: 2, depth_or_array_layers: 1 },
        TextureDimension::D2,
        bytemuck::cast_slice(&halfs).to_vec(),
        TextureFormat::Rgba16Float,
        RenderAssetUsages::default(),
    );
    // Ensure the GPU texture supports being written to after creation.
    image.texture_descriptor.usage |= bevy::render::render_resource::TextureUsages::COPY_DST;

    let handle = images.add(image);
    commands.insert_resource(ParticleLightBuffer { handle });
    commands.init_resource::<ParticleLightData>();
}

/// Each frame, packs all active LightParticle data into the main-world resource.
/// The actual GPU write happens in the render world (no Image asset mutation).
pub fn upload_particle_lights(
    particles: Query<(&GlobalTransform, &LightParticle)>,
    mut data: ResMut<ParticleLightData>,
) {
    let w = PARTICLE_TEX_WIDTH as usize;
    let total_halfs = w * 2 * 4;
    let mut halfs = vec![0u16; total_halfs];
    // Column 0 is reserved for metadata; particles start at column 1.
    let max_particles = w - 1;
    let mut count = 0usize;

    for (gtf, particle) in &particles {
        if count >= max_particles {
            break;
        }

        let pos = gtf.translation();
        let col = count + 1;

        // Row 0: position + radius
        let base0 = col * 4;
        halfs[base0]     = half::f16::from_f32(pos.x).to_bits();
        halfs[base0 + 1] = half::f16::from_f32(pos.y).to_bits();
        halfs[base0 + 2] = half::f16::from_f32(pos.z).to_bits();
        halfs[base0 + 3] = half::f16::from_f32(particle.light_radius).to_bits();

        // Row 1: color + alpha (alpha = 1.0 — light stays full brightness
        // for the particle's entire lifetime, matching the shared material)
        let t = (particle.age / particle.lifetime).clamp(0.0, 1.0);
        let r = lerp(particle.color_start.red, particle.color_end.red, t);
        let g = lerp(particle.color_start.green, particle.color_end.green, t);
        let b = lerp(particle.color_start.blue, particle.color_end.blue, t);
        let base1 = w * 4 + col * 4;
        halfs[base1]     = half::f16::from_f32(r * particle.intensity).to_bits();
        halfs[base1 + 1] = half::f16::from_f32(g * particle.intensity).to_bits();
        halfs[base1 + 2] = half::f16::from_f32(b * particle.intensity).to_bits();
        halfs[base1 + 3] = half::f16::from_f32(1.0).to_bits();

        count += 1;
    }

    // Column 0, row 0: metadata — x = particle count.
    halfs[0] = half::f16::from_f32(count as f32).to_bits();

    data.bytes = bytemuck::cast_slice(&halfs).to_vec();
}

// ── Render-world systems ────────────────────────────────────────────────────

/// Extracts particle light data + image handle from main world to render world.
fn extract_particle_lights(
    mut commands: Commands,
    data: Extract<Res<ParticleLightData>>,
    buffer: Extract<Res<ParticleLightBuffer>>,
) {
    commands.insert_resource(ExtractedParticleLightData {
        bytes: data.bytes.clone(),
    });
    commands.insert_resource(ParticleLightImageId {
        id: buffer.handle.id(),
    });
}

/// Writes particle data directly to the existing GPU texture via queue.write_texture.
/// No Image asset mutation → no GpuImage re-creation → no bind group invalidation.
fn write_particle_texture(
    data: Res<ExtractedParticleLightData>,
    image_id: Res<ParticleLightImageId>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    render_queue: Res<RenderQueue>,
) {
    let Some(gpu_image) = gpu_images.get(image_id.id) else {
        return;
    };

    if data.bytes.is_empty() {
        return;
    }

    render_queue.write_texture(
        gpu_image.texture.as_image_copy(),
        &data.bytes,
        bevy::render::render_resource::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(ROW_BYTES),
            rows_per_image: Some(2),
        },
        Extent3d {
            width: PARTICLE_TEX_WIDTH,
            height: 2,
            depth_or_array_layers: 1,
        },
    );
}

// ── Plugin setup ────────────────────────────────────────────────────────────

/// Registers render-world extraction and write systems.
/// Call this from ParticlePlugin::build after adding MaterialPlugin.
pub fn register_render_systems(app: &mut App) {
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app
        .add_systems(ExtractSchedule, extract_particle_lights)
        .add_systems(Render, write_particle_texture.in_set(RenderSystems::PrepareAssets));
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
