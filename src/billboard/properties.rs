use bevy::prelude::*;
use std::collections::HashMap;

/// Per-tile billboard property overrides.
/// Stored in TSX files as `<tile type="TileBillboard">` custom properties.
#[derive(Clone)]
pub struct TileBillboardEdit {
    /// Origin X within the tile (0-1 fraction). 0.5 = center.
    pub origin_x: f32,
    /// Origin Y within the tile (0-1 fraction). 1.0 = bottom, 0.0 = top.
    pub origin_y: f32,
    /// Pixels from the bottom of the sprite that fade to transparent.
    pub blend_height: f32,
    /// Tilt angle override in degrees. -1 = use global BILLBOARD_TILT_DEG.
    pub tilt_override: f32,
    /// Additional Z offset in world units.
    pub z_offset: f32,
    /// Collider depth (Z extent) in world units.
    pub collider_depth: f32,
    /// Collider width override. -1 = use Tiled's collision shape.
    pub collider_w: f32,
    /// Collider height override. -1 = use Tiled's collision shape.
    pub collider_h: f32,
}

impl Default for TileBillboardEdit {
    fn default() -> Self {
        Self {
            origin_x: 0.5,
            origin_y: 1.0,
            blend_height: 0.0,
            tilt_override: -1.0,
            z_offset: 0.0,
            collider_depth: 48.0,
            collider_w: -1.0,
            collider_h: -1.0,
        }
    }
}

impl TileBillboardEdit {
    /// Returns true if all values are at their defaults (no edits made).
    pub fn is_default(&self) -> bool {
        let d = Self::default();
        (self.origin_x - d.origin_x).abs() < 0.001
            && (self.origin_y - d.origin_y).abs() < 0.001
            && self.blend_height < 0.001
            && self.tilt_override < 0.0
            && self.z_offset.abs() < 0.001
            && (self.collider_depth - d.collider_depth).abs() < 0.001
            && self.collider_w < 0.0
            && self.collider_h < 0.0
    }
}

/// Per-tileset, per-tile billboard property definitions.
/// Keyed by (tileset filename, tile_id).
#[derive(Resource, Default)]
pub struct BillboardPropertyDefs {
    pub by_tileset: HashMap<String, HashMap<u32, TileBillboardEdit>>,
}

impl BillboardPropertyDefs {
    /// Look up billboard properties for a specific tile.
    pub fn get(&self, tileset_name: &str, tile_id: u32) -> Option<&TileBillboardEdit> {
        self.by_tileset.get(tileset_name)?.get(&tile_id)
    }
}

/// Component attached to billboard quads that have custom properties.
#[derive(Component)]
pub struct BillboardProperties {
    pub origin: Vec2,
    pub blend_height: f32,
    pub tilt_override: f32,
    pub z_offset: f32,
    pub collider_depth: f32,
}

// ── Runtime loading system ──────────────────────────────────────────────────

/// Scans all TSX files in assets/tilesets/ and loads billboard properties.
/// Runs once at startup.
pub fn load_billboard_properties(
    mut defs: ResMut<BillboardPropertyDefs>,
    mut loaded: Local<bool>,
) {
    if *loaded { return; }
    *loaded = true;

    let Ok(entries) = std::fs::read_dir("assets/tilesets") else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "tsx") {
            let Ok(content) = std::fs::read_to_string(&path) else { continue };
            let edits = load_existing_edits_from_tsx(&content);
            if !edits.is_empty() {
                let tsx_name = path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                info!("Loaded {} billboard properties from {}", edits.len(), tsx_name);
                defs.by_tileset.insert(tsx_name, edits);
            }
        }
    }
}

/// Apply collider depth overrides from billboard properties to billboard
/// entities that have both a BillboardProperties and a Collider component.
pub fn apply_collider_depth_overrides(
    mut commands: Commands,
    query: Query<(Entity, &BillboardProperties), (With<crate::camera::combat::BillboardTileQuad>, Added<BillboardProperties>)>,
) {
    for (entity, props) in &query {
        if (props.collider_depth - 48.0).abs() > 0.01 {
            // The collider depth differs from default — this will be used
            // when the collider system processes this entity's tile.
            info!("Billboard entity {:?} has custom collider depth: {}", entity, props.collider_depth);
        }
    }
}

// ── TSX parsing ────────────────────────────────────────────────────────────

/// Parse billboard properties from a TSX file's XML content.
pub fn load_existing_edits_from_tsx(content: &str) -> HashMap<u32, TileBillboardEdit> {
    let mut edits = HashMap::new();

    // Find all <tile> elements with type="TileBillboard"
    let mut search = content;
    while let Some(tile_start) = search.find("<tile ") {
        search = &search[tile_start..];
        let Some(tile_end) = search.find('>') else { break };

        let tag = &search[..tile_end + 1];

        // Check if this tile has type="TileBillboard"
        if !tag.contains("type=\"TileBillboard\"") {
            search = &search[1..];
            continue;
        }

        // Extract tile id
        let Some(id) = extract_attr_from_tag(tag, "id")
            .and_then(|s| s.parse::<u32>().ok()) else {
            search = &search[1..];
            continue;
        };

        // Find the closing </tile> and extract properties
        let tile_region = if let Some(close) = search.find("</tile>") {
            &search[..close]
        } else {
            search = &search[1..];
            continue;
        };

        let mut edit = TileBillboardEdit::default();

        // Parse each property
        let mut prop_search = tile_region;
        while let Some(prop_start) = prop_search.find("<property ") {
            prop_search = &prop_search[prop_start..];
            let Some(prop_end) = prop_search.find("/>") else { break };
            let prop_tag = &prop_search[..prop_end + 2];

            if let (Some(name), Some(value)) = (
                extract_attr_from_tag(prop_tag, "name"),
                extract_attr_from_tag(prop_tag, "value"),
            ) {
                let v: f32 = value.parse().unwrap_or(0.0);
                match name.as_str() {
                    "origin_x" => edit.origin_x = v,
                    "origin_y" => edit.origin_y = v,
                    "blend_height" => edit.blend_height = v,
                    "tilt_override" => edit.tilt_override = v,
                    "z_offset" => edit.z_offset = v,
                    "collider_depth" => edit.collider_depth = v,
                    "collider_w" => edit.collider_w = v,
                    "collider_h" => edit.collider_h = v,
                    _ => {}
                }
            }

            prop_search = &prop_search[prop_end + 2..];
        }

        edits.insert(id, edit);

        // Move past this tile
        if let Some(close) = search.find("</tile>") {
            search = &search[close + 7..];
        } else {
            break;
        }
    }

    edits
}

/// Format a TileBillboardEdit as XML for insertion into a TSX file.
pub fn format_tile_element(tile_id: u32, edit: &TileBillboardEdit) -> String {
    let mut props = Vec::new();
    let def = TileBillboardEdit::default();

    // Only write non-default values
    if (edit.origin_x - def.origin_x).abs() > 0.001 {
        props.push(format!("   <property name=\"origin_x\" type=\"float\" value=\"{:.3}\"/>", edit.origin_x));
    }
    if (edit.origin_y - def.origin_y).abs() > 0.001 {
        props.push(format!("   <property name=\"origin_y\" type=\"float\" value=\"{:.3}\"/>", edit.origin_y));
    }
    if edit.blend_height > 0.001 {
        props.push(format!("   <property name=\"blend_height\" type=\"float\" value=\"{:.1}\"/>", edit.blend_height));
    }
    if edit.tilt_override >= 0.0 {
        props.push(format!("   <property name=\"tilt_override\" type=\"float\" value=\"{:.1}\"/>", edit.tilt_override));
    }
    if edit.z_offset.abs() > 0.001 {
        props.push(format!("   <property name=\"z_offset\" type=\"float\" value=\"{:.1}\"/>", edit.z_offset));
    }
    if (edit.collider_depth - def.collider_depth).abs() > 0.001 {
        props.push(format!("   <property name=\"collider_depth\" type=\"float\" value=\"{:.1}\"/>", edit.collider_depth));
    }
    if edit.collider_w >= 0.0 {
        props.push(format!("   <property name=\"collider_w\" type=\"float\" value=\"{:.1}\"/>", edit.collider_w));
    }
    if edit.collider_h >= 0.0 {
        props.push(format!("   <property name=\"collider_h\" type=\"float\" value=\"{:.1}\"/>", edit.collider_h));
    }

    if props.is_empty() {
        format!(" <tile id=\"{tile_id}\" type=\"TileBillboard\"/>\n")
    } else {
        format!(
            " <tile id=\"{tile_id}\" type=\"TileBillboard\">\n  <properties>\n{}\n  </properties>\n </tile>\n",
            props.join("\n")
        )
    }
}

fn extract_attr_from_tag(tag: &str, attr: &str) -> Option<String> {
    let pattern = format!("{attr}=\"");
    let start = tag.find(&pattern)? + pattern.len();
    let end = tag[start..].find('"')? + start;
    Some(tag[start..end].to_string())
}

/// Save billboard edits to a TSX file, preserving existing non-billboard elements.
pub fn save_tsx_changes(
    tsx_path: &str,
    edits: &HashMap<u32, TileBillboardEdit>,
) -> Result<(), String> {
    let content = std::fs::read_to_string(tsx_path)
        .map_err(|e| format!("Failed to read {tsx_path}: {e}"))?;

    // Remove existing TileBillboard tile elements
    let mut cleaned = String::new();
    let mut remaining = content.as_str();
    while let Some(tile_start) = remaining.find("<tile ") {
        let before = &remaining[..tile_start];
        cleaned.push_str(before);
        remaining = &remaining[tile_start..];

        // Check if this is a TileBillboard tile
        let tag_end = remaining.find('>').unwrap_or(remaining.len());
        let tag = &remaining[..tag_end + 1];

        if tag.contains("type=\"TileBillboard\"") {
            // Skip this tile element entirely
            if tag.contains("/>") {
                remaining = &remaining[tag_end + 1..];
            } else if let Some(close) = remaining.find("</tile>") {
                remaining = &remaining[close + 7..];
                // Skip trailing newline
                if remaining.starts_with('\n') {
                    remaining = &remaining[1..];
                }
            }
        } else {
            // Keep this tile element
            cleaned.push_str(&remaining[..1]);
            remaining = &remaining[1..];
        }
    }
    cleaned.push_str(remaining);

    // Insert new billboard tile elements before </tileset>
    let insert_pos = cleaned.rfind("</tileset>")
        .ok_or("No </tileset> found in TSX")?;

    let mut new_elements = String::new();
    let mut sorted_ids: Vec<u32> = edits.keys().copied().collect();
    sorted_ids.sort();
    for id in sorted_ids {
        new_elements.push_str(&format_tile_element(id, &edits[&id]));
    }

    cleaned.insert_str(insert_pos, &new_elements);

    std::fs::write(tsx_path, &cleaned)
        .map_err(|e| format!("Failed to write {tsx_path}: {e}"))?;

    Ok(())
}
