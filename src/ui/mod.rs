pub mod battle_ui;
pub mod dialogue_box;
pub mod hud;
pub mod main_menu;
pub mod pause_menu;

use bevy::prelude::*;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(bevy_egui::EguiPlugin);
    }
}
