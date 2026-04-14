use bevy::prelude::*;

/// Commands pushed by Lua API calls, drained by Bevy systems in PostUpdate.
#[derive(Message, Debug, Clone)]
pub enum LuaCommand {
    // Camera
    CameraPan { target: Vec2, duration: f32 },
    CameraShake { intensity: f32, duration: f32 },
    CameraZoom { level: f32, duration: f32 },

    // Combat
    StartCombat { encounter_id: String },
    DealDamage { target: Entity, amount: i32 },
    Heal { target: Entity, amount: i32 },

    // Dialogue
    ShowDialogue { speaker: String, text: String, portrait: Option<String> },
    ShowChoice { options: Vec<String> },

    // Movement
    MoveTo { entity: Entity, target: Vec2, speed: f32 },
    Face { entity: Entity, direction: u8 },

    // VFX
    SpawnParticle { def_id: String, position: Vec2 },
    ScreenFlash { color: Color, duration: f32 },

    // Sound
    PlayBgm { asset_path: String, fade_in: f32 },
    StopBgm { fade_out: f32 },
    PlaySfx { asset_path: String },
    PlaySfxAt { asset_path: String, position: Vec2 },

    // Lighting
    SetAmbient { color: Color, intensity: f32 },
    SpawnLight { position: Vec2, color: Color, intensity: f32, radius: f32 },
    SetTimeOfDay { hour: f32 },
    SetTimeSpeed { speed: f32 },

    // World
    SpawnEntity { template_id: String, position: Vec2 },
    SetFlag { key: String, value: String },
    GetFlag { key: String },
}
