use std::collections::HashMap;

use bevy::prelude::*;
use bevy_hanabi::prelude::*;
use bevy_hanabi::Gradient as HanabiGradient;
use serde::{Deserialize, Serialize};

// ── Enums ────────────────────────────────────────────────────────────────────

/// Direction particles are emitted in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EmissionDirection {
    /// Random direction in a sphere.
    Sphere,
    /// Within a cone defined by angle (radians) around a direction vector.
    Cone { angle: f32, direction: [f32; 3] },
    /// Straight up (+Z in our Z-up world).
    Up,
    /// Outward from a ring of given radius.
    Ring { radius: f32 },
}

impl Default for EmissionDirection {
    fn default() -> Self {
        Self::Sphere
    }
}

/// Shape of the emission volume.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EmissionShape {
    Point,
    Sphere { radius: f32 },
    Box { half_extents: [f32; 3] },
    Ring { radius: f32, width: f32 },
}

impl Default for EmissionShape {
    fn default() -> Self {
        Self::Point
    }
}

/// Blending mode for particle rendering.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum ParticleBlend {
    #[default]
    Additive,
    Alpha,
}

/// Per-particle light emission definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticleLightDef {
    #[serde(default = "default_light_color")]
    pub color: [f32; 3],
    #[serde(default = "default_light_intensity")]
    pub intensity: f32,
    #[serde(default = "default_light_radius")]
    pub radius: f32,
}

fn default_light_color() -> [f32; 3] {
    [1.0, 0.85, 0.6]
}
fn default_light_intensity() -> f32 {
    1.0
}
fn default_light_radius() -> f32 {
    40.0
}

impl Default for ParticleLightDef {
    fn default() -> Self {
        Self {
            color: default_light_color(),
            intensity: default_light_intensity(),
            radius: default_light_radius(),
        }
    }
}

// ── ParticleDef ──────────────────────────────────────────────────────────────

/// A complete particle definition, loaded from JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticleDef {
    pub id: String,

    // Lifetime
    #[serde(default = "default_lifetime")]
    pub lifetime: (f32, f32),

    // Motion
    #[serde(default = "default_speed_range")]
    pub speed_range: (f32, f32),
    #[serde(default)]
    pub direction: EmissionDirection,
    #[serde(default)]
    pub gravity: f32,
    #[serde(default)]
    pub drag: f32,

    // Appearance
    #[serde(default = "default_color_start")]
    pub color_start: [f32; 4],
    #[serde(default = "default_color_end")]
    pub color_end: [f32; 4],
    #[serde(default = "default_size_start")]
    pub size_start: f32,
    #[serde(default = "default_size_end")]
    pub size_end: f32,
    #[serde(default)]
    pub texture: Option<String>,
    #[serde(default)]
    pub blend_mode: ParticleBlend,

    // Emission shape
    #[serde(default)]
    pub emission_shape: EmissionShape,

    // Rotation
    #[serde(default)]
    pub rotation_range: Option<(f32, f32)>,
    #[serde(default)]
    pub angular_velocity: Option<(f32, f32)>,

    // Per-particle light
    #[serde(default)]
    pub light: Option<ParticleLightDef>,
}

fn default_lifetime() -> (f32, f32) {
    (1.0, 2.0)
}
fn default_speed_range() -> (f32, f32) {
    (10.0, 30.0)
}
fn default_color_start() -> [f32; 4] {
    [1.0, 1.0, 1.0, 1.0]
}
fn default_color_end() -> [f32; 4] {
    [1.0, 1.0, 1.0, 0.0]
}
fn default_size_start() -> f32 {
    4.0
}
fn default_size_end() -> f32 {
    1.0
}

impl Default for ParticleDef {
    fn default() -> Self {
        Self {
            id: "unnamed".to_string(),
            lifetime: default_lifetime(),
            speed_range: default_speed_range(),
            direction: EmissionDirection::default(),
            gravity: 0.0,
            drag: 0.0,
            color_start: default_color_start(),
            color_end: default_color_end(),
            size_start: default_size_start(),
            size_end: default_size_end(),
            texture: None,
            blend_mode: ParticleBlend::default(),
            emission_shape: EmissionShape::default(),
            rotation_range: None,
            angular_velocity: None,
            light: None,
        }
    }
}

// ── Hanabi conversion ────────────────────────────────────────────────────────

impl ParticleDef {
    /// Convert this definition into a hanabi `EffectAsset` for GPU rendering.
    pub fn to_effect_asset(&self, rate: f32, max_particles: u32) -> EffectAsset {
        let writer = ExprWriter::new();

        // ── Lifetime ───────────────────────────────────────────
        let lifetime = writer
            .lit(self.lifetime.0)
            .uniform(writer.lit(self.lifetime.1))
            .expr();
        let init_lifetime = SetAttributeModifier::new(Attribute::LIFETIME, lifetime);
        let init_age = SetAttributeModifier::new(Attribute::AGE, writer.lit(0.0).expr());

        // ── Velocity ───────────────────────────────────────────
        let speed_min = self.speed_range.0;
        let speed_max = self.speed_range.1;
        let speed = writer.lit(speed_min).uniform(writer.lit(speed_max));

        let init_vel: SetAttributeModifier = match &self.direction {
            EmissionDirection::Up => {
                let vel = writer.lit(0.0).vec3(writer.lit(0.0), speed);
                SetAttributeModifier::new(Attribute::VELOCITY, vel.expr())
            }
            _ => {
                // For Sphere, Cone, Ring — use a random sphere direction scaled by speed.
                // (SetVelocitySphereModifier requires position to be set first,
                //  so for Point emission we use random direction instead.)
                let dir = writer.rand(VectorType::VEC3F) * writer.lit(2.0) - writer.lit(1.0);
                let vel = dir.normalized() * speed;
                SetAttributeModifier::new(Attribute::VELOCITY, vel.expr())
            }
        };

        // ── Color gradient ─────────────────────────────────────
        let mut color_gradient: HanabiGradient<Vec4> = HanabiGradient::new();
        color_gradient.add_key(
            0.0,
            Vec4::new(
                self.color_start[0],
                self.color_start[1],
                self.color_start[2],
                self.color_start[3],
            ),
        );
        color_gradient.add_key(
            1.0,
            Vec4::new(
                self.color_end[0],
                self.color_end[1],
                self.color_end[2],
                self.color_end[3],
            ),
        );

        // ── Size gradient ──────────────────────────────────────
        let mut size_gradient: HanabiGradient<Vec3> = HanabiGradient::new();
        size_gradient.add_key(0.0, Vec3::splat(self.size_start));
        size_gradient.add_key(1.0, Vec3::splat(self.size_end));

        // ── Spawner ────────────────────────────────────────────
        let spawner = SpawnerSettings::rate(rate.into());

        // ── Build effect ───────────────────────────────────────
        // We need to finish the module before building the effect, so all
        // expressions must be created before this point.

        // Gravity (Z-up world, so gravity pulls -Z).
        let gravity_expr = writer.lit(Vec3::new(0.0, 0.0, -self.gravity)).expr();
        let drag_expr = writer.lit(self.drag).expr();

        let module = writer.finish();

        let mut effect = EffectAsset::new(max_particles, spawner, module)
            .with_name(&self.id)
            .with_simulation_space(SimulationSpace::Local)
            .init(init_lifetime)
            .init(init_age)
            .init(init_vel);

        // Alpha mode.
        effect = match self.blend_mode {
            ParticleBlend::Additive => effect.with_alpha_mode(bevy_hanabi::AlphaMode::Add),
            ParticleBlend::Alpha => effect.with_alpha_mode(bevy_hanabi::AlphaMode::Blend),
        };

        // Update modifiers.
        if self.gravity != 0.0 {
            effect = effect.update(AccelModifier::new(gravity_expr));
        }
        if self.drag > 0.0 {
            effect = effect.update(LinearDragModifier::new(drag_expr));
        }

        // Render modifiers.
        effect = effect
            .render(OrientModifier::new(OrientMode::FaceCameraPosition))
            .render(ColorOverLifetimeModifier {
                gradient: color_gradient,
                blend: ColorBlendMode::Overwrite,
                mask: ColorBlendMask::RGBA,
            })
            .render(SizeOverLifetimeModifier {
                gradient: size_gradient,
                screen_space_size: false,
            });

        effect
    }
}

// ── Registry ─────────────────────────────────────────────────────────────────

/// Registry of all loaded particle definitions, keyed by ID.
#[derive(Resource, Default)]
pub struct ParticleRegistry {
    pub defs: HashMap<String, ParticleDef>,
}

/// Scans `assets/particles/*.json` at startup and populates the registry.
pub fn load_particle_defs(mut registry: ResMut<ParticleRegistry>) {
    let base = std::path::PathBuf::from("assets/particles");
    let Ok(entries) = std::fs::read_dir(&base) else {
        warn!("No assets/particles/ directory found");
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            match std::fs::read_to_string(&path) {
                Ok(contents) => match serde_json::from_str::<ParticleDef>(&contents) {
                    Ok(def) => {
                        info!("Loaded particle def: {}", def.id);
                        registry.defs.insert(def.id.clone(), def);
                    }
                    Err(e) => warn!("Failed to parse {}: {e}", path.display()),
                },
                Err(e) => warn!("Failed to read {}: {e}", path.display()),
            }
        }
    }

    info!(
        "Particle registry: {} definition(s) loaded",
        registry.defs.len()
    );
}
