use super::traits::SpriteFormat;
use bevy::prelude::*;

/// LPC Universal Spritesheet format (64x64 frames, 13 columns).
pub struct LpcFormat;

impl SpriteFormat for LpcFormat {
    fn frame_size(&self) -> UVec2 {
        UVec2::new(64, 64)
    }

    fn columns(&self) -> u32 {
        13
    }

    fn row_for(&self, animation: &str, direction: u8) -> Option<u32> {
        // LPC standard rows: spellcast(0-3), thrust(4-7), walk(8-11), slash(12-15), shoot(16-19), hurt(20)
        let base = match animation {
            "spellcast" => 0,
            "thrust" => 4,
            "walk" => 8,
            "slash" => 12,
            "shoot" => 16,
            "hurt" => return Some(20),
            _ => return None,
        };
        // direction: 0=up, 1=left, 2=down, 3=right
        Some(base + direction as u32)
    }

    fn frame_count(&self, animation: &str) -> u32 {
        match animation {
            "spellcast" => 7,
            "thrust" => 8,
            "walk" => 9,
            "slash" => 6,
            "shoot" => 13,
            "hurt" => 6,
            _ => 1,
        }
    }
}
