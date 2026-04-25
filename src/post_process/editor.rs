use bevy::prelude::*;
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::post_process::bloom::{Bloom, BloomCompositeMode};
use bevy::post_process::dof::{DepthOfField, DepthOfFieldMode};
use bevy::post_process::motion_blur::MotionBlur;
use bevy::post_process::effect_stack::ChromaticAberration;
use bevy::anti_alias::contrast_adaptive_sharpening::ContrastAdaptiveSharpening;
use bevy::anti_alias::fxaa::{Fxaa, Sensitivity};
use bevy::render::view::ColorGrading;
use bevy_egui::{EguiContexts, egui};

use super::custom::CustomPostProcess;
use super::shockwave::ShockwaveEmitter;
use crate::camera::CombatCamera3d;

#[derive(Resource)]
pub struct PostProcessEditorState {
    pub open: bool,
    pub shockwave_placer: bool,
    pub sw_radius: f32,
    pub sw_duration: f32,
    pub sw_intensity: f32,
    pub sw_thickness: f32,
    pub sw_chromatic: f32,
}

impl Default for PostProcessEditorState {
    fn default() -> Self {
        Self {
            open: false,
            shockwave_placer: false,
            sw_radius: 200.0,
            sw_duration: 0.8,
            sw_intensity: 0.04,
            sw_thickness: 40.0,
            sw_chromatic: 0.005,
        }
    }
}

pub fn toggle_editor(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<PostProcessEditorState>,
) {
    if keyboard.just_pressed(KeyCode::F8) {
        state.open = !state.open;
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn post_process_editor_ui(
    mut state: ResMut<PostProcessEditorState>,
    mut contexts: EguiContexts,
    camera_q: Query<(&Camera, &GlobalTransform, Entity), With<CombatCamera3d>>,
    mut bloom_q: Query<&mut Bloom>,
    mut tonemapping_q: Query<&mut Tonemapping, With<CombatCamera3d>>,
    mut dither_q: Query<&mut DebandDither, With<CombatCamera3d>>,
    mut color_grading_q: Query<&mut ColorGrading, With<CombatCamera3d>>,
    mut chromatic_q: Query<&mut ChromaticAberration>,
    mut dof_q: Query<&mut DepthOfField>,
    mut motion_blur_q: Query<&mut MotionBlur>,
    mut fxaa_q: Query<&mut Fxaa>,
    mut cas_q: Query<&mut ContrastAdaptiveSharpening>,
    mut custom_q: Query<&mut CustomPostProcess>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    mut commands: Commands,
) {
    if !state.open {
        return;
    }
    let Ok((camera, cam_tf, cam_entity)) = camera_q.single() else { return };
    let Ok(ctx) = contexts.ctx_mut() else { return };

    // ── Shockwave click-to-place (before UI consumes pointer) ────
    if state.shockwave_placer && mouse.just_pressed(MouseButton::Left) && !ctx.is_pointer_over_area() {
        if let Ok(window) = windows.single() {
            if let Some(cursor_pos) = window.cursor_position() {
                if let Ok(ray) = camera.viewport_to_world(cam_tf, cursor_pos) {
                    if let Some(dist) = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Z)) {
                        let world_pos = ray.get_point(dist);
                        commands.spawn(ShockwaveEmitter {
                            center: Vec2::new(world_pos.x, world_pos.y),
                            max_radius: state.sw_radius,
                            duration: state.sw_duration,
                            intensity: state.sw_intensity,
                            thickness: state.sw_thickness,
                            chromatic: state.sw_chromatic,
                            elapsed: 0.0,
                        });
                    }
                }
            }
        }
    }

    egui::Window::new("Post Processing (F8)")
        .default_width(340.0)
        .vscroll(true)
        .show(ctx, |ui| {
            // ── Custom FX (uber-shader) ────────────────────────────
            custom_fx_section(ui, &mut custom_q, cam_entity, &mut commands);
            ui.separator();

            // ── Bloom ──────────────────────────────────────────────
            bloom_section(ui, &mut bloom_q, cam_entity, &mut commands);
            ui.separator();

            // ── Tonemapping ────────────────────────────────────────
            tonemapping_section(ui, &mut tonemapping_q, &mut dither_q);
            ui.separator();

            // ── Color Grading ──────────────────────────────────────
            color_grading_section(ui, &mut color_grading_q);
            ui.separator();

            // ── Chromatic Aberration ───────────────────────────────
            chromatic_section(ui, &mut chromatic_q, cam_entity, &mut commands);
            ui.separator();

            // ── Depth of Field ─────────────────────────────────────
            dof_section(ui, &mut dof_q, cam_entity, &mut commands);
            ui.separator();

            // ── Motion Blur ────────────────────────────────────────
            motion_blur_section(ui, &mut motion_blur_q, cam_entity, &mut commands);
            ui.separator();

            // ── Anti-Aliasing ──────────────────────────────────────
            fxaa_section(ui, &mut fxaa_q, cam_entity, &mut commands);
            ui.separator();

            // ── Sharpening ─────────────────────────────────────────
            cas_section(ui, &mut cas_q, cam_entity, &mut commands);
            ui.separator();

            // ── Shockwave ──────────────────────────────────────────
            ui.heading("Shockwave");
            let place_label = if state.shockwave_placer { "Placing (click map)" } else { "Enable Placer" };
            ui.toggle_value(&mut state.shockwave_placer, place_label);
            ui.add(egui::Slider::new(&mut state.sw_radius, 10.0..=1000.0).text("Radius"));
            ui.add(egui::Slider::new(&mut state.sw_duration, 0.1..=5.0).text("Duration"));
            ui.add(egui::Slider::new(&mut state.sw_intensity, 0.001..=0.2).text("Intensity"));
            ui.add(egui::Slider::new(&mut state.sw_thickness, 5.0..=200.0).text("Thickness"));
            ui.add(egui::Slider::new(&mut state.sw_chromatic, 0.0..=0.05).text("Chromatic"));
        });
}

// ── Custom FX ──────────────────────────────────────────────────────────────

fn custom_fx_section(
    ui: &mut egui::Ui,
    custom_q: &mut Query<&mut CustomPostProcess>,
    cam_entity: Entity,
    commands: &mut Commands,
) {
    ui.heading("Custom FX");
    let Ok(mut pp) = custom_q.single_mut() else {
        if ui.button("Enable Custom FX").clicked() {
            commands.entity(cam_entity).insert(CustomPostProcess::enabled());
        }
        return;
    };

    let mut master = pp.time_resolution.w > 0.5;
    ui.checkbox(&mut master, "Master Enable");
    pp.time_resolution.w = if master { 1.0 } else { 0.0 };

    // Vignette
    ui.collapsing("Vignette", |ui| {
        ui.add(egui::Slider::new(&mut pp.vignette_color.w, 0.0..=1.0).text("Intensity"));
        ui.add(egui::Slider::new(&mut pp.vignette_params.x, 0.0..=1.0).text("Smoothness"));
        ui.add(egui::Slider::new(&mut pp.vignette_params.y, 0.2..=3.0).text("Roundness"));
        let mut color = [pp.vignette_color.x, pp.vignette_color.y, pp.vignette_color.z];
        ui.horizontal(|ui| {
            ui.label("Color");
            egui::color_picker::color_edit_button_rgb(ui, &mut color);
        });
        pp.vignette_color.x = color[0];
        pp.vignette_color.y = color[1];
        pp.vignette_color.z = color[2];
    });

    // Pixelation
    ui.collapsing("Pixelation", |ui| {
        let mut enabled = pp.pixelation_params.z > 0.5;
        ui.checkbox(&mut enabled, "Enabled");
        pp.pixelation_params.z = if enabled { 1.0 } else { 0.0 };
        ui.add(egui::Slider::new(&mut pp.pixelation_params.x, 1.0..=32.0).text("Cell Width"));
        ui.add(egui::Slider::new(&mut pp.pixelation_params.y, 1.0..=32.0).text("Cell Height"));
        if ui.button("Link W=H").clicked() {
            pp.pixelation_params.y = pp.pixelation_params.x;
        }
    });

    // Scanlines
    ui.collapsing("Scanlines", |ui| {
        ui.add(egui::Slider::new(&mut pp.scanline_params.x, 0.0..=1.0).text("Intensity"));
        ui.add(egui::Slider::new(&mut pp.scanline_params.y, 50.0..=2000.0).text("Count"));
        ui.add(egui::Slider::new(&mut pp.scanline_params.z, 0.0..=20.0).text("Speed"));
    });

    // Film Grain
    ui.collapsing("Film Grain", |ui| {
        ui.add(egui::Slider::new(&mut pp.grain_params.x, 0.0..=0.5).text("Intensity"));
        ui.add(egui::Slider::new(&mut pp.grain_params.y, 0.1..=5.0).text("Speed"));
    });

    // Color Tint
    ui.collapsing("Color Tint", |ui| {
        ui.add(egui::Slider::new(&mut pp.color_tint.w, 0.0..=1.0).text("Intensity"));
        let mut color = [pp.color_tint.x, pp.color_tint.y, pp.color_tint.z];
        ui.horizontal(|ui| {
            ui.label("Tint");
            egui::color_picker::color_edit_button_rgb(ui, &mut color);
        });
        pp.color_tint.x = color[0];
        pp.color_tint.y = color[1];
        pp.color_tint.z = color[2];
    });

    // Brightness / Contrast / Saturation
    ui.collapsing("Brightness / Contrast / Saturation", |ui| {
        ui.add(egui::Slider::new(&mut pp.misc_params.y, -1.0..=1.0).text("Brightness"));
        ui.add(egui::Slider::new(&mut pp.misc_params.z, 0.0..=3.0).text("Contrast"));
        ui.add(egui::Slider::new(&mut pp.misc_params.w, 0.0..=3.0).text("Saturation"));
        if ui.button("Reset").clicked() {
            pp.misc_params.y = 0.0;
            pp.misc_params.z = 1.0;
            pp.misc_params.w = 1.0;
        }
    });

    // Invert
    ui.collapsing("Invert", |ui| {
        ui.add(egui::Slider::new(&mut pp.misc_params.x, 0.0..=1.0).text("Amount"));
    });

    // Sine Wave
    ui.collapsing("Sine Wave", |ui| {
        ui.add(egui::Slider::new(&mut pp.sine_wave.x, 0.0..=0.1).text("Amplitude X"));
        ui.add(egui::Slider::new(&mut pp.sine_wave.y, 0.0..=0.1).text("Amplitude Y"));
        ui.add(egui::Slider::new(&mut pp.sine_wave.z, 1.0..=100.0).text("Frequency"));
        ui.add(egui::Slider::new(&mut pp.sine_wave.w, 0.0..=20.0).text("Speed"));
        if ui.button("Reset").clicked() {
            pp.sine_wave = Vec4::ZERO;
        }
    });

    // Swirl
    ui.collapsing("Swirl", |ui| {
        ui.add(egui::Slider::new(&mut pp.swirl.x, -12.0..=12.0).text("Angle (rad)"));
        ui.add(egui::Slider::new(&mut pp.swirl.y, 0.01..=1.5).text("Radius"));
        ui.add(egui::Slider::new(&mut pp.swirl.z, 0.0..=1.0).text("Center X"));
        ui.add(egui::Slider::new(&mut pp.swirl.w, 0.0..=1.0).text("Center Y"));
        if ui.button("Center").clicked() {
            pp.swirl.z = 0.5;
            pp.swirl.w = 0.5;
        }
        if ui.button("Reset").clicked() {
            pp.swirl = Vec4::ZERO;
        }
    });

    // Lens Distortion
    ui.collapsing("Lens Distortion", |ui| {
        ui.add(egui::Slider::new(&mut pp.distortion_shake.x, -2.0..=2.0).text("Intensity"));
        ui.add(egui::Slider::new(&mut pp.distortion_shake.y, 0.5..=2.0).text("Zoom Comp."));
        if ui.button("Reset").clicked() {
            pp.distortion_shake.x = 0.0;
            pp.distortion_shake.y = 1.0;
        }
    });

    // Screen Shake
    ui.collapsing("Screen Shake (UV)", |ui| {
        ui.add(egui::Slider::new(&mut pp.distortion_shake.z, 0.0..=0.1).text("Intensity"));
        ui.add(egui::Slider::new(&mut pp.distortion_shake.w, 0.1..=10.0).text("Speed"));
        if ui.button("Reset").clicked() {
            pp.distortion_shake.z = 0.0;
            pp.distortion_shake.w = 0.0;
        }
    });

    // Zoom
    ui.collapsing("Zoom", |ui| {
        ui.add(egui::Slider::new(&mut pp.zoom_rotation.x, 0.1..=5.0).text("Amount"));
        if ui.button("Reset (1.0)").clicked() {
            pp.zoom_rotation.x = 1.0;
        }
    });

    // Rotation
    ui.collapsing("Rotation", |ui| {
        ui.add(egui::Slider::new(&mut pp.zoom_rotation.y, -6.28..=6.28).text("Angle (rad)"));
        if ui.button("Reset").clicked() {
            pp.zoom_rotation.y = 0.0;
        }
    });

    // Posterization
    ui.collapsing("Posterize", |ui| {
        ui.add(egui::Slider::new(&mut pp.zoom_rotation.z, 0.0..=32.0).text("Levels (0=off)"));
        if ui.button("Reset").clicked() {
            pp.zoom_rotation.z = 0.0;
        }
    });

    // Cinema Bars
    ui.collapsing("Cinema Bars", |ui| {
        ui.add(egui::Slider::new(&mut pp.zoom_rotation.w, 0.0..=0.3).text("Bar Size"));
        let mut color = [pp.cinema_bar_color.x, pp.cinema_bar_color.y, pp.cinema_bar_color.z];
        ui.horizontal(|ui| {
            ui.label("Color");
            egui::color_picker::color_edit_button_rgb(ui, &mut color);
        });
        pp.cinema_bar_color.x = color[0];
        pp.cinema_bar_color.y = color[1];
        pp.cinema_bar_color.z = color[2];
        if ui.button("Reset").clicked() {
            pp.zoom_rotation.w = 0.0;
        }
    });

    // Fade
    ui.collapsing("Fade", |ui| {
        ui.add(egui::Slider::new(&mut pp.fade_color.w, 0.0..=1.0).text("Intensity"));
        let mut color = [pp.fade_color.x, pp.fade_color.y, pp.fade_color.z];
        ui.horizontal(|ui| {
            ui.label("Color");
            egui::color_picker::color_edit_button_rgb(ui, &mut color);
        });
        pp.fade_color.x = color[0];
        pp.fade_color.y = color[1];
        pp.fade_color.z = color[2];
    });

    if ui.button("Disable Custom FX").clicked() {
        commands.entity(cam_entity).remove::<CustomPostProcess>();
    }
}

// ── Bloom ──────────────────────────────────────────────────────────────────

fn bloom_section(
    ui: &mut egui::Ui,
    bloom_q: &mut Query<&mut Bloom>,
    cam_entity: Entity,
    commands: &mut Commands,
) {
    ui.heading("Bloom");
    let Ok(mut bloom) = bloom_q.single_mut() else {
        ui.horizontal(|ui| {
            if ui.button("Natural").clicked() {
                commands.entity(cam_entity).insert(Bloom::NATURAL);
            }
            if ui.button("Old School").clicked() {
                commands.entity(cam_entity).insert(Bloom::OLD_SCHOOL);
            }
            if ui.button("Screen Blur").clicked() {
                commands.entity(cam_entity).insert(Bloom::SCREEN_BLUR);
            }
        });
        return;
    };

    ui.add(egui::Slider::new(&mut bloom.intensity, 0.0..=1.0).text("Intensity"));
    ui.add(egui::Slider::new(&mut bloom.low_frequency_boost, 0.0..=1.0).text("LF Boost"));
    ui.add(egui::Slider::new(&mut bloom.low_frequency_boost_curvature, 0.0..=1.0).text("LF Curvature"));
    ui.add(egui::Slider::new(&mut bloom.high_pass_frequency, 0.0..=1.0).text("HP Frequency"));
    ui.add(egui::Slider::new(&mut bloom.prefilter.threshold, 0.0..=4.0).text("Threshold"));
    ui.add(egui::Slider::new(&mut bloom.prefilter.threshold_softness, 0.0..=1.0).text("Threshold Softness"));

    let mut mode_idx: usize = match bloom.composite_mode {
        BloomCompositeMode::EnergyConserving => 0,
        BloomCompositeMode::Additive => 1,
    };
    egui::ComboBox::from_label("Composite")
        .selected_text(match mode_idx { 0 => "Energy Conserving", _ => "Additive" })
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut mode_idx, 0, "Energy Conserving");
            ui.selectable_value(&mut mode_idx, 1, "Additive");
        });
    bloom.composite_mode = match mode_idx {
        0 => BloomCompositeMode::EnergyConserving,
        _ => BloomCompositeMode::Additive,
    };

    ui.horizontal(|ui| {
        if ui.button("Natural").clicked() { *bloom = Bloom::NATURAL; }
        if ui.button("Old School").clicked() { *bloom = Bloom::OLD_SCHOOL; }
        if ui.button("Disable").clicked() {
            commands.entity(cam_entity).remove::<Bloom>();
        }
    });
}

// ── Tonemapping ────────────────────────────────────────────────────────────

fn tonemapping_section(
    ui: &mut egui::Ui,
    tonemapping_q: &mut Query<&mut Tonemapping, With<CombatCamera3d>>,
    dither_q: &mut Query<&mut DebandDither, With<CombatCamera3d>>,
) {
    ui.heading("Tonemapping");
    if let Ok(mut tm) = tonemapping_q.single_mut() {
        let options = [
            ("None", Tonemapping::None),
            ("Reinhard", Tonemapping::Reinhard),
            ("Reinhard Luminance", Tonemapping::ReinhardLuminance),
            ("ACES Fitted", Tonemapping::AcesFitted),
            ("AgX", Tonemapping::AgX),
            ("SomewhatBoring", Tonemapping::SomewhatBoringDisplayTransform),
            ("TonyMcMapface", Tonemapping::TonyMcMapface),
            ("Blender Filmic", Tonemapping::BlenderFilmic),
        ];
        let current = options.iter().position(|(_, v)| *v == *tm).unwrap_or(0);
        let label = options[current].0;
        egui::ComboBox::from_label("Algorithm")
            .selected_text(label)
            .show_ui(ui, |ui| {
                for (name, val) in &options {
                    if ui.selectable_label(*val == *tm, *name).clicked() {
                        *tm = *val;
                    }
                }
            });
    }

    if let Ok(tm) = tonemapping_q.single_mut() {
        if *tm == Tonemapping::None {
            ui.colored_label(egui::Color32::YELLOW, "Color grading disabled when None");
        }
    }

    if let Ok(mut dither) = dither_q.single_mut() {
        let mut enabled = matches!(*dither, DebandDither::Enabled);
        ui.checkbox(&mut enabled, "Deband Dither");
        *dither = if enabled { DebandDither::Enabled } else { DebandDither::Disabled };
    }
}

// ── Color Grading ──────────────────────────────────────────────────────────

fn color_grading_section(
    ui: &mut egui::Ui,
    cg_q: &mut Query<&mut ColorGrading, With<CombatCamera3d>>,
) {
    ui.heading("Color Grading");
    let Ok(mut cg) = cg_q.single_mut() else { return };
    // Deref through Mut<> so the borrow checker can see disjoint field borrows.
    let cg = &mut *cg;

    ui.collapsing("Global", |ui| {
        ui.add(egui::Slider::new(&mut cg.global.exposure, -8.0..=8.0).text("Exposure (EV)"));
        ui.add(egui::Slider::new(&mut cg.global.temperature, -3.0..=3.0).text("Temperature"));
        ui.add(egui::Slider::new(&mut cg.global.tint, -3.0..=3.0).text("Tint"));
        ui.add(egui::Slider::new(&mut cg.global.hue, -3.14..=3.14).text("Hue"));
        ui.add(egui::Slider::new(&mut cg.global.post_saturation, 0.0..=3.0).text("Post Saturation"));
        ui.add(egui::Slider::new(&mut cg.global.midtones_range.start, 0.0..=0.5).text("Midtone Start"));
        ui.add(egui::Slider::new(&mut cg.global.midtones_range.end, 0.5..=1.0).text("Midtone End"));
        if ui.button("Reset").clicked() {
            cg.global = default();
        }
    });

    color_grading_section_ui(ui, "Shadows", &mut cg.shadows);
    color_grading_section_ui(ui, "Midtones", &mut cg.midtones);
    color_grading_section_ui(ui, "Highlights", &mut cg.highlights);
}

fn color_grading_section_ui(
    ui: &mut egui::Ui,
    label: &str,
    section: &mut bevy::render::view::ColorGradingSection,
) {
    ui.collapsing(label, |ui| {
        ui.add(egui::Slider::new(&mut section.saturation, 0.0..=4.0).text("Saturation"));
        ui.add(egui::Slider::new(&mut section.contrast, 0.0..=4.0).text("Contrast"));
        ui.add(egui::Slider::new(&mut section.gamma, 0.01..=5.0).logarithmic(true).text("Gamma"));
        ui.add(egui::Slider::new(&mut section.gain, 0.0..=5.0).text("Gain"));
        ui.add(egui::Slider::new(&mut section.lift, -1.0..=1.0).text("Lift"));
        if ui.button("Reset").clicked() {
            *section = default();
        }
    });
}

// ── Chromatic Aberration ───────────────────────────────────────────────────

fn chromatic_section(
    ui: &mut egui::Ui,
    chromatic_q: &mut Query<&mut ChromaticAberration>,
    cam_entity: Entity,
    commands: &mut Commands,
) {
    ui.heading("Chromatic Aberration");
    let Ok(mut ca) = chromatic_q.single_mut() else {
        if ui.button("Enable").clicked() {
            commands.entity(cam_entity).insert(ChromaticAberration {
                intensity: 0.02,
                max_samples: 8,
                ..default()
            });
        }
        return;
    };

    ui.add(egui::Slider::new(&mut ca.intensity, 0.0..=0.2).text("Intensity"));
    let mut samples = ca.max_samples as i32;
    ui.add(egui::Slider::new(&mut samples, 1..=32).text("Max Samples"));
    ca.max_samples = samples as u32;

    if ui.button("Disable").clicked() {
        commands.entity(cam_entity).remove::<ChromaticAberration>();
    }
}

// ── Depth of Field ─────────────────────────────────────────────────────────

fn dof_section(
    ui: &mut egui::Ui,
    dof_q: &mut Query<&mut DepthOfField>,
    cam_entity: Entity,
    commands: &mut Commands,
) {
    ui.heading("Depth of Field");
    let Ok(mut dof) = dof_q.single_mut() else {
        if ui.button("Enable (Gaussian)").clicked() {
            commands.entity(cam_entity).insert(DepthOfField {
                mode: DepthOfFieldMode::Gaussian,
                focal_distance: 900.0,
                aperture_f_stops: 2.0,
                max_circle_of_confusion_diameter: 64.0,
                max_depth: 3000.0,
                ..default()
            });
        }
        return;
    };

    let mut mode_idx: usize = match dof.mode {
        DepthOfFieldMode::Gaussian => 0,
        DepthOfFieldMode::Bokeh => 1,
    };
    egui::ComboBox::from_label("DoF Mode")
        .selected_text(match mode_idx { 0 => "Gaussian", _ => "Bokeh" })
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut mode_idx, 0, "Gaussian");
            ui.selectable_value(&mut mode_idx, 1, "Bokeh");
        });
    dof.mode = match mode_idx { 0 => DepthOfFieldMode::Gaussian, _ => DepthOfFieldMode::Bokeh };

    ui.add(egui::Slider::new(&mut dof.focal_distance, 1.0..=3000.0).logarithmic(true).text("Focal Distance"));
    ui.add(egui::Slider::new(&mut dof.sensor_height, 1.0..=100.0).text("Sensor Height"));
    ui.add(egui::Slider::new(&mut dof.aperture_f_stops, 0.5..=32.0).logarithmic(true).text("F-Stops"));
    ui.add(egui::Slider::new(&mut dof.max_circle_of_confusion_diameter, 1.0..=256.0).text("Max CoC"));
    ui.add(egui::Slider::new(&mut dof.max_depth, 100.0..=10000.0).logarithmic(true).text("Max Depth"));

    if ui.button("Disable").clicked() {
        commands.entity(cam_entity).remove::<DepthOfField>();
    }
}

// ── Motion Blur ────────────────────────────────────────────────────────────

fn motion_blur_section(
    ui: &mut egui::Ui,
    mb_q: &mut Query<&mut MotionBlur>,
    cam_entity: Entity,
    commands: &mut Commands,
) {
    ui.heading("Motion Blur");
    let Ok(mut mb) = mb_q.single_mut() else {
        if ui.button("Enable").clicked() {
            commands.entity(cam_entity).insert(MotionBlur {
                shutter_angle: 0.5,
                samples: 1,
            });
        }
        return;
    };

    ui.add(egui::Slider::new(&mut mb.shutter_angle, 0.0..=2.0).text("Shutter Angle"));
    let mut samples = mb.samples as i32;
    ui.add(egui::Slider::new(&mut samples, 1..=16).text("Samples"));
    mb.samples = samples as u32;

    if ui.button("Disable").clicked() {
        commands.entity(cam_entity).remove::<MotionBlur>();
    }
}

// ── FXAA ───────────────────────────────────────────────────────────────────

fn fxaa_section(
    ui: &mut egui::Ui,
    fxaa_q: &mut Query<&mut Fxaa>,
    cam_entity: Entity,
    commands: &mut Commands,
) {
    ui.heading("FXAA");
    let Ok(mut fxaa) = fxaa_q.single_mut() else {
        if ui.button("Enable FXAA").clicked() {
            commands.entity(cam_entity).insert(Fxaa::default());
        }
        return;
    };

    ui.checkbox(&mut fxaa.enabled, "Active");

    let sensitivities = [
        ("Low", Sensitivity::Low),
        ("Medium", Sensitivity::Medium),
        ("High", Sensitivity::High),
        ("Ultra", Sensitivity::Ultra),
        ("Extreme", Sensitivity::Extreme),
    ];

    let current_edge = sensitivities.iter().position(|(_, v)| *v == fxaa.edge_threshold).unwrap_or(2);
    egui::ComboBox::from_label("Edge Threshold")
        .selected_text(sensitivities[current_edge].0)
        .show_ui(ui, |ui| {
            for (name, val) in &sensitivities {
                if ui.selectable_label(*val == fxaa.edge_threshold, *name).clicked() {
                    fxaa.edge_threshold = *val;
                }
            }
        });

    let current_min = sensitivities.iter().position(|(_, v)| *v == fxaa.edge_threshold_min).unwrap_or(0);
    egui::ComboBox::from_label("Edge Threshold Min")
        .selected_text(sensitivities[current_min].0)
        .show_ui(ui, |ui| {
            for (name, val) in &sensitivities {
                if ui.selectable_label(*val == fxaa.edge_threshold_min, *name).clicked() {
                    fxaa.edge_threshold_min = *val;
                }
            }
        });

    if ui.button("Disable FXAA").clicked() {
        commands.entity(cam_entity).remove::<Fxaa>();
    }
}

// ── Contrast Adaptive Sharpening ───────────────────────────────────────────

fn cas_section(
    ui: &mut egui::Ui,
    cas_q: &mut Query<&mut ContrastAdaptiveSharpening>,
    cam_entity: Entity,
    commands: &mut Commands,
) {
    ui.heading("Sharpening (CAS)");
    let Ok(mut cas) = cas_q.single_mut() else {
        if ui.button("Enable").clicked() {
            commands.entity(cam_entity).insert(ContrastAdaptiveSharpening {
                enabled: true,
                sharpening_strength: 0.6,
                denoise: false,
            });
        }
        return;
    };

    ui.checkbox(&mut cas.enabled, "Active");
    ui.add(egui::Slider::new(&mut cas.sharpening_strength, 0.0..=1.0).text("Strength"));
    ui.checkbox(&mut cas.denoise, "Denoise");

    if ui.button("Disable").clicked() {
        commands.entity(cam_entity).remove::<ContrastAdaptiveSharpening>();
    }
}
