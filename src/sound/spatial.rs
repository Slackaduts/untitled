use bevy::prelude::*;

/// Attach to any spatial audio emitter to control its audible radius.
/// Volume scales linearly from full at `inner_radius` to silent at `outer_radius`.
#[derive(Component)]
pub struct SpatialFalloff {
    /// Distance (in world units) within which audio is at full volume.
    pub inner_radius: f32,
    /// Distance (in world units) beyond which audio is silent.
    pub outer_radius: f32,
}

impl Default for SpatialFalloff {
    fn default() -> Self {
        Self {
            inner_radius: 50.0,
            outer_radius: 300.0,
        }
    }
}

/// Marker for the spatial audio listener entity (typically the camera or player).
/// This entity must also have `bevy::audio::SpatialListener` and `Transform`.
#[derive(Component)]
pub struct GameListener;

/// Each frame, adjust the volume of spatial emitters based on distance to the listener.
pub fn update_spatial_falloff(
    listener_q: Query<&Transform, With<GameListener>>,
    mut emitter_q: Query<(&Transform, &SpatialFalloff, &AudioSink)>,
) {
    let Some(listener_tf) = listener_q.iter().next() else {
        return;
    };

    for (emitter_tf, falloff, sink) in &mut emitter_q {
        let distance = listener_tf
            .translation
            .truncate()
            .distance(emitter_tf.translation.truncate());

        let volume = if distance <= falloff.inner_radius {
            1.0
        } else if distance >= falloff.outer_radius {
            0.0
        } else {
            1.0 - (distance - falloff.inner_radius)
                / (falloff.outer_radius - falloff.inner_radius)
        };

        sink.set_volume(volume);
    }
}
