//! Per-sprite GLB shadow casters.
//!
//! When a billboard's sprite directory contains `shadow.glb`, load the Scene
//! and spawn it as an invisible shadow caster at the billboard's world
//! position. The mesh lives on `SHADOW_CASTER_LAYER` so the main camera
//! doesn't render it; the sun light's `RenderLayers` include that layer so it
//! still enters the shadow pass. The original billboard is marked
//! `NotShadowCaster` so it doesn't double-cast with the depth-extruded
//! silhouette.

use bevy::camera::visibility::RenderLayers;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::scene::SceneRoot;

use super::combat::{
    Billboard, BillboardHeight, BillboardSpriteKey, BillboardTileQuad, BillboardTilesReady,
};

/// Layer index for shadow-only meshes (camera ignores, sun includes).
pub const SHADOW_CASTER_LAYER: usize = 1;

/// Marker so each billboard is processed once.
#[derive(Component)]
pub struct ShadowMeshProcessed;

/// Marker on the SceneRoot entity of a shadow-caster mesh. Used by the
/// layer-propagation system to recursively tag child mesh entities that
/// Bevy spawns when instantiating the scene.
#[derive(Component)]
pub struct ShadowMeshRoot;

pub fn spawn_shadow_meshes(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    billboard_ready: Res<BillboardTilesReady>,
    billboards: Query<
        (Entity, &BillboardSpriteKey, &BillboardHeight, &Transform),
        (With<Billboard>, With<BillboardTileQuad>, Without<ShadowMeshProcessed>),
    >,
) {
    if !billboard_ready.0 || billboards.is_empty() {
        return;
    }

    for (entity, key, height, tf) in &billboards {
        let ts_name = key
            .0
            .rsplit_once('_')
            .map(|(name, _)| name)
            .unwrap_or(&key.0);
        let glb_rel = format!("objects/{ts_name}/{}/shadow.glb", key.0);
        let fs_path = format!("assets/{}", glb_rel);

        commands.entity(entity).insert(ShadowMeshProcessed);

        if !std::path::Path::new(&fs_path).exists() {
            continue;
        }

        let scene: Handle<Scene> = asset_server.load(format!("{glb_rel}#Scene0"));

        // Hunyuan3D output is Y-up. Compose: Y-up → Z-up first, then apply
        // the billboard's current X-axis tilt so the mesh leans toward the
        // camera the same way its sprite does. Reading the tilt off the
        // billboard's Transform requires billboard_system to have run first
        // this frame (see system ordering in CameraPlugin).
        let yup_to_zup = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
        let rotation = tf.rotation * yup_to_zup;

        // Hunyuan meshes span ~2.0 units in their height axis; scale =
        // height / 2 puts mesh height ≈ billboard height in world units.
        let scale_uniform = height.height / 2.0;
        let scale = Vec3::splat(scale_uniform);

        // The billboard's transform anchors at the sprite's bottom-center,
        // but the mesh is centered on its origin. Shift the mesh up along
        // its own (post-rotation) up axis by half the scaled height so its
        // bottom sits at the ground plane. Using the rotated up axis keeps
        // the lift aligned with the mesh's lean when tilted.
        let mesh_up_world = rotation * Vec3::Y;
        let translation = tf.translation + mesh_up_world * scale_uniform;

        commands.spawn((
            SceneRoot(scene),
            Transform {
                translation,
                rotation,
                scale,
            },
            RenderLayers::layer(SHADOW_CASTER_LAYER),
            ShadowMeshRoot,
            Name::new(format!("ShadowMesh({})", key.0)),
        ));

        commands.entity(entity).insert(NotShadowCaster);
    }
}

/// Apply `RenderLayers::layer(SHADOW_CASTER_LAYER)` to every descendant of a
/// `ShadowMeshRoot`. Bevy spawns scene meshes as children asynchronously and
/// does not propagate the parent's `RenderLayers` — without this, those
/// child meshes default to layer 0 and become visible to the main camera.
///
/// Cheap to run every frame: once all descendants have the component, the
/// inserts become no-ops (filtered by `Without<RenderLayers>`).
pub fn propagate_shadow_mesh_layers(
    mut commands: Commands,
    roots: Query<Entity, With<ShadowMeshRoot>>,
    children_q: Query<&Children>,
    tagged_q: Query<&RenderLayers>,
) {
    fn walk(
        entity: Entity,
        commands: &mut Commands,
        children_q: &Query<&Children>,
        tagged_q: &Query<&RenderLayers>,
    ) {
        if tagged_q.get(entity).is_err() {
            commands
                .entity(entity)
                .insert(RenderLayers::layer(SHADOW_CASTER_LAYER));
        }
        if let Ok(children) = children_q.get(entity) {
            for child in children.iter() {
                walk(child, commands, children_q, tagged_q);
            }
        }
    }

    for root in &roots {
        walk(root, &mut commands, &children_q, &tagged_q);
    }
}
