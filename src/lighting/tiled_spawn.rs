use bevy::prelude::*;

use super::components::LightSource;
use crate::map::properties::TiledProperties;

/// Automatically attaches a `LightSource` to any Tiled object with a `light_radius` property.
pub fn spawn_lights_from_tiled(
    mut commands: Commands,
    new_props: Query<(Entity, &TiledProperties), Added<TiledProperties>>,
) {
    for (entity, props) in &new_props {
        if let Some(radius) = props.light_radius {
            commands.entity(entity).insert(LightSource {
                outer_radius: radius,
                inner_radius: radius * 0.3,
                ..default()
            });
        }
    }
}
