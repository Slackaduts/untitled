use bevy::prelude::*;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::GlobalAmbientLight;

use crate::camera::CombatCamera3d;
use crate::entity::movement::OverworldMovement;
use crate::post_process::custom::CustomPostProcess;
use crate::post_process::shockwave::ShockwaveEmitter;
use crate::post_process::transition::{
    FxTransitions, FxKind,
    capture_bloom, capture_color_grading, capture_chromatic, capture_dof,
    capture_vignette, capture_scanlines, capture_grain, capture_fade,
    capture_pixelation, capture_tint, capture_adjust,
    capture_sine_wave, capture_swirl, capture_lens_distortion, capture_shake,
    capture_zoom, capture_rotation, capture_cinema_bars, capture_posterize,
};
use crate::sprite::animation::AnimationController;
use crate::tile_editor::state::PlacedObject;

/// Commands pushed by Lua API calls, drained by Bevy systems.
///
/// Name-based: all instance references use string names (PlacedObject.name),
/// resolved to entities at runtime.
#[derive(Message, Debug, Clone)]
pub enum LuaCommand {
    // ── Camera ──
    CameraPan { target: Vec2, duration: f32 },
    CameraShake { intensity: f32, duration: f32 },
    CameraZoom { level: f32, duration: f32 },

    // ── Combat ──
    StartCombat { encounter_id: String },

    // ── Dialogue (handled by bevy_yarnspinner, not LuaCommand) ──

    // ── Movement (name-based for Lua) ──
    MoveTo { name: String, target: Vec2, speed: f32, easing: Option<String> },
    BezierMoveTo {
        name: String,
        waypoints: Vec<crate::scripting::scene_event::SplineWaypoint>,
        duration: f32,
    },
    Face { name: String, direction: u8 },

    // ── VFX ──
    SpawnParticle { def_id: String, position: Vec2 },
    ScreenFlash { color: Color, duration: f32 },

    // ── Sound ──
    PlayBgm { asset_path: String, fade_in: f32 },
    StopBgm { fade_out: f32 },
    PlaySfx { asset_path: String },
    PlaySfxAt { asset_path: String, position: Vec2 },

    // ── Lighting ──
    SetAmbient { color: Color, intensity: f32 },
    SpawnLight { position: Vec2, color: Color, intensity: f32, radius: f32 },
    SetTimeOfDay { hour: f32 },
    SetTimeSpeed { speed: f32 },

    // ── World ──
    SpawnEntity { template_id: String, position: Vec2 },
    SetFlag { key: String, value: String },
    GetFlag { key: String },

    // ── Map ──
    MapTransition { target_map: String, spawn_x: i32, spawn_y: i32 },

    // ── Post FX ──
    PostFx { command: String, args: Vec<f32>, easing: String },
}

/// Resolve a placed object by name or `#id` reference.
/// - `"guard"` matches `PlacedObject.name == Some("guard")`
/// - `"#5"` matches `PlacedObject.sidecar_id == "5"`
fn resolve_by_name<'a>(
    query: &'a Query<(Entity, &PlacedObject)>,
    name: &str,
) -> Option<Entity> {
    if let Some(id) = name.strip_prefix('#') {
        // Match by sidecar ID
        query.iter()
            .find(|(_, po)| po.sidecar_id == id)
            .map(|(e, _)| e)
    } else {
        // Match by user-defined name
        query.iter()
            .find(|(_, po)| po.name.as_deref() == Some(name))
            .map(|(e, _)| e)
    }
}

/// System: processes LuaCommand messages and applies them to the ECS.
#[allow(clippy::too_many_arguments)]
pub fn process_lua_commands(
    mut commands: Commands,
    mut messages: MessageReader<LuaCommand>,
    placed_q: Query<(Entity, &PlacedObject)>,
    mut anim_q: Query<&mut AnimationController>,
    mut movement_q: Query<&mut OverworldMovement>,
    transform_q: Query<&Transform>,
    mut transitions: ResMut<FxTransitions>,
    cam_q: Query<
        (Option<&bevy::post_process::bloom::Bloom>,
         &bevy::render::view::ColorGrading,
         Option<&bevy::post_process::effect_stack::ChromaticAberration>,
         Option<&bevy::post_process::dof::DepthOfField>,
         &CustomPostProcess),
        With<CombatCamera3d>,
    >,
) {
    for cmd in messages.read() {
        match cmd {
            LuaCommand::MoveTo { name, target, speed, easing } => {
                let Some(entity) = resolve_by_name(&placed_q, &name) else {
                    warn!("MoveTo: no entity named '{name}'");
                    continue;
                };
                let tile = crate::map::DEFAULT_TILE_SIZE;
                let world_target = Vec2::new(
                    (target.x + 0.5) * tile,
                    (target.y + 0.5) * tile,
                );
                let px_speed = *speed * tile;

                // Compute easing state if an easing function is specified (and not Linear)
                // Easing: start position filled in by movement system on first tick
                let ease_state = easing.as_ref()
                    .filter(|e| *e != "Linear")
                    .map(|e| {
                        let ease_fn = crate::entity::movement::parse_ease_function(e);
                        crate::entity::movement::EasingState {
                            start: Vec2::ZERO, // filled in by movement system
                            elapsed: -1.0,     // sentinel: not yet initialized
                            duration: 0.0,     // computed on first tick
                            ease_fn,
                        }
                    });

                if let Ok(mut mv) = movement_q.get_mut(entity) {
                    mv.target = Some(world_target);
                    mv.speed = px_speed;
                    mv.arrived = false;
                    mv.easing = ease_state;
                    mv.bezier = None;
                } else {
                    commands.entity(entity).insert(OverworldMovement {
                        target: Some(world_target),
                        speed: px_speed,
                        arrived: false,
                        easing: ease_state,
                        bezier: None,
                    });
                }
            }
            LuaCommand::BezierMoveTo { name, waypoints, duration } => {
                let Some(entity) = resolve_by_name(&placed_q, &name) else {
                    warn!("BezierMoveTo: no entity named '{name}'");
                    continue;
                };
                if waypoints.len() < 2 {
                    warn!("BezierMoveTo: need at least 2 waypoints");
                    continue;
                }
                let tile = crate::map::DEFAULT_TILE_SIZE;
                let current_pos = transform_q.get(entity)
                    .map(|tf| tf.translation)
                    .unwrap_or(Vec3::ZERO);

                // Build multi-segment bezier from waypoints.
                // The spline starts at waypoints[0] and ends at the last waypoint.
                let mut segments: Vec<[Vec3; 4]> = Vec::new();
                let wp_to_world = |wp: &crate::scripting::scene_event::SplineWaypoint| -> Vec3 {
                    Vec3::new((wp.pos[0] + 0.5) * tile, (wp.pos[1] + 0.5) * tile, wp.z * tile)
                };
                let handle_out_world = |wp: &crate::scripting::scene_event::SplineWaypoint| -> Vec3 {
                    Vec3::new(
                        (wp.pos[0] + wp.handle_out[0] + 0.5) * tile,
                        (wp.pos[1] + wp.handle_out[1] + 0.5) * tile,
                        (wp.z + wp.handle_out_z) * tile,
                    )
                };
                let handle_in_world = |wp: &crate::scripting::scene_event::SplineWaypoint| -> Vec3 {
                    Vec3::new(
                        (wp.pos[0] + wp.handle_in[0] + 0.5) * tile,
                        (wp.pos[1] + wp.handle_in[1] + 0.5) * tile,
                        (wp.z + wp.handle_in_z) * tile,
                    )
                };

                for i in 0..waypoints.len() - 1 {
                    let wa = &waypoints[i];
                    let wb = &waypoints[i + 1];
                    segments.push([
                        wp_to_world(wa),
                        handle_out_world(wa),
                        handle_in_world(wb),
                        wp_to_world(wb),
                    ]);
                }

                let bezier = bevy::math::cubic_splines::CubicBezier::new(segments);
                let Ok(curve) = bezier.to_curve() else {
                    warn!("BezierMoveTo: failed to build curve");
                    continue;
                };

                let bez_state = crate::entity::movement::BezierState {
                    curve,
                    elapsed: 0.0,
                    duration: *duration,
                    start_pos: current_pos,
                };

                if let Ok(mut mv) = movement_q.get_mut(entity) {
                    mv.bezier = Some(bez_state);
                    mv.easing = None;
                    mv.target = None;
                    mv.arrived = false;
                } else {
                    commands.entity(entity).insert(OverworldMovement {
                        bezier: Some(bez_state),
                        easing: None,
                        target: None,
                        speed: 0.0,
                        arrived: false,
                    });
                }
            }
            LuaCommand::Face { name, direction } => {
                let Some(entity) = resolve_by_name(&placed_q, &name) else {
                    warn!("Face: no entity named '{name}'");
                    continue;
                };
                if let Ok(mut anim) = anim_q.get_mut(entity) {
                    anim.direction = *direction;
                    anim.frame = 0;
                }
            }
            LuaCommand::PostFx { command, args, easing } => {
                let Ok((bloom, cg, chromatic, dof, pp)) = cam_q.single() else { continue };
                let a = |i: usize| -> f32 { args.get(i).copied().unwrap_or(0.0) };
                match command.as_str() {
                    "set_bloom" => {
                        let start = capture_bloom(bloom);
                        let target = [a(0), a(1), a(2)];
                        transitions.push(FxKind::Bloom { start, target }, a(3), easing);
                    }
                    "reset_bloom" => {
                        let start = capture_bloom(bloom);
                        transitions.push(FxKind::ResetBloom { start }, a(0), easing);
                    }
                    "set_tonemapping" => {
                        let algo = match easing.as_str() {
                            // easing field holds the algorithm string here (first non-float)
                            "Reinhard" => Tonemapping::Reinhard,
                            "ReinhardLuminance" => Tonemapping::ReinhardLuminance,
                            "AcesFitted" => Tonemapping::AcesFitted,
                            "AgX" => Tonemapping::AgX,
                            "SomewhatBoring" => Tonemapping::SomewhatBoringDisplayTransform,
                            "TonyMcMapface" => Tonemapping::TonyMcMapface,
                            "BlenderFilmic" => Tonemapping::BlenderFilmic,
                            "None" => Tonemapping::None,
                            _ => Tonemapping::TonyMcMapface,
                        };
                        transitions.push(FxKind::Tonemapping(algo), 0.0, "Linear");
                    }
                    "set_color_grading" => {
                        let start = capture_color_grading(cg);
                        let target = [a(0), a(1), a(2), a(3), a(4)];
                        transitions.push(FxKind::ColorGrading { start, target }, a(5), easing);
                    }
                    "reset_color_grading" => {
                        let start = capture_color_grading(cg);
                        transitions.push(FxKind::ResetColorGrading { start }, a(0), easing);
                    }
                    "set_chromatic_aberration" => {
                        let start = capture_chromatic(chromatic);
                        transitions.push(FxKind::ChromaticAberration { start, target: a(0) }, a(1), easing);
                    }
                    "reset_chromatic_aberration" => {
                        let start = capture_chromatic(chromatic);
                        transitions.push(FxKind::ResetChromaticAberration { start }, a(0), easing);
                    }
                    "set_dof" => {
                        let start = capture_dof(dof);
                        transitions.push(FxKind::Dof { start, target: [a(0), a(1)] }, a(2), easing);
                    }
                    "reset_dof" => {
                        let start = capture_dof(dof);
                        transitions.push(FxKind::ResetDof { start }, a(0), easing);
                    }
                    "set_vignette" => {
                        let start = capture_vignette(pp);
                        let target = [a(0), a(1), a(2), a(3), a(4), a(5)];
                        transitions.push(FxKind::Vignette { start, target }, a(6), easing);
                    }
                    "set_scanlines" => {
                        let start = capture_scanlines(pp);
                        transitions.push(FxKind::Scanlines { start, target: [a(0), a(1), a(2)] }, a(3), easing);
                    }
                    "set_film_grain" => {
                        let start = capture_grain(pp);
                        transitions.push(FxKind::FilmGrain { start, target: [a(0), a(1)] }, a(2), easing);
                    }
                    "set_fade" => {
                        let start = capture_fade(pp);
                        transitions.push(FxKind::Fade { start, target: [a(0), a(1), a(2), a(3)] }, a(4), easing);
                    }
                    "set_pixelation" => {
                        let start = capture_pixelation(pp);
                        transitions.push(FxKind::Pixelation { start, target: a(0) }, a(1), easing);
                    }
                    "set_color_tint" => {
                        let start = capture_tint(pp);
                        transitions.push(FxKind::ColorTint { start, target: [a(0), a(1), a(2), a(3)] }, a(4), easing);
                    }
                    "set_color_adjust" => {
                        let start = capture_adjust(pp);
                        transitions.push(FxKind::ColorAdjust { start, target: [a(0), a(1), a(2), a(3)] }, a(4), easing);
                    }
                    "set_sine_wave" => {
                        let start = capture_sine_wave(pp);
                        transitions.push(FxKind::SineWave { start, target: [a(0), a(1), a(2), a(3)] }, a(4), easing);
                    }
                    "set_swirl" => {
                        let start = capture_swirl(pp);
                        transitions.push(FxKind::Swirl { start, target: [a(0), a(1), a(2), a(3)] }, a(4), easing);
                    }
                    "set_lens_distortion" => {
                        let start = capture_lens_distortion(pp);
                        transitions.push(FxKind::LensDistortion { start, target: [a(0), a(1)] }, a(2), easing);
                    }
                    "set_shake" => {
                        let start = capture_shake(pp);
                        transitions.push(FxKind::Shake { start, target: [a(0), a(1)] }, a(2), easing);
                    }
                    "set_zoom" => {
                        let start = capture_zoom(pp);
                        transitions.push(FxKind::Zoom { start, target: a(0) }, a(1), easing);
                    }
                    "set_rotation" => {
                        let start = capture_rotation(pp);
                        transitions.push(FxKind::Rotation { start, target: a(0) }, a(1), easing);
                    }
                    "set_cinema_bars" => {
                        let start = capture_cinema_bars(pp);
                        transitions.push(FxKind::CinemaBars { start, target: [a(0), a(1), a(2), a(3)] }, a(4), easing);
                    }
                    "set_posterize" => {
                        let start = capture_posterize(pp);
                        transitions.push(FxKind::Posterize { start, target: a(0) }, a(1), easing);
                    }
                    "reset_custom_fx" => {
                        transitions.push(FxKind::ResetCustomFx {
                            start_vignette: capture_vignette(pp),
                            start_scanlines: capture_scanlines(pp),
                            start_grain: capture_grain(pp),
                            start_fade: capture_fade(pp),
                            start_pixel: capture_pixelation(pp),
                            start_tint: capture_tint(pp),
                            start_adjust: capture_adjust(pp),
                            start_sine: capture_sine_wave(pp),
                            start_swirl: capture_swirl(pp),
                            start_lens: capture_lens_distortion(pp),
                            start_shake: capture_shake(pp),
                            start_zoom: capture_zoom(pp),
                            start_rotation: capture_rotation(pp),
                            start_cinema: capture_cinema_bars(pp),
                            start_posterize: capture_posterize(pp),
                        }, a(0), easing);
                    }
                    "spawn_shockwave" => {
                        let tile = crate::map::DEFAULT_TILE_SIZE;
                        commands.spawn(ShockwaveEmitter {
                            center: Vec2::new((a(0) + 0.5) * tile, (a(1) + 0.5) * tile),
                            max_radius: a(2),
                            duration: a(3),
                            intensity: a(4),
                            thickness: a(5),
                            chromatic: a(6),
                            elapsed: 0.0,
                        });
                    }
                    _ => { warn!("Unknown PostFx command: {command}"); }
                }
            }
            LuaCommand::CameraPan { target, duration } => {
                let tile = crate::map::DEFAULT_TILE_SIZE;
                let world_target = Vec2::new(
                    (target.x + 0.5) * tile,
                    (target.y + 0.5) * tile,
                );
                commands.insert_resource(crate::camera::cutscene::CameraPanState {
                    target: world_target,
                    duration: *duration,
                    elapsed: 0.0,
                    start: None,
                });
            }
            LuaCommand::CameraShake { intensity, duration } => {
                commands.insert_resource(crate::camera::cutscene::CameraShakeState {
                    intensity: *intensity,
                    duration: *duration,
                    elapsed: 0.0,
                });
            }
            LuaCommand::SetAmbient { color, intensity } => {
                commands.insert_resource(GlobalAmbientLight {
                    color: *color,
                    brightness: *intensity * 400.0, // scale to cd/m² like time_of_day.rs
                    ..default()
                });
            }
            LuaCommand::ScreenFlash { color, duration } => {
                // Use the custom post-process fade for a flash: snap to color, then fade out
                if let Ok((_, _, _, _, pp)) = cam_q.single() {
                    let c = color.to_srgba();
                    transitions.push(
                        FxKind::Fade {
                            start: [c.red, c.green, c.blue, 1.0],
                            target: [c.red, c.green, c.blue, 0.0],
                        },
                        *duration,
                        "QuadraticOut",
                    );
                }
            }
            LuaCommand::SpawnParticle { def_id, position } => {
                warn!("SpawnParticle '{def_id}' at {position} — particle system not yet implemented");
            }
            _ => {}
        }
    }
}
