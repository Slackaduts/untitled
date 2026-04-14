use std::path::PathBuf;

use bevy::prelude::*;
use std::collections::HashMap;
use bevy_egui::{EguiContexts, egui};

use super::properties::{TileBillboardEdit, BillboardPropertyDefs, load_existing_edits_from_tsx, save_tsx_changes};

// ── State ──────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct TilesetInfo {
    name: String,
    tile_width: u32,
    tile_height: u32,
    columns: u32,
    tile_count: u32,
    image_source: String,
}

/// Cached collision shapes for a tile, parsed from the TSX.
#[derive(Clone, Default)]
struct TileCollisionShapes {
    /// List of (shape_type, x, y, w, h) in tile-local pixels.
    /// shape_type: 0=rect, 1=ellipse, 2=polygon
    pub rects: Vec<(f32, f32, f32, f32)>,
    pub polygons: Vec<Vec<(f32, f32)>>,
}

#[derive(Resource)]
pub struct BillboardEditorState {
    pub open: bool,
    pub selected_tsx: Option<PathBuf>,
    pub selected_tile: Option<u32>,
    pub available_tsx: Option<Vec<PathBuf>>,
    pub tileset_info: HashMap<String, TilesetInfo>,
    pub tileset_textures: HashMap<String, egui::TextureId>,
    pub tileset_image_handles: HashMap<String, Handle<Image>>,
    pub edits: HashMap<String, HashMap<u32, TileBillboardEdit>>,
    pub dirty: bool,
    /// When true, tiles in the grid keep their original aspect ratio.
    pub preserve_ratio: bool,
    /// Scale factor for the tile grid display.
    pub grid_scale: f32,
    /// Cached collision shapes per (tsx_key, tile_id).
    pub collision_cache: HashMap<String, HashMap<u32, TileCollisionShapes>>,
}

impl Default for BillboardEditorState {
    fn default() -> Self {
        Self {
            open: false,
            selected_tsx: None,
            selected_tile: None,
            available_tsx: None,
            tileset_info: HashMap::default(),
            tileset_textures: HashMap::default(),
            tileset_image_handles: HashMap::default(),
            edits: HashMap::default(),
            dirty: false,
            preserve_ratio: false,
            grid_scale: 1.0,
            collision_cache: HashMap::default(),
        }
    }
}

// ── System ─────────────────────────────────────────────────────────────────

pub fn billboard_editor_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut contexts: EguiContexts,
    mut state: ResMut<BillboardEditorState>,
    asset_server: Res<AssetServer>,
    mut billboard_defs: ResMut<BillboardPropertyDefs>,
    images: Res<Assets<Image>>,
) {
    if keyboard.just_pressed(KeyCode::F6) {
        state.open = !state.open;
    }
    if !state.open { return; }

    // Register textures for selected tileset
    if let Some(tsx_path) = &state.selected_tsx {
        let tsx_key = tsx_path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if !state.tileset_textures.contains_key(&tsx_key) {
            if let Some(info) = state.tileset_info.get(&tsx_key) {
                let full_path = tsx_path.parent()
                    .unwrap_or(tsx_path.as_path())
                    .join(&info.image_source);
                let asset_path = full_path
                    .strip_prefix("assets/")
                    .or_else(|_| full_path.strip_prefix("assets"))
                    .unwrap_or(&full_path)
                    .to_path_buf();
                let handle: Handle<Image> = asset_server.load(asset_path);
                let id = contexts.add_image(handle.clone());
                state.tileset_image_handles.insert(tsx_key.clone(), handle);
                state.tileset_textures.insert(tsx_key, id);
            }
        }
    }

    let Some(ctx) = contexts.try_ctx_mut() else { return };
    let state = &mut *state;

    egui::Window::new("Billboard Properties")
        .default_width(400.0)
        .default_height(500.0)
        .show(ctx, |ui| {
            // ── Scan TSX files ──────────────────────────────────────
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

            // ── TSX selector ────────────────────────────────────────
            let current_label = state.selected_tsx.as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Select tileset...".to_string());

            let files = state.available_tsx.clone().unwrap_or_default();
            egui::ComboBox::from_id_salt("bb_tsx_select")
                .selected_text(&current_label)
                .show_ui(ui, |ui| {
                    for path in &files {
                        let label = path.file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        let is_sel = state.selected_tsx.as_ref() == Some(path);
                        if ui.selectable_label(is_sel, &label).clicked() && !is_sel {
                            state.selected_tsx = Some(path.clone());
                            state.selected_tile = None;

                            // Load tileset info
                            let tsx_key = label.clone();
                            if !state.tileset_info.contains_key(&tsx_key) {
                                if let Some(info) = load_tileset_info_from_tsx(path) {
                                    state.tileset_info.insert(tsx_key.clone(), info);
                                }
                            }

                            // Load existing edits from TSX
                            if !state.edits.contains_key(&tsx_key) {
                                if let Ok(content) = std::fs::read_to_string(path) {
                                    let edits = load_existing_edits_from_tsx(&content);
                                    state.edits.insert(tsx_key, edits);
                                }
                            }
                        }
                    }
                });

            ui.separator();

            // ── Tile grid ───────────────────────────────────────────
            let Some(tsx_path) = &state.selected_tsx.clone() else { return };
            let tsx_key = tsx_path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let Some(info) = state.tileset_info.get(&tsx_key).cloned() else { return };
            let Some(&tex_id) = state.tileset_textures.get(&tsx_key) else {
                ui.label("Loading tileset texture...");
                return;
            };

            let rows = (info.tile_count + info.columns - 1) / info.columns;
            let tile_uv_w = 1.0 / info.columns as f32;
            let tile_uv_h = 1.0 / rows.max(1) as f32;
            let edits = state.edits.entry(tsx_key.clone()).or_default();

            // Display options
            ui.horizontal(|ui| {
                ui.checkbox(&mut state.preserve_ratio, "Original size");
                ui.add(egui::Slider::new(&mut state.grid_scale, 0.25..=3.0).text("Scale"));
            });

            // Compute display dimensions
            let (display_w, display_h) = if state.preserve_ratio {
                (
                    info.tile_width as f32 * state.grid_scale,
                    info.tile_height as f32 * state.grid_scale,
                )
            } else {
                let s = 40.0 * state.grid_scale;
                (s, s)
            };

            let grid_height = (rows as f32 * (display_h + 2.0)).min(400.0);
            egui::ScrollArea::both()
                .max_height(grid_height)
                .id_salt("bb_tile_grid")
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for tile_id in 0..info.tile_count {
                            let col = tile_id % info.columns;
                            let row = tile_id / info.columns;
                            let uv_min = egui::pos2(col as f32 * tile_uv_w, row as f32 * tile_uv_h);
                            let uv_max = egui::pos2((col + 1) as f32 * tile_uv_w, (row + 1) as f32 * tile_uv_h);

                            let is_sel = state.selected_tile == Some(tile_id);
                            let has_props = edits.get(&tile_id)
                                .is_some_and(|e| !e.is_default());
                            let tint = if is_sel {
                                egui::Color32::WHITE
                            } else if has_props {
                                egui::Color32::from_rgb(100, 220, 220)
                            } else {
                                egui::Color32::from_gray(150)
                            };

                            let img = egui::Image::new(
                                egui::load::SizedTexture::new(tex_id, egui::vec2(display_w, display_h)),
                            ).uv(egui::Rect::from_min_max(uv_min, uv_max)).tint(tint);

                            let resp = ui.add(img).interact(egui::Sense::click());
                            if resp.clicked() {
                                state.selected_tile = Some(tile_id);
                                edits.entry(tile_id).or_default();
                            }
                        }
                    });
                });

            ui.separator();

            // ── Property panel ──────────────────────────────────────
            let Some(tile_id) = state.selected_tile else { return };
            let edit = edits.entry(tile_id).or_default();

            ui.heading(format!("Tile {} Properties", tile_id));

            // Interactive origin editor preview
            let preview_size = 120.0;
            let col = tile_id % info.columns;
            let row = tile_id / info.columns;
            let uv_min = egui::pos2(col as f32 * tile_uv_w, row as f32 * tile_uv_h);
            let uv_max = egui::pos2((col + 1) as f32 * tile_uv_w, (row + 1) as f32 * tile_uv_h);

            ui.label("Origin (click to place):");
            let (rect, resp) = ui.allocate_exact_size(
                egui::vec2(preview_size, preview_size),
                egui::Sense::click_and_drag(),
            );

            // Draw tile preview
            ui.painter().image(
                tex_id, rect,
                egui::Rect::from_min_max(uv_min, uv_max),
                egui::Color32::WHITE,
            );

            // Draw blend zone overlay
            if edit.blend_height > 0.0 {
                let blend_frac = edit.blend_height / info.tile_height as f32;
                let blend_px = blend_frac * preview_size;
                let blend_rect = egui::Rect::from_min_max(
                    egui::pos2(rect.min.x, rect.max.y - blend_px),
                    rect.max,
                );
                ui.painter().rect_filled(blend_rect, 0.0, egui::Color32::from_rgba_unmultiplied(0, 100, 255, 60));
            }

            // Draw origin crosshair
            let origin_px = egui::pos2(
                rect.min.x + edit.origin_x * preview_size,
                rect.min.y + edit.origin_y * preview_size,
            );
            let cross_size = 8.0;
            ui.painter().line_segment(
                [egui::pos2(origin_px.x - cross_size, origin_px.y), egui::pos2(origin_px.x + cross_size, origin_px.y)],
                egui::Stroke::new(2.0, egui::Color32::RED),
            );
            ui.painter().line_segment(
                [egui::pos2(origin_px.x, origin_px.y - cross_size), egui::pos2(origin_px.x, origin_px.y + cross_size)],
                egui::Stroke::new(2.0, egui::Color32::RED),
            );
            ui.painter().circle_filled(origin_px, 3.0, egui::Color32::RED);

            // Click/drag to set origin
            if resp.clicked() || resp.dragged() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let local = pos - rect.min;
                    edit.origin_x = (local.x / preview_size).clamp(0.0, 1.0);
                    edit.origin_y = (local.y / preview_size).clamp(0.0, 1.0);
                    state.dirty = true;
                }
            }

            ui.add_space(4.0);

            // Ground blend slider
            ui.horizontal(|ui| {
                ui.label("Ground blend:");
                if ui.add(egui::Slider::new(&mut edit.blend_height, 0.0..=32.0).suffix("px")).changed() {
                    state.dirty = true;
                }
            });

            // Tilt override
            ui.horizontal(|ui| {
                ui.label("Tilt override:");
                if ui.add(egui::Slider::new(&mut edit.tilt_override, -1.0..=90.0).suffix("°")).changed() {
                    state.dirty = true;
                }
            });
            if edit.tilt_override < 0.0 {
                ui.label("  (-1 = use global default)");
            }

            // Z offset
            ui.horizontal(|ui| {
                ui.label("Z offset:");
                if ui.add(egui::DragValue::new(&mut edit.z_offset).speed(0.5).suffix(" units")).changed() {
                    state.dirty = true;
                }
            });

            ui.separator();
            ui.heading("Collider");

            // Collider depth
            ui.horizontal(|ui| {
                ui.label("Depth (Z):");
                if ui.add(egui::Slider::new(&mut edit.collider_depth, 0.0..=240.0).suffix(" units")).changed() {
                    state.dirty = true;
                }
            });

            // Collider XY override
            ui.horizontal(|ui| {
                ui.label("Width override:");
                if ui.add(egui::DragValue::new(&mut edit.collider_w).speed(0.5).suffix(" (-1=Tiled)")).changed() {
                    state.dirty = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Height override:");
                if ui.add(egui::DragValue::new(&mut edit.collider_h).speed(0.5).suffix(" (-1=Tiled)")).changed() {
                    state.dirty = true;
                }
            });

            // 3D collider depth preview (taller to fit the diagonal view)
            let preview_h = 140.0;
            let (preview_rect, _) = ui.allocate_exact_size(
                egui::vec2(preview_size + 60.0, preview_h),
                egui::Sense::hover(),
            );
            // Load collision shapes if not cached
            let coll_cache = state.collision_cache
                .entry(tsx_key.clone())
                .or_default();
            if !coll_cache.contains_key(&tile_id) {
                if let Some(shapes) = load_tile_collision_from_tsx(tsx_path, tile_id) {
                    coll_cache.insert(tile_id, shapes);
                }
            }
            let collision = state.collision_cache
                .get(&tsx_key)
                .and_then(|c| c.get(&tile_id));

            // Tile UVs for the sprite face
            let tile_uv = egui::Rect::from_min_max(uv_min, uv_max);
            draw_collider_3d_preview(
                ui, preview_rect, edit.collider_depth,
                info.tile_width as f32, collision, info.tile_height as f32,
                tex_id, tile_uv,
            );

            ui.separator();

            // Save / Reset buttons
            ui.horizontal(|ui| {
                if ui.button("Save to TSX").clicked() {
                    if let Some(tsx_path) = &state.selected_tsx {
                        let path_str = tsx_path.to_string_lossy().to_string();
                        match save_tsx_changes(&path_str, edits) {
                            Ok(()) => {
                                info!("Saved billboard properties to {}", path_str);
                                // Update runtime defs
                                billboard_defs.by_tileset.insert(tsx_key.clone(), edits.clone());
                                state.dirty = false;
                            }
                            Err(e) => error!("Failed to save billboard properties: {e}"),
                        }
                    }
                }

                if ui.button("Reset tile").clicked() {
                    edits.remove(&tile_id);
                    state.dirty = true;
                }

                if state.dirty {
                    ui.label(egui::RichText::new("unsaved").color(egui::Color32::YELLOW));
                }
            });
        });
}

// ── 3D collider preview ────────────────────────────────────────────────────

fn draw_collider_3d_preview(
    ui: &egui::Ui, rect: egui::Rect, depth: f32, tile_w: f32,
    collision: Option<&TileCollisionShapes>, tile_h: f32,
    tex_id: egui::TextureId, tile_uv: egui::Rect,
) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, egui::Color32::from_gray(30));

    let scale = rect.width() / (tile_w * 3.0);
    let iso_angle = 0.4_f32;

    let cx = rect.center().x - tile_w * scale * 0.2;
    let bottom = rect.max.y - 10.0;
    let project = |x: f32, y: f32, z: f32| -> egui::Pos2 {
        egui::pos2(
            cx + (x * scale) + (y * scale * iso_angle),
            bottom - (z * scale) - (y * scale * iso_angle * 0.6),
        )
    };

    let line = egui::Stroke::new(1.5, egui::Color32::from_rgb(100, 200, 100));
    let dim = egui::Stroke::new(1.0, egui::Color32::from_rgb(60, 120, 60));
    let fill_col = egui::Color32::from_rgba_unmultiplied(80, 180, 80, 40);

    if let Some(coll) = collision {
        // Draw actual collision shapes extruded by depth
        for &(rx, ry, rw, rh) in &coll.rects {
            // Front face (y=0)
            let f_bl = project(rx, 0.0, 0.0);
            let f_br = project(rx + rw, 0.0, 0.0);
            let f_tr = project(rx + rw, 0.0, depth);
            let f_tl = project(rx, 0.0, depth);

            // Back face (y=rh, since Y is the depth axis in our iso view)
            let b_bl = project(rx, rh, 0.0);
            let b_br = project(rx + rw, rh, 0.0);
            let b_tr = project(rx + rw, rh, depth);
            let b_tl = project(rx, rh, depth);

            // Front face
            painter.line_segment([f_bl, f_br], line);
            painter.line_segment([f_br, f_tr], line);
            painter.line_segment([f_tr, f_tl], line);
            painter.line_segment([f_tl, f_bl], line);

            // Top face fill
            let top_pts = vec![f_tl, f_tr, b_tr, b_tl];
            painter.add(egui::Shape::convex_polygon(top_pts, fill_col, egui::Stroke::NONE));

            // Top face edges
            painter.line_segment([f_tl, b_tl], line);
            painter.line_segment([f_tr, b_tr], line);
            painter.line_segment([b_tl, b_tr], dim);

            // Back edges (dimmer)
            painter.line_segment([b_bl, b_br], dim);
            painter.line_segment([b_br, b_tr], dim);
            painter.line_segment([b_tl, b_bl], dim);

            // Connecting bottom edges
            painter.line_segment([f_br, b_br], line);
            painter.line_segment([f_bl, b_bl], dim);
        }

        // Draw polygon outlines extruded
        for poly in &coll.polygons {
            if poly.len() < 2 { continue; }
            // Bottom outline
            for i in 0..poly.len() {
                let (x0, y0) = poly[i];
                let (x1, y1) = poly[(i + 1) % poly.len()];
                painter.line_segment([project(x0, y0, 0.0), project(x1, y1, 0.0)], line);
                // Top outline
                painter.line_segment([project(x0, y0, depth), project(x1, y1, depth)], dim);
                // Vertical edges
                painter.line_segment([project(x0, y0, 0.0), project(x0, y0, depth)], line);
            }
        }
    } else {
        // No collision data — draw full tile bounding box
        let f_bl = project(0.0, 0.0, 0.0);
        let f_br = project(tile_w, 0.0, 0.0);
        let f_tr = project(tile_w, 0.0, depth);
        let f_tl = project(0.0, 0.0, depth);
        let b_bl = project(0.0, tile_h, 0.0);
        let b_br = project(tile_w, tile_h, 0.0);
        let b_tr = project(tile_w, tile_h, depth);
        let b_tl = project(0.0, tile_h, depth);

        painter.line_segment([f_bl, f_br], line);
        painter.line_segment([f_br, f_tr], line);
        painter.line_segment([f_tr, f_tl], line);
        painter.line_segment([f_tl, f_bl], line);
        painter.line_segment([b_bl, b_br], dim);
        painter.line_segment([b_br, b_tr], dim);
        painter.line_segment([b_tr, b_tl], dim);
        painter.line_segment([f_br, b_br], line);
        painter.line_segment([f_tr, b_tr], line);
        painter.line_segment([f_tl, b_tl], dim);
        painter.line_segment([f_bl, b_bl], dim);
    }

    // Draw the tile sprite on the front face of the collider.
    // The front face spans from (0,0,0) to (tile_w,0,depth) in 3D.
    // Map tile UVs to the projected quad corners.
    {
        let f_bl = project(0.0, 0.0, 0.0);
        let f_br = project(tile_w, 0.0, 0.0);
        let f_tr = project(tile_w, 0.0, depth);
        let f_tl = project(0.0, 0.0, depth);

        let tint = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 180);
        let mut mesh = egui::Mesh::with_texture(tex_id);
        mesh.vertices.push(egui::epaint::Vertex {
            pos: f_bl, uv: egui::pos2(tile_uv.min.x, tile_uv.max.y), color: tint,
        });
        mesh.vertices.push(egui::epaint::Vertex {
            pos: f_br, uv: egui::pos2(tile_uv.max.x, tile_uv.max.y), color: tint,
        });
        mesh.vertices.push(egui::epaint::Vertex {
            pos: f_tr, uv: egui::pos2(tile_uv.max.x, tile_uv.min.y), color: tint,
        });
        mesh.vertices.push(egui::epaint::Vertex {
            pos: f_tl, uv: egui::pos2(tile_uv.min.x, tile_uv.min.y), color: tint,
        });
        mesh.indices = vec![0, 1, 2, 0, 2, 3];
        painter.add(egui::Shape::mesh(mesh));
    }

    // Depth label
    let label_pos = project(tile_w * 0.5, 0.0, depth + 4.0);
    painter.text(
        label_pos,
        egui::Align2::CENTER_BOTTOM,
        format!("depth: {:.0}", depth),
        egui::FontId::proportional(11.0),
        egui::Color32::from_rgb(150, 255, 150),
    );
}

// ── TSX parsing helpers ────────────────────────────────────────────────────

/// Parse collision shapes from a TSX tile's <objectgroup> element.
fn load_tile_collision_from_tsx(tsx_path: &PathBuf, tile_id: u32) -> Option<TileCollisionShapes> {
    let content = std::fs::read_to_string(tsx_path).ok()?;
    let mut shapes = TileCollisionShapes::default();

    // Find <tile id="N"> that matches (without type= or with any type)
    let tile_pattern = format!("id=\"{}\"", tile_id);
    let mut search = content.as_str();
    while let Some(tile_start) = search.find("<tile ") {
        search = &search[tile_start..];
        let Some(tag_end) = search.find('>') else { search = &search[1..]; continue };
        let tag = &search[..tag_end + 1];

        if !tag.contains(&tile_pattern) {
            search = &search[1..];
            continue;
        }

        // Self-closing tag = no collision data
        if tag.contains("/>") {
            search = &search[1..];
            continue;
        }

        let Some(tile_close) = search.find("</tile>") else { break };
        let tile_content = &search[..tile_close];

        // Find <objectgroup> within this tile
        if let Some(og_start) = tile_content.find("<objectgroup") {
            let og_content = &tile_content[og_start..];

            // Parse <object> elements
            let mut obj_search = og_content;
            while let Some(obj_start) = obj_search.find("<object ") {
                obj_search = &obj_search[obj_start..];
                let Some(obj_end) = obj_search.find('>') else { break };
                let obj_tag = &obj_search[..obj_end + 1];

                let x: f32 = extract_attr_from_tag_local(obj_tag, "x")
                    .and_then(|s| s.parse().ok()).unwrap_or(0.0);
                let y: f32 = extract_attr_from_tag_local(obj_tag, "y")
                    .and_then(|s| s.parse().ok()).unwrap_or(0.0);
                let w: f32 = extract_attr_from_tag_local(obj_tag, "width")
                    .and_then(|s| s.parse().ok()).unwrap_or(0.0);
                let h: f32 = extract_attr_from_tag_local(obj_tag, "height")
                    .and_then(|s| s.parse().ok()).unwrap_or(0.0);

                // Check for polygon child
                if let Some(poly_start) = obj_search[..obj_search.find("</object>").unwrap_or(obj_search.len())].find("<polygon ") {
                    let poly_region = &obj_search[poly_start..];
                    if let Some(points_str) = extract_attr_from_tag_local(
                        &poly_region[..poly_region.find("/>").unwrap_or(poly_region.len()) + 2],
                        "points",
                    ) {
                        let pts: Vec<(f32, f32)> = points_str.split(' ')
                            .filter_map(|p| {
                                let mut parts = p.split(',');
                                let px = parts.next()?.parse::<f32>().ok()?;
                                let py = parts.next()?.parse::<f32>().ok()?;
                                Some((x + px, y + py))
                            })
                            .collect();
                        if pts.len() >= 3 {
                            shapes.polygons.push(pts);
                        }
                    }
                } else if w > 0.0 && h > 0.0 {
                    // Rectangle
                    shapes.rects.push((x, y, w, h));
                }

                obj_search = &obj_search[obj_end + 1..];
            }
        }

        break; // found our tile
    }

    if shapes.rects.is_empty() && shapes.polygons.is_empty() {
        None
    } else {
        Some(shapes)
    }
}

fn extract_attr_from_tag_local(tag: &str, attr: &str) -> Option<String> {
    let pattern = format!("{attr}=\"");
    let start = tag.find(&pattern)? + pattern.len();
    let end = tag[start..].find('"')? + start;
    Some(tag[start..end].to_string())
}

fn load_tileset_info_from_tsx(path: &PathBuf) -> Option<TilesetInfo> {
    let content = std::fs::read_to_string(path).ok()?;

    let name = extract_attr(&content, "tileset", "name")?;
    let tile_width: u32 = extract_attr(&content, "tileset", "tilewidth")?.parse().ok()?;
    let tile_height: u32 = extract_attr(&content, "tileset", "tileheight")?.parse().ok()?;
    let tile_count: u32 = extract_attr(&content, "tileset", "tilecount")?.parse().ok()?;
    let columns: u32 = extract_attr(&content, "tileset", "columns")?.parse().ok()?;
    let image_source = extract_attr(&content, "image", "source")?;

    Some(TilesetInfo {
        name, tile_width, tile_height, columns, tile_count, image_source,
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
