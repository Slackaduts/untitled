use bevy::prelude::*;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::math::curve::Curve;
use bevy::post_process::bloom::Bloom;
use bevy::post_process::dof::{DepthOfField, DepthOfFieldMode};
use bevy::post_process::effect_stack::ChromaticAberration;
use bevy::render::view::ColorGrading;

use crate::camera::CombatCamera3d;
use crate::entity::movement::parse_ease_function;
use super::custom::CustomPostProcess;

// ── Transition data ────────────────────────────────────────────────────────

/// What kind of post-fx to transition, with start and target values.
#[derive(Clone, Debug)]
pub enum FxKind {
    Bloom { start: [f32; 3], target: [f32; 3] },                 // intensity, threshold, softness
    ResetBloom { start: [f32; 3] },                               // fade to zero then remove
    ColorGrading { start: [f32; 5], target: [f32; 5] },           // exposure, temp, tint, hue, post_sat
    ResetColorGrading { start: [f32; 5] },
    ChromaticAberration { start: f32, target: f32 },
    ResetChromaticAberration { start: f32 },
    Dof { start: [f32; 2], target: [f32; 2] },                    // focal_dist, aperture
    ResetDof { start: [f32; 2] },
    Vignette { start: [f32; 6], target: [f32; 6] },               // intensity, smooth, round, r, g, b
    Scanlines { start: [f32; 3], target: [f32; 3] },              // intensity, count, speed
    FilmGrain { start: [f32; 2], target: [f32; 2] },              // intensity, speed
    Fade { start: [f32; 4], target: [f32; 4] },                   // r, g, b, intensity
    Pixelation { start: f32, target: f32 },                       // cell_size (0 = disabled)
    ColorTint { start: [f32; 4], target: [f32; 4] },              // r, g, b, intensity
    ColorAdjust { start: [f32; 4], target: [f32; 4] },            // invert, brightness, contrast, sat
    SineWave { start: [f32; 4], target: [f32; 4] },              // amp_x, amp_y, freq, speed
    Swirl { start: [f32; 4], target: [f32; 4] },                 // angle, radius, center_x, center_y
    LensDistortion { start: [f32; 2], target: [f32; 2] },        // intensity, zoom
    Shake { start: [f32; 2], target: [f32; 2] },                 // intensity, speed
    Zoom { start: f32, target: f32 },
    Rotation { start: f32, target: f32 },
    CinemaBars { start: [f32; 4], target: [f32; 4] },            // size, r, g, b
    Posterize { start: f32, target: f32 },                        // levels (0=off)
    ResetCustomFx { start_vignette: [f32; 6], start_scanlines: [f32; 3],
                    start_grain: [f32; 2], start_fade: [f32; 4],
                    start_pixel: f32, start_tint: [f32; 4],
                    start_adjust: [f32; 4],
                    start_sine: [f32; 4], start_swirl: [f32; 4],
                    start_lens: [f32; 2], start_shake: [f32; 2],
                    start_zoom: f32, start_rotation: f32,
                    start_cinema: [f32; 4], start_posterize: f32 },
    Tonemapping(Tonemapping),                                     // instant, no lerp
}

#[derive(Clone, Debug)]
pub struct FxTransition {
    pub kind: FxKind,
    pub duration: f32,
    pub elapsed: f32,
    pub ease: EaseFunction,
}

/// Active post-fx transitions, ticked each frame.
#[derive(Resource, Default)]
pub struct FxTransitions {
    pub active: Vec<FxTransition>,
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn lerp_f32(a: f32, b: f32, t: f32) -> f32 { a + (b - a) * t }

fn lerp_arr<const N: usize>(a: &[f32; N], b: &[f32; N], t: f32) -> [f32; N] {
    let mut out = *a;
    for i in 0..N { out[i] = lerp_f32(a[i], b[i], t); }
    out
}

// ── Public API for creating transitions ────────────────────────────────────

impl FxTransitions {
    pub fn push(&mut self, kind: FxKind, duration: f32, easing: &str) {
        // Instant transitions: apply immediately by setting elapsed = duration
        if duration <= 0.0 {
            self.active.push(FxTransition {
                kind,
                duration: 0.0,
                elapsed: 0.0,
                ease: EaseFunction::Linear,
            });
            return;
        }
        self.active.push(FxTransition {
            kind,
            duration,
            elapsed: 0.0,
            ease: parse_ease_function(easing),
        });
    }
}

// ── Capture current values ─────────────────────────────────────────────────

pub fn capture_bloom(bloom: Option<&Bloom>) -> [f32; 3] {
    bloom.map_or([0.0; 3], |b| [b.intensity, b.prefilter.threshold, b.prefilter.threshold_softness])
}

pub fn capture_color_grading(cg: &ColorGrading) -> [f32; 5] {
    [cg.global.exposure, cg.global.temperature, cg.global.tint, cg.global.hue, cg.global.post_saturation]
}

pub fn capture_chromatic(ca: Option<&ChromaticAberration>) -> f32 {
    ca.map_or(0.0, |c| c.intensity)
}

pub fn capture_dof(dof: Option<&DepthOfField>) -> [f32; 2] {
    dof.map_or([900.0, 2.0], |d| [d.focal_distance, d.aperture_f_stops])
}

pub fn capture_vignette(pp: &CustomPostProcess) -> [f32; 6] {
    [pp.vignette_color.w, pp.vignette_params.x, pp.vignette_params.y,
     pp.vignette_color.x, pp.vignette_color.y, pp.vignette_color.z]
}

pub fn capture_scanlines(pp: &CustomPostProcess) -> [f32; 3] {
    [pp.scanline_params.x, pp.scanline_params.y, pp.scanline_params.z]
}

pub fn capture_grain(pp: &CustomPostProcess) -> [f32; 2] {
    [pp.grain_params.x, pp.grain_params.y]
}

pub fn capture_fade(pp: &CustomPostProcess) -> [f32; 4] {
    [pp.fade_color.x, pp.fade_color.y, pp.fade_color.z, pp.fade_color.w]
}

pub fn capture_pixelation(pp: &CustomPostProcess) -> f32 {
    if pp.pixelation_params.z > 0.5 { pp.pixelation_params.x } else { 0.0 }
}

pub fn capture_tint(pp: &CustomPostProcess) -> [f32; 4] {
    [pp.color_tint.x, pp.color_tint.y, pp.color_tint.z, pp.color_tint.w]
}

pub fn capture_adjust(pp: &CustomPostProcess) -> [f32; 4] {
    [pp.misc_params.x, pp.misc_params.y, pp.misc_params.z, pp.misc_params.w]
}

pub fn capture_sine_wave(pp: &CustomPostProcess) -> [f32; 4] {
    [pp.sine_wave.x, pp.sine_wave.y, pp.sine_wave.z, pp.sine_wave.w]
}

pub fn capture_swirl(pp: &CustomPostProcess) -> [f32; 4] {
    [pp.swirl.x, pp.swirl.y, pp.swirl.z, pp.swirl.w]
}

pub fn capture_lens_distortion(pp: &CustomPostProcess) -> [f32; 2] {
    [pp.distortion_shake.x, pp.distortion_shake.y]
}

pub fn capture_shake(pp: &CustomPostProcess) -> [f32; 2] {
    [pp.distortion_shake.z, pp.distortion_shake.w]
}

pub fn capture_zoom(pp: &CustomPostProcess) -> f32 {
    pp.zoom_rotation.x
}

pub fn capture_rotation(pp: &CustomPostProcess) -> f32 {
    pp.zoom_rotation.y
}

pub fn capture_cinema_bars(pp: &CustomPostProcess) -> [f32; 4] {
    [pp.zoom_rotation.w, pp.cinema_bar_color.x, pp.cinema_bar_color.y, pp.cinema_bar_color.z]
}

pub fn capture_posterize(pp: &CustomPostProcess) -> f32 {
    pp.zoom_rotation.z
}

// ── Tick system ────────────────────────────────────────────────────────────

pub fn tick_fx_transitions(
    time: Res<Time>,
    mut transitions: ResMut<FxTransitions>,
    mut cam_q: Query<
        (Entity, &mut CustomPostProcess, &mut ColorGrading,
         Option<&mut Bloom>, Option<&mut ChromaticAberration>,
         Option<&mut DepthOfField>),
        With<CombatCamera3d>,
    >,
    mut tonemapping_q: Query<&mut Tonemapping, With<CombatCamera3d>>,
    mut commands: Commands,
) {
    if transitions.active.is_empty() { return; }
    let dt = time.delta_secs();
    let Ok((cam_entity, mut pp, mut cg, mut bloom, mut chromatic, mut dof)) = cam_q.single_mut() else {
        return;
    };

    let mut completed = Vec::new();

    for (i, tr) in transitions.active.iter_mut().enumerate() {
        tr.elapsed += dt;
        let raw_t = if tr.duration > 0.0 { (tr.elapsed / tr.duration).min(1.0) } else { 1.0 };
        let t = tr.ease.sample_unchecked(raw_t);
        let done = raw_t >= 1.0;

        match &tr.kind {
            // ── Tonemapping (instant) ──
            FxKind::Tonemapping(algo) => {
                if let Ok(mut tm) = tonemapping_q.single_mut() {
                    *tm = *algo;
                }
                completed.push(i);
                continue;
            }

            // ── Bloom ──
            FxKind::Bloom { start, target } => {
                let v = lerp_arr(start, target, t);
                if let Some(ref mut bloom) = bloom {
                    bloom.intensity = v[0];
                    bloom.prefilter.threshold = v[1];
                    bloom.prefilter.threshold_softness = v[2];
                } else {
                    commands.entity(cam_entity).insert(Bloom {
                        intensity: v[0],
                        prefilter: bevy::post_process::bloom::BloomPrefilter {
                            threshold: v[1], threshold_softness: v[2],
                        },
                        ..Bloom::NATURAL
                    });
                }
            }
            FxKind::ResetBloom { start } => {
                let v = lerp_arr(start, &[0.0; 3], t);
                if let Some(ref mut bloom) = bloom {
                    bloom.intensity = v[0];
                    bloom.prefilter.threshold = v[1];
                    bloom.prefilter.threshold_softness = v[2];
                    if done {
                        commands.entity(cam_entity).remove::<Bloom>();
                    }
                }
            }

            // ── Color Grading ──
            FxKind::ColorGrading { start, target } => {
                let v = lerp_arr(start, target, t);
                cg.global.exposure = v[0];
                cg.global.temperature = v[1];
                cg.global.tint = v[2];
                cg.global.hue = v[3];
                cg.global.post_saturation = v[4];
            }
            FxKind::ResetColorGrading { start } => {
                let defaults = [0.0, 0.0, 0.0, 0.0, 1.0];
                let v = lerp_arr(start, &defaults, t);
                cg.global.exposure = v[0];
                cg.global.temperature = v[1];
                cg.global.tint = v[2];
                cg.global.hue = v[3];
                cg.global.post_saturation = v[4];
            }

            // ── Chromatic Aberration ──
            FxKind::ChromaticAberration { start, target } => {
                let v = lerp_f32(*start, *target, t);
                if let Some(ref mut ca) = chromatic {
                    ca.intensity = v;
                } else {
                    commands.entity(cam_entity).insert(ChromaticAberration {
                        intensity: v, max_samples: 8, ..default()
                    });
                }
            }
            FxKind::ResetChromaticAberration { start } => {
                let v = lerp_f32(*start, 0.0, t);
                if let Some(ref mut ca) = chromatic {
                    ca.intensity = v;
                    if done {
                        commands.entity(cam_entity).remove::<ChromaticAberration>();
                    }
                }
            }

            // ── Depth of Field ──
            FxKind::Dof { start, target } => {
                let v = lerp_arr(start, target, t);
                if let Some(ref mut dof) = dof {
                    dof.focal_distance = v[0];
                    dof.aperture_f_stops = v[1];
                } else {
                    commands.entity(cam_entity).insert(DepthOfField {
                        mode: DepthOfFieldMode::Gaussian,
                        focal_distance: v[0], aperture_f_stops: v[1],
                        max_depth: 3000.0, ..default()
                    });
                }
            }
            FxKind::ResetDof { start } => {
                let v = lerp_arr(start, &[900.0, 32.0], t);
                if let Some(ref mut dof) = dof {
                    dof.focal_distance = v[0];
                    dof.aperture_f_stops = v[1];
                    if done {
                        commands.entity(cam_entity).remove::<DepthOfField>();
                    }
                }
            }

            // ── Custom FX (all modify CustomPostProcess directly) ──
            FxKind::Vignette { start, target } => {
                let v = lerp_arr(start, target, t);
                pp.vignette_color.w = v[0];
                pp.vignette_params.x = v[1];
                pp.vignette_params.y = v[2];
                pp.vignette_color.x = v[3];
                pp.vignette_color.y = v[4];
                pp.vignette_color.z = v[5];
            }
            FxKind::Scanlines { start, target } => {
                let v = lerp_arr(start, target, t);
                pp.scanline_params.x = v[0];
                pp.scanline_params.y = v[1];
                pp.scanline_params.z = v[2];
            }
            FxKind::FilmGrain { start, target } => {
                let v = lerp_arr(start, target, t);
                pp.grain_params.x = v[0];
                pp.grain_params.y = v[1];
            }
            FxKind::Fade { start, target } => {
                let v = lerp_arr(start, target, t);
                pp.fade_color.x = v[0]; pp.fade_color.y = v[1];
                pp.fade_color.z = v[2]; pp.fade_color.w = v[3];
            }
            FxKind::Pixelation { start, target } => {
                let v = lerp_f32(*start, *target, t);
                if v > 0.5 {
                    pp.pixelation_params.x = v;
                    pp.pixelation_params.y = v;
                    pp.pixelation_params.z = 1.0;
                } else {
                    pp.pixelation_params.z = 0.0;
                }
            }
            FxKind::ColorTint { start, target } => {
                let v = lerp_arr(start, target, t);
                pp.color_tint = Vec4::from(v);
            }
            FxKind::ColorAdjust { start, target } => {
                let v = lerp_arr(start, target, t);
                pp.misc_params = Vec4::from(v);
            }
            FxKind::SineWave { start, target } => {
                let v = lerp_arr(start, target, t);
                pp.sine_wave = Vec4::from(v);
            }
            FxKind::Swirl { start, target } => {
                let v = lerp_arr(start, target, t);
                pp.swirl = Vec4::from(v);
            }
            FxKind::LensDistortion { start, target } => {
                let v = lerp_arr(start, target, t);
                pp.distortion_shake.x = v[0];
                pp.distortion_shake.y = v[1];
            }
            FxKind::Shake { start, target } => {
                let v = lerp_arr(start, target, t);
                pp.distortion_shake.z = v[0];
                pp.distortion_shake.w = v[1];
            }
            FxKind::Zoom { start, target } => {
                pp.zoom_rotation.x = lerp_f32(*start, *target, t);
            }
            FxKind::Rotation { start, target } => {
                pp.zoom_rotation.y = lerp_f32(*start, *target, t);
            }
            FxKind::CinemaBars { start, target } => {
                let v = lerp_arr(start, target, t);
                pp.zoom_rotation.w = v[0];
                pp.cinema_bar_color.x = v[1];
                pp.cinema_bar_color.y = v[2];
                pp.cinema_bar_color.z = v[3];
            }
            FxKind::Posterize { start, target } => {
                pp.zoom_rotation.z = lerp_f32(*start, *target, t);
            }
            FxKind::ResetCustomFx {
                start_vignette, start_scanlines, start_grain, start_fade,
                start_pixel, start_tint, start_adjust,
                start_sine, start_swirl, start_lens, start_shake,
                start_zoom, start_rotation, start_cinema, start_posterize,
            } => {
                let vg = lerp_arr(start_vignette, &[0.0; 6], t);
                pp.vignette_color.w = vg[0]; pp.vignette_params.x = vg[1]; pp.vignette_params.y = vg[2];
                pp.vignette_color.x = vg[3]; pp.vignette_color.y = vg[4]; pp.vignette_color.z = vg[5];

                let sc = lerp_arr(start_scanlines, &[0.0; 3], t);
                pp.scanline_params.x = sc[0]; pp.scanline_params.y = sc[1]; pp.scanline_params.z = sc[2];

                let gr = lerp_arr(start_grain, &[0.0; 2], t);
                pp.grain_params.x = gr[0]; pp.grain_params.y = gr[1];

                let fd = lerp_arr(start_fade, &[0.0; 4], t);
                pp.fade_color = Vec4::from(fd);

                let px = lerp_f32(*start_pixel, 0.0, t);
                pp.pixelation_params.z = if px > 0.5 { 1.0 } else { 0.0 };
                pp.pixelation_params.x = px;

                let tn = lerp_arr(start_tint, &[1.0, 1.0, 1.0, 0.0], t);
                pp.color_tint = Vec4::from(tn);

                let adj = lerp_arr(start_adjust, &[0.0, 0.0, 1.0, 1.0], t);
                pp.misc_params = Vec4::from(adj);

                let sw = lerp_arr(start_sine, &[0.0; 4], t);
                pp.sine_wave = Vec4::from(sw);

                let swl = lerp_arr(start_swirl, &[0.0; 4], t);
                pp.swirl = Vec4::from(swl);

                let ld = lerp_arr(start_lens, &[0.0, 1.0], t);
                pp.distortion_shake.x = ld[0]; pp.distortion_shake.y = ld[1];

                let sh = lerp_arr(start_shake, &[0.0; 2], t);
                pp.distortion_shake.z = sh[0]; pp.distortion_shake.w = sh[1];

                pp.zoom_rotation.x = lerp_f32(*start_zoom, 1.0, t);
                pp.zoom_rotation.y = lerp_f32(*start_rotation, 0.0, t);
                pp.zoom_rotation.z = lerp_f32(*start_posterize, 0.0, t);

                let cm = lerp_arr(start_cinema, &[0.0; 4], t);
                pp.zoom_rotation.w = cm[0];
                pp.cinema_bar_color.x = cm[1]; pp.cinema_bar_color.y = cm[2]; pp.cinema_bar_color.z = cm[3];
            }
        }

        if done { completed.push(i); }
    }

    // Remove completed transitions (reverse order to preserve indices)
    for i in completed.into_iter().rev() {
        transitions.active.remove(i);
    }
}
