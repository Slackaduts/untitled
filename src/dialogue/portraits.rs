//! Portrait sheet loading and expression extraction.
//!
//! Portrait sheets are 384x192 QOI images arranged as a 4x2 grid of 96x96
//! expression cells (RPG Maker faceset style).
//!
//! Expression indices:
//!   0=default, 1=happy, 2=sad, 3=angry,
//!   4=surprised, 5=thinking, 6=hurt, 7=special

use bevy::prelude::*;

/// Cell size in pixels for each expression in the portrait grid.
pub const PORTRAIT_CELL_SIZE: u32 = 96;
/// Number of columns in the portrait sheet grid.
pub const PORTRAIT_COLS: u32 = 4;
/// Number of rows in the portrait sheet grid.
pub const PORTRAIT_ROWS: u32 = 2;

/// Named expression variants. The index matches the grid position (row-major).
pub const EXPRESSION_NAMES: &[&str] = &[
    "default", "happy", "sad", "angry",
    "surprised", "thinking", "hurt", "special",
];

/// Resolve an expression name or numeric string to a grid index.
pub fn expression_index(expr: &str) -> usize {
    // Try numeric first
    if let Ok(idx) = expr.parse::<usize>() {
        return idx.min((PORTRAIT_COLS * PORTRAIT_ROWS - 1) as usize);
    }
    // Then name lookup
    EXPRESSION_NAMES
        .iter()
        .position(|&name| name.eq_ignore_ascii_case(expr))
        .unwrap_or(0)
}

/// Cached portrait atlas data for a single character.
pub struct PortraitAtlas {
    pub texture: Handle<Image>,
    pub layout: Handle<TextureAtlasLayout>,
}

/// Resource caching loaded portrait atlases keyed by character name.
#[derive(Resource, Default)]
pub struct PortraitCache {
    pub atlases: std::collections::HashMap<String, PortraitAtlas>,
}

impl PortraitCache {
    /// Get or load a portrait atlas for the given character name.
    /// Returns the atlas handle and layout, loading from
    /// `assets/portraits/<name>.qoi` if not cached.
    pub fn get_or_load(
        &mut self,
        name: &str,
        asset_server: &AssetServer,
        layouts: &mut Assets<TextureAtlasLayout>,
    ) -> Option<&PortraitAtlas> {
        if !self.atlases.contains_key(name) {
            let path = format!("portraits/{name}.qoi");
            let texture: Handle<Image> = asset_server.load(&path);

            let layout = TextureAtlasLayout::from_grid(
                UVec2::new(PORTRAIT_CELL_SIZE, PORTRAIT_CELL_SIZE),
                PORTRAIT_COLS,
                PORTRAIT_ROWS,
                None,
                None,
            );
            let layout_handle = layouts.add(layout);

            self.atlases.insert(
                name.to_string(),
                PortraitAtlas {
                    texture,
                    layout: layout_handle,
                },
            );
        }
        self.atlases.get(name)
    }
}
