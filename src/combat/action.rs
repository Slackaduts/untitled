use bevy::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitType {
    Melee,
    Ranged,
    Magic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AoePattern {
    Single,
    Line { length: u32 },
    Cross { radius: u32 },
    Diamond { radius: u32 },
    Square { radius: u32 },
}

#[derive(Debug, Clone)]
pub struct AbilityRange {
    pub min: u32,
    pub max: u32,
}

/// An ability that can be used in combat.
#[derive(Component, Debug, Clone)]
pub struct Ability {
    pub id: String,
    pub name: String,
    pub hit_type: HitType,
    pub range: AbilityRange,
    pub aoe: AoePattern,
    pub base_power: i32,
    pub mp_cost: i32,
}
