use bevy::prelude::*;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey,
                MaterialExtensionPipeline};
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::{ShaderDefVal, ShaderRef};

/// The full billboard material type: StandardMaterial (unlit, alpha mask)
/// extended with depth-displaced shadow casting.
pub type BillboardMaterial = ExtendedMaterial<StandardMaterial, BillboardDepthExtension>;

/// Per-material uniform holding depth displacement parameters.
#[derive(Clone, Debug, ShaderType)]
pub struct BillboardDepthParams {
    /// Maximum protrusion in world units (depth_range * billboard_scale).
    pub max_depth: f32,
    /// Alpha cutoff for shadow silhouette (typically 0.5).
    pub alpha_cutoff: f32,
    /// 1 if a depth map is loaded, 0 for flat-billboard fallback.
    pub has_depth_map: u32,
    pub _pad: f32,
}

/// Extension that adds depth-displaced shadow casting to StandardMaterial.
///
/// StandardMaterial handles the main pass (alpha mask, shadows from silhouette).
/// When the prepass shader is enabled, this extension overrides it to write
/// displaced `frag_depth` values based on a depth profile texture.
///
/// The base_color_texture is duplicated here so the prepass shader can do
/// alpha testing without depending on StandardMaterial's bindless layout.
#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
pub struct BillboardDepthExtension {
    #[uniform(100)]
    pub depth_params: BillboardDepthParams,
    /// Duplicate of the base color texture for prepass alpha testing.
    #[texture(101)]
    #[sampler(102)]
    pub base_color_texture: Handle<Image>,
    /// Depth profile: R channel = normalized protrusion from billboard plane.
    #[texture(103)]
    #[sampler(104)]
    pub depth_texture: Handle<Image>,
}

fn push_def_if_missing(defs: &mut Vec<ShaderDefVal>, name: &str) {
    if !defs.iter().any(|d| match d {
        ShaderDefVal::Bool(s, _) | ShaderDefVal::Int(s, _) | ShaderDefVal::UInt(s, _) => s == name,
    }) {
        defs.push(name.into());
    }
}

impl MaterialExtension for BillboardDepthExtension {
    fn prepass_vertex_shader() -> ShaderRef {
        "shaders/billboard_shadow_prepass_vertex.wgsl".into()
    }

    fn prepass_fragment_shader() -> ShaderRef {
        "shaders/billboard_shadow_prepass.wgsl".into()
    }

    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        push_def_if_missing(&mut descriptor.vertex.shader_defs, "VERTEX_UVS_A");
        if let Some(ref mut frag) = descriptor.fragment {
            push_def_if_missing(&mut frag.shader_defs, "VERTEX_UVS_A");
        }
        Ok(())
    }
}

