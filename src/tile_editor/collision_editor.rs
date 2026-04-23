//! Collision rect editor: draw rectangles on the sprite preview.

use bevy_egui::egui;

use super::sidecar::CollisionRect;
use super::state::ComposedObject;

/// State for collision rect drawing within a frame.
#[derive(Default)]
pub struct CollisionDrawState {
    /// Whether drawing mode is active.
    pub drawing: bool,
    /// Start position of the current rect being drawn (sprite-local pixels).
    pub draw_start: Option<egui::Pos2>,
}

/// Render the collision rect editor UI below the sprite preview.
/// Returns `true` if any collision data was modified.
pub fn collision_editor_ui(
    ui: &mut egui::Ui,
    obj: &mut ComposedObject,
    draw_state: &mut CollisionDrawState,
) -> bool {
    let mut dirty = false;

    ui.heading("Collision Rects");

    // Drawing toggle
    let draw_label = if draw_state.drawing {
        "Drawing (click+drag on preview)"
    } else {
        "Enable Draw Mode"
    };
    ui.toggle_value(&mut draw_state.drawing, draw_label);

    // Rect list
    let mut remove_idx: Option<usize> = None;
    for (ri, rect) in obj.collision_rects.iter_mut().enumerate() {
        ui.push_id(format!("collision_{ri}"), |ui| {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("Rect {ri}"));
                    if ui
                        .add(
                            egui::DragValue::new(&mut rect.x)
                                .range(0.0..=512.0)
                                .speed(1.0)
                                .prefix("X: "),
                        )
                        .changed()
                    {
                        dirty = true;
                    }
                    if ui
                        .add(
                            egui::DragValue::new(&mut rect.y)
                                .range(0.0..=512.0)
                                .speed(1.0)
                                .prefix("Y: "),
                        )
                        .changed()
                    {
                        dirty = true;
                    }
                    if ui
                        .add(
                            egui::DragValue::new(&mut rect.w)
                                .range(1.0..=512.0)
                                .speed(1.0)
                                .prefix("W: "),
                        )
                        .changed()
                    {
                        dirty = true;
                    }
                    if ui
                        .add(
                            egui::DragValue::new(&mut rect.h)
                                .range(1.0..=512.0)
                                .speed(1.0)
                                .prefix("H: "),
                        )
                        .changed()
                    {
                        dirty = true;
                    }
                });
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::DragValue::new(&mut rect.depth_fwd)
                                .range(0.0..=96.0)
                                .speed(1.0)
                                .prefix("Depth fwd: "),
                        )
                        .changed()
                    {
                        dirty = true;
                    }
                    if ui
                        .add(
                            egui::DragValue::new(&mut rect.depth_back)
                                .range(0.0..=96.0)
                                .speed(1.0)
                                .prefix("back: "),
                        )
                        .changed()
                    {
                        dirty = true;
                    }
                    if ui
                        .small_button("x")
                        .on_hover_text("Remove rect")
                        .clicked()
                    {
                        remove_idx = Some(ri);
                    }
                });
            });
        });
    }

    if let Some(ri) = remove_idx {
        obj.collision_rects.remove(ri);
        dirty = true;
    }

    dirty
}

/// Draw collision rect overlays on the sprite preview and handle mouse drawing.
/// `preview_rect` is the egui rect of the sprite preview.
/// `sprite_w/h` are the sprite dimensions in pixels.
pub fn draw_collision_overlay(
    ui: &egui::Ui,
    preview_rect: egui::Rect,
    sprite_w: f32,
    sprite_h: f32,
    obj: &mut ComposedObject,
    draw_state: &mut CollisionDrawState,
) -> bool {
    let mut dirty = false;
    let scale_x = preview_rect.width() / sprite_w;
    let scale_y = preview_rect.height() / sprite_h;

    // Draw existing rects
    let collision_color = egui::Color32::from_rgba_unmultiplied(80, 220, 80, 50);
    let collision_stroke = egui::Stroke::new(1.5, egui::Color32::from_rgb(80, 220, 80));

    for rect in &obj.collision_rects {
        let draw_rect = egui::Rect::from_min_size(
            egui::pos2(
                preview_rect.min.x + rect.x * scale_x,
                // Flip Y: sprite coords have Y=0 at bottom, egui at top
                preview_rect.max.y - (rect.y + rect.h) * scale_y,
            ),
            egui::vec2(rect.w * scale_x, rect.h * scale_y),
        );
        ui.painter().rect_filled(draw_rect, 0.0, collision_color);
        ui.painter()
            .rect_stroke(draw_rect, 0.0, collision_stroke, egui::StrokeKind::Outside);
    }

    // Handle drawing new rect
    if draw_state.drawing {
        let pointer = ui.input(|i| i.pointer.clone());
        let hover_pos = pointer.hover_pos();

        if let Some(pos) = hover_pos {
            if preview_rect.contains(pos) {
                if pointer.primary_pressed() && draw_state.draw_start.is_none() {
                    // Convert screen pos to sprite-local pixels
                    let sprite_x = (pos.x - preview_rect.min.x) / scale_x;
                    let sprite_y = sprite_h - (pos.y - preview_rect.min.y) / scale_y;
                    draw_state.draw_start = Some(egui::pos2(sprite_x, sprite_y));
                }
            }
        }

        // While dragging, draw preview rect
        if let (Some(start), Some(pos)) = (draw_state.draw_start, hover_pos) {
            let curr_x = (pos.x - preview_rect.min.x) / scale_x;
            let curr_y = sprite_h - (pos.y - preview_rect.min.y) / scale_y;

            let min_x = start.x.min(curr_x);
            let min_y = start.y.min(curr_y);
            let max_x = start.x.max(curr_x);
            let max_y = start.y.max(curr_y);

            // Draw preview
            let draw_rect = egui::Rect::from_min_size(
                egui::pos2(
                    preview_rect.min.x + min_x * scale_x,
                    preview_rect.max.y - max_y * scale_y,
                ),
                egui::vec2((max_x - min_x) * scale_x, (max_y - min_y) * scale_y),
            );
            let preview_color = egui::Color32::from_rgba_unmultiplied(80, 220, 80, 80);
            let preview_stroke =
                egui::Stroke::new(2.0, egui::Color32::from_rgb(120, 255, 120));
            ui.painter().rect_filled(draw_rect, 0.0, preview_color);
            ui.painter()
                .rect_stroke(draw_rect, 0.0, preview_stroke, egui::StrokeKind::Outside);

            // Finalize on release
            if pointer.primary_released() {
                let w = max_x - min_x;
                let h = max_y - min_y;
                if w > 2.0 && h > 2.0 {
                    obj.collision_rects.push(CollisionRect {
                        x: min_x,
                        y: min_y,
                        w,
                        h,
                        depth_fwd: 48.0,
                        depth_back: 48.0,
                    });
                    dirty = true;
                }
                draw_state.draw_start = None;
            }
        }

        // Cancel on primary released without start
        if !pointer.primary_down() {
            draw_state.draw_start = None;
        }
    }

    dirty
}
