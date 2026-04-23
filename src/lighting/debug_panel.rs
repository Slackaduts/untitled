use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::{EguiContexts, egui};

use super::components::{FlickerConfig, LightShape, LightSource, PulseConfig};
use super::time_of_day::TimeOfDay;
use crate::camera::CombatCamera3d;

#[derive(Resource)]
pub struct LightingDebugPanel {
    pub open: bool,
    pub ambient_override: bool,
    // Light placer state
    pub placer_active: bool,
    pub placer_color: [f32; 3],
    pub placer_intensity: f32,
    pub placer_inner_radius: f32,
    pub placer_outer_radius: f32,
    pub placer_pulse: Option<PulseConfig>,
    pub placer_flicker: Option<FlickerConfig>,
    pub placer_shape: LightShape,
    pub placer_height: f32,
    // Interaction state
    pub dragging: Option<Entity>,
    pub hovered_light: Option<Entity>,
}

impl Default for LightingDebugPanel {
    fn default() -> Self {
        Self {
            open: false,
            ambient_override: false,
            placer_active: false,
            placer_color: [1.0, 0.85, 0.6],
            placer_intensity: 1.5,
            placer_inner_radius: 20.0,
            placer_outer_radius: 120.0,
            placer_pulse: None,
            placer_flicker: None,
            placer_shape: LightShape::Point,
            placer_height: 80.0,
            dragging: None,
            hovered_light: None,
        }
    }
}

/// Marker for lights spawned by the debug placer.
#[derive(Component)]
pub struct DebugLight;

pub fn toggle_debug_panel(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut panel: ResMut<LightingDebugPanel>,
) {
    if keyboard.just_pressed(KeyCode::F4) {
        panel.open = !panel.open;
    }
}

pub fn lighting_debug_ui(
    mut contexts: EguiContexts,
    mut panel: ResMut<LightingDebugPanel>,
    mut tod: ResMut<TimeOfDay>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut lights: Query<(Entity, &mut LightSource, &mut Transform), With<DebugLight>>,
    mut commands: Commands,
) {
    if !panel.open {
        return;
    }

    let panel = panel.as_mut();
    panel.hovered_light = None;

    let Ok(ctx) = contexts.ctx_mut() else { return };
    egui::Window::new("Lighting")
        .default_width(300.0)
        .show(ctx, |ui| {
            // ── Time of Day ─────────────────────────────────────────
            ui.heading("Time of Day");
            ui.horizontal(|ui| {
                ui.label("Hour");
                ui.add(egui::DragValue::new(&mut tod.hour).range(0.0..=23.99).speed(0.05));
            });
            ui.horizontal(|ui| {
                ui.label("Cycle speed");
                ui.add(egui::DragValue::new(&mut tod.speed).range(0.0..=10.0).speed(0.01));
            });
            ui.checkbox(&mut tod.paused, "Paused");
            ui.separator();

            // ── Ambient ─────────────────────────────────────────────
            ui.heading("Ambient");
            ui.checkbox(&mut panel.ambient_override, "Manual override");
            if panel.ambient_override {
                let c = ambient.color.to_linear();
                let mut rgb = [c.red, c.green, c.blue];
                ui.horizontal(|ui| {
                    ui.label("Color");
                    egui::color_picker::color_edit_button_rgb(ui, &mut rgb);
                });
                ambient.color = Color::linear_rgb(rgb[0], rgb[1], rgb[2]);
                ui.add(egui::Slider::new(&mut ambient.brightness, 0.0..=400.0).text("Brightness"));
            } else {
                let c = ambient.color.to_linear();
                ui.label(format!(
                    "Auto: ({:.2}, {:.2}, {:.2}) @ {:.0} cd/m²",
                    c.red, c.green, c.blue, ambient.brightness
                ));
            }
            ui.separator();

            // ── Light Placer ────────────────────────────────────────
            ui.heading("Place Lights");
            ui.horizontal(|ui| {
                ui.label("Color");
                egui::color_picker::color_edit_button_rgb(ui, &mut panel.placer_color);
            });
            ui.add(egui::Slider::new(&mut panel.placer_intensity, 0.1..=5.0).text("Intensity"));
            ui.add(egui::Slider::new(&mut panel.placer_inner_radius, 1.0..=200.0).text("Inner R"));
            ui.add(egui::Slider::new(&mut panel.placer_outer_radius, 10.0..=500.0).text("Outer R"));
            ui.add(egui::Slider::new(&mut panel.placer_height, 0.0..=300.0).text("Height"));

            shape_ui(ui, &mut panel.placer_shape, "placer");
            pulse_ui(ui, &mut panel.placer_pulse, "placer");
            flicker_ui(ui, &mut panel.placer_flicker, "placer");

            let place_label = if panel.placer_active {
                "Placing (click map)"
            } else {
                "Enable Place Mode"
            };
            ui.toggle_value(&mut panel.placer_active, place_label);

            if ui.button("Clear all debug lights").clicked() {
                for (entity, _, _) in lights.iter() {
                    commands.entity(entity).despawn();
                }
            }
            ui.separator();

            // ── Active Debug Lights ─────────────────────────────────
            ui.heading("Active Lights");
            let count = lights.iter().count();
            ui.label(format!("{count} debug light(s)"));

            let mut to_despawn = Vec::new();
            for (entity, mut light, mut tf) in lights.iter_mut() {
                ui.push_id(entity, |ui| {
                    let frame_resp = egui::Frame::new()
                        .inner_margin(egui::Margin::same(2))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let c = light.color.to_linear();
                                let mut rgb = [c.red, c.green, c.blue];
                                egui::color_picker::color_edit_button_rgb(ui, &mut rgb);
                                light.color = Color::linear_rgb(rgb[0], rgb[1], rgb[2]);
                                ui.add(
                                    egui::DragValue::new(&mut light.base_intensity)
                                        .range(0.0..=5.0)
                                        .speed(0.05)
                                        .prefix("I: "),
                                );
                                ui.add(
                                    egui::DragValue::new(&mut light.outer_radius)
                                        .range(10.0..=500.0)
                                        .speed(1.0)
                                        .prefix("R: "),
                                );
                                ui.add(
                                    egui::DragValue::new(&mut tf.translation.z)
                                        .range(0.0..=300.0)
                                        .speed(1.0)
                                        .prefix("H: "),
                                );
                                if ui.small_button("x").clicked() {
                                    to_despawn.push(entity);
                                }
                            });
                            let id = format!("{entity:?}");
                            pulse_ui(ui, &mut light.pulse, &id);
                            flicker_ui(ui, &mut light.flicker, &id);
                            shape_ui(ui, &mut light.shape, &id);
                        });

                    if frame_resp.response.hovered() {
                        panel.hovered_light = Some(entity);
                    }
                });
            }
            for entity in to_despawn {
                commands.entity(entity).despawn();
            }
            ui.separator();

        });
}

// ── Shared UI helpers ───────────────────────────────────────────────────────

pub fn pulse_ui(ui: &mut egui::Ui, pulse: &mut Option<PulseConfig>, id_salt: &str) {
    ui.push_id(format!("pulse_{id_salt}"), |ui| {
        let mut enabled = pulse.is_some();
        ui.horizontal(|ui| {
            ui.checkbox(&mut enabled, "Pulse");
            if let Some(p) = pulse.as_mut() {
                ui.add(egui::DragValue::new(&mut p.min).range(0.0..=5.0).speed(0.02).prefix("min: "));
                ui.add(egui::DragValue::new(&mut p.max).range(0.0..=5.0).speed(0.02).prefix("max: "));
                ui.add(egui::DragValue::new(&mut p.speed).range(0.1..=10.0).speed(0.05).prefix("hz: "));
            }
        });
        if enabled && pulse.is_none() {
            *pulse = Some(PulseConfig::default());
        } else if !enabled {
            *pulse = None;
        }
    });
}

pub fn shape_ui(ui: &mut egui::Ui, shape: &mut LightShape, id_salt: &str) {
    ui.push_id(format!("shape_{id_salt}"), |ui| {
        let mut kind: u8 = match shape {
            LightShape::Point => 0,
            LightShape::Cone { .. } => 1,
            LightShape::Line { .. } => 2,
            LightShape::Capsule { .. } => 3,
        };
        ui.horizontal(|ui| {
            ui.label("Shape");
            egui::ComboBox::from_id_salt(format!("shp_{id_salt}"))
                .width(80.0)
                .selected_text(match kind {
                    1 => "Cone",
                    2 => "Line",
                    3 => "Capsule",
                    _ => "Point",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut kind, 0, "Point");
                    ui.selectable_value(&mut kind, 1, "Cone");
                    ui.selectable_value(&mut kind, 2, "Line");
                    ui.selectable_value(&mut kind, 3, "Capsule");
                });
        });
        // If kind changed, initialize with defaults
        let current_kind: u8 = match shape {
            LightShape::Point => 0,
            LightShape::Cone { .. } => 1,
            LightShape::Line { .. } => 2,
            LightShape::Capsule { .. } => 3,
        };
        if kind != current_kind {
            *shape = match kind {
                1 => LightShape::Cone { direction: 0.0, angle: std::f32::consts::FRAC_PI_2 },
                2 => LightShape::Line { end_offset: Vec2::new(48.0, 0.0) },
                3 => LightShape::Capsule { direction: 0.0, half_length: 24.0 },
                _ => LightShape::Point,
            };
        }
        // Shape-specific params
        match shape {
            LightShape::Cone { direction, angle } => {
                let mut dir_deg = direction.to_degrees();
                let mut ang_deg = angle.to_degrees();
                ui.add(egui::Slider::new(&mut dir_deg, 0.0..=360.0).text("Direction"));
                ui.add(egui::Slider::new(&mut ang_deg, 1.0..=180.0).text("Cone Angle"));
                *direction = dir_deg.to_radians();
                *angle = ang_deg.to_radians();
            }
            LightShape::Line { end_offset } => {
                ui.horizontal(|ui| {
                    ui.label("End offset");
                    ui.add(egui::DragValue::new(&mut end_offset.x).range(-500.0..=500.0).speed(1.0).prefix("X: "));
                    ui.add(egui::DragValue::new(&mut end_offset.y).range(-500.0..=500.0).speed(1.0).prefix("Y: "));
                });
            }
            LightShape::Capsule { direction, half_length } => {
                let mut dir_deg = direction.to_degrees();
                ui.add(egui::Slider::new(&mut dir_deg, 0.0..=360.0).text("Direction"));
                ui.add(egui::Slider::new(half_length, 1.0..=250.0).text("Half Length"));
                *direction = dir_deg.to_radians();
            }
            LightShape::Point => {}
        }
    });
}

pub fn flicker_ui(ui: &mut egui::Ui, flicker: &mut Option<FlickerConfig>, id_salt: &str) {
    ui.push_id(format!("flicker_{id_salt}"), |ui| {
        let mut enabled = flicker.is_some();
        ui.horizontal(|ui| {
            ui.checkbox(&mut enabled, "Flicker");
            if let Some(f) = flicker.as_mut() {
                ui.add(egui::DragValue::new(&mut f.min_delay).range(0.1..=30.0).speed(0.1).prefix("del: "));
                ui.add(egui::DragValue::new(&mut f.max_delay).range(0.1..=30.0).speed(0.1).prefix("-"));
                ui.add(egui::DragValue::new(&mut f.dip).range(0.0..=1.0).speed(0.02).prefix("dip: "));
            }
        });
        if enabled && flicker.is_none() {
            *flicker = Some(FlickerConfig::default());
        } else if !enabled {
            *flicker = None;
        }
    });
}

// ── World interaction systems ───────────────────────────────────────────────

pub fn place_light_on_click(
    panel: Res<LightingDebugPanel>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<CombatCamera3d>>,
    mut contexts: EguiContexts,
    mut commands: Commands,
) {
    if !panel.placer_active || !mouse.just_pressed(MouseButton::Left) {
        return;
    }

    let Ok(ctx) = contexts.ctx_mut() else { return };
    if ctx.is_pointer_over_area() {
        return;
    }

    let Ok(window) = windows.single() else { return };
    let Some(cursor_pos) = window.cursor_position() else { return };
    let Ok((camera, cam_tf)) = cameras.single() else { return };

    let Ok(ray) = camera.viewport_to_world(cam_tf, cursor_pos) else { return };
    let Some(distance) = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Z)) else {
        return;
    };
    let world_pos = ray.get_point(distance);

    commands.spawn((
        Transform::from_xyz(world_pos.x, world_pos.y, panel.placer_height),
        LightSource {
            color: Color::linear_rgb(
                panel.placer_color[0],
                panel.placer_color[1],
                panel.placer_color[2],
            ),
            base_intensity: panel.placer_intensity,
            intensity: panel.placer_intensity,
            inner_radius: panel.placer_inner_radius,
            outer_radius: panel.placer_outer_radius,
            shape: panel.placer_shape,
            pulse: panel.placer_pulse,
            flicker: panel.placer_flicker,
            anim_seed: rand::random::<f32>() * 100.0,
            ..default()
        },
        DebugLight,
        super::components::InteractiveLight,
    ));
}

/// Draw gizmo icons at each debug light, handle dragging and hover highlight.
pub fn draw_light_gizmos(
    mut panel: ResMut<LightingDebugPanel>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<CombatCamera3d>>,
    mut lights: Query<(Entity, &mut Transform, &LightSource), With<DebugLight>>,
    mut contexts: EguiContexts,
    mut gizmos: Gizmos,
    time: Res<Time>,
) {
    if !panel.open {
        return;
    }

    let Ok(window) = windows.single() else { return };
    let Ok((camera, cam_tf)) = cameras.single() else { return };
    let cursor_pos = window.cursor_position();
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let egui_wants_pointer = ctx.is_pointer_over_area();

    // ── Draw icons and find pick target ─────────────────────────────
    let icon_world_radius = 8.0;
    let pick_screen_radius = 20.0;
    let mut closest_pick: Option<(Entity, f32)> = None;

    for (entity, tf, light) in lights.iter() {
        let world_pos = tf.translation;
        let c = light.color.to_linear();
        let light_color = Color::linear_rgb(c.red, c.green, c.blue);

        let is_hovered = panel.hovered_light == Some(entity);
        let is_dragging = panel.dragging == Some(entity);

        let light_z = world_pos.z.max(2.0);
        let icon_pos = Vec3::new(world_pos.x, world_pos.y, light_z);
        let icon_r = if is_hovered || is_dragging {
            icon_world_radius * 1.5
        } else {
            icon_world_radius
        };
        gizmos.circle(
            Isometry3d::new(icon_pos, Quat::IDENTITY),
            icon_r,
            light_color,
        );
        gizmos.circle(
            Isometry3d::new(icon_pos, Quat::IDENTITY),
            icon_r + 2.0,
            Color::WHITE,
        );
        // Vertical line from ground to light showing height
        if world_pos.z > 1.0 {
            let ground = Vec3::new(world_pos.x, world_pos.y, 0.0);
            gizmos.line(ground, icon_pos, light_color.with_alpha(0.4));
        }

        // Hover highlight: show radius rings
        if is_hovered {
            let pulse = ((time.elapsed_secs() * 3.0).sin() * 0.3 + 0.7).clamp(0.4, 1.0);
            let highlight_color = light_color.with_alpha(pulse);
            gizmos.circle(
                Isometry3d::new(icon_pos, Quat::IDENTITY),
                light.outer_radius,
                highlight_color,
            );
            gizmos.circle(
                Isometry3d::new(icon_pos, Quat::IDENTITY),
                light.inner_radius,
                highlight_color,
            );
        }

        if let Some(cursor) = cursor_pos {
            if let Ok(screen_pos) = camera.world_to_viewport(cam_tf, world_pos) {
                let dist = screen_pos.distance(cursor);
                if dist < pick_screen_radius {
                    if closest_pick.is_none() || dist < closest_pick.unwrap().1 {
                        closest_pick = Some((entity, dist));
                    }
                }
            }
        }
    }

    // ── Drag logic ──────────────────────────────────────────────────
    if panel.placer_active {
        return;
    }

    if mouse.just_pressed(MouseButton::Left) && !egui_wants_pointer {
        if let Some((entity, _)) = closest_pick {
            panel.dragging = Some(entity);
        }
    }

    if mouse.just_released(MouseButton::Left) {
        panel.dragging = None;
    }

    if let Some(drag_entity) = panel.dragging {
        if let Some(cursor) = cursor_pos {
            let Ok(ray) = camera.viewport_to_world(cam_tf, cursor) else { return };
            let Some(dist) = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Z)) else {
                return;
            };
            let world_pos = ray.get_point(dist);

            if let Ok((_, mut tf, _)) = lights.get_mut(drag_entity) {
                tf.translation.x = world_pos.x;
                tf.translation.y = world_pos.y;
            }
        }
    }
}

/// Run condition: returns true when ambient override is NOT active.
pub fn ambient_auto_active(panel: Res<LightingDebugPanel>) -> bool {
    !panel.ambient_override
}

