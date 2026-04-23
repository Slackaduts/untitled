//! Light and particle emitter editing UI for composed objects.
//! Migrated from billboard/object_editor.rs.

use bevy_egui::egui;

use crate::billboard::object_types::{MovementMode, ObjectEmitter, ObjectLight, SpriteType};

use super::state::ComposedObject;

/// Render the lights/emitters editing panel for a composed object.
/// Returns `true` if any property was modified (dirty flag).
/// `particle_def_ids` is the list of available particle definition IDs for the emitter dropdown.
pub fn lights_emitters_ui(ui: &mut egui::Ui, obj: &mut ComposedObject, particle_def_ids: &[String]) -> bool {
    let mut dirty = false;

    // ── Sprite Type ───────────────────────────────────────────────
    ui.horizontal(|ui| {
        ui.label("Sprite:");
        let label = match &obj.properties.sprite_type {
            SpriteType::Static => "Static",
            SpriteType::Lpc => "LPC",
            SpriteType::Custom { .. } => "Custom",
        };
        egui::ComboBox::from_id_salt("sprite_type")
            .selected_text(label)
            .show_ui(ui, |ui| {
                if ui.selectable_label(obj.properties.sprite_type == SpriteType::Static, "Static").clicked() {
                    obj.properties.sprite_type = SpriteType::Static;
                    dirty = true;
                }
                if ui.selectable_label(obj.properties.sprite_type == SpriteType::Lpc, "LPC").clicked() {
                    obj.properties.sprite_type = SpriteType::Lpc;
                    dirty = true;
                }
                if ui.selectable_label(matches!(obj.properties.sprite_type, SpriteType::Custom { .. }), "Custom").clicked() {
                    if !matches!(obj.properties.sprite_type, SpriteType::Custom { .. }) {
                        obj.properties.sprite_type = SpriteType::Custom { frame_w: 64, frame_h: 64, columns: 4 };
                        dirty = true;
                    }
                }
            });
    });
    if let SpriteType::Custom { ref mut frame_w, ref mut frame_h, ref mut columns } = obj.properties.sprite_type {
        ui.horizontal(|ui| {
            ui.label("Frame:");
            if ui.add(egui::DragValue::new(frame_w).range(1..=512).speed(1).prefix("W: ")).changed() { dirty = true; }
            if ui.add(egui::DragValue::new(frame_h).range(1..=512).speed(1).prefix("H: ")).changed() { dirty = true; }
            if ui.add(egui::DragValue::new(columns).range(1..=64).speed(1).prefix("Cols: ")).changed() { dirty = true; }
        });
    }

    // ── Movement Mode ─────────────────────────────────────────────
    ui.horizontal(|ui| {
        ui.label("Movement:");
        let label = match &obj.properties.movement_mode {
            MovementMode::None => "None",
            MovementMode::GridSnap => "Grid Snap",
            MovementMode::FreeMove => "Free Move",
        };
        egui::ComboBox::from_id_salt("movement_mode")
            .selected_text(label)
            .show_ui(ui, |ui| {
                if ui.selectable_label(obj.properties.movement_mode == MovementMode::None, "None").clicked() {
                    obj.properties.movement_mode = MovementMode::None;
                    dirty = true;
                }
                if ui.selectable_label(obj.properties.movement_mode == MovementMode::GridSnap, "Grid Snap").clicked() {
                    obj.properties.movement_mode = MovementMode::GridSnap;
                    dirty = true;
                }
                if ui.selectable_label(obj.properties.movement_mode == MovementMode::FreeMove, "Free Move").clicked() {
                    obj.properties.movement_mode = MovementMode::FreeMove;
                    dirty = true;
                }
            });
    });

    ui.separator();

    // ── Lights ─────────────────────────────────────────────────────
    ui.heading("Lights");

    let mut remove_idx: Option<usize> = None;
    let num_lights = obj.properties.lights.len();
    for li in 0..num_lights {
        let light = &mut obj.properties.lights[li];
        ui.push_id(format!("light_{li}"), |ui| {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(&light.ref_id)
                            .small()
                            .color(egui::Color32::from_gray(140)),
                    );
                });
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
                        dirty = true;
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
                        dirty = true;
                    }
                });

                // Shape
                ui.horizontal(|ui| {
                    ui.label("Shape:");
                    let prev_shape = light.shape.clone();
                    egui::ComboBox::from_id_salt(format!("shape_{li}"))
                        .width(80.0)
                        .selected_text(&light.shape)
                        .show_ui(ui, |ui| {
                            for shape in ["point", "cone", "line", "capsule"] {
                                ui.selectable_value(
                                    &mut light.shape,
                                    shape.to_string(),
                                    shape,
                                );
                            }
                        });
                    if light.shape != prev_shape {
                        dirty = true;
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
                        dirty = true;
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
                        dirty = true;
                    }
                    if ui
                        .add(
                            egui::DragValue::new(&mut light.offset_z)
                                .range(0.0..=2.0)
                                .speed(0.01)
                                .prefix("Depth: "),
                        )
                        .on_hover_text("Distance in front of billboard (0 = on face)")
                        .changed()
                    {
                        dirty = true;
                    }
                });

                // Pulse / Flicker / Remove
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut light.pulse, "Pulse").changed() {
                        dirty = true;
                    }
                    if ui.checkbox(&mut light.flicker, "Flicker").changed() {
                        dirty = true;
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
        dirty = true;
    }

    if ui.button("+ Add Light").clicked() {
        let idx = obj.properties.lights.len();
        let mut light = ObjectLight::default();
        light.ref_id = format!("light_{idx}");
        obj.properties.lights.push(light);
        dirty = true;
    }

    ui.separator();

    // ── Particle Emitters ──────────────────────────────────────────
    ui.heading("Particle Emitters");

    let mut remove_emitter_idx: Option<usize> = None;
    let num_emitters = obj.properties.emitters.len();
    for ei in 0..num_emitters {
        let emitter = &mut obj.properties.emitters[ei];
        ui.push_id(format!("emitter_{ei}"), |ui| {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(&emitter.ref_id)
                            .small()
                            .color(egui::Color32::from_gray(140)),
                    );
                });
                // Definition ID + rate
                ui.horizontal(|ui| {
                    ui.label("Def:");
                    let selected_text = if emitter.definition_id.is_empty() {
                        "Select..."
                    } else {
                        &emitter.definition_id
                    };
                    egui::ComboBox::from_id_salt(format!("emitter_def_{ei}"))
                        .width(120.0)
                        .selected_text(selected_text)
                        .show_ui(ui, |ui| {
                            for def_id in particle_def_ids {
                                if ui.selectable_label(emitter.definition_id == *def_id, def_id).clicked() {
                                    emitter.definition_id = def_id.clone();
                                    dirty = true;
                                }
                            }
                        });
                    if ui
                        .add(
                            egui::DragValue::new(&mut emitter.rate)
                                .range(0.1..=100.0)
                                .speed(0.5)
                                .prefix("Rate: "),
                        )
                        .changed()
                    {
                        dirty = true;
                    }
                });

                // Offset
                ui.horizontal(|ui| {
                    ui.label("Offset:");
                    if ui
                        .add(
                            egui::DragValue::new(&mut emitter.offset_x)
                                .range(0.0..=1.0)
                                .speed(0.01)
                                .prefix("X: "),
                        )
                        .changed()
                    {
                        dirty = true;
                    }
                    if ui
                        .add(
                            egui::DragValue::new(&mut emitter.offset_y)
                                .range(0.0..=1.0)
                                .speed(0.01)
                                .prefix("Y: "),
                        )
                        .changed()
                    {
                        dirty = true;
                    }
                    if ui
                        .add(
                            egui::DragValue::new(&mut emitter.offset_z)
                                .range(0.0..=2.0)
                                .speed(0.01)
                                .prefix("Depth: "),
                        )
                        .on_hover_text("Distance in front of billboard (0 = on face)")
                        .changed()
                    {
                        dirty = true;
                    }
                });

                // Active + remove
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut emitter.active, "Active").changed() {
                        dirty = true;
                    }
                    if ui
                        .small_button("x")
                        .on_hover_text("Remove emitter")
                        .clicked()
                    {
                        remove_emitter_idx = Some(ei);
                    }
                });
            });
        });
    }

    if let Some(ri) = remove_emitter_idx {
        obj.properties.emitters.remove(ri);
        dirty = true;
    }

    if ui.button("+ Add Emitter").clicked() {
        let idx = obj.properties.emitters.len();
        let mut emitter = ObjectEmitter::default();
        emitter.ref_id = format!("emitter_{idx}");
        obj.properties.emitters.push(emitter);
        dirty = true;
    }

    dirty
}

/// Draw light and emitter position dots on a sprite preview rect.
pub fn draw_preview_overlay(
    ui: &egui::Ui,
    rect: egui::Rect,
    obj: &ComposedObject,
    scale: f32,
) {
    let hover_pos = ui.input(|i| i.pointer.hover_pos());

    // Draw light positions
    for (li, light) in obj.properties.lights.iter().enumerate() {
        let light_pos = egui::pos2(
            rect.min.x + light.offset_x * rect.width(),
            rect.min.y + light.offset_y * rect.height(),
        );

        let light_color = egui::Color32::from_rgb(
            (light.color[0] * 255.0) as u8,
            (light.color[1] * 255.0) as u8,
            (light.color[2] * 255.0) as u8,
        );

        let is_hovered = hover_pos.is_some_and(|hp| {
            (hp.x - light_pos.x).hypot(hp.y - light_pos.y) < 12.0
        });

        // Radius circle
        let radius_px = light.radius * scale;
        let alpha = if is_hovered { 50 } else { 25 };
        ui.painter().circle_stroke(
            light_pos,
            radius_px,
            egui::Stroke::new(
                1.0,
                egui::Color32::from_rgba_unmultiplied(
                    (light.color[0] * 255.0) as u8,
                    (light.color[1] * 255.0) as u8,
                    (light.color[2] * 255.0) as u8,
                    alpha,
                ),
            ),
        );

        // Fill
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

        // Dot
        let dot_r = if is_hovered { 6.0 } else { 4.0 };
        ui.painter().circle_filled(light_pos, dot_r, light_color);
        let outline = if is_hovered {
            egui::Color32::WHITE
        } else {
            egui::Color32::BLACK
        };
        ui.painter()
            .circle_stroke(light_pos, dot_r, egui::Stroke::new(1.5, outline));

        if is_hovered {
            ui.painter().text(
                egui::pos2(light_pos.x, light_pos.y - dot_r - 4.0),
                egui::Align2::CENTER_BOTTOM,
                format!("Light {li}"),
                egui::FontId::proportional(11.0),
                egui::Color32::WHITE,
            );
        }
    }

    // Draw emitter positions (cyan dots)
    for (ei, emitter) in obj.properties.emitters.iter().enumerate() {
        let emitter_pos = egui::pos2(
            rect.min.x + emitter.offset_x * rect.width(),
            rect.min.y + emitter.offset_y * rect.height(),
        );

        let color = egui::Color32::from_rgb(60, 200, 220);
        let is_hovered = hover_pos.is_some_and(|hp| {
            (hp.x - emitter_pos.x).hypot(hp.y - emitter_pos.y) < 12.0
        });

        let dot_r = if is_hovered { 6.0 } else { 4.0 };
        ui.painter().circle_filled(emitter_pos, dot_r, color);
        let outline = if is_hovered {
            egui::Color32::WHITE
        } else {
            egui::Color32::BLACK
        };
        ui.painter()
            .circle_stroke(emitter_pos, dot_r, egui::Stroke::new(1.5, outline));

        if is_hovered {
            ui.painter().text(
                egui::pos2(emitter_pos.x, emitter_pos.y - dot_r - 4.0),
                egui::Align2::CENTER_BOTTOM,
                format!("Emitter {ei}"),
                egui::FontId::proportional(11.0),
                egui::Color32::WHITE,
            );
        }
    }
}
