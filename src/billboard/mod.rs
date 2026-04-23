pub mod object_types;
pub mod properties;
#[cfg(feature = "dev_tools")]
pub mod object_editor;

use bevy::prelude::*;
use properties::BillboardPropertyDefs;

pub struct BillboardPropertiesPlugin;

impl Plugin for BillboardPropertiesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BillboardPropertyDefs>()
            .add_systems(Update, properties::load_billboard_properties)
            .add_systems(Update, properties::apply_collider_depth_overrides);

        // Note: object_editor::update_object_light_positions is registered
        // in camera/mod.rs (behind dev_tools feature gate).
    }
}
