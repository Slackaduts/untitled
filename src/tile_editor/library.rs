//! Object library browser for the F6 tile editor.
//!
//! Scans `assets/objects/` for all composed objects and presents them in a
//! searchable thumbnail grid. Selecting an object loads it into Properties
//! mode for editing.

use bevy::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy_egui::egui;
use std::path::PathBuf;

use super::state::{TileEditorState, LibraryEntry, ComposedObject, EditorMode};
use crate::billboard::object_types::{ObjectProperties, SpriteType};

/// Scan `assets/objects/` for all composed objects.
pub fn scan_library_objects(state: &mut TileEditorState) {
    state.library_scanned = true;
    state.library_objects.clear();

    let base = PathBuf::from("assets/objects");
    let Ok(tilesets) = std::fs::read_dir(&base) else {
        return;
    };

    for ts_entry in tilesets.flatten() {
        let ts_path = ts_entry.path();
        if !ts_path.is_dir() {
            continue;
        }
        let tileset = ts_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let Ok(keys) = std::fs::read_dir(&ts_path) else {
            continue;
        };
        for key_entry in keys.flatten() {
            let key_path = key_entry.path();
            if !key_path.is_dir() {
                continue;
            }

            // Only consider folders that contain a sprite
            let has_qoi = key_path.join("sprite.qoi").exists();
            let has_png = key_path.join("sprite.png").exists();
            if !has_qoi && !has_png {
                continue;
            }

            let key = key_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            // Load properties.json if it exists
            let props_path = key_path.join("properties.json");
            let mut properties: ObjectProperties = if props_path.exists() {
                std::fs::read_to_string(&props_path)
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default()
            } else {
                ObjectProperties::default()
            };
            properties.ensure_ref_ids();

            // Read image dimensions for animated thumbnail UVs
            let sprite_file = if key_path.join("sprite.qoi").exists() {
                key_path.join("sprite.qoi")
            } else {
                key_path.join("sprite.png")
            };
            let image_size = image::image_dimensions(&sprite_file)
                .ok()
                .map(|(w, h)| (w, h));

            state.library_objects.push(LibraryEntry {
                tileset: tileset.clone(),
                key,
                dir: key_path,
                properties,
                sprite_texture: None,
                sprite_handle: None,
                image_size,
            });
        }
    }

    state.library_objects.sort_by(|a, b| {
        a.tileset.cmp(&b.tileset).then(a.key.cmp(&b.key))
    });

    info!("Library: scanned {} objects", state.library_objects.len());
}

/// UI for the Library mode tab.
pub fn library_mode_ui(
    ui: &mut egui::Ui,
    state: &mut TileEditorState,
    _images: &Assets<Image>,
    _current_map: &crate::map::loader::CurrentMap,
    _particle_def_ids: &[String],
) {
    ui.heading("Object Library");

    // Scan on first open
    if !state.library_scanned {
        scan_library_objects(state);
    }

    // Search bar
    ui.horizontal(|ui| {
        ui.label("Search:");
        ui.text_edit_singleline(&mut state.library_search);
        if ui.small_button("x").clicked() {
            state.library_search.clear();
        }
        if ui.button("Rescan").clicked() {
            state.library_scanned = false;
            scan_library_objects(state);
        }
        if ui.button("Import Sprite").clicked() {
            state.import_sprite_open = !state.import_sprite_open;
        }
    });

    // ── Import sprite panel ──
    if state.import_sprite_open {
        ui.group(|ui| {
            ui.label("Import a spritesheet as a new object:");
            ui.horizontal(|ui| {
                ui.label("Path:");
                ui.text_edit_singleline(&mut state.import_sprite_path);
            });
            ui.horizontal(|ui| {
                ui.label("Name:");
                ui.text_edit_singleline(&mut state.import_sprite_name);
            });

            let path_ok = !state.import_sprite_path.is_empty()
                && !state.import_sprite_name.is_empty()
                && std::path::Path::new(&state.import_sprite_path).exists();

            ui.add_enabled_ui(path_ok, |ui| {
                if ui.button("Import as LPC").clicked() {
                    import_sprite(state, SpriteType::Lpc);
                }
                if ui.button("Import as Static").clicked() {
                    import_sprite(state, SpriteType::Static);
                }
            });

            if !state.import_sprite_path.is_empty()
                && !std::path::Path::new(&state.import_sprite_path).exists()
            {
                ui.colored_label(egui::Color32::RED, "File not found");
            }
        });
    }

    let search_lower = state.library_search.to_lowercase();

    let filtered: Vec<usize> = state
        .library_objects
        .iter()
        .enumerate()
        .filter(|(_, obj)| {
            if search_lower.is_empty() {
                return true;
            }
            obj.tileset.to_lowercase().contains(&search_lower)
                || obj.key.to_lowercase().contains(&search_lower)
                || obj.properties.keywords.iter().any(|k| {
                    k.to_lowercase().contains(&search_lower)
                })
        })
        .map(|(i, _)| i)
        .collect();

    ui.label(format!(
        "{} / {} objects",
        filtered.len(),
        state.library_objects.len()
    ));

    let thumb_size = 48.0;
    let grid_height = 200.0_f32
        .min(((filtered.len() as f32 / 8.0).ceil()) * (thumb_size + 4.0))
        .max(60.0);

    // Animated thumbnail: cycle walk frames using wall-clock time
    let time_secs = ui.input(|i| i.time) as f32;

    let mut edit_idx: Option<usize> = None;
    let mut needs_repaint = false;

    egui::ScrollArea::vertical()
        .max_height(grid_height)
        .id_salt("library_grid")
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for &obj_idx in &filtered {
                    let obj = &state.library_objects[obj_idx];
                    let is_selected = state.library_selected == Some(obj_idx);

                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(thumb_size, thumb_size),
                        egui::Sense::click(),
                    );

                    // Draw thumbnail — animated sprites show a walk cycle frame
                    if let Some(tex_id) = obj.sprite_texture {
                        let tint = if is_selected {
                            egui::Color32::WHITE
                        } else {
                            egui::Color32::from_gray(180)
                        };

                        let uv_rect = sprite_thumbnail_uv(&obj.properties.sprite_type, obj.image_size, time_secs);
                        if uv_rect.width() < 1.0 {
                            needs_repaint = true;
                        }
                        ui.painter().image(tex_id, rect, uv_rect, tint);
                    } else {
                        ui.painter().rect_filled(
                            rect,
                            2.0,
                            egui::Color32::from_gray(40),
                        );
                    }

                    // Selection border
                    if is_selected {
                        ui.painter().rect_stroke(
                            rect,
                            2.0,
                            egui::Stroke::new(2.0, egui::Color32::YELLOW),
                            egui::StrokeKind::Outside,
                        );
                    }

                    let resp = resp.on_hover_text(format!("{} / {}", obj.tileset, obj.key));
                    if resp.clicked() {
                        state.library_selected = Some(obj_idx);
                    }
                    if resp.double_clicked() {
                        edit_idx = Some(obj_idx);
                    }
                }
            });
        });

    // Request continuous repaints while animated thumbnails are visible
    if needs_repaint {
        ui.ctx().request_repaint();
    }

    ui.separator();

    // Selected object details
    if let Some(sel_idx) = state.library_selected {
        if let Some(obj) = state.library_objects.get(sel_idx) {
            ui.label(format!("{} / {}", obj.tileset, obj.key));
            let type_label = match &obj.properties.sprite_type {
                SpriteType::Static => "Static",
                SpriteType::Lpc => "LPC",
                SpriteType::Custom { .. } => "Custom",
            };
            ui.label(format!(
                "Type: {}  |  Lights: {}  Emitters: {}",
                type_label,
                obj.properties.lights.len(),
                obj.properties.emitters.len()
            ));
            if !obj.properties.keywords.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Keywords:");
                    for kw in &obj.properties.keywords {
                        ui.label(
                            egui::RichText::new(kw)
                                .background_color(egui::Color32::from_gray(60))
                                .color(egui::Color32::WHITE),
                        );
                    }
                });
            }
            if ui.button("Edit Properties").clicked() {
                edit_idx = Some(sel_idx);
            }
        }
    } else {
        ui.label("Select an object to view details. Double-click to edit.");
    }

    // Handle edit action
    if let Some(idx) = edit_idx {
        load_library_for_editing(state, idx);
    }
}

/// Compute the UV rect for a library thumbnail based on sprite type.
/// Animated sprites cycle through walk-down frames; static shows full texture.
fn sprite_thumbnail_uv(
    sprite_type: &SpriteType,
    image_size: Option<(u32, u32)>,
    time_secs: f32,
) -> egui::Rect {
    let (sheet_w, sheet_h) = image_size.unwrap_or((1, 1));
    match sprite_type {
        SpriteType::Lpc => {
            // LPC: 64x64 frames, walk-down = row 10, 9 frames
            let frame_w = 64.0_f32;
            let frame_h = 64.0_f32;
            let frame_count = 9u32;
            let fps = 8.0;
            let frame = ((time_secs * fps) as u32) % frame_count;
            let row = 10; // walk down

            let u_min = frame as f32 * frame_w / sheet_w as f32;
            let u_max = (frame as f32 + 1.0) * frame_w / sheet_w as f32;
            let v_min = row as f32 * frame_h / sheet_h as f32;
            let v_max = (row as f32 + 1.0) * frame_h / sheet_h as f32;
            egui::Rect::from_min_max(egui::pos2(u_min, v_min), egui::pos2(u_max, v_max))
        }
        SpriteType::Custom { frame_w, frame_h, columns } => {
            let fw = *frame_w as f32;
            let fh = *frame_h as f32;
            let frame_count = *columns;
            let fps = 8.0;
            let frame = ((time_secs * fps) as u32) % frame_count;

            let u_min = frame as f32 * fw / sheet_w as f32;
            let u_max = (frame as f32 + 1.0) * fw / sheet_w as f32;
            let v_min = 0.0;
            let v_max = fh / sheet_h as f32;
            egui::Rect::from_min_max(egui::pos2(u_min, v_min), egui::pos2(u_max, v_max))
        }
        SpriteType::Static => {
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0))
        }
    }
}

/// Import a spritesheet from an external path into the object library.
fn import_sprite(state: &mut TileEditorState, sprite_type: SpriteType) {
    let src = std::path::PathBuf::from(&state.import_sprite_path);
    let name = state.import_sprite_name.trim().to_string();
    if name.is_empty() || !src.exists() {
        return;
    }

    // Create object folder: assets/objects/<name>/<name>/
    let obj_dir = std::path::PathBuf::from("assets/objects")
        .join(&name)
        .join(&name);
    if let Err(e) = std::fs::create_dir_all(&obj_dir) {
        error!("Failed to create directory {}: {e}", obj_dir.display());
        return;
    }

    // Copy sprite file
    let dest = obj_dir.join("sprite.png");
    if let Err(e) = std::fs::copy(&src, &dest) {
        error!("Failed to copy sprite: {e}");
        return;
    }

    // Create properties.json
    let mut props = ObjectProperties {
        sprite_type,
        ..Default::default()
    };
    props.ensure_ref_ids();
    let props_path = obj_dir.join("properties.json");
    match serde_json::to_string_pretty(&props) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&props_path, &json) {
                error!("Failed to write properties: {e}");
                return;
            }
        }
        Err(e) => {
            error!("Failed to serialize properties: {e}");
            return;
        }
    }

    info!("Imported sprite '{}' from {}", name, src.display());

    // Clear import UI and rescan
    state.import_sprite_path.clear();
    state.import_sprite_name.clear();
    state.import_sprite_open = false;
    state.library_scanned = false;
    scan_library_objects(state);
}

/// Load a library object into the Properties editor.
fn load_library_for_editing(state: &mut TileEditorState, idx: usize) {
    let obj = &state.library_objects[idx];
    let ts_name = &obj.tileset;
    let sprite_key = &obj.key;

    // Find sprite file on disk
    let qoi_path = obj.dir.join("sprite.qoi");
    let png_path = obj.dir.join("sprite.png");
    let disk_path = if qoi_path.exists() {
        qoi_path
    } else if png_path.exists() {
        png_path
    } else {
        warn!("No sprite found for {sprite_key} — cannot edit");
        return;
    };

    // Decode image
    let Ok(dyn_img) = image::open(&disk_path) else {
        warn!("Failed to load sprite image: {}", disk_path.display());
        return;
    };
    let rgba = dyn_img.to_rgba8();
    let (w, h) = rgba.dimensions();

    let bevy_image = Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba.into_raw(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );

    let composed = ComposedObject {
        sprite_key: sprite_key.clone(),
        tileset_name: ts_name.clone(),
        tile_ids: Vec::new(),
        width_px: w,
        height_px: h,
        image_handle: Handle::default(),
        #[cfg(feature = "dev_tools")]
        egui_texture: None,
        properties: obj.properties.clone(),
        collision_rects: Vec::new(),
    };

    state.current_object = Some(composed);
    state.pending_compose_image = Some(bevy_image);
    state.editing_placed_idx = None; // Not editing a placed instance
    state.mode = EditorMode::Properties;

    info!("Loaded library object {sprite_key} for editing");
}
