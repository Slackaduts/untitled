//! Scene event data model, persistence, and Lua code generation.
//!
//! Each map has an `.events.json` file alongside its `.tmx` and `.objects.json`.
//! Events are composed of triggers and ordered action lists that generate Lua scripts.

use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::scene_action::SceneActionRegistry;

// ── Data model ────────────────────────────────────────────────────────────

/// A single scene event for a map.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SceneEvent {
    /// Unique ID within the file (auto-incremented).
    pub id: String,
    /// User-friendly label.
    pub name: String,
    /// What causes this event to fire.
    pub trigger: EventTrigger,
    /// Which placed object instance triggers this (for Interact/Touch).
    #[serde(default)]
    pub trigger_target: Option<String>,
    /// Ordered list of actions to execute.
    #[serde(default)]
    pub actions: Vec<EventAction>,
    /// Whether this event is active.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// What causes a scene event to fire.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub enum EventTrigger {
    /// Player presses action key on the target instance.
    #[default]
    Interact,
    /// Player steps on the target instance's tile.
    Touch,
    /// Fires once when the map loads.
    AutoRun,
    /// Runs continuously in the background (coroutine loops).
    Parallel,
    /// Called from other scripts via `scene.call_script("name")`.
    Script,
}

impl EventTrigger {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Interact => "Interact",
            Self::Touch => "Touch",
            Self::AutoRun => "Auto Run",
            Self::Parallel => "Parallel",
            Self::Script => "Script",
        }
    }

    pub fn all() -> &'static [EventTrigger] {
        &[
            Self::Interact,
            Self::Touch,
            Self::AutoRun,
            Self::Parallel,
            Self::Script,
        ]
    }

    /// Whether this trigger requires a target instance.
    pub fn needs_target(&self) -> bool {
        matches!(self, Self::Interact | Self::Touch)
    }
}

/// A single action step within an event.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EventAction {
    /// Matches `SceneActionDef.id` (e.g. "move_to", "show_dialogue").
    pub action_id: String,
    /// Concrete argument values, keyed by `ArgDef.name`.
    #[serde(default)]
    pub args: HashMap<String, ActionArgValue>,
}

/// A concrete value for an action argument.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum ActionArgValue {
    String(String),
    Float(f64),
    Int(i64),
    Bool(bool),
    Position([f32; 2]),
    Color([f32; 3]),
    /// A bezier spline: list of waypoints, each with [x, y, z, handle_in_x, handle_in_y, handle_in_z, handle_out_x, handle_out_y, handle_out_z].
    SplinePoints(Vec<SplineWaypoint>),
    /// Speaker mapping: pairs of [yarn_character_name, instance_name].
    SpeakerMap(Vec<[String; 2]>),
}

/// A single waypoint in a bezier spline.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SplineWaypoint {
    /// Grid position XY.
    pub pos: [f32; 2],
    /// Z height above ground (in tiles).
    pub z: f32,
    /// Incoming tangent handle offset from pos (grid units).
    pub handle_in: [f32; 2],
    /// Incoming handle Z offset.
    pub handle_in_z: f32,
    /// Outgoing tangent handle offset from pos (grid units).
    pub handle_out: [f32; 2],
    /// Outgoing handle Z offset.
    pub handle_out_z: f32,
}

impl ActionArgValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f32(&self) -> f32 {
        match self {
            Self::Float(f) => *f as f32,
            Self::Int(i) => *i as f32,
            _ => 0.0,
        }
    }

    pub fn as_i32(&self) -> i32 {
        match self {
            Self::Int(i) => *i as i32,
            Self::Float(f) => *f as i32,
            _ => 0,
        }
    }

    pub fn as_bool(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            _ => false,
        }
    }

    /// Generate Lua literal for this value.
    pub fn to_lua(&self) -> String {
        match self {
            Self::String(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
            Self::Float(f) => format!("{f}"),
            Self::Int(i) => format!("{i}"),
            Self::Bool(b) => format!("{b}"),
            Self::Position([x, y]) => format!("{{x = {x}, y = {y}}}"),
            Self::Color([r, g, b]) => format!("{{r = {r}, g = {g}, b = {b}}}"),
            Self::SplinePoints(pts) => {
                let entries: Vec<String> = pts.iter().map(|wp| {
                    format!(
                        "{{x={}, y={}, z={}, hi_x={}, hi_y={}, hi_z={}, ho_x={}, ho_y={}, ho_z={}}}",
                        wp.pos[0], wp.pos[1], wp.z,
                        wp.handle_in[0], wp.handle_in[1], wp.handle_in_z,
                        wp.handle_out[0], wp.handle_out[1], wp.handle_out_z,
                    )
                }).collect();
                format!("{{{}}}", entries.join(", "))
            }
            Self::SpeakerMap(pairs) => {
                let entries: Vec<String> = pairs.iter().map(|[char_name, instance]| {
                    format!("[\"{}\"] = \"{}\"", char_name.replace('"', "\\\""), instance.replace('"', "\\\""))
                }).collect();
                format!("{{{}}}", entries.join(", "))
            }
        }
    }
}

// ── File format ───────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct MapEventsFile {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub events: Vec<SceneEvent>,
}

fn default_version() -> u32 {
    1
}

// ── I/O ───────────────────────────────────────────────────────────────────

/// Derive the events file path from a TMX path.
/// `"assets/maps/test.tmx"` → `"assets/maps/test.events.json"`
pub fn events_path_for(tmx_path: &str) -> String {
    if let Some(base) = tmx_path.strip_suffix(".tmx") {
        format!("{base}.events.json")
    } else {
        format!("{tmx_path}.events.json")
    }
}

/// Load events from disk.
pub fn load_events(tmx_path: &str) -> Option<MapEventsFile> {
    let path = events_path_for(tmx_path);
    let content = std::fs::read_to_string(&path).ok()?;
    match serde_json::from_str(&content) {
        Ok(file) => Some(file),
        Err(e) => {
            error!("Failed to parse {path}: {e}");
            None
        }
    }
}

/// Save events to disk.
pub fn save_events(tmx_path: &str, file: &MapEventsFile) -> Result<(), String> {
    let path = events_path_for(tmx_path);
    let json = serde_json::to_string_pretty(file)
        .map_err(|e| format!("Serialize error: {e}"))?;
    std::fs::write(&path, &json)
        .map_err(|e| format!("Write error for {path}: {e}"))?;
    info!("Saved {} events to {path}", file.events.len());
    Ok(())
}

/// Generate the next unique ID for a new event.
pub fn next_event_id(file: &MapEventsFile) -> String {
    let max = file
        .events
        .iter()
        .filter_map(|e| e.id.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    (max + 1).to_string()
}

// ── Lua code generation ───────────────────────────────────────────────────

/// Generate a single Lua function call for an action, or None if incomplete/unknown.
fn generate_action_lua(action: &EventAction, registry: &SceneActionRegistry) -> Option<String> {
    let def = registry.get(&action.action_id)?;

    // Skip marker actions (parallel_start/end have no lua_fn)
    if def.lua_fn.is_empty() {
        return None;
    }

    // Check all required args are present
    let has_all_required = def.args.iter().all(|arg_def| {
        arg_def.optional
            || action.args.contains_key(arg_def.name)
            || arg_def.arg_type.has_default()
    });
    if !has_all_required {
        return None;
    }

    let arg_strs: Vec<String> = def
        .args
        .iter()
        .map(|arg_def| {
            if let Some(val) = action.args.get(arg_def.name) {
                val.to_lua()
            } else if let Some(default_lua) = arg_def.arg_type.default_lua() {
                default_lua
            } else {
                "nil".to_string()
            }
        })
        .collect();

    Some(format!("{}({})", def.lua_fn, arg_strs.join(", ")))
}

/// Generate a complete Lua script for a scene event.
pub fn generate_lua(event: &SceneEvent, registry: &SceneActionRegistry) -> String {
    let mut lines = Vec::new();

    // Header comments
    lines.push(format!("-- Event: {}", event.name));
    let trigger_info = match &event.trigger {
        EventTrigger::Interact => {
            let target = event.trigger_target.as_deref().unwrap_or("?");
            format!("interact(\"{}\")", target)
        }
        EventTrigger::Touch => {
            let target = event.trigger_target.as_deref().unwrap_or("?");
            format!("touch(\"{}\")", target)
        }
        EventTrigger::AutoRun => "auto_run".to_string(),
        EventTrigger::Parallel => "parallel".to_string(),
        EventTrigger::Script => format!("script(\"{}\")", event.name),
    };
    lines.push(format!("-- Trigger: {trigger_info}"));
    lines.push(String::new());
    lines.push("function run()".to_string());

    // Action body — handles parallel blocks
    let mut i = 0;
    while i < event.actions.len() {
        let action = &event.actions[i];

        // Handle parallel block: collect actions between parallel_start..parallel_end
        if action.action_id == "parallel_start" {
            let mut parallel_actions: Vec<String> = Vec::new();
            i += 1;
            while i < event.actions.len() && event.actions[i].action_id != "parallel_end" {
                if let Some(lua_line) = generate_action_lua(&event.actions[i], registry) {
                    parallel_actions.push(lua_line);
                }
                i += 1;
            }
            if i < event.actions.len() {
                i += 1; // skip parallel_end
            }

            if parallel_actions.is_empty() {
                continue;
            }

            // Generate parallel coroutine block.
            // Each child action runs in its own coroutine. The parent drives
            // them round-robin, re-yielding each child's commands to the runner.
            // Wait commands are tracked per-child so they don't block siblings.
            lines.push("  -- parallel block".to_string());
            lines.push("  do".to_string());
            lines.push("    local threads = {}".to_string());
            lines.push("    local wait_until = {}".to_string());
            for (j, action_lua) in parallel_actions.iter().enumerate() {
                lines.push(format!("    threads[{}] = coroutine.create(function() {} end)", j + 1, action_lua));
                lines.push(format!("    wait_until[{}] = 0", j + 1));
            }
            lines.push("    local all_done = false".to_string());
            lines.push("    while not all_done do".to_string());
            lines.push("      coroutine.yield(\"parallel_tick\")".to_string());
            lines.push("      local dt = scene._dt or 0.016".to_string());
            lines.push("      all_done = true".to_string());
            lines.push("      for i, t in ipairs(threads) do".to_string());
            lines.push("        if coroutine.status(t) ~= \"dead\" then".to_string());
            lines.push("          if wait_until[i] > 0 then".to_string());
            lines.push("            wait_until[i] = wait_until[i] - dt".to_string());
            lines.push("            all_done = false".to_string());
            lines.push("          else".to_string());
            lines.push("            local ok, val = coroutine.resume(t)".to_string());
            lines.push("            if not ok then error(val) end".to_string());
            lines.push("            if coroutine.status(t) ~= \"dead\" then".to_string());
            lines.push("              all_done = false".to_string());
            lines.push("              -- Handle wait inside parallel, don't re-yield it".to_string());
            lines.push("              if val and val:sub(1,5) == \"wait:\" then".to_string());
            lines.push("                wait_until[i] = tonumber(val:sub(6)) or 0".to_string());
            lines.push("              elseif val then".to_string());
            lines.push("                coroutine.yield(val)".to_string());
            lines.push("              end".to_string());
            lines.push("            end".to_string());
            lines.push("          end".to_string());
            lines.push("        end".to_string());
            lines.push("      end".to_string());
            lines.push("    end".to_string());
            lines.push("  end".to_string());
            continue;
        }

        // Skip standalone parallel_end (shouldn't happen but be safe)
        if action.action_id == "parallel_end" {
            i += 1;
            continue;
        }

        // Normal sequential action
        if let Some(lua_line) = generate_action_lua(action, registry) {
            lines.push(format!("  {lua_line}"));
        } else {
            let Some(def) = registry.get(&action.action_id) else {
                lines.push(format!("  -- unknown action: {}", action.action_id));
                i += 1;
                continue;
            };
            lines.push(format!("  -- skipped {}: incomplete args", def.id));
        }
        i += 1;
    }

    lines.push("end".to_string());
    lines.join("\n")
}

/// Ensure the per-map yarn dialogue folder and `.yarnproject` exist.
/// Creates `assets/dialogue/<map_name>/` with a `Project.yarnproject` if missing.
/// Returns the folder path.
pub fn ensure_yarn_project(tmx_path: &str) -> Result<String, String> {
    let map_name = map_name_from_tmx(tmx_path);
    let dir = format!("assets/dialogue/{map_name}");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create {dir}: {e}"))?;

    let project_path = format!("{dir}/Project.yarnproject");
    if !std::path::Path::new(&project_path).exists() {
        let project_json = serde_json::json!({
            "projectFileVersion": 4,
            "sourceFiles": ["**/*.yarn"],
            "excludeFiles": [],
            "baseLanguage": "en",
            "projectName": &map_name,
            "authorName": [],
            "editorOptions": {
                "yarnScriptEditor": {
                    "presenter": "try",
                    "characters": []
                }
            }
        });
        let json = serde_json::to_string_pretty(&project_json)
            .map_err(|e| format!("Serialize error: {e}"))?;
        std::fs::write(&project_path, &json)
            .map_err(|e| format!("Failed to write {project_path}: {e}"))?;
        info!("Created yarn project at {project_path}");
    }
    Ok(dir)
}

/// Extract the map name from a TMX path.
/// `"assets/maps/test.tmx"` → `"test"`
fn map_name_from_tmx(tmx_path: &str) -> String {
    let base = tmx_path.strip_suffix(".tmx").unwrap_or(tmx_path);
    std::path::Path::new(base)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Extract all unique speaker/character names from a yarn node.
/// Parses raw `.yarn` files looking for `CharacterName: text` lines within the given node.
pub fn extract_yarn_speakers(tmx_path: &str, node_name: &str) -> Vec<String> {
    let map_name = map_name_from_tmx(tmx_path);
    let dir = format!("assets/dialogue/{map_name}");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut speakers = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map_or(true, |e| e != "yarn") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };

        // Find the node section
        let mut in_node = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("title:") {
                let title = trimmed["title:".len()..].trim();
                in_node = title == node_name;
                continue;
            }
            if trimmed == "---" {
                continue; // body start marker
            }
            if trimmed == "===" {
                if in_node {
                    break; // end of this node
                }
                continue;
            }
            if !in_node {
                continue;
            }

            // Skip commands, options, comments
            let stripped = trimmed.trim_start_matches(|c: char| c == ' ' || c == '\t' || c == '>');
            if stripped.starts_with("->") || stripped.starts_with("<<") || stripped.starts_with("//") || stripped.is_empty() {
                continue;
            }

            // Check for "CharacterName: dialogue text" pattern.
            // Yarn Spinner uses "Name: text" where the colon is followed by a space.
            // Names can contain spaces, digits, underscores (e.g. "Guard 1").
            if let Some(colon_pos) = stripped.find(": ") {
                let candidate = stripped[..colon_pos].trim();
                // Must be non-empty, only word chars and spaces, and not start
                // with a bracket (which would be a markup tag like [b]).
                if !candidate.is_empty()
                    && !candidate.starts_with('[')
                    && candidate.chars().all(|c| c.is_alphanumeric() || c == '_' || c == ' ')
                    && !speakers.contains(&candidate.to_string())
                {
                    speakers.push(candidate.to_string());
                }
            }
        }
        if in_node {
            break; // found and parsed the node
        }
    }
    speakers
}

/// Check whether any event in the list uses a dialogue action (run_yarn_node).
pub fn has_dialogue_actions(events: &[SceneEvent]) -> bool {
    events.iter().any(|e| {
        e.actions.iter().any(|a| a.action_id == "run_yarn_node")
    })
}

/// Save the generated Lua script to disk.
pub fn save_lua_script(tmx_path: &str, event: &SceneEvent, lua_code: &str) -> Result<(), String> {
    let map_name = map_name_from_tmx(tmx_path);

    let dir = format!("assets/scripts/maps/{map_name}");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create {dir}: {e}"))?;

    let script_name = if event.name.is_empty() {
        format!("event_{}", event.id)
    } else {
        event.name.replace(' ', "_").to_lowercase()
    };
    let path = format!("{dir}/{script_name}.lua");
    std::fs::write(&path, lua_code)
        .map_err(|e| format!("Failed to write {path}: {e}"))?;
    info!("Saved Lua script to {path}");
    Ok(())
}
