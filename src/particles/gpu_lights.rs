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
    let total_bytes = w * 2 * 4 * 2; // width × 2 rows × 4 channels × 2 bytes per f16

    // Reuse the existing allocation instead of allocating a new Vec every frame.
    data.bytes.resize(total_bytes, 0);
    data.bytes.fill(0);
    let halfs: &mut [u16] = bytemuck::cast_slice_mut(&mut data.bytes);

    // Column 0 is reserved for metadata; particles start at column 1.
    let max_particles = w - 1;
    let mut count = 0usize;

    for (gtf, particle) in &particles {
        if count >= max_particles {
            break;
        }

        let pos = gtf.translation();
        let col = count + 1;

        let t = (particle.age / particle.lifetime).clamp(0.0, 1.0);

        // Scale radius with the size gradient so the light grows/shrinks
        // with the visual particle.
        let size_scale = super::definitions::sample_gradient_size(&particle.size_stops, t);
        let radius = particle.light_radius * (size_scale / particle.size_stops.first().map_or(1.0, |s| s.size).max(0.01));

        // Row 0: position + radius.
        // XY from world position; Z near ground so lights reach billboard surfaces.
        let base0 = col * 4;
        halfs[base0]     = half::f16::from_f32(pos.x).to_bits();
        halfs[base0 + 1] = half::f16::from_f32(pos.y).to_bits();
        halfs[base0 + 2] = half::f16::from_f32(pos.z.min(5.0)).to_bits();
        halfs[base0 + 3] = half::f16::from_f32(radius).to_bits();

        // Row 1: color + alpha.
        // Color follows the gradient; alpha fades with the gradient alpha
        // so the light dies with the particle.
        let c = super::definitions::sample_gradient_color(&particle.color_stops, t);
        let alpha = c[3]; // gradient alpha controls light fadeout
        let base1 = w * 4 + col * 4;
        halfs[base1]     = half::f16::from_f32(c[0] * particle.intensity).to_bits();
        halfs[base1 + 1] = half::f16::from_f32(c[1] * particle.intensity).to_bits();
        halfs[base1 + 2] = half::f16::from_f32(c[2] * particle.intensity).to_bits();
        halfs[base1 + 3] = half::f16::from_f32(alpha).to_bits();

        count += 1;
    }

    // Column 0, row 0: metadata — x = particle count.
    halfs[0] = half::f16::from_f32(count as f32).to_bits();
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

