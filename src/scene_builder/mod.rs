pub mod state;

use std::collections::HashMap;

use bevy::prelude::*;

use crate::tile_editor::sidecar::PlacedObjectDef;

/// Get a reference name and display label for a placed object.
/// Named objects use their name; unnamed ones get a generated ref like "#3 (sprite_key)".
fn instance_label(obj: &PlacedObjectDef) -> (String, String) {
    if let Some(name) = &obj.name {
        (name.clone(), format!("{} ({},{})", name, obj.grid_pos[0], obj.grid_pos[1]))
    } else {
        let ref_name = format!("#{}", obj.id);
        let label = format!("#{} {} ({},{})", obj.id, obj.sprite_key, obj.grid_pos[0], obj.grid_pos[1]);
        (ref_name, label)
    }
}

pub struct SceneBuilderPlugin;

impl Plugin for SceneBuilderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<state::SceneBuilderState>()
            .add_systems(Update, auto_load_events);

        #[cfg(feature = "dev_tools")]
        {
            app.add_systems(
                Update,
                (
                    toggle_scene_builder,
                    scene_builder_ui
                        .after(toggle_scene_builder)
                        .run_if(|state: Res<state::SceneBuilderState>| state.open),
                    pick_position_from_world
                        .after(scene_builder_ui)
                        .run_if(|state: Res<state::SceneBuilderState>| {
                            state.open && state.picking_position.is_some()
                        }),
                    draw_pick_gizmos
                        .run_if(|state: Res<state::SceneBuilderState>| state.open),
                    drag_position_nodes
                        .after(scene_builder_ui)
                        .run_if(|state: Res<state::SceneBuilderState>| state.open),
                ),
            );
        }
    }
}

/// Auto-load events when a map is loaded, pushing them to the runner so
/// AutoRun/Parallel start and Interact/Touch/Script triggers are registered.
fn auto_load_events(
    mut state: ResMut<state::SceneBuilderState>,
    current_map: Res<crate::map::loader::CurrentMap>,
    mut scene_runner: ResMut<crate::scripting::runner::SceneRunner>,
) {
    let Some(map_path) = &current_map.path else {
        return;
    };
    // Only load once per map
    if state.loaded_for_map.as_ref() == Some(map_path) {
        return;
    }
    let file = crate::scripting::scene_event::load_events(map_path)
        .unwrap_or_default();
    state.events = file.events.clone();
    state.selected_event = None;
    state.loaded_for_map = Some(map_path.clone());
    state.dirty = false;

    // Push all events to runner (it starts AutoRun/Parallel and populates all_events)
    scene_runner.clear_auto_run();
    scene_runner.pending_start.extend(file.events);
}

#[cfg(feature = "dev_tools")]
fn toggle_scene_builder(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<state::SceneBuilderState>,
    current_map: Res<crate::map::loader::CurrentMap>,
) {
    if keyboard.just_pressed(KeyCode::F7) {
        state.open = !state.open;
        // Load events when opening if not yet loaded for this map
        if state.open {
            if let Some(map_path) = &current_map.path {
                if state.loaded_for_map.as_ref() != Some(map_path) {
                    let file = crate::scripting::scene_event::load_events(map_path)
                        .unwrap_or_default();
                    state.events = file.events;
                    state.selected_event = None;
                    state.loaded_for_map = Some(map_path.clone());
                    state.dirty = false;
                }
            }
        }
    }
}

#[cfg(feature = "dev_tools")]
fn scene_builder_ui(
    mut contexts: bevy_egui::EguiContexts,
    mut state: ResMut<state::SceneBuilderState>,
    current_map: Res<crate::map::loader::CurrentMap>,
    action_registry: Res<crate::scripting::scene_action::SceneActionRegistry>,
    tile_state: Res<crate::tile_editor::state::TileEditorState>,
    mut scene_runner: ResMut<crate::scripting::runner::SceneRunner>,
    yarn_project: Option<Res<bevy_yarnspinner::prelude::YarnProject>>,
) {
    use bevy_egui::egui;

    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    // Collect available yarn node names for the dropdown
    let yarn_nodes: Vec<String> = yarn_project
        .as_ref()
        .map(|p| {
            p.compilation()
                .program
                .as_ref()
                .map(|prog| prog.nodes.keys().cloned().collect())
                .unwrap_or_default()
        })
        .unwrap_or_default();

    egui::Window::new("Scene Builder")
        .default_width(900.0)
        .default_height(600.0)
        .resizable([true, true])
        .show(ctx, |ui| {
            let panel_height = ui.available_height();
            ui.horizontal(|ui| {
                // ── Left panel: Event list ──
                ui.vertical(|ui| {
                    ui.set_min_width(180.0);
                    ui.set_max_width(200.0);
                    ui.set_min_height(panel_height);
                    event_list_panel(ui, &mut state, &current_map);
                });

                ui.separator();

                // ── Center panel: Event editor ──
                ui.vertical(|ui| {
                    ui.set_min_width(300.0);
                    ui.set_max_width(400.0);
                    ui.set_min_height(panel_height);
                    event_editor_panel(ui, &mut state, &action_registry, &tile_state, &yarn_nodes, current_map.path.as_deref());
                });

                ui.separator();

                // ── Right panel: Lua preview ──
                ui.vertical(|ui| {
                    ui.set_min_width(250.0);
                    ui.set_min_height(panel_height);
                    lua_preview_panel(ui, &mut state, &current_map, &action_registry, &mut scene_runner);
                });
            });
        });
}

// ── Left panel: Event list ────────────────────────────────────────────────

#[cfg(feature = "dev_tools")]
fn event_list_panel(
    ui: &mut bevy_egui::egui::Ui,
    state: &mut state::SceneBuilderState,
    _current_map: &crate::map::loader::CurrentMap,
) {
    use bevy_egui::egui;
    use crate::scripting::scene_event;

    ui.heading("Events");

    ui.horizontal(|ui| {
        if ui.button("+ New").clicked() {
            let file = scene_event::MapEventsFile {
                version: 1,
                events: state.events.clone(),
            };
            let id = scene_event::next_event_id(&file);
            state.events.push(scene_event::SceneEvent {
                id: id.clone(),
                name: format!("event_{id}"),
                trigger: scene_event::EventTrigger::Interact,
                trigger_target: None,
                actions: Vec::new(),
                enabled: true,
            });
            state.selected_event = Some(state.events.len() - 1);
            state.dirty = true;
        }
    });

    ui.horizontal(|ui| {
        ui.label("Filter:");
        ui.text_edit_singleline(&mut state.event_search);
    });

    let search = state.event_search.to_lowercase();

    enum EventListAction {
        Select(usize),
        Delete(usize),
        Duplicate(usize),
    }
    let mut action: Option<EventListAction> = None;

    // Collect display data to avoid borrow conflicts
    let display: Vec<(usize, String, bool)> = state.events.iter().enumerate()
        .filter(|(_, e)| search.is_empty() || e.name.to_lowercase().contains(&search))
        .map(|(idx, e)| {
            let label = format!(
                "{} {} [{}]",
                if e.enabled { "●" } else { "○" },
                e.name,
                e.trigger.label(),
            );
            (idx, label, state.selected_event == Some(idx))
        })
        .collect();

    egui::ScrollArea::vertical()
        .max_height(ui.available_height() - 10.0)
        .id_salt("event_list")
        .show(ui, |ui| {
            for (idx, label, is_selected) in &display {
                let resp = ui.selectable_label(*is_selected, label);
                if resp.clicked() {
                    action = Some(EventListAction::Select(*idx));
                }
                resp.context_menu(|ui| {
                    if ui.button("Delete").clicked() {
                        action = Some(EventListAction::Delete(*idx));
                        ui.close();
                    }
                    if ui.button("Duplicate").clicked() {
                        action = Some(EventListAction::Duplicate(*idx));
                        ui.close();
                    }
                });
            }
        });

    match action {
        Some(EventListAction::Select(idx)) => {
            state.selected_event = Some(idx);
            state.picking_position = None;
            state.spline_init = None;
            state.dragging_node = None;
            state.line_popup_action = None;
        }
        Some(EventListAction::Delete(idx)) => {
            state.events.remove(idx);
            if state.selected_event == Some(idx) {
                state.selected_event = None;
            } else if let Some(ref mut sel) = state.selected_event {
                if *sel > idx { *sel -= 1; }
            }
            state.dirty = true;
        }
        Some(EventListAction::Duplicate(idx)) => {
            let mut dup = state.events[idx].clone();
            let file = scene_event::MapEventsFile {
                version: 1,
                events: state.events.clone(),
            };
            dup.id = scene_event::next_event_id(&file);
            dup.name = format!("{}_copy", dup.name);
            state.events.push(dup);
            state.dirty = true;
        }
        None => {}
    }
}

// ── Center panel: Event editor ────────────────────────────────────────────

#[cfg(feature = "dev_tools")]
fn event_editor_panel(
    ui: &mut bevy_egui::egui::Ui,
    state: &mut state::SceneBuilderState,
    registry: &crate::scripting::scene_action::SceneActionRegistry,
    tile_state: &crate::tile_editor::state::TileEditorState,
    yarn_nodes: &[String],
    current_map_path: Option<&str>,
) {
    use bevy_egui::egui;
    use crate::scripting::scene_event::*;
    use crate::scripting::scene_action::*;

    let Some(sel_idx) = state.selected_event else {
        ui.heading("Event Editor");
        ui.label("Select an event from the list.");
        return;
    };
    let Some(event) = state.events.get_mut(sel_idx) else {
        state.selected_event = None;
        return;
    };

    ui.heading("Event Editor");

    // Name
    ui.horizontal(|ui| {
        ui.label("Name:");
        if ui.text_edit_singleline(&mut event.name).changed() {
            state.dirty = true;
        }
    });

    // Enabled
    if ui.checkbox(&mut event.enabled, "Enabled").changed() {
        state.dirty = true;
    }

    // Trigger
    ui.horizontal(|ui| {
        ui.label("Trigger:");
        let prev = event.trigger.clone();
        egui::ComboBox::from_id_salt("event_trigger")
            .selected_text(event.trigger.label())
            .show_ui(ui, |ui| {
                for t in EventTrigger::all() {
                    if ui.selectable_label(event.trigger == *t, t.label()).clicked() {
                        event.trigger = t.clone();
                    }
                }
            });
        if event.trigger != prev {
            state.dirty = true;
        }
    });

    // Target instance (for Interact/Touch)
    if event.trigger.needs_target() {
        ui.horizontal(|ui| {
            ui.label("Target:");
            let current = event.trigger_target.clone().unwrap_or_default();
            let display = if current.is_empty() { "Select..." } else { &current };
            egui::ComboBox::from_id_salt("event_target")
                .selected_text(display)
                .show_ui(ui, |ui| {
                    for obj in &tile_state.placed_objects {
                        let (ref_name, label) = instance_label(obj);
                        if ui.selectable_label(event.trigger_target.as_deref() == Some(ref_name.as_str()), &label).clicked() {
                            event.trigger_target = Some(ref_name);
                            state.dirty = true;
                        }
                    }
                });
        });
    }

    ui.separator();

    // ── Action list ──
    ui.heading("Actions");

    let mut action_to_remove: Option<usize> = None;
    let mut swap_action: Option<(usize, usize)> = None;
    let mut copy_down_idx: Option<usize> = None;

    egui::ScrollArea::vertical()
        .max_height(ui.available_height() - 40.0)
        .id_salt("action_list")
        .show(ui, |ui| {
            let num_actions = event.actions.len();
            for ai in 0..num_actions {
                let action = &mut event.actions[ai];
                let def_label = registry.get(&action.action_id)
                    .map(|d| d.label)
                    .unwrap_or("???");

                ui.push_id(format!("action_{ai}"), |ui| {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(format!("{}.", ai + 1));
                            ui.strong(def_label);
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("x").on_hover_text("Remove").clicked() {
                                    action_to_remove = Some(ai);
                                }
                                if ui.small_button("+").on_hover_text("Copy down (duplicate with same args)").clicked() {
                                    copy_down_idx = Some(ai);
                                }
                                if ai + 1 < num_actions {
                                    if ui.small_button("v").on_hover_text("Move down").clicked() {
                                        swap_action = Some((ai, ai + 1));
                                    }
                                }
                                if ai > 0 {
                                    if ui.small_button("^").on_hover_text("Move up").clicked() {
                                        swap_action = Some((ai, ai - 1));
                                    }
                                }
                            });
                        });

                        // Argument widgets
                        if let Some(def) = registry.get(&action.action_id) {
                            for arg_def in &def.args {
                                if render_arg_widget(ui, ai, arg_def, &mut action.args, tile_state, &mut state.picking_position, yarn_nodes, current_map_path) {
                                    state.dirty = true;
                                }
                            }
                        }
                    });
                });
            }
        });

    if let Some(idx) = action_to_remove {
        event.actions.remove(idx);
        state.dirty = true;
    }
    if let Some(idx) = copy_down_idx {
        let dup = event.actions[idx].clone();
        event.actions.insert(idx + 1, dup);
        state.dirty = true;
    }
    if let Some((a, b)) = swap_action {
        event.actions.swap(a, b);
        state.dirty = true;
    }

    // ── Add action dropdown ──
    ui.horizontal(|ui| {
        ui.label("Add:");
        egui::ComboBox::from_id_salt("add_action_cat")
            .selected_text(state.add_action_category.label())
            .show_ui(ui, |ui| {
                for cat in ActionCategory::all() {
                    if ui.selectable_label(state.add_action_category == *cat, cat.label()).clicked() {
                        state.add_action_category = *cat;
                    }
                }
            });

        let actions_in_cat = registry.by_category(state.add_action_category);
        egui::ComboBox::from_id_salt("add_action_pick")
            .selected_text("Select action...")
            .show_ui(ui, |ui| {
                for def in &actions_in_cat {
                    if ui.selectable_label(false, def.label).on_hover_text(def.description).clicked() {
                        event.actions.push(EventAction {
                            action_id: def.id.to_string(),
                            args: HashMap::new(),
                        });
                        state.dirty = true;
                    }
                }
            });
    });
}

// ── Argument widget rendering ─────────────────────────────────────────────

#[cfg(feature = "dev_tools")]
fn render_arg_widget(
    ui: &mut bevy_egui::egui::Ui,
    action_idx: usize,
    arg_def: &crate::scripting::scene_action::ArgDef,
    args: &mut std::collections::HashMap<String, crate::scripting::scene_event::ActionArgValue>,
    tile_state: &crate::tile_editor::state::TileEditorState,
    picking_position: &mut Option<(usize, String, bool)>,
    yarn_nodes: &[String],
    current_map_path: Option<&str>,
) -> bool {
    use bevy_egui::egui;
    use crate::scripting::scene_action::ArgType;
    use crate::scripting::scene_event::ActionArgValue;

    let mut changed = false;
    let key = arg_def.name.to_string();

    ui.horizontal(|ui| {
        ui.label(format!("{}:", arg_def.label));

        match &arg_def.arg_type {
            ArgType::String => {
                let mut val = args.get(&key)
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_default();
                if ui.text_edit_singleline(&mut val).changed() {
                    args.insert(key, ActionArgValue::String(val));
                    changed = true;
                }
            }
            ArgType::Float { min, max, default } => {
                let mut val = args.get(&key).map(|v| v.as_f32()).unwrap_or(*default);
                if ui.add(egui::DragValue::new(&mut val).range(*min..=*max).speed(0.1)).changed() {
                    args.insert(key, ActionArgValue::Float(val as f64));
                    changed = true;
                }
            }
            ArgType::Int { min, max, default } => {
                let mut val = args.get(&key).map(|v| v.as_i32()).unwrap_or(*default);
                if ui.add(egui::DragValue::new(&mut val).range(*min..=*max).speed(1)).changed() {
                    args.insert(key, ActionArgValue::Int(val as i64));
                    changed = true;
                }
            }
            ArgType::Bool { default } => {
                let mut val = args.get(&key).map(|v| v.as_bool()).unwrap_or(*default);
                if ui.checkbox(&mut val, "").changed() {
                    args.insert(key, ActionArgValue::Bool(val));
                    changed = true;
                }
            }
            ArgType::InstanceRef => {
                let current = args.get(&key)
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_default();
                let display = if current.is_empty() { "Select..." } else { &current };
                egui::ComboBox::from_id_salt(format!("arg_{}", arg_def.name))
                    .selected_text(display)
                    .show_ui(ui, |ui| {
                        for obj in &tile_state.placed_objects {
                            let (ref_name, label) = instance_label(obj);
                            if ui.selectable_label(current == ref_name, &label).clicked() {
                                args.insert(key.clone(), ActionArgValue::String(ref_name));
                                changed = true;
                            }
                        }
                    });
            }
            ArgType::SubRef => {
                let current = args.get(&key)
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_default();
                let display = if current.is_empty() { "Select..." } else { &current };
                egui::ComboBox::from_id_salt(format!("arg_{}", arg_def.name))
                    .selected_text(display)
                    .show_ui(ui, |ui| {
                        for obj in &tile_state.placed_objects {
                            let (ref_name, _) = instance_label(obj);
                            for light in &obj.properties.lights {
                                let ref_str = format!("{}.{}", ref_name, light.ref_id);
                                if ui.selectable_label(current == ref_str, &ref_str).clicked() {
                                    args.insert(key.clone(), ActionArgValue::String(ref_str));
                                    changed = true;
                                }
                            }
                            for emitter in &obj.properties.emitters {
                                let ref_str = format!("{}.{}", ref_name, emitter.ref_id);
                                if ui.selectable_label(current == ref_str, &ref_str).clicked() {
                                    args.insert(key.clone(), ActionArgValue::String(ref_str));
                                    changed = true;
                                }
                            }
                        }
                    });
            }
            ArgType::Position => {
                let [mut x, mut y] = match args.get(&key) {
                    Some(ActionArgValue::Position(p)) => *p,
                    _ => [0.0, 0.0],
                };
                let mut pos_changed = false;
                if ui.add(egui::DragValue::new(&mut x).speed(1.0).prefix("X: ")).changed() { pos_changed = true; }
                if ui.add(egui::DragValue::new(&mut y).speed(1.0).prefix("Y: ")).changed() { pos_changed = true; }
                if pos_changed {
                    args.insert(key.clone(), ActionArgValue::Position([x, y]));
                    changed = true;
                }
                let is_picking = picking_position.as_ref()
                    .is_some_and(|(ai, name, _)| *ai == action_idx && name == arg_def.name);
                if is_picking {
                    ui.colored_label(egui::Color32::YELLOW, "Click map...");
                    if ui.small_button("Cancel").clicked() {
                        *picking_position = None;
                    }
                } else {
                    if ui.small_button("Grid").on_hover_text("Click map tile").clicked() {
                        *picking_position = Some((action_idx, key.clone(), true));
                    }
                    if ui.small_button("Pixel").on_hover_text("Click map position").clicked() {
                        *picking_position = Some((action_idx, key.clone(), false));
                    }
                }
            }
            ArgType::Color => {
                let mut rgb = match args.get(&key) {
                    Some(ActionArgValue::Color(c)) => *c,
                    _ => [1.0, 1.0, 1.0],
                };
                if egui::color_picker::color_edit_button_rgb(ui, &mut rgb).changed() {
                    args.insert(key, ActionArgValue::Color(rgb));
                    changed = true;
                }
            }
            ArgType::Direction => {
                let current = args.get(&key)
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_else(|| "down".to_string());
                egui::ComboBox::from_id_salt(format!("arg_{}", arg_def.name))
                    .selected_text(&current)
                    .show_ui(ui, |ui| {
                        for dir in ["up", "left", "down", "right"] {
                            if ui.selectable_label(current == dir, dir).clicked() {
                                args.insert(key.clone(), ActionArgValue::String(dir.to_string()));
                                changed = true;
                            }
                        }
                    });
            }
            ArgType::Easing => {
                let current = args.get(&key)
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_else(|| "Linear".to_string());
                egui::ComboBox::from_id_salt(format!("arg_{}", arg_def.name))
                    .selected_text(&current)
                    .show_ui(ui, |ui| {
                        for name in crate::scripting::scene_action::EASING_NAMES {
                            if ui.selectable_label(current == *name, *name).clicked() {
                                args.insert(key.clone(), ActionArgValue::String(name.to_string()));
                                changed = true;
                            }
                        }
                    });
            }
            ArgType::Choice(options) => {
                let current = args.get(&key)
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_else(|| options.first().map(|s| s.to_string()).unwrap_or_default());
                egui::ComboBox::from_id_salt(format!("arg_{}", arg_def.name))
                    .selected_text(&current)
                    .show_ui(ui, |ui| {
                        for opt in options {
                            if ui.selectable_label(current == *opt, *opt).clicked() {
                                args.insert(key.clone(), ActionArgValue::String(opt.to_string()));
                                changed = true;
                            }
                        }
                    });
            }
            ArgType::Spline => {
                let point_count = match args.get(&key) {
                    Some(ActionArgValue::SplinePoints(pts)) => pts.len(),
                    _ => 0,
                };
                ui.label(format!("{} waypoints (edit on map)", point_count));
                if point_count < 2 {
                    // Check if we're already in spline init pick mode for this action
                    let is_picking = picking_position.is_none(); // only show if not already picking something else
                    if is_picking {
                        ui.horizontal(|ui| {
                            if ui.small_button("Grid").on_hover_text("Click start then end on grid").clicked() {
                                *picking_position = Some((action_idx, format!("__spline_init_{}", key), true));
                            }
                            if ui.small_button("Pixel").on_hover_text("Click start then end (pixel)").clicked() {
                                *picking_position = Some((action_idx, format!("__spline_init_{}", key), false));
                            }
                        });
                    }
                }
            }
            ArgType::YarnNode => {
                let current = args.get(&key)
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_default();
                let display = if current.is_empty() { "Select node..." } else { &current };
                egui::ComboBox::from_id_salt(format!("arg_{}", arg_def.name))
                    .selected_text(display)
                    .show_ui(ui, |ui| {
                        for node_name in yarn_nodes {
                            if ui.selectable_label(current == *node_name, node_name).clicked() {
                                args.insert(key.clone(), ActionArgValue::String(node_name.clone()));
                                changed = true;
                            }
                        }
                        if yarn_nodes.is_empty() {
                            ui.label("No yarn nodes loaded. Add .yarn files to assets/dialogue/<map>/");
                        }
                    });
            }
            ArgType::SpeakerMap => {
                // Look up the selected yarn node from the sibling "node" arg
                let node_name = args.get("node")
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_default();

                if node_name.is_empty() {
                    ui.label("Select a yarn node first");
                } else {
                    // Parse speakers from the yarn file
                    let speakers = current_map_path
                        .map(|p| crate::scripting::scene_event::extract_yarn_speakers(p, &node_name))
                        .unwrap_or_default();

                    if speakers.is_empty() {
                        ui.label("No speakers found in node");
                    } else {
                        // Get or create the speaker map
                        let mut pairs: Vec<[String; 2]> = match args.get(&key) {
                            Some(ActionArgValue::SpeakerMap(p)) => p.clone(),
                            _ => Vec::new(),
                        };

                        // Ensure all speakers have an entry
                        for speaker in &speakers {
                            if !pairs.iter().any(|[s, _]| s == speaker) {
                                pairs.push([speaker.clone(), String::new()]);
                            }
                        }
                        // Remove entries for speakers no longer in the node
                        pairs.retain(|[s, _]| speakers.contains(s));

                        let mut map_changed = false;
                        for pair in &mut pairs {
                            ui.horizontal(|ui| {
                                ui.label(format!("{}:", pair[0]));
                                let display = if pair[1].is_empty() { "Select..." } else { &pair[1] };
                                egui::ComboBox::from_id_salt(format!("speaker_{}_{}", arg_def.name, pair[0]))
                                    .selected_text(display)
                                    .show_ui(ui, |ui| {
                                        for obj in &tile_state.placed_objects {
                                            let (ref_name, label) = instance_label(obj);
                                            if ui.selectable_label(pair[1] == ref_name, &label).clicked() {
                                                pair[1] = ref_name;
                                                map_changed = true;
                                            }
                                        }
                                    });
                            });
                        }

                        if map_changed {
                            changed = true;
                        }
                        args.insert(key, ActionArgValue::SpeakerMap(pairs));
                    }
                }
            }
        }
    });

    changed
}

// ── Right panel: Lua preview ──────────────────────────────────────────────

#[cfg(feature = "dev_tools")]
fn lua_preview_panel(
    ui: &mut bevy_egui::egui::Ui,
    state: &mut state::SceneBuilderState,
    current_map: &crate::map::loader::CurrentMap,
    registry: &crate::scripting::scene_action::SceneActionRegistry,
    scene_runner: &mut crate::scripting::runner::SceneRunner,
) {
    use bevy_egui::egui;
    use crate::scripting::scene_event;

    ui.heading("Lua Preview");

    let Some(sel_idx) = state.selected_event else {
        ui.label("Select an event to preview.");
        return;
    };
    let Some(event) = state.events.get(sel_idx) else {
        return;
    };

    let lua_code = scene_event::generate_lua(event, registry);

    egui::ScrollArea::vertical()
        .max_height(ui.available_height() - 40.0)
        .id_salt("lua_preview")
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut lua_code.as_str())
                    .code_editor()
                    .desired_width(f32::INFINITY),
            );
        });

    ui.separator();

    ui.horizontal(|ui| {
        let save_label = if state.dirty { "Save *" } else { "Save" };
        if ui.button(save_label).clicked() {
            if let Some(map_path) = &current_map.path {
                let file = scene_event::MapEventsFile {
                    version: 1,
                    events: state.events.clone(),
                };
                if let Err(e) = scene_event::save_events(map_path, &file) {
                    error!("Failed to save events: {e}");
                }
                // Also save the Lua script
                if let Err(e) = scene_event::save_lua_script(map_path, event, &lua_code) {
                    error!("Failed to save Lua: {e}");
                }
                // Auto-create per-map yarn project if dialogue actions exist
                if scene_event::has_dialogue_actions(&state.events) {
                    if let Err(e) = scene_event::ensure_yarn_project(map_path) {
                        error!("Failed to create yarn project: {e}");
                    }
                }
                state.dirty = false;

                // Push all events so runner updates all_events (for Interact/Touch/Script).
                // The runner only starts coroutines for AutoRun/Parallel from this list.
                scene_runner.pending_start.extend(state.events.iter().cloned());
            }
        }
        if ui.button("Save All Lua").on_hover_text("Generate and save Lua for all events").clicked() {
            if let Some(map_path) = &current_map.path {
                let file = scene_event::MapEventsFile {
                    version: 1,
                    events: state.events.clone(),
                };
                if let Err(e) = scene_event::save_events(map_path, &file) {
                    error!("Failed to save events: {e}");
                }
                for ev in &state.events {
                    let code = scene_event::generate_lua(ev, registry);
                    if let Err(e) = scene_event::save_lua_script(map_path, ev, &code) {
                        error!("Failed to save Lua for {}: {e}", ev.name);
                    }
                }
                // Auto-create per-map yarn project if dialogue actions exist
                if scene_event::has_dialogue_actions(&state.events) {
                    if let Err(e) = scene_event::ensure_yarn_project(map_path) {
                        error!("Failed to create yarn project: {e}");
                    }
                }
                state.dirty = false;

                // Push all events so runner updates all_events (for Interact/Touch/Script).
                // The runner only starts coroutines for AutoRun/Parallel from this list.
                scene_runner.pending_start.extend(state.events.iter().cloned());
            }
        }
    });
}

// ── Position pick system ──────────────────────────────────────────────────

/// System: when picking_position is set, left-click on the map fills the
/// position arg with world or grid coordinates.
#[cfg(feature = "dev_tools")]
fn pick_position_from_world(
    mut state: ResMut<state::SceneBuilderState>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&bevy::window::Window, With<bevy::window::PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::camera::CombatCamera3d>>,
    mut contexts: bevy_egui::EguiContexts,
) {
    use crate::scripting::scene_event::ActionArgValue;

    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }

    let Ok(ctx) = contexts.ctx_mut() else { return };
    if ctx.is_pointer_over_area() {
        return;
    }

    let Ok(window) = windows.single() else { return };
    let Some(cursor_pos) = window.cursor_position() else { return };
    let Ok((camera, cam_tf)) = cameras.single() else { return };
    let Ok(ray) = camera.viewport_to_world(cam_tf, cursor_pos) else { return };
    let Some(dist) = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Z)) else { return };
    let world_pos = ray.get_point(dist);

    let Some((action_idx, arg_name, grid_mode)) = state.picking_position.take() else {
        return;
    };

    // If this is NOT a spline init pick, clear any stale spline_init state
    if !arg_name.starts_with("__spline_init_") {
        state.spline_init = None;
    }

    let pos = if grid_mode {
        let tile = crate::map::DEFAULT_TILE_SIZE;
        let gx = (world_pos.x / tile).floor();
        let gy = (world_pos.y / tile).floor();
        [gx, gy]
    } else {
        [world_pos.x, world_pos.y]
    };

    // Check for spline init two-click flow
    if let Some(real_arg) = arg_name.strip_prefix("__spline_init_") {
        if let Some(ref mut init) = state.spline_init {
            // Second click — create the spline with start and end
            let start = init.start.unwrap_or(pos);
            use crate::scripting::scene_event::SplineWaypoint;
            let dx = (pos[0] - start[0]) * 0.3;
            let dy = (pos[1] - start[1]) * 0.3;
            let spline = ActionArgValue::SplinePoints(vec![
                SplineWaypoint {
                    pos: start, z: 0.0,
                    handle_in: [-dx, -dy], handle_in_z: 0.0,
                    handle_out: [dx, dy], handle_out_z: 0.0,
                },
                SplineWaypoint {
                    pos, z: 0.0,
                    handle_in: [-dx, -dy], handle_in_z: 0.0,
                    handle_out: [dx, dy], handle_out_z: 0.0,
                },
            ]);
            if let Some(sel_idx) = state.selected_event {
                if let Some(event) = state.events.get_mut(sel_idx) {
                    if let Some(action) = event.actions.get_mut(action_idx) {
                        action.args.insert(real_arg.to_string(), spline);
                        state.dirty = true;
                    }
                }
            }
            state.spline_init = None;
            info!("Created spline from ({},{}) to ({},{})", start[0], start[1], pos[0], pos[1]);
        } else {
            // First click — store start, re-enter pick mode
            state.spline_init = Some(state::SplineInitState {
                action_idx,
                arg_name: real_arg.to_string(),
                grid_mode,
                start: Some(pos),
            });
            state.picking_position = Some((action_idx, format!("__spline_init_{}", real_arg), grid_mode));
            info!("Spline start set at ({},{}), click end point", pos[0], pos[1]);
        }
        return;
    }

    // Regular position pick
    if let Some(sel_idx) = state.selected_event {
        if let Some(event) = state.events.get_mut(sel_idx) {
            if let Some(action) = event.actions.get_mut(action_idx) {
                action.args.insert(arg_name, ActionArgValue::Position(pos));
                state.dirty = true;
            }
        }
    }

    info!("Picked position: ({}, {}){}", pos[0], pos[1], if grid_mode { " (grid)" } else { "" });
}

// ── Visual pick gizmos ───────────────────────────────────────────────────

/// System: draws visual hints for the scene builder's pick operations.
/// - Position picks: highlight tile under cursor, mark already-picked positions
/// - Instance picks: could highlight target entities (future)
#[cfg(feature = "dev_tools")]
fn draw_pick_gizmos(
    state: Res<state::SceneBuilderState>,
    mut gizmos: Gizmos,
    windows: Query<&bevy::window::Window, With<bevy::window::PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::camera::CombatCamera3d>>,
    time: Res<Time>,
    elev_heights: Res<crate::map::elevation::ElevationHeights>,
    slope_maps: Res<crate::map::slope::SlopeHeightMaps>,
) {
    use crate::scripting::scene_event::ActionArgValue;

    let tile = crate::map::DEFAULT_TILE_SIZE;
    // Use level 0 for gizmos (could be improved with per-tile level lookup)
    let level = 0u8;
    let gizmo_lift = 1.0; // slight lift above ground to prevent z-fighting

    // ── Highlight tile under cursor when picking position ──
    if let Some((_, _, grid_mode)) = &state.picking_position {
        let Ok(window) = windows.single() else { return };
        let Some(cursor_pos) = window.cursor_position() else { return };
        let Ok((camera, cam_tf)) = cameras.single() else { return };
        let Ok(ray) = camera.viewport_to_world(cam_tf, cursor_pos) else { return };
        let Some(dist) = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Z)) else { return };
        let world_pos = ray.get_point(dist);

        let pulse = ((time.elapsed_secs() * 4.0).sin() * 0.3 + 0.7).clamp(0.4, 1.0);

        if *grid_mode {
            let gx = (world_pos.x / tile).floor();
            let gy = (world_pos.y / tile).floor();
            let cx = (gx + 0.5) * tile;
            let cy = (gy + 0.5) * tile;
            let half = tile / 2.0;
            let z = crate::map::slope::ground_z(&elev_heights, &slope_maps, level, cx, cy) + gizmo_lift;
            let color = Color::srgba(0.2, 0.8, 1.0, pulse);

            gizmos.line(Vec3::new(cx - half, cy - half, z), Vec3::new(cx + half, cy - half, z), color);
            gizmos.line(Vec3::new(cx + half, cy - half, z), Vec3::new(cx + half, cy + half, z), color);
            gizmos.line(Vec3::new(cx + half, cy + half, z), Vec3::new(cx - half, cy + half, z), color);
            gizmos.line(Vec3::new(cx - half, cy + half, z), Vec3::new(cx - half, cy - half, z), color);
        } else {
            let z = crate::map::slope::ground_z(&elev_heights, &slope_maps, level, world_pos.x, world_pos.y) + gizmo_lift;
            let sz = 8.0;
            let color = Color::srgba(1.0, 0.5, 0.2, pulse);
            gizmos.line(Vec3::new(world_pos.x - sz, world_pos.y, z), Vec3::new(world_pos.x + sz, world_pos.y, z), color);
            gizmos.line(Vec3::new(world_pos.x, world_pos.y - sz, z), Vec3::new(world_pos.x, world_pos.y + sz, z), color);
        }
    }

    // ── Show spline start point if first click was placed ──
    if let Some(ref init) = state.spline_init {
        if let Some([sx, sy]) = init.start {
            let wx = (sx + 0.5) * tile;
            let wy = (sy + 0.5) * tile;
            let z = crate::map::slope::ground_z(&elev_heights, &slope_maps, level, wx, wy) + gizmo_lift;
            gizmos.circle(
                Isometry3d::new(Vec3::new(wx, wy, z), Quat::IDENTITY),
                8.0,
                Color::srgba(0.2, 1.0, 0.5, 0.9),
            );
            gizmos.circle(
                Isometry3d::new(Vec3::new(wx, wy, z), Quat::IDENTITY),
                9.5,
                Color::WHITE.with_alpha(0.6),
            );
        }
    }

    // ── Draw path lines + position nodes for the selected event ──
    let Some(sel_idx) = state.selected_event else { return };
    let Some(event) = state.events.get(sel_idx) else { return };

    let pos_to_world = |x: f32, y: f32, z_offset: f32| -> Vec3 {
        let wx = (x + 0.5) * tile;
        let wy = (y + 0.5) * tile;
        let gz = crate::map::slope::ground_z(&elev_heights, &slope_maps, level, wx, wy) + gizmo_lift;
        Vec3::new(wx, wy, gz + z_offset * tile)
    };

    // Collect all nodes: main positions, control points, and endpoint positions for cross-connections.
    // Each entry: (action_idx, arg_name, world_pos)
    let mut main_nodes: Vec<(usize, String, Vec3)> = Vec::new();
    // For cross-type connection: (action_idx, endpoint_world_pos) — the final position of each movement action
    let mut action_endpoints: Vec<(usize, Vec3)> = Vec::new();

    let linear_color = Color::srgba(0.5, 0.8, 1.0, 0.5);
    let bezier_color = Color::srgba(0.2, 1.0, 0.8, 0.6);
    let handle_line_color = Color::srgba(0.8, 0.5, 1.0, 0.4);
    let connection_color = Color::srgba(0.4, 0.6, 0.8, 0.3);

    // Track last endpoint for cross-type connection lines
    let mut last_endpoint: Option<Vec3> = None;

    for (ai, action) in event.actions.iter().enumerate() {
        if action.action_id == "move_to" {
            // Linear move — draw straight line from last endpoint to position
            if let Some(ActionArgValue::Position([x, y])) = action.args.get("position") {
                let end_pos = pos_to_world(*x, *y, 0.0);

                // Cross-type connection from previous action's endpoint
                if let Some(prev_end) = last_endpoint {
                    gizmos.line(prev_end, end_pos, connection_color);
                }

                // Draw line from previous move_to in sequence
                if let Some(prev) = main_nodes.last() {
                    if prev.0 != ai { // different action
                        gizmos.line(prev.2, end_pos, linear_color);
                    }
                }

                main_nodes.push((ai, "position".to_string(), end_pos));
                action_endpoints.push((ai, end_pos));
                last_endpoint = Some(end_pos);
            }
        } else if action.action_id == "bezier_move_to" {
            if let Some(ActionArgValue::SplinePoints(waypoints)) = action.args.get("path") {
                if waypoints.len() < 2 { continue; }

                let handle_r = 4.0;
                let handle_color = Color::srgba(0.8, 0.5, 1.0, 0.8);
                let z_handle_color = Color::srgba(1.0, 0.3, 0.3, 0.6);

                // Convert waypoints to world positions
                let world_pts: Vec<Vec3> = waypoints.iter()
                    .map(|wp| pos_to_world(wp.pos[0], wp.pos[1], wp.z))
                    .collect();

                // Cross-type connection from previous action's endpoint
                if let Some(prev_end) = last_endpoint {
                    gizmos.line(prev_end, world_pts[0], connection_color);
                }

                // Build and draw bezier segments between consecutive waypoints
                for seg_i in 0..waypoints.len() - 1 {
                    let wp_a = &waypoints[seg_i];
                    let wp_b = &waypoints[seg_i + 1];

                    let p0 = world_pts[seg_i];
                    let p3 = world_pts[seg_i + 1];
                    // Handle out of A → handle in of B
                    let p1 = pos_to_world(
                        wp_a.pos[0] + wp_a.handle_out[0],
                        wp_a.pos[1] + wp_a.handle_out[1],
                        wp_a.z + wp_a.handle_out_z,
                    );
                    let p2 = pos_to_world(
                        wp_b.pos[0] + wp_b.handle_in[0],
                        wp_b.pos[1] + wp_b.handle_in[1],
                        wp_b.z + wp_b.handle_in_z,
                    );

                    let bezier = bevy::math::cubic_splines::CubicBezier::new(vec![[p0, p1, p2, p3]]);
                    if let Ok(curve) = bezier.to_curve() {
                        use bevy::math::curve::Curve;
                        let segs = 20;
                        for i in 0..segs {
                            let t0 = i as f32 / segs as f32;
                            let t1 = (i + 1) as f32 / segs as f32;
                            if let (Some(a), Some(b)) = (curve.sample(t0), curve.sample(t1)) {
                                gizmos.line(a, b, bezier_color);
                            }
                        }
                    }

                    // Handle lines: A→handle_out, B→handle_in
                    gizmos.line(p0, p1, handle_line_color);
                    gizmos.line(p3, p2, handle_line_color);

                    // Handle circles
                    for (hpos, wp_idx, suffix) in [(p1, seg_i, "ho"), (p2, seg_i + 1, "hi")] {
                        let arg_name = format!("path_{}_{}", wp_idx, suffix);
                        let is_dragged = state.dragging_node.as_ref()
                            .is_some_and(|(dai, dan)| *dai == ai && *dan == arg_name);
                        let r = if is_dragged { 6.0 } else { handle_r };
                        gizmos.circle(
                            Isometry3d::new(hpos, Quat::IDENTITY),
                            r,
                            if is_dragged { Color::srgba(1.0, 1.0, 0.3, 1.0) } else { handle_color },
                        );
                        main_nodes.push((ai, arg_name, hpos));
                    }
                }

                // Draw main waypoint nodes + Z handles
                for (wi, (wp, wpos)) in waypoints.iter().zip(world_pts.iter()).enumerate() {
                    let arg_name = format!("path_{}_pos", wi);
                    main_nodes.push((ai, arg_name, *wpos));

                    // Z handle: vertical line with draggable circle
                    if wp.z.abs() > 0.01 {
                        let ground = Vec3::new(wpos.x, wpos.y, wpos.z - wp.z * tile);
                        gizmos.line(ground, *wpos, z_handle_color);
                    }
                    // Z handle node (small circle above/below, always visible)
                    let z_node_pos = Vec3::new(wpos.x, wpos.y, wpos.z + 12.0);
                    let z_arg_name = format!("path_{}_z", wi);
                    let is_z_dragged = state.dragging_node.as_ref()
                        .is_some_and(|(dai, dan)| *dai == ai && *dan == z_arg_name);
                    let zr = if is_z_dragged { 5.0 } else { 3.0 };
                    gizmos.circle(
                        Isometry3d::new(z_node_pos, Quat::IDENTITY),
                        zr,
                        if is_z_dragged { Color::srgba(1.0, 1.0, 0.3, 1.0) } else { z_handle_color },
                    );
                    // Vertical line from node to Z handle
                    gizmos.line(*wpos, z_node_pos, z_handle_color.with_alpha(0.3));
                    main_nodes.push((ai, z_arg_name, z_node_pos));
                }

                let last_wp = world_pts.last().copied().unwrap_or(Vec3::ZERO);
                action_endpoints.push((ai, last_wp));
                last_endpoint = Some(last_wp);
            }
        }
    }

    // Draw straight-line connections between consecutive linear move_to endpoints
    // Connections between consecutive move_to actions already drawn inline above

    // Draw main position nodes (circles)
    for (ai, arg_name, pos) in &main_nodes {
        let is_ctrl = arg_name == "ctrl1" || arg_name == "ctrl2";
        if is_ctrl { continue; } // already drawn as smaller handles above

        let is_this_dragged = state.dragging_node.as_ref()
            .is_some_and(|(dai, dan)| *dai == *ai && dan == arg_name);

        let r = if is_this_dragged { 10.0 } else { 7.0 };
        let node_color = if is_this_dragged {
            Color::srgba(1.0, 1.0, 0.3, 1.0)
        } else {
            Color::srgba(0.3, 1.0, 0.3, 0.8)
        };

        gizmos.circle(
            Isometry3d::new(*pos, Quat::IDENTITY),
            r,
            node_color,
        );
        gizmos.circle(
            Isometry3d::new(*pos, Quat::IDENTITY),
            r + 1.5,
            Color::WHITE.with_alpha(0.6),
        );
    }
}

// ── Node interaction system ───────────────────────────────────────────────

/// System: handles node interactions on the map using screen-space picking.
/// - Left-click+drag on node: move it (snaps to grid)
/// - Left-click on line segment: immediately insert a new node at the clicked position
/// - Right-click on line segment: open settings popup for that action (speed, easing, etc.)
#[cfg(feature = "dev_tools")]
fn drag_position_nodes(
    mut state: ResMut<state::SceneBuilderState>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&bevy::window::Window, With<bevy::window::PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::camera::CombatCamera3d>>,
    mut contexts: bevy_egui::EguiContexts,
    elev_heights: Res<crate::map::elevation::ElevationHeights>,
    slope_maps: Res<crate::map::slope::SlopeHeightMaps>,
    action_registry: Res<crate::scripting::scene_action::SceneActionRegistry>,
) {
    use crate::scripting::scene_event::{ActionArgValue, EventAction};
    use crate::scripting::scene_action::ArgType;
    use bevy_egui::egui;

    if state.picking_position.is_some() {
        return;
    }

    let Some(sel_idx) = state.selected_event else {
        state.dragging_node = None;
        state.line_popup_action = None;
        return;
    };

    // ── Line segment settings popup (right-click) ──
    let Ok(ctx) = contexts.ctx_mut() else { return };

    if let Some(popup_ai) = state.line_popup_action {
        let mut popup_open = true;
        let mut popup_dirty = false;
        let events = &mut state.events;
        egui::Window::new("Step Settings")
            .collapsible(false)
            .resizable(false)
            .default_width(200.0)
            .open(&mut popup_open)
            .show(ctx, |ui| {
                if let Some(event) = events.get_mut(sel_idx) {
                    if let Some(action) = event.actions.get_mut(popup_ai) {
                        if let Some(def) = action_registry.get(&action.action_id) {
                            for arg_def in &def.args {
                                match &arg_def.arg_type {
                                    ArgType::Position | ArgType::InstanceRef | ArgType::SubRef | ArgType::YarnNode => continue,
                                    _ => {}
                                }
                                ui.horizontal(|ui| {
                                    ui.label(format!("{}:", arg_def.label));
                                    match &arg_def.arg_type {
                                        ArgType::Float { min, max, default } => {
                                            let mut val = action.args.get(arg_def.name)
                                                .map(|v| v.as_f32())
                                                .unwrap_or(*default);
                                            if ui.add(egui::DragValue::new(&mut val).range(*min..=*max).speed(0.1)).changed() {
                                                action.args.insert(arg_def.name.to_string(), ActionArgValue::Float(val as f64));
                                                popup_dirty = true;
                                            }
                                        }
                                        ArgType::Easing => {
                                            let current = action.args.get(arg_def.name)
                                                .and_then(|v| v.as_str().map(|s| s.to_string()))
                                                .unwrap_or_else(|| "Linear".to_string());
                                            egui::ComboBox::from_id_salt(format!("popup_easing_{popup_ai}"))
                                                .selected_text(&current)
                                                .show_ui(ui, |ui| {
                                                    for name in crate::scripting::scene_action::EASING_NAMES {
                                                        if ui.selectable_label(current == *name, *name).clicked() {
                                                            action.args.insert(arg_def.name.to_string(), ActionArgValue::String(name.to_string()));
                                                            popup_dirty = true;
                                                        }
                                                    }
                                                });
                                        }
                                        _ => {}
                                    }
                                });
                            }
                        }
                    }
                }
            });
        if popup_dirty { state.dirty = true; }
        if !popup_open {
            state.line_popup_action = None;
        }
        return;
    }

    if ctx.is_pointer_over_area() {
        if state.dragging_node.is_some() && mouse.just_released(MouseButton::Left) {
            state.dragging_node = None;
        }
        return;
    }

    let Ok(window) = windows.single() else { return };
    let Some(cursor_pos) = window.cursor_position() else { return };
    let Ok((camera, cam_tf)) = cameras.single() else { return };

    let tile = crate::map::DEFAULT_TILE_SIZE;
    let level = 0u8;
    let pick_screen_radius = 18.0;
    let line_screen_dist = 14.0;

    // ── Collect node screen positions for picking ──
    // Includes regular Position args AND spline waypoint/handle sub-nodes.
    let node_data: Vec<(usize, String, Vec2, f32, f32)> = state.events.get(sel_idx)
        .map(|event| {
            let mut nodes = Vec::new();
            for (ai, action) in event.actions.iter().enumerate() {
                for (arg_name, val) in &action.args {
                    match val {
                        ActionArgValue::Position([x, y]) => {
                            let wx = (x + 0.5) * tile;
                            let wy = (y + 0.5) * tile;
                            let wz = crate::map::slope::ground_z(&elev_heights, &slope_maps, level, wx, wy) + 1.0;
                            if let Ok(screen) = camera.world_to_viewport(cam_tf, Vec3::new(wx, wy, wz)) {
                                nodes.push((ai, arg_name.clone(), screen, *x, *y));
                            }
                        }
                        ActionArgValue::SplinePoints(waypoints) => {
                            for (wi, wp) in waypoints.iter().enumerate() {
                                // Main position node
                                let wx = (wp.pos[0] + 0.5) * tile;
                                let wy = (wp.pos[1] + 0.5) * tile;
                                let wz = crate::map::slope::ground_z(&elev_heights, &slope_maps, level, wx, wy) + 1.0 + wp.z * tile;
                                if let Ok(screen) = camera.world_to_viewport(cam_tf, Vec3::new(wx, wy, wz)) {
                                    nodes.push((ai, format!("path_{wi}_pos"), screen, wp.pos[0], wp.pos[1]));
                                }
                                // Handle out
                                let ho_x = wp.pos[0] + wp.handle_out[0];
                                let ho_y = wp.pos[1] + wp.handle_out[1];
                                let ho_wx = (ho_x + 0.5) * tile;
                                let ho_wy = (ho_y + 0.5) * tile;
                                let ho_wz = wz + wp.handle_out_z * tile;
                                if let Ok(screen) = camera.world_to_viewport(cam_tf, Vec3::new(ho_wx, ho_wy, ho_wz)) {
                                    nodes.push((ai, format!("path_{wi}_ho"), screen, ho_x, ho_y));
                                }
                                // Handle in
                                let hi_x = wp.pos[0] + wp.handle_in[0];
                                let hi_y = wp.pos[1] + wp.handle_in[1];
                                let hi_wx = (hi_x + 0.5) * tile;
                                let hi_wy = (hi_y + 0.5) * tile;
                                let hi_wz = wz + wp.handle_in_z * tile;
                                if let Ok(screen) = camera.world_to_viewport(cam_tf, Vec3::new(hi_wx, hi_wy, hi_wz)) {
                                    nodes.push((ai, format!("path_{wi}_hi"), screen, hi_x, hi_y));
                                }
                                // Z handle
                                let z_pos = Vec3::new(wx, wy, wz + 12.0);
                                if let Ok(screen) = camera.world_to_viewport(cam_tf, z_pos) {
                                    nodes.push((ai, format!("path_{wi}_z"), screen, wp.pos[0], wp.pos[1]));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            nodes
        })
        .unwrap_or_default();

    let closest_node: Option<(usize, String, f32)> = node_data.iter()
        .filter_map(|(ai, name, screen, _, _)| {
            let d = screen.distance(cursor_pos);
            if d < pick_screen_radius { Some((*ai, name.clone(), d)) } else { None }
        })
        .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

    // Find closest line segment
    let closest_line: Option<(usize, f32, f32)> = node_data.windows(2)
        .filter_map(|pair| {
            let (ai_a, _, sa, _, _) = &pair[0];
            let (_, _, sb, _, _) = &pair[1];
            let seg = *sb - *sa;
            let seg_len = seg.length();
            if seg_len < 1.0 { return None; }
            let t = ((cursor_pos - *sa).dot(seg)) / (seg_len * seg_len);
            let t = t.clamp(0.1, 0.9);
            let closest = *sa + seg * t;
            let dist = cursor_pos.distance(closest);
            if dist < line_screen_dist { Some((*ai_a, dist, t)) } else { None }
        })
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    // ── Left-click: drag node or subdivide line ──
    if mouse.just_pressed(MouseButton::Left) && state.dragging_node.is_none() {
        // Nodes take priority
        if let Some((ai, arg_name, _)) = closest_node {
            state.dragging_node = Some((ai, arg_name));
            return;
        }

        // Click on line = subdivide
        if let Some((ai_a, _, _)) = closest_line {
            let Ok(ray) = camera.viewport_to_world(cam_tf, cursor_pos) else { return };
            let Some(d) = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Z)) else { return };
            let click_world = ray.get_point(d);
            let gx = (click_world.x / tile).floor();
            let gy = (click_world.y / tile).floor();
            if let Some(event) = state.events.get_mut(sel_idx) {
                if let Some(action) = event.actions.get_mut(ai_a) {
                    if action.action_id == "bezier_move_to" {
                        // Insert waypoint into existing spline
                        if let Some(ActionArgValue::SplinePoints(pts)) = action.args.get_mut("path") {
                            // Find which segment was clicked by checking closest pair
                            let mut best_seg = 0usize;
                            let mut best_dist = f32::MAX;
                            for si in 0..pts.len().saturating_sub(1) {
                                let ax = (pts[si].pos[0] + 0.5) * tile;
                                let ay = (pts[si].pos[1] + 0.5) * tile;
                                let bx = (pts[si+1].pos[0] + 0.5) * tile;
                                let by = (pts[si+1].pos[1] + 0.5) * tile;
                                let mid_x = (ax + bx) * 0.5;
                                let mid_y = (ay + by) * 0.5;
                                let d = ((click_world.x - mid_x).powi(2) + (click_world.y - mid_y).powi(2)).sqrt();
                                if d < best_dist {
                                    best_dist = d;
                                    best_seg = si;
                                }
                            }
                            use crate::scripting::scene_event::SplineWaypoint;
                            let new_wp = SplineWaypoint {
                                pos: [gx, gy], z: 0.0,
                                handle_in: [-1.0, 0.0], handle_in_z: 0.0,
                                handle_out: [1.0, 0.0], handle_out_z: 0.0,
                            };
                            pts.insert(best_seg + 1, new_wp);
                            state.dirty = true;
                        }
                    } else {
                        // Linear move_to: insert new action after
                        let src = action.clone();
                        let mut new_args = HashMap::new();
                        for (k, v) in &src.args {
                            if k != "position" {
                                new_args.insert(k.clone(), v.clone());
                            }
                        }
                        new_args.insert("position".to_string(), ActionArgValue::Position([gx, gy]));
                        event.actions.insert(ai_a + 1, EventAction {
                            action_id: src.action_id.clone(),
                            args: new_args,
                        });
                        state.dirty = true;
                    }
                }
            }
            return;
        }
    }

    // ── Right-click: delete node/waypoint or open line settings ──
    if mouse.just_pressed(MouseButton::Right) {
        if let Some((ai, ref arg_name, _)) = closest_node {
            if let Some(event) = state.events.get_mut(sel_idx) {
                if arg_name.starts_with("path_") && arg_name.ends_with("_pos") {
                    // Spline waypoint: remove it from the spline (if >2 points)
                    if let Some(action) = event.actions.get_mut(ai) {
                        if let Some(ActionArgValue::SplinePoints(pts)) = action.args.get_mut("path") {
                            let parts: Vec<&str> = arg_name.splitn(3, '_').collect();
                            if parts.len() == 3 {
                                let idx: usize = parts[1].parse().unwrap_or(0);
                                if pts.len() > 2 && idx < pts.len() {
                                    pts.remove(idx);
                                    state.dirty = true;
                                }
                            }
                        }
                    }
                } else if !arg_name.starts_with("path_") {
                    // Regular position node: delete the whole action
                    if ai < event.actions.len() {
                        event.actions.remove(ai);
                        state.dirty = true;
                    }
                }
            }
            return;
        }
        // Right-click on line = open settings popup
        if let Some((ai_a, _, _)) = closest_line {
            state.line_popup_action = Some(ai_a);
            return;
        }
    }

    // ── Handle active drag ──
    if let Some((ai, ref arg_name)) = state.dragging_node.clone() {
        if mouse.pressed(MouseButton::Left) {
            let Ok(ray) = camera.viewport_to_world(cam_tf, cursor_pos) else { return };
            let Some(d) = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Z)) else { return };
            let world_click = ray.get_point(d);
            let gx = (world_click.x / tile).floor();
            let gy = (world_click.y / tile).floor();

            if let Some(event) = state.events.get_mut(sel_idx) {
                if let Some(action) = event.actions.get_mut(ai) {
                    // Check if this is a spline sub-node (path_N_pos, path_N_ho, path_N_hi, path_N_z)
                    if arg_name.starts_with("path_") {
                        if let Some(ActionArgValue::SplinePoints(pts)) = action.args.get_mut("path") {
                            // Parse: "path_<idx>_<suffix>"
                            let parts: Vec<&str> = arg_name.splitn(3, '_').collect();
                            if parts.len() == 3 {
                                let idx: usize = parts[1].parse().unwrap_or(0);
                                if let Some(wp) = pts.get_mut(idx) {
                                    match parts[2] {
                                        "pos" => {
                                            wp.pos = [gx, gy];
                                        }
                                        "ho" => {
                                            // Handle out: store as offset from pos
                                            wp.handle_out = [gx - wp.pos[0], gy - wp.pos[1]];
                                        }
                                        "hi" => {
                                            // Handle in: store as offset from pos
                                            wp.handle_in = [gx - wp.pos[0], gy - wp.pos[1]];
                                        }
                                        "z" => {
                                            // Z drag: map screen Y movement to Z change
                                            // Use world Y delta as Z proxy (dragging up = higher)
                                            let current_world_y = (wp.pos[1] + 0.5) * tile;
                                            let dy = world_click.y - current_world_y;
                                            wp.z = (dy / tile).clamp(-10.0, 10.0);
                                        }
                                        _ => {}
                                    }
                                    state.dirty = true;
                                }
                            }
                        }
                    } else {
                        // Regular Position arg
                        action.args.insert(arg_name.clone(), ActionArgValue::Position([gx, gy]));
                        state.dirty = true;
                    }
                }
            }
        }
        if mouse.just_released(MouseButton::Left) {
            state.dragging_node = None;
        }
    }
}
