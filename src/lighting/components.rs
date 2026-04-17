use std::f32::consts::FRAC_PI_2;

use bevy::prelude::*;

/// Marker for the sun DirectionalLight entity.
#[derive(Component)]
pub struct SunLight;

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

/// Marker for lights that interact with billboard normal maps.
/// When a light has this component, nearby billboards will show
/// normal-mapped shading from this light source.
/// Debug lights always have this.
#[derive(Component)]
pub struct InteractiveLight;

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

/// Max number of point/spot lights that cast shadows simultaneously.
/// Only the closest lights to the camera get shadows; the rest illuminate without shadows.
pub const SHADOW_BUDGET: usize = 3;

/// Intensity scaling factor: LightSource intensity (0-5 range) → lumens for Bevy lights.
/// Bevy PBR uses physical units (lumens) with inverse-square falloff. Our world units
/// are pixels (~48px per tile), so lights at range 100+ need enormous lumen values
/// to produce visible illumination at that "distance."
const INTENSITY_SCALE: f32 = 100_000_000.0;

/// Enables shadows on the closest N lights to the camera, disables on the rest.
/// Keeps total shadow map cost bounded regardless of light count.
pub fn manage_shadow_budget(
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut point_lights: Query<(Entity, &GlobalTransform, &mut PointLight)>,
    mut spot_lights: Query<(Entity, &GlobalTransform, &mut SpotLight)>,
) {
    let Ok(cam_gt) = camera.iter().next().ok_or(()) else { return };
    let cam_pos = cam_gt.translation();

    // Collect all lights with distances
    let mut light_dists: Vec<(f32, Entity, bool)> = Vec::new(); // (dist, entity, is_point)
    for (entity, gt, _) in &point_lights {
        let dist = gt.translation().distance(cam_pos);
        light_dists.push((dist, entity, true));
    }
    for (entity, gt, _) in &spot_lights {
        let dist = gt.translation().distance(cam_pos);
        light_dists.push((dist, entity, false));
    }

    light_dists.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    // Enable shadows on closest N, disable on the rest
    for (i, &(_, entity, is_point)) in light_dists.iter().enumerate() {
        let enable = i < SHADOW_BUDGET;
        if is_point {
            if let Ok((_, _, mut pl)) = point_lights.get_mut(entity) {
                pl.shadows_enabled = enable;
            }
        } else {
            if let Ok((_, _, mut sl)) = spot_lights.get_mut(entity) {
                sl.shadows_enabled = enable;
            }
        }
    }
}

/// Syncs LightSource data to Bevy PointLight/SpotLight components every frame.
/// Runs after animation so animated intensity values are propagated.
pub fn sync_light_components(
    mut commands: Commands,
    mut point_lights: Query<(&LightSource, &mut PointLight), Without<SpotLight>>,
    mut spot_lights: Query<(&LightSource, &mut SpotLight, &mut Transform)>,
    new_lights: Query<(Entity, &LightSource), (Without<PointLight>, Without<SpotLight>)>,
) {
    // Update existing PointLights
    for (ls, mut pl) in &mut point_lights {
        pl.color = ls.color;
        pl.intensity = ls.intensity * INTENSITY_SCALE;
        pl.range = ls.outer_radius;
    }

    // Update existing SpotLights
    for (ls, mut sl, mut tf) in &mut spot_lights {
        sl.color = ls.color;
        sl.intensity = ls.intensity * INTENSITY_SCALE;
        sl.range = ls.outer_radius;
        if let LightShape::Cone { direction, angle } = ls.shape {
            sl.outer_angle = (angle * 0.5).min(FRAC_PI_2 - 0.01);
            sl.inner_angle = angle * 0.15;
            // Rotate transform to face cone direction
            tf.rotation = Quat::from_rotation_z(direction);
        }
    }

    // Create Bevy light components for new LightSource entities.
    // Use try_insert to handle entities despawned between frames (e.g., editor preview lights).
    for (entity, ls) in &new_lights {
        match ls.shape {
            LightShape::Point | LightShape::Line { .. } | LightShape::Capsule { .. } => {
                commands.entity(entity).try_insert(PointLight {
                    color: ls.color,
                    intensity: ls.intensity * INTENSITY_SCALE,
                    range: ls.outer_radius,
                    shadows_enabled: false,
                    ..default()
                });
            }
            LightShape::Cone { direction, angle } => {
                commands.entity(entity).try_insert(SpotLight {
                    color: ls.color,
                    intensity: ls.intensity * INTENSITY_SCALE,
                    range: ls.outer_radius,
                    outer_angle: (angle * 0.5).min(FRAC_PI_2 - 0.01),
                    inner_angle: angle * 0.15,
                    shadows_enabled: false,
                    ..default()
                });
            }
        }
    }
}
