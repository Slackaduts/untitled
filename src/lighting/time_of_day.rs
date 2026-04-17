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

/// Maps hour-of-day to ambient color and intensity via piecewise-linear curves.
/// Writes to Bevy's built-in GlobalAmbientLight resource.
pub fn compute_ambient_from_time(
    tod: Res<TimeOfDay>,
    mut ambient: ResMut<GlobalAmbientLight>,
) {
    let h = tod.hour;

    let (color, intensity) = match h {
        // Night: 21–5
        h if h >= 21.0 || h < 5.0 => {
            (Color::linear_rgb(0.05, 0.05, 0.15), 0.08)
        }
        // Pre-dawn: 5–6
        h if h < 6.0 => {
            let t = h - 5.0;
            let c = lerp_color(
                Color::linear_rgb(0.05, 0.05, 0.15),
                Color::linear_rgb(0.8, 0.5, 0.2),
                t,
            );
            (c, lerp(0.08, 0.3, t))
        }
        // Dawn: 6–8
        h if h < 8.0 => {
            let t = (h - 6.0) / 2.0;
            let c = lerp_color(
                Color::linear_rgb(0.8, 0.5, 0.2),
                Color::WHITE,
                t,
            );
            (c, lerp(0.3, 1.0, t))
        }
        // Day: 8–17
        h if h < 17.0 => (Color::WHITE, 1.0),
        // Dusk: 17–19
        h if h < 19.0 => {
            let t = (h - 17.0) / 2.0;
            let c = lerp_color(
                Color::WHITE,
                Color::linear_rgb(0.8, 0.5, 0.2),
                t,
            );
            (c, lerp(1.0, 0.3, t))
        }
        // Twilight: 19–21
        _ => {
            let t = (h - 19.0) / 2.0;
            let c = lerp_color(
                Color::linear_rgb(0.8, 0.5, 0.2),
                Color::linear_rgb(0.05, 0.05, 0.15),
                t,
            );
            (c, lerp(0.3, 0.08, t))
        }
    };

    // GlobalAmbientLight brightness is in cd/m². Scale our 0-1 range up.
    ambient.color = color;
    ambient.brightness = intensity * 200.0;
}

/// Spawns the sun DirectionalLight entity at startup.
pub fn spawn_sun_light(mut commands: Commands) {
    use bevy::light::cascade::CascadeShadowConfigBuilder;
    use bevy::camera::visibility::RenderLayers;

    commands.spawn((
        SunLight,
        DirectionalLight {
            color: Color::WHITE,
            illuminance: 8_000.0,
            shadows_enabled: true,
            shadow_depth_bias: 0.5,
            shadow_normal_bias: 4.0,
            ..default()
        },
        Transform::from_rotation(
            Quat::from_euler(EulerRot::YXZ, PI * 0.5, -(FRAC_PI_2 - 60.0_f32.to_radians()), 0.0),
        ),
        CascadeShadowConfigBuilder {
            num_cascades: 2,
            minimum_distance: 600.0,
            maximum_distance: 1800.0,
            first_cascade_far_bound: 1000.0,
            overlap_proportion: 0.3,
        }
        .build(),
        RenderLayers::from_layers(&[0, crate::camera::shadow_mesh::SHADOW_CASTER_LAYER]),
    ));
    commands.insert_resource(bevy::light::DirectionalLightShadowMap { size: 2048 });
    // 256px per cube face × 6 faces = ~1.5MB per shadow-casting point light,
    // and 4x lower fill cost vs 512px. Still readable at typical camera distance.
    commands.insert_resource(bevy::light::PointLightShadowMap { size: 256 });
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
        sun_light.illuminance = 0.0;
        return;
    }

    let t = (h - 6.0) / 12.0; // 0 at sunrise, 1 at sunset
    let elevation = (t * PI).sin(); // peaks at noon
    let azimuth = t * PI;

    // Elevation angle: keep the sun steep (40°–80°) so shadow displacement stays small.
    let min_elev = 40.0_f32.to_radians();
    let max_elev = 80.0_f32.to_radians();
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
    // Fade smoothly at horizon
    let dawn_dusk_fade = (elevation * 4.0).clamp(0.0, 1.0);
    sun_light.illuminance = elevation * 8_000.0 * dawn_dusk_fade;

    // Color shift: warm at dawn/dusk, white at noon
    let warmth = 1.0 - elevation; // 0 at noon, 1 at horizon
    sun_light.color = Color::linear_rgb(
        1.0,
        1.0 - warmth * 0.3,
        1.0 - warmth * 0.5,
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
