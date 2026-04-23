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

/// Shape of individual particle meshes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum ParticleShape {
    #[default]
    Quad,
    Circle,
    Triangle,
    Diamond,
    Hexagon,
    Star,
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

/// A persistent light attached to the emitter itself (not per-particle).
/// Useful for constant light sources like fires, torches, etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmitterLightDef {
    #[serde(default = "default_light_color")]
    pub color: [f32; 3],
    #[serde(default = "default_emitter_light_intensity")]
    pub intensity: f32,
    #[serde(default = "default_emitter_light_radius")]
    pub radius: f32,
    #[serde(default)]
    pub pulse: bool,
    #[serde(default)]
    pub flicker: bool,
}

fn default_emitter_light_intensity() -> f32 { 2.0 }
fn default_emitter_light_radius() -> f32 { 120.0 }

impl Default for EmitterLightDef {
    fn default() -> Self {
        Self {
            color: default_light_color(),
            intensity: default_emitter_light_intensity(),
            radius: default_emitter_light_radius(),
            pulse: false,
            flicker: false,
        }
    }
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

// ── Gradient stops ──────────────────────────────────────────────────────────

/// A color key in a multi-stop gradient (position 0-1 over particle lifetime).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorStop {
    /// Position along the particle lifetime (0.0 = birth, 1.0 = death).
    pub t: f32,
    /// RGBA color at this position.
    pub color: [f32; 4],
}

/// A size key in a multi-stop gradient (position 0-1 over particle lifetime).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SizeStop {
    /// Position along the particle lifetime (0.0 = birth, 1.0 = death).
    pub t: f32,
    /// Size at this position.
    pub size: f32,
}

/// Piecewise-linear interpolation across a sorted list of stops.
pub fn sample_gradient_color(stops: &[ColorStop], t: f32) -> [f32; 4] {
    if stops.is_empty() {
        return [1.0, 1.0, 1.0, 1.0];
    }
    if stops.len() == 1 || t <= stops[0].t {
        return stops[0].color;
    }
    if t >= stops.last().unwrap().t {
        return stops.last().unwrap().color;
    }
    // Find the two surrounding stops.
    for i in 0..stops.len() - 1 {
        if t >= stops[i].t && t <= stops[i + 1].t {
            let seg_t = if stops[i + 1].t > stops[i].t {
                (t - stops[i].t) / (stops[i + 1].t - stops[i].t)
            } else {
                0.0
            };
            let a = &stops[i].color;
            let b = &stops[i + 1].color;
            return [
                a[0] + (b[0] - a[0]) * seg_t,
                a[1] + (b[1] - a[1]) * seg_t,
                a[2] + (b[2] - a[2]) * seg_t,
                a[3] + (b[3] - a[3]) * seg_t,
            ];
        }
    }
    stops.last().unwrap().color
}

pub fn sample_gradient_size(stops: &[SizeStop], t: f32) -> f32 {
    if stops.is_empty() {
        return 1.0;
    }
    if stops.len() == 1 || t <= stops[0].t {
        return stops[0].size;
    }
    if t >= stops.last().unwrap().t {
        return stops.last().unwrap().size;
    }
    for i in 0..stops.len() - 1 {
        if t >= stops[i].t && t <= stops[i + 1].t {
            let seg_t = if stops[i + 1].t > stops[i].t {
                (t - stops[i].t) / (stops[i + 1].t - stops[i].t)
            } else {
                0.0
            };
            return stops[i].size + (stops[i + 1].size - stops[i].size) * seg_t;
        }
    }
    stops.last().unwrap().size
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

    // Appearance — multi-stop gradients
    /// Color gradient over lifetime. If empty on load, synthesized from legacy color_start/end.
    #[serde(default)]
    pub color_stops: Vec<ColorStop>,
    /// Size gradient over lifetime. If empty on load, synthesized from legacy size_start/end.
    #[serde(default)]
    pub size_stops: Vec<SizeStop>,

    // Legacy fields — kept for backward compat with old JSON files.
    // On deserialization, if color_stops is empty these are used to populate it.
    #[serde(default = "default_color_start")]
    pub color_start: [f32; 4],
    #[serde(default = "default_color_end")]
    pub color_end: [f32; 4],
    #[serde(default = "default_size_start")]
    pub size_start: f32,
    #[serde(default = "default_size_end")]
    pub size_end: f32,

    #[serde(default)]
    pub shape: ParticleShape,
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

    // Persistent lights attached to the emitter itself
    #[serde(default)]
    pub emitter_lights: Vec<EmitterLightDef>,
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

impl ParticleDef {
    /// Returns the color gradient, synthesizing from legacy fields if needed.
    pub fn color_gradient(&self) -> Vec<ColorStop> {
        if !self.color_stops.is_empty() {
            return self.color_stops.clone();
        }
        vec![
            ColorStop { t: 0.0, color: self.color_start },
            ColorStop { t: 1.0, color: self.color_end },
        ]
    }

    /// Returns the size gradient, synthesizing from legacy fields if needed.
    pub fn size_gradient(&self) -> Vec<SizeStop> {
        if !self.size_stops.is_empty() {
            return self.size_stops.clone();
        }
        vec![
            SizeStop { t: 0.0, size: self.size_start },
            SizeStop { t: 1.0, size: self.size_end },
        ]
    }

    /// Ensure color_stops/size_stops are populated (call after deserialization).
    pub fn migrate_legacy_fields(&mut self) {
        if self.color_stops.is_empty() {
            self.color_stops = vec![
                ColorStop { t: 0.0, color: self.color_start },
                ColorStop { t: 1.0, color: self.color_end },
            ];
        }
        if self.size_stops.is_empty() {
            self.size_stops = vec![
                SizeStop { t: 0.0, size: self.size_start },
                SizeStop { t: 1.0, size: self.size_end },
            ];
        }
    }
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
            color_stops: vec![
                ColorStop { t: 0.0, color: default_color_start() },
                ColorStop { t: 1.0, color: default_color_end() },
            ],
            size_stops: vec![
                SizeStop { t: 0.0, size: default_size_start() },
                SizeStop { t: 1.0, size: default_size_end() },
            ],
            color_start: default_color_start(),
            color_end: default_color_end(),
            size_start: default_size_start(),
            size_end: default_size_end(),
            shape: ParticleShape::default(),
            texture: None,
            blend_mode: ParticleBlend::default(),
            emission_shape: EmissionShape::default(),
            rotation_range: None,
            angular_velocity: None,
            light: None,
            emitter_lights: Vec::new(),
        }
    }
}

// ── Hanabi conversion ────────────────────────────────────────────────────────

impl ParticleDef {
    /// Convert this definition into a hanabi `EffectAsset` for GPU rendering.
    /// `ground_z` is the terrain Z at the emitter — particles below this are killed.
    pub fn to_effect_asset(&self, rate: f32, max_particles: u32, ground_z: f32) -> EffectAsset {
        let writer = ExprWriter::new();

        // ── Lifetime ───────────────────────────────────────────
        let lifetime = writer
            .lit(self.lifetime.0)
            .uniform(writer.lit(self.lifetime.1))
            .expr();
        let init_lifetime = SetAttributeModifier::new(Attribute::LIFETIME, lifetime);
        let init_age = SetAttributeModifier::new(Attribute::AGE, writer.lit(0.0).expr());

        // ── Emission position ─────────────────────────────────
        // Position must be initialized BEFORE velocity (SetVelocitySphereModifier
        // reads particle.POSITION to compute the radial direction).
        enum PosInit {
            Attr(SetAttributeModifier),
            Sphere(SetPositionSphereModifier),
            Circle(SetPositionCircleModifier),
        }
        let pos_init = match &self.emission_shape {
            EmissionShape::Point => {
                PosInit::Attr(SetAttributeModifier::new(
                    Attribute::POSITION,
                    writer.lit(Vec3::ZERO).expr(),
                ))
            }
            EmissionShape::Sphere { radius } => {
                PosInit::Sphere(SetPositionSphereModifier {
                    center: writer.lit(Vec3::ZERO).expr(),
                    radius: writer.lit(*radius).expr(),
                    dimension: ShapeDimension::Volume,
                })
            }
            EmissionShape::Box { half_extents } => {
                // Random position in box via expressions.
                let he = *half_extents;
                let x = writer.lit(-he[0]).uniform(writer.lit(he[0]));
                let y = writer.lit(-he[1]).uniform(writer.lit(he[1]));
                let z = writer.lit(-he[2]).uniform(writer.lit(he[2]));
                let pos = x.vec3(y, z);
                PosInit::Attr(SetAttributeModifier::new(Attribute::POSITION, pos.expr()))
            }
            EmissionShape::Ring { radius, width } => {
                PosInit::Circle(SetPositionCircleModifier {
                    center: writer.lit(Vec3::ZERO).expr(),
                    axis: writer.lit(Vec3::Z).expr(),
                    radius: writer.lit(*radius + width * 0.5).expr(),
                    dimension: ShapeDimension::Surface,
                })
            }
        };

        // ── Velocity ───────────────────────────────────────────
        let speed_expr = writer
            .lit(self.speed_range.0)
            .uniform(writer.lit(self.speed_range.1))
            .expr();

        enum VelInit {
            Attr(SetAttributeModifier),
            Sphere(SetVelocitySphereModifier),
        }
        // SetVelocitySphereModifier computes normalize(POSITION - center) * speed.
        // This produces NaN when emission_shape is Point (POSITION == center == ZERO).
        // Use random direction for Point emission instead.
        let is_point_emission = matches!(self.emission_shape, EmissionShape::Point);

        let vel_init = match &self.direction {
            EmissionDirection::Up => {
                let speed = writer.lit(self.speed_range.0).uniform(writer.lit(self.speed_range.1));
                let vel = writer.lit(0.0).vec3(writer.lit(0.0), speed);
                VelInit::Attr(SetAttributeModifier::new(Attribute::VELOCITY, vel.expr()))
            }
            EmissionDirection::Sphere | EmissionDirection::Ring { .. } | EmissionDirection::Cone { .. } => {
                if is_point_emission {
                    // Random direction * speed (avoids NaN from normalize(ZERO)).
                    let dir = writer.rand(VectorType::VEC3F) * writer.lit(2.0) - writer.lit(1.0);
                    let speed = writer.lit(self.speed_range.0).uniform(writer.lit(self.speed_range.1));
                    let vel = dir.normalized() * speed;
                    VelInit::Attr(SetAttributeModifier::new(Attribute::VELOCITY, vel.expr()))
                } else {
                    // Radial outward from emission shape center.
                    VelInit::Sphere(SetVelocitySphereModifier {
                        center: writer.lit(Vec3::ZERO).expr(),
                        speed: speed_expr,
                    })
                }
            }
        };

        // ── Color gradient ─────────────────────────────────────
        let mut color_gradient: HanabiGradient<Vec4> = HanabiGradient::new();
        for stop in &self.color_gradient() {
            color_gradient.add_key(
                stop.t,
                Vec4::new(stop.color[0], stop.color[1], stop.color[2], stop.color[3]),
            );
        }

        // ── Size gradient ──────────────────────────────────────
        let mut size_gradient: HanabiGradient<Vec3> = HanabiGradient::new();
        for stop in &self.size_gradient() {
            size_gradient.add_key(stop.t, Vec3::splat(stop.size));
        }

        // ── Spawner ────────────────────────────────────────────
        let spawner = SpawnerSettings::rate(rate.into());

        // ── Build effect ───────────────────────────────────────
        let gravity_expr = writer.lit(Vec3::new(0.0, 0.0, -self.gravity)).expr();
        let drag_expr = writer.lit(self.drag).expr();

        // Kill plane: huge AABB above ground_z. Particles that fall below terrain die.
        let kill_center = writer.lit(Vec3::new(0.0, 0.0, ground_z + 5000.0)).expr();
        let kill_half = writer.lit(Vec3::new(50000.0, 50000.0, 5000.0)).expr();

        let module = writer.finish();

        let mut effect = EffectAsset::new(max_particles, spawner, module)
            .with_name(&self.id)
            .with_simulation_space(SimulationSpace::Global);

        // Position init (must come before velocity).
        effect = match pos_init {
            PosInit::Attr(m) => effect.init(m),
            PosInit::Sphere(m) => effect.init(m),
            PosInit::Circle(m) => effect.init(m),
        };

        effect = effect
            .init(init_lifetime)
            .init(init_age);

        // Velocity init (after position).
        effect = match vel_init {
            VelInit::Attr(m) => effect.init(m),
            VelInit::Sphere(m) => effect.init(m),
        };

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

        // Kill particles that fall below the terrain surface.
        effect = effect.update(KillAabbModifier {
            center: kill_center,
            half_size: kill_half,
            kill_inside: false, // kill particles OUTSIDE the box (below ground)
        });

        // Render modifiers.
        effect = effect
            .render(OrientModifier::new(OrientMode::ParallelCameraDepthPlane))
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
                    Ok(mut def) => {
                        def.migrate_legacy_fields();
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
