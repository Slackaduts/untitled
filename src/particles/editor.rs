use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::{EguiContexts, egui};

use super::definitions::{
    EmissionDirection, EmissionShape, ParticleBlend, ParticleDef, ParticleLightDef,
    ParticleRegistry,
};
use super::emitter::ParticleEmitter;
use crate::camera::CombatCamera3d;

// ── State ────────────────────────────────────────────────────────────────────

#[derive(Resource)]
pub struct ParticleEditorState {
    pub open: bool,
    /// Currently selected definition ID.
    pub selected_def: Option<String>,
    /// Working copy being edited (writes back to registry on save).
    pub editing_def: Option<ParticleDef>,
    /// Buffer for the ID text field (persists across frames, commits on focus loss).
    pub editing_id: String,
    pub dirty: bool,
    /// Set alongside dirty; consumed by preview system after one rebuild cycle.
    pub preview_needs_rebuild: bool,
    /// Hash of the last editing_def applied to the preview. Used to detect
    /// actual property changes vs persistent `dirty` flag.
    pub preview_def_hash: u64,
    // Emitter placement
    pub placer_active: bool,
    pub placer_def_id: String,
    pub placer_rate: f32,
    pub placer_height: f32,
    // Preview emitter
    pub preview_emitter: Option<Entity>,
    // World interaction
    pub dragging: Option<Entity>,
    pub hovered_emitter: Option<Entity>,
}

impl Default for ParticleEditorState {
    fn default() -> Self {
        Self {
            open: false,
            selected_def: None,
            editing_def: None,
            editing_id: String::new(),
            dirty: false,
            preview_needs_rebuild: false,
            preview_def_hash: 0,
            placer_active: false,
            placer_def_id: String::new(),
            placer_rate: 10.0,
            placer_height: 5.0,
            preview_emitter: None,
            dragging: None,
            hovered_emitter: None,
        }
    }
}

/// Marker for emitters placed by the particle editor.
#[derive(Component)]
pub struct DebugParticleEmitter;

/// Marker for the live preview emitter.
#[derive(Component)]
pub struct PreviewParticleEmitter;

// ── Toggle ───────────────────────────────────────────────────────────────────

pub fn toggle_particle_editor(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<ParticleEditorState>,
) {
    if keyboard.just_pressed(KeyCode::F5) {
        state.open = !state.open;
    }
}

// ── Main UI ──────────────────────────────────────────────────────────────────

pub fn particle_editor_ui(
    mut contexts: EguiContexts,
    mut state: ResMut<ParticleEditorState>,
    mut registry: ResMut<ParticleRegistry>,
    debug_emitters: Query<(Entity, &ParticleEmitter, &Transform), With<DebugParticleEmitter>>,
    mut commands: Commands,
) {
    if !state.open {
        return;
    }

    let state = state.as_mut();
    state.hovered_emitter = None;

    let Ok(ctx) = contexts.ctx_mut() else { return };
    egui::Window::new("Particles")
        .default_width(340.0)
        .show(ctx, |ui| {
            // ── Definition Library ─────────────────────────────────
            ui.heading("Definitions");

            let def_ids: Vec<String> = registry.defs.keys().cloned().collect();

            egui::ScrollArea::vertical()
                .max_height(120.0)
                .id_salt("def_list")
                .show(ui, |ui| {
                    for id in &def_ids {
                        let selected = state.selected_def.as_ref() == Some(id);
                        if ui.selectable_label(selected, id).clicked() {
                            state.selected_def = Some(id.clone());
                            state.editing_def = registry.defs.get(id).cloned();
                            state.editing_id = id.clone();
                            state.dirty = false;
                            state.preview_needs_rebuild = true;
                        }
                    }
                });

            ui.horizontal(|ui| {
                if ui.button("+ New").clicked() {
                    let mut id = "new_particle".to_string();
                    let mut counter = 1;
                    while registry.defs.contains_key(&id) {
                        id = format!("new_particle_{counter}");
                        counter += 1;
                    }
                    let def = ParticleDef {
                        id: id.clone(),
                        ..Default::default()
                    };
                    registry.defs.insert(id.clone(), def.clone());
                    state.selected_def = Some(id.clone());
                    state.editing_id = id;
                    state.editing_def = Some(def);
                    state.dirty = true;
                }

                if state.selected_def.is_some() {
                    if ui.button("Delete").clicked() {
                        if let Some(id) = state.selected_def.take() {
                            registry.defs.remove(&id);
                            state.editing_def = None;
                            // Delete the JSON file too.
                            let path =
                                std::path::PathBuf::from(format!("assets/particles/{id}.json"));
                            let _ = std::fs::remove_file(&path);
                        }
                    }
                }
            });

            ui.separator();

            // ── Definition Editor ──────────────────────────────────
            if let Some(def) = state.editing_def.as_mut() {
                ui.heading(format!("Edit: {}", def.id));

                // ID — edits a persistent buffer, commits on Enter / focus loss
                ui.horizontal(|ui| {
                    ui.label("ID:");
                    let resp = ui.text_edit_singleline(&mut state.editing_id);
                    if resp.lost_focus() && state.editing_id != def.id {
                        def.id = state.editing_id.clone();
                        state.dirty = true;
                    }
                });

                // ── Lifetime ───────────────────────────────────────
                if ui
                    .collapsing("Lifetime", |ui| {
                        let changed = ui
                            .horizontal(|ui| {
                                let a = ui
                                    .add(
                                        egui::DragValue::new(&mut def.lifetime.0)
                                            .range(0.01..=30.0)
                                            .speed(0.05)
                                            .prefix("min: "),
                                    )
                                    .changed();
                                let b = ui
                                    .add(
                                        egui::DragValue::new(&mut def.lifetime.1)
                                            .range(0.01..=30.0)
                                            .speed(0.05)
                                            .prefix("max: "),
                                    )
                                    .changed();
                                a || b
                            })
                            .inner;
                        if changed {
                            state.dirty = true;
                        }
                    })
                    .fully_open()
                {}

                // ── Motion ─────────────────────────────────────────
                ui.collapsing("Motion", |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .add(
                                egui::DragValue::new(&mut def.speed_range.0)
                                    .range(0.0..=500.0)
                                    .speed(0.5)
                                    .prefix("speed min: "),
                            )
                            .changed()
                        {
                            state.dirty = true;
                        }
                        if ui
                            .add(
                                egui::DragValue::new(&mut def.speed_range.1)
                                    .range(0.0..=500.0)
                                    .speed(0.5)
                                    .prefix("max: "),
                            )
                            .changed()
                        {
                            state.dirty = true;
                        }
                    });

                    if ui
                        .add(
                            egui::Slider::new(&mut def.gravity, 0.0..=200.0).text("Gravity"),
                        )
                        .changed()
                    {
                        state.dirty = true;
                    }

                    if ui
                        .add(
                            egui::Slider::new(&mut def.drag, 0.0..=1.0).text("Drag"),
                        )
                        .changed()
                    {
                        state.dirty = true;
                    }

                    emission_direction_ui(ui, &mut def.direction, &mut state.dirty);
                });

                // ── Appearance ──────────────────────────────────────
                ui.collapsing("Appearance", |ui| {
                    // ── Color gradient ──
                    ui.label("Color Gradient");
                    // Ensure we have at least 2 stops.
                    if def.color_stops.is_empty() {
                        def.migrate_legacy_fields();
                    }
                    let mut color_remove: Option<usize> = None;
                    let color_count = def.color_stops.len();
                    for ci in 0..color_count {
                        let stop = &mut def.color_stops[ci];
                        ui.push_id(format!("cstop_{ci}"), |ui| {
                            ui.horizontal(|ui| {
                                if ui.add(egui::DragValue::new(&mut stop.t).range(0.0..=1.0).speed(0.01).prefix("t: ")).changed() {
                                    state.dirty = true;
                                }
                                let mut rgb = [stop.color[0], stop.color[1], stop.color[2]];
                                if egui::color_picker::color_edit_button_rgb(ui, &mut rgb).changed() {
                                    stop.color[0] = rgb[0];
                                    stop.color[1] = rgb[1];
                                    stop.color[2] = rgb[2];
                                    state.dirty = true;
                                }
                                if ui.add(egui::DragValue::new(&mut stop.color[3]).range(0.0..=1.0).speed(0.01).prefix("A: ")).changed() {
                                    state.dirty = true;
                                }
                                if color_count > 2 {
                                    if ui.small_button("x").clicked() {
                                        color_remove = Some(ci);
                                    }
                                }
                            });
                        });
                    }
                    if let Some(ri) = color_remove {
                        def.color_stops.remove(ri);
                        state.dirty = true;
                    }
                    if ui.small_button("+ Color Stop").clicked() {
                        def.color_stops.push(super::definitions::ColorStop {
                            t: 0.5,
                            color: [1.0, 1.0, 1.0, 1.0],
                        });
                        def.color_stops.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
                        state.dirty = true;
                    }
                    // Keep sorted after edits.
                    def.color_stops.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());

                    ui.add_space(4.0);

                    // ── Size gradient ──
                    ui.label("Size Gradient");
                    if def.size_stops.is_empty() {
                        def.migrate_legacy_fields();
                    }
                    let mut size_remove: Option<usize> = None;
                    let size_count = def.size_stops.len();
                    for si in 0..size_count {
                        let stop = &mut def.size_stops[si];
                        ui.push_id(format!("sstop_{si}"), |ui| {
                            ui.horizontal(|ui| {
                                if ui.add(egui::DragValue::new(&mut stop.t).range(0.0..=1.0).speed(0.01).prefix("t: ")).changed() {
                                    state.dirty = true;
                                }
                                if ui.add(egui::DragValue::new(&mut stop.size).range(0.1..=100.0).speed(0.1).prefix("size: ")).changed() {
                                    state.dirty = true;
                                }
                                if size_count > 2 {
                                    if ui.small_button("x").clicked() {
                                        size_remove = Some(si);
                                    }
                                }
                            });
                        });
                    }
                    if let Some(ri) = size_remove {
                        def.size_stops.remove(ri);
                        state.dirty = true;
                    }
                    if ui.small_button("+ Size Stop").clicked() {
                        def.size_stops.push(super::definitions::SizeStop {
                            t: 0.5,
                            size: 2.0,
                        });
                        def.size_stops.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
                        state.dirty = true;
                    }
                    def.size_stops.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());

                    ui.add_space(4.0);

                    // Blend mode
                    ui.horizontal(|ui| {
                        ui.label("Blend");
                        let mut kind: u8 = match def.blend_mode {
                            ParticleBlend::Additive => 0,
                            ParticleBlend::Alpha => 1,
                        };
                        egui::ComboBox::from_id_salt("blend_mode")
                            .width(80.0)
                            .selected_text(match kind {
                                1 => "Alpha",
                                _ => "Additive",
                            })
                            .show_ui(ui, |ui| {
                                if ui.selectable_value(&mut kind, 0, "Additive").changed() {
                                    state.dirty = true;
                                }
                                if ui.selectable_value(&mut kind, 1, "Alpha").changed() {
                                    state.dirty = true;
                                }
                            });
                        def.blend_mode = match kind {
                            1 => ParticleBlend::Alpha,
                            _ => ParticleBlend::Additive,
                        };
                    });
                    // Particle shape
                    ui.horizontal(|ui| {
                        ui.label("Shape:");
                        use crate::particles::definitions::ParticleShape;
                        let shape_name = |s: &ParticleShape| match s {
                            ParticleShape::Quad => "Quad",
                            ParticleShape::Circle => "Circle",
                            ParticleShape::Triangle => "Triangle",
                            ParticleShape::Diamond => "Diamond",
                            ParticleShape::Hexagon => "Hexagon",
                            ParticleShape::Star => "Star",
                        };
                        egui::ComboBox::from_id_salt("particle_shape")
                            .width(100.0)
                            .selected_text(shape_name(&def.shape))
                            .show_ui(ui, |ui| {
                                for shape in [
                                    ParticleShape::Quad,
                                    ParticleShape::Circle,
                                    ParticleShape::Triangle,
                                    ParticleShape::Diamond,
                                    ParticleShape::Hexagon,
                                    ParticleShape::Star,
                                ] {
                                    if ui.selectable_value(&mut def.shape, shape, shape_name(&shape)).changed() {
                                        state.dirty = true;
                                    }
                                }
                            });
                    });

                    // Sprite texture
                    ui.horizontal(|ui| {
                        ui.label("Sprite:");
                        let selected_text = def.texture.as_deref().unwrap_or("(none — quad)");
                        egui::ComboBox::from_id_salt("particle_sprite")
                            .width(160.0)
                            .selected_text(selected_text)
                            .show_ui(ui, |ui| {
                                // "None" option = plain quad
                                if ui.selectable_label(def.texture.is_none(), "(none — quad)").clicked() {
                                    def.texture = None;
                                    state.dirty = true;
                                }
                                // Scan assets/particles/sprites/ for available images
                                let sprites_dir = std::path::Path::new("assets/particles/sprites");
                                if let Ok(entries) = std::fs::read_dir(sprites_dir) {
                                    for entry in entries.flatten() {
                                        let path = entry.path();
                                        if let Some(ext) = path.extension() {
                                            let ext = ext.to_string_lossy().to_lowercase();
                                            if ext == "png" || ext == "qoi" || ext == "jpg" {
                                                let name = path.file_name().unwrap().to_string_lossy().to_string();
                                                let is_selected = def.texture.as_deref() == Some(&name);
                                                if ui.selectable_label(is_selected, &name).clicked() {
                                                    def.texture = Some(name);
                                                    state.dirty = true;
                                                }
                                            }
                                        }
                                    }
                                }
                            });
                    });
                });

                // ── Emission Shape ──────────────────────────────────
                ui.collapsing("Emission Shape", |ui| {
                    emission_shape_ui(ui, &mut def.emission_shape, &mut state.dirty);
                });

                // ── Rotation ────────────────────────────────────────
                ui.collapsing("Rotation", |ui| {
                    let mut has_rotation = def.rotation_range.is_some();
                    if ui.checkbox(&mut has_rotation, "Initial rotation").changed() {
                        def.rotation_range = if has_rotation {
                            Some((0.0, std::f32::consts::TAU))
                        } else {
                            None
                        };
                        state.dirty = true;
                    }
                    if let Some((min, max)) = def.rotation_range.as_mut() {
                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    egui::DragValue::new(min)
                                        .range(0.0..=std::f32::consts::TAU)
                                        .speed(0.02)
                                        .prefix("min: "),
                                )
                                .changed()
                            {
                                state.dirty = true;
                            }
                            if ui
                                .add(
                                    egui::DragValue::new(max)
                                        .range(0.0..=std::f32::consts::TAU)
                                        .speed(0.02)
                                        .prefix("max: "),
                                )
                                .changed()
                            {
                                state.dirty = true;
                            }
                        });
                    }

                    let mut has_spin = def.angular_velocity.is_some();
                    if ui.checkbox(&mut has_spin, "Angular velocity").changed() {
                        def.angular_velocity = if has_spin {
                            Some((-2.0, 2.0))
                        } else {
                            None
                        };
                        state.dirty = true;
                    }
                    if let Some((min, max)) = def.angular_velocity.as_mut() {
                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    egui::DragValue::new(min)
                                        .range(-20.0..=20.0)
                                        .speed(0.05)
                                        .prefix("min: "),
                                )
                                .changed()
                            {
                                state.dirty = true;
                            }
                            if ui
                                .add(
                                    egui::DragValue::new(max)
                                        .range(-20.0..=20.0)
                                        .speed(0.05)
                                        .prefix("max: "),
                                )
                                .changed()
                            {
                                state.dirty = true;
                            }
                        });
                    }
                });

                // ── Per-Particle Light ──────────────────────────────
                ui.collapsing("Light", |ui| {
                    let mut has_light = def.light.is_some();
                    if ui
                        .checkbox(&mut has_light, "Per-particle light")
                        .changed()
                    {
                        def.light = if has_light {
                            Some(ParticleLightDef::default())
                        } else {
                            None
                        };
                        state.dirty = true;
                    }
                    if let Some(light) = def.light.as_mut() {
                        ui.horizontal(|ui| {
                            ui.label("Color");
                            if egui::color_picker::color_edit_button_rgb(ui, &mut light.color)
                                .changed()
                            {
                                state.dirty = true;
                            }
                        });
                        if ui
                            .add(
                                egui::Slider::new(&mut light.intensity, 0.1..=5.0)
                                    .text("Intensity"),
                            )
                            .changed()
                        {
                            state.dirty = true;
                        }
                        if ui
                            .add(
                                egui::Slider::new(&mut light.radius, 5.0..=200.0).text("Radius"),
                            )
                            .changed()
                        {
                            state.dirty = true;
                        }
                    }
                });

                // ── Emitter Lights (persistent) ────────────────────
                ui.collapsing("Emitter Lights", |ui| {
                    ui.label("Persistent lights at the emitter position.");

                    let mut remove_idx: Option<usize> = None;
                    let num = def.emitter_lights.len();
                    for li in 0..num {
                        let elight = &mut def.emitter_lights[li];
                        ui.push_id(format!("eml_{li}"), |ui| {
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    if egui::color_picker::color_edit_button_rgb(ui, &mut elight.color).changed() {
                                        state.dirty = true;
                                        state.preview_needs_rebuild = true;
                                    }
                                    if ui.add(egui::DragValue::new(&mut elight.intensity).range(0.1..=10.0).speed(0.05).prefix("I: ")).changed() {
                                        state.dirty = true;
                                        state.preview_needs_rebuild = true;
                                    }
                                    if ui.add(egui::DragValue::new(&mut elight.radius).range(5.0..=500.0).speed(1.0).prefix("R: ")).changed() {
                                        state.dirty = true;
                                        state.preview_needs_rebuild = true;
                                    }
                                });
                                ui.horizontal(|ui| {
                                    if ui.checkbox(&mut elight.pulse, "Pulse").changed() {
                                        state.dirty = true;
                                        state.preview_needs_rebuild = true;
                                    }
                                    if ui.checkbox(&mut elight.flicker, "Flicker").changed() {
                                        state.dirty = true;
                                        state.preview_needs_rebuild = true;
                                    }
                                    if ui.small_button("x").on_hover_text("Remove").clicked() {
                                        remove_idx = Some(li);
                                    }
                                });
                            });
                        });
                    }
                    if let Some(ri) = remove_idx {
                        def.emitter_lights.remove(ri);
                        state.dirty = true;
                        state.preview_needs_rebuild = true;
                    }
                    if ui.button("+ Add Emitter Light").clicked() {
                        def.emitter_lights.push(super::definitions::EmitterLightDef::default());
                        state.dirty = true;
                        state.preview_needs_rebuild = true;
                    }
                });

                ui.separator();

                // ── Save ───────────────────────────────────────────
                let save_label = if state.dirty { "Save *" } else { "Save" };
                if ui.button(save_label).clicked() {
                    // Write back to registry.
                    let old_id = state.selected_def.clone();
                    // If ID changed, remove old key.
                    if let Some(old) = &old_id {
                        if *old != def.id {
                            registry.defs.remove(old);
                            // Delete old JSON file.
                            let old_path = std::path::PathBuf::from(format!(
                                "assets/particles/{old}.json"
                            ));
                            let _ = std::fs::remove_file(&old_path);
                        }
                    }
                    registry.defs.insert(def.id.clone(), def.clone());
                    state.selected_def = Some(def.id.clone());
                    state.dirty = false;

                    // Write to disk.
                    let path = std::path::PathBuf::from(format!(
                        "assets/particles/{}.json",
                        def.id
                    ));
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    match serde_json::to_string_pretty(def) {
                        Ok(json) => {
                            if let Err(e) = std::fs::write(&path, &json) {
                                error!("Failed to save particle def: {e}");
                            } else {
                                info!("Saved particle def to {}", path.display());
                            }
                        }
                        Err(e) => error!("Failed to serialize particle def: {e}"),
                    }
                }
            }

            ui.separator();

            // ── Emitter Placer ─────────────────────────────────────
            ui.heading("Place Emitters");

            // Definition dropdown
            let def_ids: Vec<String> = registry.defs.keys().cloned().collect();
            ui.horizontal(|ui| {
                ui.label("Definition:");
                egui::ComboBox::from_id_salt("placer_def")
                    .width(140.0)
                    .selected_text(&state.placer_def_id)
                    .show_ui(ui, |ui| {
                        for id in &def_ids {
                            ui.selectable_value(&mut state.placer_def_id, id.clone(), id);
                        }
                    });
            });

            ui.add(egui::Slider::new(&mut state.placer_rate, 1.0..=100.0).text("Rate"));
            ui.add(egui::Slider::new(&mut state.placer_height, 0.0..=300.0).text("Height"));

            let place_label = if state.placer_active {
                "Placing (click map)"
            } else {
                "Enable Place Mode"
            };
            ui.toggle_value(&mut state.placer_active, place_label);

            if ui.button("Clear all debug emitters").clicked() {
                for (entity, _, _) in &debug_emitters {
                    commands.entity(entity).despawn();
                }
            }

            ui.separator();

            // ── Active Debug Emitters ──────────────────────────────
            ui.heading("Active Emitters");
            let count = debug_emitters.iter().count();
            ui.label(format!("{count} debug emitter(s)"));

            let mut to_despawn = Vec::new();
            for (entity, emitter, _tf) in &debug_emitters {
                ui.push_id(entity, |ui| {
                    let frame_resp = egui::Frame::new()
                        .inner_margin(egui::Margin::same(2))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(&emitter.definition_id);
                                ui.label(format!("({} active)", emitter.active_count));
                                if ui.small_button("x").clicked() {
                                    to_despawn.push(entity);
                                }
                            });
                        });

                    if frame_resp.response.hovered() {
                        state.hovered_emitter = Some(entity);
                    }
                });
            }
            for entity in to_despawn {
                commands.entity(entity).despawn();
            }
        });
}

// ── Place emitter on click ───────────────────────────────────────────────────

pub fn place_emitter_on_click(
    state: Res<ParticleEditorState>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<CombatCamera3d>>,
    mut contexts: EguiContexts,
    mut commands: Commands,
    billboards: Query<&Transform, With<crate::camera::combat::Billboard>>,
) {
    if !state.placer_active || !mouse.just_pressed(MouseButton::Left) {
        return;
    }

    if state.placer_def_id.is_empty() {
        return;
    }

    let Ok(ctx) = contexts.ctx_mut() else { return };
    if ctx.is_pointer_over_area() {
        return;
    }

    let Ok(window) = windows.single() else { return };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };
    let Ok((camera, cam_tf)) = cameras.single() else {
        return;
    };

    // Find the billboard closest to the cursor in screen space — this correctly
    // handles elevated terrain regardless of parallax.
    let mut best_bb: Option<(Vec3, f32)> = None; // (world_pos, screen_dist_sq)
    for bb_tf in &billboards {
        if let Ok(screen_pos) = camera.world_to_viewport(cam_tf, bb_tf.translation) {
            let dist_sq = screen_pos.distance_squared(cursor_pos);
            if best_bb.is_none() || dist_sq < best_bb.unwrap().1 {
                best_bb = Some((bb_tf.translation, dist_sq));
            }
        }
    }

    let place_pos = if let Some((bb_pos, _)) = best_bb {
        // Place at the billboard's XY and Z (the actual terrain surface).
        Vec3::new(bb_pos.x, bb_pos.y, bb_pos.z + state.placer_height)
    } else {
        // Fallback: ray-plane intersection at Z=0.
        let Ok(ray) = camera.viewport_to_world(cam_tf, cursor_pos) else { return };
        let Some(dist) = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Z)) else { return };
        let hit = ray.get_point(dist);
        Vec3::new(hit.x, hit.y, state.placer_height)
    };

    commands.spawn((
        Transform::from_xyz(place_pos.x, place_pos.y, place_pos.z),
        Visibility::default(),
        ParticleEmitter::new(state.placer_def_id.clone(), state.placer_rate),
        DebugParticleEmitter,
    ));
}

// ── Gizmos ───────────────────────────────────────────────────────────────────

pub fn draw_emitter_gizmos(
    mut state: ResMut<ParticleEditorState>,
    mut emitters: Query<(Entity, &mut Transform, &ParticleEmitter), With<DebugParticleEmitter>>,
    mut gizmos: Gizmos,
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<CombatCamera3d>>,
    mut contexts: EguiContexts,
) {
    if !state.open {
        return;
    }

    let Ok(window) = windows.single() else { return };
    let Ok((camera, cam_tf)) = cameras.single() else { return };
    let cursor_pos = window.cursor_position();
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let egui_wants_pointer = ctx.is_pointer_over_area();

    let icon_world_radius = 10.0;
    let pick_screen_radius = 20.0;
    let mut closest_pick: Option<(Entity, f32)> = None;

    for (entity, tf, _emitter) in &emitters {
        let pos = tf.translation;
        let is_hovered = state.hovered_emitter == Some(entity);
        let is_dragging = state.dragging == Some(entity);

        let color = Color::linear_rgb(0.2, 0.8, 0.9);
        let icon_r = if is_hovered || is_dragging {
            icon_world_radius * 1.5
        } else {
            icon_world_radius
        };

        gizmos.circle(
            Isometry3d::new(pos, Quat::IDENTITY),
            icon_r,
            color,
        );
        gizmos.circle(
            Isometry3d::new(pos, Quat::IDENTITY),
            icon_r + 2.0,
            Color::WHITE,
        );

        if pos.z > 1.0 {
            let ground = Vec3::new(pos.x, pos.y, 0.0);
            gizmos.line(ground, pos, color.with_alpha(0.4));
        }

        if is_hovered {
            let pulse = ((time.elapsed_secs() * 3.0).sin() * 0.3 + 0.7).clamp(0.4, 1.0);
            let highlight = color.with_alpha(pulse);
            let top = pos + Vec3::Z * 30.0;
            gizmos.line(pos, top, highlight);
            gizmos.line(top, top + Vec3::new(-5.0, 0.0, -8.0), highlight);
            gizmos.line(top, top + Vec3::new(5.0, 0.0, -8.0), highlight);
        }

        if let Some(cursor) = cursor_pos {
            if let Ok(screen_pos) = camera.world_to_viewport(cam_tf, pos) {
                let dist = screen_pos.distance(cursor);
                if dist < pick_screen_radius {
                    if closest_pick.is_none() || dist < closest_pick.unwrap().1 {
                        closest_pick = Some((entity, dist));
                    }
                }
            }
        }
    }

    // ── Drag logic (mirrors draw_light_gizmos in debug_panel.rs) ───────
    if state.placer_active {
        return;
    }

    if mouse.just_pressed(MouseButton::Left) && !egui_wants_pointer {
        if let Some((entity, _)) = closest_pick {
            state.dragging = Some(entity);
        }
    }

    if mouse.just_released(MouseButton::Left) {
        state.dragging = None;
    }

    if let Some(drag_entity) = state.dragging {
        if let Some(cursor) = cursor_pos {
            let Ok(ray) = camera.viewport_to_world(cam_tf, cursor) else {
                return;
            };
            let Some(dist) = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Z))
            else {
                return;
            };
            let world_pos = ray.get_point(dist);

            if let Ok((_, mut tf, _)) = emitters.get_mut(drag_entity) {
                tf.translation.x = world_pos.x;
                tf.translation.y = world_pos.y;
            }
        }
    }
}

// ── Live preview ─────────────────────────────────────────────────────────────

pub fn particle_editor_preview(
    mut commands: Commands,
    mut state: ResMut<ParticleEditorState>,
    mut registry: ResMut<ParticleRegistry>,
    mut preview_emitters: Query<
        (Entity, &mut ParticleEmitter, &mut Transform, Option<&bevy_hanabi::ParticleEffect>),
        With<PreviewParticleEmitter>,
    >,
    cameras: Query<(&Camera, &GlobalTransform), With<CombatCamera3d>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    elev_heights: Res<crate::map::elevation::ElevationHeights>,
) {
    // If editor is closed or no definition selected, despawn preview.
    let should_preview = state.open && state.editing_def.is_some();

    if !should_preview {
        for (entity, _, _, _) in &preview_emitters {
            commands.entity(entity).despawn();
        }
        return;
    }

    let def = state.editing_def.as_ref().unwrap().clone();

    // Write the working copy into the registry so the shadow particle spawner can find it.
    registry.defs.insert(def.id.clone(), def.clone());

    // Compute preview position at the billboard surface height.
    let base_z = elev_heights.z_by_level.get(&0).copied().unwrap_or(-1.0);
    let tilt_rad = crate::camera::combat::BILLBOARD_TILT_DEG.to_radians();
    let surface_z = base_z + crate::map::DEFAULT_TILE_SIZE * 0.5 * tilt_rad.sin();

    let preview_pos = (|| -> Option<Vec3> {
        let window = windows.single().ok()?;
        let (camera, cam_tf) = cameras.single().ok()?;
        let screen_pos = Vec2::new(window.width() * 0.85, window.height() * 0.80);
        let ray = camera.viewport_to_world(cam_tf, screen_pos).ok()?;
        let plane_origin = Vec3::new(0.0, 0.0, surface_z);
        let dist = ray.intersect_plane(plane_origin, InfinitePlane3d::new(Vec3::Z))?;
        Some(ray.get_point(dist) + Vec3::Z * 50.0)
    })()
    .unwrap_or(Vec3::new(0.0, 0.0, surface_z + 2.0));

    // Only do a full despawn/respawn when the definition ID changes or
    // emitter lights need rebuilding (one-shot). NOT on every dirty frame —
    // that would kill the entity before hanabi compiles the GPU effect.
    // Detect actual property changes via a hash of the serialized def.
    // This avoids rebuilding every frame while `dirty` is persistently true.
    let def_hash = {
        use std::hash::{Hash, Hasher};
        let json = serde_json::to_string(&def).unwrap_or_default();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        json.hash(&mut hasher);
        hasher.finish()
    };
    let def_changed = def_hash != state.preview_def_hash;
    let full_rebuild = def_changed || state.preview_needs_rebuild;

    if full_rebuild && !preview_emitters.is_empty() {
        for (entity, _, _, _) in &preview_emitters {
            commands.entity(entity).despawn();
        }
    }

    if full_rebuild || preview_emitters.is_empty() {
        commands.spawn((
            Transform::from_translation(preview_pos),
            Visibility::default(),
            ParticleEmitter::new(def.id.clone(), 10.0),
            PreviewParticleEmitter,
        ));
        state.preview_def_hash = def_hash;
        state.preview_needs_rebuild = false;
    } else {
        for (_entity, _emitter, mut tf, _) in &mut preview_emitters {
            tf.translation = preview_pos;
        }
    }
}

// ── UI Helpers ───────────────────────────────────────────────────────────────

fn emission_direction_ui(
    ui: &mut egui::Ui,
    direction: &mut EmissionDirection,
    dirty: &mut bool,
) {
    let mut kind: u8 = match direction {
        EmissionDirection::Sphere => 0,
        EmissionDirection::Cone { .. } => 1,
        EmissionDirection::Up => 2,
        EmissionDirection::Ring { .. } => 3,
    };

    ui.horizontal(|ui| {
        ui.label("Direction");
        egui::ComboBox::from_id_salt("emission_dir")
            .width(80.0)
            .selected_text(match kind {
                1 => "Cone",
                2 => "Up",
                3 => "Ring",
                _ => "Sphere",
            })
            .show_ui(ui, |ui| {
                if ui.selectable_value(&mut kind, 0, "Sphere").changed() {
                    *dirty = true;
                }
                if ui.selectable_value(&mut kind, 1, "Cone").changed() {
                    *dirty = true;
                }
                if ui.selectable_value(&mut kind, 2, "Up").changed() {
                    *dirty = true;
                }
                if ui.selectable_value(&mut kind, 3, "Ring").changed() {
                    *dirty = true;
                }
            });
    });

    // Check if kind changed.
    let current_kind: u8 = match direction {
        EmissionDirection::Sphere => 0,
        EmissionDirection::Cone { .. } => 1,
        EmissionDirection::Up => 2,
        EmissionDirection::Ring { .. } => 3,
    };
    if kind != current_kind {
        *direction = match kind {
            1 => EmissionDirection::Cone {
                angle: std::f32::consts::FRAC_PI_2,
                direction: [0.0, 0.0, 1.0],
            },
            2 => EmissionDirection::Up,
            3 => EmissionDirection::Ring { radius: 20.0 },
            _ => EmissionDirection::Sphere,
        };
        *dirty = true;
    }

    // Direction-specific params.
    match direction {
        EmissionDirection::Cone { angle, direction: dir } => {
            let mut ang_deg = angle.to_degrees();
            if ui
                .add(egui::Slider::new(&mut ang_deg, 1.0..=180.0).text("Cone Angle"))
                .changed()
            {
                *angle = ang_deg.to_radians();
                *dirty = true;
            }
            ui.horizontal(|ui| {
                ui.label("Dir");
                for (i, label) in ["X", "Y", "Z"].iter().enumerate() {
                    if ui
                        .add(
                            egui::DragValue::new(&mut dir[i])
                                .range(-1.0..=1.0)
                                .speed(0.01)
                                .prefix(format!("{label}: ")),
                        )
                        .changed()
                    {
                        *dirty = true;
                    }
                }
            });
        }
        EmissionDirection::Ring { radius } => {
            if ui
                .add(egui::Slider::new(radius, 1.0..=200.0).text("Ring Radius"))
                .changed()
            {
                *dirty = true;
            }
        }
        _ => {}
    }
}

fn emission_shape_ui(ui: &mut egui::Ui, shape: &mut EmissionShape, dirty: &mut bool) {
    let mut kind: u8 = match shape {
        EmissionShape::Point => 0,
        EmissionShape::Sphere { .. } => 1,
        EmissionShape::Box { .. } => 2,
        EmissionShape::Ring { .. } => 3,
    };

    ui.horizontal(|ui| {
        ui.label("Shape");
        egui::ComboBox::from_id_salt("emission_shape")
            .width(80.0)
            .selected_text(match kind {
                1 => "Sphere",
                2 => "Box",
                3 => "Ring",
                _ => "Point",
            })
            .show_ui(ui, |ui| {
                if ui.selectable_value(&mut kind, 0, "Point").changed() {
                    *dirty = true;
                }
                if ui.selectable_value(&mut kind, 1, "Sphere").changed() {
                    *dirty = true;
                }
                if ui.selectable_value(&mut kind, 2, "Box").changed() {
                    *dirty = true;
                }
                if ui.selectable_value(&mut kind, 3, "Ring").changed() {
                    *dirty = true;
                }
            });
    });

    let current_kind: u8 = match shape {
        EmissionShape::Point => 0,
        EmissionShape::Sphere { .. } => 1,
        EmissionShape::Box { .. } => 2,
        EmissionShape::Ring { .. } => 3,
    };
    if kind != current_kind {
        *shape = match kind {
            1 => EmissionShape::Sphere { radius: 10.0 },
            2 => EmissionShape::Box {
                half_extents: [10.0, 10.0, 5.0],
            },
            3 => EmissionShape::Ring {
                radius: 20.0,
                width: 5.0,
            },
            _ => EmissionShape::Point,
        };
        *dirty = true;
    }

    match shape {
        EmissionShape::Sphere { radius } => {
            if ui
                .add(egui::Slider::new(radius, 1.0..=200.0).text("Radius"))
                .changed()
            {
                *dirty = true;
            }
        }
        EmissionShape::Box { half_extents } => {
            ui.horizontal(|ui| {
                for (i, label) in ["X", "Y", "Z"].iter().enumerate() {
                    if ui
                        .add(
                            egui::DragValue::new(&mut half_extents[i])
                                .range(0.1..=200.0)
                                .speed(0.5)
                                .prefix(format!("{label}: ")),
                        )
                        .changed()
                    {
                        *dirty = true;
                    }
                }
            });
        }
        EmissionShape::Ring { radius, width } => {
            if ui
                .add(egui::Slider::new(radius, 1.0..=200.0).text("Radius"))
                .changed()
            {
                *dirty = true;
            }
            if ui
                .add(egui::Slider::new(width, 0.1..=50.0).text("Width"))
                .changed()
            {
                *dirty = true;
            }
        }
        EmissionShape::Point => {}
    }
}
