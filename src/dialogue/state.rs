//! Dialogue state tracking for the Yarn Spinner integration.

use bevy::prelude::*;

/// Resource holding the loaded dialogue font handles.
#[derive(Resource)]
pub struct DialogueFont {
    pub regular: Handle<Font>,
    pub bold: Handle<Font>,
}

pub const DIALOGUE_FONT_REGULAR: &str = "fonts/NotoSansJP/NotoSansJP-Medium.ttf";
pub const DIALOGUE_FONT_BOLD: &str = "fonts/NotoSansJP/NotoSansJP-ExtraBold.ttf";

/// Resource tracking the active dialogue session.
#[derive(Resource)]
pub struct DialogueState {
    /// Entity holding the [`bevy_yarnspinner::prelude::DialogueRunner`].
    pub runner_entity: Entity,
    /// If true, player movement and NPC AI are paused while dialogue is active.
    pub blocking: bool,
    /// Mapping of yarn character name → placed object instance name.
    /// Used to position speech bubbles near the currently speaking character.
    pub speaker_map: Vec<(String, String)>,
    /// Node name to start — consumed by `start_deferred_yarn_node` after the UI spawns.
    pub pending_start_node: Option<String>,
    /// True while dialogue is fading out — blocks input and typewriter.
    pub fading_out: bool,
}

// ── UI marker components ─────────────────────────────────────────────────

/// Root entity for the entire dialogue UI (box or bubble). Despawned on cleanup.
#[derive(Component)]
pub struct DialogueBoxRoot;

/// Tracks fade animation and smooth position for a dialogue UI entity.
#[derive(Component)]
pub struct DialogueFade {
    /// Current opacity (0.0 = invisible, 1.0 = fully visible).
    pub opacity: f32,
    /// Target opacity to lerp toward.
    pub target_opacity: f32,
    /// If true, despawn this entity once opacity reaches 0.
    pub despawn_on_fade_out: bool,
    /// Per-entity fade speed (overrides the default when set).
    pub fade_speed_override: Option<f32>,
    /// Current smooth screen position (left, top). Lerps toward target.
    pub current_pos: Option<(f32, f32)>,
    /// Smoothed computed height for speech bubble positioning. Prevents abrupt
    /// vertical jumps when text content changes between dialogue lines.
    pub smoothed_height: Option<f32>,
}

impl DialogueFade {
    /// Speed for opacity fade animation.
    pub const FADE_SPEED: f32 = 6.0;

    pub fn fade_in() -> Self {
        Self {
            opacity: 0.0,
            target_opacity: 1.0,
            despawn_on_fade_out: false,
            fade_speed_override: None,
            current_pos: None,
            smoothed_height: None,
        }
    }
}

/// The speaker name tab (floating above the main box).
#[derive(Component)]
pub struct DialogueNameTab;

/// The speaker name text inside the tab.
#[derive(Component)]
pub struct DialogueSpeakerName;

/// The main text body (dialogue content with typewriter).
#[derive(Component)]
pub struct DialogueBodyText;

/// The "continue" indicator (▾) shown centered below dialogue text.
#[derive(Component)]
pub struct DialogueContinueIndicator;

/// Container for choice buttons.
#[derive(Component)]
pub struct DialogueChoiceList;

/// A single choice button. Stores the option index.
#[derive(Component)]
pub struct DialogueChoiceButton(pub usize);

/// Tracks the target font size for smooth choice selection transitions.
#[derive(Component)]
pub struct ChoiceButtonStyle {
    pub target_font_size: f32,
    pub target_color: Color,
}

/// Portrait image (reserved for future use).
#[derive(Component)]
pub struct DialoguePortrait;

/// Stores the base alpha for a text entity so the fade system can multiply
/// against it without compounding.
#[derive(Component)]
pub struct BaseAlpha(pub f32);

/// Marks a speech-bubble that tracks a world entity.
#[derive(Component)]
pub struct SpeechBubbleAnchor {
    pub instance_name: String,
}

/// Marks the choice list as needing to be positioned near the player.
/// Stores the speaker instance name so we can avoid overlapping the speaker too.
#[derive(Component)]
pub struct ChoiceAtPlayer {
    pub speaker_instance: Option<String>,
}

// ── Typewriter state ─────────────────────────────────────────────────────

/// Characters revealed per second during typewriter animation.
pub const TYPEWRITER_CPS: f32 = 30.0;

// ── Text effect types ────────────────────────────────────────────────────

/// A segment of styled text parsed from YarnSpinner markup attributes.
#[derive(Clone, Debug)]
pub struct StyledSegment {
    pub text: String,
    pub color: Option<Color>,
    pub bold: bool,
    pub shake: bool,
    pub wave: bool,
}

/// Per-character effect flags for text animation.
#[derive(Clone, Copy, Default, Debug)]
pub struct EffectFlags {
    pub shake: bool,
    pub wave: bool,
}

/// A glyph entity spawned for per-character text effect animation.
/// Each entity is an `ImageNode` using the font atlas, positioned over the
/// original (invisible) text and animated independently.
#[derive(Component)]
pub struct EffectGlyph {
    /// Global character index in the full dialogue text.
    pub char_index: usize,
    /// Which effects apply to this glyph.
    pub flags: EffectFlags,
    /// Base position from text layout (relative to body text entity).
    /// Updated each frame from `TextLayoutInfo` so glyphs stay aligned
    /// as surrounding text reflows during typewriter animation.
    pub base_position: Vec2,
}

/// Marker: effect glyph entities have been spawned for the current line.
#[derive(Component)]
pub struct EffectGlyphsSpawned;

/// Maps a color name string from yarn `[color=X]` to a Bevy Color.
pub fn color_from_name(name: &str) -> Option<Color> {
    match name.to_lowercase().as_str() {
        "red" => Some(Color::srgba(1.0, 0.3, 0.3, 1.0)),
        "green" => Some(Color::srgba(0.3, 1.0, 0.3, 1.0)),
        "blue" => Some(Color::srgba(0.4, 0.6, 1.0, 1.0)),
        "yellow" => Some(Color::srgba(1.0, 1.0, 0.3, 1.0)),
        "cyan" => Some(Color::srgba(0.3, 1.0, 1.0, 1.0)),
        "magenta" | "pink" => Some(Color::srgba(1.0, 0.4, 0.8, 1.0)),
        "orange" => Some(Color::srgba(1.0, 0.6, 0.2, 1.0)),
        "purple" => Some(Color::srgba(0.7, 0.3, 1.0, 1.0)),
        "white" => Some(Color::srgba(1.0, 1.0, 1.0, 1.0)),
        "gray" | "grey" => Some(Color::srgba(0.6, 0.6, 0.6, 1.0)),
        _ => None,
    }
}

// ── Typewriter ───────────────────────────────────────────────────────────

/// Tracks typewriter text animation.
#[derive(Component)]
pub struct TypewriterState {
    /// The full text to display.
    pub full_text: String,
    /// Characters revealed so far.
    pub revealed: usize,
    /// Timer driving character reveal.
    pub timer: Timer,
    /// True once all characters are shown.
    pub finished: bool,
    /// If set, auto-advance after this many seconds once text is fully revealed.
    pub auto_advance: Option<f32>,
    /// Countdown for auto-advance (starts when `finished` becomes true).
    pub auto_timer: f32,
    /// Sound asset path for this speaker's voice blip (None = silent).
    pub blip_sound: Option<String>,
    /// Styled segments parsed from markup attributes.
    pub segments: Vec<StyledSegment>,
    /// Entity IDs of spawned TextSpan children (for cleanup).
    pub span_entities: Vec<Entity>,
    /// Entity IDs of spawned EffectGlyph ImageNode children (for cleanup).
    pub effect_glyph_entities: Vec<Entity>,
}

impl TypewriterState {
    pub fn new(text: String) -> Self {
        Self {
            full_text: text.clone(),
            segments: vec![StyledSegment {
                text,
                color: None,
                bold: false,
                shake: false,
                wave: false,
            }],
            span_entities: Vec::new(),
            effect_glyph_entities: Vec::new(),
            revealed: 0,
            timer: Timer::from_seconds(1.0 / TYPEWRITER_CPS, TimerMode::Repeating),
            finished: false,
            auto_advance: None,
            auto_timer: 0.0,
            blip_sound: None,
        }
    }

    pub fn new_styled(segments: Vec<StyledSegment>) -> Self {
        let full_text: String = segments.iter().map(|s| s.text.as_str()).collect();
        Self {
            full_text,
            segments,
            span_entities: Vec::new(),
            effect_glyph_entities: Vec::new(),
            revealed: 0,
            timer: Timer::from_seconds(1.0 / TYPEWRITER_CPS, TimerMode::Repeating),
            finished: false,
            auto_advance: None,
            auto_timer: 0.0,
            blip_sound: None,
        }
    }
}

/// Tracks which choice is currently highlighted and the scroll animation.
#[derive(Resource)]
pub struct ChoiceSelection {
    pub index: usize,
    pub count: usize,
    /// Current animated scroll offset (pixels). Lerps toward target.
    pub scroll_offset: f32,
    /// Target scroll offset based on selected index.
    pub target_offset: f32,
    /// Cached left/top position to prevent per-frame jitter.
    pub cached_pos: Option<(f32, f32)>,
    /// Set when a choice is confirmed — the chosen option expands during fade-out.
    pub confirmed_index: Option<usize>,
}

impl ChoiceSelection {
    /// Height per choice item in pixels.
    pub const ITEM_HEIGHT: f32 = 24.0;
    /// Animation lerp speed.
    pub const LERP_SPEED: f32 = 12.0;

    pub fn new(count: usize) -> Self {
        Self {
            index: 0,
            count,
            scroll_offset: 0.0,
            target_offset: 0.0,
            cached_pos: None,
            confirmed_index: None,
        }
    }

    /// Scroll so the selected item is at the top of the visible area.
    /// Items above the selection remain visible (no clipping) providing
    /// context for the previous option.
    pub fn update_target(&mut self) {
        self.target_offset = -(self.index as f32 * Self::ITEM_HEIGHT);
    }
}
