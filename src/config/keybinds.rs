use serde::{Deserialize, Serialize};

/// YAML schema for keybind configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeybindConfig {
    pub bindings: Vec<KeybindEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeybindEntry {
    pub action: String,
    pub key: String,
}
