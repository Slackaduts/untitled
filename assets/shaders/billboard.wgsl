// Billboard material shader with normal-map lighting and parallax.
//
// Features (all optional, controlled by bb_params flags):
// - Normal map lighting: directional light using tangent-space normals
// - Parallax mapping: UV offset from depth map to fake 3D depth
// - Unlit fallback: when no maps provided, renders flat color

#import bevy_pbr::forward_io::VertexOutput

@group(2) @binding(0) var base_texture: texture_2d<f32>;
@group(2) @binding(1) var base_sampler: sampler;
@group(2) @binding(2) var normal_texture: texture_2d<f32>;
@group(2) @binding(3) var normal_sampler: sampler;
@group(2) @binding(4) var depth_texture: texture_2d<f32>;
@group(2) @binding(5) var depth_sampler: sampler;
@group(2) @binding(6) var<uniform> bb_params: BillboardParams;

struct BillboardParams {
    /// Bitfield: bit 0 = has normal map, bit 1 = has depth map (parallax)
    features: f32,
    /// Light direction in tangent space (normalized).
    light_dir_x: f32,
    light_dir_y: f32,
    light_dir_z: f32,
    /// Ambient light intensity (0-1).
    ambient: f32,
    /// Normal map influence strength.
    normal_strength: f32,
    /// Parallax depth scale (how much UVs shift). Higher = more depth.
    parallax_scale: f32,
    /// Number of parallax layers for relief mapping (4-32).
    parallax_layers: f32,
};

fn has_normal_map() -> bool {
    return bb_params.features >= 1.0;
}

fn has_depth_map() -> bool {
    return bb_params.features >= 2.0;
}

/// Simple parallax occlusion mapping.
/// Offsets the UV based on depth to create an illusion of depth.
fn parallax_uv(uv: vec2<f32>, view_dir: vec3<f32>) -> vec2<f32> {
    let num_layers = bb_params.parallax_layers;
    let layer_depth = 1.0 / num_layers;
    var current_layer_depth = 0.0;

    // UV offset per layer, based on view direction
    let p = view_dir.xy * bb_params.parallax_scale;
    let delta_uv = p / num_layers;

    var current_uv = uv;
    var current_depth = textureSample(depth_texture, depth_sampler, current_uv).r;

    // Step through layers until we find the intersection
    for (var i = 0u; i < 32u; i = i + 1u) {
        if current_layer_depth >= current_depth || f32(i) >= num_layers {
            break;
        }
        current_uv = current_uv - delta_uv;
        current_depth = textureSample(depth_texture, depth_sampler, current_uv).r;
        current_layer_depth = current_layer_depth + layer_depth;
    }

    // Interpolate between previous and current layer for smoother result
    let prev_uv = current_uv + delta_uv;
    let after_depth = current_depth - current_layer_depth;
    let before_depth = textureSample(depth_texture, depth_sampler, prev_uv).r
        - current_layer_depth + layer_depth;
    let weight = after_depth / (after_depth - before_depth);

    return mix(current_uv, prev_uv, weight);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var uv = in.uv;

    // Parallax UV offset (if depth map available)
    if has_depth_map() {
        // Approximate view direction in tangent space from the billboard tilt.
        // Since billboards face roughly toward the camera, the view direction
        // in tangent space is approximately (0, 0, 1) with slight XY offset
        // based on the UV position (center = straight on, edges = angled).
        let view_dir = normalize(vec3<f32>(
            (uv.x - 0.5) * 0.3,
            (uv.y - 0.5) * 0.3,
            1.0,
        ));
        uv = parallax_uv(uv, view_dir);
    }

    let col = textureSample(base_texture, base_sampler, uv);

    // Discard transparent pixels
    if col.a < 0.1 {
        discard;
    }

    // If no normal map, return unlit color
    if !has_normal_map() {
        return col;
    }

    // Sample normal map and decode from [0,1] to [-1,1]
    let normal_sample = textureSample(normal_texture, normal_sampler, uv);
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
