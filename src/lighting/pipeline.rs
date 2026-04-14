use bevy::prelude::*;
use bevy::core_pipeline::FullscreenShader;
use bevy::render::render_resource::{
    BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType,
    BufferBindingType, CachedRenderPipelineId, ColorTargetState, ColorWrites, FragmentState,
    MultisampleState, PipelineCache, PrimitiveState, RenderPipelineDescriptor, Sampler,
    SamplerBindingType, SamplerDescriptor, ShaderStages, TextureFormat, TextureSampleType,
    TextureViewDimension,
};
use bevy::render::renderer::RenderDevice;

const LIGHTING_SHADER: &str = "shaders/lighting_post.wgsl";

#[derive(Resource)]
pub struct LightingPipeline {
    pub texture_layout: BindGroupLayout,
    pub uniform_layout: BindGroupLayout,
    pub sampler: Sampler,
    pub pipeline_id: CachedRenderPipelineId,
}

impl FromWorld for LightingPipeline {
    fn from_world(world: &mut World) -> Self {
        let render_device = world.resource::<RenderDevice>();

        let texture_layout_descriptor = BindGroupLayoutDescriptor::new(
            "lighting_texture_bgl",
            &[
                // @binding(0) var hdr_texture: texture_2d<f32>
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // @binding(1) var hdr_sampler: sampler
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        );

        let uniform_layout_descriptor = BindGroupLayoutDescriptor::new(
            "lighting_uniform_bgl",
            &[
                // @binding(0) var<storage, read> light_data: LightUniformData
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        );

        // Create actual BindGroupLayouts for use in the render node bind groups
        let texture_layout = render_device.create_bind_group_layout(
            "lighting_texture_bgl",
            &texture_layout_descriptor.entries,
        );
        let uniform_layout = render_device.create_bind_group_layout(
            "lighting_uniform_bgl",
            &uniform_layout_descriptor.entries,
        );

        let sampler = render_device.create_sampler(&SamplerDescriptor::default());

        let shader = world.load_asset(LIGHTING_SHADER);

        let fullscreen_shader = world.resource::<FullscreenShader>();
        let vertex = fullscreen_shader.to_vertex_state();

        let pipeline_id =
            world
                .resource_mut::<PipelineCache>()
                .queue_render_pipeline(RenderPipelineDescriptor {
                    label: Some("lighting_post_pipeline".into()),
                    layout: vec![texture_layout_descriptor, uniform_layout_descriptor],
                    push_constant_ranges: Vec::new(),
                    vertex,
                    primitive: PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: MultisampleState::default(),
                    fragment: Some(FragmentState {
                        shader,
                        shader_defs: Vec::new(),
                        entry_point: Some("fragment".into()),
                        targets: vec![Some(ColorTargetState {
                            format: TextureFormat::bevy_default(),
                            blend: None,
                            write_mask: ColorWrites::ALL,
                        })],
                    }),
                    zero_initialize_workgroup_memory: false,
                });

        Self {
            texture_layout,
            uniform_layout,
            sampler,
            pipeline_id,
        }
    }
}
