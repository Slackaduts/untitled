use bevy::prelude::*;

/// When a proc triggers (e.g. "on hit", "on crit").
#[derive(Debug, Clone)]
pub enum ProcTrigger {
    OnHit,
    OnCrit,
    OnKill,
    OnDamageTaken,
}

/// Condition that must be met for the proc to fire.
#[derive(Debug, Clone)]
pub enum ProcCondition {
    Always,
    ChancePercent(u32),
    TargetBelowHpPercent(u32),
}

/// Effect applied when a proc fires.
#[derive(Debug, Clone)]
pub enum ProcEffect {
    BonusDamage(i32),
    Heal(i32),
    ApplyStatus(String),
    BonusHit { ability_id: String },
}

/// A proc attached to an ability or equipment.
#[derive(Component, Debug, Clone)]
pub struct Proc {
    pub trigger: ProcTrigger,
    pub condition: ProcCondition,
    pub effect: ProcEffect,
}
