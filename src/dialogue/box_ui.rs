//! Bevy native UI dialogue box and speech bubbles.
//!
//! **Bottom-screen box**: Soft dark background (no hard borders), floating
//! speaker name tab above left, full-width text.
//!
//! **Speech bubble**: Floating box above the speaker entity, same styling.
//!
//! **Choices**: Selector stays fixed, options scroll smoothly. Selected option
//! is bright and full-size; unselected are dimmer and smaller.

use bevy::prelude::*;
use bevy::text::{Justify, LineBreak, TextLayout, TextLayoutInfo};
use bevy_yarnspinner::prelude::*;
use bevy_yarnspinner::events::{PresentLine, PresentOptions, DialogueCompleted};

use super::portraits;
use super::state::*;

// ── Theme colors ─────────────────────────────────────────────────────────

const SPEAKER_COLOR: Color = Color::srgba(0.333, 0.667, 1.0, 1.0);
const TEXT_COLOR: Color = Color::srgba(0.941, 0.941, 0.941, 1.0);

const CHOICE_SELECTED: Color = Color::srgba(1.0, 0.867, 0.267, 1.0);
const CHOICE_UNSELECTED: Color = Color::srgba(0.5, 0.5, 0.45, 0.5);

const CHOICE_FONT_SELECTED: f32 = 16.0;
const CHOICE_FONT_UNSELECTED: f32 = 13.0;

/// Dark shadow behind all dialogue text for readability over the game scene.
fn text_shadow() -> TextShadow {
    TextShadow {
        offset: Vec2::new(1.5, 1.5),
        color: Color::srgba(0.0, 0.0, 0.0, 0.9),
    }
}

/// Bright outer glow for the currently selected choice.
fn glow_shadow() -> TextShadow {
    TextShadow {
        offset: Vec2::new(1.0, 1.0),
        color: Color::srgba(1.0, 0.75, 0.1, 0.8),
    }
}

/// Dim shadow for unselected choices.
fn dim_shadow() -> TextShadow {
    TextShadow {
        offset: Vec2::new(1.0, 1.0),
        color: Color::srgba(0.0, 0.0, 0.0, 0.8),
    }
}

// ── Layout constants ─────────────────────────────────────────────────────

const BOX_HEIGHT: f32 = 140.0;
const BOX_MARGIN_H: f32 = 24.0;
const BOX_MARGIN_BOTTOM: f32 = 16.0;
const BOX_PADDING: f32 = 16.0;
const NAME_TAB_GAP: f32 = 4.0;

const BUBBLE_Z_OFFSET: f32 = 64.0;
const BUBBLE_MIN_GAP_PX: f32 = 12.0;
const BUBBLE_SCREEN_MARGIN: f32 = 8.0;

// ── Spawn / Despawn ──────────────────────────────────────────────────────

pub fn spawn_dialogue_box(
    mut commands: Commands,
    dialogue_state: Res<DialogueState>,
    existing: Query<(Entity, Option<&DialogueFade>), With<DialogueBoxRoot>>,
    dialogue_font: Res<DialogueFont>,
) {
    // If there are existing entities, check if they're fading out.
    // Fading-out entities: despawn immediately to make room for new ones.
    // Active (non-fading) entities: don't double-spawn.
    let mut has_active = false;
    for (entity, fade) in existing.iter() {
        if fade.is_some_and(|f| f.despawn_on_fade_out) {
            commands.entity(entity).despawn();
        } else {
            has_active = true;
        }
    }
    if has_active {
        return;
    }
    let font = dialogue_font.regular.clone();
    if dialogue_state.speaker_instance.is_some() {
        spawn_speech_bubble(&mut commands, &dialogue_state, &font);
    } else {
        spawn_bottom_box(&mut commands, &font);
    }
}

fn spawn_bottom_box(commands: &mut Commands, font: &Handle<Font>) {
    let box_bottom = BOX_MARGIN_BOTTOM;
    let tab_bottom = box_bottom + BOX_HEIGHT + NAME_TAB_GAP;

    // Name tab — transparent container, centered text
    commands.spawn((
        DialogueBoxRoot,
        DialogueFade::fade_in(),
        DialogueNameTab,
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(tab_bottom),
            left: Val::Px(BOX_MARGIN_H + 16.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        ZIndex(101),
        Visibility::Hidden,
    )).with_children(|tab| {
        tab.spawn((
            DialogueSpeakerName,
            Text::new(""),
            TextFont { font: font.clone(), font_size: 15.0, ..default() },
            TextColor(SPEAKER_COLOR),
            TextLayout::new(Justify::Center, LineBreak::NoWrap),
            text_shadow(),
            BaseAlpha(1.0),
        ));
    });

    // Main text box — fixed height, transparent container
    commands.spawn((
        DialogueBoxRoot,
        DialogueFade::fade_in(),
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(box_bottom),
            left: Val::Px(BOX_MARGIN_H),
            right: Val::Px(BOX_MARGIN_H),
            height: Val::Px(BOX_HEIGHT),
            padding: UiRect::all(Val::Px(BOX_PADDING)),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            ..default()
        },
        ZIndex(100),
    )).with_children(|mb| {
        mb.spawn((
            DialogueBodyText,
            TypewriterState::new(String::new()),
            Text::new(""),
            TextFont { font: font.clone(), font_size: 16.0, ..default() },
            TextColor(TEXT_COLOR),
            TextLayout::new(Justify::Center, LineBreak::WordBoundary),
            text_shadow(),
            BaseAlpha(1.0),
        ));
        // Continue indicator — child of the main box, flows below text
        mb.spawn((
            DialogueContinueIndicator,
            Text::new("\u{25BC}"),
            TextFont { font: font.clone(), font_size: 14.0, ..default() },
            TextColor(TEXT_COLOR.with_alpha(0.5)),
            TextLayout::new(Justify::Center, LineBreak::NoWrap),
            text_shadow(),
            BaseAlpha(0.5),
            Node { align_self: AlignSelf::Center, ..default() },
            Visibility::Hidden,
        ));
    });

    // Choice list — transparent container, positioned near the player
    commands.spawn((
        DialogueBoxRoot,
        DialogueFade::fade_in(),
        DialogueChoiceList,
        ChoiceAtPlayer { speaker_instance: None },
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            ..default()
        },
        ZIndex(101),
        Visibility::Hidden,
    ));

    info!("Dialogue box spawned (bottom-screen)");
}

fn spawn_speech_bubble(commands: &mut Commands, state: &DialogueState, font: &Handle<Font>) {
    let instance_name = state.speaker_instance.as_ref().unwrap();

    // Name tab — centered text
    commands.spawn((
        DialogueBoxRoot,
        DialogueFade::fade_in(),
        DialogueNameTab,
        SpeechBubbleAnchor { instance_name: instance_name.clone() },
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            max_width: Val::Px(320.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        ZIndex(101),
        Visibility::Hidden,
    )).with_children(|tab| {
        tab.spawn((
            DialogueSpeakerName,
            Text::new(""),
            TextFont { font: font.clone(), font_size: 13.0, ..default() },
            TextColor(SPEAKER_COLOR),
            TextLayout::new(Justify::Center, LineBreak::NoWrap),
            text_shadow(),
            BaseAlpha(1.0),
        ));
    });

    // Main bubble
    commands.spawn((
        DialogueBoxRoot,
        DialogueFade::fade_in(),
        SpeechBubbleAnchor { instance_name: instance_name.clone() },
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            max_width: Val::Px(320.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(2.0),
            ..default()
        },
        ZIndex(100),
    )).with_children(|bubble| {
        bubble.spawn((
            DialogueBodyText,
            TypewriterState::new(String::new()),
            Text::new(""),
            TextFont { font: font.clone(), font_size: 14.0, ..default() },
            TextColor(TEXT_COLOR),
            TextLayout::new(Justify::Center, LineBreak::WordBoundary),
            text_shadow(),
            BaseAlpha(1.0),
        ));
        // Continue indicator — child of bubble, flows below text
        bubble.spawn((
            DialogueContinueIndicator,
            Text::new("\u{25BC}"),
            TextFont { font: font.clone(), font_size: 12.0, ..default() },
            TextColor(TEXT_COLOR.with_alpha(0.5)),
            TextLayout::new(Justify::Center, LineBreak::NoWrap),
            text_shadow(),
            BaseAlpha(0.5),
            Node { align_self: AlignSelf::Center, ..default() },
            Visibility::Hidden,
        ));
    });

    // Choice list — positioned near the player
    commands.spawn((
        DialogueBoxRoot,
        DialogueFade::fade_in(),
        DialogueChoiceList,
        ChoiceAtPlayer { speaker_instance: Some(instance_name.clone()) },
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            max_width: Val::Px(320.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            ..default()
        },
        ZIndex(101),
        Visibility::Hidden,
    ));

    info!("Speech bubble spawned for '{instance_name}'");
}

/// Despawn dialogue UI entities that have finished fading out.
pub fn cleanup_faded_dialogue(
    mut commands: Commands,
    fade_q: Query<(Entity, &DialogueFade), With<DialogueBoxRoot>>,
) {
    let mut any_fading = false;
    let mut all_done = true;
    for (entity, fade) in fade_q.iter() {
        if fade.despawn_on_fade_out {
            any_fading = true;
            if fade.opacity <= 0.01 {
                commands.entity(entity).despawn();
            } else {
                all_done = false;
            }
        }
    }
    // Once all fading entities are despawned, clean up resources
    if any_fading && all_done {
        commands.remove_resource::<DialogueState>();
        commands.remove_resource::<ChoiceSelection>();
    }
}

// ── Present Line (Observer) ──────────────────────────────────────────────

pub fn on_present_line(
    trigger: On<PresentLine>,
    mut commands: Commands,
    mut speaker_q: Query<&mut Text, (With<DialogueSpeakerName>, Without<DialogueBodyText>)>,
    mut body_q: Query<(Entity, &mut Text, &mut TypewriterState), With<DialogueBodyText>>,
    mut choice_fade_q: Query<&mut DialogueFade, (With<DialogueChoiceList>, With<DialogueBoxRoot>)>,
    mut name_tab_vis: Query<&mut Visibility, (With<DialogueNameTab>, Without<DialogueChoiceList>, Without<DialogueContinueIndicator>)>,
    mut indicator_vis: Query<&mut Visibility, (With<DialogueContinueIndicator>, Without<DialogueNameTab>, Without<DialogueChoiceList>)>,
    dialogue_font: Res<DialogueFont>,
) {
    let line = &trigger.event().line;
    let speaker = line.character_name().unwrap_or("").to_string();

    let auto_advance = line.metadata.iter().find_map(|tag| {
        let tag = tag.trim_start_matches('#');
        if tag == "auto" {
            Some(3.0_f32)
        } else if let Some(secs) = tag.strip_prefix("auto:") {
            secs.parse::<f32>().ok()
        } else {
            None
        }
    });

    // Parse markup attributes into styled segments
    let segments = parse_markup_segments(line);

    if let Ok(mut text) = speaker_q.single_mut() {
        **text = speaker.clone();
    }
    if let Ok(mut vis) = name_tab_vis.single_mut() {
        *vis = if speaker.is_empty() { Visibility::Hidden } else { Visibility::Visible };
    }

    if let Ok((body_entity, mut text, mut tw)) = body_q.single_mut() {
        // Despawn old span children
        for old in tw.span_entities.drain(..) {
            commands.entity(old).despawn();
        }
        // Despawn old effect glyph entities
        for old in tw.effect_glyph_entities.drain(..) {
            commands.entity(old).despawn();
        }
        commands.entity(body_entity).remove::<EffectGlyphsSpawned>();

        // Root text is empty — all content goes in TextSpan children
        **text = " ".to_string();
        *tw = TypewriterState::new_styled(segments);
        tw.auto_advance = auto_advance;

        // Spawn one TextSpan child per segment.
        // Effect segments (shake/wave) get full text immediately (for layout)
        // but with alpha 0 — visibility is controlled by EffectGlyph entities.
        let font = &dialogue_font.regular;
        let bold_font = &dialogue_font.bold;
        let segments = tw.segments.clone();

        // Remove the parent TextShadow for lines with effects — the parent's
        // shadow renders ALL glyphs (including transparent effect spans) in
        // dark, creating a visible "black clone" artifact.
        let has_effects = segments.iter().any(|s| s.shake || s.wave);
        if has_effects {
            commands.entity(body_entity).remove::<TextShadow>();
        } else {
            commands.entity(body_entity).insert(text_shadow());
        }

        for seg in &segments {
            let is_effect = seg.shake || seg.wave;
            // Effect spans: full text for layout, but fully transparent so
            // only the EffectGlyph ImageNode overlays are visible.
            let span_color = if is_effect {
                Color::srgba(0.0, 0.0, 0.0, 0.0)
            } else {
                seg.color.unwrap_or(TEXT_COLOR)
            };
            // Add TextShadow to all spans so the fade system's query
            // (TextColor, TextShadow, BaseAlpha) can control their alpha
            // during dialogue fade-out.
            let span_shadow = if is_effect {
                TextShadow { offset: Vec2::ZERO, color: Color::srgba(0.0, 0.0, 0.0, 0.0) }
            } else {
                text_shadow()
            };
            let span_entity = commands.spawn((
                TextSpan::new(if is_effect { seg.text.clone() } else { String::new() }),
                TextFont {
                    font: if seg.bold { bold_font.clone() } else { font.clone() },
                    font_size: 14.0,
                    ..default()
                },
                TextColor(span_color),
                span_shadow,
                BaseAlpha(if is_effect { 0.0 } else { 1.0 }),
            )).id();

            commands.entity(body_entity).add_child(span_entity);
            tw.span_entities.push(span_entity);
        }
    }

    for mut fade in choice_fade_q.iter_mut() {
        if fade.opacity > 0.01 {
            fade.target_opacity = 0.0;
            fade.fade_speed_override = Some(FADE_OUT_SPEED);
        }
    }

    if let Ok(mut vis) = indicator_vis.single_mut() {
        *vis = Visibility::Hidden;
    }
}

/// Parse YarnSpinner markup attributes into styled text segments.
fn parse_markup_segments(line: &bevy_yarnspinner::prelude::LocalizedLine) -> Vec<StyledSegment> {
    let body = line.text_without_character_name();
    let body_chars: Vec<char> = body.chars().collect();

    if body_chars.is_empty() {
        return vec![StyledSegment {
            text: String::new(), color: None, bold: false, shake: false, wave: false,
        }];
    }

    // Skip the "character" attribute — it's just the speaker name
    let attrs: Vec<_> = line.attributes.iter()
        .filter(|a| a.name != "character")
        .collect();

    if attrs.is_empty() {
        return vec![StyledSegment {
            text: body, color: None, bold: false, shake: false, wave: false,
        }];
    }

    // Build a per-character style map
    #[derive(Clone, Default)]
    struct CharStyle {
        color: Option<Color>,
        bold: bool,
        shake: bool,
        wave: bool,
    }

    // Adjust attribute positions relative to body text (after character name removal)
    let char_name_len = line.character_name().map(|n| n.len() + 2).unwrap_or(0); // "Name: " prefix

    let mut styles = vec![CharStyle::default(); body_chars.len()];

    for attr in &attrs {
        // Attribute positions are relative to the full text (including "Speaker: ")
        // text_without_character_name() strips the "Speaker: " prefix
        let start = attr.position.saturating_sub(char_name_len);
        let end = (start + attr.length).min(body_chars.len());

        for i in start..end {
            match attr.name.as_str() {
                "color" => {
                    if let Some(val) = attr.properties.get("color") {
                        if let bevy_yarnspinner::prelude::MarkupValue::String(name) = val {
                            styles[i].color = color_from_name(name);
                        }
                    }
                    // Also check the first positional property
                    if styles[i].color.is_none() {
                        for (key, val) in &attr.properties {
                            if let bevy_yarnspinner::prelude::MarkupValue::String(name) = val {
                                if key != "color" {
                                    if let Some(c) = color_from_name(name) {
                                        styles[i].color = Some(c);
                                    }
                                }
                            }
                        }
                    }
                }
                "b" | "bold" => styles[i].bold = true,
                "shake" => styles[i].shake = true,
                "wave" => styles[i].wave = true,
                _ => {}
            }
        }
    }

    // Merge consecutive characters with the same style into segments
    let mut segments = Vec::new();
    let mut seg_start = 0;
    while seg_start < body_chars.len() {
        let ref_style = &styles[seg_start];
        let mut seg_end = seg_start + 1;

        // Merge consecutive chars with same style
        while seg_end < body_chars.len() {
            let s = &styles[seg_end];
            if s.color != ref_style.color || s.bold != ref_style.bold
                || s.shake != ref_style.shake || s.wave != ref_style.wave
            {
                break;
            }
            seg_end += 1;
        }

        segments.push(StyledSegment {
            text: body_chars[seg_start..seg_end].iter().collect(),
            color: ref_style.color,
            bold: ref_style.bold,
            shake: ref_style.shake,
            wave: ref_style.wave,
        });
        seg_start = seg_end;
    }

    segments
}

fn _parse_speaker_expression(speaker: &str) -> (&str, usize) {
    if let Some(pos) = speaker.rfind('_') {
        let suffix = &speaker[pos + 1..];
        if portraits::EXPRESSION_NAMES.contains(&suffix) {
            return (&speaker[..pos], portraits::expression_index(suffix));
        }
    }
    (speaker, 0)
}

// ── Present Options (Observer) ───────────────────────────────────────────

pub fn on_present_options(
    trigger: On<PresentOptions>,
    mut commands: Commands,
    mut choice_list: Query<(Entity, &mut Visibility, Option<&mut DialogueFade>), With<DialogueChoiceList>>,
    existing_buttons: Query<Entity, With<DialogueChoiceButton>>,
    dialogue_font: Res<DialogueFont>,
) {
    let options = &trigger.event().options;

    for entity in existing_buttons.iter() {
        commands.entity(entity).despawn();
    }

    let Ok((list_entity, mut vis, fade)) = choice_list.single_mut() else { return };
    *vis = Visibility::Visible;

    // Reset fade to trigger a fade-in for the choices
    if let Some(mut fade) = fade {
        fade.opacity = 0.0;
        fade.target_opacity = 1.0;
        fade.despawn_on_fade_out = false;
        fade.fade_speed_override = None;
    }

    let option_count = options.len();

    // Spawn a wrapper that we'll animate margin-top on for scrolling
    commands.entity(list_entity).with_children(|parent| {
        for (i, option) in options.iter().enumerate() {
            let text = option.line.text_without_character_name();
            let is_selected = i == 0;
            let color = if is_selected { CHOICE_SELECTED } else { CHOICE_UNSELECTED };
            let font_size = if is_selected { CHOICE_FONT_SELECTED } else { CHOICE_FONT_UNSELECTED };
            let prefix = if is_selected { "\u{25B6} " } else { "  " };

            let shadow = if is_selected { glow_shadow() } else { dim_shadow() };
            let base_alpha = if is_selected { 1.0 } else { CHOICE_UNSELECTED.alpha() };
            parent.spawn((
                DialogueChoiceButton(i),
                Text::new(format!("{prefix}{text}")),
                TextFont { font: dialogue_font.regular.clone(), font_size, ..default() },
                TextColor(color),
                TextLayout::new(Justify::Center, LineBreak::NoWrap),
                shadow,
                BaseAlpha(base_alpha),
                Node {
                    min_height: Val::Px(ChoiceSelection::ITEM_HEIGHT),
                    ..default()
                },
            ));
        }
    });

    commands.insert_resource(ChoiceSelection::new(option_count));
}

// ── Dialogue Completed (Observer) ────────────────────────────────────────

/// Unified fade-out speed for all dialogue popups.
const FADE_OUT_SPEED: f32 = 8.0;

pub fn on_dialogue_completed(
    _trigger: On<DialogueCompleted>,
    mut fade_q: Query<&mut DialogueFade, With<DialogueBoxRoot>>,
    mut dialogue_state: Option<ResMut<DialogueState>>,
) {
    if let Some(ref mut state) = dialogue_state {
        state.fading_out = true;
    }

    // Unified fade-out on all dialogue entities
    for mut fade in fade_q.iter_mut() {
        fade.target_opacity = 0.0;
        fade.despawn_on_fade_out = true;
        fade.fade_speed_override = Some(FADE_OUT_SPEED);
    }

    info!("Dialogue fading out");
}

// ── Typewriter ───────────────────────────────────────────────────────────

pub fn update_typewriter(
    time: Res<Time>,
    mut query: Query<(&mut Text, &mut TypewriterState), With<DialogueBodyText>>,
    mut indicator_vis: Query<&mut Visibility, With<DialogueContinueIndicator>>,
    mut span_q: Query<&mut TextSpan>,
    dialogue_state: Option<Res<DialogueState>>,
    mut runners: Query<&mut DialogueRunner>,
    asset_server: Res<AssetServer>,
    mut commands: Commands,
) {
    let Ok((mut text, mut tw)) = query.single_mut() else { return };

    let has_auto = tw.auto_advance.is_some();

    // --- Tick timer and reveal characters ---
    if !tw.finished {
        tw.timer.tick(time.delta());

        let blip_path = tw.blip_sound.clone();
        let char_count = tw.full_text.chars().count();

        while tw.timer.just_finished() {
            tw.timer.reset();
            tw.revealed += 1;

            if tw.revealed >= char_count {
                tw.revealed = char_count;
                tw.finished = true;
                break;
            }

            if let Some(ref blip) = blip_path {
                let ch = tw.full_text.chars().nth(tw.revealed.saturating_sub(1));
                if ch.is_some_and(|c| !c.is_whitespace()) && tw.revealed % 2 == 0 {
                    let handle: Handle<AudioSource> = asset_server.load(blip.clone());
                    commands.spawn((
                        AudioPlayer::new(handle),
                        PlaybackSettings::DESPAWN,
                    ));
                }
            }
        }
    }

    // --- Reveal text across spans (always runs, including after instant-reveal) ---
    if tw.span_entities.is_empty() {
        // No spans (shouldn't happen) — fallback to plain text on root
        let visible: String = tw.full_text.chars().take(tw.revealed).collect();
        **text = visible;
    } else {
        // Clear root text — content is in spans
        **text = String::new();

        // Walk segments and reveal characters across span entities
        let mut chars_remaining = tw.revealed;
        for (i, seg) in tw.segments.iter().enumerate() {
            let seg_char_count = seg.text.chars().count();
            if i >= tw.span_entities.len() { break; }

            if seg.shake || seg.wave {
                // Effect span: text is always full (set at spawn time with alpha 0).
                // Visibility is controlled by EffectGlyph entities.
                chars_remaining = chars_remaining.saturating_sub(seg_char_count);
            } else {
                let entity = tw.span_entities[i];
                if let Ok(mut span) = span_q.get_mut(entity) {
                    let reveal_count = chars_remaining.min(seg_char_count);
                    let revealed: String = seg.text.chars().take(reveal_count).collect();
                    **span = revealed;
                    chars_remaining = chars_remaining.saturating_sub(seg_char_count);
                }
            }
        }
    }

    // Show/hide the continue indicator
    if let Ok(mut vis) = indicator_vis.single_mut() {
        *vis = if tw.finished && !has_auto {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }

    // --- Auto-advance (only when finished) ---
    if tw.finished {
        if let Some(delay) = tw.auto_advance {
            tw.auto_timer += time.delta_secs();
            if tw.auto_timer >= delay {
                tw.auto_advance = None;
                if let Some(ref state) = dialogue_state {
                    if state.fading_out { return; }
                    if let Ok(mut runner) = runners.get_mut(state.runner_entity) {
                        if !runner.is_waiting_for_option_selection() {
                            runner.continue_in_next_update();
                        }
                    }
                }
            }
        }
    }
}

// ── Input Handling ───────────────────────────────────────────────────────

pub fn handle_dialogue_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    dialogue_state: Option<Res<DialogueState>>,
    mut runners: Query<&mut DialogueRunner>,
    mut body_q: Query<(&mut Text, &mut TypewriterState), With<DialogueBodyText>>,
    // NOTE: fading_out check is below
    mut choice_sel: Option<ResMut<ChoiceSelection>>,
    mut choice_buttons: Query<(&DialogueChoiceButton, &mut Text, &mut TextFont, &mut TextColor, &mut TextShadow, &mut BaseAlpha), Without<DialogueBodyText>>,
) {
    let Some(state) = dialogue_state else { return };
    if state.fading_out { return; }

    let action = keyboard.just_pressed(KeyCode::Space)
        || keyboard.just_pressed(KeyCode::Enter)
        || keyboard.just_pressed(KeyCode::KeyZ);
    let up = keyboard.just_pressed(KeyCode::ArrowUp) || keyboard.just_pressed(KeyCode::KeyW);
    let down = keyboard.just_pressed(KeyCode::ArrowDown) || keyboard.just_pressed(KeyCode::KeyS);

    let Ok(mut runner) = runners.get_mut(state.runner_entity) else { return };

    if runner.is_waiting_for_option_selection() {
        if let Some(ref mut sel) = choice_sel {
            if up && sel.index > 0 { sel.index -= 1; }
            if down && sel.index + 1 < sel.count { sel.index += 1; }

            if up || down {
                sel.update_target();
                // Immediately update selected/unselected styling
                for (btn, mut text, mut font, mut color, mut shadow, mut base) in choice_buttons.iter_mut() {
                    let body = text.as_str()
                        .trim_start_matches("\u{25B6} ")
                        .trim_start_matches("  ")
                        .to_string();
                    if btn.0 == sel.index {
                        **text = format!("\u{25B6} {body}");
                        font.font_size = CHOICE_FONT_SELECTED;
                        *color = TextColor(CHOICE_SELECTED);
                        *shadow = glow_shadow();
                        base.0 = 1.0;
                    } else {
                        **text = format!("  {body}");
                        font.font_size = CHOICE_FONT_UNSELECTED;
                        *color = TextColor(CHOICE_UNSELECTED);
                        *shadow = dim_shadow();
                        base.0 = CHOICE_UNSELECTED.alpha();
                    }
                }
            }
            if action {
                sel.confirmed_index = Some(sel.index);
                let _ = runner.select_option(OptionId(sel.index));
            }
        }
        return;
    }

    if action {
        if let Ok((_text, mut tw)) = body_q.single_mut() {
            if !tw.finished {
                tw.revealed = tw.full_text.chars().count();
                tw.finished = true;
                // Don't set root text — update_typewriter handles span reveals
                return;
            }
        }
        runner.continue_in_next_update();
    }
}

// ── Fade & Smooth Position Animation ─────────────────────────────────────

/// System: animates opacity fade-in/out on all dialogue UI entities and
/// applies the opacity to descendant text colors.
pub fn animate_dialogue_fade(
    time: Res<Time>,
    mut fade_q: Query<(Entity, &mut DialogueFade), With<DialogueBoxRoot>>,
    children_q: Query<&Children>,
    mut text_q: Query<(&mut TextColor, &mut TextShadow, &BaseAlpha), Without<ImageNode>>,
    mut image_q: Query<(&mut ImageNode, &BaseAlpha), Without<TextColor>>,
) {
    let dt = time.delta_secs();

    for (root_entity, mut fade) in fade_q.iter_mut() {
        let speed = fade.fade_speed_override.unwrap_or(DialogueFade::FADE_SPEED);
        let diff = fade.target_opacity - fade.opacity;
        if diff.abs() < 0.01 {
            fade.opacity = fade.target_opacity;
        } else {
            fade.opacity += diff * (speed * dt).min(1.0);
        }

        let alpha = fade.opacity;

        // Apply opacity to all descendant text and image entities
        let mut stack = vec![root_entity];
        while let Some(ent) = stack.pop() {
            if let Ok((mut color, mut shadow, base)) = text_q.get_mut(ent) {
                color.0 = color.0.with_alpha(base.0 * alpha);
                shadow.color = shadow.color.with_alpha(alpha.min(base.0));
            }
            if let Ok((mut image, base)) = image_q.get_mut(ent) {
                image.color = image.color.with_alpha(base.0 * alpha);
            }
            if let Ok(children) = children_q.get(ent) {
                for child in children.iter() {
                    stack.push(child);
                }
            }
        }
    }
}


/// System: once a choice is confirmed, the chosen option expands (font scales
/// up to 200%) while the choice list fades out. Triggers immediately on
/// confirmation, not at end of dialogue.
pub fn animate_chosen_expansion(
    sel: Option<Res<ChoiceSelection>>,
    choice_fade_q: Query<&DialogueFade, (With<DialogueChoiceList>, With<DialogueBoxRoot>)>,
    mut choice_buttons: Query<(&DialogueChoiceButton, &mut TextFont)>,
) {
    let Some(sel) = sel else { return };
    let Some(confirmed) = sel.confirmed_index else { return };

    // Get the choice list's fade progress (1.0 = fully visible, 0.0 = fully faded)
    let fade_progress = choice_fade_q.iter().next()
        .map(|f| f.opacity)
        .unwrap_or(1.0);

    // Scale: 1.0 at full opacity → 2.0 at zero opacity
    let scale = 1.0 + (1.0 - fade_progress);

    for (btn, mut font) in choice_buttons.iter_mut() {
        if btn.0 == confirmed {
            font.font_size = CHOICE_FONT_SELECTED * scale;
        }
    }
}

// ── Per-Character Text Effects ───────────────────────────────────────────

/// System: after `TextLayoutInfo` is populated, spawns `ImageNode` entities
/// for each glyph in an effect span (shake/wave). The original text stays
/// invisible (alpha 0) and these overlay entities are animated per-character.
pub fn spawn_effect_glyphs(
    mut commands: Commands,
    mut body_q: Query<
        (Entity, &mut TypewriterState, &TextLayoutInfo, &ComputedNode),
        (With<DialogueBodyText>, Without<EffectGlyphsSpawned>),
    >,
    parent_q: Query<&ChildOf>,
    mut images: ResMut<Assets<Image>>,
    mut atlases: ResMut<Assets<TextureAtlasLayout>>,
) {
    let Ok((body_entity, mut tw, layout_info, computed)) = body_q.single_mut() else { return };
    // Parent the ImageNode entities to the body text's parent (dialogue box)
    // rather than the body text entity itself, because absolute-positioned
    // children of Text entities are positioned from the text's center, not
    // its top-left edge.
    let container_entity = parent_q.get(body_entity)
        .map(|c| c.parent())
        .unwrap_or(body_entity);

    // Check if any segments have effects
    let has_effects = tw.segments.iter().any(|s| s.shake || s.wave);
    if !has_effects {
        commands.entity(body_entity).insert(EffectGlyphsSpawned);
        return;
    }

    // Wait for layout to be computed (one-frame delay after text is set)
    if layout_info.glyphs.is_empty() { return; }

    // Diagnostic: log first few effect glyph X positions to diagnose spacing
    let node_phys_size = computed.size();
    {
        let sf = layout_info.scale_factor.max(1.0);
        let mut effect_positions = Vec::new();
        for g in &layout_info.glyphs {
            let si = g.span_index.saturating_sub(1);
            if si < tw.segments.len() && (tw.segments[si].shake || tw.segments[si].wave) {
                let left = (g.position.x - g.size.x / 2.0) / sf;
                effect_positions.push(format!(
                    "pos=({:.1},{:.1}) sz=({:.0},{:.0}) left={:.1}",
                    g.position.x, g.position.y, g.size.x, g.size.y, left
                ));
                if effect_positions.len() >= 6 { break; }
            }
        }
        if !effect_positions.is_empty() {
            warn!(
                "EffectGlyph diag: sf={}, node={:?}, text={:?}\n  glyphs: [{}]",
                layout_info.scale_factor, node_phys_size, layout_info.size,
                effect_positions.join(", ")
            );
        }
    }

    // Build cumulative character offsets per segment for global char indexing
    let segment_offsets: Vec<usize> = {
        let mut offsets = Vec::with_capacity(tw.segments.len());
        let mut off = 0;
        for seg in &tw.segments {
            offsets.push(off);
            off += seg.text.chars().count();
        }
        offsets
    };

    // Track how many glyphs we've seen per span for char indexing within span.
    // +1 because span_index 0 is the root Text entity.
    let mut span_glyph_counts = vec![0usize; tw.segments.len() + 1];

    for glyph in &layout_info.glyphs {
        let span_idx = glyph.span_index;
        let seg_idx = span_idx.saturating_sub(1); // root Text is span 0
        if seg_idx >= tw.segments.len() { continue; }

        let seg = &tw.segments[seg_idx];

        // Count ALL glyphs per span (not just effect ones) for correct indexing
        let char_within_span = span_glyph_counts[span_idx];
        span_glyph_counts[span_idx] += 1;

        if !seg.shake && !seg.wave { continue; }

        let global_char_idx = segment_offsets[seg_idx] + char_within_span;

        let Some(image_handle) = images.get_strong_handle(glyph.atlas_info.texture) else { continue };
        let Some(atlas_handle) = atlases.get_strong_handle(glyph.atlas_info.texture_atlas) else { continue };

        let color = seg.color.unwrap_or(TEXT_COLOR);

        // glyph.position is the CENTER of the glyph in physical pixels.
        // Convert to top-left in logical UI pixels for Node positioning.
        let sf = layout_info.scale_factor.max(1.0);
        let base_pos = Vec2::new(
            (glyph.position.x - glyph.size.x / 2.0) / sf,
            (glyph.position.y - glyph.size.y / 2.0) / sf,
        );
        let glyph_w = glyph.size.x / sf;
        let glyph_h = glyph.size.y / sf;

        let glyph_entity = commands.spawn((
            EffectGlyph {
                char_index: global_char_idx,
                flags: EffectFlags { shake: seg.shake, wave: seg.wave },
                base_position: base_pos,
            },
            ImageNode::from_atlas_image(
                image_handle,
                TextureAtlas {
                    layout: atlas_handle,
                    index: glyph.atlas_info.location.glyph_index,
                },
            ).with_color(color),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(base_pos.x),
                top: Val::Px(base_pos.y),
                width: Val::Px(glyph_w),
                height: Val::Px(glyph_h),
                ..default()
            },
            BaseAlpha(1.0),
            Visibility::Hidden,
        )).id();

        commands.entity(container_entity).add_child(glyph_entity);
        tw.effect_glyph_entities.push(glyph_entity);
    }

    commands.entity(body_entity).insert(EffectGlyphsSpawned);
}

/// System: updates effect glyph base positions each frame from `TextLayoutInfo`
/// so glyphs stay aligned as surrounding text reflows during typewriter.
pub fn update_effect_glyph_positions(
    body_q: Query<
        (&TypewriterState, &TextLayoutInfo),
        (With<DialogueBodyText>, With<EffectGlyphsSpawned>),
    >,
    mut glyph_q: Query<&mut EffectGlyph>,
) {
    let Ok((tw, layout_info)) = body_q.single() else { return };
    if tw.effect_glyph_entities.is_empty() { return; }

    // Walk layout glyphs, matching effect glyphs by spawn order
    let mut effect_idx = 0;

    for glyph in &layout_info.glyphs {
        let span_idx = glyph.span_index;
        let seg_idx = span_idx.saturating_sub(1);
        if seg_idx >= tw.segments.len() { continue; }

        let seg = &tw.segments[seg_idx];
        if !seg.shake && !seg.wave { continue; }

        if effect_idx >= tw.effect_glyph_entities.len() { break; }
        let entity = tw.effect_glyph_entities[effect_idx];
        effect_idx += 1;

        if let Ok(mut eg) = glyph_q.get_mut(entity) {
            let sf = layout_info.scale_factor.max(1.0);
            eg.base_position = Vec2::new(
                (glyph.position.x - glyph.size.x / 2.0) / sf,
                (glyph.position.y - glyph.size.y / 2.0) / sf,
            );
        }
    }
}

/// System: shows/hides effect glyph entities based on typewriter reveal count.
pub fn update_effect_glyph_visibility(
    body_q: Query<&TypewriterState, With<DialogueBodyText>>,
    mut glyph_q: Query<(&EffectGlyph, &mut Visibility)>,
) {
    let Ok(tw) = body_q.single() else { return };

    for (glyph, mut vis) in glyph_q.iter_mut() {
        *vis = if glyph.char_index < tw.revealed {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

/// System: per-character animation for shake and wave effects.
/// Applies offsets via `Node.left`/`Node.top` (not `Transform`, which the UI
/// layout system overwrites each frame for absolute-positioned nodes).
pub fn animate_effect_glyphs(
    time: Res<Time>,
    mut glyph_q: Query<(&EffectGlyph, &mut Node)>,
) {
    let t = time.elapsed_secs();

    for (glyph, mut node) in glyph_q.iter_mut() {
        let ci = glyph.char_index as f32;
        let mut dx = 0.0_f32;
        let mut dy = 0.0_f32;

        if glyph.flags.shake {
            // Per-character pseudo-random jitter using different frequency offsets
            dx += ((t * 25.0 + ci * 7.0).sin() * 1.5).round();
            dy += ((t * 30.0 + ci * 11.0).cos() * 1.5).round();
        }

        if glyph.flags.wave {
            // Sine wave with per-character phase offset
            dy += (t * 3.0 + ci * 0.5).sin() * 3.0;
        }

        node.left = Val::Px(glyph.base_position.x + dx);
        node.top = Val::Px(glyph.base_position.y + dy);
    }
}

// ── Choice Scroll Animation ──────────────────────────────────────────────

/// System: smoothly scrolls the choice list so the selected option stays
/// at a fixed selector position, with one item visible above when possible.
/// Uses margin-top on the first child to shift the entire flex column.
pub fn animate_choice_selection(
    time: Res<Time>,
    mut sel: ResMut<ChoiceSelection>,
    choice_list_q: Query<&Children, With<DialogueChoiceList>>,
    mut choice_nodes: Query<&mut Node, With<DialogueChoiceButton>>,
) {
    let dt = time.delta_secs();
    let diff = sel.target_offset - sel.scroll_offset;
    if diff.abs() < 0.3 {
        sel.scroll_offset = sel.target_offset;
    } else {
        sel.scroll_offset += diff * (ChoiceSelection::LERP_SPEED * dt).min(1.0);
    }

    // Apply scroll offset as margin-top on the first choice button only.
    // The rest of the items follow naturally in the flex column.
    let Ok(children) = choice_list_q.single() else { return };
    for (i, child) in children.iter().enumerate() {
        if let Ok(mut node) = choice_nodes.get_mut(child) {
            if i == 0 {
                node.margin.top = Val::Px(sel.scroll_offset);
            } else {
                node.margin.top = Val::Px(0.0);
            }
        }
    }
}

// ── Entity Screen Rect Helpers ───────────────────────────────────────────

use crate::camera::combat::BillboardHeight;

/// A screen-space rectangle (in viewport pixels, top-left origin).
#[derive(Clone, Copy)]
struct ScreenRect {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl ScreenRect {
    fn overlaps(&self, other: &ScreenRect) -> bool {
        self.left < other.right && self.right > other.left
            && self.top < other.bottom && self.bottom > other.top
    }

    fn center_x(&self) -> f32 { (self.left + self.right) * 0.5 }

    /// Expand this rect to also encompass `other`, returning the union.
    fn union(&self, other: &ScreenRect) -> ScreenRect {
        ScreenRect {
            left: self.left.min(other.left),
            top: self.top.min(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }
}

/// Build a single combined exclusion zone from the player's sprite rect to the
/// speaker's sprite rect (covering both entities and the space between them).
fn build_exclusion_zone(
    player_ent: Entity,
    speaker_ent: Option<Entity>,
    transform_q: &Query<&GlobalTransform>,
    billboard_q: &Query<&BillboardHeight>,
    camera: &Camera,
    cam_tf: &GlobalTransform,
) -> Option<ScreenRect> {
    let player_rect = entity_screen_rect(player_ent, transform_q, billboard_q, camera, cam_tf)?;

    if let Some(speaker) = speaker_ent {
        if let Some(speaker_rect) = entity_screen_rect(speaker, transform_q, billboard_q, camera, cam_tf) {
            // Union of both rects = one big exclusion zone from one entity's
            // furthest edge to the other's furthest edge
            return Some(player_rect.union(&speaker_rect));
        }
    }
    Some(player_rect)
}

/// Resolve a placed-object entity by name or `#id`.
fn resolve_placed_object(
    name: &str,
    placed_q: &Query<(Entity, &crate::tile_editor::state::PlacedObject)>,
) -> Option<Entity> {
    if let Some(id) = name.strip_prefix('#') {
        placed_q.iter().find(|(_, po)| po.sidecar_id == id).map(|(e, _)| e)
    } else {
        placed_q.iter().find(|(_, po)| po.name.as_deref() == Some(name)).map(|(e, _)| e)
    }
}

/// Approximate sprite half-width in world units (billboard quads are typically
/// as wide as the sprite).
const SPRITE_HALF_WIDTH_WORLD: f32 = 20.0;

/// Compute the screen-space bounding rect of an entity using its transform
/// and BillboardHeight. Falls back to a fixed offset if no BillboardHeight.
fn entity_screen_rect(
    entity: Entity,
    transform_q: &Query<&GlobalTransform>,
    billboard_q: &Query<&BillboardHeight>,
    camera: &Camera,
    cam_tf: &GlobalTransform,
) -> Option<ScreenRect> {
    let tf = transform_q.get(entity).ok()?;
    let world = tf.translation();

    // Get sprite height from BillboardHeight or use default
    let sprite_h = billboard_q.get(entity)
        .map(|bh| bh.height)
        .unwrap_or(BUBBLE_Z_OFFSET);

    // Project base (feet) and top of sprite to screen
    let base_screen = camera.world_to_viewport(cam_tf, world).ok()?;
    let top_world = world + Vec3::new(0.0, 0.0, sprite_h);
    let top_screen = camera.world_to_viewport(cam_tf, top_world).ok()?;
    // Project left and right edges
    let left_world = world + Vec3::new(-SPRITE_HALF_WIDTH_WORLD, 0.0, sprite_h * 0.5);
    let right_world = world + Vec3::new(SPRITE_HALF_WIDTH_WORLD, 0.0, sprite_h * 0.5);
    let left_screen = camera.world_to_viewport(cam_tf, left_world).ok()?;
    let right_screen = camera.world_to_viewport(cam_tf, right_world).ok()?;

    Some(ScreenRect {
        left: left_screen.x,
        top: top_screen.y,   // top of sprite (lower Y = higher on screen)
        right: right_screen.x,
        bottom: base_screen.y,
    })
}

/// Place a UI rect near an anchor entity, preferring positions that avoid
/// the exclusion zone but falling back to overlapping if necessary.
///
/// Priority (highest = most important):
///   3. Stay on screen
///   2. Stay near the anchor entity
///   1. Avoid the exclusion zone
///
/// So we always anchor to the entity, and only shift to avoid the zone
/// if we can do so without going off-screen. If we can't avoid the zone
/// while staying near the anchor, we infringe on the zone.
fn find_best_position(
    anchor_x: f32,      // screen-space center X of the anchor entity
    anchor_top: f32,     // screen-space top edge of the anchor sprite
    anchor_bottom: f32,  // screen-space bottom edge of the anchor sprite
    ui_w: f32,
    ui_h: f32,
    exclusion: Option<&ScreenRect>,
    win_w: f32,
    win_h: f32,
) -> (f32, f32) {
    let margin = BUBBLE_SCREEN_MARGIN;
    let gap = BUBBLE_MIN_GAP_PX;
    let half_w = ui_w / 2.0;
    let clamp_x = |x: f32| x.max(margin).min(win_w - ui_w - margin);
    let clamp_y = |y: f32| y.max(margin).min(win_h - ui_h - margin);

    let left = clamp_x(anchor_x - half_w);

    // Preferred: above the anchor entity's sprite
    let above_y = clamp_y(anchor_top - gap - ui_h);
    // Alternative: below the anchor entity's sprite
    let below_y = clamp_y(anchor_bottom + gap);

    if let Some(zone) = exclusion {
        let above_rect = ScreenRect { left, top: above_y, right: left + ui_w, bottom: above_y + ui_h };
        let below_rect = ScreenRect { left, top: below_y, right: left + ui_w, bottom: below_y + ui_h };

        // Try above first (preferred), then below
        if !above_rect.overlaps(zone) {
            return (left, above_y);
        }
        if !below_rect.overlaps(zone) {
            return (left, below_y);
        }
    }

    // Fallback: place above the anchor even if it infringes on the zone
    (left, above_y)
}

// ── Speech Bubble Positioning ────────────────────────────────────────────

/// System: repositions speech bubble entities (main bubble + name tab) near
/// the speaker entity, avoiding the player-to-speaker exclusion zone when possible.
pub fn update_speech_bubble_position(
    mut main_q: Query<
        (&SpeechBubbleAnchor, &mut Node, &ComputedNode, &mut DialogueFade),
        (With<DialogueBoxRoot>, Without<DialogueNameTab>, Without<DialogueChoiceList>, Without<DialogueContinueIndicator>),
    >,
    mut tab_q: Query<
        (&SpeechBubbleAnchor, &mut Node, &ComputedNode, &mut DialogueFade),
        (With<DialogueNameTab>, Without<DialogueChoiceList>),
    >,
    placed_q: Query<(Entity, &crate::tile_editor::state::PlacedObject)>,
    player_q: Query<Entity, With<crate::camera::follow::CameraTarget>>,
    transform_q: Query<&GlobalTransform>,
    billboard_q: Query<&BillboardHeight>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::camera::CombatCamera3d>>,
    windows: Query<&bevy::window::Window, With<bevy::window::PrimaryWindow>>,
) {
    let Ok((camera, cam_tf)) = cameras.single() else { return };
    let Ok(window) = windows.single() else { return };
    let win_w = window.resolution.width();
    let win_h = window.resolution.height();

    let player_ent = player_q.single().ok();

    for (anchor, mut node, computed, mut fade) in main_q.iter_mut() {
        let Some(speaker_ent) = resolve_placed_object(&anchor.instance_name, &placed_q) else { continue };
        let Some(speaker_rect) = entity_screen_rect(speaker_ent, &transform_q, &billboard_q, camera, cam_tf) else { continue };

        // Skip positioning until layout has computed a valid size.
        // The entity is invisible during fade-in so this is imperceptible.
        let ui_w = computed.size().x;
        let ui_h = computed.size().y;
        if ui_w < 1.0 || ui_h < 1.0 { continue; }

        let exclusion = build_exclusion_zone(
            player_ent.unwrap_or(speaker_ent),
            Some(speaker_ent),
            &transform_q, &billboard_q, camera, cam_tf,
        );

        let (left, top) = find_best_position(
            speaker_rect.center_x(), speaker_rect.top, speaker_rect.bottom,
            ui_w, ui_h, exclusion.as_ref(),
            win_w, win_h,
        );

        // Snap directly — no lerp. The fade-in provides visual softness.
        // Lerping causes the bubble to lag behind camera movement.
        node.left = Val::Px(left);
        node.top = Val::Px(top);
        fade.current_pos = Some((left, top));
    }

    // Name tab: centered on the speaker entity's screen X, stacked above the main bubble.
    for (anchor, mut node, computed, mut fade) in tab_q.iter_mut() {
        // Get the main bubble's top Y position
        let main_top = main_q.iter().find_map(|(a, _, _, f)| {
            if a.instance_name == anchor.instance_name {
                f.current_pos.map(|(_, top)| top)
            } else { None }
        });

        // Get the speaker's screen center X for stable horizontal centering
        let speaker_center_x = resolve_placed_object(&anchor.instance_name, &placed_q)
            .and_then(|ent| entity_screen_rect(ent, &transform_q, &billboard_q, camera, cam_tf))
            .map(|r| r.center_x());

        if let (Some(bubble_top), Some(center_x)) = (main_top, speaker_center_x) {
            let tab_h = computed.size().y;
            let tab_w = computed.size().x;
            let left = center_x - tab_w * 0.5;
            let top = bubble_top - tab_h - NAME_TAB_GAP;
            node.left = Val::Px(left);
            node.top = Val::Px(top);
            fade.current_pos = Some((left, top));
        }
    }

}

/// System: positions the choice list near the player, avoiding the combined
/// player-to-speaker bounding box when possible. Caches position to prevent jitter.
pub fn update_choice_position(
    mut choice_q: Query<(&ChoiceAtPlayer, &mut Node, &ComputedNode), With<DialogueChoiceList>>,
    mut sel: Option<ResMut<ChoiceSelection>>,
    player_q: Query<Entity, With<crate::camera::follow::CameraTarget>>,
    placed_q: Query<(Entity, &crate::tile_editor::state::PlacedObject)>,
    transform_q: Query<&GlobalTransform>,
    billboard_q: Query<&BillboardHeight>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::camera::CombatCamera3d>>,
    windows: Query<&bevy::window::Window, With<bevy::window::PrimaryWindow>>,
) {
    // Use cached position if available
    if let Some(ref sel) = sel {
        if let Some((cached_left, cached_top)) = sel.cached_pos {
            for (_, mut node, _) in choice_q.iter_mut() {
                node.left = Val::Px(cached_left);
                node.top = Val::Px(cached_top);
            }
            return;
        }
    }

    let Ok((camera, cam_tf)) = cameras.single() else { return };
    let Ok(window) = windows.single() else { return };
    let win_w = window.resolution.width();
    let win_h = window.resolution.height();

    let Ok(player_ent) = player_q.single() else { return };
    let Some(player_rect) = entity_screen_rect(player_ent, &transform_q, &billboard_q, camera, cam_tf) else { return };

    for (choice_at, mut node, computed) in choice_q.iter_mut() {
        let ui_w = computed.size().x.max(150.0);
        let ui_h = computed.size().y;
        if ui_h < 1.0 { continue; }

        let speaker_ent = choice_at.speaker_instance.as_ref()
            .and_then(|name| resolve_placed_object(name, &placed_q));

        // Build combined exclusion zone
        let exclusion = build_exclusion_zone(
            player_ent, speaker_ent,
            &transform_q, &billboard_q, camera, cam_tf,
        );

        // Anchor to the PLAYER, avoid zone if possible
        let (left, top) = find_best_position(
            player_rect.center_x(), player_rect.top, player_rect.bottom,
            ui_w, ui_h, exclusion.as_ref(),
            win_w, win_h,
        );

        node.left = Val::Px(left);
        node.top = Val::Px(top);

        if let Some(ref mut sel) = sel {
            sel.cached_pos = Some((left, top));
        }
    }
}
