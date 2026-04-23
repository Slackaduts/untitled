//! Tile composition: assembly grid → composite sprite + properties.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::state::{ComposedObject, TileEditorState};
use crate::billboard::object_types::ObjectProperties;

/// Build a composed object from the assembly grid.
/// Returns the composed object and its Image data so the caller can add it to assets.
/// Returns `None` if the assembly is empty or the atlas image isn't loaded.
pub fn compose_from_assembly(
    state: &TileEditorState,
    images: &Assets<Image>,
) -> Option<(ComposedObject, Image)> {
    if state.assembly.is_empty() {
        return None;
    }

    // All non-blank assembly slots must be from the same tileset
    let Some(first_real) = state.assembly.iter().find(|s| !s.blank) else {
        warn!("Assembly contains only blank tiles");
        return None;
    };
    let ts_idx = first_real.tileset_idx;
    if state.assembly.iter().any(|s| !s.blank && s.tileset_idx != ts_idx) {
        warn!("Cannot compose tiles from different tilesets");
        return None;
    }

    let ts = &state.tilesets[ts_idx];
    let atlas_handle = ts.atlas_handle.as_ref()?;
    let atlas_img = images.get(atlas_handle)?;
    let atlas_data = atlas_img.data.clone().unwrap_or_default();
    let atlas_w = atlas_img.texture_descriptor.size.width;
    let tile_w = ts.tile_width;
    let tile_h = ts.tile_height;
    let cols = ts.columns;

    // Compute bounding rect from assembly positions
    let max_col = state.assembly.iter().map(|s| s.col).max().unwrap();
    let max_row = state.assembly.iter().map(|s| s.row).max().unwrap();
    let grid_w = max_col + 1;
    let grid_h = max_row + 1;
    let comp_w = grid_w * tile_w;
    let comp_h = grid_h * tile_h;

    let bpp = 4u32;
    let mut comp_pixels = vec![0u8; (comp_w * comp_h * bpp) as usize];

    // Collect tile IDs for the sprite key (u32::MAX marks blank positions)
    let tile_ids: Vec<u32> = state.assembly.iter().map(|s| if s.blank { u32::MAX } else { s.tile_id }).collect();

    for slot in &state.assembly {
        if slot.blank {
            continue;
        }
        let ac = slot.tile_id % cols;
        let ar = slot.tile_id / cols;
        let src_x0 = ac * tile_w;
        let src_y0 = ar * tile_h;

        // Assembly row 0 = visual top = pixel row 0 in the output image.
        // Bevy/GPU textures have row 0 at top, so no flip needed here.
        // The billboard mesh handles the Y-up display.
        let dst_x0 = slot.col * tile_w;
        let dst_y0 = slot.row * tile_h;

        for py in 0..tile_h {
            let src_off = ((src_y0 + py) * atlas_w + src_x0) * bpp;
            let dst_off = ((dst_y0 + py) * comp_w + dst_x0) * bpp;
            let ss = src_off as usize;
            let se = ss + (tile_w * bpp) as usize;
            let dd = dst_off as usize;
            let de = dd + (tile_w * bpp) as usize;

            if se <= atlas_data.len() && de <= comp_pixels.len() {
                comp_pixels[dd..de].copy_from_slice(&atlas_data[ss..se]);
            }
        }
    }

    // Unpremultiply alpha
    for chunk in comp_pixels.chunks_exact_mut(4) {
        let a = chunk[3] as f32 / 255.0;
        if a > 0.01 && a < 0.99 {
            chunk[0] = (chunk[0] as f32 / a).min(255.0) as u8;
            chunk[1] = (chunk[1] as f32 / a).min(255.0) as u8;
            chunk[2] = (chunk[2] as f32 / a).min(255.0) as u8;
        }
    }

    let sprite_key = generate_sprite_key(&ts.name, &tile_ids);

    // Export to disk
    let dir = std::path::Path::new("assets/objects")
        .join(&ts.name)
        .join(&sprite_key);
    let png_path = dir.join("sprite.png");
    if !png_path.exists() {
        let _ = std::fs::create_dir_all(&dir);
        if let Some(img) = image::RgbaImage::from_raw(comp_w, comp_h, comp_pixels.clone()) {
            match img.save(&png_path) {
                Ok(()) => info!("Exported sprite: {}", png_path.display()),
                Err(e) => warn!("Failed to export sprite: {e}"),
            }
        }
    }

    // Load existing properties or create defaults
    let props_path = dir.join("properties.json");
    let mut properties: ObjectProperties = if props_path.exists() {
        std::fs::read_to_string(&props_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    } else {
        ObjectProperties::default()
    };
    properties.ensure_ref_ids();

    let image = Image::new(
        Extent3d {
            width: comp_w,
            height: comp_h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        comp_pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );

    let composed = ComposedObject {
        sprite_key,
        tileset_name: ts.name.clone(),
        tile_ids,
        width_px: comp_w,
        height_px: comp_h,
        image_handle: Handle::default(),
        #[cfg(feature = "dev_tools")]
        egui_texture: None,
        properties,
        collision_rects: Vec::new(),
    };

    Some((composed, image))
}

/// Generate a deterministic sprite key from tileset name + sorted tile IDs.
pub fn generate_sprite_key(tileset_name: &str, tile_ids: &[u32]) -> String {
    use std::hash::{Hash, Hasher};

    let mut sorted = tile_ids.to_vec();
    sorted.sort();
    sorted.dedup();

    let key_str = sorted
        .iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join("_");

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key_str.hash(&mut hasher);
    format!("{tileset_name}_{:08x}", hasher.finish() as u32)
}
