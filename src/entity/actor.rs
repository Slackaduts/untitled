use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Allegiance {
    Player,
    Enemy,
    Neutral,
}

#[derive(Component, Debug, Clone, Serialize, Deserialize)]
pub struct Stats {
    pub hp: i32,
    pub max_hp: i32,
    pub mp: i32,
    pub max_mp: i32,
    pub attack: i32,
    pub defense: i32,
    pub magic: i32,
    pub speed: i32,
    pub movement: u32,
}

/// Core actor identity.
#[derive(Component, Debug, Clone)]
pub struct Actor {
    pub id: String,
    pub name: String,
    pub allegiance: Allegiance,
}

/// Marks an entity as participating in the current combat.
#[derive(Component)]
pub struct CombatActor {
    pub grid_pos: IVec2,
    pub has_moved: bool,
    pub has_acted: bool,
}
