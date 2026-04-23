//! Editor state resource and core types for the unified tile editor.

use bevy::prelude::*;

use super::sidecar::PlacedObjectDef;

// ── Editor mode ────────────────────────────────────────────────────────────

#[derive(Default, PartialEq, Eq, Clone, Copy)]
pub enum EditorMode {
    /// Browsing tilesets and selecting tiles.
    #[default]
    Browse,
    /// Editing properties (lights, emitters, collision) of the composed object.
    Properties,
    /// Grid-snapped placement mode.
    Place,
    /// Door/portal placement and configuration.
    Door,
    /// Browse the library of all composed objects in assets/objects/.
    Library,
}

// ── Main state resource ────────────────────────────────────────────────────

#[derive(Resource)]
pub struct TileEditorState {
    pub open: bool,
    pub mode: EditorMode,

    // ── Tileset browser ──
    pub tilesets: Vec<TilesetInfo>,
    pub selected_tileset: Option<usize>,
    pub selected_tiles: Vec<SelectedTile>,
    pub tileset_search: String,
    pub tilesets_scanned: bool,

    // ── Assembly grid ──
    /// Tiles arranged in a grid for composition. Each entry is (col, row, tile_id).
    /// Row 0 = top of the sprite (visual top). Users drag to rearrange.
    pub assembly: Vec<AssemblySlot>,
    /// Number of columns in the assembly grid.
    pub assembly_cols: u32,

    // ── Composition ──
    pub current_object: Option<ComposedObject>,

    // ── World selection ──
    pub selected_placed: Option<Entity>,
    /// Sidecar ID of the currently selected placed object (set alongside selected_placed).
    pub selected_sidecar_id: Option<String>,
    /// Sidecar ID to select on next frame (set by UI list, resolved by ECS system).
    pub pending_select_sidecar_id: Option<String>,
    /// Sidecar ID to delete on next frame (set by UI list, resolved by ECS system).
    pub pending_delete_sidecar_id: Option<String>,
    /// Search filter for placed objects list.
    pub placed_search: String,
    /// Index into `placed_objects` of the object currently being edited in Properties mode.
    /// When set, Properties Save writes back to this sidecar entry instead of properties.json.
    pub editing_placed_idx: Option<usize>,

    // ── Placement ──
    pub placement_ghost: Option<Entity>,
    pub placed_objects: Vec<PlacedObjectDef>,

    // ── Collision drawing ──
    pub collision_drawing: bool,
    pub collision_draw_start: Option<[f32; 2]>,

    // ── Pending compose ──
    /// Image data waiting to be registered with egui (set in browse, consumed in editor_ui).
    pub pending_compose_image: Option<Image>,
    /// Sidecar ID whose lights/emitters need respawning (set on save, consumed by system).
    pub pending_respawn_sidecar_id: Option<String>,
    /// Sprite key whose ALL placed instances need property refresh + respawn
    /// (set when saving root object from Library mode, consumed by system).
    pub pending_respawn_sprite_key: Option<String>,

    // ── Object Library ──
    #[cfg(feature = "dev_tools")]
    pub library_objects: Vec<LibraryEntry>,
    #[cfg(feature = "dev_tools")]
    pub library_search: String,
    #[cfg(feature = "dev_tools")]
    pub library_selected: Option<usize>,
    #[cfg(feature = "dev_tools")]
    pub library_scanned: bool,
    /// Import sprite UI state.
    #[cfg(feature = "dev_tools")]
    pub import_sprite_path: String,
    #[cfg(feature = "dev_tools")]
    pub import_sprite_name: String,
    #[cfg(feature = "dev_tools")]
    pub import_sprite_open: bool,

    // ── Persistence ──
    pub dirty: bool,
    pub sidecar_path: Option<String>,
}

impl Default for TileEditorState {
    fn default() -> Self {
        Self {
            open: false,
            mode: EditorMode::Browse,
            tilesets: Vec::new(),
            selected_tileset: None,
            selected_tiles: Vec::new(),
            tileset_search: String::new(),
            tilesets_scanned: false,
            assembly: Vec::new(),
            assembly_cols: 1,
            current_object: None,
            selected_placed: None,
            selected_sidecar_id: None,
            pending_select_sidecar_id: None,
            pending_delete_sidecar_id: None,
            placed_search: String::new(),
            editing_placed_idx: None,
            placement_ghost: None,
            placed_objects: Vec::new(),
            collision_drawing: false,
            collision_draw_start: None,
            pending_compose_image: None,
            pending_respawn_sidecar_id: None,
            pending_respawn_sprite_key: None,
            #[cfg(feature = "dev_tools")]
            library_objects: Vec::new(),
            #[cfg(feature = "dev_tools")]
            library_search: String::new(),
            #[cfg(feature = "dev_tools")]
            library_selected: None,
            #[cfg(feature = "dev_tools")]
            library_scanned: false,
            #[cfg(feature = "dev_tools")]
            import_sprite_path: String::new(),
            #[cfg(feature = "dev_tools")]
            import_sprite_name: String::new(),
            #[cfg(feature = "dev_tools")]
            import_sprite_open: false,
            dirty: false,
            sidecar_path: None,
        }
    }
}

// ── Tileset info ───────────────────────────────────────────────────────────

pub struct TilesetInfo {
    pub name: String,
    pub tsx_path: std::path::PathBuf,
    /// Image path relative to `assets/` (e.g. `"tilesets/TileB_exterior1.png"`).
    pub image_path: String,
    pub tile_width: u32,
    pub tile_height: u32,
    pub columns: u32,
    pub tile_count: u32,
    pub atlas_handle: Option<Handle<Image>>,
    #[cfg(feature = "dev_tools")]
    pub egui_texture: Option<bevy_egui::egui::TextureId>,
}

// ── Assembly slot ──────────────────────────────────────────────────────────

/// A tile placed in the assembly grid at a specific position.
#[derive(Clone, Debug)]
pub struct AssemblySlot {
    /// Column in the assembly grid (0 = left).
    pub col: u32,
    /// Row in the assembly grid (0 = top visually, which becomes bottom of sprite in Bevy Y-up).
    pub row: u32,
    /// Tileset index in `TileEditorState::tilesets`.
    pub tileset_idx: usize,
    /// Tile ID within that tileset (ignored when `blank` is true).
    pub tile_id: u32,
    /// When true, this slot is fully transparent (no tile data copied).
    pub blank: bool,
}

// ── Selected tile ──────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectedTile {
    pub tileset_idx: usize,
    pub tile_id: u32,
}

// ── Library entry ─────────────────────────────────────────────────────────

#[cfg(feature = "dev_tools")]
pub struct LibraryEntry {
    pub tileset: String,
    pub key: String,
    pub dir: std::path::PathBuf,
    pub properties: crate::billboard::object_types::ObjectProperties,
    pub sprite_texture: Option<bevy_egui::egui::TextureId>,
    pub sprite_handle: Option<Handle<Image>>,
    /// Sprite image dimensions in pixels (sheet_width, sheet_height).
    pub image_size: Option<(u32, u32)>,
}

// ── Composed object ────────────────────────────────────────────────────────

pub struct ComposedObject {
    pub sprite_key: String,
    pub tileset_name: String,
    pub tile_ids: Vec<u32>,
    pub width_px: u32,
    pub height_px: u32,
    pub image_handle: Handle<Image>,
    #[cfg(feature = "dev_tools")]
    pub egui_texture: Option<bevy_egui::egui::TextureId>,
    pub properties: crate::billboard::object_types::ObjectProperties,
    pub collision_rects: Vec<super::sidecar::CollisionRect>,
}

// ── Component markers ──────────────────────────────────────────────────────

/// Marks billboard entities spawned from the sidecar file.
/// Stores the sidecar object ID for cross-referencing.
#[derive(Component)]
pub struct PlacedObject {
    pub sidecar_id: String,
    pub name: Option<String>,
}

/// Marks light/emitter entities that belong to a specific sidecar-placed object.
/// Used to find and despawn/respawn them when properties are edited.
#[derive(Component)]
pub struct SidecarChild {
    pub sidecar_id: String,
    pub ref_id: String,
}
