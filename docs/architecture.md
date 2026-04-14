# Untitled JRPG Engine — Architecture Guide

## Overview

The engine is built on **Bevy 0.15** with a plugin-per-subsystem architecture. Every module exposes a single `Plugin` struct registered through the top-level `UntitledPlugin`. Game data is defined in **Lua**, maps in **Tiled**, and configuration in **YAML**.

```
UntitledPlugin
  ├── ConfigPlugin        — display settings, keybind schemas
  ├── InputPlugin         — action mapping, context switching
  ├── CameraPlugin        — follow, zoom, scripted camera ops
  ├── MapPlugin           — Tiled loading, collision, properties, z-ordering
  ├── SpritePlugin        — pluggable sheet formats, LPC default, animation
  ├── LightingPlugin      — bevy_light_2d wrapper, ambient, emissive linking
  ├── ParticlePlugin      — emitters, pooling, Lua-defined particle types
  ├── EntityPlugin        — actors, stats, inventory
  ├── CombatPlugin        — grid, phases, actions, AI, targeting, procs
  ├── SoundPlugin         — spatial 2D audio, BGM, SFX with per-source falloff
  ├── ScriptingPlugin     — Lua VM, event bridge, cutscene coroutines
  ├── UiPlugin            — egui menus, HUD, dialogue, battle UI
  └── SavePlugin          — per-room YAML, slot management, Saveable derive
```

Registration order matters: gameplay types must exist before `ScriptingPlugin` registers Lua bindings, and `UiPlugin` renders on top of everything.

---

## Game States

Defined in `src/app_state.rs`.

| State | Purpose |
|-------|---------|
| `Loading` | Asset loading, splash screen |
| `MainMenu` | Title screen |
| `Overworld` | Exploration, NPC interaction |
| `Combat` | Tactical grid combat (has sub-states) |
| `Cutscene` | Lua-driven scripted sequences |
| `Paused` | Pause overlay |

**`CombatPhase`** is a `SubStates` of `GameState::Combat`:

```
GridSetup → PlayerTurnSelect → PlayerExecute → EnemyTurnSelect → EnemyExecute → Cleanup
                                                                                    ↓
                                                                          (loops back to PlayerTurnSelect
                                                                           or exits combat)
```

---

## Subsystem Reference

### Camera (`src/camera/`)

**Purpose:** 2D camera with smooth follow, zoom, and scripted operations.

- `CameraTarget` component marks the entity the camera tracks.
- `SmoothFollow` component configures follow speed and zoom level.
- `CameraCommand` event enum drives scripted ops: `PanTo`, `Shake`, `Flash`, `Zoom` — all with duration for interpolation.
- The camera spawns as a `Camera2d` at startup.

**How it works:** A follow system lerps the camera transform toward the `CameraTarget` entity each frame. During cutscenes, `CameraCommand` events override the follow behavior temporarily.

### Map (`src/map/`)

**Purpose:** Load Tiled `.tmx`/`.tmj` maps via `bevy_ecs_tiled` and extract gameplay data, plus procedural terrain rendering.

| Module | Role |
|--------|------|
| `loader.rs` | `ActiveMap` marker, `CurrentMap` resource tracking loaded map path |
| `collision.rs` | `CollisionShape` component, `CornerSlip` for smooth corner sliding |
| `properties.rs` | `TiledProperties` parsed from custom Tiled properties (combat zones, triggers, lights) |
| `height.rs` | `HeightLayer` enum for z-ordering: Ground(0), Objects(10), Bridge(20), Overhead(30) |
| `terrain_material.rs` | `TerrainMaterial` (custom tilemap shader material), terrain type map generation, terrain IDs |
| `terrain_edges.rs` | Greedy-merged colliders for impassable terrain (river, shallows) |

**Z-ordering formula:** `transform.z = layer_base + (max_y - entity_y) / (max_y * 2)`. Large sprites sort by feet position, not center. This gives correct depth for a 3/4-view perspective.

**Adding maps:** Create `.tmx` files in Tiled. Use object layers for collision shapes (rectangles/polygons). Set custom properties on objects:
- `combat_zone: bool` — marks area as a potential encounter zone
- `trigger_script: string` — Lua script ID to run on contact
- `light_radius: float` — attach a point light to this object

#### Terrain Rendering System

The terrain system is a **shader-based state machine** where each terrain type is a "state" with its own fill shader, and transitions between adjacent types define edge shaders.

**Terrain Type Map:** A tiny texture (1 pixel per map tile) with the R channel encoding terrain state IDs (0-255). Built at runtime from `terrain_surfaces` tileset tile indices. Passed to the shader via `TerrainMaterial`.

**Terrain IDs** (defined in `terrain_material.rs::terrain_id` and mirrored in `terrain_fill.wgsl`):

| ID | Name | Tile Index | Fill Source | Walkable |
|----|------|-----------|-------------|----------|
| 0 | EMPTY | — | — | — |
| 1 | RIVER | 0 | Procedural water shader | No |
| 2 | GRASS | 1 | `grass.qoi` (894×894) | Yes |
| 3 | SAND | 2 | (not yet implemented) | Yes |
| 4 | DIRT | 3 | `dirt.qoi` (2048×2048) | Yes |
| 5 | SNOW | 4 | (not yet implemented) | Yes |
| 6 | LAVA | 5 | (not yet implemented) | — |
| 7 | MUD | 6 | (not yet implemented) | — |
| 8 | STONE | 7 | (not yet implemented) | — |
| 9 | SHALLOWS | 8 | `stone.qoi` under water overlay | No |

**Fill Shaders** (dispatched by `terrain_fill()` in `terrain_fill.wgsl`):
- **River:** Directional flowing water with ripples, wave crests, foam. Depth-darkened via `river_depth_at()` — bilinearly interpolated distance-to-shore for smooth Minecraft-style depth blending. Center of wide rivers is near-black blue.
- **Grass/Dirt:** 1:1 texture sampling from QOI textures, wrapping naturally.
- **Shallows:** Stone texture with water layered ON TOP using darken blend mode (`min(stone, water)`). Vertically stacks — `shallows_stack_info()` walks the column to compute depth. Water opacity increases in 5 discrete steps with depth. Caustic shimmer fades with depth.

**Transition Types** (dispatched by `get_transition_type()` → `transition_edge()`):

| Transition | Between | Width | Behavior |
|-----------|---------|-------|----------|
| WATER_SHORE | Land ↔ River/Shallows | 0.35 | Submerged land fading to water. Land side: wet darkening. Water side: land texture visible through tinted water. Full opacity both sides. |
| GRASS_BLEND | Grass ↔ Dirt | 0.5 | Minecraft-style biome blend — noise-masked mix of both textures. Wide and organic. |
| WATER_DEPTH | River ↔ Shallows | 0.5 | Smooth blend between shallows stone-under-water and deep river. |

**Multi-transition blending:** At triple junction points (e.g., grass/dirt/river meeting), ALL active boundary transitions are evaluated independently and accumulated with proximity-based weights. Convex/concave corners are skipped for wide blends (GRASS_BLEND, WATER_DEPTH) since the cardinal transitions already cover the area.

**Corner slipping** (`collision.rs`): `CornerSlip` component enables RPG-style corner sliding. When movement is blocked, the system shape-casts perpendicular to detect if nudging around a corner would clear the path.

**Collider generation** (`terrain_edges.rs`): Greedy rectangle merging over all impassable tiles (RIVER + SHALLOWS). Produces minimal `RigidBody::Static` + `Collider::rectangle` entities.

**Debug overlay:** F3 toggles physics gizmos and FPS counter. Game runs with `PresentMode::Immediate` for uncapped framerate.

**Texture format:** All terrain textures use QOI format for fast decode. Bevy feature `"qoi"` is enabled.

**Tile size independence:** The shader reads tile size from `tilemap_data.tile_size` — no hardcoded pixel dimensions. The Rust side uses `DEFAULT_TILE_SIZE` constant for physics setup.

**Adding new terrain types:**
1. Add constant to `terrain_id` module and shader `ID_*` constants (keep in sync)
2. Add tile to `terrain_surfaces.tsx/png` (tile index = ID - 1)
3. Add fill function in shader and case to `terrain_fill()` dispatcher
4. Add transition rules to `get_transition_type()`
5. If impassable, add to `is_impassable` closure in `terrain_edges.rs`
6. If water-like, add to `is_watery()` in the shader

### Sprite (`src/sprite/`)

**Purpose:** Pluggable spritesheet system with LPC Universal Spritesheet as the default.

- **`SpriteFormat` trait** — implement this for any sheet layout. Methods: `frame_size()`, `columns()`, `row_for(animation, direction)`, `frame_count(animation)`.
- **`LpcFormat`** — default implementation. 64×64 frames, 13 columns, standard LPC row layout (spellcast, thrust, walk, slash, shoot, hurt × 4 directions).
- **`AtlasMeta`** — component for runtime atlas splitting metadata.
- **`AnimationController`** — component driving frame ticking. Tracks current animation name, direction (0=up, 1=left, 2=down, 3=right), frame index, and a repeating timer.

**Adding sprite formats:** Implement `SpriteFormat` for your sheet layout. The splitter uses the trait's metadata to extract frames at runtime — no pre-processing needed.

### Combat (`src/combat/`)

**Purpose:** Tactical grid-based combat that materializes on the existing overworld map.

#### Grid Generation (`grid.rs`)
1. Collect world positions of all `CombatActor` entities
2. Compute axis-aligned bounding box in tile coordinates
3. Expand +1 tile in each direction
4. Clamp to camera viewport bounds
5. Query collision map to mark non-walkable cells
6. `CombatObstacle` components occupy full grid squares
7. `ScriptedGridOverride` bypasses auto-generation entirely

#### Phase State Machine (`phase.rs`)
`CombatPhase` sub-state drives turn flow. `AdvancePhase` event triggers transitions. `next_phase()` maps each phase to its successor; `Cleanup` loops back to `PlayerTurnSelect` (or exits combat when the encounter ends).

#### Actions (`action.rs`)
- `Ability` — id, name, hit type (Melee/Ranged/Magic), range (min/max), AoE pattern, base power, MP cost
- `AoePattern` — Single, Line, Cross, Diamond, Square (each with radius/length)
- `AbilityRange` — min and max tile distance

#### Movement (`movement.rs`)
A* pathfinding via the `pathfinding` crate. `MovePath` component stores the computed path. `PathArrow` renders a preview polyline.

#### Targeting (`targeting.rs`)
`resolve_aoe()` expands an `AoePattern` from an origin point into the set of affected cells. `TargetHighlights` resource stores the currently highlighted valid targets.

#### Proc System (`proc_system.rs`)
Procs fire after each hit. A `Proc` has:
- **Trigger:** OnHit, OnCrit, OnKill, OnDamageTaken
- **Condition:** Always, ChancePercent, TargetBelowHpPercent
- **Effect:** BonusDamage, Heal, ApplyStatus, BonusHit (chains into another ability)

This allows equipment and abilities to have conditional bonus effects that cascade.

#### Cursor (`cursor.rs`)
`GridCursor` resource tracks the player's selected grid cell during combat.

#### AI (`ai/`)
`AiBehavior` trait takes an entity + world reference and returns an `AiTurnPlan` (move_path + ability_id + target). The `DefaultAi` implementation evaluates all `(destination, ability, target)` triples: for each ability, it finds reachable positions where that ability can hit an enemy, scores by expected damage, and picks the best. This naturally handles move-then-melee vs. stay-and-cast-ranged decisions.

### Entity (`src/entity/`)

**Purpose:** Core gameplay entities — actors and their inventories.

- **`Actor`** — identity (id, name, allegiance: Player/Enemy/Neutral)
- **`Stats`** — hp, max_hp, mp, max_mp, attack, defense, magic, speed, movement (serde-enabled for saving)
- **`CombatActor`** — marks an entity as active in combat with grid position and action flags
- **`Item`** / `ItemStack` / `Inventory` — item definitions and container component (all serde-enabled)

### Lighting (`src/lighting/`)

**Purpose:** Custom screen-space post-process lighting with day/night cycle and point lights.

Uses a **multiply-blend post-process pass** inserted into Bevy's `core_3d` render graph (between `EndMainPass` and `Tonemapping`). All materials (terrain, sprites, UI) are lit uniformly — no per-material shader changes needed.

#### Components & Resources

| Type | Role |
|------|------|
| `LightSource` | Point light component: `color`, `intensity`, `inner_radius`, `outer_radius` (world units) |
| `AmbientConfig` | Resource: global ambient `color` and `intensity` (driven by time of day) |
| `TimeOfDay` | Resource: `hour` (0–24), `speed` (game-hours/real-second), `paused` flag |
| `EmissiveLink` | Component: bridges a `ParticleEmitter` to a `LightSource`, scaling intensity with emitter activity |
| `LightingPostProcess` | Marker component on the camera to enable the lighting pass |

#### Day/Night Cycle

`TimeOfDay` advances each frame by `delta * speed`. A piecewise-linear curve maps hour → ambient:

| Period | Hours | Ambient |
|--------|-------|---------|
| Night | 21–5 | Dark blue, 0.08 |
| Pre-dawn | 5–6 | Blue → orange, 0.08 → 0.3 |
| Dawn | 6–8 | Orange → white, 0.3 → 1.0 |
| Day | 8–17 | White, 1.0 |
| Dusk | 17–19 | White → orange, 1.0 → 0.3 |
| Twilight | 19–21 | Orange → blue, 0.3 → 0.08 |

#### Render Pipeline

1. **Extract** (`ExtractSchedule`): reads `AmbientConfig` + all `(LightSource, GlobalTransform)`, projects lights to screen space via `camera.world_to_viewport()`, packs into `ExtractedLightData`
2. **Prepare** (`RenderSet::Prepare`): writes GPU uniform/storage buffer with ambient + up to 128 point lights
3. **Node** (`LightingNode`): fullscreen triangle pass — samples HDR scene color, multiplies by `ambient + sum(point_light_contributions)` using `smoothstep` radial falloff

The post-process shader (`assets/shaders/lighting_post.wgsl`) accumulates light contributions and clamps to [0, 2] to allow slight over-brightening for strong lights at night without blowout.

#### Tiled Integration

Tiled objects with a `light_radius: float` property automatically get a `LightSource` component (via `Added<TiledProperties>` query). Inner radius defaults to 30% of outer.

#### Lua API

```lua
lighting.set_ambient(r, g, b, intensity)   -- override ambient directly
lighting.spawn_light(x, y, r, g, b, intensity, radius)
lighting.set_time(hour)                     -- jump to time
lighting.set_time_speed(speed)              -- 0 = paused, 0.1 = slow
```

### Particles (`src/particles/`)

**Purpose:** Lua-defined particle effects with object pooling.

- `ParticleDef` — loaded from Lua: lifetime, speed range, start/end color, start/end size, gravity
- `ParticleRegistry` resource — stores all loaded definitions
- `ParticleEmitter` component — references a definition by ID, controls spawn rate and active state

### Sound (`src/sound/`)

**Purpose:** Positional 2D audio with left/right panning and per-source distance falloff, plus global BGM and SFX.

Uses **Bevy's built-in audio** with `SpatialScale::new_2d()` configured in main. The `AudioPlugin` handles stereo panning via `SpatialListener` ear offsets — if the player is left of a campfire, the campfire is louder in the right ear.

#### Spatial Falloff (`spatial.rs`)
Bevy's built-in spatial audio only has a global distance scale. We add **per-source falloff** via the `SpatialFalloff` component:
- `inner_radius` — full volume within this distance (default 50 world units)
- `outer_radius` — silent beyond this distance (default 300 world units)
- Linear interpolation between the two

The `update_spatial_falloff` system runs each frame, computing distance from each emitter to the `GameListener` entity and adjusting `AudioSink` volume accordingly.

#### BGM (`bgm.rs`)
`BgmState` resource tracks the current background music track and its entity. BGM plays globally (non-spatial).

#### Sound Commands
`SoundCommand` event enum for system/Lua integration:
- `PlayBgm { asset_path, fade_in }` — crossfade to new BGM
- `StopBgm { fade_out }` — fade out current BGM
- `PlaySfx { asset_path }` — one-shot global sound
- `PlaySfxAt { asset_path, position }` — one-shot spatial sound at a world position

#### Setup
The `SpatialListener` is spawned with the camera (ear gap configurable). World sound sources get:
```rust
commands.spawn((
    Transform::from_translation(campfire_pos.extend(0.0)),
    AudioPlayer::new(asset_server.load("sounds/campfire.ogg")),
    PlaybackSettings::LOOP.with_spatial(true),
    SpatialFalloff {
        inner_radius: 30.0,
        outer_radius: 200.0,
    },
));
```

#### Lua API
```lua
sound.play_bgm("music/battle_theme.ogg", 1.5)  -- 1.5s fade in
sound.stop_bgm(0.5)                              -- 0.5s fade out
sound.play_sfx("sfx/menu_select.ogg")            -- global one-shot
sound.play_sfx_at("sfx/explosion.ogg", x, y)     -- spatial one-shot
```

### Scripting (`src/scripting/`)

**Purpose:** Lua scripting layer. Lua never directly mutates the ECS.

#### Execution Model
1. Lua API functions (e.g., `camera.pan(x, y, duration)`) push `LuaCommand` events
2. Bevy systems drain `LuaCommand` in `PostUpdate`
3. This keeps Lua and ECS execution cleanly separated

#### Event Bridge (`event_bridge.rs`)
`LuaCommand` enum covers all scripted operations: camera control, combat actions, dialogue, entity movement, VFX, and world manipulation (spawn, flags).

#### Cutscenes (`cutscene.rs`)
Lua coroutines drive cutscenes. Each `coroutine.yield()` pauses execution for one frame. The `ActiveCutscene` resource tracks the running script. The cutscene system resumes one step per frame.

#### API Modules (`api/`)
Lua-facing API tables organized by domain: `camera`, `combat`, `dialogue`, `movement`, `vfx`, `world`. Each module registers functions into the Lua VM that construct and push `LuaCommand` events.

#### Data Loaders (`data/`)
Load game content from Lua files: items, abilities, enemies, encounters, AI behaviors. See "Adding Game Content" below.

### UI (`src/ui/`)

**Purpose:** All user interface via `bevy_egui`.

| Module | Content |
|--------|---------|
| `main_menu.rs` | Title screen — new game, load, settings |
| `pause_menu.rs` | Pause overlay, inventory access |
| `battle_ui.rs` | Combat: move select, ability list, target confirmation |
| `dialogue_box.rs` | Dialogue with speaker portraits and branching choices |
| `hud.rs` | HP/MP bars, status effect icons |

### Save (`src/save/`)

**Purpose:** Per-room YAML save files with opt-in component serialization.

#### Save Format
```
saves/slot_0/
  global.yaml         — party composition, flags, current room, playtime
  rooms/
    forest_01.yaml    — per-entity saved field data
    town_square.yaml
```

#### Opt-In via Derive
```rust
#[derive(Saveable)]
struct MyComponent {
    #[save] health: i32,      // included in save data
    #[save] position: Vec2,   // included
    transient_vfx: Handle<Image>, // NOT saved
}
```

`SaveRegistry` auto-discovers all registered `Saveable` types.

#### Slot Management (`slots.rs`)
Helper functions: `slot_path(n)`, `room_path(slot, room_id)`, `global_path(slot)`.

### Input (`src/input/`)

**Purpose:** Context-sensitive input mapping.

- `InputAction` enum — logical actions (movement, confirm, cancel, menu, cursor)
- `InputMapping` — loaded from YAML, maps actions to key name strings
- `InputContext` resource — gates which action set is active: Overworld, Combat, Menu, Dialogue, Cutscene

Gamepad support comes from Bevy's built-in gilrs integration.

### Config (`src/config/`)

**Purpose:** YAML-driven configuration.

- `DisplayConfig` — resolution, vsync, fullscreen
- `KeybindConfig` / `KeybindEntry` — action-to-key mappings loaded from `config/keybinds.yaml`

---

## Adding Game Content

All game data lives in `data/` as Lua files. Each file returns a table that the engine's data loaders parse into Bevy components/resources.

### Items (`data/items/*.lua`)

```lua
return {
    id = "potion",
    name = "Potion",
    description = "Restores 50 HP.",
    stackable = true,
    max_stack = 99,
    on_use = function(user, target)
        combat.heal(target, 50)
        vfx.spawn_particle("heal_sparkle", target)
    end,
}
```

### Abilities (`data/abilities/*.lua`)

```lua
return {
    id = "fireball",
    name = "Fireball",
    hit_type = "magic",
    range = { min = 2, max = 4 },
    aoe = { pattern = "diamond", radius = 1 },
    base_power = 45,
    mp_cost = 12,
    procs = {
        {
            trigger = "on_hit",
            condition = { type = "chance_percent", value = 25 },
            effect = { type = "apply_status", status = "burn" },
        },
    },
    on_use = function(user, targets)
        vfx.spawn_particle("fire_explosion", targets[1])
        camera.shake(0.3, 0.2)
    end,
}
```

### Enemies (`data/enemies/*.lua`)

```lua
return {
    id = "goblin",
    name = "Goblin",
    sprite = "sprites/enemies/goblin.png",
    sprite_format = "lpc",
    stats = {
        hp = 30, max_hp = 30,
        mp = 5,  max_mp = 5,
        attack = 8, defense = 3,
        magic = 2, speed = 6,
        movement = 4,
    },
    abilities = { "slash_basic", "throw_rock" },
    ai = "aggressive",  -- references data/ai/aggressive.lua
    drops = {
        { item = "potion", chance = 0.3 },
        { item = "gold_coin", chance = 1.0, count = { 2, 5 } },
    },
}
```

### Encounters (`data/encounters/*.lua`)

```lua
return {
    id = "forest_ambush",
    enemies = {
        { template = "goblin", grid_pos = { 5, 2 } },
        { template = "goblin", grid_pos = { 6, 4 } },
        { template = "goblin_chief", grid_pos = { 7, 3 } },
    },
    -- Optional: override auto grid generation
    grid_override = {
        origin = { 3, 0 },
        width = 8,
        height = 7,
    },
    music = "battle_forest",
    on_start = function()
        dialogue.show("Goblin Chief", "You won't pass through here alive!")
    end,
    on_victory = function()
        world.set_flag("forest_ambush_cleared", "true")
    end,
}
```

### AI Behaviors (`data/ai/*.lua`)

```lua
return {
    id = "aggressive",
    -- Called each turn for each enemy with this AI
    evaluate = function(self_entity, enemies, allies)
        -- Return best (destination, ability, target) triple
        -- The default engine AI handles this if you return nil
        return nil
    end,
}
```

### Particles (`data/particles/*.lua`)

```lua
return {
    id = "fire_explosion",
    lifetime = 0.8,
    speed = { 20, 80 },
    color_start = { 1.0, 0.6, 0.1, 1.0 },
    color_end = { 0.3, 0.0, 0.0, 0.0 },
    size_start = 4.0,
    size_end = 1.0,
    gravity = -20.0,
}
```

### Cutscenes (`data/cutscenes/*.lua`)

```lua
return function()
    camera.pan(500, 300, 1.5)
    coroutine.yield()  -- wait one frame

    entity.move_to("hero", 500, 300, 100)
    coroutine.yield()  -- wait for move to complete

    dialogue.show("Elder", "The forest ahead is dangerous.", "elder_portrait")
    coroutine.yield()  -- wait for dialogue dismiss

    local choice = dialogue.choice({
        "I'm ready.",
        "I need to prepare first.",
    })
    coroutine.yield()

    if choice == 1 then
        world.set_flag("accepted_quest", "true")
        dialogue.show("Elder", "Then go with my blessing.")
    else
        dialogue.show("Elder", "Come back when you're prepared.")
    end
end
```

### Dialogue (`data/dialogue/*.lua`)

```lua
return {
    id = "elder_intro",
    lines = {
        { speaker = "Elder", text = "Welcome, traveler.", portrait = "elder_neutral" },
        { speaker = "Elder", text = "Dark times are upon us.", portrait = "elder_worried" },
        {
            speaker = "Elder",
            text = "Will you help?",
            portrait = "elder_hopeful",
            choices = {
                { text = "Of course.", next = "elder_accept" },
                { text = "What's in it for me?", next = "elder_negotiate" },
            },
        },
    },
}
```

### Maps

Create `.tmx` or `.tmj` files with **Tiled**. Place them wherever your asset pipeline expects (typically `assets/maps/`).

**Collision:** Add an object layer named `collision`. Draw rectangles or polygons — these become `CollisionShape` components.

**Properties:** On any Tiled object, set custom properties:
- `combat_zone` (bool) — encounter trigger area
- `trigger_script` (string) — Lua script ID to execute on player contact
- `light_radius` (float) — spawns a point light at this object's position

**Height layers:** Use Tiled layer names or properties to assign `HeightLayer` values (Ground, Objects, Bridge, Overhead) for correct z-ordering.

### Keybinds (`config/keybinds.yaml`)

```yaml
bindings:
  - action: MoveUp
    key: W
  - action: MoveDown
    key: S
  - action: MoveLeft
    key: A
  - action: MoveRight
    key: D
  - action: Confirm
    key: Return
  - action: Cancel
    key: Escape
  - action: Menu
    key: Tab
```
