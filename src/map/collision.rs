use avian3d::prelude::*;
use bevy::prelude::*;

/// Collision shape extracted from Tiled object layers.
#[derive(Component)]
pub enum CollisionShape {
    Rect { min: Vec2, max: Vec2 },
    Polygon { vertices: Vec<Vec2> },
}

/// Opt-in component for corner/edge slipping. When an entity is moving along
/// one axis and barely clips a wall corner, the system nudges them perpendicular
/// to slide around it — standard RPG feel.
#[derive(Component)]
pub struct CornerSlip {
    /// Max overlap (in pixels) that triggers a slip correction.
    pub threshold: f32,
}

impl Default for CornerSlip {
    fn default() -> Self {
        Self { threshold: 12.0 }
    }
}

/// Detects when an entity's desired velocity is blocked by a nearby corner
/// and adds a perpendicular nudge so they slip past it.
pub fn corner_slip_system(
    spatial_query: SpatialQuery,
    mut query: Query<(
        Entity,
        &Transform,
        &mut LinearVelocity,
        &Collider,
        &CornerSlip,
    )>,
) {
    for (entity, transform, mut vel, collider, slip) in &mut query {
        let desired = vel.0.truncate(); // XY only
        if desired.length_squared() < 1.0 {
            continue;
        }

        let pos = transform.translation;
        let filter = SpatialQueryFilter::default().with_excluded_entities([entity]);
        let probe_dist = 2.0;

        let abs = desired.abs();
        let is_horizontal = abs.x > abs.y * 2.0;
        let is_vertical = abs.y > abs.x * 2.0;

        if !is_horizontal && !is_vertical {
            continue;
        }

        let Ok(dir) = Dir3::new(Vec3::new(desired.x, desired.y, 0.0).normalize()) else {
            continue;
        };
        let config = ShapeCastConfig::from_max_distance(probe_dist);

        if spatial_query
            .cast_shape(collider, pos, Quat::IDENTITY, dir, &config, &filter)
            .is_none()
        {
            continue;
        }

        let perp = if is_horizontal { Vec2::Y } else { Vec2::X };

        for sign in [1.0_f32, -1.0] {
            let nudge_vec = perp * sign;
            let nudge_dir3 = Vec3::new(nudge_vec.x, nudge_vec.y, 0.0);

            let Ok(nudge_dir) = Dir3::new(nudge_dir3) else {
                continue;
            };
            let nudge_config = ShapeCastConfig::from_max_distance(slip.threshold);
            if spatial_query
                .cast_shape(collider, pos, Quat::IDENTITY, nudge_dir, &nudge_config, &filter)
                .is_some()
            {
                continue;
            }

            let nudged_pos = pos + nudge_dir3 * slip.threshold;
            if spatial_query
                .cast_shape(collider, nudged_pos, Quat::IDENTITY, dir, &config, &filter)
                .is_some()
            {
                continue;
            }

            vel.0 = Vec3::new(nudge_vec.x, nudge_vec.y, 0.0) * desired.length();
            break;
        }
    }
}
