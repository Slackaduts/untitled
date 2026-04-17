// Billboard shadow prepass fragment shader: pure alpha test.
//
// The heavy lifting — reconstructing the 3D shape from the depth map — is
// done in the prepass vertex shader, which displaces each tessellated vertex
// along the billboard's face normal. The rasterizer then produces fragments
// whose interpolated depth already reflects the 3D extrusion, so this
// fragment shader only needs to discard transparent pixels so shadows match
// the sprite silhouette.

#import bevy_pbr::prepass_io::VertexOutput

struct BillboardDepthParams {
    max_depth: f32,
    alpha_cutoff: f32,
    has_depth_map: u32,
    _pad: f32,
}

@group(3) @binding(100) var<uniform> depth_params: BillboardDepthParams;
@group(3) @binding(101) var base_texture: texture_2d<f32>;
@group(3) @binding(102) var base_sampler: sampler;
@group(3) @binding(103) var depth_texture: texture_2d<f32>;
@group(3) @binding(104) var depth_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) {
    let color = textureSample(base_texture, base_sampler, in.uv);
    if color.a < depth_params.alpha_cutoff {
        discard;
    }
}
