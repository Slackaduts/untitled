#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

// ── Bindings ────────────────────────────────────────────────────────────────

@group(0) @binding(0) var hdr_texture: texture_2d<f32>;
@group(0) @binding(1) var hdr_sampler: sampler;

// Light type constants (must match Rust)
const LIGHT_POINT: u32 = 0u;
const LIGHT_CONE: u32 = 1u;
const LIGHT_LINE: u32 = 2u;
const LIGHT_CAPSULE: u32 = 3u;

struct GpuLight {
    position: vec2<f32>,
    inner_radius: f32,
    outer_radius: f32,
    color: vec3<f32>,
    intensity: f32,
    light_type: u32,
    shape_param: f32,           // cone: half_angle (rad), capsule: half_length (px)
    direction_or_end2: vec2<f32>, // cone/capsule: normalized dir, line: endpoint2 screen pos
};

struct LightUniformData {
    ambient_color: vec3<f32>,
    ambient_intensity: f32,
    screen_size: vec2<f32>,
    num_lights: u32,
    _pad: u32,
    lights: array<GpuLight>,
};

@group(1) @binding(0) var<storage, read> light_data: LightUniformData;

// ── Falloff functions ───────────────────────────────────────────────────────

fn radial_falloff(dist: f32, inner: f32, outer: f32) -> f32 {
    return 1.0 - smoothstep(inner, outer, dist);
}

fn point_falloff(frag: vec2<f32>, light: GpuLight) -> f32 {
    let dist = distance(frag, light.position);
    return radial_falloff(dist, light.inner_radius, light.outer_radius);
}

fn cone_falloff(frag: vec2<f32>, light: GpuLight) -> f32 {
    let to_frag = frag - light.position;
    let dist = length(to_frag);
    if dist < 0.001 {
        return 1.0;
    }

    let dir = light.direction_or_end2;
    let half_angle = light.shape_param;

    // Angle between light direction and vector to fragment
    let frag_dir = to_frag / dist;
    let cos_angle = dot(frag_dir, dir);
    let cos_half = cos(half_angle);

    // Soft edge: smoothstep over a small angular range at the cone boundary
    let edge_softness = 0.15;
    let cos_soft = cos(half_angle + edge_softness);
    let angular = smoothstep(cos_soft, cos_half, cos_angle);

    let distance_falloff = radial_falloff(dist, light.inner_radius, light.outer_radius);
    return angular * distance_falloff;
}

fn line_falloff(frag: vec2<f32>, light: GpuLight) -> f32 {
    let a = light.position;
    let b = light.direction_or_end2; // endpoint2 in screen space
    let ab = b - a;
    let ab_len_sq = dot(ab, ab);

    var closest: vec2<f32>;
    if ab_len_sq < 0.001 {
        // Degenerate: endpoints coincide, treat as point
        closest = a;
    } else {
        // Project frag onto line segment, clamp to [0, 1]
        let t = clamp(dot(frag - a, ab) / ab_len_sq, 0.0, 1.0);
        closest = a + t * ab;
    }

    let dist = distance(frag, closest);
    return radial_falloff(dist, light.inner_radius, light.outer_radius);
}

fn capsule_falloff(frag: vec2<f32>, light: GpuLight) -> f32 {
    let dir = light.direction_or_end2;
    let half_len = light.shape_param;

    // Capsule is a line segment centered at position, extending ±half_len along direction
    let a = light.position - dir * half_len;
    let b = light.position + dir * half_len;
    let ab = b - a;
    let ab_len_sq = dot(ab, ab);

    var closest: vec2<f32>;
    if ab_len_sq < 0.001 {
        closest = light.position;
    } else {
        let t = clamp(dot(frag - a, ab) / ab_len_sq, 0.0, 1.0);
        closest = a + t * ab;
    }

    let dist = distance(frag, closest);
    return radial_falloff(dist, light.inner_radius, light.outer_radius);
}

// ── Fragment ────────────────────────────────────────────────────────────────

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let scene_color = textureSample(hdr_texture, hdr_sampler, in.uv);
    let frag_pos = in.uv * light_data.screen_size;

    // Start with ambient contribution
    var accumulated = light_data.ambient_color * light_data.ambient_intensity;

    // Accumulate light contributions
    let count = light_data.num_lights;
    for (var i = 0u; i < count; i = i + 1u) {
        let light = light_data.lights[i];

        var falloff = 0.0;
        switch light.light_type {
            case LIGHT_POINT:   { falloff = point_falloff(frag_pos, light); }
            case LIGHT_CONE:    { falloff = cone_falloff(frag_pos, light); }
            case LIGHT_LINE:    { falloff = line_falloff(frag_pos, light); }
            case LIGHT_CAPSULE: { falloff = capsule_falloff(frag_pos, light); }
            default: {}
        }

        accumulated += light.color * light.intensity * falloff;
    }

    // Clamp to prevent blowout, but allow slight over-brightening for strong lights
    accumulated = clamp(accumulated, vec3<f32>(0.0), vec3<f32>(2.0));

    return vec4<f32>(scene_color.rgb * accumulated, scene_color.a);
}
