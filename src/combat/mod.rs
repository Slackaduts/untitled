pub mod action;
pub mod ai;
pub mod cursor;
pub mod grid;
pub mod movement;
pub mod phase;
pub mod proc_system;
pub mod targeting;

use bevy::prelude::*;

use crate::app_state::CombatPhase;

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.add_sub_state::<CombatPhase>();
    }
}
