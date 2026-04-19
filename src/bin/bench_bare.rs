use bevy::prelude::*;
use bevy::window::PresentMode;
use bevy::diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Bare Bevy Bench".into(),
                present_mode: PresentMode::Immediate,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .add_plugins(LogDiagnosticsPlugin::default())
        .add_systems(Startup, setup)
        .run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::default())),
        MeshMaterial3d(materials.add(Color::WHITE)),
    ));
    commands.spawn((
        DirectionalLight::default(),
        Transform::from_rotation(Quat::from_rotation_x(-1.0)),
    ));
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 2.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}
