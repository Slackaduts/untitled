// Billboard shadow prepass: writes depth-displaced frag_depth so the shadow
// map reflects the object's actual depth silhouette rather than a flat billboard.
//
// This is a MaterialExtension prepass — it replaces StandardMaterial's prepass
// while the main pass still uses StandardMaterial for unlit rendering.
//
// Extension bindings start at 100 to avoid collisions with StandardMaterial.

#import bevy_pbr::mesh_view_bindings::view

struct BillboardDepthParams {
    max_depth: f32,
    alpha_cutoff: f32,
    has_depth_map: u32,
    _pad: f32,
}

// Minimal prepass fragment input — only the fields we need.
struct PrepassInput {
    @builtin(position) position: vec4<f32>,
#ifdef VERTEX_UVS_A
    @location(0) uv: vec2<f32>,
#endif
    @location(4) world_position: vec4<f32>,
}

// Extension bindings (material bind group 3, indices 100+).
@group(3) @binding(100) var<uniform> depth_params: BillboardDepthParams;
@group(3) @binding(101) var base_texture: texture_2d<f32>;
@group(3) @binding(102) var base_sampler: sampler;
@group(3) @binding(103) var depth_texture: texture_2d<f32>;
@group(3) @binding(104) var depth_sampler: sampler;

struct DepthFragmentOutput {
    @builtin(frag_depth) frag_depth: f32,
}

@fragment
fn fragment(in: PrepassInput) -> DepthFragmentOutput {
    // Alpha test: discard transparent pixels so shadows follow the sprite silhouette.
    let color = textureSample(base_texture, base_sampler, in.uv);
    if color.a < depth_params.alpha_cutoff {
        discard;
    }

    var out: DepthFragmentOutput;

    // Determine protrusion amount.
    var protrusion: f32;
    if depth_params.has_depth_map != 0u {
        // Per-pixel depth from the offline-extracted depth profile.
        let d = textureSample(depth_texture, depth_sampler, in.uv).r;
        protrusion = d * depth_params.max_depth;
    } else {
        // No depth map: use a uniform protrusion for all opaque pixels.
        // This gives the billboard some thickness so shadows aren't paper-flat.
        protrusion = depth_params.max_depth;
    }

    // Skip displacement if max_depth is zero (no depth data at all).
    if protrusion <= 0.0 {
        out.frag_depth = in.position.z;
        return out;
    }

    // Derive face normal from world position derivatives (works for flat quads).
    let normal = normalize(cross(
        dpdx(in.world_position.xyz),
        dpdy(in.world_position.xyz),
    ));

    // Offset world position along the billboard face normal.
    let offset_pos = in.world_position.xyz + normal * protrusion;

    // Reproject to clip space and write displaced depth.
    let clip = view.clip_from_world * vec4<f32>(offset_pos, 1.0);
    out.frag_depth = clip.z / clip.w;

    return out;
}
