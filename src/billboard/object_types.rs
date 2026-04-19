//! Data types for object properties (lights, emitters, metadata).
//! These are always available — the editor UI that uses them is behind `dev_tools`.

use bevy::prelude::*;

// ── Serde defaults ──────────────────────────────────────────────────────────

pub(crate) fn default_half() -> f32 { 0.5 }
pub(crate) fn default_color() -> [f32; 3] { [1.0, 0.85, 0.6] }
pub(crate) fn default_intensity() -> f32 { 1.5 }
pub(crate) fn default_radius() -> f32 { 100.0 }
pub(crate) fn default_type() -> String { "organic".to_string() }
pub(crate) fn default_shape() -> String { "point".to_string() }
pub(crate) fn default_rate() -> f32 { 10.0 }
pub(crate) fn default_true() -> bool { true }

// ── Serializable property structs ───────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
pub struct ObjectProperties {
    #[serde(default)]
    pub lights: Vec<ObjectLight>,
    #[serde(default)]
    pub emitters: Vec<ObjectEmitter>,
    #[serde(default)]
    pub blend_height: f32,
    #[serde(default = "default_half")]
    pub shadow_offset_x: f32,
    #[serde(default = "default_half")]
    pub shadow_offset_y: f32,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default = "default_type")]
    pub obj_type: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct ObjectEmitter {
    #[serde(default = "default_half")]
    pub offset_x: f32,
    #[serde(default = "default_half")]
    pub offset_y: f32,
    #[serde(default)]
    pub definition_id: String,
    #[serde(default = "default_rate")]
    pub rate: f32,
    #[serde(default = "default_true")]
    pub active: bool,
}

impl Default for ObjectEmitter {
    fn default() -> Self {
        Self {
            offset_x: default_half(),
            offset_y: default_half(),
            definition_id: String::new(),
            rate: default_rate(),
            active: true,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct ObjectLight {
    #[serde(default = "default_half")]
    pub offset_x: f32,
    #[serde(default = "default_half")]
    pub offset_y: f32,
    #[serde(default = "default_color")]
    pub color: [f32; 3],
    #[serde(default = "default_intensity")]
    pub intensity: f32,
    #[serde(default = "default_radius")]
    pub radius: f32,
    #[serde(default = "default_shape")]
    pub shape: String,
    #[serde(default)]
    pub pulse: bool,
    #[serde(default)]
    pub flicker: bool,
}

impl Default for ObjectLight {
    fn default() -> Self {
        Self {
            offset_x: default_half(),
            offset_y: default_half(),
            color: default_color(),
            intensity: default_intensity(),
            radius: default_radius(),
            shape: default_shape(),
            pulse: false,
            flicker: false,
        }
    }
}

// ── ECS markers ─────────────────────────────────────────────────────────────

/// Marker for lights spawned from object properties.
/// Stores billboard-local offset so the light follows the billboard's tilt.
#[derive(Component)]
pub struct ObjectSpriteLight {
    pub sprite_key: String,
    pub offset_x: f32,
    pub offset_y: f32,
}

/// Marker for emitters spawned from object properties.
#[derive(Component)]
pub struct ObjectSpriteEmitter {
    pub sprite_key: String,
    pub offset_x: f32,
    pub offset_y: f32,
}
