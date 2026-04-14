use bevy::prelude::*;

use super::action::AoePattern;

/// Highlighted cells showing valid targets.
#[derive(Resource, Default)]
pub struct TargetHighlights {
    pub cells: Vec<IVec2>,
}

/// Resolve the set of cells affected by an AoE pattern centered on `origin`.
pub fn resolve_aoe(pattern: &AoePattern, origin: IVec2) -> Vec<IVec2> {
    match pattern {
        AoePattern::Single => vec![origin],
        AoePattern::Line { length } => {
            (0..*length as i32).map(|i| origin + IVec2::new(i, 0)).collect()
        }
        AoePattern::Cross { radius } => {
            let r = *radius as i32;
            let mut cells = vec![origin];
            for i in 1..=r {
                cells.push(origin + IVec2::new(i, 0));
                cells.push(origin + IVec2::new(-i, 0));
                cells.push(origin + IVec2::new(0, i));
                cells.push(origin + IVec2::new(0, -i));
            }
            cells
        }
        AoePattern::Diamond { radius } => {
            let r = *radius as i32;
            let mut cells = Vec::new();
            for dx in -r..=r {
                for dy in -r..=r {
                    if dx.abs() + dy.abs() <= r {
                        cells.push(origin + IVec2::new(dx, dy));
                    }
                }
            }
            cells
        }
        AoePattern::Square { radius } => {
            let r = *radius as i32;
            let mut cells = Vec::new();
            for dx in -r..=r {
                for dy in -r..=r {
                    cells.push(origin + IVec2::new(dx, dy));
                }
            }
            cells
        }
    }
}
