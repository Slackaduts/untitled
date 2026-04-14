use serde::{Deserialize, Serialize};

/// Logical input actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InputAction {
    MoveUp,
    MoveDown,
    MoveLeft,
    MoveRight,
    Confirm,
    Cancel,
    Menu,
    CursorUp,
    CursorDown,
    CursorLeft,
    CursorRight,
}

/// A single action-to-key binding using string key names.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionBinding {
    pub action: InputAction,
    pub key: String,
}

/// Maps logical actions to physical keys, loaded from YAML config.
#[derive(Default, Serialize, Deserialize)]
pub struct InputMapping {
    pub bindings: Vec<ActionBinding>,
}
