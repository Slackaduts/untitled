// Billboard material shader with per-pixel depth writing.
//
// Each fragment writes a depth value based on its Y position within the
// billboard quad, mapped to world-space depth. This gives pixel-perfect
// depth sorting — the bottom of a tree is in front of objects behind it,
// while the top is behind objects in front of it.

#import bevy_pbr::forward_io::VertexOutput

@group(2) @binding(0) var base_texture: texture_2d<f32>;
@group(2) @binding(1) var base_sampler: sampler;
@group(2) @binding(2) var<uniform> bb_params: BillboardParams;

struct BillboardParams {
    /// How much depth range the billboard spans (in NDC units).
    /// Higher = more depth variation from bottom to top.
    depth_range: f32,
    _pad: vec3<f32>,
};

struct FragmentOutput {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    let col = textureSample(base_texture, base_sampler, in.uv);

    // Discard transparent pixels — Mask mode needs explicit discard
    if col.a < 0.05 {
        discard;
    }

    var out: FragmentOutput;
    out.color = col;

    // Per-pixel depth: shift depth slightly based on Y position within
    // the billboard so top pixels sort behind bottom pixels. This is a
    // subtle effect — just enough for correct sorting, not enough to
    // push pixels behind the terrain.
    let depth_offset = (0.5 - in.uv.y) * bb_params.depth_range;
    out.depth = in.position.z + depth_offset;

    return out;
}
