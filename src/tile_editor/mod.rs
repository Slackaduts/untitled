pub mod door;
pub mod sidecar;
pub mod spawner;
pub mod state;
#[cfg(feature = "dev_tools")]
pub mod collision_editor;
#[cfg(feature = "dev_tools")]
pub mod composition;
#[cfg(feature = "dev_tools")]
pub mod gizmos;
#[cfg(feature = "dev_tools")]
pub mod library;
#[cfg(feature = "dev_tools")]
pub mod lights_emitters;
#[cfg(feature = "dev_tools")]
pub mod live_preview;
#[cfg(feature = "dev_tools")]
pub mod placement;
#[cfg(feature = "dev_tools")]
pub mod tileset_browser;

use bevy::prelude::*;

pub struct TileEditorPlugin;

impl Plugin for TileEditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<state::TileEditorState>()
            .init_resource::<spawner::SidecarObjectsSpawned>()
            .add_systems(Update, (
                spawner::spawn_sidecar_objects,
                door::door_trigger_system,
            ));

        #[cfg(feature = "dev_tools")]
        {
            app.add_systems(
                Update,
                (
                    toggle_editor,
                    editor_ui
                        .after(toggle_editor)
                        .run_if(|state: Res<state::TileEditorState>| state.open),
                    gizmos::draw_placed_object_gizmos
                        .run_if(|state: Res<state::TileEditorState>| state.open),
                    gizmos::draw_placed_light_gizmos
                        .run_if(|state: Res<state::TileEditorState>| state.open),
                    placement::place_object_on_click
                        .after(editor_ui)
                        .run_if(|state: Res<state::TileEditorState>| {
                            state.open && state.mode == state::EditorMode::Place
                        }),
                    placement::select_placed_object
                        .after(editor_ui)
                        .run_if(|state: Res<state::TileEditorState>| state.open),
                    placement::delete_selected_object
                        .after(placement::select_placed_object)
                        .run_if(|state: Res<state::TileEditorState>| state.open),
                    placement::draw_placement_grid
                        .run_if(|state: Res<state::TileEditorState>| {
                            state.open && state.mode == state::EditorMode::Place
                        }),
                    placement::draw_selection_gizmo
                        .run_if(|state: Res<state::TileEditorState>| state.open),
                    placement::respawn_edited_children
                        .after(editor_ui)
                        .run_if(|state: Res<state::TileEditorState>| {
                            state.pending_respawn_sidecar_id.is_some()
                        }),
                    placement::respawn_by_sprite_key
                        .after(editor_ui)
                        .run_if(|state: Res<state::TileEditorState>| {
                            state.pending_respawn_sprite_key.is_some()
                        }),
                    placement::delete_pending_object
                        .after(editor_ui)
                        .run_if(|state: Res<state::TileEditorState>| {
                            state.pending_delete_sidecar_id.is_some()
                        }),
                    live_preview::live_preview_system
                        .after(editor_ui)
                        .run_if(|state: Res<state::TileEditorState>| {
                            state.open && state.mode == state::EditorMode::Properties
                        }),
                ),
            );
        }
    }
}

#[cfg(feature = "dev_tools")]
fn toggle_editor(keyboard: Res<ButtonInput<KeyCode>>, mut state: ResMut<state::TileEditorState>) {
    if keyboard.just_pressed(KeyCode::F6) {
        state.open = !state.open;
        if state.open && !state.tilesets_scanned {
            tileset_browser::scan_tilesets(&mut state);
        }
    }
}

#[cfg(feature = "dev_tools")]
fn editor_ui(
    mut contexts: bevy_egui::EguiContexts,
    mut state: ResMut<state::TileEditorState>,
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    current_map: Res<crate::map::loader::CurrentMap>,
    particle_registry: Res<crate::particles::definitions::ParticleRegistry>,
) {
    use bevy_egui::egui;
    use state::EditorMode;

    // Load atlas textures before borrowing ctx (needs &mut contexts)
    if let Some(ts_idx) = state.selected_tileset {
        let ts = &mut state.tilesets[ts_idx];
        if ts.atlas_handle.is_none() {
            let handle: Handle<Image> = asset_server.load(&ts.image_path);
            let tex_id =
                contexts.add_image(bevy_egui::EguiTextureHandle::Strong(handle.clone()));
            ts.atlas_handle = Some(handle);
            ts.egui_texture = Some(tex_id);
        }
    }

    // Register library sprite textures (must happen before borrowing ctx)
    {
        let needs_tex: Vec<usize> = state
            .library_objects
            .iter()
            .enumerate()
            .filter(|(_, obj)| obj.sprite_texture.is_none() && obj.sprite_handle.is_none())
            .map(|(i, _)| i)
            .collect();
        for idx in needs_tex {
            let obj = &mut state.library_objects[idx];
            let qoi_path = obj.dir.join("sprite.qoi");
            let png_path = obj.dir.join("sprite.png");
            let sprite_path = if qoi_path.exists() { qoi_path } else { png_path };
            let asset_path = sprite_path
                .strip_prefix("assets/")
                .or_else(|_| sprite_path.strip_prefix("assets"))
                .unwrap_or(&sprite_path)
                .to_path_buf();
            let handle: Handle<Image> = asset_server.load(asset_path);
            let id = contexts.add_image(bevy_egui::EguiTextureHandle::Strong(handle.clone()));
            obj.sprite_handle = Some(handle);
            obj.sprite_texture = Some(id);
        }
    }

    // Handle pending compose image: add to Assets<Image> and register with egui
    if let Some(compose_img) = state.pending_compose_image.take() {
        let handle = images.add(compose_img);
        let tex_id = contexts.add_image(bevy_egui::EguiTextureHandle::Strong(handle.clone()));
        if let Some(obj) = state.current_object.as_mut() {
            obj.image_handle = handle;
            obj.egui_texture = Some(tex_id);
        }
    }

    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    egui::Window::new("Tile Editor")
        .default_width(500.0)
        .default_height(700.0)
        .show(ctx, |ui| {
            // ── Mode tabs ──────────────────────────────────────────
            ui.horizontal(|ui| {
                let mode = &mut state.mode;
                ui.selectable_value(mode, EditorMode::Browse, "Browse");
                ui.selectable_value(mode, EditorMode::Properties, "Properties");
                ui.selectable_value(mode, EditorMode::Place, "Place");
                ui.selectable_value(mode, EditorMode::Door, "Door");
                ui.selectable_value(mode, EditorMode::Library, "Library");
            });

            ui.separator();

            match state.mode {
                EditorMode::Browse => {
                    browse_mode_ui(ui, &mut state, &asset_server, &images, &keyboard);
                }
                EditorMode::Properties => {
                    let def_ids: Vec<String> = particle_registry.defs.keys().cloned().collect();
                    properties_mode_ui(ui, &mut state, &images, &current_map, &def_ids);
                }
                EditorMode::Place => {
                    place_mode_ui(ui, &mut state, &current_map);
                }
                EditorMode::Door => {
                    door_mode_ui(ui, &mut state);
                }
                EditorMode::Library => {
                    let def_ids: Vec<String> = particle_registry.defs.keys().cloned().collect();
                    library::library_mode_ui(ui, &mut state, &images, &current_map, &def_ids);
                }
            }
        });
}

#[cfg(feature = "dev_tools")]
fn place_mode_ui(
    ui: &mut bevy_egui::egui::Ui,
    state: &mut state::TileEditorState,
    current_map: &crate::map::loader::CurrentMap,
) {
    use bevy_egui::egui;

    ui.heading("Placement");

    if state.current_object.is_some() {
        ui.label("Left-click to place. Right-click to select. Del to remove.");
    } else {
        ui.label("Compose an object in Browse mode first.");
    }

    // Show selected object info
    if let Some(_selected) = state.selected_placed {
        ui.separator();

        // Find the placed object index matching the selected sidecar_id
        let placed_idx = state.selected_sidecar_id.as_ref().and_then(|sid| {
            state.placed_objects.iter().position(|o| o.id == *sid)
        });

        if let Some(idx) = placed_idx {
            let obj = &state.placed_objects[idx];
            ui.colored_label(
                egui::Color32::YELLOW,
                format!("#{} {} at ({},{})", obj.id, obj.sprite_key, obj.grid_pos[0], obj.grid_pos[1]),
            );
            ui.label(format!(
                "Lights: {}  Emitters: {}  Colliders: {}",
                obj.properties.lights.len(),
                obj.properties.emitters.len(),
                obj.collision_rects.len(),
            ));

            // Instance name field
            ui.horizontal(|ui| {
                ui.label("Name:");
                let mut name_buf = state.placed_objects[idx].name.clone().unwrap_or_default();
                if ui.add(egui::TextEdit::singleline(&mut name_buf).desired_width(150.0).hint_text("optional instance name")).changed() {
                    let new_name = if name_buf.is_empty() { None } else { Some(name_buf) };
                    state.placed_objects[idx].name = new_name;
                    // Auto-save sidecar
                    if let Some(map_path) = &current_map.path {
                        let file = sidecar::MapObjectsFile {
                            version: 1,
                            objects: state.placed_objects.clone(),
                        };
                        let _ = sidecar::save_sidecar(map_path, &file);
                    }
                }
            });
        } else {
            ui.colored_label(egui::Color32::YELLOW, "Selected (unresolved)");
        }

        let mut edit_action = false;
        ui.horizontal(|ui| {
            if ui.button("Edit Properties").on_hover_text("Open in Properties editor").clicked() {
                edit_action = true;
            }
            ui.add_enabled_ui(false, |ui| {
                ui.button("Attach Script").on_hover_text("Coming soon — attach Lua event scripts to this instance");
            });
        });

        if edit_action {
            if let Some(idx) = placed_idx {
                load_placed_for_editing(state, idx);
            }
        }
    }

    ui.separator();

    // ── Placed objects list ───────────────────────────────────────
    ui.heading(format!("Placed Objects ({})", state.placed_objects.len()));

    ui.horizontal(|ui| {
        ui.label("Filter:");
        ui.text_edit_singleline(&mut state.placed_search);
    });

    let search = state.placed_search.to_lowercase();

    // Collect matching indices first to avoid borrow issues
    let matching: Vec<usize> = state
        .placed_objects
        .iter()
        .enumerate()
        .filter(|(_, o)| {
            search.is_empty()
                || o.sprite_key.to_lowercase().contains(&search)
                || o.tileset.to_lowercase().contains(&search)
                || o.id.contains(&search)
        })
        .map(|(i, _)| i)
        .collect();

    if matching.is_empty() {
        ui.label("No matching objects.");
    }

    let mut action: Option<PlacedObjectAction> = None;

    egui::ScrollArea::vertical()
        .max_height(400.0)
        .id_salt("placed_objects_list")
        .show(ui, |ui| {
            for &idx in &matching {
                let obj = &state.placed_objects[idx];
                let is_editing = state.editing_placed_idx == Some(idx);

                ui.push_id(format!("placed_{}", obj.id), |ui| {
                    let frame_color = if is_editing {
                        egui::Color32::from_rgba_unmultiplied(80, 80, 20, 40)
                    } else {
                        egui::Color32::TRANSPARENT
                    };
                    egui::Frame::NONE
                        .fill(frame_color)
                        .inner_margin(4.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let name_display = obj.name.as_deref().unwrap_or("");
                                if name_display.is_empty() {
                                    ui.label(format!(
                                        "#{} {} ({},{})",
                                        obj.id, obj.sprite_key, obj.grid_pos[0], obj.grid_pos[1],
                                    ));
                                } else {
                                    ui.label(format!(
                                        "#{} \"{}\" ({},{})",
                                        obj.id, name_display, obj.grid_pos[0], obj.grid_pos[1],
                                    ));
                                }
                            });
                            ui.horizontal(|ui| {
                                if ui.small_button("Select").on_hover_text("Highlight in world").clicked() {
                                    action = Some(PlacedObjectAction::Select(obj.id.clone()));
                                }
                                if ui.small_button("Edit").on_hover_text("Load into Properties editor").clicked() {
                                    action = Some(PlacedObjectAction::Edit(idx));
                                }
                                if ui.small_button("Delete").clicked() {
                                    action = Some(PlacedObjectAction::Delete(idx));
                                }
                            });
                        });
                    ui.separator();
                });
            }
        });

    // Process action
    match action {
        Some(PlacedObjectAction::Select(id)) => {
            state.pending_select_sidecar_id = Some(id);
        }
        Some(PlacedObjectAction::Edit(idx)) => {
            load_placed_for_editing(state, idx);
        }
        Some(PlacedObjectAction::Delete(idx)) => {
            let removed = state.placed_objects.remove(idx);
            // Clear editing index if it was this one
            if state.editing_placed_idx == Some(idx) {
                state.editing_placed_idx = None;
                state.current_object = None;
            } else if let Some(ref mut ei) = state.editing_placed_idx {
                if *ei > idx {
                    *ei -= 1;
                }
            }
            // Queue ECS entity + children for deletion next frame
            state.pending_delete_sidecar_id = Some(removed.id.clone());
            // Clear selection if it was this object
            if state.selected_sidecar_id.as_ref() == Some(&removed.id) {
                state.selected_placed = None;
                state.selected_sidecar_id = None;
            }
            // Auto-save sidecar
            if let Some(map_path) = &current_map.path {
                let file = sidecar::MapObjectsFile {
                    version: 1,
                    objects: state.placed_objects.clone(),
                };
                let _ = sidecar::save_sidecar(map_path, &file);
            }
            info!("Deleted placed object {} from sidecar", removed.id);
        }
        None => {}
    }
}

#[cfg(feature = "dev_tools")]
enum PlacedObjectAction {
    Select(String),
    Edit(usize),
    Delete(usize),
}

/// Load a placed object from the sidecar into the editor for property editing.
#[cfg(feature = "dev_tools")]
fn load_placed_for_editing(state: &mut state::TileEditorState, idx: usize) {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    let def = &state.placed_objects[idx];
    let ts_name = &def.tileset;
    let sprite_key = &def.sprite_key;

    // Find the sprite file on disk
    let qoi_path = format!("assets/objects/{ts_name}/{sprite_key}/sprite.qoi");
    let png_path = format!("assets/objects/{ts_name}/{sprite_key}/sprite.png");
    let disk_path = if std::path::Path::new(&qoi_path).exists() {
        qoi_path
    } else if std::path::Path::new(&png_path).exists() {
        png_path
    } else {
        warn!("No sprite found for {sprite_key} — cannot edit");
        return;
    };

    // Decode image from disk
    let Ok(dyn_img) = image::open(&disk_path) else {
        warn!("Failed to load sprite image: {disk_path}");
        return;
    };
    let rgba = dyn_img.to_rgba8();
    let (w, h) = rgba.dimensions();

    let bevy_image = Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba.into_raw(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );

    let composed = state::ComposedObject {
        sprite_key: def.sprite_key.clone(),
        tileset_name: def.tileset.clone(),
        tile_ids: def.tile_ids.clone(),
        width_px: w,
        height_px: h,
        image_handle: Handle::default(),
        #[cfg(feature = "dev_tools")]
        egui_texture: None,
        properties: def.properties.clone(),
        collision_rects: def.collision_rects.clone(),
    };

    state.current_object = Some(composed);
    state.pending_compose_image = Some(bevy_image);
    state.editing_placed_idx = Some(idx);
    state.mode = state::EditorMode::Properties;

    info!("Loaded placed object #{} ({sprite_key}) for editing", def.id);
}

#[cfg(feature = "dev_tools")]
fn door_mode_ui(
    ui: &mut bevy_egui::egui::Ui,
    state: &mut state::TileEditorState,
) {
    ui.heading("Door/Portal Placement");

    let Some(obj) = state.current_object.as_ref() else {
        ui.label("Compose an object in Browse mode first, then configure it as a door here.");
        return;
    };

    ui.label(format!("Object: {}", obj.sprite_key));
    ui.separator();

    // Door properties are stored in a temporary state for the next placement
    // They'll be attached when the user places the object
    ui.label("Configure door properties for the next placement:");

    // We store door config in a static-like local. Since egui is immediate mode,
    // use the state's sidecar_path field area or add door config to state.
    // For now, show the UI and explain the workflow.

    ui.horizontal(|ui| {
        ui.label("Target map:");
        // Display a text field (stored temporarily)
    });

    ui.label("To place a door:");
    ui.label("1. Browse and compose a door sprite");
    ui.label("2. Switch to Place mode");
    ui.label("3. Place the object on the grid");
    ui.label("4. Edit the sidecar JSON to add door data");
    ui.label("");
    ui.label("Full door UI editor coming soon.");
    ui.label("For now, add door data directly to the .objects.json:");

    ui.code(r#"{
  "door": {
    "target_map": "maps/other.tmx",
    "spawn_point": [5, 10],
    "script": null
  }
}"#);
}

#[cfg(feature = "dev_tools")]
fn browse_mode_ui(
    ui: &mut bevy_egui::egui::Ui,
    state: &mut state::TileEditorState,
    _asset_server: &AssetServer,
    images: &Assets<Image>,
    keyboard: &ButtonInput<KeyCode>,
) {
    use bevy_egui::egui;

    // ── Tileset selector ──
    ui.horizontal(|ui| {
        ui.label("Tileset:");
        let selected_name = state
            .selected_tileset
            .and_then(|i| state.tilesets.get(i))
            .map(|ts| ts.name.clone())
            .unwrap_or_else(|| "Select...".to_string());

        egui::ComboBox::from_id_salt("tileset_combo")
            .width(200.0)
            .selected_text(&selected_name)
            .show_ui(ui, |ui| {
                let search = state.tileset_search.to_lowercase();
                for (idx, ts) in state.tilesets.iter().enumerate() {
                    if !search.is_empty() && !ts.name.to_lowercase().contains(&search) {
                        continue;
                    }
                    if ui
                        .selectable_label(state.selected_tileset == Some(idx), &ts.name)
                        .clicked()
                    {
                        state.selected_tileset = Some(idx);
                        state.selected_tiles.clear();
                    }
                }
            });
    });

    ui.horizontal(|ui| {
        ui.label("Search:");
        ui.text_edit_singleline(&mut state.tileset_search);
    });

    let Some(ts_idx) = state.selected_tileset else {
        ui.label("Select a tileset to browse tiles.");
        return;
    };

    // Extract tileset info before entering closures that mutate state
    let (ts_has_atlas, ts_tex_id, ts_tile_w, ts_tile_h, ts_cols, ts_tile_count) = {
        let ts = &state.tilesets[ts_idx];
        (
            ts.atlas_handle.is_some(),
            ts.egui_texture,
            ts.tile_width as f32,
            ts.tile_height as f32,
            ts.columns,
            ts.tile_count,
        )
    };

    if !ts_has_atlas {
        ui.label("Loading atlas...");
        return;
    }
    let Some(tex_id) = ts_tex_id else {
        ui.label("Loading atlas texture...");
        return;
    };

    let tile_w = ts_tile_w;
    let tile_h = ts_tile_h;
    let cols = ts_cols;
    let rows = (ts_tile_count + cols - 1) / cols;
    let atlas_w = cols as f32 * tile_w;
    let atlas_h = rows as f32 * tile_h;

    let selected_count = state
        .selected_tiles
        .iter()
        .filter(|t| t.tileset_idx == ts_idx)
        .count();
    ui.label(format!(
        "{selected_count} tile(s) selected — Shift+click to multi-select"
    ));

    // ── Tile grid ──
    let thumb_size = 40.0_f32;
    let grid_height = (rows as f32 * (thumb_size + 2.0)).min(400.0).max(60.0);

    let shift_held =
        keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);

    egui::ScrollArea::vertical()
        .max_height(grid_height)
        .id_salt("tile_grid")
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for tile_id in 0..ts_tile_count {
                    let col = tile_id % cols;
                    let row = tile_id / cols;

                    let u_min = col as f32 * tile_w / atlas_w;
                    let v_min = row as f32 * tile_h / atlas_h;
                    let u_max = (col as f32 + 1.0) * tile_w / atlas_w;
                    let v_max = (row as f32 + 1.0) * tile_h / atlas_h;

                    let is_selected = state.selected_tiles.iter().any(|t| {
                        t.tileset_idx == ts_idx && t.tile_id == tile_id
                    });

                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(thumb_size, thumb_size),
                        egui::Sense::click(),
                    );

                    let tint = if is_selected {
                        egui::Color32::WHITE
                    } else {
                        egui::Color32::from_gray(180)
                    };
                    ui.painter().image(
                        tex_id,
                        rect,
                        egui::Rect::from_min_max(
                            egui::pos2(u_min, v_min),
                            egui::pos2(u_max, v_max),
                        ),
                        tint,
                    );

                    if is_selected {
                        ui.painter().rect_stroke(
                            rect,
                            1.0,
                            egui::Stroke::new(2.0, egui::Color32::YELLOW),
                            egui::StrokeKind::Outside,
                        );
                    }

                    let resp = resp.on_hover_text(format!("Tile #{tile_id}"));

                    if resp.clicked() {
                        let sel = state::SelectedTile {
                            tileset_idx: ts_idx,
                            tile_id,
                        };
                        if shift_held {
                            // Toggle in multi-select
                            if let Some(pos) = state.selected_tiles.iter().position(|t| {
                                t.tileset_idx == ts_idx && t.tile_id == tile_id
                            }) {
                                state.selected_tiles.remove(pos);
                                // Also remove from assembly
                                state.assembly.retain(|s| {
                                    !(s.tileset_idx == ts_idx && s.tile_id == tile_id)
                                });
                            } else {
                                state.selected_tiles.push(sel);
                                // Auto-add to assembly at next available slot
                                auto_add_to_assembly(state, ts_idx, tile_id);
                            }
                        } else {
                            state.selected_tiles.clear();
                            state.assembly.clear();
                            state.selected_tiles.push(sel);
                            state.assembly.push(state::AssemblySlot {
                                col: 0,
                                row: 0,
                                tileset_idx: ts_idx,
                                tile_id,
                                blank: false,
                            });
                            state.assembly_cols = 1;
                        }
                        state.current_object = None;
                    }
                }
            });
        });

    ui.separator();

    // ── Assembly grid ─────────────────────────────────────────────
    if !state.assembly.is_empty() {
        ui.heading("Assembly");

        // Grid dimensions control
        ui.horizontal(|ui| {
            ui.label("Columns:");
            let prev_cols = state.assembly_cols;
            ui.add(egui::DragValue::new(&mut state.assembly_cols).range(1..=16).speed(0.1));
            if state.assembly_cols != prev_cols {
                reflow_assembly(state);
                state.current_object = None;
            }
        });

        let asm_thumb = 48.0_f32;
        let asm_cols = state.assembly_cols;

        // Collect tile rendering data (avoid borrow conflict with tilesets)
        struct AsmTileVis {
            tex_id: bevy_egui::egui::TextureId,
            uv_min: [f32; 2],
            uv_max: [f32; 2],
            tile_id: u32,
        }
        enum AsmSlotVis {
            Tile(AsmTileVis),
            Blank,
            Missing,
        }
        let tile_vis: Vec<AsmSlotVis> = state
            .assembly
            .iter()
            .map(|slot| {
                if slot.blank {
                    return AsmSlotVis::Blank;
                }
                let Some(ts) = state.tilesets.get(slot.tileset_idx) else {
                    return AsmSlotVis::Missing;
                };
                let Some(tex_id) = ts.egui_texture else {
                    return AsmSlotVis::Missing;
                };
                let tw = ts.tile_width as f32;
                let th = ts.tile_height as f32;
                let aw = ts.columns as f32 * tw;
                let ah = ((ts.tile_count + ts.columns - 1) / ts.columns) as f32 * th;
                let c = slot.tile_id % ts.columns;
                let r = slot.tile_id / ts.columns;
                AsmSlotVis::Tile(AsmTileVis {
                    tex_id,
                    uv_min: [c as f32 * tw / aw, r as f32 * th / ah],
                    uv_max: [(c as f32 + 1.0) * tw / aw, (r as f32 + 1.0) * th / ah],
                    tile_id: slot.tile_id,
                })
            })
            .collect();

        // Track which tile to swap/remove
        let mut swap_action: Option<(usize, usize)> = None;
        let mut remove_idx: Option<usize> = None;

        egui::Grid::new("assembly_grid")
            .spacing(egui::vec2(4.0, 4.0))
            .show(ui, |ui| {
                for (slot_idx, vis) in tile_vis.iter().enumerate() {
                    ui.vertical(|ui| {
                        let (rect, _resp) = ui.allocate_exact_size(
                            egui::vec2(asm_thumb, asm_thumb),
                            egui::Sense::hover(),
                        );
                        match vis {
                            AsmSlotVis::Tile(tv) => {
                                ui.painter().image(
                                    tv.tex_id,
                                    rect,
                                    egui::Rect::from_min_max(
                                        egui::pos2(tv.uv_min[0], tv.uv_min[1]),
                                        egui::pos2(tv.uv_max[0], tv.uv_max[1]),
                                    ),
                                    egui::Color32::WHITE,
                                );
                                ui.painter().rect_stroke(
                                    rect,
                                    1.0,
                                    egui::Stroke::new(1.0, egui::Color32::from_gray(100)),
                                    egui::StrokeKind::Outside,
                                );
                                ui.painter().text(
                                    egui::pos2(rect.min.x + 2.0, rect.min.y + 2.0),
                                    egui::Align2::LEFT_TOP,
                                    format!("#{}", tv.tile_id),
                                    egui::FontId::proportional(9.0),
                                    egui::Color32::from_gray(200),
                                );
                            }
                            AsmSlotVis::Blank => {
                                // Checkerboard pattern for blank tiles
                                let check = 8.0;
                                let c1 = egui::Color32::from_gray(40);
                                let c2 = egui::Color32::from_gray(60);
                                let cols_n = (asm_thumb / check).ceil() as i32;
                                for cy in 0..cols_n {
                                    for cx in 0..cols_n {
                                        let color = if (cx + cy) % 2 == 0 { c1 } else { c2 };
                                        let r = egui::Rect::from_min_size(
                                            egui::pos2(rect.min.x + cx as f32 * check, rect.min.y + cy as f32 * check),
                                            egui::vec2(check, check),
                                        ).intersect(rect);
                                        ui.painter().rect_filled(r, 0.0, color);
                                    }
                                }
                                ui.painter().rect_stroke(
                                    rect,
                                    1.0,
                                    egui::Stroke::new(1.0, egui::Color32::from_gray(70)),
                                    egui::StrokeKind::Outside,
                                );
                            }
                            AsmSlotVis::Missing => {
                                ui.painter().rect_filled(rect, 1.0, egui::Color32::from_gray(30));
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "?",
                                    egui::FontId::proportional(16.0),
                                    egui::Color32::RED,
                                );
                            }
                        }
                        // Buttons row
                        ui.horizontal(|ui| {
                            ui.spacing_mut().button_padding = egui::vec2(2.0, 0.0);
                            if slot_idx > 0 {
                                if ui.small_button("\u{2190}").on_hover_text("Move earlier").clicked() {
                                    swap_action = Some((slot_idx, slot_idx - 1));
                                }
                            }
                            if slot_idx + 1 < tile_vis.len() {
                                if ui.small_button("\u{2192}").on_hover_text("Move later").clicked() {
                                    swap_action = Some((slot_idx, slot_idx + 1));
                                }
                            }
                            if ui.small_button("x").on_hover_text("Remove").clicked() {
                                remove_idx = Some(slot_idx);
                            }
                        });
                    });

                    // New row after every `asm_cols` tiles
                    if (slot_idx as u32 + 1) % asm_cols == 0 {
                        ui.end_row();
                    }
                }
            });

        // Apply swap
        if let Some((a, b)) = swap_action {
            state.assembly.swap(a, b);
            reflow_assembly(state);
            state.current_object = None;
        }

        // Apply remove
        if let Some(idx) = remove_idx {
            let removed = state.assembly.remove(idx);
            if !removed.blank {
                state.selected_tiles.retain(|t| {
                    !(t.tileset_idx == removed.tileset_idx && t.tile_id == removed.tile_id)
                });
            }
            reflow_assembly(state);
            state.current_object = None;
        }

        ui.horizontal(|ui| {
            if ui.button("+ Blank").on_hover_text("Add a transparent gap tile").clicked() {
                let cols = state.assembly_cols;
                let next_idx = state.assembly.len() as u32;
                state.assembly.push(state::AssemblySlot {
                    col: next_idx % cols,
                    row: next_idx / cols,
                    tileset_idx: 0,
                    tile_id: 0,
                    blank: true,
                });
                state.current_object = None;
            }
            if ui.button("Reverse rows").on_hover_text("Flip assembly vertically").clicked() {
                let cols = state.assembly_cols as usize;
                let num_rows = (state.assembly.len() + cols - 1) / cols;
                let mut new_asm = Vec::with_capacity(state.assembly.len());
                for row in (0..num_rows).rev() {
                    let start = row * cols;
                    let end = (start + cols).min(state.assembly.len());
                    new_asm.extend_from_slice(&state.assembly[start..end]);
                }
                state.assembly = new_asm;
                reflow_assembly(state);
                state.current_object = None;
            }
            if ui.button("Clear assembly").clicked() {
                state.assembly.clear();
                state.selected_tiles.clear();
                state.current_object = None;
            }
        });

        ui.separator();
    }

    // ── Compose button ──
    ui.horizontal(|ui| {
        if !state.assembly.is_empty() {
            if ui.button("Compose Object").clicked() {
                if let Some((composed, image)) = composition::compose_from_assembly(state, images) {
                    info!(
                        "Composed object: {} ({}x{})",
                        composed.sprite_key, composed.width_px, composed.height_px
                    );
                    state.current_object = Some(composed);
                    // Defer image registration to next frame (needs &mut EguiContexts)
                    state.pending_compose_image = Some(image);
                }
            }
        }
        if state.current_object.is_some() {
            ui.label(format!(
                "Current: {}",
                state
                    .current_object
                    .as_ref()
                    .map(|o| o.sprite_key.as_str())
                    .unwrap_or("none")
            ));
        }
        if !state.assembly.is_empty() && ui.button("Clear").clicked() {
            state.selected_tiles.clear();
            state.assembly.clear();
            state.current_object = None;
        }
    });
}

/// Auto-add a tile to the assembly grid at the next available position.
#[cfg(feature = "dev_tools")]
fn auto_add_to_assembly(state: &mut state::TileEditorState, tileset_idx: usize, tile_id: u32) {
    let cols = state.assembly_cols;
    let next_idx = state.assembly.len() as u32;
    let col = next_idx % cols;
    let row = next_idx / cols;
    state.assembly.push(state::AssemblySlot {
        col,
        row,
        tileset_idx,
        tile_id,
        blank: false,
    });
}

/// Reflow assembly positions after reordering or column count change.
#[cfg(feature = "dev_tools")]
fn reflow_assembly(state: &mut state::TileEditorState) {
    let cols = state.assembly_cols;
    for (i, slot) in state.assembly.iter_mut().enumerate() {
        slot.col = i as u32 % cols;
        slot.row = i as u32 / cols;
    }
}

#[cfg(feature = "dev_tools")]
fn properties_mode_ui(
    ui: &mut bevy_egui::egui::Ui,
    state: &mut state::TileEditorState,
    _images: &Assets<Image>,
    current_map: &crate::map::loader::CurrentMap,
    particle_def_ids: &[String],
) {
    use bevy_egui::egui;

    let Some(obj) = state.current_object.as_mut() else {
        ui.label("No object composed. Go to Browse mode and compose tiles first.");
        return;
    };

    ui.heading(format!("Object: {}", obj.sprite_key));
    ui.label(format!("Size: {}x{} px", obj.width_px, obj.height_px));

    ui.separator();

    // ── Sprite preview with overlays ──
    if let Some(tex_id) = obj.egui_texture {
        let max_dim = 250.0_f32;
        let img_w = obj.width_px as f32;
        let img_h = obj.height_px as f32;
        let scale = (max_dim / img_w).min(max_dim / img_h);
        let preview_w = img_w * scale;
        let preview_h = img_h * scale;

        let (rect, _resp) = ui.allocate_exact_size(
            egui::vec2(preview_w, preview_h),
            egui::Sense::click_and_drag(),
        );

        // Draw sprite
        ui.painter().image(
            tex_id,
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        ui.painter().rect_stroke(
            rect,
            1.0,
            egui::Stroke::new(1.0, egui::Color32::from_gray(80)),
            egui::StrokeKind::Outside,
        );

        // Light/emitter overlay dots
        lights_emitters::draw_preview_overlay(ui, rect, obj, scale);

        // Collision rect overlay + drawing
        let mut draw_state = collision_editor::CollisionDrawState {
            drawing: state.collision_drawing,
            draw_start: state.collision_draw_start.map(|p| egui::pos2(p[0], p[1])),
        };
        if collision_editor::draw_collision_overlay(
            ui, rect, img_w, img_h, obj, &mut draw_state,
        ) {
            state.dirty = true;
        }
        state.collision_drawing = draw_state.drawing;
        state.collision_draw_start = draw_state.draw_start.map(|p| [p.x, p.y]);

        ui.add_space(4.0);
    }

    ui.separator();

    // ── Blend height ──
    ui.horizontal(|ui| {
        ui.label("Blend height:");
        if ui
            .add(egui::Slider::new(&mut obj.properties.blend_height, 0.0..=48.0).suffix(" px"))
            .changed()
        {
            state.dirty = true;
        }
    });

    ui.separator();

    // ── Lights & Emitters ──
    if lights_emitters::lights_emitters_ui(ui, obj, particle_def_ids) {
        state.dirty = true;
    }

    ui.separator();

    // ── Collision Rects ──
    let mut draw_state = collision_editor::CollisionDrawState {
        drawing: state.collision_drawing,
        draw_start: state.collision_draw_start.map(|p| egui::pos2(p[0], p[1])),
    };
    if collision_editor::collision_editor_ui(ui, obj, &mut draw_state) {
        state.dirty = true;
    }
    state.collision_drawing = draw_state.drawing;
    state.collision_draw_start = draw_state.draw_start.map(|p| [p.x, p.y]);

    ui.separator();

    // ── Editing context label ──
    if let Some(edit_idx) = state.editing_placed_idx {
        if let Some(placed) = state.placed_objects.get(edit_idx) {
            ui.colored_label(
                egui::Color32::YELLOW,
                format!("Editing instance #{} at ({},{}) — saves to this instance only", placed.id, placed.grid_pos[0], placed.grid_pos[1]),
            );
        }
    } else {
        ui.colored_label(
            egui::Color32::from_rgb(100, 200, 255),
            "Editing root object — saves to all instances",
        );
    }

    // ── Save ──
    let save_label = if state.dirty { "Save *" } else { "Save" };
    if ui.button(save_label).clicked() {
        if let Some(edit_idx) = state.editing_placed_idx {
            // Per-instance edit — only update this sidecar entry, NOT properties.json
            let sidecar_id = state.placed_objects.get(edit_idx).map(|p| p.id.clone());
            if let Some(placed) = state.placed_objects.get_mut(edit_idx) {
                placed.properties = obj.properties.clone();
                placed.collision_rects = obj.collision_rects.clone();
            }
            if let Some(map_path) = &current_map.path {
                let file = sidecar::MapObjectsFile {
                    version: 1,
                    objects: state.placed_objects.clone(),
                };
                if let Err(e) = sidecar::save_sidecar(map_path, &file) {
                    error!("Failed to save sidecar: {e}");
                }
            }
            state.dirty = false;
            info!("Saved instance #{}", sidecar_id.as_deref().unwrap_or("?"));
            // Trigger respawn of lights/emitters in the world
            state.pending_respawn_sidecar_id = sidecar_id;
        } else {
            // Editing root object — save properties.json and update ALL instances
            let dir = std::path::Path::new("assets/objects")
                .join(&obj.tileset_name)
                .join(&obj.sprite_key);
            let _ = std::fs::create_dir_all(&dir);
            let props_path = dir.join("properties.json");
            match serde_json::to_string_pretty(&obj.properties) {
                Ok(json) => match std::fs::write(&props_path, &json) {
                    Ok(()) => info!("Saved properties to {}", props_path.display()),
                    Err(e) => error!("Failed to save: {e}"),
                },
                Err(e) => error!("Failed to serialize: {e}"),
            }

            let sprite_key = obj.sprite_key.clone();
            let new_props = obj.properties.clone();
            let new_collisions = obj.collision_rects.clone();

            // Update the library entry so it stays in sync
            if let Some(lib_obj) = state.library_objects.iter_mut().find(|l| l.key == sprite_key) {
                lib_obj.properties = new_props.clone();
            }
            let mut any_updated = false;
            for placed in &mut state.placed_objects {
                if placed.sprite_key == sprite_key {
                    placed.properties = new_props.clone();
                    placed.collision_rects = new_collisions.clone();
                    any_updated = true;
                }
            }
            if any_updated {
                if let Some(map_path) = &current_map.path {
                    let file = sidecar::MapObjectsFile {
                        version: 1,
                        objects: state.placed_objects.clone(),
                    };
                    if let Err(e) = sidecar::save_sidecar(map_path, &file) {
                        error!("Failed to save sidecar: {e}");
                    }
                }
                // Trigger respawn of ALL instances with this sprite key
                state.pending_respawn_sprite_key = Some(sprite_key);
            }
            state.dirty = false;
        }
    }
}
