//! Tileset browser: scans .tsx files, displays tile atlases, handles selection.

use std::path::PathBuf;

use bevy::prelude::*;

use super::state::{SelectedTile, TileEditorState, TilesetInfo};

/// Scan all .tsx files in assets/tilesets/ and populate tileset info.
pub fn scan_tilesets(state: &mut TileEditorState) {
    if state.tilesets_scanned {
        return;
    }
    state.tilesets_scanned = true;

    let base = PathBuf::from("assets/tilesets");
    let Ok(entries) = std::fs::read_dir(&base) else {
        warn!("Cannot read assets/tilesets/");
        return;
    };

    let mut tilesets = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "tsx") {
            if let Some(info) = parse_tsx(&path) {
                tilesets.push(info);
            }
        }
    }

    tilesets.sort_by(|a, b| a.name.cmp(&b.name));
    info!("Tileset browser: scanned {} tilesets", tilesets.len());
    state.tilesets = tilesets;
}

/// Parse a .tsx file to extract tileset metadata.
fn parse_tsx(path: &PathBuf) -> Option<TilesetInfo> {
    let content = std::fs::read_to_string(path).ok()?;

    // Find <tileset ...> tag
    let ts_start = content.find("<tileset ")?;
    let ts_end = content[ts_start..].find('>')? + ts_start;
    let ts_tag = &content[ts_start..=ts_end];

    let name = extract_attr(ts_tag, "name")?;
    let tile_width: u32 = extract_attr(ts_tag, "tilewidth")?.parse().ok()?;
    let tile_height: u32 = extract_attr(ts_tag, "tileheight")?.parse().ok()?;
    let tile_count: u32 = extract_attr(ts_tag, "tilecount")?.parse().ok()?;
    let columns: u32 = extract_attr(ts_tag, "columns")?.parse().ok()?;

    // Find <image source="...">
    let img_start = content.find("<image ")?;
    let img_end = content[img_start..]
        .find("/>")
        .unwrap_or_else(|| content[img_start..].find('>').unwrap_or(0))
        + img_start;
    let img_tag = &content[img_start..=img_end];
    let source = extract_attr(img_tag, "source")?;

    // Image path relative to assets/
    let image_path = format!("tilesets/{source}");

    Some(TilesetInfo {
        name,
        tsx_path: path.clone(),
        image_path,
        tile_width,
        tile_height,
        columns,
        tile_count,
        atlas_handle: None,
        #[cfg(feature = "dev_tools")]
        egui_texture: None,
    })
}

fn extract_attr(tag: &str, attr: &str) -> Option<String> {
    let pattern = format!("{attr}=\"");
    let start = tag.find(&pattern)? + pattern.len();
    let end = tag[start..].find('"')? + start;
    Some(tag[start..end].to_string())
}

/// Render the tileset browser panel inside an egui UI region.
#[cfg(feature = "dev_tools")]
pub fn tileset_browser_ui(
    ui: &mut bevy_egui::egui::Ui,
    state: &mut TileEditorState,
    contexts: &mut bevy_egui::EguiContexts,
    asset_server: &AssetServer,
    keyboard: &ButtonInput<KeyCode>,
) {
    use bevy_egui::egui;

    // ── Tileset list ───────────────────────────────────────────────
    ui.horizontal(|ui| {
        ui.label("Tileset:");
        let selected_name = state
            .selected_tileset
            .and_then(|i| state.tilesets.get(i))
            .map(|ts| ts.name.clone())
            .unwrap_or_else(|| "Select...".to_string());

        egui::ComboBox::from_id_salt("tileset_combo")
            .width(200.0)
            .selected_text(&selected_name)
            .show_ui(ui, |ui| {
                // Search filter
                ui.text_edit_singleline(&mut state.tileset_search);
                let search = state.tileset_search.to_lowercase();

                for (idx, ts) in state.tilesets.iter().enumerate() {
                    if !search.is_empty() && !ts.name.to_lowercase().contains(&search) {
                        continue;
                    }
                    if ui
                        .selectable_label(state.selected_tileset == Some(idx), &ts.name)
                        .clicked()
                    {
                        state.selected_tileset = Some(idx);
                    }
                }
            });
    });

    // ── Load atlas texture if needed ───────────────────────────────
    let Some(ts_idx) = state.selected_tileset else {
        ui.label("Select a tileset to browse tiles.");
        return;
    };

    // Load atlas handle and register egui texture
    {
        let ts = &mut state.tilesets[ts_idx];
        if ts.atlas_handle.is_none() {
            let handle: Handle<Image> = asset_server.load(&ts.image_path);
            let tex_id = contexts
                .add_image(bevy_egui::EguiTextureHandle::Strong(handle.clone()));
            ts.atlas_handle = Some(handle);
            ts.egui_texture = Some(tex_id);
        }
    }

    let ts = &state.tilesets[ts_idx];
    let Some(tex_id) = ts.egui_texture else {
        return;
    };

    let tile_w = ts.tile_width as f32;
    let tile_h = ts.tile_height as f32;
    let cols = ts.columns;
    let rows = (ts.tile_count + cols - 1) / cols;
    let atlas_w = cols as f32 * tile_w;
    let atlas_h = rows as f32 * tile_h;

    // ── Selection info ─────────────────────────────────────────────
    let selected_count = state
        .selected_tiles
        .iter()
        .filter(|t| t.tileset_idx == ts_idx)
        .count();
    ui.label(format!(
        "{} tile(s) selected — Shift+click to multi-select",
        selected_count
    ));

    // ── Tile grid ──────────────────────────────────────────────────
    let thumb_size = 40.0_f32;
    let grid_height = (rows as f32 * (thumb_size + 2.0)).min(400.0).max(60.0);

    let shift_held = keyboard.pressed(KeyCode::ShiftLeft)
        || keyboard.pressed(KeyCode::ShiftRight);

    egui::ScrollArea::vertical()
        .max_height(grid_height)
        .id_salt("tile_grid")
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for tile_id in 0..ts.tile_count {
                    let col = tile_id % cols;
                    let row = tile_id / cols;

                    // UV rect for this tile in the atlas
                    let u_min = col as f32 * tile_w / atlas_w;
                    let v_min = row as f32 * tile_h / atlas_h;
                    let u_max = (col as f32 + 1.0) * tile_w / atlas_w;
                    let v_max = (row as f32 + 1.0) * tile_h / atlas_h;

                    let is_selected = state.selected_tiles.iter().any(|t| {
                        t.tileset_idx == ts_idx && t.tile_id == tile_id
                    });

                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(thumb_size, thumb_size),
                        egui::Sense::click(),
                    );

                    // Draw tile from atlas sub-rect
                    let tint = if is_selected {
                        egui::Color32::WHITE
                    } else {
                        egui::Color32::from_gray(180)
                    };
                    ui.painter().image(
                        tex_id,
                        rect,
                        egui::Rect::from_min_max(
                            egui::pos2(u_min, v_min),
                            egui::pos2(u_max, v_max),
                        ),
                        tint,
                    );

                    // Selection border
                    if is_selected {
                        ui.painter().rect_stroke(
                            rect,
                            1.0,
                            egui::Stroke::new(2.0, egui::Color32::YELLOW),
                            egui::StrokeKind::Outside,
                        );
                    }

                    // Tile index on hover
                    let resp = resp.on_hover_text(format!("Tile #{tile_id}"));

                    if resp.clicked() {
                        let sel = SelectedTile {
                            tileset_idx: ts_idx,
                            tile_id,
                        };
                        if shift_held {
                            // Toggle in multi-select
                            if let Some(pos) = state.selected_tiles.iter().position(|t| {
                                t.tileset_idx == ts_idx && t.tile_id == tile_id
                            }) {
                                state.selected_tiles.remove(pos);
                            } else {
                                state.selected_tiles.push(sel);
                            }
                        } else {
                            // Single select — clear and select
                            state.selected_tiles.clear();
                            state.selected_tiles.push(sel);
                        }
                    }
                }
            });
        });

    // ── Clear selection button ─────────────────────────────────────
    ui.horizontal(|ui| {
        if !state.selected_tiles.is_empty() && ui.button("Clear Selection").clicked() {
            state.selected_tiles.clear();
        }
    });
}
