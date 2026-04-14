// Billboard material shader with optional normal-map lighting.
//
// When a normal map is provided, applies simple directional lighting
// to give sprites a sense of depth and form. Without a normal map,
// renders unlit (same as StandardMaterial unlit).

#import bevy_pbr::forward_io::VertexOutput

@group(2) @binding(0) var base_texture: texture_2d<f32>;
@group(2) @binding(1) var base_sampler: sampler;
@group(2) @binding(2) var normal_texture: texture_2d<f32>;
@group(2) @binding(3) var normal_sampler: sampler;
@group(2) @binding(4) var<uniform> bb_params: BillboardParams;

struct BillboardParams {
    /// Whether a normal map is bound (1.0 = yes, 0.0 = no/unlit).
    has_normal_map: f32,
    /// Light direction in tangent space (normalized).
    light_dir_x: f32,
    light_dir_y: f32,
    light_dir_z: f32,
    /// Ambient light intensity (0-1).
    ambient: f32,
    /// Normal map influence strength.
    normal_strength: f32,
    _pad: vec2<f32>,
};

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let col = textureSample(base_texture, base_sampler, in.uv);

    // Discard transparent pixels
    if col.a < 0.1 {
        discard;
    }

    // If no normal map, return unlit color
    if bb_params.has_normal_map < 0.5 {
        return col;
    }

    // Sample normal map and decode from [0,1] to [-1,1]
    let normal_sample = textureSample(normal_texture, normal_sampler, in.uv);
    var normal = normal_sample.rgb * 2.0 - 1.0;
    normal = normalize(vec3<f32>(
        normal.x * bb_params.normal_strength,
        normal.y * bb_params.normal_strength,
        normal.z,
    ));

    // Simple directional lighting in tangent space
    let light_dir = normalize(vec3<f32>(
        bb_params.light_dir_x,
        bb_params.light_dir_y,
        bb_params.light_dir_z,
    ));
    let ndotl = max(dot(normal, light_dir), 0.0);
    let lighting = bb_params.ambient + (1.0 - bb_params.ambient) * ndotl;

    return vec4<f32>(col.rgb * lighting, col.a);
}
