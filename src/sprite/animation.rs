use bevy::prelude::*;

/// Controls sprite animation playback.
#[derive(Component)]
pub struct AnimationController {
    pub current_animation: String,
    pub direction: u8,
    pub frame: u32,
    pub timer: Timer,
    pub looping: bool,
}

impl Default for AnimationController {
    fn default() -> Self {
        Self {
            current_animation: "walk".into(),
            direction: 2, // facing down
            frame: 0,
            timer: Timer::from_seconds(0.1, TimerMode::Repeating),
            looping: true,
        }
    }
}
