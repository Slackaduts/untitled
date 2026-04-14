use bevy::prelude::*;

/// The player's grid cursor in combat.
#[derive(Resource, Default)]
pub struct GridCursor {
    pub position: IVec2,
}
