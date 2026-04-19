// Particle light evaluation — MaterialExtension fragment shader.
//
// Replaces the standard PBR fragment to add per-fragment lighting from
// particle emitters. Particle data is packed into a texture each frame;
// this shader reads it with textureLoad (direct, no sampler/filtering).
//
// Texture layout (Rgba16Float, PARTICLE_TEX_WIDTH × 2):
//   Row 0, col 0: metadata — x = particle count (as float)
//   Row 0, col 1..N: xyz = world position, w = falloff radius
//   Row 1, col 1..N: rgb = color × intensity, a = current alpha

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
}

#ifdef PREPASS_PIPELINE
#import bevy_pbr::{
    prepass_io::{VertexOutput, FragmentOutput},
    pbr_deferred_functions::deferred_output,
}
#else
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT,
}
#endif

@group(#{MATERIAL_BIND_GROUP}) @binding(32)
var particle_data: texture_2d<f32>;

// ── Particle lighting evaluation ────────────────────────────────────────

fn evaluate_particle_lights(world_pos: vec3<f32>, world_normal: vec3<f32>) -> vec3<f32> {
    // Column 0 holds metadata: x = particle count.
    let header = textureLoad(particle_data, vec2<i32>(0, 0), 0);
    let count = u32(header.x);
    if count == 0u {
        return vec3(0.0);
    }

    var total = vec3(0.0);

    // Particles start at column 1.
    for (var i = 1u; i <= count; i++) {
        // Read position + radius (cheap early-out).
        let pos_radius = textureLoad(particle_data, vec2<i32>(i32(i), 0), 0);
        let to_light = pos_radius.xyz - world_pos;
        let radius = pos_radius.w;

        // Squared distance check — avoids sqrt for out-of-range particles.
        let dist_sq = dot(to_light, to_light);
        if dist_sq > radius * radius {
            continue;
        }

        // Only read color row for in-range particles.
        let col_alpha = textureLoad(particle_data, vec2<i32>(i32(i), 1), 0);

        let dist = sqrt(dist_sq);
        let dir = to_light / max(dist, 0.001);
        let ndotl = max(dot(world_normal, dir), 0.0);
        let inner = radius * 0.3;
        let falloff = 1.0 - smoothstep(inner, radius, dist);

        total += col_alpha.rgb * col_alpha.a * falloff * ndotl;
    }

    return total;
}

// ── Fragment ────────────────────────────────────────────────────────────

@fragment
fn fragment(
    vertex_output: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var in = vertex_output;

    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(
        pbr_input.material,
        pbr_input.material.base_color,
    );

#ifdef PREPASS_PIPELINE
    let out = deferred_output(in, pbr_input);
#else
    var out: FragmentOutput;

    if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);

        let world_pos = in.world_position.xyz;
        let world_normal = normalize(in.world_normal);
        let particle_contrib = evaluate_particle_lights(world_pos, world_normal);
        let albedo = pbr_input.material.base_color.rgb;
        out.color = vec4(out.color.rgb + particle_contrib * albedo, out.color.a);
    } else {
        out.color = pbr_input.material.base_color;
    }

    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
#endif

    return out;
}
