use bevy::prelude::*;

/// Parsed custom properties from Tiled objects.
#[derive(Component)]
pub struct TiledProperties {
    pub combat_zone: bool,
    pub trigger_script: Option<String>,
    pub light_radius: Option<f32>,
}
