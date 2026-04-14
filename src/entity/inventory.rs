use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub description: String,
    pub stackable: bool,
    pub max_stack: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemStack {
    pub item_id: String,
    pub count: u32,
}

#[derive(Component, Debug, Clone, Default, Serialize, Deserialize)]
pub struct Inventory {
    pub items: Vec<ItemStack>,
}
