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
pub mod dev_menu;
pub mod dialog;
pub mod drag_drop;
pub mod driving;
pub mod parts_db;
pub mod save;
pub mod scene_script;
pub mod scenes;

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
        };

        // Boot → Menu transition
        state.switch_scene(Scene::Menu);
        state
    }

    pub fn update(&mut self) {
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

        // Driving physics when on the World map
        if self.current_scene == Scene::World {
            if let Some(car) = &mut self.drive_car {
                let event = car.update(&[], |_, _| 0);
                match event {
                    driving::DriveEvent::FuelEmpty => {
                        self.play_dialog("05d011v0"); // "Tank ist leer!"
                    }
                    driving::DriveEvent::ReachedDestination { object_id, dir_resource } => {
                        tracing::info!("Reached destination object {} → {}", object_id, dir_resource);
                        // Parse dir_resource like "85" → Scene::Destination(85)
                        if let Ok(n) = dir_resource.parse::<u8>() {
                            self.drive_session = car.save_session();
                            self.switch_scene(Scene::Destination(n));
                        }
                    }
                    driving::DriveEvent::TileTransition { delta_col, delta_row } => {
                        car.do_tile_transition(delta_col, delta_row);
                    }
                    driving::DriveEvent::TerrainBlocked { reason } => {
                        tracing::debug!("Terrain blocked: {}", reason);
                    }
                    driving::DriveEvent::None => {}
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
        // Forward drag processing to scene handler
        let result = self.scene_handler.process_drag(x, y, down);
        self.handle_drop_result(result);
        self.scene_handler.on_mouse_move(x, y);

        // Track dragging state for cursor and UI feedback
        if self.scene_handler.drag_drop.is_dragging() {
            if let Some(item) = self.scene_handler.drag_drop.dragged_item() {
                tracing::trace!("Dragging part #{} at ({}, {})", item.part_id, x, y);
            }
        }
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
        if let Some(car) = &mut self.drive_car {
            car.throttle = up;
            car.braking = down;
            car.steer_left = left;
            car.steer_right = right;
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
        self.scene_handler.draw_ui(fb);

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

        // Driving HUD: fuel gauge + speed
        if self.current_scene == Scene::World {
            if let Some(car) = &self.drive_car {
                let fuel_pct = car.fuel_percent();
                let bar_w = (120.0 * fuel_pct) as i32;
                font::draw_rect(fb, 10, 440, 122, 12, 0xFF333333);
                let fuel_color = if fuel_pct > 0.3 { 0xFF00CC00 } else { 0xFFCC0000 };
                font::draw_rect(fb, 11, 441, bar_w, 10, fuel_color);
                font::draw_text(fb, 11, 430, "Benzin", 0xFFCCCCCC);

                let speed_text = format!("{}km/h", (car.speed * 30.0) as i32);
                font::draw_text_shadow(fb, 140, 440, &speed_text, 0xFFFFFFFF);

                // Show engine type and FPS in debug
                let (wo_x, wo_y) = car.wheel_offset();
                let debug_text = format!("Motor:{} FPS:{} Rad:({:.0},{:.0})",
                    car.engine_type(), driving::DriveCar::fps(), wo_x, wo_y);
                font::draw_text(fb, 10, 10, &debug_text, 0xFF888888);
            }
        }

        // Road legality indicator in Garage
        if self.current_scene == Scene::Garage {
            if self.car.is_road_legal() {
                font::draw_text_shadow(fb, 10, 460, "Fahrtauglich!", 0xFF00FF00);
            } else {
                let failures = self.car.properties().road_legal_failures();
                let hint = format!("Noch nicht fahrtauglich ({})", failures.join(", "));
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

        // On World map, render the driving car sprite
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
            if let Some(tid) = start_tile_id {
                if let Some(tile) = world_map.get_tile(tid) {
                    tracing::info!("World map loaded: start tile {} ('{}', topo='{}'), {} objects, start_dir={}",
                        tile.id, tile.map_image, tile.topology, tile.objects.len(),
                        world_map.start_direction);
                    tracing::debug!("Start pos: ({:.0}, {:.0})", world_map.start_pos.0, world_map.start_pos.1);
                }
            }

            // Restore previous session if active
            if self.drive_session.active {
                drive_car.restore_session(&self.drive_session);
                tracing::info!("Drive session restored at tile ({},{})",
                    self.drive_session.tile_col, self.drive_session.tile_row);
            }

            self.drive_car = Some(drive_car);
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

        // --- Scene entry setup ---
        if scene == Scene::Garage {
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
                let item = drag_drop::DraggableItem::new(pid, x, y, junk_sprite, 100 + i as i32);
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
                // Menu music / ambient — 10e001v0 or 10e002v0
                snd.play_background("10e001v0", &self.assets);
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
