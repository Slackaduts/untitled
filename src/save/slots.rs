use std::path::PathBuf;

/// Returns the path for a save slot directory.
pub fn slot_path(slot: u32) -> PathBuf {
    PathBuf::from(format!("saves/slot_{}", slot))
}

/// Returns the path for a room's save file within a slot.
pub fn room_path(slot: u32, room_id: &str) -> PathBuf {
    slot_path(slot).join("rooms").join(format!("{}.yaml", room_id))
}

/// Returns the path for the global save file within a slot.
pub fn global_path(slot: u32) -> PathBuf {
    slot_path(slot).join("global.yaml")
}
