use bevy::audio::{AudioPlugin, SpatialScale};
use bevy::prelude::*;
use bevy::render::settings::{RenderCreation, WgpuSettings};
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

/// Workaround for wgpu on Windows: launching with AutoNoVsync then switching
/// to AutoVsync after a few frames fixes frame timing issues where
/// prepare_windows blocks for 20-30ms regardless of present mode.
#[cfg(target_os = "windows")]
fn windows_present_mode_fix(
    mut windows: Query<&mut Window>,
    mut frames: Local<u32>,
) {
    *frames += 1;
    if *frames == 10 {
        if let Some(mut window) = windows.iter_mut().next() {
            window.present_mode = PresentMode::AutoVsync;
            info!("Switched present mode to AutoVsync (Windows wgpu workaround)");
        }
    }
}

fn main() {
    let mut app = App::new();

    // macOS: Fifo (vsync) produces smooth, consistent frame pacing.
    // Immediate/AutoNoVsync causes erratic `nextDrawable` blocking from the
    // compositor's adaptive refresh rate stepping (60→80→100→120Hz).
    // Other platforms: uncapped for maximum throughput.
    let present_mode = if cfg!(target_os = "macos") {
        PresentMode::AutoVsync
    } else {
        PresentMode::AutoNoVsync
    };

    // macOS: borderless fullscreen bypasses some compositor overhead and
    // locks the display to its maximum refresh rate.
    let window_mode = if cfg!(target_os = "macos") {
        WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Current)
    } else {
        WindowMode::Windowed
    };

    let default_plugins = DefaultPlugins
        .set(WindowPlugin {
            primary_window: Some(Window {
                title: "Untitled JRPG".into(),
                resolution: bevy::window::WindowResolution::new(1280, 720),
                present_mode,
                mode: window_mode,
                ..default()
            }),
            ..default()
        })
        .set(AudioPlugin {
            default_spatial_scale: SpatialScale::new_2d(1.0 / 100.0),
            ..default()
        });

    #[allow(unused_mut)]
    let mut wgpu_settings = WgpuSettings::default();

    // Windows: force DX12 backend. The Vulkan backend on Windows has frame
    // timing issues, and DX12 + StaticDxc (enabled via Cargo feature) gives
    // better shader codegen than FXC — important for AMD drivers where wgpu's
    // Vulkan path has known perf regressions.
    #[cfg(target_os = "windows")]
    {
        use bevy::render::settings::Backends;
        wgpu_settings.backends = Some(Backends::DX12);
    }

    // POLYGON_MODE_LINE is only needed for wireframe debugging. Requesting it
    // unconditionally forces a feature path that's unused in release.
    #[cfg(feature = "dev_tools")]
    {
        use bevy::render::render_resource::WgpuFeatures;
        wgpu_settings.features |= WgpuFeatures::POLYGON_MODE_LINE;
    }

    let default_plugins = default_plugins.set(RenderPlugin {
        render_creation: RenderCreation::Automatic(wgpu_settings),
        ..default()
    });

    app.add_plugins(default_plugins);

    // WireframePlugin traverses every mesh in the render world each frame
    // looking for Wireframe components; keep it out of release builds.
    #[cfg(feature = "dev_tools")]
    app.add_plugins(bevy::pbr::wireframe::WireframePlugin::default());

    app.add_plugins(UntitledPlugin)
        .add_systems(Update, toggle_fullscreen);

    #[cfg(target_os = "windows")]
    app.add_systems(Update, windows_present_mode_fix);

    app.run();
}
