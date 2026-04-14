pub mod properties;
#[cfg(feature = "dev_tools")]
pub mod editor;

use bevy::prelude::*;
use properties::BillboardPropertyDefs;

pub struct BillboardPropertiesPlugin;

impl Plugin for BillboardPropertiesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BillboardPropertyDefs>()
            .add_systems(Update, properties::load_billboard_properties)
            .add_systems(Update, properties::apply_collider_depth_overrides);

        #[cfg(feature = "dev_tools")]
        {
            app.init_resource::<editor::BillboardEditorState>()
                .add_systems(Update, editor::billboard_editor_system);
        }
    }
}
