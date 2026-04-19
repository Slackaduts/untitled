#import bevy_ecs_tilemap::common::{tilemap_data, sprite_texture, sprite_sampler}
#import bevy_ecs_tilemap::vertex_output::MeshVertexOutput
#import bevy_sprite::mesh2d_view_bindings::globals

// ── Bindings ────────────────────────────────────────────────────────────────

@group(3) @binding(0) var terrain_map: texture_2d<f32>;
@group(3) @binding(1) var terrain_sampler: sampler;
@group(3) @binding(2) var grass_texture: texture_2d<f32>;
@group(3) @binding(3) var grass_sampler: sampler;
@group(3) @binding(4) var dirt_texture: texture_2d<f32>;
@group(3) @binding(5) var dirt_sampler: sampler;
@group(3) @binding(6) var stone_texture: texture_2d<f32>;
@group(3) @binding(7) var stone_sampler: sampler;

struct TerrainParams {
    // Water state
    water_deep: vec4<f32>,
    water_mid: vec4<f32>,
    water_surface: vec4<f32>,
    water_highlight: vec4<f32>,
    water_flow_dir: vec2<f32>,
    water_flow_speed: f32,
    _pad0: f32,
    // Grass state
    grass_scale: f32,
    stochastic_cell: f32,
    // Layout
    map_size: vec2<f32>,
    // Transition
    transition_type: u32,
    ledge_half_width: f32,
    _pad1: vec2<f32>,
};
@group(3) @binding(8) var<uniform> params: TerrainParams;

// ── Terrain state IDs ───────────────────────────────────────────────────────
// Must match terrain_id module in terrain_material.rs

const ID_EMPTY:    u32 = 0u;
const ID_RIVER:    u32 = 1u;
const ID_GRASS:    u32 = 2u;
const ID_SAND:     u32 = 3u;
const ID_DIRT:     u32 = 4u;
const ID_SNOW:     u32 = 5u;
const ID_LAVA:     u32 = 6u;
const ID_MUD:      u32 = 7u;
const ID_STONE:    u32 = 8u;
const ID_SHALLOWS: u32 = 9u;

// ── Transition IDs ──────────────────────────────────────────────────────────

const TRANS_WATER_SHORE: u32 = 1u;
const TRANS_GRASS_BLEND: u32 = 2u;
const TRANS_WATER_DEPTH: u32 = 3u; // water ↔ shallows — depth fade

// ── Terrain map ─────────────────────────────────────────────────────────────

fn terrain_id_at(tile: vec2<i32>) -> u32 {
    let ms = params.map_size;
    if tile.x < 0 || tile.y < 0 || f32(tile.x) >= ms.x || f32(tile.y) >= ms.y {
        return ID_EMPTY;
    }
    let uv = (vec2<f32>(f32(tile.x), f32(tile.y)) + 0.5) / ms;
    let s = textureSampleLevel(terrain_map, terrain_sampler, uv, 0.0);
    // R channel stores the terrain ID as a normalized float (id/255).
    // ID 0 = empty (A channel now stores shallows info, not emptiness).
    let id = u32(round(s.r * 255.0));
    if id == 0u { return ID_EMPTY; }
    return id;
}

// ── Precomputed channel helpers ────────────────────────────────────────────
// Terrain map packs: R=terrain ID, G=neighbor bitmask, B=river depth, A=shallows info
// Bit ordering for bitmask: 0=N 1=S 2=E 3=W 4=NE 5=NW 6=SE 7=SW

fn terrain_sample_all(tile: vec2<i32>) -> vec4<f32> {
    let ms = params.map_size;
    let uv = (vec2<f32>(f32(tile.x), f32(tile.y)) + 0.5) / ms;
    return textureSampleLevel(terrain_map, terrain_sampler, uv, 0.0);
}

fn neighbor_bitmask(s: vec4<f32>) -> u32 {
    return u32(round(s.g * 255.0));
}

fn precomputed_river_depth(s: vec4<f32>) -> f32 {
    return s.b; // Already 0-1 range (was quantized from 0-255 on CPU)
}

fn precomputed_shallows_info(s: vec4<f32>) -> vec2<f32> {
    // A channel: high nibble = depth_index, low nibble = stack_height
    let packed = u32(round(s.a * 255.0));
    let depth_idx = f32(packed >> 4u);
    let stack_h = f32(packed & 0xFu);
    return vec2(depth_idx, stack_h);
}

// ── Noise ───────────────────────────────────────────────────────────────────

fn hash21(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

fn hash22(p: vec2<f32>) -> vec2<f32> {
    return fract(sin(vec2<f32>(
        dot(p, vec2<f32>(127.1, 311.7)),
        dot(p, vec2<f32>(269.5, 183.3)),
    )) * 43758.5453);
}

fn value_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(hash21(i), hash21(i + vec2(1.0, 0.0)), u.x),
        mix(hash21(i + vec2(0.0, 1.0)), hash21(i + vec2(1.0, 1.0)), u.x),
        u.y,
    );
}

fn fbm2(p: vec2<f32>) -> f32 {
    let v0 = value_noise(p);
    let v1 = value_noise(p * 2.0 + vec2(100.0));
    return v0 * 0.5 + v1 * 0.25;
}

fn fbm3(p: vec2<f32>) -> f32 {
    var v = 0.0; var a = 0.5; var q = p;
    for (var i = 0; i < 3; i++) {
        v += a * value_noise(q);
        q = q * 2.0 + vec2(100.0);
        a *= 0.5;
    }
    return v;
}

// ═══════════════════════════════════════════════════════════════════════════
//  TERRAIN STATES — fill shaders dispatched by terrain ID
// ═══════════════════════════════════════════════════════════════════════════

// ── State: RIVER ────────────────────────────────────────────────────────────

// Bilinearly interpolated depth — reads precomputed B channel from 4 neighboring tiles
// for smooth blending across tile boundaries. Replaces the expensive per-tile walk.
fn river_depth_at(tc: vec2<i32>, local: vec2<f32>) -> f32 {
    // Offset so we interpolate between the 4 nearest tile centers
    let offset = local - 0.5; // -0.5..0.5 from tile center
    let base = tc + vec2(select(0, -1, offset.x < 0.0), select(0, -1, offset.y < 0.0));
    let f = fract(local + 0.5); // interpolation weight (0..1 between the two centers)

    let d00 = precomputed_river_depth(terrain_sample_all(base));
    let d10 = precomputed_river_depth(terrain_sample_all(base + vec2(1, 0)));
    let d01 = precomputed_river_depth(terrain_sample_all(base + vec2(0, 1)));
    let d11 = precomputed_river_depth(terrain_sample_all(base + vec2(1, 1)));

    return mix(mix(d00, d10, f.x), mix(d01, d11, f.x), f.y);
}

fn fill_water(world_px: vec2<f32>, time: f32) -> vec4<f32> {
    let p = world_px / tilemap_data.tile_size.x;
    let spd = params.water_flow_speed * 1.8;
    let flow  = params.water_flow_dir * spd * time;
    let flow2 = vec2<f32>(params.water_flow_dir.x * 0.57, params.water_flow_dir.y * -1.3) * spd * time;

    let deep = params.water_deep.rgb;
    let mid  = params.water_mid.rgb;
    let surf = params.water_surface.rgb;
    let hi   = params.water_highlight.rgb;

    // Deep base — fbm2 is enough for low-frequency color variation
    var col = mix(deep, mid, fbm2(p * 0.25 + flow * 0.12) * 0.5);

    // Broad current streaks
    col = mix(col, surf, fbm2(vec2(p.x * 0.8, p.y * 2.5) + flow) * 0.35);

    // Ripple layers — two is sufficient
    let r1 = value_noise(vec2(p.x * 4.0, p.y * 1.5) + flow2 * 1.2);
    let r2 = value_noise(vec2(p.x * 6.0, p.y * 2.0) + flow * 1.6);
    col = mix(col, surf * 1.1, (r1 * 0.5 + r2 * 0.5) * 0.3);

    // Ridged wave crests — two layers, cheaper exponents
    let c1 = pow(1.0 - abs(value_noise(vec2(p.x * 2.5, p.y * 5.0) + flow * 1.1) * 2.0 - 1.0), 4.0);
    let c2 = pow(1.0 - abs(value_noise(vec2(p.x * 4.0, p.y * 2.5) + flow2 * 1.3 + 30.0) * 2.0 - 1.0), 3.0);
    col += hi * (c1 * 0.3 + c2 * 0.25);

    // Foam sparkles — single cheap check
    let foam = value_noise(vec2(p.x * 5.0, p.y * 7.0) + flow * 2.0);
    col += hi * 0.5 * smoothstep(0.7, 0.8, foam);

    return vec4<f32>(clamp(col, vec3(0.0), vec3(1.0)), 1.0);
}

// Lighter water approximation for transition zones — 2 noise calls instead of ~8.
// Includes surface color so shore blending isn't unnaturally dark.
fn fill_water_cheap(world_px: vec2<f32>, time: f32) -> vec3<f32> {
    let p = world_px / tilemap_data.tile_size.x;
    let flow = params.water_flow_dir * params.water_flow_speed * 1.8 * time;
    let n1 = value_noise(p * 0.8 + flow * 0.5);
    let n2 = value_noise(p * 2.5 + flow * 1.2);
    var col = mix(params.water_mid.rgb, params.water_surface.rgb, n1 * 0.5);
    col += params.water_highlight.rgb * n2 * 0.15;
    return col;
}

// River with depth darkening — called from fragment when we have tile coords
fn fill_river_with_depth(world_px: vec2<f32>, time: f32, tc: vec2<i32>, local: vec2<f32>) -> vec4<f32> {
    var col = fill_water(world_px, time).rgb;

    // Depth: 0 at shore, 1 at river center
    let depth = river_depth_at(tc, local);

    // Aggressive darkening toward near-black blue at center
    let abyss = params.water_deep.rgb * 0.25;
    col = mix(col, abyss, depth * depth * 0.85); // quadratic — accelerates into deep

    // Suppress surface detail at depth — deep water is dark and still
    col = mix(col, abyss, depth * 0.4);

    return vec4<f32>(clamp(col, vec3(0.0), vec3(1.0)), 1.0);
}

// ── State: GRASS ────────────────────────────────────────────────────────────

fn fill_grass(world_px: vec2<f32>) -> vec4<f32> {
    // 1:1 pixel mapping — texture is 894x894, wraps naturally
    let uv = fract(world_px / 894.0);
    return textureSample(grass_texture, grass_sampler, uv);
}

// ── State: DIRT ─────────────────────────────────────────────────────────────

fn fill_dirt(world_px: vec2<f32>) -> vec4<f32> {
    // 1:1 pixel mapping — texture is 2048x2048, wraps naturally
    let uv = fract(world_px / 2048.0);
    return textureSample(dirt_texture, dirt_sampler, uv);
}

// ── Stone texture ───────────────────────────────────────────────────────────

fn fill_stone(world_px: vec2<f32>) -> vec4<f32> {
    let uv = fract(world_px / 2048.0);
    return textureSample(stone_texture, stone_sampler, uv);
}

// ── State: SHALLOWS ────────────────────────────────────────────────────────
// Submerged rock/stone visible through shallow water. Vertically stacks:
// counts the contiguous column of shallows tiles and applies a segmented
// opacity falloff so deeper rows show more water over the rock.

fn shallows_stack_info(tc: vec2<i32>, sample: vec4<f32>) -> vec2<f32> {
    // Read precomputed stack info from A channel (high nibble=depth_idx, low=stack_height)
    return precomputed_shallows_info(sample);
}

fn fill_shallows(world_px: vec2<f32>, time: f32, tc: vec2<i32>, local: vec2<f32>, sample: vec4<f32>) -> vec4<f32> {
    let stone_col = fill_stone(world_px).rgb;
    let water_col = fill_river_with_depth(world_px, time, tc, local).rgb;

    // Stack info: x = depth index (0=top), y = total height
    let info = shallows_stack_info(tc, sample);
    let depth_idx = info.x;
    let stack_h = info.y;

    // Continuous depth: 0 at top of shallowest tile, 1 at bottom of deepest
    let depth = (depth_idx + (1.0 - local.y)) / stack_h;

    // ── Stone base with underwater tint ─────────────────────────────
    let tint = mix(vec3(0.95, 0.95, 1.0), vec3(0.6, 0.7, 0.85), depth);
    let submerged_stone = stone_col * tint;

    // ── Darken blend — water only darkens the stone ─────────────────
    let darkened = min(submerged_stone, water_col);

    // ── Water layered OVER with stepped depth opacity ───────────────
    let steps = 5.0;
    let segment = floor(depth * steps) / steps;
    let water_alpha = mix(segment, depth, 0.35);
    let clamped_alpha = clamp(water_alpha * 0.9, 0.0, 0.85);

    var col = mix(darkened, water_col, clamped_alpha);

    // ── Caustic shimmer (fades with depth) ──────────────────────────
    let caustic = value_noise(world_px / 20.0 + vec2(time * 0.3, time * -0.2));
    let caustic2 = value_noise(world_px / 15.0 - vec2(time * 0.2, time * 0.35));
    let shimmer = pow(caustic * caustic2, 2.0) * 0.2 * (1.0 - clamped_alpha);
    col += params.water_surface.rgb * shimmer;

    return vec4<f32>(clamp(col, vec3(0.0), vec3(1.0)), 1.0);
}

// ── Fill dispatcher ─────────────────────────────────────────────────────────

fn terrain_fill(id: u32, world_px: vec2<f32>, time: f32) -> vec4<f32> {
    switch id {
        case 1u /* RIVER    */ { return fill_water(world_px, time); }
        case 2u /* GRASS    */ { return fill_grass(world_px); }
        case 4u /* DIRT     */ { return fill_dirt(world_px); }
        case 9u /* SHALLOWS */ {
            // Simplified fill — used by transitions sampling neighbors
            let stone_col = fill_stone(world_px).rgb;
            let water_col = fill_water_cheap(world_px, time);
            return vec4<f32>(mix(min(stone_col, water_col), water_col, 0.25), 1.0);
        }
        default                { return vec4<f32>(1.0, 0.0, 1.0, 1.0); }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  TRANSITIONS — edge shaders dispatched by (from, to) pair
// ═══════════════════════════════════════════════════════════════════════════

const SHORE_WIDTH: f32 = 0.35; // wide enough to see the depth gradient

// ── Transition: WATER_SHORE (any land ↔ water) ─────────────────────────────
// Simulates shallow water depth: land texture visible through water near the
// shore, fading to full water deeper in. Animated foam line at the waterline.

fn water_shore(sdf: f32, world_px: vec2<f32>, land_col: vec3<f32>,
               on_water: bool, here_id: u32, time: f32) -> vec4<f32> {
    // shore_t: 0 = far from edge, 1 = right at the boundary
    let shore_t = 1.0 - clamp(abs(sdf) / SHORE_WIDTH, 0.0, 1.0);

    if on_water {
        // ── WATERY TILE: land visible through shallow water ─────────
        let depth = 1.0 - shore_t; // 0 at shore, 1 deep

        // Submerge the land: darken and tint with the water's own colors
        let deep_col = params.water_deep.rgb;
        let mid_col = params.water_mid.rgb;
        let submerged = land_col * mix(vec3(0.7, 0.75, 0.85), deep_col * 2.0, depth * 0.6);

        // What we blend toward: for shallows, blend into stone-under-water;
        // for open water, blend into water (cheap approximation — transition zone)
        let water_col = fill_water_cheap(world_px, time);
        var far_col: vec3<f32>;
        if here_id == ID_SHALLOWS {
            // Blend toward stone — the shallows base fill handles the water overlay
            far_col = fill_stone(world_px).rgb;
        } else {
            far_col = water_col;
        }

        var col = mix(submerged, far_col, smoothstep(0.0, 0.65, depth));

        // Subtle foam at waterline — use water colors
        let foam_t = smoothstep(0.12, 0.0, depth);
        let foam_noise = value_noise(world_px / 10.0 + vec2(time * 0.5, time * -0.25));
        let foam_col = mix(mid_col, params.water_surface.rgb, 0.5) * 1.4;
        col = mix(col, foam_col, foam_t * foam_noise * 0.4);

        // Always 1.0 — the color itself already blends to water at depth
        return vec4<f32>(clamp(col, vec3(0.0), vec3(1.0)), 1.0);
    } else {
        // ── LAND TILE: wet darkening near the water's edge ──────────
        let wetness = shore_t * shore_t;
        var col = land_col * mix(1.0, 0.7, wetness);
        // Tint toward water's deep color instead of generic blue
        col = mix(col, col * (vec3(1.0) + params.water_deep.rgb * 2.0) * 0.5, wetness * 0.4);

        // Alpha starts at 1.0 at the boundary so no tile seams show
        return vec4<f32>(clamp(col, vec3(0.0), vec3(1.0)), 1.0);
    }
}

// ── Per-direction SDF helpers ────────────────────────────────────────────────
// Raw distance from pixel to tile edge, no noise. Used by the multi-blend loop.

fn sdf_cardinal(tx: f32, ty: f32, dir: u32) -> f32 {
    switch dir {
        case 0u { return 1.0 - ty; } // N
        case 1u { return ty; }       // S
        case 2u { return 1.0 - tx; } // E
        case 3u { return tx; }       // W
        default { return 999.0; }
    }
}

fn sdf_corner(tx: f32, ty: f32, dir: u32) -> f32 {
    switch dir {
        case 0u { return  sqrt((1.0-tx)*(1.0-tx) + (1.0-ty)*(1.0-ty)); } // NE convex
        case 1u { return  sqrt(tx*tx + (1.0-ty)*(1.0-ty)); }             // NW convex
        case 2u { return  sqrt((1.0-tx)*(1.0-tx) + ty*ty); }             // SE convex
        case 3u { return  sqrt(tx*tx + ty*ty); }                         // SW convex
        case 4u { return -sqrt((1.0-tx)*(1.0-tx) + (1.0-ty)*(1.0-ty)); } // NE concave
        case 5u { return -sqrt(tx*tx + (1.0-ty)*(1.0-ty)); }             // NW concave
        case 6u { return -sqrt((1.0-tx)*(1.0-tx) + ty*ty); }             // SE concave
        case 7u { return -sqrt(tx*tx + ty*ty); }                         // SW concave
        default { return 999.0; }
    }
}

// ── Transition: GRASS_BLEND (dirt ↔ grass) ──────────────────────────────────
// Organic blend: sparse grass tufts growing through dirt on the dirt side,
// grass getting patchy and thin on the grass side. Uses noise masking to
// sample both fill shaders and blend between them.

const GRASS_BLEND_WIDTH: f32 = 0.5; // wide biome-style blend

fn grass_blend(sdf: f32, world_px: vec2<f32>, base_fill: vec3<f32>, here_id: u32) -> vec4<f32> {
    // t: 0 = dirt side, 1 = grass side
    let t = clamp((GRASS_BLEND_WIDTH - sdf) / (2.0 * GRASS_BLEND_WIDTH), 0.0, 1.0);

    // Sample the other terrain's texture — reuse base_fill for our side
    var other: vec3<f32>;
    var grass_t: f32;
    if here_id == ID_DIRT {
        other = fill_grass(world_px).rgb;
        grass_t = t;
    } else {
        other = fill_dirt(world_px).rgb;
        grass_t = 1.0 - t;
    }

    // Noisy blend — organic edge like Minecraft biome blending
    let n = value_noise(world_px / tilemap_data.tile_size.x * 6.0 + 400.0);
    let blend = smoothstep(0.35, 0.65, grass_t + (n - 0.5) * 0.5);

    let dirt_col = select(base_fill, other, here_id == ID_GRASS);
    let grass_col = select(other, base_fill, here_id == ID_GRASS);
    let col = mix(dirt_col, grass_col, blend);

    let alpha = smoothstep(GRASS_BLEND_WIDTH, GRASS_BLEND_WIDTH * 0.2, abs(sdf));
    return vec4<f32>(col, alpha);
}

// ── Transition dispatcher ───────────────────────────────────────────────────

fn is_watery(id: u32) -> bool {
    return id == ID_RIVER || id == ID_SHALLOWS;
}

fn get_transition_type(here_id: u32, neighbor_id: u32) -> u32 {
    // Water ↔ Shallows — depth fade
    if is_watery(here_id) && is_watery(neighbor_id) {
        return TRANS_WATER_DEPTH;
    }
    // Any land ↔ water/shallows → water shore
    if is_watery(here_id) || is_watery(neighbor_id) {
        return TRANS_WATER_SHORE;
    }
    // Dirt ↔ Grass → biome blend
    if (here_id == ID_DIRT && neighbor_id == ID_GRASS) ||
       (here_id == ID_GRASS && neighbor_id == ID_DIRT) {
        return TRANS_GRASS_BLEND;
    }
    // Default: water shore
    return TRANS_WATER_SHORE;
}

const DEPTH_WIDTH: f32 = 0.5; // wide smooth depth transition

fn get_transition_width(trans_type: u32) -> f32 {
    switch trans_type {
        case 1u { return SHORE_WIDTH; }
        case 2u { return GRASS_BLEND_WIDTH; }
        case 3u { return DEPTH_WIDTH; }
        default { return SHORE_WIDTH; }
    }
}

fn transition_edge(
    trans_type: u32,
    sdf: f32,
    world_px: vec2<f32>,
    base_fill: vec3<f32>,
    here_id: u32,
    neighbor_id: u32,
    time: f32,
) -> vec4<f32> {
    switch trans_type {
        case 1u /* WATER_SHORE */ {
            let on_water = is_watery(here_id);
            // Land color: if we're on a watery tile, sample the neighbor's fill;
            // if we're on land, use our own base fill
            var land_col: vec3<f32>;
            if on_water {
                land_col = terrain_fill(neighbor_id, world_px, time).rgb;
            } else {
                land_col = base_fill;
            }
            let shore = water_shore(sdf, world_px, land_col, on_water, here_id, time);
            return vec4<f32>(shore.rgb, shore.a);
        }
        case 2u /* GRASS_BLEND */ {
            let gb = grass_blend(sdf, world_px, base_fill, here_id);
            return vec4<f32>(gb.rgb, gb.a);
        }
        case 3u /* WATER_DEPTH */ {
            // Smooth blend between shallows (rock under water) and deep water
            let t = clamp((DEPTH_WIDTH - sdf) / (2.0 * DEPTH_WIDTH), 0.0, 1.0);
            let other_col = terrain_fill(neighbor_id, world_px, time).rgb;
            let blend_t = smoothstep(0.3, 0.7, t);
            let col = mix(base_fill, other_col, blend_t);
            let alpha = smoothstep(DEPTH_WIDTH, DEPTH_WIDTH * 0.2, abs(sdf));
            return vec4<f32>(col, alpha);
        }
        default { return vec4(0.0); }
    }
}

// ── Transition color helper (returns premultiplied rgb + weight) ─────────

fn eval_transition(here_id: u32, neighbor_id: u32, sdf: f32,
                   world_px: vec2<f32>, base_fill: vec3<f32>, time: f32) -> vec4<f32> {
    let tt = get_transition_type(here_id, neighbor_id);
    let w = get_transition_width(tt);
    if abs(sdf) > w { return vec4(0.0); }

    let col = transition_edge(tt, sdf, world_px, base_fill, here_id, neighbor_id, time);
    // Water shore: full weight near the edge, taper at the far end to merge with base
    var weight: f32;
    if tt == TRANS_WATER_SHORE {
        weight = smoothstep(w, w * 0.5, abs(sdf)) * col.a;
    } else {
        weight = smoothstep(w, w * 0.2, abs(sdf)) * col.a;
    }
    return vec4(col.rgb * weight, weight);
}

fn is_wide_blend(here_id: u32, neighbor_id: u32) -> bool {
    let tt = get_transition_type(here_id, neighbor_id);
    return tt == TRANS_GRASS_BLEND || tt == TRANS_WATER_DEPTH;
}

// ═══════════════════════════════════════════════════════════════════════════
//  FRAGMENT — multi-transition blending
// ═══════════════════════════════════════════════════════════════════════════

@fragment
fn fragment(in: MeshVertexOutput) -> @location(0) vec4<f32> {
    let tile_pos = vec2<f32>(in.storage_position) + tilemap_data.chunk_pos;
    let world_px = vec2<f32>(
        (tile_pos.x + in.uv.z) * tilemap_data.tile_size.x,
        (tile_pos.y + 1.0 - in.uv.w) * tilemap_data.tile_size.y,
    );
    let tc = vec2<i32>(tile_pos);

    // Single texture read gets all precomputed channels
    let sample = terrain_sample_all(tc);
    let here = u32(round(sample.r * 255.0));
    if here == ID_EMPTY || here >= 10u { discard; } // IDs 10-17 are depth/slope tiles

    // Specialized fills that need tile coords / local position
    let local = vec2(in.uv.z, 1.0 - in.uv.w); // x=left→right, y=bottom→top
    var base: vec4<f32>;
    if here == ID_RIVER {
        base = fill_river_with_depth(world_px, globals.time, tc, local);
    } else if here == ID_SHALLOWS {
        base = fill_shallows(world_px, globals.time, tc, local, sample);
    } else {
        base = terrain_fill(here, world_px, globals.time);
    }
    base.a = 1.0;

    // ── Early exit: precomputed bitmask tells us if any neighbors differ ──
    let mask = neighbor_bitmask(sample);
    if mask == 0u {
        return base;
    }

    // Bit ordering: 0=N 1=S 2=E 3=W 4=NE 5=NW 6=SE 7=SW
    let bn  = (mask & 1u)   != 0u;
    let bs  = (mask & 2u)   != 0u;
    let be  = (mask & 4u)   != 0u;
    let bw  = (mask & 8u)   != 0u;
    let bne = (mask & 16u)  != 0u;
    let bnw = (mask & 32u)  != 0u;
    let bse = (mask & 64u)  != 0u;
    let bsw = (mask & 128u) != 0u;

    // Only read neighbor IDs for directions that actually differ
    var id_n  = ID_EMPTY; if bn  { id_n  = terrain_id_at(tc + vec2( 0,  1)); }
    var id_s  = ID_EMPTY; if bs  { id_s  = terrain_id_at(tc + vec2( 0, -1)); }
    var id_e  = ID_EMPTY; if be  { id_e  = terrain_id_at(tc + vec2( 1,  0)); }
    var id_w  = ID_EMPTY; if bw  { id_w  = terrain_id_at(tc + vec2(-1,  0)); }
    var id_ne = ID_EMPTY; if bne { id_ne = terrain_id_at(tc + vec2( 1,  1)); }
    var id_nw = ID_EMPTY; if bnw { id_nw = terrain_id_at(tc + vec2(-1,  1)); }
    var id_se = ID_EMPTY; if bse { id_se = terrain_id_at(tc + vec2( 1, -1)); }
    var id_sw = ID_EMPTY; if bsw { id_sw = terrain_id_at(tc + vec2(-1, -1)); }

    // Diagonal shallows → treat as water so they don't block adjacent water transitions
    if !is_watery(here) {
        if id_ne == ID_SHALLOWS { id_ne = ID_RIVER; }
        if id_nw == ID_SHALLOWS { id_nw = ID_RIVER; }
        if id_se == ID_SHALLOWS { id_se = ID_RIVER; }
        if id_sw == ID_SHALLOWS { id_sw = ID_RIVER; }
    }

    // ── Tile-local coords for SDF ───────────────────────────────────
    let tx = in.uv.z;
    let ty = 1.0 - in.uv.w;

    // Organic noise offset — single value_noise instead of fbm3
    let tile_sz = tilemap_data.tile_size.x;
    let wx = world_px.x / tile_sz;
    let wy = world_px.y / tile_sz;
    let noise = (value_noise(vec2(wx * 8.0, wy * 8.0)) - 0.5) * 0.15;

    let bf = base.rgb; // pre-computed base fill to pass into transitions

    // ── Accumulate ALL active transitions, weighted by proximity ────
    var accum_color = vec3(0.0);
    var accum_weight = 0.0;

    // Cardinal edges
    if bn {
        let sdf = sdf_cardinal(tx, ty, 0u) + noise;
        let c = eval_transition(here, id_n, sdf, world_px, bf, globals.time);
        accum_color += c.rgb; accum_weight += c.a;
    }
    if bs {
        let sdf = sdf_cardinal(tx, ty, 1u) + noise;
        let c = eval_transition(here, id_s, sdf, world_px, bf, globals.time);
        accum_color += c.rgb; accum_weight += c.a;
    }
    if be {
        let sdf = sdf_cardinal(tx, ty, 2u) + noise;
        let c = eval_transition(here, id_e, sdf, world_px, bf, globals.time);
        accum_color += c.rgb; accum_weight += c.a;
    }
    if bw {
        let sdf = sdf_cardinal(tx, ty, 3u) + noise;
        let c = eval_transition(here, id_w, sdf, world_px, bf, globals.time);
        accum_color += c.rgb; accum_weight += c.a;
    }

    // Convex corners — skip when both adjacent cardinals are wide blends
    if bn && be && !(is_wide_blend(here, id_n) && is_wide_blend(here, id_e)) {
        let sdf = sdf_corner(tx, ty, 0u) + noise;
        let c = eval_transition(here, id_ne, sdf, world_px, bf, globals.time);
        accum_color += c.rgb; accum_weight += c.a;
    }
    if bn && bw && !(is_wide_blend(here, id_n) && is_wide_blend(here, id_w)) {
        let sdf = sdf_corner(tx, ty, 1u) + noise;
        let c = eval_transition(here, id_nw, sdf, world_px, bf, globals.time);
        accum_color += c.rgb; accum_weight += c.a;
    }
    if bs && be && !(is_wide_blend(here, id_s) && is_wide_blend(here, id_e)) {
        let sdf = sdf_corner(tx, ty, 2u) + noise;
        let c = eval_transition(here, id_se, sdf, world_px, bf, globals.time);
        accum_color += c.rgb; accum_weight += c.a;
    }
    if bs && bw && !(is_wide_blend(here, id_s) && is_wide_blend(here, id_w)) {
        let sdf = sdf_corner(tx, ty, 3u) + noise;
        let c = eval_transition(here, id_sw, sdf, world_px, bf, globals.time);
        accum_color += c.rgb; accum_weight += c.a;
    }

    // Concave corners — skip for wide blends
    if !bn && !be && bne && !is_wide_blend(here, id_ne) {
        let sdf = sdf_corner(tx, ty, 4u) + noise;
        let c = eval_transition(here, id_ne, sdf, world_px, bf, globals.time);
        accum_color += c.rgb; accum_weight += c.a;
    }
    if !bn && !bw && bnw && !is_wide_blend(here, id_nw) {
        let sdf = sdf_corner(tx, ty, 5u) + noise;
        let c = eval_transition(here, id_nw, sdf, world_px, bf, globals.time);
        accum_color += c.rgb; accum_weight += c.a;
    }
    if !bs && !be && bse && !is_wide_blend(here, id_se) {
        let sdf = sdf_corner(tx, ty, 6u) + noise;
        let c = eval_transition(here, id_se, sdf, world_px, bf, globals.time);
        accum_color += c.rgb; accum_weight += c.a;
    }
    if !bs && !bw && bsw && !is_wide_blend(here, id_sw) {
        let sdf = sdf_corner(tx, ty, 7u) + noise;
        let c = eval_transition(here, id_sw, sdf, world_px, bf, globals.time);
        accum_color += c.rgb; accum_weight += c.a;
    }

    // ── Blend accumulated transitions over base fill ────────────────
    if accum_weight < 0.01 {
        return base;
    }

    let blended = accum_color / accum_weight;
    let opacity = clamp(accum_weight, 0.0, 1.0);
    return vec4<f32>(mix(base.rgb, blended, opacity), 1.0);
}
