use bevy::audio::{AudioPlugin, SpatialScale};
use bevy::pbr::wireframe::WireframePlugin;
use bevy::prelude::*;
use bevy::render::render_resource::WgpuFeatures;
use bevy::render::settings::{RenderCreation, WgpuSettings};
use bevy::render::RenderPlugin;
use bevy::window::PresentMode;
use untitled::UntitledPlugin;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Untitled JRPG".into(),
                        resolution: bevy::window::WindowResolution::new(1280, 720),
                        present_mode: PresentMode::Immediate,
                        ..default()
                    }),
                    ..default()
                })
                .set(AudioPlugin {
                    default_spatial_scale: SpatialScale::new_2d(1.0 / 100.0),
                    ..default()
                })
                .set(RenderPlugin {
                    render_creation: RenderCreation::Automatic(WgpuSettings {
                        features: WgpuFeatures::POLYGON_MODE_LINE,
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins(WireframePlugin::default())
        .add_plugins(UntitledPlugin)
        .run();
}
