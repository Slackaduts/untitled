pub mod battle_ui;
pub mod hud;
pub mod main_menu;
pub mod pause_menu;

use bevy::prelude::*;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        // Disable multipass mode so egui UI systems can run in Update
        // (multipass requires EguiPrimaryContextPass schedule instead).
        #[allow(deprecated)]
        app.add_plugins(bevy_egui::EguiPlugin {
            enable_multipass_for_primary_context: false,
            ..default()
        });
    }
}
