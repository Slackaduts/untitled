use bevy::prelude::*;

use crate::camera::CombatCamera3d;
use super::custom::CustomPostProcess;

/// Spawn this component on any entity (or a standalone entity) to trigger a
/// shockwave distortion on the terrain plane.  The system auto-ticks the
/// animation and despawns the entity when the wave finishes.
#[derive(Component)]
pub struct ShockwaveEmitter {
    /// World-space center on the terrain plane (XY).
    pub center: Vec2,
    /// Maximum world-space radius the ring expands to.
    pub max_radius: f32,
    /// Total animation duration in seconds.
    pub duration: f32,
    /// Current elapsed time.
    pub elapsed: f32,
    /// Peak UV displacement strength.
    pub intensity: f32,
    /// Ring thickness in world units.
    pub thickness: f32,
    /// Chromatic split amount (0 = none, 0.01+ = prismatic edge).
    pub chromatic: f32,
}

impl Default for ShockwaveEmitter {
    fn default() -> Self {
        Self {
            center: Vec2::ZERO,
            max_radius: 200.0,
            duration: 0.8,
            elapsed: 0.0,
            intensity: 0.04,
            thickness: 40.0,
            chromatic: 0.005,
        }
    }
}

/// Tick active shockwave emitters, project to screen space, pack into the
/// custom post-process uniform (up to 4 slots), despawn finished ones.
pub fn tick_shockwaves(
    time: Res<Time>,
    mut emitters: Query<(Entity, &mut ShockwaveEmitter)>,
    camera_q: Query<(&Camera, &GlobalTransform), With<CombatCamera3d>>,
    mut pp_q: Query<&mut CustomPostProcess>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    let Ok((camera, cam_tf)) = camera_q.single() else { return };
    let Ok(mut pp) = pp_q.single_mut() else { return };

    // Clear all slots
    pp.shockwave_0 = Vec4::ZERO;
    pp.shockwave_0_extra = Vec4::ZERO;
    pp.shockwave_1 = Vec4::ZERO;
    pp.shockwave_1_extra = Vec4::ZERO;
    pp.shockwave_2 = Vec4::ZERO;
    pp.shockwave_2_extra = Vec4::ZERO;
    pp.shockwave_3 = Vec4::ZERO;
    pp.shockwave_3_extra = Vec4::ZERO;

    let viewport_size = camera.logical_viewport_size().unwrap_or(Vec2::new(1280.0, 720.0));
    let mut slot = 0u32;
    let mut to_despawn = Vec::new();

    for (entity, mut emitter) in emitters.iter_mut() {
        emitter.elapsed += dt;
        let t = (emitter.elapsed / emitter.duration).min(1.0);

        if t >= 1.0 {
            to_despawn.push(entity);
            continue;
        }

        if slot >= 4 { continue; }

        // Current radius expands linearly
        let current_radius = emitter.max_radius * t;
        // Intensity fades out as wave expands
        let fade = 1.0 - t * t; // quadratic fade
        let current_intensity = emitter.intensity * fade;

        if current_intensity < 0.0001 { continue; }

        // Project world center to screen UV
        let world_pos = Vec3::new(emitter.center.x, emitter.center.y, 0.0);
        let Ok(screen_pos) = camera.world_to_viewport(cam_tf, world_pos) else { continue };
        let center_uv = screen_pos / viewport_size;

        // Project a point at (center + radius) to get screen-space radius
        let edge_world = Vec3::new(
            emitter.center.x + current_radius,
            emitter.center.y,
            0.0,
        );
        let Ok(edge_screen) = camera.world_to_viewport(cam_tf, edge_world) else { continue };
        let radius_uv = (edge_screen.x - screen_pos.x).abs() / viewport_size.x;

        // Same for thickness
        let thick_world = Vec3::new(
            emitter.center.x + emitter.thickness,
            emitter.center.y,
            0.0,
        );
        let Ok(thick_screen) = camera.world_to_viewport(cam_tf, thick_world) else { continue };
        let thickness_uv = (thick_screen.x - screen_pos.x).abs() / viewport_size.x;

        let main = Vec4::new(center_uv.x, center_uv.y, radius_uv, current_intensity);
        let extra = Vec4::new(thickness_uv.max(0.001), emitter.chromatic, 0.0, 0.0);

        match slot {
            0 => { pp.shockwave_0 = main; pp.shockwave_0_extra = extra; }
            1 => { pp.shockwave_1 = main; pp.shockwave_1_extra = extra; }
            2 => { pp.shockwave_2 = main; pp.shockwave_2_extra = extra; }
            3 => { pp.shockwave_3 = main; pp.shockwave_3_extra = extra; }
            _ => {}
        }
        slot += 1;
    }

    for entity in to_despawn {
        commands.entity(entity).despawn();
    }
}
