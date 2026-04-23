use bevy::prelude::*;

use crate::entity::movement::OverworldMovement;
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
pub fn process_lua_commands(
    mut commands: Commands,
    mut messages: MessageReader<LuaCommand>,
    placed_q: Query<(Entity, &PlacedObject)>,
    mut anim_q: Query<&mut AnimationController>,
    mut movement_q: Query<&mut OverworldMovement>,
    transform_q: Query<&Transform>,
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
            // Other commands will be handled as their systems are implemented
            _ => {}
        }
    }
}
