//! Developer menu — hidden panel activated by pressing # five times quickly.
//!
//! Contains cheat toggles, dev triggers (scene warps, refuel), and the
//! meme physics mode.  Opened/closed with 5× '#' within 2 seconds,
//! confirmed by a quiet beep sound.

use std::time::Instant;
use crate::engine::font;
use crate::engine::{SCREEN_WIDTH, SCREEN_HEIGHT};
use crate::game::Scene;

// ─── Menu definition ────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum ItemKind {
    Toggle,
    Trigger,
    Close,
}

struct MenuItem {
    label: &'static str,
    kind: ItemKind,
}

const MENU: &[MenuItem] = &[
    // ── Cheats ──
    MenuItem { label: "Unendlich Benzin",      kind: ItemKind::Toggle },  // 0
    MenuItem { label: "Noclip (durch Waende)", kind: ItemKind::Toggle },  // 1
    MenuItem { label: "Hitboxen anzeigen",     kind: ItemKind::Toggle },  // 2
    MenuItem { label: "Dialoge ueberspringen", kind: ItemKind::Toggle }, // 3
    MenuItem { label: "Meme-Modus",            kind: ItemKind::Toggle },  // 4
    // ── Video ──
    MenuItem { label: "Detail-Rauschen",       kind: ItemKind::Toggle },  // 5
    // ── Dev Triggers ──
    MenuItem { label: "-> Werkstatt",          kind: ItemKind::Trigger }, // 6
    MenuItem { label: "-> Hof",                kind: ItemKind::Trigger }, // 7
    MenuItem { label: "-> Weltkarte",          kind: ItemKind::Trigger }, // 8
    MenuItem { label: "-> Autoshow",           kind: ItemKind::Trigger }, // 9
    MenuItem { label: "-> Schrottplatz",       kind: ItemKind::Trigger }, // 10
    MenuItem { label: "Tank auffuellen",       kind: ItemKind::Trigger }, // 11
    MenuItem { label: "Figge in Werkstatt",    kind: ItemKind::Trigger }, // 12
    // ── Close ──
    MenuItem { label: "Schliessen",            kind: ItemKind::Close },   // 13
];

// ─── Public types ───────────────────────────────────────────────────────

/// Result from a dev-menu interaction
pub enum DevAction {
    /// Nothing happened / toggle was flipped in place
    None,
    /// Menu was closed
    Close,
    /// Warp to a different scene
    GotoScene(Scene),
    /// Refill the driving car's fuel tank
    RefuelTank,
    /// Set #FiggeIsComing and go to Garage to trigger Figge cutscene
    TriggerFigge,
}

/// The dev menu state
pub struct DevMenu {
    pub open: bool,
    pub selected: usize,

    // ── Cheat toggles ──
    pub infinite_fuel: bool,
    pub noclip: bool,
    pub show_hitboxes: bool,
    pub skip_dialogs: bool,
    pub meme_mode: bool,

    // ── Video ──
    pub detail_noise: bool,

    // ── Activation detector ──
    hash_times: Vec<Instant>,
}

// ─── Implementation ─────────────────────────────────────────────────────

impl DevMenu {
    pub fn new() -> Self {
        Self {
            open: false,
            selected: 0,
            infinite_fuel: false,
            noclip: false,
            show_hitboxes: false,
            skip_dialogs: false,
            meme_mode: false,
            detail_noise: false,
            hash_times: Vec::new(),
        }
    }

    /// Record a '#' key press.  Returns `true` when 5 presses landed inside
    /// a 2-second window → the menu should be toggled and the beep played.
    pub fn on_hash_press(&mut self) -> bool {
        let now = Instant::now();
        self.hash_times.push(now);
        // Keep only recent presses
        self.hash_times
            .retain(|t| now.duration_since(*t).as_millis() < 2000);
        if self.hash_times.len() >= 5 {
            self.hash_times.clear();
            self.open = !self.open;
            self.selected = 0;
            true
        } else {
            false
        }
    }

    // ── Keyboard navigation ─────────────────────────────────────────────

    pub fn nav_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn nav_down(&mut self) {
        if self.selected < MENU.len() - 1 {
            self.selected += 1;
        }
    }

    /// Activate the currently selected item.
    pub fn activate(&mut self) -> DevAction {
        let item = &MENU[self.selected];
        match item.kind {
            ItemKind::Toggle => {
                self.flip_toggle(self.selected);
                DevAction::None
            }
            ItemKind::Trigger => self.fire_trigger(self.selected),
            ItemKind::Close => {
                self.open = false;
                DevAction::Close
            }
        }
    }

    /// Handle a mouse click while the menu is open.
    pub fn on_click(&mut self, mx: i32, my: i32) -> DevAction {
        let (box_x, box_y, box_w, _box_h, item_h) = Self::layout();
        let items_y = box_y + 40;
        for i in 0..MENU.len() {
            let iy = items_y + i as i32 * item_h;
            if mx >= box_x + 4
                && mx < box_x + box_w - 4
                && my >= iy
                && my < iy + item_h
            {
                self.selected = i;
                return self.activate();
            }
        }
        DevAction::None
    }

    // ── Rendering ───────────────────────────────────────────────────────

    /// Draw the dev-menu overlay onto the 640×480 framebuffer.
    pub fn draw(&self, fb: &mut [u32]) {
        if !self.open {
            return;
        }

        // Darken background
        for pixel in fb.iter_mut() {
            let r = (*pixel >> 16) & 0xFF;
            let g = (*pixel >> 8) & 0xFF;
            let b = *pixel & 0xFF;
            *pixel = 0xFF000000 | ((r / 3) << 16) | ((g / 3) << 8) | (b / 3);
        }

        let (box_x, box_y, box_w, box_h, item_h) = Self::layout();

        // Panel
        font::draw_rect(fb, box_x, box_y, box_w, box_h, 0xFF0d0d1a);
        font::draw_rect_outline(fb, box_x, box_y, box_w, box_h, 0xFF00CC66);
        font::draw_rect_outline(
            fb,
            box_x + 2,
            box_y + 2,
            box_w - 4,
            box_h - 4,
            0xFF006633,
        );

        // Title
        let title = "~ DEV MENU ~";
        font::draw_text_shadow(
            fb,
            box_x + (box_w - font::text_width(title)) / 2,
            box_y + 14,
            title,
            0xFF00FF88,
        );

        // Items
        let items_y = box_y + 40;
        for (i, item) in MENU.iter().enumerate() {
            let iy = items_y + i as i32 * item_h;
            let is_sel = i == self.selected;

            // Highlight
            if is_sel {
                font::draw_rect(fb, box_x + 4, iy, box_w - 8, item_h - 2, 0xFF1a3326);
            }

            // Section dividers
            if i == 5 || i == 6 || i == MENU.len() - 1 {
                font::draw_rect(fb, box_x + 10, iy - 2, box_w - 20, 1, 0xFF336644);
            }

            // Label + toggle state
            let prefix = if is_sel { "> " } else { "  " };
            let suffix = match self.toggle_state(i) {
                Some(true) => " [ON]",
                Some(false) => " [OFF]",
                None => "",
            };

            let color = if is_sel {
                0xFFFFFFFF
            } else {
                match item.kind {
                    ItemKind::Toggle => {
                        if self.toggle_state(i).unwrap_or(false) {
                            0xFF00FF88
                        } else {
                            0xFFBBBBBB
                        }
                    }
                    ItemKind::Trigger => 0xFFFFCC44,
                    ItemKind::Close => 0xFF888888,
                }
            };

            let text = format!("{}{}{}", prefix, item.label, suffix);
            font::draw_text_shadow(fb, box_x + 16, iy + 4, &text, color);
        }

        // Footer
        font::draw_text(
            fb,
            box_x + 10,
            box_y + box_h - 18,
            "Up/Down + Enter | Klick | 5x# schliesst",
            0xFF446655,
        );
    }

    // ── Internals ───────────────────────────────────────────────────────

    fn layout() -> (i32, i32, i32, i32, i32) {
        let item_h: i32 = 22;
        let box_w: i32 = 320;
        let box_h: i32 = 50 + MENU.len() as i32 * item_h;
        let box_x = (SCREEN_WIDTH as i32 - box_w) / 2;
        let box_y = (SCREEN_HEIGHT as i32 - box_h) / 2;
        (box_x, box_y, box_w, box_h, item_h)
    }

    fn toggle_state(&self, idx: usize) -> Option<bool> {
        match idx {
            0 => Some(self.infinite_fuel),
            1 => Some(self.noclip),
            2 => Some(self.show_hitboxes),
            3 => Some(self.skip_dialogs),
            4 => Some(self.meme_mode),
            5 => Some(self.detail_noise),
            _ => None,
        }
    }

    fn flip_toggle(&mut self, idx: usize) {
        match idx {
            0 => self.infinite_fuel = !self.infinite_fuel,
            1 => self.noclip = !self.noclip,
            2 => self.show_hitboxes = !self.show_hitboxes,
            3 => self.skip_dialogs = !self.skip_dialogs,
            4 => self.meme_mode = !self.meme_mode,
            5 => self.detail_noise = !self.detail_noise,
            _ => {}
        }
        let name = MENU.get(idx).map(|m| m.label).unwrap_or("?");
        let state = self.toggle_state(idx);
        tracing::info!("Dev toggle '{}' → {:?}", name, state);
    }

    fn fire_trigger(&mut self, idx: usize) -> DevAction {
        self.open = false;
        match idx {
            6 => DevAction::GotoScene(Scene::Garage),
            7 => DevAction::GotoScene(Scene::Yard),
            8 => DevAction::GotoScene(Scene::World),
            9 => DevAction::GotoScene(Scene::CarShow),
            10 => DevAction::GotoScene(Scene::Junkyard),
            11 => DevAction::RefuelTank,
            12 => DevAction::TriggerFigge,
            _ => DevAction::None,
        }
    }
}
