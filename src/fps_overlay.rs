//! Standalone FPS overlay — no game-logic dependencies.
//!
//! Tracks frame times with `std::time::Instant` so measurements are
//! unaffected by Bevy's virtual `Time` (pause / time-scale).
//!
//! Displays: current FPS, 1-second average, 1% low, 0.1% low.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use bevy::prelude::*;

/// How many frame samples to keep for percentile calculations.
const MAX_SAMPLES: usize = 2000;

/// How often (in frames) to recompute percentile lows.
const PERCENTILE_INTERVAL: u32 = 30;

pub struct FpsOverlayPlugin;

impl Plugin for FpsOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FrameTimeHistory>()
            .add_systems(Startup, spawn_fps_display)
            .add_systems(Last, (record_frame_time, update_fps_display).chain());
    }
}

/// Ring buffer of recent frame durations, stamped so we can window by wall-time.
#[derive(Resource)]
struct FrameTimeHistory {
    samples: VecDeque<(Instant, Duration)>,
    last_instant: Option<Instant>,
    /// Cached percentile values, recomputed every PERCENTILE_INTERVAL frames.
    cached_low_1: f64,
    cached_low_01: f64,
    frames_since_percentile: u32,
}

impl Default for FrameTimeHistory {
    fn default() -> Self {
        Self {
            samples: VecDeque::with_capacity(MAX_SAMPLES),
            last_instant: None,
            cached_low_1: 0.0,
            cached_low_01: 0.0,
            frames_since_percentile: PERCENTILE_INTERVAL,
        }
    }
}

impl FrameTimeHistory {
    fn push(&mut self, now: Instant) {
        if let Some(prev) = self.last_instant {
            let dt = now.duration_since(prev);
            if self.samples.len() >= MAX_SAMPLES {
                self.samples.pop_front();
            }
            self.samples.push_back((now, dt));
        }
        self.last_instant = Some(now);
        self.frames_since_percentile += 1;
    }

    /// Current (instantaneous) FPS from last frame.
    fn current_fps(&self) -> Option<f64> {
        self.samples.back().map(|(_, dt)| 1.0 / dt.as_secs_f64())
    }

    /// Average FPS over the last `window` of wall-clock time.
    fn average_fps(&self, window: Duration) -> Option<f64> {
        let cutoff = Instant::now() - window;
        let mut sum = 0.0;
        let mut count = 0u32;
        for &(stamp, dt) in self.samples.iter().rev() {
            if stamp < cutoff {
                break;
            }
            sum += dt.as_secs_f64();
            count += 1;
        }
        if count > 0 {
            Some(count as f64 / sum)
        } else {
            None
        }
    }

    /// Recompute percentile lows if enough frames have elapsed.
    fn maybe_update_percentiles(&mut self) {
        if self.frames_since_percentile < PERCENTILE_INTERVAL {
            return;
        }
        self.frames_since_percentile = 0;

        let n = self.samples.len();
        if n < 10 {
            return;
        }

        // Single allocation + sort, reused for both percentiles.
        let mut dts: Vec<f64> = self.samples.iter().map(|(_, dt)| dt.as_secs_f64()).collect();
        dts.sort_unstable_by(|a, b| b.partial_cmp(a).unwrap());

        let take_1 = (n as f64 * 0.01).ceil().max(1.0) as usize;
        let avg_1 = dts[..take_1].iter().sum::<f64>() / take_1 as f64;
        self.cached_low_1 = 1.0 / avg_1;

        let take_01 = (n as f64 * 0.001).ceil().max(1.0) as usize;
        let avg_01 = dts[..take_01].iter().sum::<f64>() / take_01 as f64;
        self.cached_low_01 = 1.0 / avg_01;
    }
}

#[derive(Component)]
struct FpsText;

fn spawn_fps_display(mut commands: Commands) {
    commands.spawn((
        Text::new("FPS: --"),
        TextFont {
            font_size: 16.0,
            ..default()
        },
        TextColor(Color::srgb(1.0, 1.0, 0.0)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(5.0),
            left: Val::Px(5.0),
            ..default()
        },
        FpsText,
    ));
}

fn record_frame_time(mut history: ResMut<FrameTimeHistory>) {
    history.push(Instant::now());
}

fn update_fps_display(
    mut history: ResMut<FrameTimeHistory>,
    mut query: Query<&mut Text, With<FpsText>>,
) {
    history.maybe_update_percentiles();

    let current = history.current_fps().unwrap_or(0.0);
    let avg = history.average_fps(Duration::from_secs(1)).unwrap_or(0.0);
    let low_1 = history.cached_low_1;
    let low_01 = history.cached_low_01;

    for mut text in &mut query {
        **text = format!(
            "FPS: {current:.0}  avg: {avg:.0}  1%: {low_1:.0}  0.1%: {low_01:.0}"
        );
    }
}
