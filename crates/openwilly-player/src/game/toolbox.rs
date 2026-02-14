//! Toolbox / Popup menu for the driving world view
//!
//! A tab at the right edge of the screen that can be clicked to open a popup
//! menu with Home/Quit/Cancel buttons (plus Steering/Diploma placeholders).

use crate::assets::AssetStore;
use crate::engine::Sprite;

/// Popup menu button regions (in popup-local coordinates)
struct MenuButton {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    action: PopupAction,
    #[allow(dead_code)] // Will be used for popup button hover sounds
    hover_sound: &'static str,
}

/// What happens when a popup button is clicked
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PopupAction {
    /// Go back to Yard
    Home,
    /// Go back to main Menu
    Quit,
    /// Close the popup
    Cancel,
    /// Toggle keyboard/mouse steering
    Steering,
    /// Show earned medals / diploma info
    Diploma,
}

/// Toolbox state for the world scene
pub struct Toolbox {
    /// Whether the popup menu is currently showing
    pub popup_open: bool,
    /// Pre-loaded toolbox icon sprite data (00.CXT member 97)
    icon: Option<SpriteData>,
    /// Pre-loaded popup menu sprite data (05.DXR member 53)
    popup: Option<SpriteData>,
    /// Whether the toolbox icon is currently hovered
    pub hovered: bool,
    /// Whether hint sound was already played this hover
    hover_sound_played: bool,
}

struct SpriteData {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    member_num: u32,
}

/// Icon position
const ICON_X: i32 = 659;
const ICON_Y: i32 = 439;
const ICON_SLIDE: i32 = 40;

/// Popup hover sound for toolbox tab
pub const TOOLBOX_HOVER_SOUND: &str = "00e040v0";

/// Menu button definitions with click regions (relative to popup sprite origin)
const MENU_BUTTONS: [MenuButton; 5] = [
    MenuButton { x: 116, y: 74,  w: 81, h: 130, action: PopupAction::Steering, hover_sound: "09d005v0" },
    MenuButton { x: 216, y: 76,  w: 65, h: 133, action: PopupAction::Home,     hover_sound: "09d006v0" },
    MenuButton { x: 316, y: 87,  w: 52, h: 137, action: PopupAction::Diploma,  hover_sound: "09d002v0" },
    MenuButton { x: 390, y: 85,  w: 70, h: 141, action: PopupAction::Quit,     hover_sound: "09d003v0" },
    MenuButton { x: 470, y: 215, w: 58, h: 110, action: PopupAction::Cancel,   hover_sound: "09d004v0" },
];

impl Toolbox {
    /// Create a new toolbox, pre-loading sprites from assets
    pub fn new(assets: &AssetStore) -> Self {
        // Load toolbox icon from 00.CXT member 97
        let icon = load_sprite(assets, "00.CXT", 97, ICON_X, ICON_Y);

        // Load popup menu from 05.DXR member 53
        let popup_file = if assets.files.contains_key("05.DXR") { "05.DXR" } else { "05.CXT" };
        let popup = load_sprite(assets, popup_file, 53, 0, 0).map(|mut s| {
            // Center the popup on screen
            s.x = (640 - s.width as i32) / 2;
            s.y = (480 - s.height as i32) / 2;
            s
        });

        Toolbox {
            popup_open: false,
            icon,
            popup,
            hovered: false,
            hover_sound_played: false,
        }
    }

    /// Get the sprites to render this frame
    pub fn sprites(&self) -> Vec<Sprite> {
        let mut out = Vec::new();

        // Toolbox icon (slides left 40px when hovered)
        if let Some(icon) = &self.icon {
            let x = if self.hovered { icon.x - ICON_SLIDE } else { icon.x };
            out.push(Sprite {
                x,
                y: icon.y,
                width: icon.width,
                height: icon.height,
                pixels: icon.pixels.clone(),
                visible: true,
                z_order: 52, // above dashboard
                name: "toolbox".into(),
                interactive: true,
                member_num: icon.member_num,
            });
        }

        // Popup menu (when open)
        if self.popup_open {
            if let Some(popup) = &self.popup {
                out.push(Sprite {
                    x: popup.x,
                    y: popup.y,
                    width: popup.width,
                    height: popup.height,
                    pixels: popup.pixels.clone(),
                    visible: true,
                    z_order: 100, // on top of everything
                    name: "popup_menu".into(),
                    interactive: false,
                    member_num: popup.member_num,
                });
            }
        }

        out
    }

    /// Check if a click hits the toolbox icon
    pub fn icon_hit(&self, x: i32, y: i32) -> bool {
        if let Some(icon) = &self.icon {
            let ix = if self.hovered { icon.x - ICON_SLIDE } else { icon.x };
            x >= ix && y >= icon.y
                && x < ix + icon.width as i32
                && y < icon.y + icon.height as i32
        } else {
            false
        }
    }

    /// Check if a click hits any popup button. Returns the action if hit.
    pub fn popup_hit(&self, screen_x: i32, screen_y: i32) -> Option<PopupAction> {
        if !self.popup_open {
            return None;
        }
        let popup = self.popup.as_ref()?;
        // Convert screen coords to popup-local coords
        let lx = screen_x - popup.x;
        let ly = screen_y - popup.y;

        for btn in &MENU_BUTTONS {
            if lx >= btn.x && ly >= btn.y
                && lx < btn.x + btn.w
                && ly < btn.y + btn.h
            {
                return Some(btn.action);
            }
        }
        None
    }



    /// Update hover state based on mouse position
    pub fn update_hover(&mut self, x: i32, y: i32) -> bool {
        let was_hovered = self.hovered;
        self.hovered = self.icon_hit(x, y) || (was_hovered && {
            // When already hovered, use wider hitbox (slid-out position)
            if let Some(icon) = &self.icon {
                let ix = icon.x - ICON_SLIDE;
                x >= ix && y >= icon.y
                    && x < ix + icon.width as i32
                    && y < icon.y + icon.height as i32
            } else { false }
        });

        // Return true if hover just started (for sound trigger)
        let just_hovered = self.hovered && !was_hovered;
        if !self.hovered {
            self.hover_sound_played = false;
        }
        if just_hovered && !self.hover_sound_played {
            self.hover_sound_played = true;
            return true;
        }
        false
    }

    /// Toggle the popup menu open/closed
    pub fn toggle(&mut self) {
        self.popup_open = !self.popup_open;
    }
}

fn load_sprite(assets: &AssetStore, file: &str, member: u32, default_x: i32, default_y: i32) -> Option<SpriteData> {
    let bmp = assets.decode_bitmap_transparent(file, member)?;
    let (px, py) = assets.files.get(file)
        .and_then(|df| df.cast_members.get(&member))
        .and_then(|m| m.bitmap_info.as_ref())
        .map(|bi| (bi.pos_x as i32, bi.pos_y as i32))
        .unwrap_or((default_x, default_y));
    Some(SpriteData {
        x: if px == 0 && py == 0 { default_x } else { px },
        y: if px == 0 && py == 0 { default_y } else { py },
        width: bmp.width,
        height: bmp.height,
        pixels: bmp.pixels,
        member_num: member,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_button_regions_non_overlapping() {
        // Ensure no two buttons overlap
        for (i, a) in MENU_BUTTONS.iter().enumerate() {
            for (j, b) in MENU_BUTTONS.iter().enumerate() {
                if i == j { continue; }
                let overlap_x = a.x < b.x + b.w && a.x + a.w > b.x;
                let overlap_y = a.y < b.y + b.h && a.y + a.h > b.y;
                assert!(!(overlap_x && overlap_y),
                    "Buttons {} ({:?}) and {} ({:?}) overlap", i, a.action, j, b.action);
            }
        }
    }

    #[test]
    fn popup_toggle() {
        // No assets → icon/popup are None, but toggle still works
        let mut tb = Toolbox {
            popup_open: false,
            icon: None,
            popup: None,
            hovered: false,
            hover_sound_played: false,
        };
        assert!(!tb.popup_open);
        tb.toggle();
        assert!(tb.popup_open);
        tb.toggle();
        assert!(!tb.popup_open);
    }
}
