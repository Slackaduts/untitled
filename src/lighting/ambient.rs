use bevy::prelude::*;

/// Global ambient light configuration.
#[derive(Resource)]
pub struct AmbientConfig {
    pub color: Color,
    pub intensity: f32,
}

impl Default for AmbientConfig {
    fn default() -> Self {
        Self {
            color: Color::WHITE,
            intensity: 1.0,
        }
    }
}
