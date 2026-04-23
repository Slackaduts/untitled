//! Overworld movement system for placed objects and NPCs.
//!
//! Supports three movement modes:
//! - **Linear**: move toward target at constant speed
//! - **Eased**: move toward target with an easing function (time-based)
//! - **Bezier**: follow a cubic bezier curve in 3D space (time-based)

use bevy::prelude::*;
use bevy::math::cubic_splines::CubicCurve;
use bevy::math::curve::Curve;

use crate::sprite::animation::AnimationController;

/// Drives smooth movement toward a world-space target.
#[derive(Component)]
pub struct OverworldMovement {
    /// Target position in world space. `None` = idle.
    pub target: Option<Vec2>,
    /// Movement speed in pixels per second (for linear mode).
    pub speed: f32,
    /// Set to true when the entity arrives at the target.
    pub arrived: bool,
    /// Optional easing: duration-based movement with an easing curve.
    pub easing: Option<EasingState>,
    /// Optional bezier curve path (3D, time-based).
    pub bezier: Option<BezierState>,
}

impl Default for OverworldMovement {
    fn default() -> Self {
        Self {
            target: None,
            speed: 100.0,
            arrived: false,
            easing: None,
            bezier: None,
        }
    }
}

/// Easing state for time-based movement between start and target.
pub struct EasingState {
    pub start: Vec2,
    pub elapsed: f32,
    pub duration: f32,
    pub ease_fn: EaseFunction,
}

/// Bezier curve state for 3D path movement.
pub struct BezierState {
    pub curve: CubicCurve<Vec3>,
    pub elapsed: f32,
    pub duration: f32,
    pub start_pos: Vec3,
}

/// Direction constants matching LPC convention.
pub const DIR_UP: u8 = 0;
pub const DIR_LEFT: u8 = 1;
pub const DIR_DOWN: u8 = 2;
pub const DIR_RIGHT: u8 = 3;

/// Compute the LPC direction from a movement delta vector.
pub fn direction_from_delta(delta: Vec2) -> u8 {
    if delta.x.abs() > delta.y.abs() {
        if delta.x > 0.0 { DIR_RIGHT } else { DIR_LEFT }
    } else {
        if delta.y > 0.0 { DIR_UP } else { DIR_DOWN }
    }
}

/// Parse an easing function name to a Bevy `EaseFunction`.
pub fn parse_ease_function(name: &str) -> EaseFunction {
    match name {
        "QuadraticIn" => EaseFunction::QuadraticIn,
        "QuadraticOut" => EaseFunction::QuadraticOut,
        "QuadraticInOut" => EaseFunction::QuadraticInOut,
        "CubicIn" => EaseFunction::CubicIn,
        "CubicOut" => EaseFunction::CubicOut,
        "CubicInOut" => EaseFunction::CubicInOut,
        "QuarticIn" => EaseFunction::QuarticIn,
        "QuarticOut" => EaseFunction::QuarticOut,
        "QuarticInOut" => EaseFunction::QuarticInOut,
        "QuinticIn" => EaseFunction::QuinticIn,
        "QuinticOut" => EaseFunction::QuinticOut,
        "QuinticInOut" => EaseFunction::QuinticInOut,
        "SineIn" => EaseFunction::SineIn,
        "SineOut" => EaseFunction::SineOut,
        "SineInOut" => EaseFunction::SineInOut,
        "ExponentialIn" => EaseFunction::ExponentialIn,
        "ExponentialOut" => EaseFunction::ExponentialOut,
        "ExponentialInOut" => EaseFunction::ExponentialInOut,
        "CircularIn" => EaseFunction::CircularIn,
        "CircularOut" => EaseFunction::CircularOut,
        "CircularInOut" => EaseFunction::CircularInOut,
        "ElasticIn" => EaseFunction::ElasticIn,
        "ElasticOut" => EaseFunction::ElasticOut,
        "ElasticInOut" => EaseFunction::ElasticInOut,
        "BackIn" => EaseFunction::BackIn,
        "BackOut" => EaseFunction::BackOut,
        "BackInOut" => EaseFunction::BackInOut,
        "BounceIn" => EaseFunction::BounceIn,
        "BounceOut" => EaseFunction::BounceOut,
        "BounceInOut" => EaseFunction::BounceInOut,
        _ => EaseFunction::Linear,
    }
}

/// Outcome of a single movement tick.
enum MoveResult {
    /// Still moving. (delta_x, delta_y) for animation direction.
    Moving(Vec2),
    /// Arrived at destination.
    Arrived,
    /// No movement target — idle.
    Idle,
}

/// System: moves entities with [`OverworldMovement`] toward their target.
/// Handles linear, eased, and bezier movement modes.
pub fn overworld_movement_system(
    time: Res<Time>,
    mut query: Query<(&mut OverworldMovement, &mut Transform, Option<&mut AnimationController>)>,
) {
    let dt = time.delta_secs();

    for (mut movement, mut tf, mut anim) in &mut query {
        let prev_pos = tf.translation;

        let result = tick_movement(&mut movement, &mut tf, dt);

        match result {
            MoveResult::Moving(delta) => {
                if let Some(ref mut anim) = anim {
                    if delta.length_squared() > 0.01 {
                        let new_dir = direction_from_delta(delta);
                        if anim.direction != new_dir { anim.direction = new_dir; }
                    }
                    if !anim.playing || anim.current_animation != "walk" {
                        anim.current_animation = "walk".into();
                        anim.playing = true;
                        anim.looping = true;
                    }
                }
            }
            MoveResult::Arrived | MoveResult::Idle => {
                if let Some(ref mut anim) = anim {
                    if anim.playing && anim.current_animation == "walk" {
                        anim.playing = false;
                        anim.frame = 0;
                    }
                }
            }
        }
    }
}

fn tick_movement(movement: &mut OverworldMovement, tf: &mut Transform, dt: f32) -> MoveResult {
    // ── Bezier curve movement ──
    if let Some(ref mut bez) = movement.bezier {
        bez.elapsed += dt;
        let t = (bez.elapsed / bez.duration).clamp(0.0, 1.0);
        // CubicCurve domain is [0, num_segments], not [0, 1].
        // Scale t by the domain length so multi-segment curves are fully traversed.
        let curve_t = t * bez.curve.domain().length();
        let prev = tf.translation;
        tf.translation = bez.curve.sample(curve_t).unwrap_or(prev);
        let delta = tf.translation.truncate() - prev.truncate();

        if t >= 1.0 {
            movement.bezier = None;
            movement.target = None;
            movement.arrived = true;
            return MoveResult::Arrived;
        }
        return MoveResult::Moving(delta);
    }

    // ── Eased movement ──
    if let Some(ref mut ease) = movement.easing {
        let Some(target) = movement.target else {
            movement.easing = None;
            return MoveResult::Idle;
        };

        // Initialize on first tick (sentinel: elapsed < 0)
        if ease.elapsed < 0.0 {
            ease.start = tf.translation.truncate();
            ease.elapsed = 0.0;
            let dist = (target - ease.start).length();
            ease.duration = if movement.speed > 0.0 { dist / movement.speed } else { 1.0 };
        }

        ease.elapsed += dt;
        let raw_t = (ease.elapsed / ease.duration).clamp(0.0, 1.0);

        let eased_t = EasingCurve::new(0.0, 1.0, ease.ease_fn)
            .sample(raw_t)
            .unwrap_or(raw_t);

        let pos = ease.start.lerp(target, eased_t);
        let prev_pos = tf.translation.truncate();
        tf.translation.x = pos.x;
        tf.translation.y = pos.y;

        if raw_t >= 1.0 {
            tf.translation.x = target.x;
            tf.translation.y = target.y;
            movement.target = None;
            movement.easing = None;
            movement.arrived = true;
            return MoveResult::Arrived;
        }
        return MoveResult::Moving(pos - prev_pos);
    }

    // ── Linear movement ──
    let Some(target) = movement.target else {
        return MoveResult::Idle;
    };

    let current = tf.translation.truncate();
    let delta = target - current;
    let dist = delta.length();
    let step = movement.speed * dt;

    if dist <= step {
        tf.translation.x = target.x;
        tf.translation.y = target.y;
        movement.target = None;
        movement.arrived = true;
        MoveResult::Arrived
    } else {
        let dir = delta / dist;
        tf.translation.x += dir.x * step;
        tf.translation.y += dir.y * step;
        movement.arrived = false;
        MoveResult::Moving(delta)
    }
}
