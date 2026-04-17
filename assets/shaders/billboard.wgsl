// Billboard material shader with normal-map lighting, parallax, and self-shadowing.
//
// Normal modulation approach: the shader computes how much brighter/darker
// each pixel is compared to a flat surface under the same lighting.
// The post-process lighting pass handles actual scene brightness (ambient,
// distance falloff). This avoids double-counting lights.
//
// Features (controlled by bb_params.features):
// - 0: Unlit — flat base color, no normal processing
// - 1: Normal map — sun + interactive light normal modulation
// - 2: Normal map + parallax — adds depth-based UV offset
// - 3: Normal map + parallax + self-shadows — depth map shadow tracing

#import bevy_pbr::forward_io::VertexOutput

// ── Light data ──────────────────────────────────────────────────────────

struct BillboardLight {
    // xyz = world position, w = inner radius
    position_inner: vec4<f32>,
    // rgb = color × intensity, a = outer radius
    color_outer: vec4<f32>,
}

struct BillboardParams {
    features: f32,
    normal_strength: f32,
    parallax_scale: f32,
    parallax_layers: f32,
    // xyz = sun direction (world space), w = sun intensity
    sun_direction: vec4<f32>,
    num_lights: u32,
    _pad1: u32,
    _pad2: u32,
    _pad3: u32,
    lights: array<BillboardLight, 8>,
}

// ── Bindings ────────────────────────────────────────────────────────────

@group(3) @binding(0) var base_texture: texture_2d<f32>;
@group(3) @binding(1) var base_sampler: sampler;
@group(3) @binding(2) var normal_texture: texture_2d<f32>;
@group(3) @binding(3) var normal_sampler: sampler;
@group(3) @binding(4) var depth_texture: texture_2d<f32>;
@group(3) @binding(5) var depth_sampler: sampler;
@group(3) @binding(6) var<uniform> bb_params: BillboardParams;

// ── Constants ───────────────────────────────────────────────────────────

const SHADOW_STEPS: u32 = 8u;
// Total UV distance to trace for shadows (fraction of billboard).
// 0.2 = trace across 20% of the sprite.
const SHADOW_TRACE_DISTANCE: f32 = 0.2;

// ── Helpers ─────────────────────────────────────────────────────────────

fn has_normal_map() -> bool {
    return bb_params.features >= 1.0;
}

fn has_depth_map() -> bool {
    return bb_params.features >= 2.0;
}

fn has_shadows() -> bool {
    return bb_params.features >= 3.0;
}

fn radial_falloff(dist: f32, inner: f32, outer: f32) -> f32 {
    return 1.0 - smoothstep(inner, outer, dist);
}

/// Simple parallax occlusion mapping.
fn parallax_uv(uv: vec2<f32>, view_dir: vec3<f32>) -> vec2<f32> {
    let num_layers = bb_params.parallax_layers;
    let layer_depth = 1.0 / num_layers;
    var current_layer_depth = 0.0;

    let p = view_dir.xy * bb_params.parallax_scale;
    let delta_uv = p / num_layers;

    var current_uv = uv;
    var current_depth = textureSample(depth_texture, depth_sampler, current_uv).r;

    for (var i = 0u; i < 32u; i = i + 1u) {
        if current_layer_depth >= current_depth || f32(i) >= num_layers {
            break;
        }
        current_uv = current_uv - delta_uv;
        current_depth = textureSample(depth_texture, depth_sampler, current_uv).r;
        current_layer_depth = current_layer_depth + layer_depth;
    }

    let prev_uv = current_uv + delta_uv;
    let after_depth = current_depth - current_layer_depth;
    let before_depth = textureSample(depth_texture, depth_sampler, prev_uv).r
        - current_layer_depth + layer_depth;
    let weight = after_depth / (after_depth - before_depth);

    return mix(current_uv, prev_uv, weight);
}

/// Self-shadow trace through the depth map.
/// Steps from the current pixel toward the light in UV space.
/// If a taller part of the depth map blocks the path, the pixel is shadowed.
/// Returns 0.0 (fully shadowed) to 1.0 (fully lit).
fn shadow_trace(uv: vec2<f32>, current_depth: f32, light_dir_tangent: vec3<f32>) -> f32 {
    // Only trace when light has a significant lateral component.
    // Head-on light (mostly Z) can't cast self-shadows.
    let lateral = length(light_dir_tangent.xy);
    if lateral < 0.15 {
        return 1.0;
    }

    // Step direction in UV space (normalized lateral direction)
    let dir_2d = light_dir_tangent.xy / lateral;
    let uv_step = dir_2d * (SHADOW_TRACE_DISTANCE / f32(SHADOW_STEPS));

    // How much the ray rises (in depth units) per step.
    // Steeper light angle = ray rises faster = harder to occlude.
    let rise_per_step = (light_dir_tangent.z / lateral)
        * (SHADOW_TRACE_DISTANCE / f32(SHADOW_STEPS));

    var shadow = 0.0;
    var trace_uv = uv;
    var ray_depth = current_depth;

    for (var i = 1u; i <= SHADOW_STEPS; i = i + 1u) {
        trace_uv = trace_uv + uv_step;
        ray_depth = ray_depth + rise_per_step;

        // Stop if outside texture bounds
        if trace_uv.x < 0.0 || trace_uv.x > 1.0 || trace_uv.y < 0.0 || trace_uv.y > 1.0 {
            break;
        }

        let sampled_depth = textureSample(depth_texture, depth_sampler, trace_uv).r;

        // Surface at this UV is above our ray → it blocks the light
        let occlusion = sampled_depth - ray_depth;
        if occlusion > 0.01 {
            // Closer occluders cast harder shadows (step_factor fades with distance)
            let step_factor = 1.0 - f32(i) / f32(SHADOW_STEPS + 1u);
            shadow = max(shadow, clamp(occlusion * 4.0, 0.0, 1.0) * step_factor);
        }
    }

    return 1.0 - clamp(shadow, 0.0, 0.7);
}

// ── Fragment ────────────────────────────────────────────────────────────

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var uv = in.uv;

    // Parallax UV offset (if depth map available)
    if has_depth_map() {
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

    // Unlit: return flat color, let post-process handle all lighting
    if !has_normal_map() {
        return col;
    }

    // ── Normal map sampling ──
    let normal_sample = textureSample(normal_texture, normal_sampler, uv);
    var normal = normal_sample.rgb * 2.0 - 1.0;
    normal = normalize(vec3<f32>(
        normal.x * bb_params.normal_strength,
        normal.y * bb_params.normal_strength,
        normal.z,
    ));

    // Flat surface normal in tangent space (no normal map)
    let flat_normal = vec3<f32>(0.0, 0.0, 1.0);

    // Current depth at this pixel (for shadow tracing)
    let current_depth = textureSample(depth_texture, depth_sampler, uv).r;
    let do_shadows = has_shadows() && current_depth > 0.01;

    // ── Build TBN matrix from world normal ──
    let N = normalize(in.world_normal);
    var T = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), N));
    if length(cross(vec3<f32>(0.0, 1.0, 0.0), N)) < 0.001 {
        T = vec3<f32>(1.0, 0.0, 0.0);
    }
    let B = cross(N, T);

    // ── Accumulate normal modulation ──
    var normal_lighting = 0.0;
    var flat_lighting = 0.0;

    // Sun / key light
    let sun_dir_world = normalize(bb_params.sun_direction.xyz);
    let sun_intensity = bb_params.sun_direction.w;
    if sun_intensity > 0.0 {
        let sun_tangent = vec3<f32>(
            dot(sun_dir_world, T),
            dot(sun_dir_world, B),
            dot(sun_dir_world, N),
        );

        var sun_shadow = 1.0;
        if do_shadows {
            sun_shadow = shadow_trace(uv, current_depth, sun_tangent);
        }

        normal_lighting += max(dot(normal, sun_tangent), 0.0) * sun_intensity * sun_shadow;
        flat_lighting += max(dot(flat_normal, sun_tangent), 0.0) * sun_intensity;
    }

    // Interactive lights
    let frag_world = in.world_position.xyz;
    for (var i = 0u; i < bb_params.num_lights; i = i + 1u) {
        let light = bb_params.lights[i];
        let light_pos = light.position_inner.xyz;
        let inner = light.position_inner.w;
        let light_color = light.color_outer.xyz;
        let outer = light.color_outer.w;

        let to_light = light_pos - frag_world;
        let dist = length(to_light);
        let dir_world = to_light / max(dist, 0.001);

        // Distance falloff (same formula as post-process)
        let falloff = radial_falloff(dist, inner, outer);
        if falloff <= 0.0 {
            continue;
        }

        // Luminance of light color for weighting
        let lum = dot(light_color, vec3<f32>(0.2126, 0.7152, 0.0722));

        // Transform to tangent space
        let dir_tangent = vec3<f32>(
            dot(dir_world, T),
            dot(dir_world, B),
            dot(dir_world, N),
        );

        // Self-shadow from depth map
        var light_shadow = 1.0;
        if do_shadows {
            light_shadow = shadow_trace(uv, current_depth, dir_tangent);
        }

        normal_lighting += max(dot(normal, dir_tangent), 0.0) * falloff * lum * light_shadow;
        flat_lighting += max(dot(flat_normal, dir_tangent), 0.0) * falloff * lum;
    }

    // Compute modulation
    var modulation = 1.0;
    if flat_lighting > 0.01 {
        modulation = normal_lighting / flat_lighting;
    } else if normal_lighting > 0.01 {
        modulation = 1.0 + normal_lighting;
    }

    modulation = clamp(modulation, 0.3, 2.0);

    return vec4<f32>(col.rgb * modulation, col.a);
}
