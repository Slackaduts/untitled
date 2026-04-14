pub mod bgm;
pub mod spatial;

use bevy::prelude::*;

pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<bgm::BgmState>()
            .add_message::<SoundCommand>()
            .add_systems(Update, spatial::update_spatial_falloff);
    }
}

/// Commands for the sound system, usable from Lua via the event bridge.
#[derive(Message, Debug, Clone)]
pub enum SoundCommand {
    /// Play background music, crossfading from current.
    PlayBgm { asset_path: String, fade_in: f32 },
    /// Stop background music with fade out.
    StopBgm { fade_out: f32 },
    /// Play a one-shot global (non-spatial) sound effect.
    PlaySfx { asset_path: String },
    /// Play a one-shot spatial sound at a world position.
    PlaySfxAt { asset_path: String, position: Vec2 },
}
