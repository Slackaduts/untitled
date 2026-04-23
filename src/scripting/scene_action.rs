//! Unified scene action system for Lua scripting and the scene builder UI.
//!
//! Every Lua-callable scene function is described by a [`SceneActionDef`] that
//! provides metadata (category, argument descriptors) for the scene builder
//! and a Lua function name for code generation.
//!
//! The [`SceneActionRegistry`] collects all definitions at startup so the
//! editor can enumerate available actions by category.

use bevy::prelude::*;

// ── Argument descriptors ──────────────────────────────────────────────────

/// Describes what kind of value an argument expects.
/// The scene builder UI uses this to render the appropriate input widget.
#[derive(Debug, Clone, PartialEq)]
pub enum ArgType {
    /// A string literal (free text).
    String,
    /// A floating-point number.
    Float { min: f32, max: f32, default: f32 },
    /// An integer.
    Int { min: i32, max: i32, default: i32 },
    /// A boolean toggle.
    Bool { default: bool },
    /// An instance name — scene builder shows an object picker.
    InstanceRef,
    /// A sub-reference (e.g. "instance.light_0") — picker for lights/emitters.
    SubRef,
    /// A 2D position — scene builder shows a coordinate picker or map click.
    Position,
    /// An RGB color.
    Color,
    /// A direction (0=up, 1=left, 2=down, 3=right) — dropdown.
    Direction,
    /// A choice from a fixed set of string options.
    Choice(Vec<&'static str>),
    /// An easing function (Bevy EaseFunction names).
    Easing,
    /// A bezier spline path (edited visually, not via form widgets).
    Spline,
    /// A yarn dialogue node name — scene builder shows a searchable dropdown
    /// populated from the compiled YarnProject.
    YarnNode,
}

impl ArgType {
    /// Whether this arg type has a built-in default value.
    pub fn has_default(&self) -> bool {
        matches!(self,
            Self::Float { .. } | Self::Int { .. } | Self::Bool { .. }
            | Self::Easing | Self::Direction | Self::Color | Self::Spline
        )
    }

    /// Generate the default Lua literal for this arg type.
    pub fn default_lua(&self) -> Option<String> {
        match self {
            Self::Float { default, .. } => Some(format!("{default}")),
            Self::Int { default, .. } => Some(format!("{default}")),
            Self::Bool { default } => Some(format!("{default}")),
            Self::Easing => Some("\"Linear\"".to_string()),
            Self::Spline => Some("{}".to_string()),
            Self::Direction => Some("\"down\"".to_string()),
            Self::Color => Some("{r = 1.0, g = 1.0, b = 1.0}".to_string()),
            _ => None,
        }
    }
}

/// Easing function names matching Bevy's `EaseFunction` enum.
pub const EASING_NAMES: &[&str] = &[
    "Linear",
    "QuadraticIn", "QuadraticOut", "QuadraticInOut",
    "CubicIn", "CubicOut", "CubicInOut",
    "QuarticIn", "QuarticOut", "QuarticInOut",
    "QuinticIn", "QuinticOut", "QuinticInOut",
    "SineIn", "SineOut", "SineInOut",
    "ExponentialIn", "ExponentialOut", "ExponentialInOut",
    "CircularIn", "CircularOut", "CircularInOut",
    "ElasticIn", "ElasticOut", "ElasticInOut",
    "BackIn", "BackOut", "BackInOut",
    "BounceIn", "BounceOut", "BounceInOut",
];

/// One argument in a scene action's signature.
#[derive(Debug, Clone)]
pub struct ArgDef {
    /// Argument name (used as Lua table key).
    pub name: &'static str,
    /// Human-readable label for the scene builder.
    pub label: &'static str,
    /// The type of value expected.
    pub arg_type: ArgType,
    /// Whether this argument is optional (nil-able in Lua).
    pub optional: bool,
}

// ── Action definition ─────────────────────────────────────────────────────

/// Category for grouping actions in the scene builder dropdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionCategory {
    Movement,
    Camera,
    Dialogue,
    Lighting,
    Sound,
    Vfx,
    Combat,
    World,
    Flow,
}

impl ActionCategory {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Movement => "Movement",
            Self::Camera => "Camera",
            Self::Dialogue => "Dialogue",
            Self::Lighting => "Lighting",
            Self::Sound => "Sound",
            Self::Vfx => "VFX",
            Self::Combat => "Combat",
            Self::World => "World",
            Self::Flow => "Flow",
        }
    }

    pub fn all() -> &'static [ActionCategory] {
        &[
            Self::Movement, Self::Camera, Self::Dialogue,
            Self::Lighting, Self::Sound, Self::Vfx,
            Self::Combat, Self::World, Self::Flow,
        ]
    }
}

/// Describes a single scene action that can be invoked from Lua or the scene builder.
#[derive(Debug, Clone)]
pub struct SceneActionDef {
    /// Unique identifier (e.g. "move_to", "camera_pan").
    pub id: &'static str,
    /// Human-readable name for the scene builder UI.
    pub label: &'static str,
    /// Category for grouping in dropdowns.
    pub category: ActionCategory,
    /// Lua function name (e.g. "scene.move_to").
    pub lua_fn: &'static str,
    /// Ordered list of arguments.
    pub args: Vec<ArgDef>,
    /// Brief description shown as tooltip in the scene builder.
    pub description: &'static str,
}

impl SceneActionDef {
    /// Generate a Lua code snippet for this action with placeholder argument values.
    pub fn lua_template(&self) -> String {
        let args_str: Vec<String> = self.args.iter().map(|a| {
            match &a.arg_type {
                ArgType::String | ArgType::InstanceRef | ArgType::SubRef => format!("\"{}\"", a.name),
                ArgType::Float { default, .. } => format!("{default}"),
                ArgType::Int { default, .. } => format!("{default}"),
                ArgType::Bool { default } => format!("{default}"),
                ArgType::Position => "{{x = 0, y = 0}}".to_string(),
                ArgType::Color => "{{r = 1.0, g = 1.0, b = 1.0}}".to_string(),
                ArgType::Direction => "\"down\"".to_string(),
                ArgType::Choice(opts) => format!("\"{}\"", opts.first().unwrap_or(&"")),
                ArgType::Easing => "\"Linear\"".to_string(),
                ArgType::Spline => "{}".to_string(),
                ArgType::YarnNode => "\"NodeName\"".to_string(),
            }
        }).collect();
        format!("{}({})", self.lua_fn, args_str.join(", "))
    }
}

// ── Registry ──────────────────────────────────────────────────────────────

/// Resource holding all registered scene action definitions.
/// Populated at startup by [`register_builtin_actions`].
#[derive(Resource, Default)]
pub struct SceneActionRegistry {
    pub actions: Vec<SceneActionDef>,
}

impl SceneActionRegistry {
    /// Get all actions in a given category.
    pub fn by_category(&self, cat: ActionCategory) -> Vec<&SceneActionDef> {
        self.actions.iter().filter(|a| a.category == cat).collect()
    }

    /// Find an action by its unique ID.
    pub fn get(&self, id: &str) -> Option<&SceneActionDef> {
        self.actions.iter().find(|a| a.id == id)
    }
}

// ── Built-in action definitions ───────────────────────────────────────────

/// Registers all built-in scene actions into the registry.
pub fn register_builtin_actions(mut registry: ResMut<SceneActionRegistry>) {
    registry.actions = vec![
        // ── Movement ──
        SceneActionDef {
            id: "move_to",
            label: "Move To",
            category: ActionCategory::Movement,
            lua_fn: "scene.move_to",
            description: "Move an instance to a target position",
            args: vec![
                ArgDef { name: "target", label: "Instance", arg_type: ArgType::InstanceRef, optional: false },
                ArgDef { name: "position", label: "Position", arg_type: ArgType::Position, optional: false },
                ArgDef { name: "speed", label: "Speed", arg_type: ArgType::Float { min: 0.1, max: 100.0, default: 2.0 }, optional: false },
                ArgDef { name: "easing", label: "Easing", arg_type: ArgType::Easing, optional: true },
            ],
        },
        SceneActionDef {
            id: "bezier_move_to",
            label: "Bezier Move To",
            category: ActionCategory::Movement,
            lua_fn: "scene.bezier_move_to",
            description: "Move an instance along a bezier spline curve in 3D",
            args: vec![
                ArgDef { name: "target", label: "Instance", arg_type: ArgType::InstanceRef, optional: false },
                ArgDef { name: "path", label: "Spline Path", arg_type: ArgType::Spline, optional: false },
                ArgDef { name: "duration", label: "Duration (s)", arg_type: ArgType::Float { min: 0.1, max: 30.0, default: 2.0 }, optional: false },
                ArgDef { name: "easing", label: "Easing", arg_type: ArgType::Easing, optional: true },
            ],
        },
        SceneActionDef {
            id: "face",
            label: "Face Direction",
            category: ActionCategory::Movement,
            lua_fn: "scene.face",
            description: "Face an instance in a direction",
            args: vec![
                ArgDef { name: "target", label: "Instance", arg_type: ArgType::InstanceRef, optional: false },
                ArgDef { name: "direction", label: "Direction", arg_type: ArgType::Direction, optional: false },
            ],
        },
        SceneActionDef {
            id: "wait_for_movement",
            label: "Wait for Movement",
            category: ActionCategory::Movement,
            lua_fn: "scene.wait_for_movement",
            description: "Pause script until an instance finishes moving",
            args: vec![
                ArgDef { name: "target", label: "Instance", arg_type: ArgType::InstanceRef, optional: false },
            ],
        },

        // ── Camera ──
        SceneActionDef {
            id: "camera_pan",
            label: "Pan Camera",
            category: ActionCategory::Camera,
            lua_fn: "scene.camera_pan",
            description: "Smoothly pan the camera to a target",
            args: vec![
                ArgDef { name: "target", label: "Target", arg_type: ArgType::Position, optional: false },
                ArgDef { name: "duration", label: "Duration (s)", arg_type: ArgType::Float { min: 0.0, max: 30.0, default: 1.0 }, optional: false },
                ArgDef { name: "easing", label: "Easing", arg_type: ArgType::Easing, optional: true },
            ],
        },
        SceneActionDef {
            id: "camera_shake",
            label: "Shake Camera",
            category: ActionCategory::Camera,
            lua_fn: "scene.camera_shake",
            description: "Shake the camera for impact effect",
            args: vec![
                ArgDef { name: "intensity", label: "Intensity", arg_type: ArgType::Float { min: 0.0, max: 50.0, default: 5.0 }, optional: false },
                ArgDef { name: "duration", label: "Duration (s)", arg_type: ArgType::Float { min: 0.0, max: 10.0, default: 0.5 }, optional: false },
            ],
        },

        // ── Dialogue ──
        SceneActionDef {
            id: "run_yarn_node",
            label: "Run Yarn Node",
            category: ActionCategory::Dialogue,
            lua_fn: "scene.run_yarn_node",
            description: "Start a Yarn Spinner dialogue node (bottom-screen box)",
            args: vec![
                ArgDef { name: "node", label: "Node Name", arg_type: ArgType::YarnNode, optional: false },
                ArgDef { name: "blocking", label: "Blocking", arg_type: ArgType::Bool { default: true }, optional: false },
            ],
        },
        SceneActionDef {
            id: "run_yarn_node_at",
            label: "Run Yarn Node At",
            category: ActionCategory::Dialogue,
            lua_fn: "scene.run_yarn_node_at",
            description: "Start a Yarn dialogue as a speech bubble near a speaker instance",
            args: vec![
                ArgDef { name: "node", label: "Node Name", arg_type: ArgType::YarnNode, optional: false },
                ArgDef { name: "speaker", label: "Speaker", arg_type: ArgType::InstanceRef, optional: false },
                ArgDef { name: "blocking", label: "Blocking", arg_type: ArgType::Bool { default: true }, optional: false },
            ],
        },

        // ── Lighting ──
        SceneActionDef {
            id: "set_ambient",
            label: "Set Ambient Light",
            category: ActionCategory::Lighting,
            lua_fn: "scene.set_ambient",
            description: "Change ambient lighting color and intensity",
            args: vec![
                ArgDef { name: "color", label: "Color", arg_type: ArgType::Color, optional: false },
                ArgDef { name: "intensity", label: "Intensity", arg_type: ArgType::Float { min: 0.0, max: 5.0, default: 1.0 }, optional: false },
            ],
        },
        SceneActionDef {
            id: "set_time_of_day",
            label: "Set Time of Day",
            category: ActionCategory::Lighting,
            lua_fn: "scene.set_time_of_day",
            description: "Set the world time (affects sun/ambient)",
            args: vec![
                ArgDef { name: "hour", label: "Hour (0-24)", arg_type: ArgType::Float { min: 0.0, max: 24.0, default: 12.0 }, optional: false },
            ],
        },

        // ── Sound ──
        SceneActionDef {
            id: "play_bgm",
            label: "Play BGM",
            category: ActionCategory::Sound,
            lua_fn: "scene.play_bgm",
            description: "Start background music",
            args: vec![
                ArgDef { name: "path", label: "Asset Path", arg_type: ArgType::String, optional: false },
                ArgDef { name: "fade_in", label: "Fade In (s)", arg_type: ArgType::Float { min: 0.0, max: 10.0, default: 1.0 }, optional: false },
            ],
        },
        SceneActionDef {
            id: "play_sfx",
            label: "Play SFX",
            category: ActionCategory::Sound,
            lua_fn: "scene.play_sfx",
            description: "Play a sound effect",
            args: vec![
                ArgDef { name: "path", label: "Asset Path", arg_type: ArgType::String, optional: false },
            ],
        },

        // ── VFX ──
        SceneActionDef {
            id: "spawn_particle",
            label: "Spawn Particle",
            category: ActionCategory::Vfx,
            lua_fn: "scene.spawn_particle",
            description: "Spawn a particle effect at a position",
            args: vec![
                ArgDef { name: "def_id", label: "Definition", arg_type: ArgType::String, optional: false },
                ArgDef { name: "position", label: "Position", arg_type: ArgType::Position, optional: false },
            ],
        },
        SceneActionDef {
            id: "screen_flash",
            label: "Screen Flash",
            category: ActionCategory::Vfx,
            lua_fn: "scene.screen_flash",
            description: "Flash the screen with a color",
            args: vec![
                ArgDef { name: "color", label: "Color", arg_type: ArgType::Color, optional: false },
                ArgDef { name: "duration", label: "Duration (s)", arg_type: ArgType::Float { min: 0.0, max: 5.0, default: 0.3 }, optional: false },
            ],
        },

        // ── World ──
        SceneActionDef {
            id: "set_flag",
            label: "Set Flag",
            category: ActionCategory::World,
            lua_fn: "scene.set_flag",
            description: "Set a game flag (persistent variable)",
            args: vec![
                ArgDef { name: "key", label: "Flag Name", arg_type: ArgType::String, optional: false },
                ArgDef { name: "value", label: "Value", arg_type: ArgType::String, optional: false },
            ],
        },
        SceneActionDef {
            id: "map_transition",
            label: "Map Transition",
            category: ActionCategory::World,
            lua_fn: "scene.map_transition",
            description: "Transition to another map",
            args: vec![
                ArgDef { name: "target_map", label: "Target Map", arg_type: ArgType::String, optional: false },
                ArgDef { name: "spawn_x", label: "Spawn X", arg_type: ArgType::Int { min: 0, max: 999, default: 0 }, optional: false },
                ArgDef { name: "spawn_y", label: "Spawn Y", arg_type: ArgType::Int { min: 0, max: 999, default: 0 }, optional: false },
            ],
        },

        // ── Flow ──
        SceneActionDef {
            id: "wait",
            label: "Wait",
            category: ActionCategory::Flow,
            lua_fn: "scene.wait",
            description: "Pause the script for a duration",
            args: vec![
                ArgDef { name: "seconds", label: "Seconds", arg_type: ArgType::Float { min: 0.0, max: 60.0, default: 1.0 }, optional: false },
            ],
        },
        SceneActionDef {
            id: "call_script",
            label: "Call Script",
            category: ActionCategory::Flow,
            lua_fn: "scene.call_script",
            description: "Call a Script-triggered event by name and wait for it to finish",
            args: vec![
                ArgDef { name: "name", label: "Script Name", arg_type: ArgType::String, optional: false },
            ],
        },
        SceneActionDef {
            id: "parallel_start",
            label: "Parallel Start",
            category: ActionCategory::Flow,
            lua_fn: "",
            description: "Begin a parallel block — all actions until Parallel End run concurrently",
            args: vec![],
        },
        SceneActionDef {
            id: "parallel_end",
            label: "Parallel End",
            category: ActionCategory::Flow,
            lua_fn: "",
            description: "End a parallel block and wait for all parallel actions to finish",
            args: vec![],
        },
    ];
}
