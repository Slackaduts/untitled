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

        #[cfg(feature = "dev_tools")]
        {
            app.init_resource::<object_editor::ObjectEditorState>()
                .add_systems(
                    Update,
                    (
                        object_editor::toggle_object_editor,
                        object_editor::scan_objects
                            .after(object_editor::toggle_object_editor),
                        object_editor::object_editor_ui
                            .after(object_editor::scan_objects),
                        object_editor::live_preview_system
                            .after(object_editor::object_editor_ui),
                        object_editor::draw_object_light_gizmos,
                    ),
                );
        }
    }
}
