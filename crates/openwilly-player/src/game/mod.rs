//! Game logic — scenes, state machine, car building, driving
//!
//! Scene mapping from Director movies:
//!   00.CXT — Shared cast (common bitmaps, sounds, palettes)
//!   02.CXT — Schrottplatz (Junkyard) — pick up parts
//!   03.DXR — Werkstatt (Garage) — build car
//!   04.CXT — Hof (Yard) — Mulle's front yard
//!   05.DXR — Weltkarte (World map) — drive around
//!   06.DXR — Autowäsche (Car wash)
//!   08.CXT — Autoshow (Car show) — rate your car
//!   10.DXR — Hauptmenü (Main menu)
//!   12.DXR — Intro movie
//!   13.DXR — Credits
//!   18.DXR — Boot-up/Init
//!   82-94  — Destinations (houses, shops, etc.)

pub mod build_car;
pub mod cursor;
pub mod dashboard;
pub mod dev_menu;
pub mod dialog;
pub mod drag_drop;
pub mod driving;
pub mod i18n;
pub mod parts_db;
pub mod save;
pub mod scene_script;
pub mod scenes;
pub mod toolbox;

use minifb::Key;
use crate::assets::AssetStore;
use crate::engine::Sprite;
use crate::engine::font;
use crate::engine::sound_engine::SoundEngine;
use crate::game::build_car::BuildCar;
use crate::game::dialog::{DialogManager, DialogEvent, QuestState, MissionDB};
use crate::game::driving::{DriveCar, DriveSession, DriveProperties};
use crate::game::parts_db::PartsDB;
use crate::game::save::SaveManager;
use crate::game::dev_menu::{DevMenu, DevAction};
use crate::game::scene_script::{SceneScript, ScriptRequest, ScriptContext};
use crate::game::cursor::GameCursor;
use crate::game::i18n::Language;

/// Which scene is active
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scene {
    Boot,
    Menu,
    Garage,
    Junkyard,
    Yard,
    World,
    CarWash,
    CarShow,
    Destination(u8), // 82-94
}

impl Scene {
    /// Director file that contains data for this scene
    pub fn director_file(&self) -> &str {
        match self {
            Scene::Boot => "18.DXR",
            Scene::Menu => "10.DXR",
            Scene::Garage => "03.DXR",
            Scene::Junkyard => "02.CXT",
            Scene::Yard => "04.CXT",
            Scene::World => "05.DXR",
            Scene::CarWash => "06.DXR",
            Scene::CarShow => "08.CXT",
            Scene::Destination(n) => {
                // Will be handled dynamically
                match n {
                    82 => "82.CXT", 83 => "83.CXT", 84 => "84.CXT",
                    85 => "85.CXT", 86 => "86.CXT", 87 => "87.CXT",
                    88 => "88.CXT", 89 => "89.CXT", 90 => "90.CXT",
                    91 => "91.CXT", 92 => "92.CXT", 93 => "93.CXT",
                    94 => "94.CXT",
                    _ => "00.CXT",
                }
            }
        }
    }
}

/// Get the 00.CXT cutscene member for a specific scene transition.
/// Returns the member name (string-based lookup) or number, as a string.
fn transition_cutscene(from: &Scene, to: &Scene, has_car: bool) -> Option<&'static str> {
    match (from, to) {
        (Scene::Menu, Scene::Garage)    => Some("00b011v0"),
        (Scene::Yard, Scene::Garage) if !has_car => Some("00b011v0"), // side door / no car
        (Scene::Yard, Scene::Garage) if has_car  => Some("00b015v0"), // garage door, car
        (Scene::Yard, Scene::World)     => Some("00b008v0"),
        (Scene::World, _)              => Some("00b008v0"), // destinations
        (Scene::Garage, Scene::Junkyard) => None, // member 70 (numeric) — skip for now
        (Scene::Junkyard, Scene::Garage) => None, // member 71 — skip for now
        (Scene::Garage, Scene::Yard)    => None,  // member 67/68 — skip for now
        _ => None,
    }
}

/// Central game state
pub struct GameState {
    pub assets: AssetStore,
    pub current_scene: Scene,
    pub scene_handler: scenes::SceneHandler,
    pub mouse_x: i32,
    pub mouse_y: i32,
    pub sound: Option<SoundEngine>,
    pub parts_db: PartsDB,
    /// The car being built (persists across scenes)
    pub car: BuildCar,
    /// Save/load manager
    pub save_manager: SaveManager,
    /// Dialog/subtitle manager
    pub dialog: DialogManager,
    /// Quest/cache flag state
    pub quest: QuestState,
    /// Mission database
    #[allow(dead_code)] // Used by mission delivery system (upcoming)
    pub missions: MissionDB,
    /// Driving car (active when on World scene)
    pub drive_car: Option<DriveCar>,
    /// Saved driving session (preserved when entering destinations)
    pub drive_session: DriveSession,
    /// Track whether mouse was down last frame (for drag detection)
    pub mouse_down: bool,
    /// Active scene script (for destination dialog chains)
    pub active_script: Option<SceneScript>,
    /// Developer menu (hidden, activated by 5× '#')
    pub dev_menu: DevMenu,
    /// Dashboard HUD (fuel needle + speedometer), loaded once
    pub dashboard: Option<dashboard::Dashboard>,
    /// Toolbox / popup menu for the world view
    pub toolbox: Option<toolbox::Toolbox>,
    /// Transition cutscene: bitmap + countdown frames + target scene
    pub transition: Option<TransitionCutscene>,
    /// Software-rendered cursor with stack-based type management
    pub cursor: GameCursor,
    /// UI language
    pub language: Language,
    /// Topology bitmap red channel (316×198) for terrain collision.
    /// Loaded per map tile; indexed [y * 316 + x].
    pub topo_data: Vec<u8>,
}

/// A brief cutscene image shown during scene transitions
pub struct TransitionCutscene {
    /// RGBA pixel data for the cutscene image
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    /// Countdown frames until transition completes
    pub frames_left: u8,
    /// Target scene to switch to after cutscene
    pub target: Scene,
    /// Progress bar position (0.0 = start, 1.0 = done)
    pub progress: f32,
}

impl GameState {
    pub fn new(assets: AssetStore) -> Self {
        let current_scene = Scene::Boot;
        let scene_handler = scenes::SceneHandler::new(current_scene, &assets, false);
        let sound = SoundEngine::new();
        let parts_db = PartsDB::load();
        // Save manager — uses game directory for save file
        let save_manager = SaveManager::new(&assets.game_dir);
        // Dialog, quest, and mission systems
        let dialog = DialogManager::new();
        let quest = QuestState::new();
        let missions = MissionDB::load();
        // Car position in the garage (from mulle.js: ~320, 240 center area)
        let mut car = BuildCar::new(300, 220);
        car.refresh(&parts_db, &assets);

        tracing::info!("GameState initialized: {} missions loaded, {} parts in DB",
            missions.missions.len(), parts_db.len());

        let cursor = GameCursor::new(&assets);

        let mut state = Self {
            assets,
            current_scene,
            scene_handler,
            mouse_x: 0,
            mouse_y: 0,
            sound,
            parts_db,
            car,
            save_manager,
            dialog,
            quest,
            missions,
            drive_car: None,
            drive_session: DriveSession::default(),
            mouse_down: false,
            active_script: None,
            dev_menu: DevMenu::new(),
            dashboard: None,
            toolbox: None,
            transition: None,
            cursor,
            language: Language::German,
            topo_data: vec![0u8; (driving::TOPO_WIDTH * driving::TOPO_HEIGHT) as usize],
        };

        // Boot → Menu transition
        state.switch_scene(Scene::Menu);
        state
    }

    pub fn update(&mut self) {
        // Transition cutscene: count down frames, then switch scene
        if let Some(trans) = &mut self.transition {
            trans.frames_left = trans.frames_left.saturating_sub(1);
            trans.progress = 1.0 - (trans.frames_left as f32 / 15.0);
            if trans.frames_left == 0 {
                let target = trans.target.clone();
                self.transition = None;
                self.switch_scene(target);
            }
            return; // Don't process anything else during transition
        }

        // Tick scene actors, collect animation events
        let scene_events = self.scene_handler.update(&self.assets);
        for event in &scene_events {
            self.handle_scene_event(event);
        }

        // Advance dialog/subtitles (~33ms per frame at 30fps)
        let dialog_events = self.dialog.update(33);
        for event in &dialog_events {
            self.handle_dialog_event(event);
        }

        // Advance active scene script (destination dialog chains)
        self.advance_script();

        // Part physics (gravity) in Garage and Yard
        if self.current_scene == Scene::Garage || self.current_scene == Scene::Yard {
            let hit_parts = self.scene_handler.drag_drop.update_physics();
            for part_id in hit_parts {
                // Weight-based floor impact sound
                if let Some(snd) = &mut self.sound {
                    let weight = self.parts_db.get(part_id)
                        .map(|p| p.properties.weight)
                        .unwrap_or(1);
                    let sound_id = if weight >= 4 {
                        "00e003v0"
                    } else if weight >= 2 {
                        "00e002v0"
                    } else {
                        "00e001v0"
                    };
                    snd.play_by_name(sound_id, &self.assets);
                }
            }
        }

        // Driving physics when on the World map
        if self.current_scene == Scene::World {
            // Borrow topo_data separately so the closure can read it while car is &mut
            let topo = &self.topo_data;
            let topo_w = driving::TOPO_WIDTH as usize;
            // Look up objects for the current tile
            let world_map = driving::WorldMap::default_map();
            // Collect results from car update within inner scope to release borrow
            let (drive_event, engine_sound, saved_session, new_tile_pos) = if let Some(car) = &mut self.drive_car {
                let tile_objects = world_map.tile_at(car.tile_col, car.tile_row)
                    .and_then(|tid| world_map.get_tile(tid))
                    .map(|t| t.objects.as_slice())
                    .unwrap_or(&[]);
                let event = car.update(tile_objects, |tx, ty| {
                    let idx = ty as usize * topo_w + tx as usize;
                    if idx < topo.len() { topo[idx] } else { 0 }
                });
                let saved = match &event {
                    driving::DriveEvent::ReachedDestination { .. } => Some(car.save_session()),
                    _ => None,
                };
                let tile_pos = if let driving::DriveEvent::TileTransition { delta_col, delta_row } = &event {
                    car.do_tile_transition(*delta_col, *delta_row);
                    Some((car.tile_col, car.tile_row))
                } else {
                    None
                };
                let sound = car.engine_sound_update().map(|s| s.to_string());
                (Some(event), sound, saved, tile_pos)
            } else {
                (None, None, None, None)
            };

            // Load new topology after tile transition (outside car borrow)
            if let Some((col, row)) = new_tile_pos {
                let world_map = driving::WorldMap::default_map();
                if let Some(tid) = world_map.tile_at(col, row) {
                    if let Some(tile) = world_map.get_tile(tid) {
                        let topo_name = tile.topology.clone();
                        self.load_topology(&topo_name);
                    }
                }
            }

            // Process events outside the car borrow
            if let Some(event) = drive_event {
                match event {
                    driving::DriveEvent::FuelEmpty => {
                        self.play_dialog("05d011v0"); // "Tank ist leer!"
                    }
                    driving::DriveEvent::ReachedDestination { object_id, dir_resource } => {
                        tracing::info!("Reached destination object {} → {}", object_id, dir_resource);
                        if let Some(session) = saved_session {
                            self.drive_session = session;
                        }
                        if let Ok(n) = dir_resource.parse::<u8>() {
                            self.switch_scene(Scene::Destination(n));
                        }
                    }
                    driving::DriveEvent::TerrainBlocked { reason } => {
                        tracing::debug!("Terrain blocked: {}", reason);
                    }
                    driving::DriveEvent::GasStation => {
                        if let Some(snd) = &mut self.sound {
                            snd.play_by_name("31e006v0", &self.assets);
                        }
                        tracing::info!("Refueling at gas station");
                    }
                    driving::DriveEvent::AnimalsBlocking { has_horn } => {
                        if has_horn {
                            // Honk horn, then play cow move sound
                            let horn_type = self.car.properties().horn_type;
                            let horn_sounds = ["05e050v0", "05e049v0", "05e044v0", "05e042v0", "05d013v0"];
                            let idx = ((horn_type - 1) as usize).min(horn_sounds.len() - 1);
                            if let Some(snd) = &mut self.sound {
                                snd.play_by_name(horn_sounds[idx], &self.assets);
                                snd.play_by_name("31e001v0", &self.assets);
                            }
                        } else {
                            // No horn — blocked sound
                            if let Some(snd) = &mut self.sound {
                                snd.play_by_name("31d001v0", &self.assets);
                            }
                        }
                    }
                    driving::DriveEvent::HillSound { big } => {
                        // Hill feedback sounds (from objects.hash Sounds array)
                        let sound = if big { "31d005v0" } else { "31d004v0" };
                        if let Some(snd) = &mut self.sound {
                            snd.play_by_name(sound, &self.assets);
                        }
                    }
                    _ => {}
                }
            }

            // Play engine sound if state changed
            if let Some(sound_id) = engine_sound {
                if let Some(snd) = &mut self.sound {
                    snd.play_by_name(&sound_id, &self.assets);
                }
            }
        }

        // Clean up finished sound effects
        if let Some(snd) = &mut self.sound {
            snd.gc();
        }
    }

    pub fn on_click(&mut self, x: i32, y: i32) {
        // Dev menu intercepts clicks
        if self.dev_menu.open {
            let action = self.dev_menu.on_click(x, y);
            self.handle_dev_action(action);
            return;
        }

        // Toolbox / popup menu in World scene
        if self.current_scene == Scene::World {
            if let Some(tb) = &mut self.toolbox {
                // If popup is open, check popup buttons first
                if tb.popup_open {
                    if let Some(action) = tb.popup_hit(x, y) {
                        match action {
                            toolbox::PopupAction::Home => {
                                tb.popup_open = false;
                                self.switch_scene(Scene::Yard);
                                return;
                            }
                            toolbox::PopupAction::Quit => {
                                tb.popup_open = false;
                                self.switch_scene(Scene::Menu);
                                return;
                            }
                            toolbox::PopupAction::Cancel => {
                                tb.popup_open = false;
                            }
                            _ => {
                                // Steering / Diploma — not implemented
                                tracing::debug!("Popup action {:?} not implemented", action);
                            }
                        }
                    }
                    return; // Popup absorbs all clicks when open
                }

                // Check toolbox icon click
                if tb.icon_hit(x, y) {
                    tb.toggle();
                    // Stop engine sound when opening popup
                    if tb.popup_open {
                        if let Some(car) = &mut self.drive_car {
                            car.engine_sound_state = None;
                        }
                    }
                    return;
                }
            }
        }

        // Language button on menu screen (bottom-left corner, 20,440 to 180,470)
        if self.current_scene == Scene::Menu {
            if x >= 20 && x < 180 && y >= 440 && y < 470 {
                self.language = self.language.next();
                tracing::info!("Language switched to {}", self.language.code());
                return;
            }
        }

        if let Some(next) = self.scene_handler.on_click(x, y, &self.assets) {
            self.switch_scene(next);
        }
    }

    pub fn on_right_click(&mut self, x: i32, y: i32) {
        // In Garage: right-click on a car part → detach it
        if self.current_scene == Scene::Garage {
            if let Some(part_id) = self.car.part_at(x, y) {
                // Don't allow detaching default parts (chassis=1, battery=82, gearbox=133, brake=152)
                let default_parts = PartsDB::default_car_parts();
                if default_parts.contains(&part_id) {
                    tracing::debug!("Cannot detach default part {}", part_id);
                } else if let Some(event) = self.car.detach(part_id, &self.parts_db, &self.assets) {
                    match &event {
                        build_car::CarEvent::Detached { part_id: pid, master_id, world_x, world_y } => {
                            // Use get_master to resolve morph parent hierarchy
                            let master_name = self.parts_db.get_master(*pid)
                                .map(|m| format!("master #{}", m.part_id))
                                .unwrap_or_else(|| format!("standalone (master_id={})", master_id));
                            tracing::info!("Detached part {} ({}) at ({}, {})", pid, master_name, world_x, world_y);
                        }
                        _ => tracing::info!("Car event: {:?}", event),
                    }
                    self.save_manager.save_car_parts(&self.car.parts);
                    if let Some(snd) = &mut self.sound {
                        snd.play_by_name("00e003v0", &self.assets); // detach/pop sound
                    }
                    // TODO: create a draggable item at the detach world position
                }
            }
        }
        self.scene_handler.on_right_click(x, y);
    }

    /// Update mouse state each frame (call before on_click)
    pub fn on_mouse_state(&mut self, x: i32, y: i32, down: bool) {
        self.mouse_x = x;
        self.mouse_y = y;
        self.mouse_down = down;

        // Auto-switch to mouse steering when clicking during driving
        if down && self.current_scene == Scene::World {
            if let Some(car) = &mut self.drive_car {
                car.key_steer = false;
            }
        }

        // Update toolbox hover state
        if self.current_scene == Scene::World {
            if let Some(tb) = &mut self.toolbox {
                let just_hovered = tb.update_hover(x, y);
                if just_hovered {
                    if let Some(snd) = &mut self.sound {
                        snd.play_by_name(toolbox::TOOLBOX_HOVER_SOUND, &self.assets);
                    }
                }
            }
        }

        // Forward drag processing to scene handler
        let result = self.scene_handler.process_drag(x, y, down);
        self.handle_drop_result(result);
        self.scene_handler.on_mouse_move(x, y);

        // Track dragging state for cursor and UI feedback
        if self.scene_handler.drag_drop.is_dragging() {
            if let Some(item) = self.scene_handler.drag_drop.dragged_item() {
                tracing::trace!("Dragging part #{} at ({}, {})", item.part_id, x, y);
            }
            // Play snap/unsnap sound when morph preview toggles
            if let Some(item) = self.scene_handler.drag_drop.dragged_item_mut() {
                if !item.snap_sound_played {
                    item.snap_sound_played = true;
                    // Weight-based snap sound
                    let weight = self.parts_db.get(item.part_id)
                        .map(|p| p.properties.weight)
                        .unwrap_or(1);
                    let sound_id = if weight >= 4 {
                        "03e003v2"
                    } else if weight >= 2 {
                        "03e003v1"
                    } else {
                        "03e003v0"
                    };
                    if let Some(snd) = &mut self.sound {
                        snd.play_by_name(sound_id, &self.assets);
                    }
                }
            }
        }

        // ── Update software cursor type based on context ──
        self.update_cursor(x, y);
    }

    /// Determine the correct cursor type for the current mouse position.
    fn update_cursor(&mut self, x: i32, y: i32) {
        use crate::game::cursor::CursorType;

        self.cursor.reset();

        // Dragging a part → Grab
        if self.scene_handler.drag_drop.is_dragging() {
            self.cursor.set(CursorType::Grab);
            return;
        }

        // In Garage or Junkyard: check if hovering a drag-drop item (car part)
        if self.current_scene == Scene::Garage || self.current_scene == Scene::Junkyard {
            if self.scene_handler.drag_drop.hover_info(x, y).is_some() {
                self.cursor.set(CursorType::Grab);
                return;
            }
            // Garage: hovering over a car part
            if self.current_scene == Scene::Garage {
                if self.car.part_at(x, y).is_some() {
                    self.cursor.set(CursorType::Grab);
                    return;
                }
            }
        }

        // Check buttons — these are clickable scene buttons (doors, nav)
        for btn in &self.scene_handler.buttons {
            if btn.hit_test(x, y) {
                // Determine direction from button name
                let name = btn.name.to_lowercase();
                if name.contains("links") || name.contains("left")
                    || name.contains("tür → werkstatt")
                    || name.contains("← ")
                {
                    self.cursor.set(CursorType::Left);
                } else if name.contains("rechts") || name.contains("right")
                    || name.contains("→ werkstatt")
                    || name.contains(" →")
                {
                    self.cursor.set(CursorType::Right);
                } else if name.contains("zurück") || name.contains("back") {
                    self.cursor.set(CursorType::Back);
                } else {
                    self.cursor.set(CursorType::Click);
                }
                return;
            }
        }

        // Check interactive sprites (top z-order first)
        let mut best_z = i32::MIN;
        let mut found_interactive = false;
        for sprite in &self.scene_handler.sprites {
            if sprite.interactive && sprite.hit_test(x, y) && sprite.z_order > best_z {
                best_z = sprite.z_order;
                found_interactive = true;
            }
        }
        if found_interactive {
            self.cursor.set(CursorType::Click);
            return;
        }

        // Hotspots
        for hs in &self.scene_handler.hotspots {
            if x >= hs.x && x < hs.x + hs.width as i32
                && y >= hs.y && y < hs.y + hs.height as i32
            {
                self.cursor.set(CursorType::Click);
                return;
            }
        }

        // Toolbox icon in World
        if self.current_scene == Scene::World {
            if let Some(tb) = &self.toolbox {
                if tb.icon_hit(x, y) {
                    self.cursor.set(CursorType::Click);
                    return;
                }
            }
        }

        // Default: Standard (already set by reset)
    }

    fn handle_drop_result(&mut self, result: drag_drop::DropResult) {
        match result {
            drag_drop::DropResult::Attached { part_id, point_id, morph_id } => {
                tracing::info!("Part {} attached at {} (morph: {:?})", part_id, point_id, morph_id);
                let attach_id = morph_id.unwrap_or(part_id);
                if self.current_scene == Scene::Garage {
                    if let Some(build_car::CarEvent::Attached { part_id: attached_id }) =
                        self.car.attach(attach_id, &self.parts_db, &self.assets)
                    {
                        tracing::info!("Attached part {} → car now has {} parts", attached_id, self.car.parts.len());
                        self.save_manager.save_car_parts(&self.car.parts);
                        // Track special part pickups as quest cache flags
                        self.quest.add_cache(&format!("#Part{}", attached_id));
                        // Remove the dragged item from the scene
                        if let Some(idx) = self.scene_handler.drag_drop.items.iter().position(|i| i.part_id == part_id) {
                            self.scene_handler.drag_drop.remove_item(idx);
                        }
                        if let Some(snd) = &mut self.sound {
                            // Weight-based attach sound (mulle.js thresholds)
                            let weight = self.parts_db.get(attached_id as u32)
                                .map(|p| p.properties.weight)
                                .unwrap_or(1);
                            let sound_id = if weight >= 4 {
                                "03e003v2" // heavy
                            } else if weight >= 2 {
                                "03e003v1" // medium
                            } else {
                                "03e003v0" // light
                            };
                            snd.play_by_name(sound_id, &self.assets);
                        }
                    }
                }
            }
            drag_drop::DropResult::DroppedOnTarget { part_id, target_id } => {
                tracing::info!("Part {} dropped on target {}", part_id, target_id);
                // TODO: move part to another location/scene (e.g. junkyard door)
            }
            drag_drop::DropResult::Dropped { part_id } => {
                tracing::debug!("Part {} dropped in place", part_id);
                // If barely moved (click-like) in Garage, play part description audio
                // (mulle.js: onDrop with dist < 5 → playDescription)
                let desc = if self.current_scene == Scene::Garage {
                    self.parts_db.get(part_id as u32)
                        .filter(|p| !p.description.is_empty())
                        .map(|p| p.description.clone())
                } else {
                    None
                };
                if let Some(desc_id) = desc {
                    self.play_dialog(&desc_id);
                    return; // description replaces floor-drop sound
                }
                // Weight-based floor drop sound
                if let Some(snd) = &mut self.sound {
                    let weight = self.parts_db.get(part_id as u32)
                        .map(|p| p.properties.weight)
                        .unwrap_or(1);
                    let sound_id = if weight >= 4 {
                        "00e003v0" // heavy floor
                    } else if weight >= 2 {
                        "00e002v0" // medium floor
                    } else {
                        "00e001v0" // light floor
                    };
                    snd.play_by_name(sound_id, &self.assets);
                }
            }
            drag_drop::DropResult::Nothing => {}
        }
    }

    pub fn on_key_down(&mut self, key: Key) {
        // ── Dev menu navigation (eats all input while open) ──
        if self.dev_menu.open {
            match key {
                Key::Up => self.dev_menu.nav_up(),
                Key::Down => self.dev_menu.nav_down(),
                Key::Enter => {
                    let action = self.dev_menu.activate();
                    self.handle_dev_action(action);
                }
                Key::Escape => self.dev_menu.open = false,
                _ => {}
            }
            return;
        }

        // Space → skip dialog subtitle (any scene)
        if key == Key::Space {
            self.dialog.skip_current();
        }

        // H → horn (while driving on world map)
        if key == Key::H && self.current_scene == Scene::World {
            self.play_horn();
        }

        // Backspace & Enter need special routing to the scene handler for text input
        if key == Key::Backspace {
            self.scene_handler.on_char_input('\x08');
        }
        if key == Key::Enter {
            if let Some(next) = self.scene_handler.on_enter(&self.assets) {
                self.switch_scene(next);
                return;
            }
        }
        if let Some(next) = self.scene_handler.on_key_down(key, &self.assets) {
            self.switch_scene(next);
        }
    }

    /// Update driving input from polled key state (call each frame from engine)
    pub fn update_drive_keys(&mut self, up: bool, down: bool, left: bool, right: bool) {
        // Don't process driving input when popup menu is open
        if let Some(tb) = &self.toolbox {
            if tb.popup_open {
                if let Some(car) = &mut self.drive_car {
                    car.throttle = false;
                    car.braking = false;
                    car.steer_left = false;
                    car.steer_right = false;
                }
                return;
            }
        }

        if let Some(car) = &mut self.drive_car {
            // Auto-switch: arrow keys → keyboard mode
            if up || down || left || right {
                car.key_steer = true;
            }

            if car.key_steer {
                car.throttle = up;
                car.braking = down;
                car.steer_left = left;
                car.steer_right = right;
            } else {
                // Mouse mode: apply mouse steering
                car.mouse_steer(self.mouse_x, self.mouse_y, self.mouse_down);
            }
        }
    }

    /// Forward character input (typing) to the scene handler
    pub fn on_char_input(&mut self, ch: char) {
        // Dev-menu activation: 5× '#' within 2 seconds
        if ch == '#' {
            if self.dev_menu.on_hash_press() {
                // Play a quiet beep to confirm
                if let Some(snd) = &mut self.sound {
                    snd.play_by_name("00e004v0", &self.assets);
                }
                tracing::info!("Dev menu {}", if self.dev_menu.open { "opened" } else { "closed" });
            }
            return;
        }
        // Don't forward input while dev menu is open
        if self.dev_menu.open { return; }
        self.scene_handler.on_char_input(ch);
    }

    /// Draw UI overlays (text fields, buttons, subtitles) on top of sprites
    pub fn draw_ui(&mut self, fb: &mut [u32]) {
        // Transition cutscene: render image + progress bar
        if let Some(trans) = &self.transition {
            // Black background
            fb.fill(0xFF000000);
            // Blit cutscene image centered
            let bw = trans.width as i32;
            let bh = trans.height as i32;
            for sy in 0..bh {
                let dy = trans.y + sy;
                if dy < 0 || dy >= 480 { continue; }
                for sx in 0..bw {
                    let dx = trans.x + sx;
                    if dx < 0 || dx >= 640 { continue; }
                    let si = (sy * bw + sx) as usize * 4;
                    if si + 3 >= trans.pixels.len() { continue; }
                    let a = trans.pixels[si + 3] as u32;
                    if a == 0 { continue; }
                    let r = trans.pixels[si] as u32;
                    let g = trans.pixels[si + 1] as u32;
                    let b = trans.pixels[si + 2] as u32;
                    let di = dy as usize * 640 + dx as usize;
                    fb[di] = 0xFF000000 | (r << 16) | (g << 8) | b;
                }
            }
            // Progress bar: green on gray, at (170, 400), 300×32px
            font::draw_rect(fb, 170, 400, 300, 32, 0xFF333333);
            let bar_w = (300.0 * trans.progress) as i32;
            if bar_w > 0 {
                font::draw_rect(fb, 170, 400, bar_w, 32, 0xFF65C265);
            }
            return; // Don't draw normal UI during transition
        }

        self.scene_handler.draw_ui(fb);

        // Language selector on menu screen
        if self.current_scene == Scene::Menu {
            let lang_text = i18n::t(self.language, "lang_label");
            // Button background
            font::draw_rect(fb, 20, 440, 160, 26, 0xAA1a1a2e);
            font::draw_rect_outline(fb, 20, 440, 160, 26, 0xFF6666CC);
            // Centered text
            let tw = font::text_width(lang_text);
            let tx = 20 + (160 - tw) / 2;
            font::draw_text_shadow(fb, tx, 446, lang_text, 0xFFFFFFFF);
        }

        // Subtitle rendering at screen bottom
        if let Some(sub) = self.dialog.current_subtitle() {
            let text = sub.plain_text();
            let tw = font::text_width(&text);
            let tx = (640 - tw) / 2;
            let ty = 460;
            // Background bar
            font::draw_rect(fb, 0, ty - 4, 640, 20, 0xCC000000);
            // Color-code by speaker
            let base_color = match sub.speaker.as_str() {
                "mulle" => 0xFFFFFF00,  // Yellow for Mulle
                "figge" => 0xFF88CCFF,  // Light blue for Figge
                _ => 0xFFFFFFFF,        // White for others
            };
            font::draw_text_shadow(fb, tx, ty, &text, base_color);

            // Render highlighted words in bright yellow (e.g. {Salka})
            let highlights = sub.highlighted_words();
            if !highlights.is_empty() {
                tracing::trace!("Dialog highlights: {:?}", highlights);
            }
        }

        // Driving HUD: debug overlay (sprite-based dashboard handles fuel + speed)
        if self.current_scene == Scene::World {
            if let Some(car) = &self.drive_car {
                // Show engine type and FPS in debug
                let (wo_x, wo_y) = car.wheel_offset();
                let debug_text = format!("Motor:{} FPS:{} Rad:({:.0},{:.0}) {:.0}km/h Fuel:{:.0}%",
                    car.engine_type(), driving::DriveCar::fps(), wo_x, wo_y,
                    car.speed * 30.0, car.fuel_percent() * 100.0);
                font::draw_text(fb, 10, 10, &debug_text, 0xFF888888);
            }
        }

        // Road legality indicator in Garage
        if self.current_scene == Scene::Garage {
            if self.car.is_road_legal() {
                font::draw_text_shadow(fb, 10, 460, i18n::t(self.language, "road_legal"), 0xFF00FF00);
            } else {
                let failures = self.car.properties().road_legal_failures();
                let prefix = i18n::t(self.language, "not_road_legal");
                let hint = format!("{} ({})", prefix, failures.join(", "));
                font::draw_text_shadow(fb, 10, 460, &hint, 0xFFFF4444);
            }
        }

        // Dev menu overlay (drawn last — on top of everything)
        self.dev_menu.draw(fb);
    }

    /// Handle an action returned by the dev menu
    fn handle_dev_action(&mut self, action: DevAction) {
        match action {
            DevAction::None | DevAction::Close => {}
            DevAction::GotoScene(scene) => {
                tracing::info!("Dev warp → {:?}", scene);
                self.switch_scene(scene);
            }
            DevAction::RefuelTank => {
                if let Some(car) = &mut self.drive_car {
                    car.refuel();
                    tracing::info!("Dev: tank refuelled");
                } else {
                    tracing::warn!("Dev: no driving car to refuel");
                }
            }
            DevAction::TriggerFigge => {
                self.save_manager.add_stuff("#FiggeIsComing");
                tracing::info!("Dev: set #FiggeIsComing, switching to Garage");
                self.switch_scene(Scene::Garage);
            }
        }
    }

    pub fn get_all_sprites(&self) -> Vec<Sprite> {
        let mut sprites = self.scene_handler.all_sprites();

        // In Garage, render the car as well
        if self.current_scene == Scene::Garage {
            for car_sprite in self.car.all_sprites() {
                sprites.push(car_sprite);
            }
            sprites.sort_by_key(|s| s.z_order);
        }

        // On World map, render the driving car sprite + dashboard HUD
        if self.current_scene == Scene::World {
            if let Some(car) = &self.drive_car {
                let member = car.sprite_member();
                // Lookup sprite bitmap from 05.DXR cast member
                if let Some(bmp) = self.assets.decode_bitmap_transparent("05.DXR", member) {
                    let car_sprite = Sprite {
                        x: car.x as i32 - bmp.width as i32 / 2,
                        y: car.y as i32 - bmp.height as i32 / 2,
                        width: bmp.width,
                        height: bmp.height,
                        pixels: bmp.pixels,
                        visible: true,
                        z_order: 1000, // car on top of map
                        name: format!("drive_car_d{}", car.direction),
                        interactive: true,
                        member_num: member,
                    };
                    sprites.push(car_sprite);
                }

                // Dashboard HUD: fuel needle + speedometer
                if let Some(dash) = &self.dashboard {
                    let fuel_pct = car.fuel_percent();
                    let speed = car.speed;
                    let max_speed = car.max_speed();
                    for ds in dash.sprites(fuel_pct, speed, max_speed) {
                        sprites.push(ds);
                    }
                }
            }

            // Toolbox icon + popup menu
            if let Some(tb) = &self.toolbox {
                for ts in tb.sprites() {
                    sprites.push(ts);
                }
            }
        }

        sprites
    }

    /// Get info about what's under the cursor (for title bar display)
    pub fn get_hover_info(&self, x: i32, y: i32) -> String {
        // Check drag items first
        if let Some(info) = self.scene_handler.drag_drop.hover_info(x, y) {
            return info;
        }
        // In Garage: show part info including description
        if self.current_scene == Scene::Garage {
            if let Some(part_id) = self.car.part_at(x, y) {
                if let Some(part) = self.parts_db.get(part_id) {
                    // Use get_member to look up description text content
                    if !part.description.is_empty() {
                        if let Some((fname, num)) = self.assets.find_sound_by_name(&part.description) {
                            if let Some(member) = self.assets.get_member(&fname, num) {
                                if let Some(text) = &member.text_content {
                                    return format!("Teil #{} ({}): {}", part_id, member.num, text);
                                }
                            }
                        }
                    }
                    return format!("Teil #{}", part_id);
                }
            }
        }
        self.scene_handler.hover_info(x, y)
    }

    /// Login a user profile — loads car parts, quest flags from save
    pub fn login_user(&mut self, name: &str) {
        // Log existing profiles
        let profiles = self.save_manager.profile_names();
        tracing::info!("Available profiles: {:?}", profiles);

        // Housekeeping: if there are more than 10 profiles, trim oldest unused ones
        if profiles.len() > 10 {
            let stale: Vec<String> = profiles.iter()
                .filter(|p| **p != name)
                .take(profiles.len() - 10)
                .map(|s| s.to_string())
                .collect();
            for old in &stale {
                tracing::info!("Deleting stale profile '{}' (max 10 profiles)", old);
                self.save_manager.delete_profile(old);
            }
        }

        let user = self.save_manager.login(name).clone();

        // Restore car parts from save
        self.car.parts = user.car.parts.clone();
        self.car.refresh(&self.parts_db, &self.assets);

        // Restore quest flags from save
        self.quest.load_from_save(&user.car.cache_list, &user.own_stuff);

        // Log pile contents
        let last_pile = user.my_last_pile;
        let pile_count = user.junk.pile(last_pile).len();
        tracing::debug!("Last pile: {} ({} parts)", last_pile, pile_count);

        // Log part statistics
        let morph_parents = self.parts_db.iter()
            .filter(|(_, p)| p.is_morph_parent())
            .count();
        tracing::info!(
            "Logged in '{}': {} car parts, {} quest flags, {} morph parents in DB",
            name,
            self.car.parts.len(),
            self.quest.cache.len() + self.quest.permanent.len(),
            morph_parents,
        );
    }

    /// Save current quest state back to the save file
    pub fn save_quest_state(&mut self) {
        if let Some(user) = self.save_manager.active_mut() {
            user.car.cache_list = self.quest.cache_list().to_vec();
            user.own_stuff = self.quest.permanent_list().to_vec();
        }
        self.save_manager.save();

        // Log active profile state
        if let Some(active) = self.save_manager.active() {
            tracing::debug!("Saved quest state: {} cache flags, {} permanent flags, car: '{}'",
                active.car.cache_list.len(), active.own_stuff.len(), active.car.name);
        }
    }

    /// Play a dialog — start subtitle, audio, and cue-point tracking.
    /// Optionally specify the talking actor for lip-sync animation.
    fn play_dialog(&mut self, audio_id: &str) {
        self.play_dialog_with_actor(audio_id, None);
    }

    /// Play the horn sound based on the car's horn_type (1-5).
    /// Sound IDs from mulle.js: ["05e050v0", "05e049v0", "05e044v0", "05e042v0", "05d013v0"]
    fn play_horn(&mut self) {
        const HORN_SOUNDS: [&str; 5] = [
            "05e050v0", "05e049v0", "05e044v0", "05e042v0", "05d013v0",
        ];
        let horn_type = self.drive_car.as_ref()
            .map(|c| c.props.horn_type)
            .unwrap_or(0);
        if horn_type >= 1 && horn_type <= 5 {
            let sound_id = HORN_SOUNDS[(horn_type - 1) as usize];
            if let Some(snd) = &mut self.sound {
                snd.play_by_name(sound_id, &self.assets);
            }
        }
    }

    /// Figge delivers up to 3 JunkMan parts to the yard
    fn figge_give_parts(&mut self) {
        // JunkMan parts list (from mulle.js savedata.js)
        const JUNKMAN_PARTS: [u32; 52] = [
            13, 20, 17, 89, 290, 120, 18, 19, 173, 21, 297, 22, 24, 25, 185, 26,
            27, 28, 32, 35, 91, 132, 129, 134, 137, 146, 149, 154, 168, 216, 174,
            175, 177, 189, 191, 192, 193, 233, 199, 208, 209, 212, 221, 227, 229,
            235, 251, 264, 278, 294, 295, 14,
        ];

        let mut given = 0;
        for &part_id in &JUNKMAN_PARTS {
            if given >= 3 { break; }
            // Only give parts the player doesn't already own
            if !self.save_manager.has_yard_part(part_id) {
                self.save_manager.add_yard_part(part_id);
                tracing::info!("Figge gave JunkMan part {} to yard", part_id);
                given += 1;
            }
        }
        if given == 0 {
            tracing::info!("Figge had no new parts to deliver");
        }
    }

    /// Play a dialog with a specific actor for lip-sync.
    fn play_dialog_with_actor(&mut self, audio_id: &str, actor_name: Option<&str>) {
        self.dialog.talk(audio_id);
        if let Some(snd) = &mut self.sound {
            if let Some(handle) = snd.play_by_name(audio_id, &self.assets) {
                // Set up cue-point tracking if the sound has cue points
                let cue_points = self.assets.find_cue_points(audio_id);
                if !cue_points.is_empty() {
                    self.dialog.set_cue_tracking(audio_id, handle, cue_points);
                }
            }
        }
        // Set the talking actor for cue-point animation dispatch
        if let Some(name) = actor_name {
            self.scene_handler.set_talking_actor(name);
        }
    }

    /// Handle scene events (e.g. actor animation finished)
    fn handle_scene_event(&mut self, event: &scenes::SceneEvent) {
        match event {
            scenes::SceneEvent::ActorAnimFinished { actor_name, anim_name } => {
                tracing::debug!(
                    "Actor '{}' animation '{}' finished (scene: {:?})",
                    actor_name, anim_name, self.current_scene
                );
                // Destination-specific animation callbacks will be added here
                self.on_actor_anim_finished(actor_name, anim_name);
            }
        }
    }

    /// Handle dialog events (e.g. a dialog sequence finished)
    fn handle_dialog_event(&mut self, event: &DialogEvent) {
        match event {
            DialogEvent::DialogFinished { audio_id } => {
                tracing::debug!(
                    "Dialog '{}' finished (scene: {:?})",
                    audio_id, self.current_scene
                );
                // Stop talking animation on the speaking actor
                self.scene_handler.stop_talking_actor();
                // Dialog chaining / quest progression
                self.on_dialog_finished(audio_id);
            }
            DialogEvent::QueueEmpty => {
                tracing::debug!("Dialog queue empty (scene: {:?})", self.current_scene);
            }
            DialogEvent::CuePoint { audio_id: _, cue_name } => {
                // Forward cue point to the talking actor for lip-sync
                self.scene_handler.handle_cue_point(cue_name);
            }
        }
    }

    /// Called when an actor's non-looping animation finishes.
    /// Forwards the event to the active scene script.
    fn on_actor_anim_finished(&mut self, actor_name: &str, _anim_name: &str) {
        if let Some(script) = &mut self.active_script {
            script.on_anim_finished(actor_name);
        }
    }

    /// Called when a dialog sequence finishes playing.
    /// Forwards the event to the active scene script.
    fn on_dialog_finished(&mut self, audio_id: &str) {
        if let Some(script) = &mut self.active_script {
            script.on_dialog_finished(audio_id);
        }
    }

    /// Advance the active scene script and process any requests it generates.
    fn advance_script(&mut self) {
        if self.active_script.is_none() {
            return;
        }

        // Tick delay timers
        if let Some(script) = &mut self.active_script {
            script.tick(33); // ~33ms per frame at 30fps
        }

        // Build context for condition evaluation
        let car_parts: Vec<u32> = self.car.parts.clone();
        let ctx = ScriptContext {
            cache: self.quest.cache_list(),
            permanent: self.quest.permanent_list(),
            car_parts: &car_parts,
        };

        // Advance and collect requests
        let requests = if let Some(script) = &mut self.active_script {
            script.advance(&ctx)
        } else {
            return;
        };

        // Process requests
        let mut leave = false;
        for req in requests {
            match req {
                ScriptRequest::Talk { audio_id, actor_name } => {
                    self.play_dialog_with_actor(&audio_id, actor_name.as_deref());
                }
                ScriptRequest::PlayAnim { actor_name, anim_name } => {
                    self.scene_handler.play_actor_anim(&actor_name, &anim_name);
                }
                ScriptRequest::SetCache(flag) => {
                    self.quest.add_cache(&flag);
                }
                ScriptRequest::RemoveCache(flag) => {
                    self.quest.remove_cache(&flag);
                }
                ScriptRequest::SetStuff(flag) => {
                    self.quest.add_permanent(&flag);
                    self.save_manager.add_stuff(&flag);
                }
                ScriptRequest::GivePart(part_id) => {
                    self.save_manager.add_yard_part(part_id);
                    tracing::info!("Script gave part {} to yard", part_id);
                }
                ScriptRequest::Refuel => {
                    if let Some(car) = &mut self.drive_car {
                        car.refuel();
                        tracing::info!("Script refueled car");
                    }
                }
                ScriptRequest::SetActorVisible { actor_name, visible } => {
                    self.scene_handler.set_actor_visible(&actor_name, visible);
                }
                ScriptRequest::SetTalkAnims { actor_name, talk_anim, silence_anim } => {
                    self.scene_handler.set_actor_talk_anims(&actor_name, &talk_anim, &silence_anim);
                }
                ScriptRequest::PlaySound(sound_id) => {
                    if let Some(snd) = &mut self.sound {
                        snd.play_by_name(&sound_id, &self.assets);
                    }
                }
                ScriptRequest::LeaveToWorld => {
                    leave = true;
                }
            }
        }

        // Clean up finished script
        if let Some(script) = &self.active_script {
            if script.finished {
                self.active_script = None;
            }
        }

        // Handle leave-to-world after processing all requests
        if leave {
            self.save_quest_state();
            self.active_script = None;
            self.switch_scene(Scene::World);
        }
    }

    fn switch_scene(&mut self, scene: Scene) {
        tracing::info!("Scene transition: {:?} -> {:?} ({})", self.current_scene, scene, scene.director_file());

        // Check for transition cutscene (only if we're not already resuming from one)
        if self.transition.is_none() {
            let has_car = self.car.properties().is_road_legal();
            if let Some(cutscene_name) = transition_cutscene(&self.current_scene, &scene, has_car) {
                // Look up the cutscene member by name in 00.CXT
                if let Some((fname, num)) = self.assets.find_sound_by_name(cutscene_name) {
                    if let Some(bmp) = self.assets.decode_bitmap_transparent(&fname, num) {
                        let cx = (640 - bmp.width as i32) / 2;
                        let cy = (480 - bmp.height as i32) / 2;
                        self.transition = Some(TransitionCutscene {
                            pixels: bmp.pixels,
                            width: bmp.width,
                            height: bmp.height,
                            x: cx,
                            y: cy,
                            frames_left: 15, // ~0.5 seconds at 30fps
                            target: scene,
                            progress: 0.0,
                        });
                        return; // Don't switch yet — cutscene plays first
                    }
                }
            }
        }

        // Log the active dialog audio_id if still talking when switching
        if self.dialog.is_talking() {
            if let Some(d) = &self.dialog.active_dialog {
                tracing::debug!("Interrupting dialog '{}' for scene switch", d.audio_id);
            }
        }

        // Clear any active dialogs on scene switch
        self.dialog.clear();

        // Stop all sounds from the previous scene
        if let Some(snd) = &mut self.sound {
            snd.stop_all();
        }

        // Reset cursor stack on scene switch (like mulle.js MulleState.create)
        self.cursor.reset();

        // --- Scene exit logic ---
        match self.current_scene {
            Scene::World => {
                // Save driving session when leaving the world map
                if let Some(car) = &self.drive_car {
                    self.drive_session = car.save_session();
                    tracing::info!("Drive session saved at tile ({},{})",
                        self.drive_session.tile_col, self.drive_session.tile_row);
                }
                self.drive_car = None;
            }
            Scene::Garage => {
                // Save car state and shop floor when leaving the garage
                self.save_manager.save_car_parts(&self.car.parts);
                // Persist shop-floor part positions
                let floor_parts = self.scene_handler.drag_drop.item_positions();
                self.save_manager.save_shop_floor(&floor_parts);
                self.save_quest_state();
            }
            Scene::Junkyard => {
                // Save junk pile contents and last-visited pile index
                let pile_parts = self.scene_handler.drag_drop.item_positions();
                let pile_idx = self.current_pile_index();
                self.save_manager.save_pile(pile_idx, &pile_parts);
                self.save_manager.save_last_pile(pile_idx);
                tracing::debug!("Saved junkyard pile {} ({} parts)", pile_idx, pile_parts.len());
            }
            Scene::Yard => {
                // Persist yard part positions when leaving
                let yard_parts = self.scene_handler.drag_drop.item_positions();
                self.save_manager.save_yard(&yard_parts);
                tracing::debug!("Saved yard state ({} parts)", yard_parts.len());
            }
            _ => {}
        }

        // --- Scene entry gate checks ---
        if scene == Scene::World {
            // Road legality check before allowing driving
            let props = self.car.properties().clone();
            if !props.is_road_legal() {
                let failures = props.road_legal_failures();
                let hints = dialog::road_legal_hint_sounds(&failures);
                for hint_id in &hints {
                    self.play_dialog(hint_id);
                }
                tracing::info!("Car not road legal: {:?}", failures);
                // Stay in current scene
                return;
            }

            // Car is road legal — compute drive properties and create DriveCar
            let drive_props = DriveProperties::from_car_properties(&props);
            let mut drive_car = DriveCar::new(320.0, 200.0, 1, drive_props);

            // Load world map skeleton (tile/object data)
            let world_map = driving::WorldMap::default_map();
            let start_tile_id = world_map.tile_at(world_map.start_tile.0, world_map.start_tile.1);
            let mut start_topo = String::new();
            if let Some(tid) = start_tile_id {
                if let Some(tile) = world_map.get_tile(tid) {
                    start_topo = tile.topology.clone();
                    tracing::info!("World map loaded: start tile {} ('{}', topo='{}'), {} objects, start_dir={}",
                        tile.id, tile.map_image, tile.topology, tile.objects.len(),
                        world_map.start_direction);
                    tracing::debug!("Start pos: ({:.0}, {:.0})", world_map.start_pos.0, world_map.start_pos.1);
                }
            }

            // Determine start topology — use restored session tile if resuming
            let topo_name = if self.drive_session.active {
                let col = self.drive_session.tile_col;
                let row = self.drive_session.tile_row;
                world_map.tile_at(col, row)
                    .and_then(|tid| world_map.get_tile(tid))
                    .map(|t| t.topology.clone())
                    .unwrap_or(start_topo)
            } else {
                start_topo
            };
            if !topo_name.is_empty() {
                self.load_topology(&topo_name);
            }

            // Restore previous session if active
            if self.drive_session.active {
                drive_car.restore_session(&self.drive_session);
                tracing::info!("Drive session restored at tile ({},{})",
                    self.drive_session.tile_col, self.drive_session.tile_row);
            }

            self.drive_car = Some(drive_car);

            // Load dashboard HUD (once)
            if self.dashboard.is_none() {
                self.dashboard = dashboard::Dashboard::new(&self.assets);
            }
            // Load toolbox (once)
            if self.toolbox.is_none() {
                self.toolbox = Some(toolbox::Toolbox::new(&self.assets));
            }
            // Reset popup state on each World entry
            if let Some(tb) = &mut self.toolbox {
                tb.popup_open = false;
            }
        }

        // --- Login on menu → garage transition ---
        if self.current_scene == Scene::Menu && scene == Scene::Garage {
            // Auto-login default profile
            self.login_user("default");
        }

        // --- Destination interactions (legacy — now handled by SceneScript) ---
        // Save quest state when leaving a destination
        if let Scene::Destination(_) = self.current_scene {
            self.save_quest_state();
        }

        // Reset quest cache when leaving yard (as per mulle.js behavior)
        if self.current_scene == Scene::Yard && scene != Scene::Garage {
            self.quest.reset_cache();
        }

        self.current_scene = scene;
        let has_car = self.car.is_road_legal();

        // For CarShow, compute rating and pass it to the scene handler
        if scene == Scene::CarShow {
            let ff = self.car.properties().funny_factor;
            let rating = scene_script::carshow_rating(ff);
            self.scene_handler = scenes::SceneHandler::new_with_rating(scene, &self.assets, has_car, rating);
            self.active_script = Some(scene_script::build_carshow_script(ff));
            tracing::info!("CarShow: funny_factor={}, rating={}", ff, rating);
        } else {
            self.scene_handler = scenes::SceneHandler::new(scene, &self.assets, has_car);

            // Activate scene script for destinations
            if let Scene::Destination(n) = scene {
                self.active_script = scene_script::build_destination_script(n);
                if self.active_script.is_some() {
                    tracing::info!("Destination {} script activated", n);
                }
            } else {
                self.active_script = None;
            }
        }

        // --- Scene entry setup ---
        if scene == Scene::Garage {
            // Check for Figge delivery cutscene
            // Trigger: has #FiggeIsComing flag (set when leaving dest 92 with #ExtraTank)
            if self.save_manager.has_stuff("#FiggeIsComing") && self.active_script.is_none() {
                self.save_manager.remove_stuff("#FiggeIsComing");
                self.active_script = Some(scene_script::build_figge_script());
                // Give up to 3 junkman parts to the yard
                self.figge_give_parts();
                tracing::info!("Figge garage cutscene activated");
            }

            // Populate snap targets from car attachment points
            let free_points = self.car.free_attachment_points();
            for (id, wx, wy) in &free_points {
                // Check which parts could attach here and mark coverage
                let compatible = self.parts_db.parts_for_attachment(id);
                tracing::trace!("Snap target {}: {} compatible parts", id, compatible.len());

                self.scene_handler.drag_drop.snap_targets.push(
                    drag_drop::SnapTarget {
                        point_id: id.to_string(),
                        x: *wx,
                        y: *wy,
                        occupied: false,
                        covered_by: None,
                    }
                );
            }
            // Log total part IDs for debugging
            tracing::debug!("PartsDB: {} total IDs available", self.parts_db.all_ids().len());
        }

        if scene == Scene::Junkyard {
            // Populate draggable items from junkyard parts
            // Use part category system to filter parts for the junkman
            let junkman_ids = parts_db::PartsDB::junkman_part_ids();
            let dest_ids = parts_db::PartsDB::destination_part_ids();
            let random_ids = parts_db::PartsDB::random_part_ids();
            tracing::debug!("Part categories: {} junkman, {} destination, {} random",
                junkman_ids.len(), dest_ids.len(), random_ids.len());

            let junk_parts = self.parts_db.junkyard_parts();
            for (i, part_data) in junk_parts.iter().enumerate() {
                let pid = part_data.part_id;
                let category = self.parts_db.part_category(pid);
                tracing::trace!("Junk part {} - category: {:?}, has_use_view: {}, color: {}, covers: {:?}, desc: '{}'",
                    pid, category, part_data.has_use_view(),
                    part_data.properties.color, part_data.covers, part_data.description);
                let x = 60 + (i as i32 % 8) * 70;
                let y = 120 + (i as i32 / 8) * 60;
                // Create a placeholder sprite for the junk item
                let junk_sprite = Sprite {
                    x,
                    y,
                    width: 40,
                    height: 40,
                    pixels: vec![0; 40 * 40 * 4], // transparent placeholder
                    visible: true,
                    z_order: 100 + i as i32,
                    name: format!("junk_{}", pid),
                    interactive: true,
                    member_num: pid,
                };
                let mut item = drag_drop::DraggableItem::new(pid, x, y, junk_sprite, 100 + i as i32);
                item.physics_enabled = true;
                self.scene_handler.drag_drop.add_item(item);
            }
        }

        self.play_scene_sounds();
    }

    /// Get the current junkyard pile index (1-6), defaults based on save data
    fn current_pile_index(&self) -> u8 {
        self.save_manager.active()
            .map(|u| u.my_last_pile)
            .unwrap_or(1)
    }

    /// Load a topology bitmap by member name (e.g. "30t001v0") into `topo_data`.
    /// Extracts the red channel of each pixel into the 316x198 array.
    fn load_topology(&mut self, topo_name: &str) {
        // Topology bitmaps live in 05.DXR (world map file)
        let file = self.scene_handler.resolve_file("05", &self.assets);
        // Find member by name
        if let Some(df) = self.assets.files.get(&file) {
            let member_num = df.cast_members.iter()
                .find(|(_, m)| m.name == topo_name)
                .map(|(n, _)| *n);
            if let Some(num) = member_num {
                if let Some(bmp) = self.assets.decode_bitmap(&file, num) {
                    let tw = driving::TOPO_WIDTH as usize;
                    let th = driving::TOPO_HEIGHT as usize;
                    self.topo_data.resize(tw * th, 0);
                    // Extract red channel — bitmap is RGBA, 4 bytes per pixel
                    for y in 0..th.min(bmp.height as usize) {
                        for x in 0..tw.min(bmp.width as usize) {
                            let si = (y * bmp.width as usize + x) * 4;
                            if si < bmp.pixels.len() {
                                self.topo_data[y * tw + x] = bmp.pixels[si]; // R channel
                            }
                        }
                    }
                    tracing::info!("Topology '{}' loaded: {}x{}", topo_name, bmp.width, bmp.height);
                    return;
                }
            }
        }
        // Fallback: all road (0)
        tracing::warn!("Topology '{}' not found, using flat road", topo_name);
        self.topo_data.fill(0);
    }

    /// Trigger sounds appropriate for the current scene
    fn play_scene_sounds(&mut self) {
        let snd = match &mut self.sound {
            Some(s) => s,
            None => return,
        };

        // Stop previous scene's background audio
        snd.stop_background();

        match self.current_scene {
            Scene::Menu => {
                // Intro jingle — one-shot, NOT a loop (10e001v0 is a short jingle)
                snd.play_by_name("10e001v0", &self.assets);
                // Ambient background — 10e002v0 (loops)
                // TODO: proper sequencing: play 10e002v0 only after 10e001v0 finishes
                snd.play_background("10e002v0", &self.assets);
                // Mulle greeting — 11d001v0 (one-shot dialog)
                snd.play_by_name("11d001v0", &self.assets);
            }
            Scene::Garage => {
                // Garage ambient sounds
                snd.play_background("03e009v0", &self.assets);
                // Mulle workshop greeting
                snd.play_by_name("03e010v0", &self.assets);
            }
            Scene::Junkyard => {
                // Junkyard ambient
                snd.play_background("02e015v0", &self.assets);
                // Arrival sound
                snd.play_by_name("02e016v0", &self.assets);
            }
            Scene::Yard => {
                // Outdoor ambient — use shared sound
                snd.play_by_name("00e004v0", &self.assets);
            }
            Scene::World => {
                // Driving / map ambient
                snd.play_by_name("00e004v0", &self.assets);
            }
            Scene::CarShow => {
                // Car show fanfare
                snd.play_by_name("94e001v0", &self.assets);
                // Save the car's name when entering the car show
                let car_name = self.save_manager.active()
                    .map(|u| u.car.name.clone())
                    .unwrap_or_default();
                let display_name = if car_name.is_empty() { "Unbenannt".to_string() } else { car_name };
                self.save_manager.save_car_name(&display_name);
                tracing::info!("Car show: saved car name '{}'", display_name);
            }
            Scene::Destination(n) => {
                // Destination-specific sounds
                match n {
                    92 => { snd.play_by_name("92e002v0", &self.assets); }
                    _ => {}
                }
            }
            _ => {}
        }

        // Adjust volume for driving scenes (slightly quieter ambient)
        if self.current_scene == Scene::World {
            snd.set_volume(0.7);
        } else {
            snd.set_volume(1.0);
        }
    }
}
