//! Sidecar JSON format for editor-placed objects.
//!
//! Each map gets a companion `.objects.json` file alongside its `.tmx`:
//!   `assets/maps/test.tmx` → `assets/maps/test.objects.json`
//!
//! This file is the authoritative source for placed objects; the TMX stays
//! terrain-only.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::billboard::object_types::ObjectProperties;

// ── Top-level file ─────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct MapObjectsFile {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub objects: Vec<PlacedObjectDef>,
}

fn default_version() -> u32 {
    1
}

// ── Per-object definition ──────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
pub struct PlacedObjectDef {
    /// Unique ID within the file (incrementing integer as string).
    pub id: String,
    /// Optional user-editable name for Lua scripting references.
    #[serde(default)]
    pub name: Option<String>,
    /// Hash-based key matching the billboard sprite export convention.
    /// Format: `<tileset>_<hex_hash>` (same as BillboardSpriteKey).
    pub sprite_key: String,
    /// Tileset name this object was composed from.
    pub tileset: String,
    /// Tile indices within the tileset atlas used to build this object.
    pub tile_ids: Vec<u32>,
    /// Grid position in tile coordinates (origin = bottom-left of map).
    pub grid_pos: [i32; 2],
    /// Elevation level (matches terrain elevation system).
    #[serde(default)]
    pub elevation: u8,
    /// Object properties (lights, emitters, blend, type, etc.).
    #[serde(default)]
    pub properties: ObjectProperties,
    /// Collision rectangles in sprite-local pixel coordinates.
    #[serde(default)]
    pub collision_rects: Vec<CollisionRect>,
    /// Door/portal data if this object acts as a map transition.
    #[serde(default)]
    pub door: Option<DoorDef>,
}

// ── Collision ──────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CollisionRect {
    /// X offset from sprite left edge (pixels).
    pub x: f32,
    /// Y offset from sprite bottom edge (pixels).
    pub y: f32,
    /// Width in pixels.
    pub w: f32,
    /// Height in pixels.
    pub h: f32,
    /// Depth extending forward from the billboard face (world units).
    #[serde(default = "default_depth")]
    pub depth_fwd: f32,
    /// Depth extending behind the billboard face (world units).
    #[serde(default = "default_depth")]
    pub depth_back: f32,
}

fn default_depth() -> f32 {
    48.0
}

// ── Door/portal ────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DoorDef {
    /// Target map path relative to assets (e.g. "maps/interior.tmx").
    pub target_map: String,
    /// Spawn position in the target map (tile grid coordinates).
    pub spawn_point: [i32; 2],
    /// Optional Lua script path for custom transition effects.
    #[serde(default)]
    pub script: Option<String>,
}

// ── I/O ────────────────────────────────────────────────────────────────────

/// Derive the sidecar path from a TMX path.
/// `"assets/maps/test.tmx"` → `"assets/maps/test.objects.json"`
pub fn sidecar_path_for(tmx_path: &str) -> String {
    if let Some(base) = tmx_path.strip_suffix(".tmx") {
        format!("{base}.objects.json")
    } else {
        format!("{tmx_path}.objects.json")
    }
}

/// Load a sidecar file from disk. Returns `None` if the file doesn't exist.
pub fn load_sidecar(tmx_path: &str) -> Option<MapObjectsFile> {
    let path = sidecar_path_for(tmx_path);
    let content = std::fs::read_to_string(&path).ok()?;
    match serde_json::from_str::<MapObjectsFile>(&content) {
        Ok(mut file) => {
            // Backfill ref_ids for lights/emitters that don't have them
            for obj in &mut file.objects {
                obj.properties.ensure_ref_ids();
            }
            Some(file)
        }
        Err(e) => {
            error!("Failed to parse {path}: {e}");
            None
        }
    }
}

/// Save a sidecar file to disk.
pub fn save_sidecar(tmx_path: &str, file: &MapObjectsFile) -> Result<(), String> {
    let path = sidecar_path_for(tmx_path);
    let json = serde_json::to_string_pretty(file)
        .map_err(|e| format!("Serialize error: {e}"))?;
    std::fs::write(&path, &json)
        .map_err(|e| format!("Write error for {path}: {e}"))?;
    info!("Saved {} objects to {path}", file.objects.len());
    Ok(())
}

/// Generate the next unique ID for a new object in the file.
pub fn next_id(file: &MapObjectsFile) -> String {
    let max = file
        .objects
        .iter()
        .filter_map(|o| o.id.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    (max + 1).to_string()
}
