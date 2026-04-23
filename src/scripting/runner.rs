//! Scene event runner — executes Lua coroutines for active scene events.
//!
//! AutoRun events fire once and pause other coroutines while running.
//! Parallel events loop continuously and don't block others.
//!
//! Lua functions in the `scene` table yield with a command string.
//! The runner interprets the yielded value to set wait timers or emit LuaCommands.

use bevy::prelude::*;
use mlua::prelude::*;

use super::scene_action::SceneActionRegistry;
use super::scene_event::{EventTrigger, SceneEvent};

/// A single running Lua coroutine.
struct RunningCoroutine {
    event_id: String,
    event_name: String,
    thread_key: LuaRegistryKey,
    wait_timer: f32,
    /// Wait until a named entity's OverworldMovement.arrived is true.
    wait_for_movement: Option<String>,
    /// Wait until a called sub-script coroutine finishes.
    wait_for_script: Option<String>,
    /// Wait until the active dialogue completes (DialogueState resource removed).
    wait_for_dialogue: bool,
    blocking: bool,
    finished: bool,
}

/// Resource managing all running scene event coroutines.
#[derive(Resource, Default)]
pub struct SceneRunner {
    coroutines: Vec<RunningCoroutine>,
    auto_run_fired: Vec<String>,
    /// Pending events to start (set by save or map load).
    pub pending_start: Vec<SceneEvent>,
    /// All events for the current map (used to look up Script-triggered events).
    pub all_events: Vec<SceneEvent>,
    /// Pending yarn node requests: (node_name, blocking, speaker_instance).
    pub pending_yarn_nodes: Vec<(String, bool, Option<String>)>,
}

/// The Lua preamble that defines the `scene` table.
/// Each function yields a command string that the Rust runner interprets.
const LUA_SCENE_API: &str = r#"
scene = scene or {}

function scene.wait(seconds)
    coroutine.yield("wait:" .. tostring(seconds))
end

function scene.move_to(name, pos, speed, easing)
    speed = speed or 100
    easing = easing or "Linear"
    coroutine.yield("move_to:" .. name .. ":" .. tostring(pos.x) .. ":" .. tostring(pos.y) .. ":" .. tostring(speed) .. ":" .. easing)
end

function scene.bezier_move_to(name, path, duration, easing)
    duration = duration or 2.0
    -- path is a table of waypoints; serialize to JSON-like string for the runner
    local pts = {}
    for _, wp in ipairs(path) do
        table.insert(pts, string.format("%.2f,%.2f,%.2f,%.2f,%.2f,%.2f,%.2f,%.2f,%.2f",
            wp.x or 0, wp.y or 0, wp.z or 0,
            wp.hi_x or 0, wp.hi_y or 0, wp.hi_z or 0,
            wp.ho_x or 0, wp.ho_y or 0, wp.ho_z or 0))
    end
    coroutine.yield("bezier_move_to:" .. name .. ":" .. tostring(duration) .. ":" .. table.concat(pts, "|"))
end

function scene.face(name, direction)
    coroutine.yield("face:" .. name .. ":" .. tostring(direction))
end

function scene.run_yarn_node(node, blocking)
    if blocking == nil then blocking = true end
    coroutine.yield("run_yarn_node:" .. node .. ":" .. tostring(blocking))
end

function scene.run_yarn_node_at(node, speaker, blocking)
    if blocking == nil then blocking = true end
    coroutine.yield("run_yarn_node_at:" .. node .. ":" .. speaker .. ":" .. tostring(blocking))
end

function scene.set_time_of_day(hour)
    coroutine.yield("set_time_of_day:" .. tostring(hour))
end

function scene.play_sfx(path)
    coroutine.yield("play_sfx:" .. path)
end

function scene.play_bgm(path, fade_in)
    fade_in = fade_in or 1.0
    coroutine.yield("play_bgm:" .. path .. ":" .. tostring(fade_in))
end

function scene.set_ambient(color, intensity)
    coroutine.yield("set_ambient:" .. tostring(color.r) .. ":" .. tostring(color.g) .. ":" .. tostring(color.b) .. ":" .. tostring(intensity))
end

function scene.camera_pan(target, duration)
    coroutine.yield("camera_pan:" .. tostring(target.x) .. ":" .. tostring(target.y) .. ":" .. tostring(duration))
end

function scene.camera_shake(intensity, duration)
    coroutine.yield("camera_shake:" .. tostring(intensity) .. ":" .. tostring(duration))
end

function scene.set_flag(key, value)
    coroutine.yield("set_flag:" .. key .. ":" .. value)
end

function scene.map_transition(target_map, spawn_x, spawn_y)
    coroutine.yield("map_transition:" .. target_map .. ":" .. tostring(spawn_x) .. ":" .. tostring(spawn_y))
end

function scene.spawn_particle(def_id, pos)
    coroutine.yield("spawn_particle:" .. def_id .. ":" .. tostring(pos.x) .. ":" .. tostring(pos.y))
end

function scene.wait_for_movement(name)
    coroutine.yield("wait_for_movement:" .. name)
end

function scene.call_script(name)
    coroutine.yield("call_script:" .. name)
end
"#;

/// Parse a yielded command string into a LuaCommand.
fn parse_yield(yield_str: &str) -> YieldAction {
    let parts: Vec<&str> = yield_str.splitn(14, ':').collect();
    match parts[0] {
        "wait" => {
            let secs: f32 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.0);
            YieldAction::Wait(secs)
        }
        "wait_for_movement" if parts.len() >= 2 => {
            YieldAction::WaitForMovement(parts[1].to_string())
        }
        "bezier_move_to" if parts.len() >= 4 => {
            use super::event_bridge::LuaCommand;
            use super::scene_event::SplineWaypoint;
            let name = parts[1].to_string();
            let duration: f32 = parts[2].parse().unwrap_or(2.0);
            // Parse waypoints from "x,y,z,hi_x,hi_y,hi_z,ho_x,ho_y,ho_z|..." format
            let waypoints: Vec<SplineWaypoint> = parts[3].split('|')
                .filter_map(|wp_str| {
                    let v: Vec<f32> = wp_str.split(',').filter_map(|s| s.parse().ok()).collect();
                    if v.len() >= 9 {
                        Some(SplineWaypoint {
                            pos: [v[0], v[1]], z: v[2],
                            handle_in: [v[3], v[4]], handle_in_z: v[5],
                            handle_out: [v[6], v[7]], handle_out_z: v[8],
                        })
                    } else { None }
                })
                .collect();
            YieldAction::Command {
                cmd: LuaCommand::BezierMoveTo {
                    name: name.clone(),
                    waypoints,
                    duration,
                },
                wait_movement: Some(name),
            }
        }
        "move_to" if parts.len() >= 5 => {
            use super::event_bridge::LuaCommand;
            let name = parts[1].to_string();
            let x: f32 = parts[2].parse().unwrap_or(0.0);
            let y: f32 = parts[3].parse().unwrap_or(0.0);
            let speed: f32 = parts[4].parse().unwrap_or(100.0);
            let easing = parts.get(5).map(|s| s.to_string());
            YieldAction::Command {
                cmd: LuaCommand::MoveTo {
                    name: name.clone(),
                    target: Vec2::new(x, y),
                    speed,
                    easing,
                },
                wait_movement: Some(name),
            }
        }
        "face" if parts.len() >= 3 => {
            use super::event_bridge::LuaCommand;
            let dir_str = parts[2];
            let dir: u8 = match dir_str {
                "up" => 0, "left" => 1, "down" => 2, "right" => 3,
                _ => dir_str.parse().unwrap_or(2),
            };
            YieldAction::Command {
                cmd: LuaCommand::Face {
                    name: parts[1].to_string(),
                    direction: dir,
                },
                wait_movement: None,
            }
        }
        "run_yarn_node" if parts.len() >= 2 => {
            let blocking = parts.get(2).map(|s| *s == "true").unwrap_or(true);
            YieldAction::RunYarnNode {
                node: parts[1].to_string(),
                blocking,
                speaker: None,
            }
        }
        "run_yarn_node_at" if parts.len() >= 3 => {
            let blocking = parts.get(3).map(|s| *s == "true").unwrap_or(true);
            YieldAction::RunYarnNode {
                node: parts[1].to_string(),
                blocking,
                speaker: Some(parts[2].to_string()),
            }
        }
        "set_time_of_day" if parts.len() >= 2 => {
            use super::event_bridge::LuaCommand;
            YieldAction::Command {
                cmd: LuaCommand::SetTimeOfDay {
                    hour: parts[1].parse().unwrap_or(12.0),
                },
                wait_movement: None,
            }
        }
        "play_sfx" if parts.len() >= 2 => {
            use super::event_bridge::LuaCommand;
            YieldAction::Command {
                cmd: LuaCommand::PlaySfx {
                    asset_path: parts[1].to_string(),
                },
                wait_movement: None,
            }
        }
        "play_bgm" if parts.len() >= 2 => {
            use super::event_bridge::LuaCommand;
            let fade = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(1.0);
            YieldAction::Command {
                cmd: LuaCommand::PlayBgm {
                    asset_path: parts[1].to_string(),
                    fade_in: fade,
                },
                wait_movement: None,
            }
        }
        "set_flag" if parts.len() >= 3 => {
            use super::event_bridge::LuaCommand;
            YieldAction::Command {
                cmd: LuaCommand::SetFlag {
                    key: parts[1].to_string(),
                    value: parts[2].to_string(),
                },
                wait_movement: None,
            }
        }
        "map_transition" if parts.len() >= 4 => {
            use super::event_bridge::LuaCommand;
            YieldAction::Command {
                cmd: LuaCommand::MapTransition {
                    target_map: parts[1].to_string(),
                    spawn_x: parts[2].parse().unwrap_or(0),
                    spawn_y: parts[3].parse().unwrap_or(0),
                },
                wait_movement: None,
            }
        }
        "call_script" if parts.len() >= 2 => {
            YieldAction::CallScript(parts[1].to_string())
        }
        "parallel_tick" => {
            // Parallel block requesting a tick — resume immediately with dt.
            YieldAction::ParallelTick
        }
        _ => {
            warn!("Unknown Lua yield: {yield_str}");
            YieldAction::None
        }
    }
}

enum YieldAction {
    None,
    Wait(f32),
    WaitForMovement(String),
    /// Call another event by name as a blocking sub-script.
    CallScript(String),
    /// Start a Yarn Spinner dialogue node and wait for it to complete.
    RunYarnNode { node: String, blocking: bool, speaker: Option<String> },
    /// Parallel block requesting a tick — resume immediately with delta time.
    ParallelTick,
    Command {
        cmd: super::event_bridge::LuaCommand,
        /// If set, also wait for this entity's movement to complete.
        wait_movement: Option<String>,
    },
}

/// System: start pending events as Lua coroutines.
pub fn start_pending_events(
    mut runner: ResMut<SceneRunner>,
    vm: Res<super::LuaVm>,
    registry: Res<SceneActionRegistry>,
) {
    let events: Vec<SceneEvent> = runner.pending_start.drain(..).collect();
    if events.is_empty() {
        return;
    }

    // Update all_events so call_script can find Script-triggered events
    runner.all_events = events.clone();

    // Ensure scene API is loaded
    if let Err(e) = vm.lua.load(LUA_SCENE_API).exec() {
        error!("Failed to load scene API: {e}");
        return;
    }

    for event in &events {
        if !event.enabled {
            continue;
        }
        match event.trigger {
            EventTrigger::AutoRun => {
                if runner.auto_run_fired.contains(&event.id) {
                    continue;
                }
                runner.auto_run_fired.push(event.id.clone());
                start_coroutine(&vm.lua, &mut runner, event, &registry, true);
            }
            EventTrigger::Parallel => {
                if runner.coroutines.iter().any(|c| c.event_id == event.id && !c.finished) {
                    continue;
                }
                start_coroutine(&vm.lua, &mut runner, event, &registry, false);
            }
            _ => {}
        }
    }
}

fn start_coroutine(
    lua: &Lua,
    runner: &mut SceneRunner,
    event: &SceneEvent,
    registry: &SceneActionRegistry,
    blocking: bool,
) {
    let lua_code = super::scene_event::generate_lua(event, registry);
    info!("Starting event '{}' ({})\n{}", event.name, if blocking { "blocking" } else { "parallel" }, &lua_code);

    // Load the script (defines `run` function)
    if let Err(e) = lua.load(&lua_code).exec() {
        error!("Lua load error in event '{}': {e}", event.name);
        return;
    }

    // Get the `run` function
    let run_fn: LuaFunction = match lua.globals().get("run") {
        Ok(f) => f,
        Err(e) => {
            error!("No `run` function in event '{}': {e}", event.name);
            return;
        }
    };

    // Create coroutine thread
    let thread = match lua.create_thread(run_fn) {
        Ok(t) => t,
        Err(e) => {
            error!("Failed to create coroutine for '{}': {e}", event.name);
            return;
        }
    };

    let thread_key = match lua.create_registry_value(thread) {
        Ok(k) => k,
        Err(e) => {
            error!("Failed to register coroutine for '{}': {e}", event.name);
            return;
        }
    };

    runner.coroutines.push(RunningCoroutine {
        event_id: event.id.clone(),
        event_name: event.name.clone(),
        thread_key,
        wait_timer: 0.0,
        wait_for_movement: None,
        wait_for_script: None,
        wait_for_dialogue: false,
        blocking,
        finished: false,
    });
}

/// System: resume active coroutines each frame.
pub fn tick_coroutines(
    time: Res<Time>,
    mut runner: ResMut<SceneRunner>,
    vm: Res<super::LuaVm>,
    registry: Res<SceneActionRegistry>,
    mut cmd_writer: MessageWriter<super::event_bridge::LuaCommand>,
    placed_q: Query<(Entity, &crate::tile_editor::state::PlacedObject)>,
    movement_q: Query<&crate::entity::movement::OverworldMovement>,
) {
    let dt = time.delta_secs();

    // Set scene._dt so parallel blocks can read frame delta time.
    let _ = vm.lua.load(&format!("scene._dt = {dt}")).exec();

    let has_blocker = runner.coroutines.iter().any(|c| c.blocking && !c.finished);

    // Collect script calls to spawn after iteration.
    let mut scripts_to_start: Vec<String> = Vec::new();
    // Collect yarn node requests to spawn after iteration.
    let mut yarn_nodes_to_start: Vec<(String, bool, Option<String>)> = Vec::new();

    let num = runner.coroutines.len();
    for i in 0..num {
        if runner.coroutines[i].finished {
            continue;
        }
        if has_blocker && !runner.coroutines[i].blocking {
            continue;
        }
        if runner.coroutines[i].wait_timer > 0.0 {
            runner.coroutines[i].wait_timer -= dt;
            continue;
        }

        // Check wait_for_script condition — a called sub-script must finish first
        if let Some(ref script_name) = runner.coroutines[i].wait_for_script {
            let still_running = runner.coroutines.iter().enumerate().any(|(j, other)| {
                j != i
                    && other.event_name == *script_name
                    && other.event_id.starts_with("script_call_")
                    && !other.finished
            });
            if still_running {
                continue;
            }
            runner.coroutines[i].wait_for_script = None;
        }

        // Check wait_for_movement condition
        if let Some(ref name) = runner.coroutines[i].wait_for_movement {
            let arrived = placed_q.iter()
                .find(|(_, po)| {
                    if let Some(n) = name.strip_prefix('#') {
                        po.sidecar_id == n
                    } else {
                        po.name.as_deref() == Some(name.as_str())
                    }
                })
                .and_then(|(entity, _)| movement_q.get(entity).ok())
                .map(|mv| mv.arrived || (mv.target.is_none() && mv.bezier.is_none() && mv.easing.is_none()))
                .unwrap_or(true); // entity not found = treat as arrived

            if !arrived {
                continue; // still moving, don't resume
            }
            runner.coroutines[i].wait_for_movement = None;
        }

        // Check wait_for_dialogue condition — dialogue must complete first
        if runner.coroutines[i].wait_for_dialogue {
            // DialogueState resource absence is checked by the caller via system params
            // We can't check resources here, so we use a flag that gets cleared externally
            continue;
        }

        let thread: LuaThread = match vm.lua.registry_value(&runner.coroutines[i].thread_key) {
            Ok(t) => t,
            Err(_) => {
                runner.coroutines[i].finished = true;
                continue;
            }
        };

        match thread.status() {
            LuaThreadStatus::Resumable => {}
            _ => {
                runner.coroutines[i].finished = true;
                continue;
            }
        }

        match thread.resume::<LuaMultiValue>(()) {
            Ok(values) => {
                let yield_str: Option<String> = values.iter().find_map(|v| {
                    if let LuaValue::String(s) = v {
                        s.to_str().ok().map(|s| s.to_string())
                    } else {
                        None
                    }
                });

                if let Some(s) = yield_str {
                    match parse_yield(&s) {
                        YieldAction::Wait(secs) => {
                            runner.coroutines[i].wait_timer = secs;
                        }
                        YieldAction::WaitForMovement(name) => {
                            runner.coroutines[i].wait_for_movement = Some(name);
                        }
                        YieldAction::CallScript(ref name) => {
                            runner.coroutines[i].wait_for_script = Some(name.clone());
                            scripts_to_start.push(name.clone());
                        }
                        YieldAction::RunYarnNode { node, blocking, speaker } => {
                            runner.coroutines[i].wait_for_dialogue = true;
                            yarn_nodes_to_start.push((node, blocking, speaker));
                        }
                        YieldAction::ParallelTick => {
                            // Parallel block wants to resume next frame — no timer needed.
                        }
                        YieldAction::Command { cmd, wait_movement } => {
                            if let Some(name) = wait_movement {
                                runner.coroutines[i].wait_for_movement = Some(name);
                            }
                            cmd_writer.write(cmd);
                        }
                        YieldAction::None => {}
                    }
                } else {
                    match thread.status() {
                        LuaThreadStatus::Resumable => {}
                        _ => runner.coroutines[i].finished = true,
                    }
                }
            }
            Err(e) => {
                error!("Lua error in event '{}': {e}", runner.coroutines[i].event_name);
                runner.coroutines[i].finished = true;
            }
        }
    }

    // Spawn coroutines for called scripts
    if !scripts_to_start.is_empty() {
        let registry_ref = &*registry;
        for script_name in scripts_to_start {
            let event = runner.all_events.iter()
                .find(|e| e.name == script_name && e.trigger == EventTrigger::Script && e.enabled)
                .cloned();
            if let Some(event) = event {
                // Load scene API before starting
                if let Err(e) = vm.lua.load(LUA_SCENE_API).exec() {
                    error!("Failed to load scene API for call_script: {e}");
                    continue;
                }
                let lua_code = super::scene_event::generate_lua(&event, registry_ref);
                info!("call_script: starting '{}'\n{}", script_name, &lua_code);

                if let Err(e) = vm.lua.load(&lua_code).exec() {
                    error!("Lua load error in call_script '{}': {e}", script_name);
                    continue;
                }
                let run_fn: LuaFunction = match vm.lua.globals().get("run") {
                    Ok(f) => f,
                    Err(e) => {
                        error!("No `run` function in call_script '{}': {e}", script_name);
                        continue;
                    }
                };
                let thread = match vm.lua.create_thread(run_fn) {
                    Ok(t) => t,
                    Err(e) => {
                        error!("Failed to create coroutine for call_script '{}': {e}", script_name);
                        continue;
                    }
                };
                let thread_key = match vm.lua.create_registry_value(thread) {
                    Ok(k) => k,
                    Err(e) => {
                        error!("Failed to register coroutine for call_script '{}': {e}", script_name);
                        continue;
                    }
                };

                runner.coroutines.push(RunningCoroutine {
                    event_id: format!("script_call_{}", event.id),
                    event_name: event.name.clone(),
                    thread_key,
                    wait_timer: 0.0,
                    wait_for_movement: None,
                    wait_for_script: None,
                    wait_for_dialogue: false,
                    blocking: false,
                    finished: false,
                });
            } else {
                warn!("call_script: no Script-triggered event named '{script_name}'");
            }
        }
    }

    // Queue yarn node requests for the dialogue system to process.
    if !yarn_nodes_to_start.is_empty() {
        runner.pending_yarn_nodes.extend(yarn_nodes_to_start);
    }

    runner.coroutines.retain(|c| !c.finished);
}

/// System: starts pending yarn dialogue nodes. Runs after tick_coroutines.
/// Requires YarnProject to be loaded (run_if guard in plugin).
pub fn start_pending_yarn_nodes(
    mut runner: ResMut<SceneRunner>,
    mut commands: Commands,
    project: Res<bevy_yarnspinner::prelude::YarnProject>,
    dialogue_state: Option<Res<crate::dialogue::state::DialogueState>>,
) {
    // Don't start a new dialogue if one is already active
    if dialogue_state.is_some() || runner.pending_yarn_nodes.is_empty() {
        return;
    }

    // Start the first pending node (queue the rest)
    let (node, blocking, speaker) = runner.pending_yarn_nodes.remove(0);
    crate::dialogue::start_yarn_node(&mut commands, &project, &node, blocking, speaker);
}

/// System: when DialogueState is removed, clear the wait_for_dialogue flag
/// on all coroutines so they can resume.
pub fn clear_dialogue_wait(
    mut runner: ResMut<SceneRunner>,
    dialogue_state: Option<Res<crate::dialogue::state::DialogueState>>,
) {
    if dialogue_state.is_some() {
        return;
    }
    for co in &mut runner.coroutines {
        if co.wait_for_dialogue {
            co.wait_for_dialogue = false;
        }
    }
}
