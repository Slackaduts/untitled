//! Dialogue system powered by Yarn Spinner with a custom Bevy native UI.
//!
//! Dialogue is authored in `.yarn` files under `assets/dialogue/` and triggered
//! from the scene builder via a "Run Yarn Node" action. The system renders a
//! bottom-screen dialogue box with typewriter text, speaker portraits, and
//! branching choices.
//!
//! Two dialogue modes:
//! - **Run Yarn Node** — classic bottom-screen dialogue box
//! - **Run Yarn Node At** — speech-bubble positioned near a speaker entity

pub mod box_ui;
pub mod portraits;
pub mod state;
pub mod yarn_bridge;

use bevy::prelude::*;
use bevy_yarnspinner::prelude::*;

use state::{DialogueState, ChoiceSelection};

pub struct DialoguePlugin;

impl Plugin for DialoguePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(YarnSpinnerPlugin::new())
            .init_resource::<portraits::PortraitCache>()
            .add_systems(Startup, load_dialogue_font)
            .add_systems(
                Update,
                setup_dialogue_runner.run_if(resource_added::<YarnProject>),
            )
            .add_systems(
                Update,
                (
                    box_ui::spawn_dialogue_box.run_if(resource_added::<DialogueState>),
                    // Start the yarn node ONE FRAME after DialogueState is inserted,
                    // so spawn_dialogue_box has created the UI entities first.
                    start_deferred_yarn_node
                        .after(box_ui::spawn_dialogue_box)
                        .run_if(resource_exists::<DialogueState>),
                    box_ui::animate_dialogue_fade,
                    box_ui::cleanup_faded_dialogue,
                    box_ui::update_typewriter.run_if(resource_exists::<DialogueState>),
                    box_ui::handle_dialogue_input.run_if(resource_exists::<DialogueState>),
                    box_ui::animate_choice_selection.run_if(resource_exists::<ChoiceSelection>),
                    box_ui::animate_chosen_expansion,
                    box_ui::spawn_effect_glyphs.run_if(resource_exists::<DialogueState>),
                    box_ui::update_effect_glyph_positions
                        .after(box_ui::spawn_effect_glyphs)
                        .run_if(resource_exists::<DialogueState>),
                    box_ui::update_effect_glyph_visibility.run_if(resource_exists::<DialogueState>),
                    box_ui::animate_effect_glyphs
                        .after(box_ui::update_effect_glyph_positions)
                        .run_if(resource_exists::<DialogueState>),
                    box_ui::update_speech_bubble_position.run_if(resource_exists::<DialogueState>),
                    box_ui::update_choice_position.run_if(resource_exists::<DialogueState>),
                ),
            );
    }
}

fn load_dialogue_font(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.insert_resource(state::DialogueFont {
        regular: asset_server.load(state::DIALOGUE_FONT_REGULAR),
        bold: asset_server.load(state::DIALOGUE_FONT_BOLD),
    });
}

/// System: when the YarnProject finishes compiling, log that it's ready.
fn setup_dialogue_runner(_project: Res<YarnProject>) {
    info!("YarnProject ready");
}

/// System: starts the yarn node once the UI entities have been flushed into the world.
///
/// `spawn_dialogue_box` creates UI via deferred commands, so the entities don't
/// exist until the next command flush. This system waits until [`DialogueBodyText`]
/// exists before calling `start_node`, guaranteeing the `PresentLine` observer
/// can find the UI entities.
fn start_deferred_yarn_node(
    mut state: ResMut<DialogueState>,
    mut runners: Query<&mut DialogueRunner>,
    ui_ready: Query<Entity, With<state::DialogueBodyText>>,
) {
    if state.pending_start_node.is_none() {
        return;
    }
    // Wait until the UI entities exist (commands from spawn_dialogue_box flushed)
    if ui_ready.is_empty() {
        return;
    }
    let node_name = state.pending_start_node.take().unwrap();
    let Ok(mut runner) = runners.get_mut(state.runner_entity) else {
        warn!("start_deferred_yarn_node: DialogueRunner entity missing");
        return;
    };
    runner.start_node(&node_name);
    info!("Deferred start of yarn node '{node_name}'");
}

/// Prepare a yarn dialogue node. Called from the Lua runner.
///
/// Spawns a [`DialogueRunner`] entity with observers, inserts [`DialogueState`]
/// with `pending_start_node`. The actual `start_node` call is deferred to
/// [`start_deferred_yarn_node`] so the UI entities exist first.
pub fn start_yarn_node(
    commands: &mut Commands,
    project: &YarnProject,
    node_name: &str,
    blocking: bool,
    speaker_map: Vec<(String, String)>,
) {
    let mut dialogue_runner = project.create_dialogue_runner(commands);

    // Register custom commands
    dialogue_runner.commands_mut()
        .add_command("set_flag", commands.register_system(yarn_bridge::set_flag_command))
        .add_command("play_sfx", commands.register_system(yarn_bridge::play_sfx_command))
        .add_command("shake", commands.register_system(yarn_bridge::shake_command));

    // Spawn entity with observers — do NOT call start_node yet.
    let entity = commands.spawn(dialogue_runner)
        .observe(box_ui::on_present_line)
        .observe(box_ui::on_present_options)
        .observe(box_ui::on_dialogue_completed)
        .id();

    // Insert state with pending node name — start_deferred_yarn_node will
    // call start_node after spawn_dialogue_box has created the UI.
    commands.insert_resource(DialogueState {
        runner_entity: entity,
        blocking,
        speaker_map,
        pending_start_node: Some(node_name.to_string()),
        fading_out: false,
    });

    info!("Prepared yarn node '{node_name}' (blocking={blocking})");
}
