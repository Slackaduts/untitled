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
    /// When true, this coroutine blocks all non-parallel coroutines until finished (AutoRun).
    blocking: bool,
    /// When true, this coroutine keeps running even when a blocker is active (Parallel events).
    parallel: bool,
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
    /// Pending yarn node requests: (node_name, blocking, speaker_map).
    pub pending_yarn_nodes: Vec<(String, bool, Vec<(String, String)>)>,
    /// Pending interact/touch triggers: (event_id, blocking).
    /// Set by interaction systems, drained by start_pending_events.
    pub pending_trigger: Vec<(String, EventTrigger)>,
}

impl SceneRunner {
    /// Request that an Interact or Touch event be started for the given instance name.
    /// The runner will find matching events and start coroutines for them.
    pub fn trigger_for_instance(&mut self, instance_name: &str, trigger: EventTrigger) {
        for i in 0..self.all_events.len() {
            let e = &self.all_events[i];
            if !e.enabled || e.trigger != trigger || e.trigger_target.as_deref() != Some(instance_name) {
                continue;
            }
            let id = &e.id;
            if self.coroutines.iter().any(|c| c.event_id == *id && !c.finished) {
                continue;
            }
            if self.pending_trigger.iter().any(|(pid, _)| pid == id) {
                continue;
            }
            self.pending_trigger.push((id.clone(), trigger.clone()));
        }
    }

    /// Whether any blocking (AutoRun/Interact) coroutine is currently running.
    pub fn has_blocker(&self) -> bool {
        self.coroutines.iter().any(|c| c.blocking && !c.finished)
    }

    /// Reset the auto-run guard so AutoRun events fire again (e.g. on map reload).
    pub fn clear_auto_run(&mut self) {
        self.auto_run_fired.clear();
    }
}

/// The Lua preamble that defines the `scene` table.
/// Each function yields a command string that the Rust runner interprets.
const LUA_SCENE_API: &str = r#"
scene = scene or {}

function scene.wait(seconds)
    coroutine.yield("wait:" .. tostring(seconds))
end

-- Helper: yield a command, then automatically wait for its duration.
-- This makes sequential events block until the transition finishes.
function scene._do(cmd_str, duration)
    coroutine.yield(cmd_str)
    if duration and duration > 0 then
        coroutine.yield("wait:" .. tostring(duration))
    end
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

function scene.run_yarn_node_at(node, speakers, blocking)
    if blocking == nil then blocking = true end
    -- speakers is a table { ["CharName"] = "instance_name", ... }
    -- Serialize as "CharName=instance|CharName2=instance2"
    local parts = {}
    if speakers then
        for char_name, inst in pairs(speakers) do
            table.insert(parts, char_name .. "=" .. inst)
        end
    end
    coroutine.yield("run_yarn_node_at:" .. node .. ":" .. table.concat(parts, "|") .. ":" .. tostring(blocking))
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
    scene._do("camera_pan:" .. tostring(target.x) .. ":" .. tostring(target.y) .. ":" .. tostring(duration), duration)
end

function scene.camera_shake(intensity, duration)
    scene._do("camera_shake:" .. tostring(intensity) .. ":" .. tostring(duration), duration)
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

function scene.screen_flash(color, duration)
    duration = duration or 0.3
    scene._do("screen_flash:" .. tostring(color.r) .. ":" .. tostring(color.g) .. ":" .. tostring(color.b) .. ":" .. tostring(duration), duration)
end

function scene.wait_for_movement(name)
    coroutine.yield("wait_for_movement:" .. name)
end

function scene.call_script(name)
    coroutine.yield("call_script:" .. name)
end

-- ── Post FX ──

function scene.set_bloom(intensity, threshold, softness, duration, easing)
    threshold = threshold or 0
    softness = softness or 0
    duration = duration or 0
    easing = easing or "Linear"
    scene._do("set_bloom:" .. tostring(intensity) .. ":" .. tostring(threshold) .. ":" .. tostring(softness) .. ":" .. tostring(duration) .. ":" .. easing, duration)
end

function scene.reset_bloom(duration, easing)
    duration = duration or 0
    easing = easing or "Linear"
    scene._do("reset_bloom:" .. tostring(duration) .. ":" .. easing, duration)
end

function scene.reset_fx(effect, duration, easing)
    duration = duration or 0
    easing = easing or "Linear"
    local cmd = nil
    if effect == "Bloom" then cmd = "reset_bloom"
    elseif effect == "Color Grading" then cmd = "reset_color_grading"
    elseif effect == "Chromatic Aberration" then cmd = "reset_chromatic_aberration"
    elseif effect == "Depth of Field" then cmd = "reset_dof"
    elseif effect == "All Custom FX" then cmd = "reset_custom_fx"
    end
    if cmd then
        scene._do(cmd .. ":" .. tostring(duration) .. ":" .. easing, duration)
    end
end

function scene.set_tonemapping(algorithm)
    coroutine.yield("set_tonemapping:" .. algorithm)
end

function scene.set_color_grading(exposure, temperature, tint, hue, post_saturation, duration, easing)
    temperature = temperature or 0
    tint = tint or 0
    hue = hue or 0
    post_saturation = post_saturation or 1
    duration = duration or 0
    easing = easing or "Linear"
    scene._do("set_color_grading:" .. tostring(exposure) .. ":" .. tostring(temperature) .. ":" .. tostring(tint) .. ":" .. tostring(hue) .. ":" .. tostring(post_saturation) .. ":" .. tostring(duration) .. ":" .. easing, duration)
end

function scene.reset_color_grading(duration, easing)
    duration = duration or 0
    easing = easing or "Linear"
    scene._do("reset_color_grading:" .. tostring(duration) .. ":" .. easing, duration)
end

function scene.set_chromatic_aberration(intensity, duration, easing)
    duration = duration or 0
    easing = easing or "Linear"
    scene._do("set_chromatic_aberration:" .. tostring(intensity) .. ":" .. tostring(duration) .. ":" .. easing, duration)
end

function scene.reset_chromatic_aberration(duration, easing)
    duration = duration or 0
    easing = easing or "Linear"
    scene._do("reset_chromatic_aberration:" .. tostring(duration) .. ":" .. easing, duration)
end

function scene.set_dof(focal_distance, aperture, duration, easing)
    duration = duration or 0
    easing = easing or "Linear"
    scene._do("set_dof:" .. tostring(focal_distance) .. ":" .. tostring(aperture) .. ":" .. tostring(duration) .. ":" .. easing, duration)
end

function scene.reset_dof(duration, easing)
    duration = duration or 0
    easing = easing or "Linear"
    scene._do("reset_dof:" .. tostring(duration) .. ":" .. easing, duration)
end

function scene.set_vignette(intensity, smoothness, roundness, color, duration, easing)
    smoothness = smoothness or 0.5
    roundness = roundness or 1.0
    color = color or {r = 0, g = 0, b = 0}
    duration = duration or 0
    easing = easing or "Linear"
    scene._do("set_vignette:" .. tostring(intensity) .. ":" .. tostring(smoothness) .. ":" .. tostring(roundness) .. ":" .. tostring(color.r) .. ":" .. tostring(color.g) .. ":" .. tostring(color.b) .. ":" .. tostring(duration) .. ":" .. easing, duration)
end

function scene.set_scanlines(intensity, count, speed, duration, easing)
    count = count or 400
    speed = speed or 0
    duration = duration or 0
    easing = easing or "Linear"
    scene._do("set_scanlines:" .. tostring(intensity) .. ":" .. tostring(count) .. ":" .. tostring(speed) .. ":" .. tostring(duration) .. ":" .. easing, duration)
end

function scene.set_film_grain(intensity, speed, duration, easing)
    speed = speed or 1
    duration = duration or 0
    easing = easing or "Linear"
    scene._do("set_film_grain:" .. tostring(intensity) .. ":" .. tostring(speed) .. ":" .. tostring(duration) .. ":" .. easing, duration)
end

function scene.set_fade(color, intensity, duration, easing)
    duration = duration or 0
    easing = easing or "Linear"
    scene._do("set_fade:" .. tostring(color.r) .. ":" .. tostring(color.g) .. ":" .. tostring(color.b) .. ":" .. tostring(intensity) .. ":" .. tostring(duration) .. ":" .. easing, duration)
end

function scene.set_pixelation(cell_size, duration, easing)
    duration = duration or 0
    easing = easing or "Linear"
    scene._do("set_pixelation:" .. tostring(cell_size) .. ":" .. tostring(duration) .. ":" .. easing, duration)
end

function scene.set_color_tint(color, intensity, duration, easing)
    duration = duration or 0
    easing = easing or "Linear"
    scene._do("set_color_tint:" .. tostring(color.r) .. ":" .. tostring(color.g) .. ":" .. tostring(color.b) .. ":" .. tostring(intensity) .. ":" .. tostring(duration) .. ":" .. easing, duration)
end

function scene.set_color_adjust(brightness, contrast, saturation, invert, duration, easing)
    invert = invert or 0
    duration = duration or 0
    easing = easing or "Linear"
    scene._do("set_color_adjust:" .. tostring(brightness) .. ":" .. tostring(contrast) .. ":" .. tostring(saturation) .. ":" .. tostring(invert) .. ":" .. tostring(duration) .. ":" .. easing, duration)
end

function scene.reset_custom_fx(duration, easing)
    duration = duration or 0
    easing = easing or "Linear"
    scene._do("reset_custom_fx:" .. tostring(duration) .. ":" .. easing, duration)
end

function scene.spawn_shockwave(pos, radius, duration, intensity, thickness, chromatic)
    intensity = intensity or 0.04
    thickness = thickness or 40
    chromatic = chromatic or 0.005
    coroutine.yield("spawn_shockwave:" .. tostring(pos.x) .. ":" .. tostring(pos.y) .. ":" .. tostring(radius) .. ":" .. tostring(duration) .. ":" .. tostring(intensity) .. ":" .. tostring(thickness) .. ":" .. tostring(chromatic))
end

function scene.set_sine_wave(amp_x, amp_y, freq, speed, duration, easing)
    freq = freq or 20
    speed = speed or 3
    duration = duration or 0
    easing = easing or "Linear"
    scene._do("set_sine_wave:" .. tostring(amp_x) .. ":" .. tostring(amp_y) .. ":" .. tostring(freq) .. ":" .. tostring(speed) .. ":" .. tostring(duration) .. ":" .. easing, duration)
end

function scene.set_swirl(angle, radius, center_x, center_y, duration, easing)
    radius = radius or 0.5
    center_x = center_x or 0.5
    center_y = center_y or 0.5
    duration = duration or 0
    easing = easing or "Linear"
    scene._do("set_swirl:" .. tostring(angle) .. ":" .. tostring(radius) .. ":" .. tostring(center_x) .. ":" .. tostring(center_y) .. ":" .. tostring(duration) .. ":" .. easing, duration)
end

function scene.set_lens_distortion(intensity, zoom, duration, easing)
    zoom = zoom or 1
    duration = duration or 0
    easing = easing or "Linear"
    scene._do("set_lens_distortion:" .. tostring(intensity) .. ":" .. tostring(zoom) .. ":" .. tostring(duration) .. ":" .. easing, duration)
end

function scene.set_shake(intensity, speed, duration, easing)
    speed = speed or 1
    duration = duration or 0
    easing = easing or "Linear"
    scene._do("set_shake:" .. tostring(intensity) .. ":" .. tostring(speed) .. ":" .. tostring(duration) .. ":" .. easing, duration)
end

function scene.set_zoom(amount, duration, easing)
    duration = duration or 0
    easing = easing or "Linear"
    scene._do("set_zoom:" .. tostring(amount) .. ":" .. tostring(duration) .. ":" .. easing, duration)
end

function scene.set_rotation(angle, duration, easing)
    duration = duration or 0
    easing = easing or "Linear"
    scene._do("set_rotation:" .. tostring(angle) .. ":" .. tostring(duration) .. ":" .. easing, duration)
end

function scene.set_cinema_bars(size, color, duration, easing)
    color = color or {r = 0, g = 0, b = 0}
    duration = duration or 0
    easing = easing or "Linear"
    scene._do("set_cinema_bars:" .. tostring(size) .. ":" .. tostring(color.r) .. ":" .. tostring(color.g) .. ":" .. tostring(color.b) .. ":" .. tostring(duration) .. ":" .. easing, duration)
end

function scene.set_posterize(levels, duration, easing)
    duration = duration or 0
    easing = easing or "Linear"
    scene._do("set_posterize:" .. tostring(levels) .. ":" .. tostring(duration) .. ":" .. easing, duration)
end
"#;

/// Parse a yielded command string into a LuaCommand.
fn parse_yield(yield_str: &str) -> YieldAction {
    let parts: Vec<&str> = yield_str.splitn(20, ':').collect();
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
                speaker_map: Vec::new(),
            }
        }
        "run_yarn_node_at" if parts.len() >= 3 => {
            let blocking = parts.get(3).map(|s| *s == "true").unwrap_or(true);
            // Parse speaker map from "CharName=instance|CharName2=instance2"
            let speaker_map: Vec<(String, String)> = parts[2].split('|')
                .filter(|s| !s.is_empty())
                .filter_map(|pair| {
                    let mut kv = pair.splitn(2, '=');
                    let k = kv.next()?.to_string();
                    let v = kv.next()?.to_string();
                    Some((k, v))
                })
                .collect();
            YieldAction::RunYarnNode {
                node: parts[1].to_string(),
                blocking,
                speaker_map,
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
        "set_ambient" if parts.len() >= 5 => {
            use super::event_bridge::LuaCommand;
            let r: f32 = parts[1].parse().unwrap_or(1.0);
            let g: f32 = parts[2].parse().unwrap_or(1.0);
            let b: f32 = parts[3].parse().unwrap_or(1.0);
            let intensity: f32 = parts[4].parse().unwrap_or(1.0);
            YieldAction::Command {
                cmd: LuaCommand::SetAmbient {
                    color: Color::srgb(r, g, b),
                    intensity,
                },
                wait_movement: None,
            }
        }
        "camera_pan" if parts.len() >= 4 => {
            use super::event_bridge::LuaCommand;
            let x: f32 = parts[1].parse().unwrap_or(0.0);
            let y: f32 = parts[2].parse().unwrap_or(0.0);
            let duration: f32 = parts[3].parse().unwrap_or(1.0);
            YieldAction::Command {
                cmd: LuaCommand::CameraPan {
                    target: Vec2::new(x, y),
                    duration,
                },
                wait_movement: None,
            }
        }
        "camera_shake" if parts.len() >= 3 => {
            use super::event_bridge::LuaCommand;
            let intensity: f32 = parts[1].parse().unwrap_or(5.0);
            let duration: f32 = parts[2].parse().unwrap_or(0.5);
            YieldAction::Command {
                cmd: LuaCommand::CameraShake {
                    intensity,
                    duration,
                },
                wait_movement: None,
            }
        }
        "spawn_particle" if parts.len() >= 4 => {
            use super::event_bridge::LuaCommand;
            let x: f32 = parts[2].parse().unwrap_or(0.0);
            let y: f32 = parts[3].parse().unwrap_or(0.0);
            YieldAction::Command {
                cmd: LuaCommand::SpawnParticle {
                    def_id: parts[1].to_string(),
                    position: Vec2::new(x, y),
                },
                wait_movement: None,
            }
        }
        "screen_flash" if parts.len() >= 5 => {
            use super::event_bridge::LuaCommand;
            let r: f32 = parts[1].parse().unwrap_or(1.0);
            let g: f32 = parts[2].parse().unwrap_or(1.0);
            let b: f32 = parts[3].parse().unwrap_or(1.0);
            let duration: f32 = parts[4].parse().unwrap_or(0.3);
            YieldAction::Command {
                cmd: LuaCommand::ScreenFlash {
                    color: Color::srgb(r, g, b),
                    duration,
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
        // ── Post FX commands — all fire-and-forget via generic PostFx variant ──
        "set_bloom" | "reset_bloom" | "set_tonemapping" | "set_color_grading"
        | "reset_color_grading" | "set_chromatic_aberration" | "reset_chromatic_aberration"
        | "set_dof" | "reset_dof" | "set_vignette" | "set_scanlines" | "set_film_grain"
        | "set_fade" | "set_pixelation" | "set_color_tint" | "set_color_adjust"
        | "set_sine_wave" | "set_swirl" | "set_lens_distortion" | "set_shake"
        | "set_zoom" | "set_rotation" | "set_cinema_bars" | "set_posterize"
        | "reset_custom_fx" | "spawn_shockwave" => {
            use super::event_bridge::LuaCommand;
            let cmd_name = parts[0].to_string();
            let float_args: Vec<f32> = parts[1..].iter()
                .filter_map(|s| s.parse::<f32>().ok())
                .collect();
            let easing = parts[1..].iter()
                .find(|s| s.parse::<f32>().is_err() && !s.is_empty())
                .unwrap_or(&"Linear")
                .to_string();
            YieldAction::Command {
                cmd: LuaCommand::PostFx { command: cmd_name, args: float_args, easing },
                wait_movement: None,
            }
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
    RunYarnNode { node: String, blocking: bool, speaker_map: Vec<(String, String)> },
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
    let new_events: Vec<SceneEvent> = runner.pending_start.drain(..).collect();
    let triggered: Vec<(String, EventTrigger)> = runner.pending_trigger.drain(..).collect();

    if new_events.is_empty() && triggered.is_empty() {
        return;
    }

    // If new events were pushed (map load / save), update the master list
    if !new_events.is_empty() {
        runner.all_events = new_events.clone();
    }

    // Ensure scene API is loaded
    if let Err(e) = vm.lua.load(LUA_SCENE_API).exec() {
        error!("Failed to load scene API: {e}");
        return;
    }

    // Start AutoRun / Parallel events from new_events
    for event in &new_events {
        if !event.enabled {
            continue;
        }
        match event.trigger {
            EventTrigger::AutoRun => {
                if runner.auto_run_fired.contains(&event.id) {
                    continue;
                }
                runner.auto_run_fired.push(event.id.clone());
                start_coroutine(&vm.lua, &mut runner, event, &registry, true, false);
            }
            EventTrigger::Parallel => {
                if runner.coroutines.iter().any(|c| c.event_id == event.id && !c.finished) {
                    continue;
                }
                start_coroutine(&vm.lua, &mut runner, event, &registry, false, true);
            }
            _ => {}
        }
    }

    // Start triggered events (Interact / Touch)
    for (event_id, trigger) in &triggered {
        let Some(event) = runner.all_events.iter().find(|e| e.id == *event_id).cloned() else {
            warn!("Triggered event not found: {event_id}");
            continue;
        };
        // Don't start duplicates
        if runner.coroutines.iter().any(|c| c.event_id == *event_id && !c.finished) {
            continue;
        }
        // Interact/Touch events block like AutoRun (sequential, pauses other events)
        let blocking = matches!(trigger, EventTrigger::Interact | EventTrigger::Touch);
        start_coroutine(&vm.lua, &mut runner, &event, &registry, blocking, false);
    }
}

fn start_coroutine(
    lua: &Lua,
    runner: &mut SceneRunner,
    event: &SceneEvent,
    registry: &SceneActionRegistry,
    blocking: bool,
    parallel: bool,
) {
    let lua_code = super::scene_event::generate_lua(event, registry);
    let kind = if blocking { "blocking" } else if parallel { "parallel" } else { "normal" };
    info!("Starting event '{}' ({kind})\n{}", event.name, &lua_code);

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
        parallel,
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
    let mut yarn_nodes_to_start: Vec<(String, bool, Vec<(String, String)>)> = Vec::new();

    let num = runner.coroutines.len();
    for i in 0..num {
        if runner.coroutines[i].finished {
            continue;
        }
        // When a blocking coroutine (AutoRun) is active, pause everything
        // except the blocker itself and parallel events.
        if has_blocker && !runner.coroutines[i].blocking && !runner.coroutines[i].parallel {
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
                        YieldAction::RunYarnNode { node, blocking, speaker_map } => {
                            runner.coroutines[i].wait_for_dialogue = true;
                            yarn_nodes_to_start.push((node, blocking, speaker_map));
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
                    parallel: false,
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
    let (node, blocking, speaker_map) = runner.pending_yarn_nodes.remove(0);
    crate::dialogue::start_yarn_node(&mut commands, &project, &node, blocking, speaker_map);
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

/// Interaction radius squared (avoids sqrt per object per frame).
const INTERACT_RADIUS_SQ: f32 = 52.0 * 52.0;
/// Touch radius squared.
const TOUCH_RADIUS_SQ: f32 = 24.0 * 24.0;

/// System: detect player proximity to placed objects and fire Interact (action key)
/// or Touch (overlap) events.
pub fn detect_interactions(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut runner: ResMut<SceneRunner>,
    player_q: Query<&Transform, With<crate::dev_scene::Player>>,
    placed_q: Query<(&Transform, &crate::tile_editor::state::PlacedObject), Without<crate::dev_scene::Player>>,
) {
    let action_pressed = keyboard.just_pressed(KeyCode::Space);

    // Don't trigger new events while a blocking one is running
    if runner.has_blocker() {
        if action_pressed {
            debug!("detect_interactions: blocked by running coroutine");
        }
        return;
    }
    // Nothing to trigger if no events are loaded
    if runner.all_events.is_empty() {
        if action_pressed {
            debug!("detect_interactions: no events loaded");
        }
        return;
    }

    let Ok(player_tf) = player_q.single() else {
        if action_pressed {
            debug!("detect_interactions: no player entity");
        }
        return;
    };
    let player_pos = player_tf.translation.truncate();

    if action_pressed {
        debug!("detect_interactions: Space pressed, player at {:?}, checking {} objects", player_pos, placed_q.iter().count());
    }

    for (tf, po) in placed_q.iter() {
        let dist_sq = player_pos.distance_squared(tf.translation.truncate());

        // Skip objects too far for either trigger
        if dist_sq > INTERACT_RADIUS_SQ {
            continue;
        }

        // Use the object's name if set, otherwise "#id" to match the convention
        // used by the scene builder for trigger_target references.
        let name_owned;
        let name = match po.name.as_deref() {
            Some(n) => n,
            None => {
                name_owned = format!("#{}", po.sidecar_id);
                &name_owned
            }
        };

        if action_pressed {
            let dist = dist_sq.sqrt();
            debug!("  object '{name}' at dist {dist:.1}, trigger_target matches: {:?}",
                runner.all_events.iter()
                    .filter(|e| e.trigger == EventTrigger::Interact && e.trigger_target.as_deref() == Some(name))
                    .map(|e| &e.name)
                    .collect::<Vec<_>>()
            );
        }

        if dist_sq < TOUCH_RADIUS_SQ {
            runner.trigger_for_instance(name, EventTrigger::Touch);
        }

        if action_pressed {
            runner.trigger_for_instance(name, EventTrigger::Interact);
        }
    }
}
