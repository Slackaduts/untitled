use bevy::prelude::*;

use super::ambient::AmbientConfig;

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
pub fn compute_ambient_from_time(tod: Res<TimeOfDay>, mut ambient: ResMut<AmbientConfig>) {
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

    ambient.color = color;
    ambient.intensity = intensity;
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
