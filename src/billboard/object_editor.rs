use std::path::PathBuf;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

// ── Data structures ───────────────────────────────────────────────────────

fn default_half() -> f32 {
    0.5
}
fn default_color() -> [f32; 3] {
    [1.0, 0.85, 0.6]
}
fn default_intensity() -> f32 {
    1.5
}
fn default_radius() -> f32 {
    100.0
}
fn default_type() -> String {
    "organic".to_string()
}
fn default_shape() -> String {
    "point".to_string()
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
pub struct ObjectProperties {
    #[serde(default)]
    pub lights: Vec<ObjectLight>,
    #[serde(default)]
    pub blend_height: f32,
    /// Shadow mesh offset from billboard base, as fraction of sprite dimensions (0-1).
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
pub struct ObjectLight {
    /// X position on billboard as fraction (0=left, 1=right).
    #[serde(default = "default_half")]
    pub offset_x: f32,
    /// Y position on billboard as fraction (0=top, 1=bottom).
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

#[derive(Resource)]
pub struct ObjectEditorState {
    pub open: bool,
    pub search_text: String,
    pub objects: Vec<ObjectEntry>,
    pub selected: Option<usize>,
    pub dirty: bool,
    pub scanned: bool,
}

impl Default for ObjectEditorState {
    fn default() -> Self {
        Self {
            open: false,
            search_text: String::new(),
            objects: Vec::new(),
            selected: None,
            dirty: false,
            scanned: false,
        }
    }
}

pub struct ObjectEntry {
    pub tileset: String,
    pub key: String,
    pub dir: PathBuf,
    pub properties: ObjectProperties,
    pub has_mesh: bool,
    pub has_shadow: bool,
    pub has_depth: bool,
    pub sprite_texture: Option<egui::TextureId>,
    pub sprite_handle: Option<Handle<Image>>,
}

/// Marker for ALL lights spawned from object properties (startup + editor preview).
/// Stores billboard-local offset so the light follows the billboard's tilt.
#[derive(Component)]
pub struct ObjectSpriteLight {
    pub sprite_key: String,
    /// Horizontal offset on billboard face as fraction (0=left, 1=right).
    pub offset_x: f32,
    /// Vertical offset on billboard face as fraction (0=bottom, 1=top).
    pub offset_y: f32,
}

/// Additional marker for lights managed by the live preview system.
/// Removed when editor closes, making the lights permanent.
#[derive(Component)]
pub struct ObjectEditorLight;

// ── Systems ───────────────────────────────────────────────────────────────

pub fn toggle_object_editor(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<ObjectEditorState>,
) {
    if keyboard.just_pressed(KeyCode::F7) {
        state.open = !state.open;
    }
}

pub fn scan_objects(mut state: ResMut<ObjectEditorState>) {
    if !state.open || state.scanned {
        return;
    }
    state.scanned = true;

    let base = PathBuf::from("assets/objects");
    let Ok(tilesets) = std::fs::read_dir(&base) else {
        return;
    };

    let mut objects = Vec::new();
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
            // Only consider folders that contain sprite.png
            if !key_path.join("sprite.png").exists() {
                continue;
            }

            let key = key_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            // Load properties.json if it exists
            let props_path = key_path.join("properties.json");
            let properties = if props_path.exists() {
                std::fs::read_to_string(&props_path)
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default()
            } else {
                // Try to read type from type.txt
                let obj_type = key_path
                    .join("type.txt")
                    .pipe_read_type();
                let blend_height = key_path
                    .join("blend_height.txt")
                    .pipe_read_f32();
                ObjectProperties {
                    obj_type,
                    blend_height,
                    ..Default::default()
                }
            };

            let has_mesh = key_path.join("mesh.glb").exists();
            let has_shadow = key_path.join("shadow.glb").exists();
            let has_depth = key_path.join("sprite_depth.png").exists();

            objects.push(ObjectEntry {
                tileset: tileset.clone(),
                key,
                dir: key_path,
                properties,
                has_mesh,
                has_shadow,
                has_depth,
                sprite_texture: None,
                sprite_handle: None,
            });
        }
    }

    objects.sort_by(|a, b| {
        a.tileset
            .cmp(&b.tileset)
            .then(a.key.cmp(&b.key))
    });

    state.objects = objects;
    info!("Object editor: scanned {} objects", state.objects.len());
}

trait PathReadHelpers {
    fn pipe_read_type(&self) -> String;
    fn pipe_read_f32(&self) -> f32;
}

impl PathReadHelpers for PathBuf {
    fn pipe_read_type(&self) -> String {
        std::fs::read_to_string(self)
            .ok()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| s == "organic" || s == "structural")
            .unwrap_or_else(default_type)
    }

    fn pipe_read_f32(&self) -> f32 {
        std::fs::read_to_string(self)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0.0)
    }
}

// ── Main UI system ────────────────────────────────────────────────────────

pub fn object_editor_ui(
    mut contexts: EguiContexts,
    mut state: ResMut<ObjectEditorState>,
    asset_server: Res<AssetServer>,
    images: Res<Assets<Image>>,
) {
    if !state.open {
        return;
    }

    // Register sprite textures for visible objects that don't have one yet.
    // We do this before borrowing ctx so we can use contexts.add_image().
    let objects_needing_textures: Vec<usize> = state
        .objects
        .iter()
        .enumerate()
        .filter(|(_, obj)| obj.sprite_texture.is_none() && obj.sprite_handle.is_none())
        .map(|(i, _)| i)
        .collect();

    for idx in objects_needing_textures {
        let obj = &mut state.objects[idx];
        let sprite_path = obj.dir.join("sprite.png");
        let asset_path = sprite_path
            .strip_prefix("assets/")
            .or_else(|_| sprite_path.strip_prefix("assets"))
            .unwrap_or(&sprite_path)
            .to_path_buf();
        let handle: Handle<Image> = asset_server.load(asset_path);
        let id = contexts.add_image(bevy_egui::EguiTextureHandle::Strong(handle.clone()));
        obj.sprite_handle = Some(handle);
        obj.sprite_texture = Some(id);
    }

    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let state = &mut *state;

    egui::Window::new("Object Editor")
        .default_width(450.0)
        .default_height(600.0)
        .show(ctx, |ui| {
            // ── Search bar ─────────────────────────────────────────
            ui.horizontal(|ui| {
                ui.label("Search:");
                ui.text_edit_singleline(&mut state.search_text);
                if ui.small_button("x").clicked() {
                    state.search_text.clear();
                }
            });

            let search_lower = state.search_text.to_lowercase();

            // ── Object grid ────────────────────────────────────────
            let filtered: Vec<usize> = state
                .objects
                .iter()
                .enumerate()
                .filter(|(_, obj)| {
                    if search_lower.is_empty() {
                        return true;
                    }
                    obj.tileset.to_lowercase().contains(&search_lower)
                        || obj.key.to_lowercase().contains(&search_lower)
                        || obj.properties.obj_type.to_lowercase().contains(&search_lower)
                        || obj.properties.keywords.iter().any(|k| {
                            k.to_lowercase().contains(&search_lower)
                        })
                })
                .map(|(i, _)| i)
                .collect();

            ui.label(format!(
                "{} / {} objects",
                filtered.len(),
                state.objects.len()
            ));

            let thumb_size = 48.0;
            let grid_height = 200.0_f32.min(
                ((filtered.len() as f32 / 8.0).ceil()) * (thumb_size + 4.0),
            ).max(60.0);

            egui::ScrollArea::vertical()
                .max_height(grid_height)
                .id_salt("obj_grid")
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for &obj_idx in &filtered {
                            let obj = &state.objects[obj_idx];
                            let is_selected = state.selected == Some(obj_idx);

                            let (rect, resp) = ui.allocate_exact_size(
                                egui::vec2(thumb_size, thumb_size),
                                egui::Sense::click(),
                            );

                            // Draw thumbnail
                            if let Some(tex_id) = obj.sprite_texture {
                                let tint = if is_selected {
                                    egui::Color32::WHITE
                                } else {
                                    egui::Color32::from_gray(180)
                                };
                                ui.painter().image(
                                    tex_id,
                                    rect,
                                    egui::Rect::from_min_max(
                                        egui::pos2(0.0, 0.0),
                                        egui::pos2(1.0, 1.0),
                                    ),
                                    tint,
                                );
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

                            // Type badge overlay
                            let badge_color = if obj.properties.obj_type == "structural" {
                                egui::Color32::from_rgb(80, 140, 255)
                            } else {
                                egui::Color32::from_rgb(80, 200, 80)
                            };
                            let badge_text = if obj.properties.obj_type == "structural" {
                                "S"
                            } else {
                                "O"
                            };
                            let badge_pos = egui::pos2(rect.max.x - 2.0, rect.min.y + 2.0);
                            ui.painter().text(
                                badge_pos,
                                egui::Align2::RIGHT_TOP,
                                badge_text,
                                egui::FontId::proportional(10.0),
                                badge_color,
                            );

                            // Tooltip + click
                            let resp = resp.on_hover_text(format!("{} / {}", obj.tileset, obj.key));
                            if resp.clicked() {
                                state.selected = Some(obj_idx);
                                state.dirty = false;
                            }
                        }
                    });
                });

            ui.separator();

            // ── Selected object details ────────────────────────────
            let Some(sel_idx) = state.selected else {
                ui.label("Select an object to edit.");
                return;
            };
            if sel_idx >= state.objects.len() {
                state.selected = None;
                return;
            }

            let obj = &mut state.objects[sel_idx];
            ui.heading(format!("{} / {}", obj.tileset, obj.key));

            // Type combo
            ui.horizontal(|ui| {
                ui.label("Type:");
                let prev = obj.properties.obj_type.clone();
                egui::ComboBox::from_id_salt("obj_type")
                    .selected_text(&obj.properties.obj_type)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut obj.properties.obj_type,
                            "organic".to_string(),
                            "organic",
                        );
                        ui.selectable_value(
                            &mut obj.properties.obj_type,
                            "structural".to_string(),
                            "structural",
                        );
                    });
                if obj.properties.obj_type != prev {
                    state.dirty = true;
                }
            });

            // Blend height
            ui.horizontal(|ui| {
                ui.label("Blend height:");
                if ui
                    .add(
                        egui::Slider::new(&mut obj.properties.blend_height, 0.0..=48.0)
                            .suffix(" px"),
                    )
                    .changed()
                {
                    state.dirty = true;
                }
            });

            // Shadow offset
            ui.horizontal(|ui| {
                ui.label("Shadow offset X:");
                if ui
                    .add(egui::DragValue::new(&mut obj.properties.shadow_offset_x)
                        .range(0.0..=1.0).speed(0.01))
                    .changed()
                {
                    state.dirty = true;
                }
                ui.label("Y:");
                if ui
                    .add(egui::DragValue::new(&mut obj.properties.shadow_offset_y)
                        .range(0.0..=1.0).speed(0.01))
                    .changed()
                {
                    state.dirty = true;
                }
            });

            ui.separator();

            // ── Lights ─────────────────────────────────────────────
            ui.heading("Lights");

            // ── Light position preview ────────────────────────────
            if let Some(tex_id) = obj.sprite_texture {
                // Get actual image dimensions for aspect ratio
                let (img_w, img_h) = obj
                    .sprite_handle
                    .as_ref()
                    .and_then(|h| images.get(h))
                    .map(|img| {
                        let size = img.size();
                        (size.x as f32, size.y as f32)
                    })
                    .unwrap_or((1.0, 1.0));

                let max_dim = 200.0_f32;
                let scale = (max_dim / img_w).min(max_dim / img_h);
                let preview_w = img_w * scale;
                let preview_h = img_h * scale;

                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(preview_w, preview_h),
                    egui::Sense::hover(),
                );

                // Draw the full-resolution sprite
                ui.painter().image(
                    tex_id,
                    rect,
                    egui::Rect::from_min_max(
                        egui::pos2(0.0, 0.0),
                        egui::pos2(1.0, 1.0),
                    ),
                    egui::Color32::WHITE,
                );

                // Draw a subtle border
                ui.painter().rect_stroke(
                    rect,
                    1.0,
                    egui::Stroke::new(1.0, egui::Color32::from_gray(80)),
                    egui::StrokeKind::Outside,
                );

                // Draw shadow offset crosshair
                let shadow_pos = egui::pos2(
                    rect.min.x + obj.properties.shadow_offset_x * preview_w,
                    rect.min.y + obj.properties.shadow_offset_y * preview_h,
                );
                // Horizontal line
                ui.painter().line_segment(
                    [egui::pos2(rect.min.x, shadow_pos.y), egui::pos2(rect.max.x, shadow_pos.y)],
                    egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255, 100, 100, 120)),
                );
                // Vertical line
                ui.painter().line_segment(
                    [egui::pos2(shadow_pos.x, rect.min.y), egui::pos2(shadow_pos.x, rect.max.y)],
                    egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255, 100, 100, 120)),
                );
                // Center dot
                ui.painter().circle_filled(
                    shadow_pos, 3.0,
                    egui::Color32::from_rgb(255, 80, 80),
                );
                // Label
                ui.painter().text(
                    egui::pos2(shadow_pos.x + 5.0, shadow_pos.y - 5.0),
                    egui::Align2::LEFT_BOTTOM,
                    "shadow",
                    egui::FontId::proportional(9.0),
                    egui::Color32::from_rgb(255, 120, 120),
                );

                // Get hover position to detect which light is hovered
                let hover_pos = resp.hover_pos();

                // Draw each light's position on the preview
                for (li, light) in obj.properties.lights.iter().enumerate() {
                    let light_pos = egui::pos2(
                        rect.min.x + light.offset_x * preview_w,
                        rect.min.y + light.offset_y * preview_h,
                    );

                    let light_color = egui::Color32::from_rgb(
                        (light.color[0] * 255.0) as u8,
                        (light.color[1] * 255.0) as u8,
                        (light.color[2] * 255.0) as u8,
                    );

                    // Check if this light dot is hovered (within 8px)
                    let is_hovered = hover_pos.is_some_and(|hp| {
                        let dx = hp.x - light_pos.x;
                        let dy = hp.y - light_pos.y;
                        (dx * dx + dy * dy).sqrt() < 12.0
                    });

                    // Draw radius circle (faint)
                    // Scale radius relative to preview: radius is in world pixels,
                    // so scale it the same way as the sprite
                    let radius_px = light.radius * scale;
                    let radius_alpha = if is_hovered { 50 } else { 25 };
                    ui.painter().circle_stroke(
                        light_pos,
                        radius_px,
                        egui::Stroke::new(
                            1.0,
                            egui::Color32::from_rgba_unmultiplied(
                                (light.color[0] * 255.0) as u8,
                                (light.color[1] * 255.0) as u8,
                                (light.color[2] * 255.0) as u8,
                                radius_alpha,
                            ),
                        ),
                    );
                    // Fill the radius area very faintly
                    let fill_alpha = if is_hovered { 30 } else { 12 };
                    ui.painter().circle_filled(
                        light_pos,
                        radius_px,
                        egui::Color32::from_rgba_unmultiplied(
                            (light.color[0] * 255.0) as u8,
                            (light.color[1] * 255.0) as u8,
                            (light.color[2] * 255.0) as u8,
                            fill_alpha,
                        ),
                    );

                    // Draw the filled dot
                    let dot_radius = if is_hovered { 6.0 } else { 4.0 };
                    ui.painter().circle_filled(light_pos, dot_radius, light_color);

                    // Outline for visibility
                    let outline_color = if is_hovered {
                        egui::Color32::WHITE
                    } else {
                        egui::Color32::BLACK
                    };
                    ui.painter().circle_stroke(
                        light_pos,
                        dot_radius,
                        egui::Stroke::new(1.5, outline_color),
                    );

                    // Show label on hover
                    if is_hovered {
                        ui.painter().text(
                            egui::pos2(light_pos.x, light_pos.y - dot_radius - 4.0),
                            egui::Align2::CENTER_BOTTOM,
                            format!("Light {li}"),
                            egui::FontId::proportional(11.0),
                            egui::Color32::WHITE,
                        );
                    }
                }

                ui.add_space(4.0);
            }

            let mut remove_idx: Option<usize> = None;
            let num_lights = obj.properties.lights.len();
            for li in 0..num_lights {
                let light = &mut obj.properties.lights[li];
                ui.push_id(format!("light_{li}"), |ui| {
                    ui.group(|ui| {
                        // Color + intensity + radius
                        ui.horizontal(|ui| {
                            egui::color_picker::color_edit_button_rgb(ui, &mut light.color);
                            if ui
                                .add(
                                    egui::DragValue::new(&mut light.intensity)
                                        .range(0.1..=10.0)
                                        .speed(0.05)
                                        .prefix("I: "),
                                )
                                .changed()
                            {
                                state.dirty = true;
                            }
                            if ui
                                .add(
                                    egui::DragValue::new(&mut light.radius)
                                        .range(1.0..=500.0)
                                        .speed(1.0)
                                        .prefix("R: "),
                                )
                                .changed()
                            {
                                state.dirty = true;
                            }
                        });

                        // Shape combo
                        ui.horizontal(|ui| {
                            ui.label("Shape:");
                            let prev_shape = light.shape.clone();
                            egui::ComboBox::from_id_salt(format!("shape_{li}"))
                                .width(80.0)
                                .selected_text(&light.shape)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut light.shape,
                                        "point".to_string(),
                                        "point",
                                    );
                                    ui.selectable_value(
                                        &mut light.shape,
                                        "cone".to_string(),
                                        "cone",
                                    );
                                    ui.selectable_value(
                                        &mut light.shape,
                                        "line".to_string(),
                                        "line",
                                    );
                                    ui.selectable_value(
                                        &mut light.shape,
                                        "capsule".to_string(),
                                        "capsule",
                                    );
                                });
                            if light.shape != prev_shape {
                                state.dirty = true;
                            }
                        });

                        // Offset
                        ui.horizontal(|ui| {
                            ui.label("Offset:");
                            if ui
                                .add(
                                    egui::DragValue::new(&mut light.offset_x)
                                        .range(0.0..=1.0)
                                        .speed(0.01)
                                        .prefix("X: "),
                                )
                                .changed()
                            {
                                state.dirty = true;
                            }
                            if ui
                                .add(
                                    egui::DragValue::new(&mut light.offset_y)
                                        .range(0.0..=1.0)
                                        .speed(0.01)
                                        .prefix("Y: "),
                                )
                                .changed()
                            {
                                state.dirty = true;
                            }
                        });

                        // Pulse / Flicker / Remove
                        ui.horizontal(|ui| {
                            if ui.checkbox(&mut light.pulse, "Pulse").changed() {
                                state.dirty = true;
                            }
                            if ui.checkbox(&mut light.flicker, "Flicker").changed() {
                                state.dirty = true;
                            }
                            if ui
                                .small_button("x")
                                .on_hover_text("Remove light")
                                .clicked()
                            {
                                remove_idx = Some(li);
                            }
                        });
                    });
                });
            }

            if let Some(ri) = remove_idx {
                obj.properties.lights.remove(ri);
                state.dirty = true;
            }

            if ui.button("+ Add Light").clicked() {
                obj.properties.lights.push(ObjectLight::default());
                state.dirty = true;
            }

            ui.separator();

            // ── Info ───────────────────────────────────────────────
            ui.heading("Info");
            let check = |b: bool| if b { "Y" } else { "N" };
            ui.label(format!(
                "mesh.glb: {}  |  shadow.glb: {}  |  depth map: {}",
                check(obj.has_mesh),
                check(obj.has_shadow),
                check(obj.has_depth),
            ));

            // Keywords (read-only chips)
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

            ui.separator();

            // ── Save ───────────────────────────────────────────────
            let save_label = if state.dirty { "Save *" } else { "Save" };
            if ui.button(save_label).clicked() {
                save_object_properties(obj);
                state.dirty = false;
            }
        });
}

// ── Save ──────────────────────────────────────────────────────────────────

fn save_object_properties(obj: &ObjectEntry) {
    let props_path = obj.dir.join("properties.json");
    match serde_json::to_string_pretty(&obj.properties) {
        Ok(json) => match std::fs::write(&props_path, &json) {
            Ok(()) => {
                info!("Saved properties to {}", props_path.display());
                // Also sync type.txt for tools that read it separately
                let type_path = obj.dir.join("type.txt");
                let _ = std::fs::write(&type_path, &obj.properties.obj_type);
            }
            Err(e) => error!("Failed to save {}: {e}", props_path.display()),
        },
        Err(e) => error!("Failed to serialize properties: {e}"),
    }
}

// ── Live preview system ──────────────────────────────────────────────────

/// Applies object editor changes in real-time to matching billboard entities.
/// - Spawns/updates/despawns lights for the selected object
/// - Updates shadow mesh offset
pub fn live_preview_system(
    mut commands: Commands,
    state: Res<ObjectEditorState>,
    mut prev_selected: Local<Option<String>>,
    mut queries: ParamSet<(
        Query<(Entity, &crate::camera::combat::BillboardSpriteKey, &Transform, &crate::camera::combat::BillboardHeight), With<crate::camera::combat::Billboard>>,
        Query<(Entity, &mut ObjectSpriteLight, &mut Transform, &mut crate::lighting::components::LightSource)>,
    )>,
) {
    // Track selection changes
    let current_key = state.open
        .then(|| state.selected)
        .flatten()
        .and_then(|idx| state.objects.get(idx))
        .map(|obj| obj.key.clone());

    // If selection changed, clear prev so we rebuild
    if current_key != *prev_selected {
        *prev_selected = current_key.clone();
    }

    if !state.open || current_key.is_none() {
        return;
    }

    let selected_idx = state.selected.unwrap();
    let obj = &state.objects[selected_idx];
    let sprite_key = &obj.key;

    // ── Read billboard transforms ──
    let mut billboard_data: Vec<(Vec3, Quat, f32)> = Vec::new();
    {
        let bb_query = queries.p0();
        for (_bb_entity, key, bb_tf, bb_height) in &bb_query {
            if key.0 == *sprite_key {
                billboard_data.push((bb_tf.translation, bb_tf.rotation, bb_height.height));
            }
        }
    }

    // ── Update existing lights in-place, or spawn/despawn as needed ──
    {
        let light_query = queries.p1();
        let existing_count = light_query.iter()
            .filter(|(_, m, _, _)| m.sprite_key == *sprite_key)
            .count();
        let desired_count = obj.properties.lights.len() * billboard_data.len();

        if existing_count != desired_count {
            // Light count changed — despawn all and respawn
            let to_despawn: Vec<Entity> = light_query.iter()
                .filter(|(_, m, _, _)| m.sprite_key == *sprite_key)
                .map(|(e, _, _, _)| e)
                .collect();
            drop(light_query); // release borrow before commands
            for entity in to_despawn {
                commands.entity(entity).despawn();
            }

            for &(bb_pos, bb_rot, bb_h) in &billboard_data {
                for light_def in &obj.properties.lights {
                    use crate::lighting::components::*;

                    let shape = match light_def.shape.as_str() {
                        "cone" => LightShape::Cone { direction: 0.0, angle: std::f32::consts::FRAC_PI_2 },
                        "line" => LightShape::Line { end_offset: Vec2::new(48.0, 0.0) },
                        "capsule" => LightShape::Capsule { direction: 0.0, half_length: 24.0 },
                        _ => LightShape::Point,
                    };

                    let light_pos = light_world_pos(
                        bb_pos, bb_rot, bb_h,
                        light_def.offset_x, light_def.offset_y,
                    );

                    commands.spawn((
                        Transform::from_translation(light_pos),
                        LightSource {
                            color: Color::linear_rgb(light_def.color[0], light_def.color[1], light_def.color[2]),
                            base_intensity: light_def.intensity,
                            intensity: light_def.intensity,
                            inner_radius: light_def.radius * 0.3,
                            outer_radius: light_def.radius,
                            shape,
                            pulse: if light_def.pulse { Some(PulseConfig::default()) } else { None },
                            flicker: if light_def.flicker { Some(FlickerConfig::default()) } else { None },
                            anim_seed: rand::random::<f32>() * 100.0,
                            ..default()
                        },
                        ObjectSpriteLight {
                            sprite_key: sprite_key.clone(),
                            offset_x: light_def.offset_x,
                            offset_y: light_def.offset_y,
                        },
                    ));
                }
            }
        } else {
            drop(light_query); // release immutable borrow
            // Same count — update existing lights in-place
            let mut light_query_mut = queries.p1();
            let mut light_iter: Vec<_> = light_query_mut.iter_mut()
                .filter(|(_, m, _, _)| m.sprite_key == *sprite_key)
                .collect();

            let mut li = 0;
            for &(bb_pos, bb_rot, bb_h) in &billboard_data {
                for light_def in &obj.properties.lights {
                    if li >= light_iter.len() { break; }
                    let (_, ref mut osl, ref mut tf, ref mut ls) = light_iter[li];

                    osl.offset_x = light_def.offset_x;
                    osl.offset_y = light_def.offset_y;

                    tf.translation = light_world_pos(
                        bb_pos, bb_rot, bb_h,
                        light_def.offset_x, light_def.offset_y,
                    );

                    ls.color = Color::linear_rgb(light_def.color[0], light_def.color[1], light_def.color[2]);
                    ls.base_intensity = light_def.intensity;
                    ls.intensity = light_def.intensity;
                    ls.inner_radius = light_def.radius * 0.3;
                    ls.outer_radius = light_def.radius;

                    li += 1;
                }
            }
        }
    }
}

/// Computes the world position of a light on a billboard face.
/// The offset is in billboard-local space (XY plane of the quad), then
/// rotated by the billboard's tilt so the light physically sits on the surface.
fn light_world_pos(bb_pos: Vec3, bb_rot: Quat, bb_h: f32, offset_x: f32, offset_y: f32) -> Vec3 {
    // Billboard local space: X = horizontal, +Y = up on the sprite, Z = 0 (on the face).
    // offset_y 0 = bottom of sprite, 1 = top.
    let local = Vec3::new(
        (offset_x - 0.5) * bb_h,
        offset_y * bb_h,
        0.0,
    );
    bb_pos + bb_rot * local
}

/// Repositions object lights each frame so they follow billboard tilt.
/// Runs after billboard_system to use the current frame's rotation.
pub fn update_object_light_positions(
    billboards: Query<
        (&crate::camera::combat::BillboardSpriteKey, &Transform, &crate::camera::combat::BillboardHeight),
        With<crate::camera::combat::Billboard>,
    >,
    mut lights: Query<(&ObjectSpriteLight, &mut Transform), Without<crate::camera::combat::Billboard>>,
) {
    let mut bb_map: std::collections::HashMap<&str, Vec<(Vec3, Quat, f32)>> =
        std::collections::HashMap::new();
    for (key, tf, bb_h) in &billboards {
        bb_map.entry(key.0.as_str())
            .or_default()
            .push((tf.translation, tf.rotation, bb_h.height));
    }

    for (osl, mut tf) in &mut lights {
        let Some(bbs) = bb_map.get(osl.sprite_key.as_str()) else {
            continue;
        };
        let &(bb_pos, bb_rot, bb_h) = if bbs.len() == 1 {
            &bbs[0]
        } else {
            bbs.iter()
                .min_by(|a, b| {
                    let da = a.0.distance_squared(tf.translation);
                    let db = b.0.distance_squared(tf.translation);
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                })
                .unwrap()
        };

        tf.translation = light_world_pos(bb_pos, bb_rot, bb_h, osl.offset_x, osl.offset_y);
    }
}

/// Draws wireframe gizmo spheres for each light belonging to the selected tile object.
pub fn draw_object_light_gizmos(
    state: Res<ObjectEditorState>,
    lights: Query<(&ObjectSpriteLight, &Transform, &crate::lighting::components::LightSource)>,
    mut gizmos: Gizmos,
) {
    if !state.open {
        return;
    }
    let Some(idx) = state.selected else { return };
    let Some(obj) = state.objects.get(idx) else { return };
    let sprite_key = &obj.key;

    for (osl, tf, ls) in &lights {
        if osl.sprite_key != *sprite_key {
            continue;
        }

        let color = ls.color.with_alpha(0.6);

        // Inner radius: solid-ish sphere
        gizmos.sphere(
            Isometry3d::from_translation(tf.translation),
            ls.inner_radius,
            color,
        );
        // Outer radius: faint wireframe showing falloff boundary
        gizmos.sphere(
            Isometry3d::from_translation(tf.translation),
            ls.outer_radius,
            ls.color.with_alpha(0.15),
        );
    }
}
