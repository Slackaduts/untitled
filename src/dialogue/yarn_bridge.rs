//! Custom Yarn Spinner commands that bridge dialogue to game systems.
//!
//! These are registered on the [`DialogueRunner`] so that `.yarn` files can
//! call them with `<<command_name arg1 arg2>>`.

use bevy::prelude::*;

use crate::scripting::event_bridge::LuaCommand;

/// `<<set_flag key value>>` — set a persistent game flag.
pub fn set_flag_command(
    In((key, value)): In<(String, String)>,
    mut cmd_writer: MessageWriter<LuaCommand>,
) {
    info!("Yarn <<set_flag {key} {value}>>");
    cmd_writer.write(LuaCommand::SetFlag { key, value });
}

/// `<<play_sfx path>>` — play a sound effect during dialogue.
pub fn play_sfx_command(
    In(path): In<String>,
    mut cmd_writer: MessageWriter<LuaCommand>,
) {
    info!("Yarn <<play_sfx {path}>>");
    cmd_writer.write(LuaCommand::PlaySfx { asset_path: path });
}

/// `<<shake intensity duration>>` — shake the camera during dialogue.
pub fn shake_command(
    In((intensity, duration)): In<(f32, f32)>,
    mut cmd_writer: MessageWriter<LuaCommand>,
) {
    info!("Yarn <<shake {intensity} {duration}>>");
    cmd_writer.write(LuaCommand::CameraShake { intensity, duration });
}
