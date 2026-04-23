//! Runtime spawner: loads sidecar `.objects.json` and creates Billboard entities.
//!
//! This module is NOT behind `dev_tools` — placed objects spawn in release builds.

use avian3d::parry::math::{Pose, Real};
use avian3d::parry::shape::SharedShape;
use avian3d::prelude::*;
use bevy::light::NotShadowReceiver;
use bevy::prelude::*;

use crate::billboard::object_types::ObjectSpriteLight;
use crate::camera::combat::{
    Billboard, BillboardCache, BillboardElevation, BillboardHeight, BillboardLayerOffset,
    BillboardSpriteKey, BillboardTileQuad, BillboardTilesReady,
};
use crate::lighting::components::*;
use crate::map::loader::CurrentMap;
use crate::map::DEFAULT_TILE_SIZE;
use crate::particles::gpu_lights::{ParticleLightBuffer, ParticleLightExt};

use super::sidecar::{self, CollisionRect, PlacedObjectDef};
use super::state::PlacedObject;

// ── Resources ──────────────────────────────────────────────────────────────

/// Tracks whether sidecar objects have been spawned for the current map.
#[derive(Resource, Default)]
pub struct SidecarObjectsSpawned {
    pub spawned_for: Option<String>,
}

// ── System ─────────────────────────────────────────────────────────────────

/// Spawns placed objects from the sidecar file after billboards are ready.
pub fn spawn_sidecar_objects(
    mut commands: Commands,
    billboard_ready: Res<BillboardTilesReady>,
    current_map: Res<CurrentMap>,
    mut spawned: ResMut<SidecarObjectsSpawned>,
    mut editor_state: ResMut<super::state::TileEditorState>,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut bb_materials: ResMut<Assets<bevy::pbr::ExtendedMaterial<StandardMaterial, ParticleLightExt>>>,
    particle_buf: Res<ParticleLightBuffer>,
    mut images: ResMut<Assets<Image>>,
) {
    // Wait for the tilemap billboard pass to finish
    if !billboard_ready.0 {
        return;
    }

    // Only spawn once per map
    let Some(map_path) = &current_map.path else {
        return;
    };
    if spawned.spawned_for.as_ref() == Some(map_path) {
        return;
    }
    spawned.spawned_for = Some(map_path.clone());

    // Load sidecar
    let Some(sidecar_file) = sidecar::load_sidecar(map_path) else {
        info!("No sidecar file for {map_path}");
        return;
    };

    if sidecar_file.objects.is_empty() {
        return;
    }

    // Populate editor state so the Place mode UI can list/edit these objects
    editor_state.placed_objects = sidecar_file.objects.clone();

    let mut object_count = 0u32;
    let mut light_count = 0u32;
    let mut emitter_count = 0u32;

    for obj_def in &sidecar_file.objects {
        let Some(_entity) = spawn_placed_object(
            &mut commands,
            &asset_server,
            &mut meshes,
            &mut bb_materials,
            &particle_buf,
            &mut images,
            obj_def,
        ) else {
            warn!(
                "Failed to spawn placed object '{}' (sprite_key: {})",
                obj_def.id, obj_def.sprite_key
            );
            continue;
        };

        // Spawn lights
        for light_def in &obj_def.properties.lights {
            spawn_object_light(&mut commands, obj_def, light_def);
            light_count += 1;
        }

        // Spawn emitters
        for emitter_def in &obj_def.properties.emitters {
            spawn_object_emitter(&mut commands, obj_def, emitter_def);
            emitter_count += 1;
        }

        object_count += 1;
    }

    if object_count > 0 {
        info!(
            "Spawned {object_count} placed objects ({light_count} lights, {emitter_count} emitters) from sidecar"
        );
    }
}

// ── Spawn helpers ──────────────────────────────────────────────────────────

/// Spawn a single placed object immediately (for editor placement).
/// Delegates to `spawn_placed_object` + `spawn_object_light`.
pub fn spawn_sidecar_object_immediate(
    commands: &mut Commands,
    asset_server: &AssetServer,
    meshes: &mut Assets<Mesh>,
    bb_materials: &mut Assets<bevy::pbr::ExtendedMaterial<StandardMaterial, ParticleLightExt>>,
    particle_buf: &ParticleLightBuffer,
    images: &mut Assets<Image>,
    def: &PlacedObjectDef,
) {
    if let Some(_entity) = spawn_placed_object(
        commands, asset_server, meshes, bb_materials, particle_buf, images, def,
    ) {
        for light_def in &def.properties.lights {
            spawn_object_light(commands, def, light_def);
        }
        for emitter_def in &def.properties.emitters {
            spawn_object_emitter(commands, def, emitter_def);
        }
    }
}

/// Spawn a single placed object as a Billboard entity. Returns the entity ID.
fn spawn_placed_object(
    commands: &mut Commands,
    asset_server: &AssetServer,
    meshes: &mut Assets<Mesh>,
    bb_materials: &mut Assets<bevy::pbr::ExtendedMaterial<StandardMaterial, ParticleLightExt>>,
    particle_buf: &ParticleLightBuffer,
    images: &mut Assets<Image>,
    def: &PlacedObjectDef,
) -> Option<Entity> {
    // Try loading the sprite (QOI first, then PNG)
    let ts_name = &def.tileset;
    let sprite_key = &def.sprite_key;
    let qoi_path = format!("objects/{ts_name}/{sprite_key}/sprite.qoi");
    let png_path = format!("objects/{ts_name}/{sprite_key}/sprite.png");

    let sprite_path = if std::path::Path::new(&format!("assets/{qoi_path}")).exists() {
        qoi_path
    } else if std::path::Path::new(&format!("assets/{png_path}")).exists() {
        png_path
    } else {
        warn!("No sprite found for placed object {sprite_key}");
        return None;
    };

    let texture_handle: Handle<Image> = asset_server.load(&sprite_path);

    // We need the image dimensions. Try the asset first, then read from disk.
    let (img_w, img_h) = if let Some(img) = images.get(&texture_handle) {
        let size = img.size();
        (size.x as f32, size.y as f32)
    } else {
        // Asset not loaded yet — read dimensions directly from the file on disk.
        let disk_path = format!("assets/{sprite_path}");
        if let Ok((w, h)) = image::image_dimensions(&disk_path) {
            (w as f32, h as f32)
        } else {
            let tile_count = def.tile_ids.len().max(1) as f32;
            (DEFAULT_TILE_SIZE * tile_count, DEFAULT_TILE_SIZE)
        }
    };

    use crate::billboard::object_types::SpriteType;

    // Determine quad size based on sprite type
    let (quad_w, quad_h, quad_mesh) = match &def.properties.sprite_type {
        SpriteType::Lpc => {
            let fw = 64.0_f32;
            let fh = 64.0_f32;
            // LPC default idle: row 10 (walk down), col 0
            let mesh = crate::camera::combat::create_animated_billboard_quad(
                fw, fh, img_w, img_h, 0, 10,
            );
            (fw, fh, mesh)
        }
        SpriteType::Custom { frame_w, frame_h, .. } => {
            let fw = *frame_w as f32;
            let fh = *frame_h as f32;
            let mesh = crate::camera::combat::create_animated_billboard_quad(
                fw, fh, img_w, img_h, 0, 0,
            );
            (fw, fh, mesh)
        }
        SpriteType::Static => {
            let origin_px_x = img_w * 0.5;
            let origin_px_y = DEFAULT_TILE_SIZE * 0.5;
            let mesh = crate::camera::combat::create_billboard_quad(
                img_w, img_h, origin_px_x, origin_px_y,
            );
            (img_w, img_h, mesh)
        }
    };

    // Normal map: use flat normal for placed objects
    let _flat_normal = create_flat_normal(images, quad_w as u32, quad_h as u32);

    let mat = bb_materials.add(bevy::pbr::ExtendedMaterial {
        base: StandardMaterial {
            base_color_texture: Some(texture_handle),
            alpha_mode: AlphaMode::Mask(0.5),
            unlit: false,
            perceptual_roughness: 1.0,
            metallic: 0.0,
            reflectance: 0.0,
            double_sided: true,
            cull_mode: None,
            ..default()
        },
        extension: ParticleLightExt {
            particle_data: particle_buf.handle.clone(),
        },
    });

    // World position from grid coordinates
    let world_x = (def.grid_pos[0] as f32 + 0.5) * DEFAULT_TILE_SIZE;
    let world_y = (def.grid_pos[1] as f32 + 0.5) * DEFAULT_TILE_SIZE;

    let mut entity_cmds = commands.spawn((
        Mesh3d(meshes.add(quad_mesh)),
        MeshMaterial3d(mat),
        Transform::from_xyz(world_x, world_y, 0.0),
        Billboard,
        BillboardTileQuad,
        BillboardHeight {
            height: quad_h,
            base_y: world_y,
        },
        BillboardElevation {
            level: def.elevation,
        },
        BillboardLayerOffset(0.0),
        BillboardSpriteKey(def.sprite_key.clone()),
        BillboardCache::default(),
        NotShadowReceiver,
        PlacedObject {
            sidecar_id: def.id.clone(),
            name: def.name.clone(),
        },
        // Prevent combat.rs::spawn_object_lights from also spawning lights
        // for this entity — the sidecar spawner handles lights/emitters itself.
        crate::camera::combat::ObjectLightsSpawned,
    ));

    // Add Bevy Name component if the instance has a user-defined name
    if let Some(name) = &def.name {
        entity_cmds.insert(Name::new(name.clone()));
    }

    // Add animation components for animated sprites
    match &def.properties.sprite_type {
        SpriteType::Lpc => {
            entity_cmds.insert((
                crate::sprite::animation::AnimationController::default(),
                crate::sprite::animation::SpriteFormatKind::Lpc,
                crate::sprite::splitter::AtlasMeta {
                    frame_size: UVec2::new(64, 64),
                    columns: 13,
                    rows: (img_h as u32) / 64,
                },
            ));
        }
        SpriteType::Custom { frame_w, frame_h, columns } => {
            let rows = if *frame_h > 0 { (img_h as u32) / frame_h } else { 1 };
            entity_cmds.insert((
                crate::sprite::animation::AnimationController::default(),
                crate::sprite::animation::SpriteFormatKind::Custom { columns: *columns },
                crate::sprite::splitter::AtlasMeta {
                    frame_size: UVec2::new(*frame_w, *frame_h),
                    columns: *columns,
                    rows,
                },
            ));
        }
        SpriteType::Static => {}
    }

    // Collision from rects
    if !def.collision_rects.is_empty() {
        if let Some(collider) = build_compound_collider(&def.collision_rects, quad_w, quad_h) {
            entity_cmds.insert(collider);
            entity_cmds.insert(RigidBody::Static);
        }
    }

    // Door sensor
    if let Some(door) = &def.door {
        entity_cmds.insert(super::door::DoorPortal {
            target_map: door.target_map.clone(),
            spawn_point: IVec2::new(door.spawn_point[0], door.spawn_point[1]),
            script: door.script.clone(),
        });
    }

    Some(entity_cmds.id())
}

/// Compute billboard world position, tilt rotation, height, and width for a placed object.
fn billboard_transform_for(obj_def: &PlacedObjectDef) -> (Vec3, Quat, f32, f32) {
    let world_x = (obj_def.grid_pos[0] as f32 + 0.5) * DEFAULT_TILE_SIZE;
    let world_y = (obj_def.grid_pos[1] as f32 + 0.5) * DEFAULT_TILE_SIZE;
    let bb_pos = Vec3::new(world_x, world_y, 0.0);

    // Read actual sprite dimensions from disk
    let (bb_w, bb_h) = sprite_dimensions_for(obj_def);

    // Match the tilt logic from combat.rs: taller billboards stand more upright
    let default_tilt = crate::camera::combat::BILLBOARD_TILT_DEG.to_radians();
    let tiles_tall = bb_h / DEFAULT_TILE_SIZE;
    let max_upright = 55.0_f32.to_radians();
    let t = ((tiles_tall - 1.0) / 4.0).clamp(0.0, 1.0);
    let tilt = default_tilt + (max_upright - default_tilt) * t;
    let rotation = Quat::from_rotation_x(tilt);

    (bb_pos, rotation, bb_h, bb_w)
}

/// Get the actual sprite pixel dimensions for a placed object (reads from disk).
fn sprite_dimensions_for(obj_def: &PlacedObjectDef) -> (f32, f32) {
    let ts = &obj_def.tileset;
    let key = &obj_def.sprite_key;
    let qoi = format!("assets/objects/{ts}/{key}/sprite.qoi");
    let png = format!("assets/objects/{ts}/{key}/sprite.png");
    let path = if std::path::Path::new(&qoi).exists() { qoi } else { png };
    if let Ok((w, h)) = image::image_dimensions(&path) {
        (w as f32, h as f32)
    } else {
        (DEFAULT_TILE_SIZE, DEFAULT_TILE_SIZE)
    }
}

/// Public entry point for respawning a light (used by editor refresh).
pub fn spawn_object_light_pub(
    commands: &mut Commands,
    obj_def: &PlacedObjectDef,
    light_def: &crate::billboard::object_types::ObjectLight,
) {
    spawn_object_light(commands, obj_def, light_def);
}

/// Public entry point for respawning an emitter (used by editor refresh).
pub fn spawn_object_emitter_pub(
    commands: &mut Commands,
    obj_def: &PlacedObjectDef,
    emitter_def: &crate::billboard::object_types::ObjectEmitter,
) {
    spawn_object_emitter(commands, obj_def, emitter_def);
}

/// Spawn a light attached to a placed object billboard.
fn spawn_object_light(
    commands: &mut Commands,
    obj_def: &PlacedObjectDef,
    light_def: &crate::billboard::object_types::ObjectLight,
) {
    let (bb_pos, rotation, bb_h, bb_w) = billboard_transform_for(obj_def);

    let shape = match light_def.shape.as_str() {
        "cone" => LightShape::Cone {
            direction: 0.0,
            angle: std::f32::consts::FRAC_PI_2,
        },
        "line" => LightShape::Line {
            end_offset: Vec2::new(48.0, 0.0),
        },
        "capsule" => LightShape::Capsule {
            direction: 0.0,
            half_length: 24.0,
        },
        _ => LightShape::Point,
    };

    // Local offset in billboard space — Z is forward (away from billboard face)
    // offset_y is in image space (0=top, 1=bottom), billboard Y is up, so flip.
    // Billboard origin: X centered, Y at center of bottom tile.
    let tile = DEFAULT_TILE_SIZE;
    let local = Vec3::new(
        (light_def.offset_x - 0.5) * bb_w,
        (1.0 - light_def.offset_y) * bb_h - tile * 0.5,
        light_def.offset_z * bb_h,
    );
    // Apply billboard tilt rotation so the light follows the billboard's angle
    let light_pos = bb_pos + rotation * local;

    commands.spawn((
        Transform::from_translation(light_pos),
        Visibility::default(),
        LightSource {
            color: Color::linear_rgb(
                light_def.color[0],
                light_def.color[1],
                light_def.color[2],
            ),
            base_intensity: light_def.intensity,
            intensity: light_def.intensity,
            inner_radius: light_def.radius * 0.3,
            outer_radius: light_def.radius,
            shape,
            pulse: if light_def.pulse {
                Some(PulseConfig::default())
            } else {
                None
            },
            flicker: if light_def.flicker {
                Some(FlickerConfig::default())
            } else {
                None
            },
            anim_seed: rand::random::<f32>() * 100.0,
            ..default()
        },
        ObjectSpriteLight {
            sprite_key: obj_def.sprite_key.clone(),
            ref_id: light_def.ref_id.clone(),
            offset_x: light_def.offset_x,
            offset_y: light_def.offset_y,
            offset_z: light_def.offset_z,
            sprite_width: bb_w,
        },
        super::state::SidecarChild {
            sidecar_id: obj_def.id.clone(),
            ref_id: light_def.ref_id.clone(),
        },
    ));
}

/// Spawn a particle emitter attached to a placed object billboard.
fn spawn_object_emitter(
    commands: &mut Commands,
    obj_def: &PlacedObjectDef,
    emitter_def: &crate::billboard::object_types::ObjectEmitter,
) {
    if emitter_def.definition_id.is_empty() {
        return;
    }

    let (bb_pos, rotation, bb_h, bb_w) = billboard_transform_for(obj_def);

    // Billboard origin: X centered, Y at center of bottom tile.
    let tile = DEFAULT_TILE_SIZE;
    let local = Vec3::new(
        (emitter_def.offset_x - 0.5) * bb_w,
        (1.0 - emitter_def.offset_y) * bb_h - tile * 0.5,
        emitter_def.offset_z * bb_h,
    );
    let emitter_pos = bb_pos + rotation * local;

    commands.spawn((
        Transform::from_translation(emitter_pos),
        Visibility::default(),
        crate::particles::emitter::ParticleEmitter::new(
            &emitter_def.definition_id,
            emitter_def.rate,
        ),
        crate::billboard::object_types::ObjectSpriteEmitter {
            sprite_key: obj_def.sprite_key.clone(),
            ref_id: emitter_def.ref_id.clone(),
            offset_x: emitter_def.offset_x,
            offset_y: emitter_def.offset_y,
            offset_z: emitter_def.offset_z,
            sprite_width: bb_w,
        },
        super::state::SidecarChild {
            sidecar_id: obj_def.id.clone(),
            ref_id: emitter_def.ref_id.clone(),
        },
    ));
}

/// Build a compound collider from collision rects.
fn build_compound_collider(rects: &[CollisionRect], sprite_w: f32, _sprite_h: f32) -> Option<Collider> {
    if rects.is_empty() {
        return None;
    }

    let shapes: Vec<(Pose, SharedShape)> = rects
        .iter()
        .map(|r| {
            let half_w = (r.w / 2.0).max(0.5) as Real;
            let half_h = (r.h / 2.0).max(0.5) as Real;
            let half_depth = ((r.depth_fwd + r.depth_back) / 2.0).max(0.5) as Real;

            // Convert sprite-local coords to billboard-local coords.
            // Sprite origin is at center-bottom, so:
            //   local_x = rect_center_x - sprite_w/2
            //   local_y = rect_center_y (from bottom)
            let cx = (r.x + r.w / 2.0 - sprite_w / 2.0) as Real;
            let cy = (r.y + r.h / 2.0) as Real;
            let cz = ((r.depth_fwd - r.depth_back) / 2.0) as Real;

            (
                Pose::from_translation(avian3d::parry::math::Vector::new(cx, cy, cz)),
                SharedShape::cuboid(half_w, half_h, half_depth),
            )
        })
        .collect();

    Some(SharedShape::compound(shapes).into())
}

/// Create a flat normal map texture (pointing straight out: 128, 128, 255).
fn create_flat_normal(images: &mut Assets<Image>, w: u32, h: u32) -> Handle<Image> {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    let mut pixels = vec![0u8; (w * h * 4) as usize];
    for chunk in pixels.chunks_exact_mut(4) {
        chunk[0] = 128;
        chunk[1] = 128;
        chunk[2] = 255;
        chunk[3] = 255;
    }
    images.add(Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ))
}
