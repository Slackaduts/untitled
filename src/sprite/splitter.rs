use bevy::prelude::*;

/// Metadata for splitting a spritesheet atlas at runtime.
#[derive(Component)]
pub struct AtlasMeta {
    pub frame_size: UVec2,
    pub columns: u32,
    pub rows: u32,
}
