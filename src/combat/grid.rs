use bevy::prelude::*;

/// The combat grid, generated dynamically from actor positions.
#[derive(Resource)]
pub struct CombatGrid {
    pub origin: IVec2,
    pub width: u32,
    pub height: u32,
    pub walkable: Vec<bool>,
}

impl CombatGrid {
    pub fn is_walkable(&self, x: i32, y: i32) -> bool {
        let lx = x - self.origin.x;
        let ly = y - self.origin.y;
        if lx < 0 || ly < 0 || lx >= self.width as i32 || ly >= self.height as i32 {
            return false;
        }
        self.walkable[(ly as u32 * self.width + lx as u32) as usize]
    }
}

/// Allows a Lua script to override grid generation entirely.
#[derive(Component)]
pub struct ScriptedGridOverride {
    pub origin: IVec2,
    pub width: u32,
    pub height: u32,
}

/// Marks an entity as occupying a full grid square during combat.
#[derive(Component)]
pub struct CombatObstacle;
