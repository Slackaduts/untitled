use bevy::audio::{AudioPlugin, SpatialScale};
use bevy::prelude::*;
#[cfg(feature = "dev_tools")]
use bevy::render::render_resource::WgpuFeatures;
#[cfg(feature = "dev_tools")]
use bevy::render::settings::{RenderCreation, WgpuSettings};
#[cfg(feature = "dev_tools")]
use bevy::render::RenderPlugin;
use bevy::window::{PresentMode, WindowMode};
use untitled::UntitledPlugin;

fn toggle_fullscreen(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut windows: Query<&mut Window>,
) {
    if keyboard.just_pressed(KeyCode::F11) {
        let mut window = windows.single_mut().unwrap();
        window.mode = match window.mode {
            WindowMode::Windowed => WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Current),
            _ => WindowMode::Windowed,
        };
    }
}

fn main() {
    let mut app = App::new();

    let default_plugins = DefaultPlugins
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
        });

    // POLYGON_MODE_LINE is only needed for wireframe debugging. Requesting it
    // unconditionally forces a feature path that's unused in release.
    #[cfg(feature = "dev_tools")]
    let default_plugins = default_plugins.set(RenderPlugin {
        render_creation: RenderCreation::Automatic(WgpuSettings {
            features: WgpuFeatures::POLYGON_MODE_LINE,
            ..default()
        }),
        ..default()
    });

    app.add_plugins(default_plugins);

    // WireframePlugin traverses every mesh in the render world each frame
    // looking for Wireframe components; keep it out of release builds.
    #[cfg(feature = "dev_tools")]
    app.add_plugins(bevy::pbr::wireframe::WireframePlugin::default());

    app.add_plugins(UntitledPlugin)
        .add_systems(Update, toggle_fullscreen)
        .run();
}
