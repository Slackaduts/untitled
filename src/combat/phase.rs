// CombatPhase is defined in app_state.rs as a SubState.
// This module holds phase transition logic.

use bevy::prelude::*;

use crate::app_state::CombatPhase;

/// Event to request advancing to the next combat phase.
#[derive(Event)]
pub struct AdvancePhase;

pub fn next_phase(current: &CombatPhase) -> CombatPhase {
    match current {
        CombatPhase::GridSetup => CombatPhase::PlayerTurnSelect,
        CombatPhase::PlayerTurnSelect => CombatPhase::PlayerExecute,
        CombatPhase::PlayerExecute => CombatPhase::EnemyTurnSelect,
        CombatPhase::EnemyTurnSelect => CombatPhase::EnemyExecute,
        CombatPhase::EnemyExecute => CombatPhase::Cleanup,
        CombatPhase::Cleanup => CombatPhase::PlayerTurnSelect,
    }
}
