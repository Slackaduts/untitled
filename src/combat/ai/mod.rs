pub mod default_behaviors;

use bevy::prelude::*;

/// A planned AI turn: move to a destination, then use an ability on a target.
#[derive(Debug, Clone)]
pub struct AiTurnPlan {
    pub move_path: Vec<IVec2>,
    pub ability_id: String,
    pub target: IVec2,
}

/// Trait for AI behavior evaluation.
pub trait AiBehavior: Send + Sync + 'static {
    /// Evaluate and return the best turn plan for the given entity.
    fn plan(&self, entity: Entity, world: &World) -> Option<AiTurnPlan>;
}
