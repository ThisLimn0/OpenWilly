//! Scene system — correct scene layout, buttons, and animations
//! based on mulle.js reference implementation.
//!
//! Each scene has:
//!  - A background sprite (opaque, full 640×480)
//!  - Overlay sprites (transparent, positioned by reg point)
//!  - Animated actors (frame sequences from Director cast members)
//!  - MulleButtons (default/hover state with scene transition target)
//!  - Hotspots (fallback clickable rectangles)

use minifb::Key;

use crate::assets::AssetStore;
use crate::assets::director::CastType;
use crate::engine::Sprite;
use crate::engine::font;
use crate::game::Scene;
use crate::game::drag_drop::{DragDropState, DropResult};

// ─── Animation system ─────────────────────────────────────────────────────

/// A decoded animation frame (pixel data)
#[derive(Debug, Clone)]
pub struct AnimFrame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>, // RGBA
    /// Registration point X (origin offset for positioning)
    pub reg_x: i32,
    /// Registration point Y
    pub reg_y: i32,
}

/// A named animation — a sequence of frames, played at a given fps
#[derive(Debug, Clone)]
pub struct Animation {
    pub name: String,
    pub frames: Vec<AnimFrame>,
    pub fps: u32,
    pub looping: bool,
    pub current_frame: usize,
    pub ticks_per_frame: u32,
    pub tick: u32,
    pub playing: bool,
    pub finished: bool,
}

impl Animation {
    pub fn new(name: &str, fps: u32, looping: bool) -> Self {
        let tpf = if fps > 0 { 30 / fps.max(1) } else { 1 };
        Self {
            name: name.to_string(),
            frames: Vec::new(),
            fps,
            looping,
            current_frame: 0,
            ticks_per_frame: tpf.max(1),
            tick: 0,
            playing: false,
            finished: false,
        }
    }

    pub fn play(&mut self) {
        self.current_frame = 0;
        self.tick = 0;
        // Recompute ticks_per_frame from fps in case it was changed
        self.ticks_per_frame = if self.fps > 0 { (30 / self.fps.max(1)).max(1) } else { 1 };
        self.playing = true;
        self.finished = false;
    }

    /// Advance one game tick (30 fps). Returns true if frame changed.
    pub fn tick(&mut self) -> bool {
        if !self.playing || self.frames.is_empty() {
            return false;
        }
        self.tick += 1;
        if self.tick >= self.ticks_per_frame {
            self.tick = 0;
            self.current_frame += 1;
            if self.current_frame >= self.frames.len() {
                if self.looping {
                    self.current_frame = 0;
                } else {
                    self.current_frame = self.frames.len() - 1;
                    self.playing = false;
                    self.finished = true;
                }
            }
            return true;
        }
        false
    }

    pub fn current_pixels(&self) -> Option<&AnimFrame> {
        self.frames.get(self.current_frame)
    }
}

/// Events emitted by an actor on each tick
#[derive(Debug, Clone)]
pub enum ActorEvent {
    /// A non-looping animation finished playing
    AnimationFinished {
        actor_name: String,
        anim_name: String,
    },
}

/// Events emitted by the scene handler on each update
#[derive(Debug, Clone)]
pub enum SceneEvent {
    /// An actor's animation finished
    ActorAnimFinished {
        actor_name: String,
        anim_name: String,
    },
}

/// An animated actor — has a position, multiple named animations, one active
pub struct Actor {
    pub x: i32,
    pub y: i32,
    pub animations: Vec<Animation>,
    pub active_anim: usize,
    pub name: String,
    pub visible: bool,
    pub z_order: i32,
    /// Animation to play while talking (lip-sync open mouth)
    pub talk_anim: Option<String>,
    /// Animation to play during silence pauses
    pub silence_anim: Option<String>,
    /// Whether this actor is currently talking
    pub is_talking: bool,
}

impl Actor {
    pub fn new(name: &str, x: i32, y: i32, z_order: i32) -> Self {
        Self {
            x, y,
            animations: Vec::new(),
            active_anim: 0,
            name: name.to_string(),
            visible: true,
            z_order,
            talk_anim: None,
            silence_anim: None,
            is_talking: false,
        }
    }

    /// Configure talk/silence animations for lip-sync cue points
    #[allow(dead_code)] // Used when setting up destination scene actors
    pub fn set_talk_anims(&mut self, talk: &str, silence: &str) {
        self.talk_anim = Some(talk.to_string());
        self.silence_anim = Some(silence.to_string());
    }

    /// Start talking — play the talk animation
    pub fn start_talking(&mut self) {
        self.is_talking = true;
        if let Some(anim_name) = &self.talk_anim.clone() {
            self.play(anim_name);
        }
    }

    /// Stop talking — play the silence animation
    pub fn stop_talking(&mut self) {
        self.is_talking = false;
        if let Some(anim_name) = &self.silence_anim.clone() {
            self.play(anim_name);
        }
    }

    /// Handle a cue point event (switch between talk/silence animation)
    pub fn on_cue(&mut self, cue_name: &str) {
        match cue_name.to_lowercase().as_str() {
            "talk" => self.start_talking(),
            "silence" => self.stop_talking(),
            _ => {
                tracing::debug!("Actor '{}': unhandled cue '{}'", self.name, cue_name);
            }
        }
    }

    /// Add an animation, loading frame pixel data from assets
    pub fn add_animation(
        &mut self,
        name: &str,
        member_refs: &[(&str, u32)],
        fps: u32,
        looping: bool,
        assets: &AssetStore,
    ) {
        let mut anim = Animation::new(name, fps, looping);
        for &(file, num) in member_refs {
            if let Some(bmp) = assets.decode_bitmap_transparent(file, num) {
                // Get registration point from BitmapInfo
                let (rx, ry) = assets.files.get(file)
                    .and_then(|df| df.cast_members.get(&num))
                    .and_then(|m| m.bitmap_info.as_ref())
                    .map(|bi| (bi.reg_x as i32, bi.reg_y as i32))
                    .unwrap_or((0, 0));
                anim.frames.push(AnimFrame {
                    width: bmp.width,
                    height: bmp.height,
                    pixels: bmp.pixels,
                    reg_x: rx,
                    reg_y: ry,
                });
            } else {
                tracing::warn!(
                    "Actor '{}' anim '{}': failed to load {}/{}",
                    self.name, name, file, num
                );
            }
        }
        anim.playing = true;
        self.animations.push(anim);
    }

    pub fn play(&mut self, name: &str) {
        for (i, a) in self.animations.iter_mut().enumerate() {
            if a.name == name {
                a.play();
                self.active_anim = i;
                return;
            }
        }
    }

    /// Tick the active animation. Returns an event if a non-looping animation finished.
    pub fn tick(&mut self) -> Option<ActorEvent> {
        if let Some(anim) = self.animations.get_mut(self.active_anim) {
            let was_playing = anim.playing;
            anim.tick();
            // Fire event only on the exact frame the animation becomes finished
            if was_playing && !anim.playing && anim.finished {
                return Some(ActorEvent::AnimationFinished {
                    actor_name: self.name.clone(),
                    anim_name: anim.name.clone(),
                });
            }
        }
        None
    }

    /// Get current frame as a temporary Sprite for blitting
    pub fn current_sprite(&self) -> Option<Sprite> {
        let anim = self.animations.get(self.active_anim)?;
        let frame = anim.current_pixels()?;
        Some(Sprite {
            x: self.x - frame.reg_x,
            y: self.y - frame.reg_y,
            width: frame.width,
            height: frame.height,
            pixels: frame.pixels.clone(),
            visible: self.visible,
            z_order: self.z_order,
            name: format!("actor:{}", self.name),
            interactive: false,
            member_num: 0,
        })
    }
}

// ─── MulleButton ──────────────────────────────────────────────────────────

/// A button with default/hover sprite states and a click action
pub struct MulleButton {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub default_pixels: Vec<u8>,
    pub hover_pixels: Option<Vec<u8>>,
    pub hover_width: u32,
    pub hover_height: u32,
    pub hovered: bool,
    pub target: Option<Scene>,
    pub z_order: i32,
    pub visible: bool,
    /// Sound to play on click (mulle.js `soundDefault`)
    pub sound_default: Option<String>,
    /// Sound to play on hover enter (mulle.js `soundHover`)
    pub sound_hover: Option<String>,
}

impl MulleButton {
    /// Create from Director member references.
    /// `ax, ay` = anchor point (mulle.js always uses 320, 240).
    /// Position = anchor − regPoint.
    pub fn new(
        name: &str,
        file: &str,
        default_num: u32,
        hover_num: Option<u32>,
        ax: i32,
        ay: i32,
        target: Option<Scene>,
        z_order: i32,
        assets: &AssetStore,
    ) -> Option<Self> {
        let def = assets.decode_bitmap_transparent(file, default_num)?;
        let df = assets.files.get(file)?;
        let bi = df.cast_members.get(&default_num)?.bitmap_info.as_ref()?;

        let (hover_px, hw, hh) = if let Some(hnum) = hover_num {
            if let Some(hbmp) = assets.decode_bitmap_transparent(file, hnum) {
                (Some(hbmp.pixels), hbmp.width, hbmp.height)
            } else {
                (None, 0, 0)
            }
        } else {
            (None, 0, 0)
        };

        Some(Self {
            name: name.to_string(),
            x: ax - bi.reg_x as i32,
            y: ay - bi.reg_y as i32,
            width: def.width,
            height: def.height,
            default_pixels: def.pixels,
            hover_pixels: hover_px,
            hover_width: hw,
            hover_height: hh,
            hovered: false,
            target,
            z_order,
            visible: true,
            sound_default: None,
            sound_hover: None,
        })
    }

    pub fn hit_test(&self, px: i32, py: i32) -> bool {
        if !self.visible { return false; }
        let lx = px - self.x;
        let ly = py - self.y;
        lx >= 0 && ly >= 0 && lx < self.width as i32 && ly < self.height as i32
    }

    pub fn as_sprite(&self) -> Sprite {
        let (pixels, w, h) = if self.hovered && self.hover_pixels.is_some() {
            (self.hover_pixels.as_ref().unwrap().clone(), self.hover_width, self.hover_height)
        } else {
            (self.default_pixels.clone(), self.width, self.height)
        };
        Sprite {
            x: self.x,
            y: self.y,
            width: w,
            height: h,
            pixels,
            visible: self.visible,
            z_order: self.z_order,
            name: format!("btn:{}", self.name),
            interactive: true,
            member_num: 0,
        }
    }
}

// ─── Hotspot ──────────────────────────────────────────────────────────────

/// Clickable region in a scene
#[derive(Debug, Clone)]
pub struct Hotspot {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub name: String,
    pub target: Option<Scene>,
}

// ─── SceneHandler ─────────────────────────────────────────────────────────

/// Scene handler — manages sprites, buttons, actors, and interaction
pub struct SceneHandler {
    scene: Scene,
    pub sprites: Vec<Sprite>,
    pub buttons: Vec<MulleButton>,
    actors: Vec<Actor>,
    pub hotspots: Vec<Hotspot>,
    // Menu UI state
    input_text: String,
    cursor_visible: bool,
    frame_counter: u32,
    saved_names: Vec<String>,
    selected_name: Option<usize>,
    // Junkyard sub-state
    junk_pile: u8,
    // Drag & Drop
    pub drag_drop: DragDropState,
    /// Name of the actor currently talking (for cue-point routing)
    talking_actor: Option<String>,
    /// Whether a road-legal car exists (affects Yard layout)
    has_car: bool,
    /// Pre-computed carshow rating (1–5), set when entering CarShow
    carshow_rating: u8,
}

impl SceneHandler {
    pub fn new(scene: Scene, assets: &AssetStore, has_car: bool) -> Self {
        Self::new_with_rating(scene, assets, has_car, 1)
    }

    /// Create a scene handler with a pre-computed carshow rating
    pub fn new_with_rating(scene: Scene, assets: &AssetStore, has_car: bool, carshow_rating: u8) -> Self {
        let mut handler = Self {
            scene,
            sprites: Vec::new(),
            buttons: Vec::new(),
            actors: Vec::new(),
            hotspots: Vec::new(),
            input_text: String::new(),
            cursor_visible: true,
            frame_counter: 0,
            saved_names: Vec::new(),
            selected_name: None,
            junk_pile: 1,
            drag_drop: DragDropState::new(),
            talking_actor: None,
            has_car,
            carshow_rating,
        };

        handler.load_scene(assets);

        let vis = handler.sprites.iter().filter(|s| s.visible).count();
        tracing::info!(
            "Scene {:?}: {} sprites ({} vis), {} buttons, {} actors, {} hotspots",
            handler.scene,
            handler.sprites.len(), vis,
            handler.buttons.len(),
            handler.actors.len(),
            handler.hotspots.len(),
        );

        handler
    }

    // ─── Scene loading dispatch ─────────────────────────────────────────

    fn load_scene(&mut self, assets: &AssetStore) {
        match self.scene {
            Scene::Menu => self.load_menu(assets),
            Scene::Garage => self.load_garage(assets),
            Scene::Junkyard => self.load_junkyard(assets),
            Scene::Yard => self.load_yard(assets),
            Scene::World => self.load_world(assets),
            Scene::CarShow => self.load_carshow(assets),
            Scene::Destination(n) => self.load_destination(n, assets),
            _ => self.load_generic(assets),
        }
    }

    // ─── Helper: load a single member as sprite ─────────────────────────

    fn load_bg(&mut self, file: &str, num: u32, assets: &AssetStore) {
        if let Some(bmp) = assets.decode_bitmap(file, num) {
            // Position: center anchor (320,240) minus regPoint → gives top-left
            let (rx, ry) = Self::reg_point(file, num, assets);
            self.sprites.push(Sprite {
                x: 320 - rx,
                y: 240 - ry,
                width: bmp.width, height: bmp.height,
                pixels: bmp.pixels,
                visible: true,
                z_order: 0,
                name: format!("bg#{}", num),
                interactive: false,
                member_num: num,
            });
        } else {
            tracing::warn!("Failed to load bg {}#{}", file, num);
        }
    }

    /// Load a sprite overlay positioned at anchor (ax, ay) minus regPoint.
    /// In mulle.js every overlay/button uses an anchor (usually 320,240)
    /// and the Director regPoint determines the top-left corner.
    fn load_overlay_at(&mut self, file: &str, num: u32, ax: i32, ay: i32, z: i32, visible: bool, assets: &AssetStore) {
        if let Some(bmp) = assets.decode_bitmap_transparent(file, num) {
            let (rx, ry) = Self::reg_point(file, num, assets);
            let name = assets.files.get(file)
                .and_then(|df| df.cast_members.get(&num))
                .map(|m| m.name.clone())
                .unwrap_or_default();
            self.sprites.push(Sprite {
                x: ax - rx,
                y: ay - ry,
                width: bmp.width, height: bmp.height,
                pixels: bmp.pixels,
                visible,
                z_order: z,
                name: format!("#{} {}", num, name),
                interactive: true,
                member_num: num,
            });
        }
    }

    /// Convenience: load overlay anchored at screen center (320, 240)
    fn load_overlay(&mut self, file: &str, num: u32, z: i32, visible: bool, assets: &AssetStore) {
        self.load_overlay_at(file, num, 320, 240, z, visible, assets);
    }

    /// Get (reg_x, reg_y) for a bitmap cast member.
    fn reg_point(file: &str, num: u32, assets: &AssetStore) -> (i32, i32) {
        assets.files.get(file)
            .and_then(|df| df.cast_members.get(&num))
            .and_then(|m| m.bitmap_info.as_ref())
            .map(|bi| (bi.reg_x as i32, bi.reg_y as i32))
            .unwrap_or((0, 0))
    }

    // ─── Menu (10.DXR) ─────────────────────────────────────────────────

    fn load_menu(&mut self, assets: &AssetStore) {
        let f = "10.DXR";
        // Background: member #2
        self.load_bg(f, 2, assets);

        // Mulle body: member #125 (static sprite, anchored at same point as head/mouth)
        self.load_overlay_at(f, 125, 139, 296, 10, true, assets);

        // Mulle head animation: #126 (idle), #136-#137 (point)
        let mut head = Actor::new("mulleMenuHead", 139, 296, 15);
        head.add_animation("idle", &[(f, 126)], 0, true, assets);
        head.add_animation("point", &[
            (f, 136), (f, 137), (f, 137), (f, 137), (f, 137),
            (f, 137), (f, 137), (f, 137), (f, 137), (f, 136), (f, 126),
        ], 10, false, assets);
        head.play("idle");
        self.actors.push(head);

        // Mulle mouth animation: #115 (idle), #115-#122 (talk)
        let mut mouth = Actor::new("mulleMenuMouth", 139, 296, 16);
        mouth.add_animation("idle", &[(f, 115)], 5, true, assets);
        mouth.add_animation("talkPlayer", &[
            (f, 115), (f, 116), (f, 117), (f, 118),
            (f, 119), (f, 120), (f, 121), (f, 122),
        ], 10, true, assets);
        mouth.set_talk_anims("talkPlayer", "idle");
        mouth.play("idle");
        self.actors.push(mouth);
    }

    // ─── Garage (03.DXR) ───────────────────────────────────────────────

    fn load_garage(&mut self, assets: &AssetStore) {
        let f = "03.DXR";
        // Background: member #33
        self.load_bg(f, 33, assets);

        // Door → Junkyard: #34 default, #35 hover
        if let Some(mut btn) = MulleButton::new(
            "Tür → Schrottplatz", f, 34, Some(35), 320, 240, Some(Scene::Junkyard), 5, assets
        ) {
            btn.sound_default = Some("02e015v0".into());
            btn.sound_hover = Some("02e016v0".into());
            // DropTarget: drag parts onto junkyard door → pile1
            self.drag_drop.drop_targets.push(crate::game::drag_drop::DropTarget {
                x: btn.x, y: btn.y, width: btn.width, height: btn.height,
                id: "door_junk".into(), name: "Tür → Schrottplatz".into(),
            });
            self.buttons.push(btn);
        }

        // Door → Yard (garage door, with car): #36 default, #37 hover
        if let Some(mut btn) = MulleButton::new(
            "Garagentor → Hof", f, 36, Some(37), 320, 240, Some(Scene::Yard), 5, assets
        ) {
            btn.sound_default = Some("02e015v0".into());
            btn.sound_hover = Some("02e016v0".into());
            // DropTarget: drag parts onto garage door → yard
            self.drag_drop.drop_targets.push(crate::game::drag_drop::DropTarget {
                x: btn.x, y: btn.y, width: btn.width, height: btn.height,
                id: "door_yard".into(), name: "Garagentor → Hof".into(),
            });
            self.buttons.push(btn);
        }

        // Side door → Yard (without car): #38 default, #39 hover
        if let Some(mut btn) = MulleButton::new(
            "Seitentür → Hof", f, 38, Some(39), 320, 240, Some(Scene::Yard), 5, assets
        ) {
            btn.sound_default = Some("02e015v0".into());
            btn.sound_hover = Some("02e016v0".into());
            // DropTarget: drag parts onto side door → yard
            self.drag_drop.drop_targets.push(crate::game::drag_drop::DropTarget {
                x: btn.x, y: btn.y, width: btn.width, height: btn.height,
                id: "door_yard".into(), name: "Seitentür → Hof".into(),
            });
            self.buttons.push(btn);
        }

        // Mulle actor at (118, 188) — from 00.CXT shared cast
        let b = "00.CXT";
        let mut mulle = Actor::new("mulleDefault", 118, 188, 20);
        mulle.add_animation("idle", &[(b, 271)], 10, true, assets);
        mulle.add_animation("lookPlayer", &[(b, 287), (b, 288)], 10, true, assets);
        mulle.add_animation("talkPlayer", &[
            (b, 289), (b, 290), (b, 291), (b, 292),
            (b, 293), (b, 294), (b, 295),
        ], 10, true, assets);
        mulle.add_animation("scratchChin", &[
            (b, 271), (b, 272), (b, 273), (b, 274), (b, 275), (b, 276),
        ], 10, false, assets);
        mulle.add_animation("lookLeft", &[(b, 283)], 10, true, assets);
        mulle.play("idle");
        self.actors.push(mulle);

        // Figge actor at the side door (hidden until triggered)
        // Sprites from 03.DXR members 81-93
        let mut figge = Actor::new("figge", 320, 240, 18);
        figge.add_animation("enter", &[
            (f, 81), (f, 82), (f, 83), (f, 84), (f, 85),
        ], 10, false, assets);
        figge.add_animation("entered", &[(f, 86)], 10, true, assets);
        figge.add_animation("exit", &[
            (f, 85), (f, 84), (f, 83), (f, 82), (f, 81),
        ], 10, false, assets);
        figge.add_animation("talk", &[
            (f, 86), (f, 87), (f, 88), (f, 89),
            (f, 90), (f, 91), (f, 92), (f, 93),
        ], 10, true, assets);
        figge.set_talk_anims("talk", "entered");
        figge.visible = false;
        self.actors.push(figge);
    }

    // ─── Junkyard (02.DXR / 02.CXT) ────────────────────────────────────

    fn load_junkyard(&mut self, assets: &AssetStore) {
        self.load_junkyard_pile(self.junk_pile, assets);
    }

    fn load_junkyard_pile(&mut self, pile: u8, assets: &AssetStore) {
        let f = self.resolve_file("02", assets);

        // Set pile drop rects (3 stacked rects per pile from mulle.js)
        self.drag_drop.drop_rects = crate::game::drag_drop::DropRect::pile_rects(pile);

        // Pile member data from mulle.js:
        //   (door default, door hover, right arrow default, right arrow hover,
        //    left arrow default, left arrow hover)
        let pile_data: [(u32, u32, u32, u32, u32, u32); 6] = [
            (85, 86, 162, 163, 174, 175), // Pile 1
            (87, 88, 164, 165, 176, 177), // Pile 2
            (89, 90, 166, 167, 178, 179), // Pile 3
            (91, 92, 168, 169, 180, 181), // Pile 4
            (93, 94, 170, 171, 182, 183), // Pile 5
            (95, 96, 172, 173, 184, 185), // Pile 6
        ];
        let bg_names = ["02b001v0", "02b002v0", "02b003v0", "02b004v0", "02b005v0", "02b006v0"];

        let idx = (pile - 1).min(5) as usize;
        let (door_def, door_hov, right_def, right_hov, left_def, left_hov) = pile_data[idx];

        // Background by name, fallback to largest bitmap
        let bg_name = bg_names[idx];
        if let Some(num) = self.find_member_by_name(&f, bg_name, assets) {
            self.load_bg(&f, num, assets);
        } else {
            self.load_largest_bg(&f, assets);
        }

        // Door → Garage
        if let Some(mut btn) = MulleButton::new(
            "Tür → Werkstatt", &f, door_def, Some(door_hov),
            320, 240, Some(Scene::Garage), 5, assets
        ) {
            btn.sound_default = Some("02e015v0".into());
            btn.sound_hover = Some("02e016v0".into());
            // DropTarget: drag parts onto door → shop_floor
            self.drag_drop.drop_targets.push(crate::game::drag_drop::DropTarget {
                x: btn.x, y: btn.y, width: btn.width, height: btn.height,
                id: "door_shop".into(), name: "Tür → Werkstatt".into(),
            });
            self.buttons.push(btn);
        }

        // Debug: log what members exist at the expected numbers
        if let Some(df) = assets.files.get(&f) {
            tracing::debug!("  02.DXR has {} total cast members", df.cast_members.len());
            for &num in &[door_def, door_hov, right_def, right_hov, left_def, left_hov] {
                if let Some(m) = df.cast_members.get(&num) {
                    let (w, h, rx, ry) = m.bitmap_info.as_ref()
                        .map(|b| (b.width, b.height, b.reg_x, b.reg_y))
                        .unwrap_or((0, 0, 0, 0));
                    tracing::debug!("  Pile {} member #{}: name='{}' type={:?} size={}x{} reg=({},{})",
                        pile, num, m.name, m.cast_type, w, h, rx, ry);
                } else {
                    tracing::warn!("  Pile {} member #{}: NOT FOUND in {}", pile, num, f);
                }
            }
            // Dump arrow bitmap to file for inspection
            if pile == 1 {
                if let Some(bmp) = assets.decode_bitmap_transparent(&f, right_def) {
                    let path = format!("debug_member_{}.raw", right_def);
                    tracing::info!("Dumping member {} bitmap ({}x{}, {} pixels) to {}", right_def, bmp.width, bmp.height, bmp.pixels.len(), path);
                    // Write as PPM for easy viewing
                    let ppm_path = format!("debug_member_{}.ppm", right_def);
                    let mut data = format!("P6\n{} {}\n255\n", bmp.width, bmp.height);
                    let mut rgb_bytes = Vec::with_capacity(bmp.width as usize * bmp.height as usize * 3);
                    for px in &bmp.pixels {
                        rgb_bytes.push(((px >> 16) & 0xFF) as u8); // R
                        rgb_bytes.push(((px >> 8) & 0xFF) as u8);  // G
                        rgb_bytes.push((px & 0xFF) as u8);         // B
                    }
                    let _ = std::fs::write(&ppm_path, [data.as_bytes(), &rgb_bytes].concat());
                    tracing::info!("Wrote {} (PPM image)", ppm_path);
                }
                // Also dump what the background lookup resolves to
                let bg_name = bg_names[idx];
                if let Some(bg_num) = self.find_member_by_name(&f, bg_name, assets) {
                    tracing::info!("Background '{}' resolved to member #{}", bg_name, bg_num);
                }
            }
        }

        // Arrow right → next pile (target=None, handled specially in on_click)
        let right_pile = if pile >= 6 { 1 } else { pile + 1 };
        if let Some(mut btn) = MulleButton::new(
            &format!("→ Haufen {}", right_pile), &f, right_def, Some(right_hov),
            320, 240, None, 5, assets
        ) {
            // DropTarget: drag parts onto right arrow → next pile
            self.drag_drop.drop_targets.push(crate::game::drag_drop::DropTarget {
                x: btn.x, y: btn.y, width: btn.width, height: btn.height,
                id: format!("arrow_right_{}", right_pile), name: format!("→ Haufen {}", right_pile),
            });
            self.buttons.push(btn);
        }

        // Arrow left → prev pile
        let left_pile = if pile <= 1 { 6 } else { pile - 1 };
        if let Some(mut btn) = MulleButton::new(
            &format!("← Haufen {}", left_pile), &f, left_def, Some(left_hov),
            320, 240, None, 5, assets
        ) {
            // DropTarget: drag parts onto left arrow → prev pile
            self.drag_drop.drop_targets.push(crate::game::drag_drop::DropTarget {
                x: btn.x, y: btn.y, width: btn.width, height: btn.height,
                id: format!("arrow_left_{}", left_pile), name: format!("← Haufen {}", left_pile),
            });
            self.buttons.push(btn);
        }
    }

    // ─── Yard (04.DXR / 04.CXT) ────────────────────────────────────────

    fn load_yard(&mut self, assets: &AssetStore) {
        let f = self.resolve_file("04", assets);

        // Background: member #118
        self.load_bg(&f, 118, assets);

        // Mailbox: #42 default, #43 hover (no scene transition)
        if let Some(mut btn) = MulleButton::new(
            "Briefkasten", &f, 42, Some(43), 320, 240, None, 5, assets
        ) {
            btn.sound_default = Some("04e009v0".into());
            btn.sound_hover = Some("04e010v0".into());
            self.buttons.push(btn);
        }

        if self.has_car {
            // ── Car mode: side door is static overlay, garage door clickable
            //    Road overlay + hotspot to World ──
            self.load_overlay(&f, 13, 4, true, assets);  // side door static

            // Garage door → Garage
            if let Some(mut btn) = MulleButton::new(
                "Garagentor → Werkstatt", &f, 40, Some(41),
                320, 240, Some(Scene::Garage), 6, assets
            ) {
                btn.sound_default = Some("02e015v0".into());
                btn.sound_hover = Some("02e016v0".into());
                // DropTarget: drag parts onto garage door → shop_floor
                self.drag_drop.drop_targets.push(crate::game::drag_drop::DropTarget {
                    x: btn.x, y: btn.y, width: btn.width, height: btn.height,
                    id: "door_shop".into(), name: "Garagentor → Werkstatt".into(),
                });
                self.buttons.push(btn);
            }

            // Road → World
            self.load_overlay(&f, 16, 4, true, assets);
            self.hotspots.push(Hotspot {
                x: 0, y: 0, width: 640, height: 200,
                name: "Straße → Weltkarte".into(),
                target: Some(Scene::World),
            });
        } else {
            // ── Door mode: both doors clickable → Garage, no road to World ──
            if let Some(mut btn) = MulleButton::new(
                "Seitentür → Werkstatt", &f, 13, Some(14),
                320, 240, Some(Scene::Garage), 5, assets
            ) {
                btn.sound_default = Some("02e015v0".into());
                btn.sound_hover = Some("02e016v0".into());
                // DropTarget: drag parts onto side door → shop_floor
                self.drag_drop.drop_targets.push(crate::game::drag_drop::DropTarget {
                    x: btn.x, y: btn.y, width: btn.width, height: btn.height,
                    id: "door_shop".into(), name: "Seitentür → Werkstatt".into(),
                });
                self.buttons.push(btn);
            }

            if let Some(mut btn) = MulleButton::new(
                "Garagentor → Werkstatt", &f, 40, Some(41),
                320, 240, Some(Scene::Garage), 6, assets
            ) {
                btn.sound_default = Some("02e015v0".into());
                btn.sound_hover = Some("02e016v0".into());
                // DropTarget: drag parts onto garage door → shop_floor
                self.drag_drop.drop_targets.push(crate::game::drag_drop::DropTarget {
                    x: btn.x, y: btn.y, width: btn.width, height: btn.height,
                    id: "door_shop".into(), name: "Garagentor → Werkstatt".into(),
                });
                self.buttons.push(btn);
            }
        }
    }

    // ─── World (05.DXR) ────────────────────────────────────────────────

    fn load_world(&mut self, assets: &AssetStore) {
        let f = self.resolve_file("05", assets);

        // Load the largest bitmap as map background
        self.load_largest_bg(&f, assets);

        // Dashboard overlay at bottom: member #25, anchored at (320, 440) per mulle.js
        self.load_overlay_at(&f, 25, 320, 440, 50, true, assets);

        // Hotspot: click map area → Yard (simplified navigation)
        self.hotspots.push(Hotspot {
            x: 0, y: 0, width: 640, height: 400,
            name: "Karte (klick = Hof)".into(),
            target: Some(Scene::Yard),
        });
    }

    // ─── CarShow (94.DXR / 08.CXT) ────────────────────────────────────

    fn load_carshow(&mut self, assets: &AssetStore) {
        let f = self.resolve_file("94", assets);

        // Background: member #200
        self.load_bg(&f, 200, assets);

        // Judge actor at (155, 210) — full animation set
        let mut judge = Actor::new("judge", 155, 210, 20);
        judge.add_animation("idle", &[(&f, 31)], 10, true, assets);
        judge.add_animation("talk", &[
            (&f, 43), (&f, 44), (&f, 45), (&f, 46), (&f, 47),
        ], 10, true, assets);
        judge.add_animation("raiseScore", &[
            (&f, 32), (&f, 33), (&f, 34), (&f, 35),
        ], 5, false, assets);
        judge.add_animation("idleScore", &[(&f, 36)], 10, true, assets);
        judge.add_animation("talkScore", &[
            (&f, 37), (&f, 38), (&f, 39), (&f, 41), (&f, 42),
        ], 10, true, assets);
        judge.add_animation("lowerScore", &[
            (&f, 35), (&f, 34), (&f, 33), (&f, 32),
        ], 5, false, assets);
        judge.set_talk_anims("talk", "idle");
        judge.play("idle");
        self.actors.push(judge);

        // Score sprite at (177, 93) — 94.DXR members 17–21 (rating 1→17 … 5→21)
        // Use the pre-computed rating to select the right member
        let score_member = 16 + self.carshow_rating as u32; // 17..21
        let mut score = Actor::new("score", 177, 93, 25);
        score.add_animation("show", &[(&f, score_member)], 10, true, assets);
        score.play("show");
        score.visible = false; // Hidden until script reveals it
        self.actors.push(score);

        // Mulle at (89, 337) looking left
        let b = "00.CXT";
        let mut mulle = Actor::new("mulleDefault", 89, 337, 15);
        mulle.add_animation("lookLeft", &[(b, 283)], 10, true, assets);
        mulle.add_animation("idle", &[(b, 271)], 10, true, assets);
        mulle.add_animation("talkRegular", &[
            (b, 296), (b, 297), (b, 298), (b, 299),
            (b, 300), (b, 301), (b, 302),
        ], 10, true, assets);
        mulle.set_talk_anims("talkRegular", "idle");
        mulle.play("lookLeft");
        self.actors.push(mulle);

        // Return to world
        self.hotspots.push(Hotspot {
            x: 0, y: 400, width: 640, height: 80,
            name: "← Weltkarte".into(),
            target: Some(Scene::World),
        });
    }

    // ─── Destinations (82-94) ──────────────────────────────────────────

    fn load_destination(&mut self, num: u8, assets: &AssetStore) {
        let f = self.resolve_file(&format!("{:02}", num), assets);
        let b = "00.CXT";

        // Per-destination Mulle position and setup
        let (mulle_x, mulle_y) = match num {
            85 => (95, 300),   // RoadDog — default position
            86 => (350, 398),  // Solhem — near Mia's house
            87 => (496, 332),  // Saftfabrik — right side
            88 => (351, 234),  // StureStortand — center
            92 => (95, 300),   // FiggeFerrum — left side
            _ => (95, 300),    // Generic default
        };

        match num {
            85 => {
                // ─── RoadDog — Salka on the road ────────────────────────
                self.load_bg(&f, 25, assets);

                let mut salka = Actor::new("salkaRight", 480, 386, 20);
                salka.add_animation("idle", &[
                    (&f, 26), (&f, 27), (&f, 28), (&f, 29),
                    (&f, 30), (&f, 29), (&f, 28), (&f, 27),
                ], 15, true, assets);
                salka.play("idle");
                self.actors.push(salka);
            }
            86 => {
                // ─── Solhem — Mia and the cat ───────────────────────────
                self.load_bg(&f, 1, assets);

                // Mia body at (277, 246)
                let mut mia_body = Actor::new("miaBody", 277, 246, 18);
                mia_body.add_animation("idle", &[(&f, 55)], 10, true, assets);
                mia_body.add_animation("catchIntro", &[
                    (&f, 55), (&f, 56), (&f, 57), (&f, 58),
                ], 10, false, assets);
                mia_body.add_animation("catchEnd", &[
                    (&f, 47), (&f, 48), (&f, 49), (&f, 50),
                ], 10, false, assets);
                mia_body.play("idle");
                self.actors.push(mia_body);

                // Mia head at (535, 336) — talks about the cat
                let mut mia_head = Actor::new("miaHead", 535, 336, 19);
                mia_head.add_animation("idle", &[(&f, 62)], 10, true, assets);
                mia_head.add_animation("talk", &[
                    (&f, 63), (&f, 64), (&f, 65), (&f, 66), (&f, 67),
                ], 10, true, assets);
                mia_head.add_animation("idleCat", &[(&f, 69)], 10, true, assets);
                mia_head.add_animation("talkCat", &[
                    (&f, 69), (&f, 70), (&f, 71), (&f, 72), (&f, 73), (&f, 74),
                ], 10, true, assets);
                mia_head.set_talk_anims("talk", "idle");
                mia_head.play("idle");
                self.actors.push(mia_head);

                // Cat at (278, 240)
                let mut cat = Actor::new("cat", 278, 240, 20);
                cat.add_animation("idle", &[(&f, 30)], 10, true, assets);
                // jump1: members 31–42 (12 frames)
                let jump1_refs: Vec<(&str, u32)> = (31..=42).map(|n| (f.as_str(), n)).collect();
                cat.add_animation("jump1", &jump1_refs, 10, false, assets);
                // jump2: members 42–45 (4 frames)
                cat.add_animation("jump2", &[
                    (&f, 42), (&f, 43), (&f, 44), (&f, 45),
                ], 10, false, assets);
                cat.play("idle");
                self.actors.push(cat);
            }
            87 => {
                // ─── Saftfabrik — Lemonade factory ──────────────────────
                self.load_bg(&f, 208, assets);

                let mut garson = Actor::new("garson", 537, 218, 20);
                garson.add_animation("idle", &[(&f, 15)], 10, true, assets);
                garson.add_animation("talk", &[
                    (&f, 16), (&f, 17), (&f, 18),
                ], 8, true, assets);
                garson.set_talk_anims("talk", "idle");
                garson.play("idle");
                self.actors.push(garson);
            }
            88 => {
                // ─── StureStortand — Sture's party ──────────────────────
                // BG 32 (with lemonade) or 40 (without) — selected later
                // For now, use 32 as default (the scene_script will handle logic)
                self.load_bg(&f, 32, assets);

                // Sture sad at (285, 162)
                let mut sture_sad = Actor::new("stureSad", 285, 162, 20);
                sture_sad.add_animation("idle", &[(&f, 42)], 10, true, assets);
                sture_sad.add_animation("talk", &[
                    (&f, 43), (&f, 44), (&f, 45), (&f, 46), (&f, 47),
                ], 10, true, assets);
                sture_sad.set_talk_anims("talk", "idle");
                sture_sad.play("idle");
                self.actors.push(sture_sad);

                // Sture happy at (285, 162) — initially hidden
                let mut sture_happy = Actor::new("stureHappy", 285, 162, 20);
                sture_happy.add_animation("idle", &[(&f, 34)], 10, true, assets);
                sture_happy.add_animation("talk", &[
                    (&f, 35), (&f, 36), (&f, 37), (&f, 38), (&f, 39),
                ], 8, true, assets);
                sture_happy.set_talk_anims("talk", "idle");
                sture_happy.visible = false;
                sture_happy.play("idle");
                self.actors.push(sture_happy);
            }
            92 => {
                // ─── FiggeFerrum — Figge and his dog ────────────────────
                self.load_bg(&f, 1, assets);

                // Buffa (dog) from 00.CXT
                let mut buffa = Actor::new("buffa", 271, 347, 15);
                buffa.add_animation("idle", &[(b, 214)], 10, true, assets);
                buffa.add_animation("scratch1", &[(b, 214), (b, 215)], 10, true, assets);
                buffa.add_animation("sleep_intro", &[
                    (b, 214), (b, 216), (b, 217), (b, 218),
                ], 10, false, assets);
                buffa.add_animation("sleep_loop", &[(b, 219), (b, 220)], 1, false, assets);
                buffa.add_animation("bark", &[(b, 222), (b, 223)], 10, true, assets);
                buffa.play("idle");
                self.actors.push(buffa);

                // Figge body (static overlay, anchored at 102, 292 like the head)
                self.load_overlay_at(&f, 16, 102, 292, 18, true, assets);

                // Figge head (animated) at (102, 292)
                let mut figge = Actor::new("figge", 102, 292, 20);
                figge.add_animation("idle", &[(&f, 17)], 10, true, assets);
                figge.add_animation("talkPlayer", &[
                    (&f, 17), (&f, 18), (&f, 19), (&f, 20),
                    (&f, 21), (&f, 22), (&f, 23), (&f, 24), (&f, 25),
                ], 10, true, assets);
                figge.set_talk_anims("talkPlayer", "idle");
                figge.play("idle");
                self.actors.push(figge);

                // SalkaLeft — appears when dog is returned (initially hidden)
                let mut salka_l = Actor::new("salkaLeft", 200, 363, 18);
                salka_l.add_animation("idle", &[
                    (&f, 40), (&f, 41), (&f, 42), (&f, 43),
                    (&f, 44), (&f, 43), (&f, 42), (&f, 41),
                ], 15, true, assets);
                salka_l.visible = false;
                salka_l.play("idle");
                self.actors.push(salka_l);
            }
            94 => {
                self.load_carshow(assets);
                return;
            }
            _ => {
                // ─── Generic destination (82, 83, 84, 89, 90, 91, 93) ──
                // Load the largest bitmap as background
                self.load_largest_bg(&f, assets);
            }
        }

        // ─── Common: Mulle actor + back-to-world hotspot ────────────────
        if num != 94 {
            let mut mulle = Actor::new("mulleDefault", mulle_x, mulle_y, 25);
            mulle.add_animation("idle", &[(b, 271)], 10, true, assets);
            mulle.add_animation("lookPlayer", &[(b, 287), (b, 288)], 10, true, assets);
            mulle.add_animation("talkPlayer", &[
                (b, 289), (b, 290), (b, 291), (b, 292),
                (b, 293), (b, 294), (b, 295),
            ], 10, true, assets);
            mulle.add_animation("talkRegular", &[
                (b, 296), (b, 297), (b, 298), (b, 299),
                (b, 300), (b, 301), (b, 302),
            ], 10, true, assets);
            mulle.add_animation("lookLeft", &[(b, 283)], 10, true, assets);
            mulle.add_animation("turnBack", &[(b, 285)], 10, true, assets);
            mulle.add_animation("scratchChin", &[
                (b, 271), (b, 272), (b, 273), (b, 274), (b, 275), (b, 276),
            ], 10, false, assets);
            mulle.add_animation("scratchHead", &[
                (b, 277), (b, 278), (b, 279), (b, 280), (b, 281), (b, 282),
            ], 10, false, assets);
            mulle.set_talk_anims("talkRegular", "idle");
            mulle.play("idle");
            self.actors.push(mulle);

            self.hotspots.push(Hotspot {
                x: 0, y: 400, width: 640, height: 80,
                name: "← Weltkarte".into(),
                target: Some(Scene::World),
            });
        }
    }

    // ─── Generic fallback ──────────────────────────────────────────────

    fn load_generic(&mut self, assets: &AssetStore) {
        let file = match self.scene {
            Scene::CarGallery => "06",
            Scene::Boot => "18",
            _ => return,
        };
        let f = self.resolve_file(file, assets);
        self.load_largest_bg(&f, assets);

        // Always provide a way back
        self.hotspots.push(Hotspot {
            x: 0, y: 0, width: 80, height: 480,
            name: "← Zurück".into(),
            target: Some(Scene::Garage),
        });
    }

    // ─── Utility ───────────────────────────────────────────────────────

    pub(crate) fn resolve_file(&self, stem: &str, assets: &AssetStore) -> String {
        let dxr = format!("{}.DXR", stem);
        let cxt = format!("{}.CXT", stem);
        if assets.files.contains_key(&dxr) { dxr }
        else if assets.files.contains_key(&cxt) { cxt }
        else { dxr } // fallback
    }

    fn find_member_by_name(&self, file: &str, name: &str, assets: &AssetStore) -> Option<u32> {
        assets.files.get(file)?.cast_members.iter()
            .find(|(_, m)| m.name == name)
            .map(|(n, _)| *n)
    }

    fn load_largest_bg(&mut self, file: &str, assets: &AssetStore) {
        if let Some(df) = assets.files.get(file) {
            let mut best: Option<u32> = None;
            let mut best_px = 0u32;
            for (num, m) in &df.cast_members {
                if m.cast_type == CastType::Bitmap {
                    if let Some(bi) = &m.bitmap_info {
                        let px = bi.width as u32 * bi.height as u32;
                        if px > best_px {
                            best_px = px;
                            best = Some(*num);
                        }
                    }
                }
            }
            if let Some(num) = best {
                self.load_bg(file, num, assets);
            }
        }
    }

    // ─── Update (animation ticking) ────────────────────────────────────

    /// Tick all actors. Returns any events that occurred (e.g. animation finished).
    pub fn update(&mut self, _assets: &AssetStore) -> Vec<SceneEvent> {
        let mut events = Vec::new();
        for actor in &mut self.actors {
            if let Some(ActorEvent::AnimationFinished { actor_name, anim_name }) = actor.tick() {
                events.push(SceneEvent::ActorAnimFinished { actor_name, anim_name });
            }
        }
        events
    }

    // ─── Actor control (used by SceneScript) ────────────────────────────

    /// Play a named animation on a named actor
    pub fn play_actor_anim(&mut self, actor_name: &str, anim_name: &str) {
        for actor in &mut self.actors {
            if actor.name == actor_name {
                actor.play(anim_name);
                return;
            }
        }
        tracing::warn!("play_actor_anim: actor '{}' not found", actor_name);
    }

    /// Set actor visibility
    pub fn set_actor_visible(&mut self, actor_name: &str, visible: bool) {
        for actor in &mut self.actors {
            if actor.name == actor_name {
                actor.visible = visible;
                return;
            }
        }
        tracing::warn!("set_actor_visible: actor '{}' not found", actor_name);
    }

    /// Change an actor's talk/silence animation pair (e.g. for carshow judge scoring)
    pub fn set_actor_talk_anims(&mut self, actor_name: &str, talk: &str, silence: &str) {
        for actor in &mut self.actors {
            if actor.name == actor_name {
                actor.set_talk_anims(talk, silence);
                return;
            }
        }
        tracing::warn!("set_actor_talk_anims: actor '{}' not found", actor_name);
    }

    /// Mark an actor as the one currently talking (for cue-point routing)
    pub fn set_talking_actor(&mut self, actor_name: &str) {
        self.talking_actor = Some(actor_name.to_string());
        for actor in &mut self.actors {
            if actor.name == actor_name {
                actor.start_talking();
                return;
            }
        }
    }

    /// Stop the current talking actor's lip-sync
    pub fn stop_talking_actor(&mut self) {
        if let Some(name) = self.talking_actor.take() {
            for actor in &mut self.actors {
                if actor.name == name {
                    actor.stop_talking();
                    return;
                }
            }
        }
    }

    /// Forward a cue point to the currently talking actor
    pub fn handle_cue_point(&mut self, cue_name: &str) {
        if let Some(name) = &self.talking_actor {
            let name = name.clone();
            for actor in &mut self.actors {
                if actor.name == name {
                    actor.on_cue(cue_name);
                    break;
                }
            }
            // "point" cue: trigger the companion head actor's "point" animation
            // (mulle.js: mulleMenuMouth talks, mulleMenuHead does point gesture)
            if cue_name.eq_ignore_ascii_case("point") {
                let head_name = name.replace("Mouth", "Head");
                for actor in &mut self.actors {
                    if actor.name == head_name {
                        actor.play("point");
                        break;
                    }
                }
            }
        }
    }

    // ─── Drag & Drop integration ────────────────────────────────────────

    /// Process drag input. Returns DropResult if drop occurred.
    pub fn process_drag(&mut self, x: i32, y: i32, mouse_down: bool) -> DropResult {
        self.drag_drop.process_mouse(x, y, mouse_down)
    }

    // ─── All sprites (for rendering) ───────────────────────────────────

    pub fn all_sprites(&self) -> Vec<Sprite> {
        let mut result: Vec<Sprite> = self.sprites.clone();

        // Add button sprites
        for btn in &self.buttons {
            result.push(btn.as_sprite());
        }

        // Add actor sprites
        for actor in &self.actors {
            if let Some(s) = actor.current_sprite() {
                result.push(s);
            }
        }

        // Add draggable item sprites
        for sprite in self.drag_drop.all_sprites() {
            result.push(sprite);
        }

        // Sort by z_order
        result.sort_by_key(|s| s.z_order);
        result
    }

    // ─── Mouse interaction ─────────────────────────────────────────────

    pub fn on_mouse_move(&mut self, x: i32, y: i32) -> Option<String> {
        let mut hover_sound = None;
        for btn in &mut self.buttons {
            let was_hovered = btn.hovered;
            btn.hovered = btn.hit_test(x, y);
            // Trigger hover sound on entering button
            if btn.hovered && !was_hovered {
                if let Some(snd) = &btn.sound_hover {
                    hover_sound = Some(snd.clone());
                }
            }
        }
        hover_sound
    }

    /// Handle left click — buttons first, then sprites, then hotspots
    pub fn on_click(&mut self, x: i32, y: i32, assets: &AssetStore) -> Option<Scene> {
        // Menu UI elements take priority
        if self.scene == Scene::Menu {
            if let Some(next) = self.menu_click(x, y) {
                return Some(next);
            }
        }

        // Check buttons (highest z-order first)
        let mut clicked_btn: Option<(usize, i32)> = None;
        for (i, btn) in self.buttons.iter().enumerate() {
            if btn.hit_test(x, y) {
                if clicked_btn.is_none() || btn.z_order > clicked_btn.unwrap().1 {
                    clicked_btn = Some((i, btn.z_order));
                }
            }
        }
        if let Some((idx, _)) = clicked_btn {
            let btn = &self.buttons[idx];
            tracing::info!("CLICK button '{}' at ({},{})", btn.name, x, y);

            // Special handling for junkyard arrows (no target scene)
            if self.scene == Scene::Junkyard && btn.target.is_none() {
                if btn.name.starts_with('→') || btn.name.starts_with("→") {
                    let next = if self.junk_pile >= 6 { 1 } else { self.junk_pile + 1 };
                    self.junk_pile = next;
                    self.sprites.clear();
                    self.buttons.clear();
                    self.actors.clear();
                    self.hotspots.clear();
                    self.load_junkyard_pile(next, assets);
                    tracing::info!("Junkyard → Pile {}", next);
                    return None;
                }
                if btn.name.starts_with('←') || btn.name.starts_with("←") {
                    let prev = if self.junk_pile <= 1 { 6 } else { self.junk_pile - 1 };
                    self.junk_pile = prev;
                    self.sprites.clear();
                    self.buttons.clear();
                    self.actors.clear();
                    self.hotspots.clear();
                    self.load_junkyard_pile(prev, assets);
                    tracing::info!("Junkyard → Pile {}", prev);
                    return None;
                }
            }

            return btn.target;
        }

        // Check sprites (reverse z-order)
        let mut hit_sprites: Vec<(usize, i32)> = Vec::new();
        for (i, sprite) in self.sprites.iter().enumerate() {
            if sprite.hit_test(x, y) {
                hit_sprites.push((i, sprite.z_order));
            }
        }
        hit_sprites.sort_by(|a, b| b.1.cmp(&a.1));
        if let Some((idx, _)) = hit_sprites.first() {
            let sprite = &self.sprites[*idx];
            tracing::info!("CLICK sprite '{}' at ({},{})", sprite.name, x, y);
        }

        // Check hotspots
        for hotspot in &self.hotspots {
            if x >= hotspot.x && x < hotspot.x + hotspot.width as i32
                && y >= hotspot.y && y < hotspot.y + hotspot.height as i32
            {
                tracing::info!("Hotspot '{}' at ({},{})", hotspot.name, x, y);
                return hotspot.target;
            }
        }
        None
    }

    pub fn on_right_click(&mut self, x: i32, y: i32) {
        for sprite in self.sprites.iter_mut().rev() {
            if sprite.member_num == 0 { continue; }
            if sprite.visible && sprite.bbox_hit(x, y) {
                sprite.visible = false;
                tracing::debug!("Hide '{}' (right-click)", sprite.name);
                return;
            }
        }
        for sprite in self.sprites.iter_mut() {
            if !sprite.visible {
                let lx = x - sprite.x;
                let ly = y - sprite.y;
                if lx >= 0 && ly >= 0 && lx < sprite.width as i32 && ly < sprite.height as i32 {
                    sprite.visible = true;
                    tracing::info!("Show '{}' (right-click)", sprite.name);
                    return;
                }
            }
        }
    }

    pub fn hover_info(&self, x: i32, y: i32) -> String {
        // Buttons first
        for btn in &self.buttons {
            if btn.hit_test(x, y) {
                return format!("[{}]", btn.name);
            }
        }
        // Then sprites
        let mut best: Option<&Sprite> = None;
        let mut best_z = i32::MIN;
        for sprite in &self.sprites {
            if sprite.hit_test(x, y) && sprite.z_order > best_z {
                best = Some(sprite);
                best_z = sprite.z_order;
            }
        }
        if let Some(s) = best {
            return s.name.clone();
        }
        // Hotspots
        for hs in &self.hotspots {
            if x >= hs.x && x < hs.x + hs.width as i32
                && y >= hs.y && y < hs.y + hs.height as i32
            {
                return format!("[{}]", hs.name);
            }
        }
        String::new()
    }

    pub fn on_key_down(&mut self, key: Key, assets: &AssetStore) -> Option<Scene> {
        match key {
            Key::F1 => Some(Scene::Menu),
            Key::F2 => Some(Scene::Garage),
            Key::F3 => Some(Scene::Junkyard),
            Key::F4 => Some(Scene::Yard),
            Key::F5 => Some(Scene::World),
            Key::F6 => Some(Scene::CarShow),
            Key::F7 => Some(Scene::CarGallery),
            Key::F8 => Some(Scene::Destination(85)),
            Key::F9 => Some(Scene::Destination(92)),
            Key::Tab => {
                let all_vis = self.sprites.iter().all(|s| s.visible);
                for s in &mut self.sprites { s.visible = !all_vis; }
                tracing::info!("Tab: {} all sprites",
                    if all_vis { "HIDE" } else { "SHOW" });
                None
            }
            Key::Space => {
                let mut found = false;
                for s in &mut self.sprites {
                    if !s.visible && s.z_order > 0 {
                        s.visible = true;
                        tracing::info!("Space: show '{}' z={}", s.name, s.z_order);
                        found = true;
                        break;
                    }
                }
                if !found {
                    for s in &mut self.sprites { s.visible = s.z_order == 0; }
                    tracing::info!("Space: reset to background");
                }
                None
            }
            Key::Left if self.scene == Scene::Junkyard => {
                let prev = if self.junk_pile <= 1 { 6 } else { self.junk_pile - 1 };
                self.junk_pile = prev;
                self.sprites.clear();
                self.buttons.clear();
                self.actors.clear();
                self.hotspots.clear();
                self.load_junkyard_pile(prev, assets);
                None
            }
            Key::Right if self.scene == Scene::Junkyard => {
                let next = if self.junk_pile >= 6 { 1 } else { self.junk_pile + 1 };
                self.junk_pile = next;
                self.sprites.clear();
                self.buttons.clear();
                self.actors.clear();
                self.hotspots.clear();
                self.load_junkyard_pile(next, assets);
                None
            }
            _ => None,
        }
    }

    // ─── Menu UI ────────────────────────────────────────────────────────

    const NAME_FIELD_X: i32 = 90;
    const NAME_FIELD_Y: i32 = 60;
    const NAME_FIELD_W: i32 = 180;
    const NAME_FIELD_H: i32 = 22;

    const NAME_LIST_X: i32 = 350;
    const NAME_LIST_Y: i32 = 60;
    const NAME_LIST_W: i32 = 230;
    const NAME_LIST_H: i32 = 160;
    const NAME_LIST_ITEM_H: i32 = 25;

    const PLAY_BTN_X: i32 = 350;
    const PLAY_BTN_Y: i32 = 234;
    const PLAY_BTN_W: i32 = 110;
    const PLAY_BTN_H: i32 = 28;

    const DELETE_BTN_X: i32 = 470;
    const DELETE_BTN_Y: i32 = 234;
    const DELETE_BTN_W: i32 = 110;
    const DELETE_BTN_H: i32 = 28;

    pub fn on_char_input(&mut self, ch: char) {
        if self.scene != Scene::Menu { return; }
        if ch == '\x08' {
            self.input_text.pop();
        } else if ch.is_ascii_graphic() || ch == ' ' {
            if self.input_text.len() < 20 {
                self.input_text.push(ch);
            }
        }
    }

    pub fn on_enter(&mut self, _assets: &AssetStore) -> Option<Scene> {
        if self.scene != Scene::Menu { return None; }
        let name = self.effective_name();
        if name.is_empty() { return None; }
        tracing::info!("Starting game as '{}'", name);
        if !self.saved_names.iter().any(|n| n == &name) {
            self.saved_names.push(name);
        }
        Some(Scene::Garage)
    }

    fn effective_name(&self) -> String {
        if !self.input_text.trim().is_empty() {
            self.input_text.trim().to_string()
        } else if let Some(idx) = self.selected_name {
            self.saved_names.get(idx).cloned().unwrap_or_default()
        } else {
            String::new()
        }
    }

    fn menu_click(&mut self, x: i32, y: i32) -> Option<Scene> {
        if x >= Self::PLAY_BTN_X && x < Self::PLAY_BTN_X + Self::PLAY_BTN_W
            && y >= Self::PLAY_BTN_Y && y < Self::PLAY_BTN_Y + Self::PLAY_BTN_H
        {
            let name = self.effective_name();
            if !name.is_empty() {
                tracing::info!("Play → Garage as '{}'", name);
                if !self.saved_names.iter().any(|n| n == &name) {
                    self.saved_names.push(name);
                }
                return Some(Scene::Garage);
            }
        }
        if x >= Self::DELETE_BTN_X && x < Self::DELETE_BTN_X + Self::DELETE_BTN_W
            && y >= Self::DELETE_BTN_Y && y < Self::DELETE_BTN_Y + Self::DELETE_BTN_H
        {
            if let Some(idx) = self.selected_name {
                if idx < self.saved_names.len() {
                    self.saved_names.remove(idx);
                    self.selected_name = None;
                }
            }
        }
        if x >= Self::NAME_LIST_X && x < Self::NAME_LIST_X + Self::NAME_LIST_W
            && y >= Self::NAME_LIST_Y && y < Self::NAME_LIST_Y + Self::NAME_LIST_H
        {
            let rel_y = y - Self::NAME_LIST_Y;
            let idx = (rel_y / Self::NAME_LIST_ITEM_H) as usize;
            if idx < self.saved_names.len() {
                self.selected_name = Some(idx);
                self.input_text.clear();
            }
        }
        None
    }

    pub fn draw_ui(&mut self, fb: &mut [u32]) {
        if self.scene != Scene::Menu { return; }

        self.frame_counter = self.frame_counter.wrapping_add(1);
        self.cursor_visible = (self.frame_counter / 15) % 2 == 0;

        // Name input field
        font::draw_rect(fb, Self::NAME_FIELD_X, Self::NAME_FIELD_Y,
            Self::NAME_FIELD_W, Self::NAME_FIELD_H, 0xFFFFFFFF);
        font::draw_rect_outline(fb, Self::NAME_FIELD_X, Self::NAME_FIELD_Y,
            Self::NAME_FIELD_W, Self::NAME_FIELD_H, 0xFF333333);
        font::draw_text_shadow(fb, Self::NAME_FIELD_X, Self::NAME_FIELD_Y - 14,
            "Dein Name:", 0xFFFFFF00);

        let display_text = self.input_text.clone();
        font::draw_text(fb, Self::NAME_FIELD_X + 4, Self::NAME_FIELD_Y + 6,
            &display_text, 0xFF000000);
        if self.cursor_visible && self.selected_name.is_none() {
            let cx = Self::NAME_FIELD_X + 4 + font::text_width(&display_text);
            font::draw_text(fb, cx, Self::NAME_FIELD_Y + 6, "_", 0xFF000000);
        }

        // Name list
        font::draw_rect(fb, Self::NAME_LIST_X, Self::NAME_LIST_Y,
            Self::NAME_LIST_W, Self::NAME_LIST_H, 0xFFFFFFFF);
        font::draw_rect_outline(fb, Self::NAME_LIST_X, Self::NAME_LIST_Y,
            Self::NAME_LIST_W, Self::NAME_LIST_H, 0xFF333333);

        if self.saved_names.is_empty() {
            font::draw_text(fb, Self::NAME_LIST_X + 8, Self::NAME_LIST_Y + 8,
                "(keine Namen)", 0xFF999999);
        } else {
            for i in 0..self.saved_names.len() {
                let iy = Self::NAME_LIST_Y + 2 + (i as i32 * Self::NAME_LIST_ITEM_H);
                if iy + Self::NAME_LIST_ITEM_H > Self::NAME_LIST_Y + Self::NAME_LIST_H { break; }
                let name = self.saved_names[i].clone();
                if self.selected_name == Some(i) {
                    font::draw_rect(fb, Self::NAME_LIST_X + 1, iy,
                        Self::NAME_LIST_W - 2, Self::NAME_LIST_ITEM_H, 0xFF4444AA);
                    font::draw_text(fb, Self::NAME_LIST_X + 8, iy + 5,
                        &name, 0xFFFFFFFF);
                } else {
                    font::draw_text(fb, Self::NAME_LIST_X + 8, iy + 5,
                        &name, 0xFF000000);
                }
            }
        }

        // Spielen button
        let has_name = !self.effective_name().is_empty();
        let bg = if has_name { 0xFF228B22 } else { 0xFF666666 };
        font::draw_rect(fb, Self::PLAY_BTN_X, Self::PLAY_BTN_Y,
            Self::PLAY_BTN_W, Self::PLAY_BTN_H, bg);
        font::draw_rect_outline(fb, Self::PLAY_BTN_X, Self::PLAY_BTN_Y,
            Self::PLAY_BTN_W, Self::PLAY_BTN_H, 0xFF003300);
        let pt = "Spielen";
        font::draw_text_shadow(fb,
            Self::PLAY_BTN_X + (Self::PLAY_BTN_W - font::text_width(pt)) / 2,
            Self::PLAY_BTN_Y + (Self::PLAY_BTN_H - 8) / 2, pt, 0xFFFFFFFF);

        // Löschen button
        font::draw_rect(fb, Self::DELETE_BTN_X, Self::DELETE_BTN_Y,
            Self::DELETE_BTN_W, Self::DELETE_BTN_H, 0xFFAA3333);
        font::draw_rect_outline(fb, Self::DELETE_BTN_X, Self::DELETE_BTN_Y,
            Self::DELETE_BTN_W, Self::DELETE_BTN_H, 0xFF550000);
        let dt = "Loeschen";
        font::draw_text_shadow(fb,
            Self::DELETE_BTN_X + (Self::DELETE_BTN_W - font::text_width(dt)) / 2,
            Self::DELETE_BTN_Y + (Self::DELETE_BTN_H - 8) / 2, dt, 0xFFFFFFFF);

        // Hint
        font::draw_text_shadow(fb, Self::NAME_LIST_X, Self::DELETE_BTN_Y + 40,
            "Enter=Spielen | F1-F9=Szene", 0xFFCCCCCC);
    }
}
