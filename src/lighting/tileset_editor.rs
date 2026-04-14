use std::path::PathBuf;

use bevy::prelude::*;
use bevy::utils::HashMap;
use bevy_egui::{EguiContexts, egui};
use bevy_ecs_tiled::prelude::*;


// ── Types ───────────────────────────────────────────────────────────────────

#[derive(Default, Clone)]
pub struct TileLightEdit {
    pub enabled: bool,
    pub radius: f32,
    pub intensity: f32,
    pub color: [f32; 3],
    pub pulse: bool,
    pub flicker: bool,
    pub offset_x: f32,
    pub offset_y: f32,
    /// 0=point, 1=cone, 2=line, 3=capsule
    pub shape: u8,
    /// Direction angle in degrees (cone, capsule).
    pub direction: f32,
    /// Full cone spread in degrees (cone).
    pub angle: f32,
    /// Half-length in world units (capsule).
    pub length: f32,
    /// Line endpoint2 offset as tile fraction.
    pub end_offset_x: f32,
    pub end_offset_y: f32,
}

#[derive(Resource, Default)]
pub struct TilesetEditorState {
    pub tsx_path: Option<PathBuf>,
    pub selected_tileset_idx: usize,
    pub selected_tile: Option<u32>,
    /// Cached egui texture ID per tileset name.
    pub tileset_textures: HashMap<String, egui::TextureId>,
    /// Pending edits keyed by (tsx filename, tile_id). Persists across tileset switches.
    pub all_edits: HashMap<String, HashMap<u32, TileLightEdit>>,
    pub dirty: bool,
    /// Parsed from TSX: tile_width, tile_height, columns, total tiles, image path.
    pub tileset_info: Option<TilesetInfo>,
    /// Cached list of .tsx files found in assets/tilesets/.
    pub available_tsx: Option<Vec<PathBuf>>,
}

#[derive(Clone)]
pub struct TilesetInfo {
    pub name: String,
    pub tile_width: u32,
    pub tile_height: u32,
    pub columns: u32,
    pub tile_count: u32,
    pub image_source: String,
}

// ── UI ──────────────────────────────────────────────────────────────────────

/// Register tileset texture with egui if needed. Call before UI rendering.
pub fn ensure_texture_registered(
    state: &mut TilesetEditorState,
    contexts: &mut EguiContexts,
    asset_server: &AssetServer,
) {
    let (Some(tsx_path), Some(info)) = (&state.tsx_path, &state.tileset_info) else {
        return;
    };
    if state.tileset_textures.contains_key(&info.name) {
        return;
    }
    let Some(_ctx) = contexts.try_ctx_mut() else {
        return;
    };
    // Build path relative to assets/ for the asset server
    let full_path = tsx_path
        .parent()
        .unwrap_or(tsx_path.as_path())
        .join(&info.image_source);
    let asset_path = full_path
        .strip_prefix("assets/")
        .or_else(|_| full_path.strip_prefix("assets"))
        .unwrap_or(&full_path);
    let handle: Handle<Image> = asset_server.load(asset_path.to_path_buf());
    let id = contexts.add_image(handle);
    state.tileset_textures.insert(info.name.clone(), id);
}

pub fn tileset_editor_section(
    ui: &mut egui::Ui,
    state: &mut TilesetEditorState,
    _asset_server: &AssetServer,
    _map_assets: &Assets<TiledMap>,
    _map_handles: &Query<(Entity, &TiledMapHandle)>,
    _commands: &mut Commands,
) {
    ui.heading("Tileset Lights");

    // ── Scan for .tsx files on first open ──────────────────────────
    if state.available_tsx.is_none() {
        let mut files = Vec::new();
        if let Ok(entries) = std::fs::read_dir("assets/tilesets") {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "tsx") {
                    files.push(path);
                }
            }
        }
        files.sort();
        state.available_tsx = Some(files);
    }

    // ── TSX file selector ───────────────────────────────────────────
    let files = state.available_tsx.clone().unwrap_or_default();
    let current_label = state
        .tsx_path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "Select tileset...".to_string());

    egui::ComboBox::from_label("Tileset")
        .selected_text(&current_label)
        .show_ui(ui, |ui| {
            for path in &files {
                let label = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let is_selected = state.tsx_path.as_ref() == Some(path);
                if ui.selectable_label(is_selected, &label).clicked() && !is_selected {
                    state.tsx_path = Some(path.clone());
                    state.selected_tile = None;
                    state.tileset_info = None;
                }
            }
        });

    if ui.button("Refresh list").clicked() {
        state.available_tsx = None;
    }

    let Some(ref tsx_path) = state.tsx_path.clone() else {
        return;
    };
    let tsx_key = tsx_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    // ── Load tileset info + existing edits from TSX file on disk ────
    if state.tileset_info.is_none() {
        if let Some(info) = load_tileset_info_from_tsx(tsx_path) {
            if !state.all_edits.contains_key(&tsx_key) {
                let edits = state.all_edits.entry(tsx_key.clone()).or_default();
                load_existing_edits_from_tsx(tsx_path, edits);
            }
            state.tileset_info = Some(info);
        }
    }

    let Some(ref info) = state.tileset_info.clone() else {
        ui.label("Could not parse TSX file");
        return;
    };

    // Get the edit map for the current tileset
    let edits = state.all_edits.entry(tsx_key.clone()).or_default();

    // ── Get tileset texture (registered by ensure_texture_registered) ──
    let Some(&tex_id) = state.tileset_textures.get(&info.name) else {
        ui.label("Loading tileset texture...");
        return;
    };

    // ── Tile grid ───────────────────────────────────────────────────
    let display_size = 40.0_f32; // pixels per tile in the grid
    let cols = info.columns as usize;
    let rows = ((info.tile_count as usize) + cols - 1) / cols;

    ui.label(format!(
        "{}: {}x{} tiles, {}x{}px each",
        info.name, cols, rows, info.tile_width, info.tile_height
    ));

    let grid_height = (rows as f32 * (display_size + 2.0)).min(300.0);
    egui::ScrollArea::vertical()
        .max_height(grid_height)
        .show(ui, |ui| {
            let tile_uv_w = 1.0 / info.columns as f32;
            let tile_uv_h = 1.0 / rows.max(1) as f32;

            ui.horizontal_wrapped(|ui| {
                for tile_id in 0..info.tile_count {
                    let col = tile_id % info.columns;
                    let row = tile_id / info.columns;
                    let uv_min = egui::pos2(
                        col as f32 * tile_uv_w,
                        row as f32 * tile_uv_h,
                    );
                    let uv_max = egui::pos2(
                        (col + 1) as f32 * tile_uv_w,
                        (row + 1) as f32 * tile_uv_h,
                    );

                    let is_selected = state.selected_tile == Some(tile_id);
                    let has_light = edits
                        .get(&tile_id)
                        .is_some_and(|e| e.enabled);

                    let tint = if is_selected {
                        egui::Color32::WHITE
                    } else if has_light {
                        egui::Color32::from_rgb(255, 200, 100)
                    } else {
                        egui::Color32::from_gray(180)
                    };

                    let img = egui::Image::new(egui::load::SizedTexture::new(
                        tex_id,
                        egui::vec2(display_size, display_size),
                    ))
                    .uv(egui::Rect::from_min_max(uv_min, uv_max))
                    .tint(tint);

                    let resp = ui.add(img).interact(egui::Sense::click());
                    if resp.clicked() {
                        state.selected_tile = Some(tile_id);
                        // Init edit entry if not present
                        edits.entry(tile_id).or_insert_with(|| TileLightEdit {
                            enabled: false,
                            radius: 100.0,
                            intensity: 1.0,
                            color: [1.0, 0.85, 0.6],
                            pulse: false,
                            flicker: false,
                            offset_x: 0.0,
                            offset_y: 0.0,
                            shape: 0,
                            direction: 0.0,
                            angle: 90.0,
                            length: 48.0,
                            end_offset_x: 1.0,
                            end_offset_y: 0.0,
                        });
                    }
                }
            });
        });

    // ── Tile editor ─────────────────────────────────────────────────
    if let Some(tile_id) = state.selected_tile {
        ui.separator();
        ui.label(format!("Tile #{tile_id}"));

        if let Some(edit) = edits.get_mut(&tile_id) {
            let was_enabled = edit.enabled;
            ui.checkbox(&mut edit.enabled, "Has Light");

            if edit.enabled {
                ui.horizontal(|ui| {
                    ui.label("Color");
                    egui::color_picker::color_edit_button_rgb(ui, &mut edit.color);
                });
                ui.add(egui::Slider::new(&mut edit.radius, 10.0..=500.0).text("Radius"));
                ui.add(egui::Slider::new(&mut edit.intensity, 0.1..=5.0).text("Intensity"));

                // Shape
                ui.horizontal(|ui| {
                    ui.label("Shape");
                    egui::ComboBox::from_id_salt("tile_shape")
                        .width(80.0)
                        .selected_text(match edit.shape {
                            1 => "Cone",
                            2 => "Line",
                            3 => "Capsule",
                            _ => "Point",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut edit.shape, 0, "Point");
                            ui.selectable_value(&mut edit.shape, 1, "Cone");
                            ui.selectable_value(&mut edit.shape, 2, "Line");
                            ui.selectable_value(&mut edit.shape, 3, "Capsule");
                        });
                });
                // Shape-specific params
                match edit.shape {
                    1 => {
                        // Cone
                        ui.add(egui::Slider::new(&mut edit.direction, 0.0..=360.0).text("Direction"));
                        ui.add(egui::Slider::new(&mut edit.angle, 1.0..=180.0).text("Cone Angle"));
                    }
                    2 => {
                        // Line
                        ui.horizontal(|ui| {
                            ui.label("End offset");
                            ui.add(egui::DragValue::new(&mut edit.end_offset_x).range(-3.0..=3.0).speed(0.05).prefix("X: "));
                            ui.add(egui::DragValue::new(&mut edit.end_offset_y).range(-3.0..=3.0).speed(0.05).prefix("Y: "));
                        });
                    }
                    3 => {
                        // Capsule
                        ui.add(egui::Slider::new(&mut edit.direction, 0.0..=360.0).text("Direction"));
                        ui.add(egui::Slider::new(&mut edit.length, 1.0..=500.0).text("Length"));
                    }
                    _ => {} // Point: no extra params
                }

                // Pulse/Flicker as simple checkboxes (uses default configs)
                ui.checkbox(&mut edit.pulse, "Pulse");
                ui.checkbox(&mut edit.flicker, "Flicker");

                // Offset
                ui.horizontal(|ui| {
                    ui.label("Offset");
                    ui.add(
                        egui::DragValue::new(&mut edit.offset_x)
                            .range(-0.5..=0.5)
                            .speed(0.01)
                            .prefix("X: "),
                    );
                    ui.add(
                        egui::DragValue::new(&mut edit.offset_y)
                            .range(-0.5..=0.5)
                            .speed(0.01)
                            .prefix("Y: "),
                    );
                });

                // Offset preview: tile image with light position dot
                let preview_size = 80.0;
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(preview_size, preview_size),
                    egui::Sense::hover(),
                );

                // Draw the selected tile as background
                if let Some(info) = &state.tileset_info {
                    if let Some(&tex) = state.tileset_textures.get(&info.name) {
                        let col = tile_id % info.columns;
                        let row = tile_id / info.columns;
                        let rows = (info.tile_count + info.columns - 1) / info.columns;
                        let uv_w = 1.0 / info.columns as f32;
                        let uv_h = 1.0 / rows.max(1) as f32;
                        let uv = egui::Rect::from_min_max(
                            egui::pos2(col as f32 * uv_w, row as f32 * uv_h),
                            egui::pos2((col + 1) as f32 * uv_w, (row + 1) as f32 * uv_h),
                        );
                        ui.painter().image(
                            tex, rect, uv, egui::Color32::WHITE,
                        );
                    }
                }

                // Border
                ui.painter().rect_stroke(
                    rect,
                    0.0,
                    egui::Stroke::new(1.0, egui::Color32::GRAY),
                    egui::StrokeKind::Outside,
                );

                // ── Shape-aware light visualization ─────────────────
                let light_x = rect.center().x + edit.offset_x * preview_size;
                let light_y = rect.center().y - edit.offset_y * preview_size;
                let light_pos = egui::pos2(light_x, light_y);
                let c = edit.color;
                let dot_color = egui::Color32::from_rgb(
                    (c[0] * 255.0) as u8,
                    (c[1] * 255.0) as u8,
                    (c[2] * 255.0) as u8,
                );
                let faint = egui::Color32::from_rgba_unmultiplied(
                    (c[0] * 255.0) as u8,
                    (c[1] * 255.0) as u8,
                    (c[2] * 255.0) as u8,
                    60,
                );
                let painter = ui.painter();
                // Scale factor: map world-unit radius to preview pixels
                let scale = preview_size / edit.radius.max(1.0) * 0.4;

                match edit.shape {
                    0 => {
                        // Point: radial falloff circles
                        let outer_r = edit.radius * scale;
                        let inner_r = outer_r * 0.3;
                        painter.circle_filled(light_pos, outer_r, faint);
                        painter.circle_stroke(light_pos, outer_r, egui::Stroke::new(1.0, dot_color));
                        painter.circle_stroke(light_pos, inner_r, egui::Stroke::new(0.5, dot_color));
                        painter.circle_filled(light_pos, 3.0, dot_color);
                    }
                    1 => {
                        // Cone: wedge shape
                        let range = edit.radius * scale;
                        let half_angle = (edit.angle * 0.5).to_radians();
                        // Direction in preview: 0=right, angles in screen space (Y down)
                        let dir_rad = -edit.direction.to_radians();
                        let segments = 20;
                        let mut points = vec![light_pos];
                        for i in 0..=segments {
                            let t = i as f32 / segments as f32;
                            let a = dir_rad - half_angle + t * 2.0 * half_angle;
                            points.push(egui::pos2(
                                light_x + a.cos() * range,
                                light_y + a.sin() * range,
                            ));
                        }
                        // Filled wedge via triangles
                        for i in 1..points.len() - 1 {
                            painter.add(egui::Shape::convex_polygon(
                                vec![points[0], points[i], points[i + 1]],
                                faint,
                                egui::Stroke::NONE,
                            ));
                        }
                        // Outline
                        let mut outline = points.clone();
                        outline.push(points[0]);
                        painter.add(egui::Shape::line(outline, egui::Stroke::new(1.0, dot_color)));
                        painter.circle_filled(light_pos, 3.0, dot_color);
                    }
                    2 => {
                        // Line: segment with perpendicular falloff bands
                        let end_x = light_x + edit.end_offset_x * preview_size;
                        let end_y = light_y - edit.end_offset_y * preview_size;
                        let end_pos = egui::pos2(end_x, end_y);
                        let dx = end_x - light_x;
                        let dy = end_y - light_y;
                        let len = (dx * dx + dy * dy).sqrt().max(0.001);
                        // Perpendicular direction
                        let perp = egui::vec2(-dy / len, dx / len);
                        let outer_w = edit.radius * scale;

                        // Outer band (faint rectangle around the line)
                        let corners = [
                            egui::pos2(light_x + perp.x * outer_w, light_y + perp.y * outer_w),
                            egui::pos2(end_x + perp.x * outer_w, end_y + perp.y * outer_w),
                            egui::pos2(end_x - perp.x * outer_w, end_y - perp.y * outer_w),
                            egui::pos2(light_x - perp.x * outer_w, light_y - perp.y * outer_w),
                        ];
                        painter.add(egui::Shape::convex_polygon(
                            corners.to_vec(), faint, egui::Stroke::new(1.0, dot_color),
                        ));
                        // Center line
                        painter.line_segment([light_pos, end_pos], egui::Stroke::new(2.0, dot_color));
                        painter.circle_filled(light_pos, 3.0, dot_color);
                        painter.circle_filled(end_pos, 3.0, dot_color);
                    }
                    3 => {
                        // Capsule: elongated shape along direction
                        let dir_rad = -edit.direction.to_radians();
                        let half_len = edit.length * 0.5 * scale;
                        let outer_r = edit.radius * scale;
                        let dir = egui::vec2(dir_rad.cos(), dir_rad.sin());
                        let perp = egui::vec2(-dir.y, dir.x);
                        let a = egui::pos2(light_x - dir.x * half_len, light_y - dir.y * half_len);
                        let b = egui::pos2(light_x + dir.x * half_len, light_y + dir.y * half_len);

                        // Rectangle body
                        let corners = [
                            egui::pos2(a.x + perp.x * outer_r, a.y + perp.y * outer_r),
                            egui::pos2(b.x + perp.x * outer_r, b.y + perp.y * outer_r),
                            egui::pos2(b.x - perp.x * outer_r, b.y - perp.y * outer_r),
                            egui::pos2(a.x - perp.x * outer_r, a.y - perp.y * outer_r),
                        ];
                        painter.add(egui::Shape::convex_polygon(
                            corners.to_vec(), faint, egui::Stroke::NONE,
                        ));
                        // End caps
                        painter.circle_filled(a, outer_r, faint);
                        painter.circle_filled(b, outer_r, faint);
                        // Outline
                        painter.circle_stroke(a, outer_r, egui::Stroke::new(1.0, dot_color));
                        painter.circle_stroke(b, outer_r, egui::Stroke::new(1.0, dot_color));
                        // Center line
                        painter.line_segment([a, b], egui::Stroke::new(1.5, dot_color));
                        painter.circle_filled(light_pos, 3.0, dot_color);
                    }
                    _ => {
                        painter.circle_filled(light_pos, 4.0, dot_color);
                    }
                }
            }

            if edit.enabled != was_enabled {
                state.dirty = true;
            }
            // Any interaction marks dirty (simplified)
            state.dirty = true;
        }
    }

    // ── Save button ─────────────────────────────────────────────────
    ui.separator();
    let save_label = if state.dirty { "Save *" } else { "Save" };
    if ui.button(save_label).clicked() {
        if let Err(e) = save_tsx_changes(tsx_path, &edits) {
            error!("Failed to save TSX: {e}");
        } else {
            info!("Saved tileset light changes to {}", tsx_path.display());
            state.dirty = false;
        }
    }
}

// ── TSX parsing ─────────────────────────────────────────────────────────────

fn load_tileset_info_from_tsx(path: &PathBuf) -> Option<TilesetInfo> {
    let content = std::fs::read_to_string(path).ok()?;

    // Parse key attributes from <tileset> element
    let name = extract_attr(&content, "tileset", "name")?;
    let tile_width: u32 = extract_attr(&content, "tileset", "tilewidth")?.parse().ok()?;
    let tile_height: u32 = extract_attr(&content, "tileset", "tileheight")?.parse().ok()?;
    let tile_count: u32 = extract_attr(&content, "tileset", "tilecount")?.parse().ok()?;
    let columns: u32 = extract_attr(&content, "tileset", "columns")?.parse().ok()?;
    let image_source = extract_attr(&content, "image", "source")?;

    Some(TilesetInfo {
        name,
        tile_width,
        tile_height,
        columns,
        tile_count,
        image_source,
    })
}

fn extract_attr(xml: &str, element: &str, attr: &str) -> Option<String> {
    let tag_start = xml.find(&format!("<{element}"))?;
    let tag_region = &xml[tag_start..];
    let tag_end = tag_region.find('>')?;
    let tag = &tag_region[..tag_end];

    let attr_pattern = format!("{attr}=\"");
    let attr_start = tag.find(&attr_pattern)? + attr_pattern.len();
    let attr_end = tag[attr_start..].find('"')? + attr_start;
    Some(tag[attr_start..attr_end].to_string())
}

/// Parse existing TileLight entries directly from the TSX file on disk.
/// This works regardless of whether the tileset is part of the loaded map.
fn load_existing_edits_from_tsx(tsx_path: &PathBuf, edits: &mut HashMap<u32, TileLightEdit>) {
    let Ok(content) = std::fs::read_to_string(tsx_path) else {
        return;
    };

    // Find all <tile ... type="TileLight"> elements
    let mut search_from = 0;
    while let Some(tile_start) = content[search_from..].find("<tile ") {
        let abs_start = search_from + tile_start;
        let Some(rel_end) = find_tile_element_end(&content[abs_start..]) else {
            break;
        };
        let element = &content[abs_start..abs_start + rel_end];
        search_from = abs_start + rel_end;

        if !element.contains("type=\"TileLight\"") {
            continue;
        }

        // Extract tile id
        let Some(id_str) = extract_attr_from(element, "tile", "id") else {
            continue;
        };
        let Ok(tile_id) = id_str.parse::<u32>() else {
            continue;
        };

        let mut edit = TileLightEdit {
            enabled: true,
            radius: 100.0,
            intensity: 1.0,
            color: [1.0, 0.85, 0.6],
            pulse: false,
            flicker: false,
            offset_x: 0.0,
            offset_y: 0.0,
            shape: 0,
            direction: 0.0,
            angle: 90.0,
            length: 48.0,
            end_offset_x: 1.0,
            end_offset_y: 0.0,
        };

        // Parse <property> elements within this <tile>
        for prop_line in element.lines() {
            let trimmed = prop_line.trim();
            if !trimmed.starts_with("<property ") {
                continue;
            }
            let Some(name) = extract_attr_from(trimmed, "property", "name") else {
                continue;
            };
            let Some(value) = extract_attr_from(trimmed, "property", "value") else {
                continue;
            };
            match name.as_str() {
                "radius" => edit.radius = value.parse().unwrap_or(edit.radius),
                "intensity" => edit.intensity = value.parse().unwrap_or(edit.intensity),
                "color_r" => edit.color[0] = value.parse().unwrap_or(edit.color[0]),
                "color_g" => edit.color[1] = value.parse().unwrap_or(edit.color[1]),
                "color_b" => edit.color[2] = value.parse().unwrap_or(edit.color[2]),
                "pulse" => edit.pulse = value == "true",
                "flicker" => edit.flicker = value == "true",
                "offset_x" => edit.offset_x = value.parse().unwrap_or(0.0),
                "offset_y" => edit.offset_y = value.parse().unwrap_or(0.0),
                "shape" => {
                    edit.shape = match value.as_str() {
                        "cone" => 1,
                        "line" => 2,
                        "capsule" => 3,
                        _ => 0,
                    };
                }
                "direction" => edit.direction = value.parse().unwrap_or(0.0),
                "angle" => edit.angle = value.parse().unwrap_or(90.0),
                "length" => edit.length = value.parse().unwrap_or(48.0),
                "end_offset_x" => edit.end_offset_x = value.parse().unwrap_or(1.0),
                "end_offset_y" => edit.end_offset_y = value.parse().unwrap_or(0.0),
                _ => {}
            }
        }

        edits.insert(tile_id, edit);
    }
}

/// Extract an attribute value from an XML element string.
fn extract_attr_from(xml: &str, element: &str, attr: &str) -> Option<String> {
    let tag_start = xml.find(&format!("<{element}"))?;
    let tag_region = &xml[tag_start..];
    let tag_end = tag_region.find('>')?;
    let tag = &tag_region[..tag_end];
    let attr_pattern = format!("{attr}=\"");
    let attr_start = tag.find(&attr_pattern)? + attr_pattern.len();
    let attr_end = tag[attr_start..].find('"')? + attr_start;
    Some(tag[attr_start..attr_end].to_string())
}

// ── TSX save ────────────────────────────────────────────────────────────────

fn save_tsx_changes(
    tsx_path: &PathBuf,
    edits: &HashMap<u32, TileLightEdit>,
) -> Result<(), String> {
    let content = std::fs::read_to_string(tsx_path)
        .map_err(|e| format!("read {}: {e}", tsx_path.display()))?;

    info!("TSX save: read {} bytes from {}", content.len(), tsx_path.display());

    // Remove all existing <tile> elements that we manage (type="TileLight")
    let cleaned = remove_tile_light_elements(&content);

    // Build new <tile> elements for enabled edits
    let mut tile_elements = Vec::new();
    for (&tile_id, edit) in edits {
        if !edit.enabled {
            continue;
        }
        tile_elements.push(format_tile_element(tile_id, edit));
    }

    info!("TSX save: {} enabled light tile(s) to write", tile_elements.len());

    let mut result = cleaned;
    if !tile_elements.is_empty() {
        let insert_text = tile_elements.join("\n") + "\n";
        if let Some(pos) = result.rfind("</tileset>") {
            result.insert_str(pos, &insert_text);
        } else {
            return Err("No </tileset> closing tag found".to_string());
        }
    }

    std::fs::write(tsx_path, &result)
        .map_err(|e| format!("write {}: {e}", tsx_path.display()))?;

    info!("TSX save: wrote {} bytes to {}", result.len(), tsx_path.display());

    Ok(())
}

/// Remove <tile> elements with type="TileLight" from the TSX content.
/// Preserves other <tile> elements (e.g., animation, collision).
fn remove_tile_light_elements(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut i = 0;
    let bytes = content.as_bytes();

    while i < content.len() {
        // Look for <tile ... type="TileLight"
        if content[i..].starts_with("<tile ") {
            let tag_start = i;
            // Find the end of this tile element
            if let Some(rel_end) = find_tile_element_end(&content[i..]) {
                let element = &content[i..i + rel_end];
                if element.contains("type=\"TileLight\"") {
                    // Skip this element (and any preceding whitespace on the same line)
                    let mut ws_start = tag_start;
                    while ws_start > 0 && matches!(bytes[ws_start - 1], b' ' | b'\t') {
                        ws_start -= 1;
                    }
                    // Also eat the newline after
                    let mut skip_end = i + rel_end;
                    if skip_end < content.len() && bytes[skip_end] == b'\n' {
                        skip_end += 1;
                    }
                    // Truncate result to remove preceding whitespace
                    result.truncate(result.len() - (tag_start - ws_start));
                    i = skip_end;
                    continue;
                }
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }

    result
}

fn find_tile_element_end(s: &str) -> Option<usize> {
    // Self-closing: <tile ... />
    if let Some(pos) = s.find("/>") {
        // Check there's no </tile> before it
        if let Some(close_pos) = s.find("</tile>") {
            if close_pos < pos {
                return Some(close_pos + "</tile>".len());
            }
        }
        return Some(pos + 2);
    }
    // Closing tag: </tile>
    if let Some(pos) = s.find("</tile>") {
        return Some(pos + "</tile>".len());
    }
    None
}

fn format_tile_element(tile_id: u32, edit: &TileLightEdit) -> String {
    let mut props = Vec::new();
    props.push(format!(
        "   <property name=\"radius\" type=\"float\" value=\"{}\"/>",
        edit.radius
    ));
    props.push(format!(
        "   <property name=\"intensity\" type=\"float\" value=\"{}\"/>",
        edit.intensity
    ));
    props.push(format!(
        "   <property name=\"color_r\" type=\"float\" value=\"{}\"/>",
        edit.color[0]
    ));
    props.push(format!(
        "   <property name=\"color_g\" type=\"float\" value=\"{}\"/>",
        edit.color[1]
    ));
    props.push(format!(
        "   <property name=\"color_b\" type=\"float\" value=\"{}\"/>",
        edit.color[2]
    ));
    props.push(format!(
        "   <property name=\"pulse\" type=\"bool\" value=\"{}\"/>",
        edit.pulse
    ));
    props.push(format!(
        "   <property name=\"flicker\" type=\"bool\" value=\"{}\"/>",
        edit.flicker
    ));
    if edit.offset_x != 0.0 {
        props.push(format!(
            "   <property name=\"offset_x\" type=\"float\" value=\"{}\"/>",
            edit.offset_x
        ));
    }
    if edit.offset_y != 0.0 {
        props.push(format!(
            "   <property name=\"offset_y\" type=\"float\" value=\"{}\"/>",
            edit.offset_y
        ));
    }
    let shape_name = match edit.shape {
        1 => "cone",
        2 => "line",
        3 => "capsule",
        _ => "point",
    };
    if edit.shape != 0 {
        props.push(format!(
            "   <property name=\"shape\" type=\"string\" value=\"{shape_name}\"/>",
        ));
    }
    if edit.shape == 1 || edit.shape == 3 {
        props.push(format!(
            "   <property name=\"direction\" type=\"float\" value=\"{}\"/>",
            edit.direction
        ));
    }
    if edit.shape == 1 {
        props.push(format!(
            "   <property name=\"angle\" type=\"float\" value=\"{}\"/>",
            edit.angle
        ));
    }
    if edit.shape == 3 {
        props.push(format!(
            "   <property name=\"length\" type=\"float\" value=\"{}\"/>",
            edit.length
        ));
    }
    if edit.shape == 2 {
        props.push(format!(
            "   <property name=\"end_offset_x\" type=\"float\" value=\"{}\"/>",
            edit.end_offset_x
        ));
        props.push(format!(
            "   <property name=\"end_offset_y\" type=\"float\" value=\"{}\"/>",
            edit.end_offset_y
        ));
    }

    format!(
        " <tile id=\"{tile_id}\" type=\"TileLight\">\n  <properties>\n{}\n  </properties>\n </tile>",
        props.join("\n")
    )
}
