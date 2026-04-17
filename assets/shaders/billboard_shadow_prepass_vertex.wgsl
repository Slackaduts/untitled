// Billboard shadow prepass vertex shader: samples the per-sprite depth map
// at each vertex UV and displaces along local +Z, reconstructing a real 3D
// heightfield from the flat sprite. This lets the shadow pass produce a
// proper silhouette of the depth-extruded object from any sun angle — a
// flat quad rasterizes to zero area when viewed edge-on, producing no
// shadow; a displaced heightfield always has a visible profile.
//
// The main render pass uses StandardMaterial's default vertex shader and
// sees only the flat tessellated quad, so the sprite still looks 2D on
// screen. Only the shadow caster sees the 3D reconstruction.

#import bevy_pbr::{
    prepass_io::{Vertex, VertexOutput},
    mesh_functions::{
        get_world_from_local,
        mesh_position_local_to_world,
    },
    view_transformations::position_world_to_clip,
}

struct BillboardDepthParams {
    max_depth: f32,
    alpha_cutoff: f32,
    has_depth_map: u32,
    _pad: f32,
}

@group(3) @binding(100) var<uniform> depth_params: BillboardDepthParams;
@group(3) @binding(103) var depth_texture: texture_2d<f32>;
@group(3) @binding(104) var depth_sampler: sampler;

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;

    var local_pos = vertex.position;

#ifdef VERTEX_UVS_A
    // Displace along local +Z (the quad's face normal in mesh space). After
    // the billboard's X-axis world rotation, this becomes the direction the
    // sprite faces in world space — the extrusion pokes forward out of the
    // billboard plane like a low-relief sculpture.
    if depth_params.has_depth_map != 0u {
        let d = textureSampleLevel(depth_texture, depth_sampler, vertex.uv, 0.0).r;
        local_pos.z = local_pos.z + d * depth_params.max_depth;
    } else {
        // Flat fallback: push the whole quad forward by max_depth so it still
        // has some thickness to cast a shadow when no depth map is provided.
        local_pos.z = local_pos.z + depth_params.max_depth;
    }
#endif

    let world_from_local = get_world_from_local(vertex.instance_index);
    let world_pos = mesh_position_local_to_world(world_from_local, vec4<f32>(local_pos, 1.0));
    out.world_position = world_pos;
    out.position = position_world_to_clip(world_pos.xyz);

#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif

    // Required by Bevy's prepass pipeline — downstream draws look up
    // per-instance data through this index. Missing it silently breaks
    // shadow casting on many backends.
    out.instance_index = vertex.instance_index;

    // Directional shadow cascades use an orthographic projection with
    // depth-clip emulation on hardware without native DEPTH_CLIP_CONTROL.
    // We must pass through the unclipped Z and clamp position.z the same
    // way Bevy's default prepass does, or the cascade depth comparison
    // will produce garbage.
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
#endif

    return out;
}
