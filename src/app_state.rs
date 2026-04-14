use bevy::prelude::*;

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GameState {
    #[default]
    Loading,
    MainMenu,
    Overworld,
    Combat,
    Cutscene,
    Paused,
}

#[derive(SubStates, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[source(GameState = GameState::Combat)]
pub enum CombatPhase {
    #[default]
    GridSetup,
    PlayerTurnSelect,
    PlayerExecute,
    EnemyTurnSelect,
    EnemyExecute,
    Cleanup,
}

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MenuState {
    #[default]
    None,
    Title,
    Pause,
    Inventory,
}
