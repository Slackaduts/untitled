use std::f32::consts::TAU;

use bevy::prelude::*;

use super::components::LightSource;

/// Drives LightSource intensity from pulse and flicker configs.
/// Pulse and flicker compose multiplicatively: final = base * pulse_factor * flicker_factor.
pub fn animate_lights(time: Res<Time>, mut lights: Query<&mut LightSource>) {
    let t = time.elapsed_secs();
    let dt = time.delta_secs();

    for mut light in lights.iter_mut() {
        let mut factor = 1.0;

        // ── Pulse: smooth sine oscillation ──────────────────────────
        if let Some(pulse) = &light.pulse {
            let wave = ((t * pulse.speed + light.anim_seed) * TAU).sin() * 0.5 + 0.5;
            factor *= pulse.min + wave * (pulse.max - pulse.min);
        }

        // ── Flicker: occasional sharp dip ───────────────────────────
        if let Some(flicker) = light.flicker {
            if light.flicker_remaining > 0.0 {
                // Currently flickering — apply dip
                light.flicker_remaining -= dt;
                // Ease out of the flicker (sharp in, smooth out)
                let progress = (light.flicker_remaining / flicker.duration).clamp(0.0, 1.0);
                let dip_factor = flicker.dip + (1.0 - flicker.dip) * (1.0 - progress);
                factor *= dip_factor;
            } else {
                // Count down to next flicker
                light.flicker_countdown -= dt;
                if light.flicker_countdown <= 0.0 {
                    // Trigger a flicker
                    light.flicker_remaining = flicker.duration;
                    // Schedule next one
                    let range = flicker.max_delay - flicker.min_delay;
                    let random_offset = pseudo_random(t + light.anim_seed) * range;
                    light.flicker_countdown = flicker.min_delay + random_offset;
                }
            }
        }

        light.intensity = light.base_intensity * factor;
    }
}

/// Cheap deterministic pseudo-random in [0, 1] from a float seed.
fn pseudo_random(x: f32) -> f32 {
    let x = (x * 12.9898 + 78.233).sin() * 43758.5453;
    x.fract().abs()
}
