use bevy::prelude::*;

/// Which action set is currently active.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputContext {
    #[default]
    Overworld,
    Combat,
    Menu,
    Dialogue,
    Cutscene,
}
