use std::f32::consts::{FRAC_PI_2, PI};

use bevy::prelude::*;


use super::components::SunLight;

/// Tracks in-game time of day (0.0–24.0 hours).
#[derive(Resource)]
pub struct TimeOfDay {
    pub hour: f32,
    /// Game-hours per real-second.
    pub speed: f32,
    pub paused: bool,
}

impl Default for TimeOfDay {
    fn default() -> Self {
        Self {
            hour: 12.0,
            speed: 0.0,
            paused: false,
        }
    }
}

pub fn advance_time_of_day(time: Res<Time>, mut tod: ResMut<TimeOfDay>) {
    if tod.paused {
        return;
    }
    tod.hour += time.delta_secs() * tod.speed;
    tod.hour %= 24.0;
}

// Palette constants for day/night tinting.
const NIGHT_COLOR: Color = Color::linear_rgb(0.08, 0.10, 0.25);
const NIGHT_INTENSITY: f32 = 0.15;
const DAWN_COLOR: Color = Color::linear_rgb(0.9, 0.55, 0.25);
const DAY_COLOR: Color = Color::linear_rgb(1.0, 0.95, 0.80);
const DAY_INTENSITY: f32 = 1.0;

/// Maps hour-of-day to ambient color and intensity via piecewise-linear curves.
/// Night is blue moonlight (never black), daytime is warm yellow.
pub fn compute_ambient_from_time(
    tod: Res<TimeOfDay>,
    mut ambient: ResMut<GlobalAmbientLight>,
) {
    let h = tod.hour;

    let (color, intensity) = match h {
        // Night: 21–5 — dark blue moonlight, always visible
        h if h >= 21.0 || h < 5.0 => {
            (NIGHT_COLOR, NIGHT_INTENSITY)
        }
        // Pre-dawn: 5–6
        h if h < 6.0 => {
            let t = h - 5.0;
            (lerp_color(NIGHT_COLOR, DAWN_COLOR, t), lerp(NIGHT_INTENSITY, 0.35, t))
        }
        // Dawn: 6–8
        h if h < 8.0 => {
            let t = (h - 6.0) / 2.0;
            (lerp_color(DAWN_COLOR, DAY_COLOR, t), lerp(0.35, DAY_INTENSITY, t))
        }
        // Day: 8–17 — warm yellow
        h if h < 17.0 => (DAY_COLOR, DAY_INTENSITY),
        // Dusk: 17–19
        h if h < 19.0 => {
            let t = (h - 17.0) / 2.0;
            (lerp_color(DAY_COLOR, DAWN_COLOR, t), lerp(DAY_INTENSITY, 0.35, t))
        }
        // Twilight: 19–21
        _ => {
            let t = (h - 19.0) / 2.0;
            (lerp_color(DAWN_COLOR, NIGHT_COLOR, t), lerp(0.35, NIGHT_INTENSITY, t))
        }
    };

    // GlobalAmbientLight brightness is in cd/m². Scale our 0-1 range up.
    ambient.color = color;
    ambient.brightness = intensity * 200.0;
}

/// Spawns the sun DirectionalLight entity at startup.
pub fn spawn_sun_light(mut commands: Commands) {
    use bevy::light::cascade::CascadeShadowConfigBuilder;

    commands.spawn((
        SunLight,
        DirectionalLight {
            color: Color::WHITE,
            illuminance: 8_000.0,
            shadows_enabled: true,
            // Higher biases push shadow edges outward, softening them and
            // hiding hard stair-step artifacts from the shadow map.
            shadow_depth_bias: 1.0,
            shadow_normal_bias: 6.0,
            ..default()
        },
        Transform::from_rotation(
            Quat::from_euler(EulerRot::YXZ, PI * 0.5, -(FRAC_PI_2 - 60.0_f32.to_radians()), 0.0),
        ),
        CascadeShadowConfigBuilder {
            num_cascades: 1,
            minimum_distance: 600.0,
            maximum_distance: 1800.0,
            first_cascade_far_bound: 1800.0,
            overlap_proportion: 0.0,
        }
        .build(),
    ));
    // Lower shadow map resolution → larger texels → naturally softer shadow
    // edges. 1024 with Gaussian PCF gives a gentle, diffuse look.
    commands.insert_resource(bevy::light::DirectionalLightShadowMap { size: 1024 });

}

/// Rotates the sun DirectionalLight based on TimeOfDay.
pub fn update_sun_light(
    tod: Res<TimeOfDay>,
    mut sun: Query<(&mut DirectionalLight, &mut Transform), With<SunLight>>,
) {
    let Ok((mut sun_light, mut sun_tf)) = sun.single_mut() else {
        return;
    };

    let h = tod.hour;
    let is_day = h >= 6.0 && h <= 18.0;

    if !is_day {
        // Faint blue-white moonlight so nighttime is never pitch black.
        sun_light.illuminance = 400.0;
        sun_light.color = Color::linear_rgb(0.6, 0.7, 1.0);
        return;
    }

    let t = (h - 6.0) / 12.0; // 0 at sunrise, 1 at sunset
    let elevation = (t * PI).sin(); // peaks at noon

    // Azimuth: sweep 60°–120° so the sun always faces into the billboard
    // plane (billboards lie in XY, face +Z toward camera). The narrow range
    // keeps shadows consistently thick — the player can't see the sun, so
    // visual shadow quality matters more than a wide sweep.
    let az_min = 60.0_f32.to_radians();
    let az_max = 120.0_f32.to_radians();
    let azimuth = az_min + t * (az_max - az_min);

    // Elevation angle: 45°–58° keeps the sun steep enough for short,
    // compact shadows, but never so overhead that billboard shadows go thin.
    let min_elev = 45.0_f32.to_radians();
    let max_elev = 58.0_f32.to_radians();
    let elevation_angle = min_elev + elevation * (max_elev - min_elev);

    // Rotate directional light: it shines along its -Z in local space
    // We want it pointing downward at the elevation angle, sweeping east to west
    sun_tf.rotation = Quat::from_euler(
        EulerRot::YXZ,
        azimuth,
        -(FRAC_PI_2 - elevation_angle),
        0.0,
    );

    // Illuminance: bright at noon, dim at dawn/dusk
    let dawn_dusk_fade = (elevation * 4.0).clamp(0.0, 1.0);
    sun_light.illuminance = elevation * 8_000.0 * dawn_dusk_fade;

    // Color: warm golden at dawn/dusk, soft warm yellow at noon.
    let warmth = 1.0 - elevation; // 1 at horizon, 0 at noon
    sun_light.color = Color::linear_rgb(
        1.0,
        lerp(0.92, 0.65, warmth),  // noon: slightly warm, horizon: golden
        lerp(0.75, 0.25, warmth),   // noon: warm yellow, horizon: deep amber
    );
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let a = a.to_linear();
    let b = b.to_linear();
    Color::linear_rgb(
        lerp(a.red, b.red, t),
        lerp(a.green, b.green, t),
        lerp(a.blue, b.blue, t),
    )
}
