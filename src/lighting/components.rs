use bevy::prelude::*;

/// Occasional intensity dip (campfire pop, torch gutter).
#[derive(Clone, Copy, Reflect)]
pub struct FlickerConfig {
    /// Minimum seconds between flickers.
    pub min_delay: f32,
    /// Maximum seconds between flickers.
    pub max_delay: f32,
    /// Intensity multiplier during a flicker (e.g., 0.3 = dips to 30%).
    pub dip: f32,
    /// How long the flicker lasts in seconds.
    pub duration: f32,
}

impl Default for FlickerConfig {
    fn default() -> Self {
        Self {
            min_delay: 2.0,
            max_delay: 4.0,
            dip: 0.3,
            duration: 0.08,
        }
    }
}

/// Smooth sine-wave intensity oscillation (magical glow, lantern sway).
#[derive(Clone, Copy, Reflect)]
pub struct PulseConfig {
    /// Minimum intensity multiplier.
    pub min: f32,
    /// Maximum intensity multiplier.
    pub max: f32,
    /// Cycles per second.
    pub speed: f32,
}

impl Default for PulseConfig {
    fn default() -> Self {
        Self {
            min: 0.8,
            max: 1.0,
            speed: 1.5,
        }
    }
}

/// The geometric shape of a light source.
#[derive(Clone, Copy, Reflect, Default, PartialEq)]
pub enum LightShape {
    /// Radial falloff from a single point.
    #[default]
    Point,
    /// Wedge-shaped light (wall torch casting into a room).
    /// `direction` is the angle in radians (0 = right, PI/2 = up).
    /// `angle` is the full cone spread in radians.
    Cone { direction: f32, angle: f32 },
    /// Light emitting along a line segment (doorway, window sill).
    /// `end_offset` is the world-unit offset from position to the second endpoint.
    Line { end_offset: Vec2 },
    /// Elongated point light along a direction (corridor sconce).
    /// Like a line light with rounded caps.
    Capsule { direction: f32, half_length: f32 },
}

/// A 2D light source with configurable shape and falloff.
#[derive(Component, Reflect)]
pub struct LightSource {
    pub color: Color,
    /// Base intensity — animations modulate around this value.
    pub base_intensity: f32,
    /// Current effective intensity (written by animation system, read by renderer).
    pub intensity: f32,
    /// Full brightness within this world-unit radius.
    pub inner_radius: f32,
    /// Fades to zero at this world-unit radius.
    pub outer_radius: f32,
    /// Geometric shape of the light.
    pub shape: LightShape,
    /// Optional smooth oscillation. Composable with flicker.
    pub pulse: Option<PulseConfig>,
    /// Optional occasional intensity dip. Composable with pulse.
    pub flicker: Option<FlickerConfig>,
    /// Per-instance phase seed (keeps animations unique per light).
    pub anim_seed: f32,
    // ── Flicker runtime state (managed by animation system) ─────────
    /// Countdown until next flicker event.
    pub flicker_countdown: f32,
    /// Time remaining in current flicker (0 = not flickering).
    pub flicker_remaining: f32,
}

impl Default for LightSource {
    fn default() -> Self {
        Self {
            color: Color::WHITE,
            base_intensity: 1.0,
            intensity: 1.0,
            inner_radius: 32.0,
            outer_radius: 128.0,
            shape: LightShape::Point,
            pulse: None,
            flicker: None,
            anim_seed: 0.0,
            flicker_countdown: 0.0,
            flicker_remaining: 0.0,
        }
    }
}
