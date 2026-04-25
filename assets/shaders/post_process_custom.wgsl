#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var texture_sampler: sampler;

struct CustomPostProcess {
    // Vignette: rgb = color, w = intensity
    vignette_color: vec4<f32>,
    // Vignette: x = smoothness, y = roundness
    vignette_params: vec4<f32>,
    // Pixelation: x = cell_x, y = cell_y, z = enabled
    pixelation_params: vec4<f32>,
    // Scanlines: x = intensity, y = count, z = speed
    scanline_params: vec4<f32>,
    // Film grain: x = intensity, y = speed
    grain_params: vec4<f32>,
    // Fade: rgb = color, w = intensity
    fade_color: vec4<f32>,
    // Color tint: rgb = tint color, w = intensity
    color_tint: vec4<f32>,
    // Misc: x = invert, y = brightness, z = contrast, w = saturation
    misc_params: vec4<f32>,
    // x = time, y = resolution_x, z = resolution_y, w = enabled (master)
    time_resolution: vec4<f32>,
    // Sine wave: x = amp_x, y = amp_y, z = frequency, w = speed
    sine_wave: vec4<f32>,
    // Swirl: x = angle, y = radius, z = center_x, w = center_y
    swirl: vec4<f32>,
    // x = lens_intensity, y = lens_zoom, z = shake_intensity, w = shake_speed
    distortion_shake: vec4<f32>,
    // x = zoom(1=none), y = rotation(rad), z = posterize_levels(0=off), w = cinema_bar_size(0=off)
    zoom_rotation: vec4<f32>,
    // Cinema bar color: rgb, w = unused
    cinema_bar_color: vec4<f32>,
    // Shockwaves: x = center_u, y = center_v, z = radius_uv, w = intensity
    shockwave_0: vec4<f32>,
    shockwave_0_extra: vec4<f32>,  // x = thickness_uv, y = chromatic_split
    shockwave_1: vec4<f32>,
    shockwave_1_extra: vec4<f32>,
    shockwave_2: vec4<f32>,
    shockwave_2_extra: vec4<f32>,
    shockwave_3: vec4<f32>,
    shockwave_3_extra: vec4<f32>,
}

@group(0) @binding(2) var<uniform> settings: CustomPostProcess;

// ── Color space helpers ────────────────────────────────────────────────────

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3(0.0)), vec3(1.0 / 2.2));
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3(0.0)), vec3(2.2));
}

fn hash(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

// ── Shockwave distortion ──────────────────────────────────────────────────
// Returns UV offset for a single shockwave ring.
// main: (center_u, center_v, radius_uv, intensity)
// extra: (thickness_uv, chromatic_split, -, -)

fn shockwave_displacement(uv: vec2<f32>, main: vec4<f32>, extra: vec4<f32>) -> vec2<f32> {
    if main.w < 0.0001 {
        return vec2(0.0);
    }
    let center = main.xy;
    let radius = main.z;
    let intensity = main.w;
    let thickness = extra.x;

    let delta = uv - center;
    let dist = length(delta);

    // Distance from the ring edge, normalized by thickness
    let ring_dist = abs(dist - radius) / thickness;
    // Smooth falloff: 1 at ring center, 0 at edges
    let ring = 1.0 - smoothstep(0.0, 1.0, ring_dist);

    // Radial push direction
    let dir = select(normalize(delta), vec2(1.0, 0.0), dist < 0.0001);

    return dir * ring * intensity;
}

// Sample with chromatic split around a shockwave
fn sample_with_chromatic(uv: vec2<f32>, offset: vec2<f32>, chromatic: f32) -> vec3<f32> {
    if chromatic < 0.0001 {
        return textureSample(screen_texture, texture_sampler, uv + offset).rgb;
    }
    let perp = vec2(-offset.y, offset.x);
    let r = textureSample(screen_texture, texture_sampler, uv + offset - perp * chromatic).r;
    let g = textureSample(screen_texture, texture_sampler, uv + offset).g;
    let b = textureSample(screen_texture, texture_sampler, uv + offset + perp * chromatic).b;
    return vec3(r, g, b);
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    // Master bypass
    if settings.time_resolution.w < 0.5 {
        return textureSample(screen_texture, texture_sampler, in.uv);
    }

    var uv = in.uv;
    let time = settings.time_resolution.x;
    let resolution = vec2<f32>(settings.time_resolution.y, settings.time_resolution.z);

    // ═══════════════════════════════════════════════════════════════════════
    // UV DISTORTIONS (before texture sampling)
    // ═══════════════════════════════════════════════════════════════════════

    // --- Rotation (around screen center) ---
    let rotation_angle = settings.zoom_rotation.y;
    if abs(rotation_angle) > 0.0001 {
        let centered = uv - 0.5;
        let cos_a = cos(rotation_angle);
        let sin_a = sin(rotation_angle);
        uv = vec2(
            centered.x * cos_a - centered.y * sin_a,
            centered.x * sin_a + centered.y * cos_a,
        ) + 0.5;
    }

    // --- Zoom (scale from center) ---
    let zoom_amount = settings.zoom_rotation.x;
    if abs(zoom_amount - 1.0) > 0.0001 {
        uv = (uv - 0.5) / zoom_amount + 0.5;
    }

    // --- Screen Shake (UV offset) ---
    let shake_intensity = settings.distortion_shake.z;
    if shake_intensity > 0.0001 {
        let shake_speed = settings.distortion_shake.w;
        uv += shake_intensity * vec2(
            sin(time * shake_speed * 17.3),
            cos(time * shake_speed * 13.7 + 1.5),
        );
    }

    // --- Lens Distortion (barrel/pincushion) ---
    let lens_intensity = settings.distortion_shake.x;
    if abs(lens_intensity) > 0.0001 {
        let lens_zoom = settings.distortion_shake.y;
        let centered = uv - 0.5;
        let dist_sq = dot(centered, centered);
        uv = centered * (1.0 + dist_sq * lens_intensity) / lens_zoom + 0.5;
    }

    // --- Swirl ---
    let swirl_angle = settings.swirl.x;
    if abs(swirl_angle) > 0.0001 {
        let swirl_radius = settings.swirl.y;
        let swirl_center = settings.swirl.zw;
        let delta = uv - swirl_center;
        let dist = length(delta);
        if dist < swirl_radius && swirl_radius > 0.0001 {
            let factor = 1.0 - dist / swirl_radius;
            let theta = swirl_angle * factor * factor; // quadratic falloff
            let cos_t = cos(theta);
            let sin_t = sin(theta);
            uv = vec2(
                delta.x * cos_t - delta.y * sin_t,
                delta.x * sin_t + delta.y * cos_t,
            ) + swirl_center;
        }
    }

    // --- Sine Wave (wavy screen) ---
    let sine_amp_x = settings.sine_wave.x;
    let sine_amp_y = settings.sine_wave.y;
    if abs(sine_amp_x) > 0.0001 || abs(sine_amp_y) > 0.0001 {
        let freq = settings.sine_wave.z;
        let speed = settings.sine_wave.w;
        uv.x += sin(uv.y * freq + time * speed) * sine_amp_x;
        uv.y += sin(uv.x * freq + time * speed * 0.7) * sine_amp_y;
    }

    // --- Shockwaves (UV distortion, before sampling) ---
    var sw_offset = vec2(0.0, 0.0);
    var sw_chromatic = 0.0;
    let d0 = shockwave_displacement(uv, settings.shockwave_0, settings.shockwave_0_extra);
    let d1 = shockwave_displacement(uv, settings.shockwave_1, settings.shockwave_1_extra);
    let d2 = shockwave_displacement(uv, settings.shockwave_2, settings.shockwave_2_extra);
    let d3 = shockwave_displacement(uv, settings.shockwave_3, settings.shockwave_3_extra);
    sw_offset = d0 + d1 + d2 + d3;
    // Max chromatic from any active shockwave contributing to this pixel
    if length(d0) > 0.0001 { sw_chromatic = max(sw_chromatic, settings.shockwave_0_extra.y); }
    if length(d1) > 0.0001 { sw_chromatic = max(sw_chromatic, settings.shockwave_1_extra.y); }
    if length(d2) > 0.0001 { sw_chromatic = max(sw_chromatic, settings.shockwave_2_extra.y); }
    if length(d3) > 0.0001 { sw_chromatic = max(sw_chromatic, settings.shockwave_3_extra.y); }

    // --- Pixelation (UV modification) ---
    if settings.pixelation_params.z > 0.5 {
        let cell_size = settings.pixelation_params.xy;
        uv = floor(uv * resolution / cell_size) * cell_size / resolution;
    }

    // Apply shockwave offset to UV
    uv += sw_offset;

    // ═══════════════════════════════════════════════════════════════════════
    // TEXTURE SAMPLING
    // ═══════════════════════════════════════════════════════════════════════

    // Sample the screen — chromatic split if shockwaves are active
    var color: vec4<f32>;
    if sw_chromatic > 0.0001 {
        color = vec4<f32>(sample_with_chromatic(uv, vec2(0.0), sw_chromatic * length(sw_offset) * 10.0), 1.0);
    } else {
        color = textureSample(screen_texture, texture_sampler, uv);
    }

    // Convert to sRGB for perceptual operations
    color = vec4<f32>(linear_to_srgb(color.rgb), color.a);

    // ═══════════════════════════════════════════════════════════════════════
    // COLOR OPERATIONS (in sRGB space)
    // ═══════════════════════════════════════════════════════════════════════

    // --- Scanlines ---
    let scanline_intensity = settings.scanline_params.x;
    if scanline_intensity > 0.001 {
        let count = settings.scanline_params.y;
        let speed = settings.scanline_params.z;
        let scanline = sin((uv.y * count + time * speed) * 3.14159265) * 0.5 + 0.5;
        color = vec4<f32>(color.rgb * (1.0 - scanline_intensity * (1.0 - scanline)), color.a);
    }

    // --- Film Grain ---
    let grain_intensity = settings.grain_params.x;
    if grain_intensity > 0.001 {
        let grain_speed = settings.grain_params.y;
        let seed = floor(time * grain_speed * 60.0);
        let noise = hash(uv * resolution + vec2<f32>(seed, seed * 0.7)) * 2.0 - 1.0;
        color = vec4<f32>(color.rgb + vec3<f32>(noise * grain_intensity), color.a);
    }

    // --- Color Tint ---
    let tint_intensity = settings.color_tint.w;
    if tint_intensity > 0.001 {
        color = vec4<f32>(mix(color.rgb, color.rgb * settings.color_tint.rgb, tint_intensity), color.a);
    }

    // --- Brightness (additive) ---
    let brightness = settings.misc_params.y;
    if abs(brightness) > 0.001 {
        color = vec4<f32>(color.rgb + vec3<f32>(brightness), color.a);
    }

    // --- Contrast (scale from midpoint) ---
    let contrast = settings.misc_params.z;
    if abs(contrast - 1.0) > 0.001 {
        color = vec4<f32>((color.rgb - 0.5) * contrast + 0.5, color.a);
    }

    // --- Saturation ---
    let saturation = settings.misc_params.w;
    if abs(saturation - 1.0) > 0.001 {
        let luma = dot(color.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
        color = vec4<f32>(mix(vec3<f32>(luma), color.rgb, saturation), color.a);
    }

    // --- Posterization ---
    let posterize_levels = settings.zoom_rotation.z;
    if posterize_levels > 1.5 {
        color = vec4<f32>(floor(color.rgb * posterize_levels) / (posterize_levels - 1.0), color.a);
    }

    // --- Invert ---
    let invert = settings.misc_params.x;
    if invert > 0.001 {
        color = vec4<f32>(mix(color.rgb, 1.0 - color.rgb, invert), color.a);
    }

    // --- Vignette ---
    let vignette_intensity = settings.vignette_color.w;
    if vignette_intensity > 0.001 {
        let smoothness = settings.vignette_params.x;
        let roundness = settings.vignette_params.y;
        let vignette_col = settings.vignette_color.rgb;

        let center = uv - 0.5;
        let dist = length(center * vec2<f32>(1.0, roundness));
        let vig = 1.0 - smoothstep(1.0 - smoothness - vignette_intensity, 1.0 - smoothness, dist * 2.0);
        color = vec4<f32>(mix(vignette_col, color.rgb, vig), color.a);
    }

    // --- Cinema Bars (letterboxing) ---
    let bar_size = settings.zoom_rotation.w;
    if bar_size > 0.001 {
        let bar_color = settings.cinema_bar_color.rgb;
        // Top and bottom bars
        let bar_mask = smoothstep(0.0, 0.005, in.uv.y - bar_size)
                     * smoothstep(0.0, 0.005, (1.0 - in.uv.y) - bar_size);
        color = vec4<f32>(mix(bar_color, color.rgb, bar_mask), color.a);
    }

    // --- Fade (full-screen color overlay) ---
    let fade_intensity = settings.fade_color.w;
    if fade_intensity > 0.001 {
        color = vec4<f32>(mix(color.rgb, settings.fade_color.rgb, fade_intensity), color.a);
    }

    // Convert back to linear for the render pipeline
    color = vec4<f32>(srgb_to_linear(color.rgb), color.a);

    return color;
}
