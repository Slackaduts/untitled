use bevy::prelude::*;

/// Pluggable sprite sheet format. Implement this for custom spritesheet layouts.
pub trait SpriteFormat: Send + Sync + 'static {
    /// Frame size in pixels.
    fn frame_size(&self) -> UVec2;
    /// Number of columns in the sheet.
    fn columns(&self) -> u32;
    /// Row index for a given animation name and direction.
    fn row_for(&self, animation: &str, direction: u8) -> Option<u32>;
    /// Number of frames in a given animation.
    fn frame_count(&self, animation: &str) -> u32;
}
