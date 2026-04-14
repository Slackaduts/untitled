use bevy::prelude::*;

/// Scripted camera operations triggered by Lua cutscenes.
#[derive(Event)]
pub enum CameraCommand {
    PanTo { target: Vec2, duration: f32 },
    Shake { intensity: f32, duration: f32 },
    Flash { color: Color, duration: f32 },
    Zoom { level: f32, duration: f32 },
}
