use bevy::prelude::*;

/// Controls sprite animation playback.
#[derive(Component)]
pub struct AnimationController {
    pub current_animation: String,
    pub direction: u8,
    pub frame: u32,
    pub timer: Timer,
    pub looping: bool,
    /// True when the animation is actively playing (not paused/stopped).
    pub playing: bool,
}

impl Default for AnimationController {
    fn default() -> Self {
        Self {
            current_animation: "walk".into(),
            direction: 2, // facing down
            frame: 0,
            timer: Timer::from_seconds(0.1, TimerMode::Repeating),
            looping: true,
            playing: true,
        }
    }
}

/// Which spritesheet format to use for frame lookup.
/// Stored as a component so the UV update system can resolve frames.
#[derive(Component, Clone)]
pub enum SpriteFormatKind {
    Lpc,
    Custom {
        /// Map of animation name → (base_row, frame_count, directions)
        /// For simple sheets: one entry "default" → (0, columns, 1)
        columns: u32,
    },
}

impl SpriteFormatKind {
    /// Get the row for a given animation and direction.
    pub fn row_for(&self, animation: &str, direction: u8) -> u32 {
        match self {
            Self::Lpc => {
                let base = match animation {
                    "spellcast" => 0,
                    "thrust" => 4,
                    "walk" => 8,
                    "slash" => 12,
                    "shoot" => 16,
                    "hurt" => return 20,
                    _ => 8, // default to walk
                };
                base + direction.min(3) as u32
            }
            Self::Custom { .. } => 0,
        }
    }

    /// Get the number of frames for a given animation.
    pub fn frame_count(&self, animation: &str) -> u32 {
        match self {
            Self::Lpc => match animation {
                "spellcast" => 7,
                "thrust" => 8,
                "walk" => 9,
                "slash" => 6,
                "shoot" => 13,
                "hurt" => 6,
                _ => 1,
            },
            Self::Custom { columns } => *columns,
        }
    }
}

/// System: advances animation frame timers.
pub fn tick_animations(time: Res<Time>, mut query: Query<(&mut AnimationController, &SpriteFormatKind)>) {
    for (mut anim, format) in &mut query {
        if !anim.playing {
            continue;
        }
        anim.timer.tick(time.delta());
        if anim.timer.just_finished() {
            let max_frames = format.frame_count(&anim.current_animation);
            anim.frame += 1;
            if anim.frame >= max_frames {
                if anim.looping {
                    anim.frame = 0;
                } else {
                    anim.frame = max_frames.saturating_sub(1);
                    anim.playing = false;
                }
            }
        }
    }
}

/// System: updates billboard mesh UVs to show the current animation frame.
pub fn update_sprite_uvs(
    query: Query<
        (&AnimationController, &SpriteFormatKind, &super::splitter::AtlasMeta, &Mesh3d),
    >,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    for (anim, format, atlas, mesh_handle) in &query {
        let row = format.row_for(&anim.current_animation, anim.direction);
        let col = anim.frame;

        let sheet_w = atlas.columns as f32 * atlas.frame_size.x as f32;
        let sheet_h = atlas.rows as f32 * atlas.frame_size.y as f32;
        let fw = atlas.frame_size.x as f32;
        let fh = atlas.frame_size.y as f32;

        let u_min = col as f32 * fw / sheet_w;
        let u_max = (col as f32 + 1.0) * fw / sheet_w;
        let v_min = row as f32 * fh / sheet_h;
        let v_max = (row as f32 + 1.0) * fh / sheet_h;

        let Some(mesh) = meshes.get_mut(&mesh_handle.0) else {
            continue;
        };

        let uvs = vec![
            [u_min, v_max],  // bottom-left
            [u_max, v_max],  // bottom-right
            [u_max, v_min],  // top-right
            [u_min, v_min],  // top-left
        ];
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    }
}
