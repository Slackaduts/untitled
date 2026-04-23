//! Data types for object properties (lights, emitters, metadata).
//! These are always available — the editor UI that uses them is behind `dev_tools`.

use bevy::prelude::*;

// ── Serde defaults ──────────────────────────────────────────────────────────

pub(crate) fn default_half() -> f32 { 0.5 }
pub(crate) fn default_color() -> [f32; 3] { [1.0, 0.85, 0.6] }
pub(crate) fn default_intensity() -> f32 { 1.5 }
pub(crate) fn default_radius() -> f32 { 100.0 }
pub(crate) fn default_shape() -> String { "point".to_string() }
pub(crate) fn default_rate() -> f32 { 10.0 }
pub(crate) fn default_true() -> bool { true }
pub(crate) fn default_depth() -> f32 { 0.55 }

// ── Sprite / movement classification ───────────────────────────────────────

/// How the sprite sheet is laid out.
#[derive(serde::Serialize, serde::Deserialize, Clone, Default, PartialEq)]
pub enum SpriteType {
    /// Single static image (tile-composed objects, decorations).
    #[default]
    Static,
    /// LPC Universal Spritesheet (64x64 frames, 13 columns, standard row layout).
    Lpc,
    /// Custom spritesheet with user-defined frame dimensions.
    Custom { frame_w: u32, frame_h: u32, columns: u32 },
}

/// How the entity moves in the overworld.
#[derive(serde::Serialize, serde::Deserialize, Clone, Default, PartialEq)]
pub enum MovementMode {
    /// No movement — static decoration or furniture.
    #[default]
    None,
    /// Moves tile-by-tile (grid-aligned steps).
    GridSnap,
    /// Smooth pixel-level movement (free roaming).
    FreeMove,
}

// ── Serializable property structs ───────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
pub struct ObjectProperties {
    #[serde(default)]
    pub sprite_type: SpriteType,
    #[serde(default)]
    pub movement_mode: MovementMode,
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
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct ObjectEmitter {
    /// Stable reference ID for Lua scripting (e.g. "emitter_0").
    #[serde(default)]
    pub ref_id: String,
    #[serde(default = "default_half")]
    pub offset_x: f32,
    #[serde(default = "default_half")]
    pub offset_y: f32,
    /// Depth offset in front of billboard face (0.0 = on face, 1.0 = one billboard-height forward).
    #[serde(default = "default_depth")]
    pub offset_z: f32,
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
            ref_id: String::new(),
            offset_x: default_half(),
            offset_y: default_half(),
            offset_z: default_depth(),
            definition_id: String::new(),
            rate: default_rate(),
            active: true,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct ObjectLight {
    /// Stable reference ID for Lua scripting (e.g. "light_0").
    #[serde(default)]
    pub ref_id: String,
    #[serde(default = "default_half")]
    pub offset_x: f32,
    #[serde(default = "default_half")]
    pub offset_y: f32,
    /// Depth offset in front of billboard face (0.0 = on face, 1.0 = one billboard-height forward).
    #[serde(default = "default_depth")]
    pub offset_z: f32,
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
            ref_id: String::new(),
            offset_x: default_half(),
            offset_y: default_half(),
            offset_z: default_depth(),
            color: default_color(),
            intensity: default_intensity(),
            radius: default_radius(),
            shape: default_shape(),
            pulse: false,
            flicker: false,
        }
    }
}

impl ObjectProperties {
    /// Assign ref_ids to any lights/emitters that don't have one yet.
    pub fn ensure_ref_ids(&mut self) {
        for (i, light) in self.lights.iter_mut().enumerate() {
            if light.ref_id.is_empty() {
                light.ref_id = format!("light_{i}");
            }
        }
        for (i, emitter) in self.emitters.iter_mut().enumerate() {
            if emitter.ref_id.is_empty() {
                emitter.ref_id = format!("emitter_{i}");
            }
        }
    }
}

// ── ECS markers ─────────────────────────────────────────────────────────────

/// Marker for lights spawned from object properties.
/// Stores billboard-local offset so the light follows the billboard's tilt.
#[derive(Component)]
pub struct ObjectSpriteLight {
    pub sprite_key: String,
    pub ref_id: String,
    pub offset_x: f32,
    pub offset_y: f32,
    pub offset_z: f32,
    /// Sprite pixel width (needed for correct X scaling; height comes from BillboardHeight).
    pub sprite_width: f32,
}

/// Marker for emitters spawned from object properties.
#[derive(Component)]
pub struct ObjectSpriteEmitter {
    pub sprite_key: String,
    pub ref_id: String,
    pub offset_x: f32,
    pub offset_y: f32,
    pub offset_z: f32,
    /// Sprite pixel width (needed for correct X scaling; height comes from BillboardHeight).
    pub sprite_width: f32,
}
