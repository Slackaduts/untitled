use bevy::prelude::*;

/// Path computed via A* for a combat unit's movement.
#[derive(Component)]
pub struct MovePath {
    pub steps: Vec<IVec2>,
}

/// Visual arrow preview polyline showing the planned path.
#[derive(Component)]
pub struct PathArrow;
