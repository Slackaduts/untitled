use bevy::prelude::*;

use super::{AiBehavior, AiTurnPlan};

/// Default AI: for each ability, find reachable positions where the ability
/// can hit an enemy, score by damage, pick the best (destination, ability, target) triple.
pub struct DefaultAi;

impl AiBehavior for DefaultAi {
    fn plan(&self, _entity: Entity, _world: &World) -> Option<AiTurnPlan> {
        // Stub — will be implemented with pathfinding + ability range queries
        None
    }
}
