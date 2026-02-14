//! Software-rendered cursor system
//!
//! Loads 9 cursor sprites from 00.DXR (members 73-81) with per-cursor
//! hotspots matching the original Director game.  Manages a cursor
//! **stack** identical to mulle.js `MulleCursor`: pushing a type
//! overrides the current cursor, popping restores the previous one.
//! The engine hides the OS cursor and blits the software cursor onto
//! the framebuffer every frame.

use crate::assets::AssetStore;

/// All cursor types available in the game (00.DXR members 73-81)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorType {
    Standard,  // member 73 — default arrow
    Grab,      // member 74 — open hand (hover over parts)
    Left,      // member 75 — left arrow (left door)
    Click,     // member 76 — pointing finger (clickable)
    Back,      // member 77 — return arrow
    Right,     // member 78 — right arrow (right door)
    MoveLeft,  // member 79 — drag left
    MoveRight, // member 80 — drag right
    MoveIn,    // member 81 — drag forward
}

impl CursorType {
    /// 00.DXR member number
    fn member(self) -> u32 {
        match self {
            CursorType::Standard  => 73,
            CursorType::Grab      => 74,
            CursorType::Left      => 75,
            CursorType::Click     => 76,
            CursorType::Back      => 77,
            CursorType::Right     => 78,
            CursorType::MoveLeft  => 79,
            CursorType::MoveRight => 80,
            CursorType::MoveIn    => 81,
        }
    }

    /// Hotspot (x, y) — the pixel in the cursor image that represents
    /// the actual click point.  Values from mulle.js `style.scss`.
    fn hotspot(self) -> (i32, i32) {
        match self {
            CursorType::Standard  => (5, 1),
            CursorType::Grab      => (13, 10),
            CursorType::Left      => (0, 5),
            CursorType::Click     => (1, 1),
            CursorType::Back      => (14, 7),
            CursorType::Right     => (27, 5),
            CursorType::MoveLeft  => (20, 17),
            CursorType::MoveRight => (16, 15),
            CursorType::MoveIn    => (15, 16),
        }
    }

    fn index(self) -> usize {
        match self {
            CursorType::Standard  => 0,
            CursorType::Grab      => 1,
            CursorType::Left      => 2,
            CursorType::Click     => 3,
            CursorType::Back      => 4,
            CursorType::Right     => 5,
            CursorType::MoveLeft  => 6,
            CursorType::MoveRight => 7,
            CursorType::MoveIn    => 8,
        }
    }

    const ALL: [CursorType; 9] = [
        CursorType::Standard,
        CursorType::Grab,
        CursorType::Left,
        CursorType::Click,
        CursorType::Back,
        CursorType::Right,
        CursorType::MoveLeft,
        CursorType::MoveRight,
        CursorType::MoveIn,
    ];

    /// Director cast member name used for name-based fallback lookup.
    /// In 00.CXT the members are named "C_standard", "C_Grab", etc.
    fn director_name(self) -> &'static str {
        match self {
            CursorType::Standard  => "C_standard",
            CursorType::Grab      => "C_Grab",
            CursorType::Left      => "C_Left",
            CursorType::Click     => "C_Click",
            CursorType::Back      => "C_Back",
            CursorType::Right     => "C_Right",
            CursorType::MoveLeft  => "C_MoveLeft",
            CursorType::MoveRight => "C_MoveRight",
            CursorType::MoveIn    => "C_MoveIn",
        }
    }
}

/// Pre-decoded cursor bitmap
struct CursorFrame {
    width: u32,
    height: u32,
    pixels: Vec<u8>, // RGBA
    hotspot_x: i32,
    hotspot_y: i32,
}

/// Software cursor with a stack for nested states.
pub struct GameCursor {
    frames: Vec<CursorFrame>,
    /// Stack of cursor types — last entry is the active cursor.
    /// Empty stack → Standard cursor.
    history: Vec<CursorType>,
}

impl GameCursor {
    /// Load all cursor sprites from 00.DXR/CXT.
    ///
    /// The cursor bitmaps live in the shared cast (00.DXR or 00.CXT).
    /// In DXR builds they are members 73–81 directly.  In some CXT
    /// builds the member numbering differs, so we also accept a name-
    /// based lookup as fallback: the Director member names match the
    /// `CursorType` debug names (e.g. "Standard", "Grab", …).
    pub fn new(assets: &AssetStore) -> Self {
        let file = if assets.files.contains_key("00.DXR") {
            "00.DXR"
        } else if assets.files.contains_key("00.CXT") {
            "00.CXT"
        } else {
            tracing::warn!("Cursor: no 00.DXR/CXT found");
            return Self { frames: Vec::new(), history: Vec::new() };
        };

        let mut frames = Vec::with_capacity(9);
        for ct in &CursorType::ALL {
            let (hx, hy) = ct.hotspot();

            // Primary: search by Director cast member name (reliable across DXR/CXT builds
            // where member numbering differs)
            let bmp = {
                let name = ct.director_name();
                let df = assets.files.get(file);
                df.and_then(|df| {
                    let (&num, _) = df.cast_members.iter()
                        .find(|(_, m)| m.name.eq_ignore_ascii_case(name) && m.cast_type == crate::assets::director::CastType::Bitmap)?;
                    tracing::debug!("Cursor: resolved {:?} by name '{}' → member {}", ct, name, num);
                    assets.decode_bitmap_transparent(file, num)
                })
            }
            // Fallback: try hardcoded member number (DXR layout)
            .or_else(|| assets.decode_bitmap_transparent(file, ct.member()));

            if let Some(bmp) = bmp {
                frames.push(CursorFrame {
                    width: bmp.width,
                    height: bmp.height,
                    pixels: bmp.pixels,
                    hotspot_x: hx,
                    hotspot_y: hy,
                });
            } else {
                tracing::warn!("Cursor: missing member {} ({:?})", ct.member(), ct);
                // 1×1 transparent stub
                frames.push(CursorFrame {
                    width: 1, height: 1,
                    pixels: vec![0, 0, 0, 0],
                    hotspot_x: 0,
                    hotspot_y: 0,
                });
            }
        }

        GameCursor { frames, history: Vec::new() }
    }

    /// Current active cursor type (top of stack, or Standard)
    pub fn current(&self) -> CursorType {
        self.history.last().copied().unwrap_or(CursorType::Standard)
    }

    /// Push a cursor type onto the stack (makes it the active cursor).
    pub fn set(&mut self, ct: CursorType) {
        self.history.push(ct);
    }

    /// Pop the top cursor off the stack (restoring the previous one).
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.history.pop();
    }

    /// Clear the entire stack (back to Standard).
    pub fn reset(&mut self) {
        self.history.clear();
    }

    /// Remove a specific cursor type from the stack (like mulle.js `remove()`).
    #[allow(dead_code)]
    pub fn remove(&mut self, ct: CursorType) {
        if let Some(idx) = self.history.iter().position(|&c| c == ct) {
            self.history.remove(idx);
        }
    }

    /// Blit the current cursor onto the framebuffer at (mouse_x, mouse_y).
    /// The hotspot offset is applied so the click-point aligns with the
    /// mouse position.
    pub fn blit(&self, fb: &mut [u32], fb_width: usize, fb_height: usize, mouse_x: i32, mouse_y: i32) {
        if self.frames.is_empty() {
            return;
        }
        let ct = self.current();
        let frame = &self.frames[ct.index()];

        let draw_x = mouse_x - frame.hotspot_x;
        let draw_y = mouse_y - frame.hotspot_y;

        for sy in 0..frame.height as i32 {
            let dy = draw_y + sy;
            if dy < 0 || dy >= fb_height as i32 {
                continue;
            }
            for sx in 0..frame.width as i32 {
                let dx = draw_x + sx;
                if dx < 0 || dx >= fb_width as i32 {
                    continue;
                }

                let src_idx = (sy as usize * frame.width as usize + sx as usize) * 4;
                if src_idx + 3 >= frame.pixels.len() {
                    continue;
                }

                let r = frame.pixels[src_idx] as u32;
                let g = frame.pixels[src_idx + 1] as u32;
                let b = frame.pixels[src_idx + 2] as u32;
                let a = frame.pixels[src_idx + 3] as u32;

                if a == 0 {
                    continue;
                }

                let dst_idx = dy as usize * fb_width + dx as usize;

                if a >= 255 {
                    fb[dst_idx] = 0xFF000000 | (r << 16) | (g << 8) | b;
                } else {
                    let dst = fb[dst_idx];
                    let dr = (dst >> 16) & 0xFF;
                    let dg = (dst >> 8) & 0xFF;
                    let db = dst & 0xFF;
                    let inv_a = 255 - a;
                    let out_r = (r * a + dr * inv_a) / 255;
                    let out_g = (g * a + dg * inv_a) / 255;
                    let out_b = (b * a + db * inv_a) / 255;
                    fb[dst_idx] = 0xFF000000 | (out_r << 16) | (out_g << 8) | out_b;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_stack_operations() {
        let mut gc = GameCursor { frames: Vec::new(), history: Vec::new() };
        assert_eq!(gc.current(), CursorType::Standard);

        gc.set(CursorType::Grab);
        assert_eq!(gc.current(), CursorType::Grab);

        gc.set(CursorType::Click);
        assert_eq!(gc.current(), CursorType::Click);

        gc.clear();
        assert_eq!(gc.current(), CursorType::Grab);

        gc.clear();
        assert_eq!(gc.current(), CursorType::Standard);

        // remove by type
        gc.set(CursorType::Left);
        gc.set(CursorType::MoveLeft);
        gc.remove(CursorType::Left);
        assert_eq!(gc.current(), CursorType::MoveLeft);
        gc.clear();
        assert_eq!(gc.current(), CursorType::Standard);
    }

    #[test]
    fn cursor_reset_clears_all() {
        let mut gc = GameCursor { frames: Vec::new(), history: Vec::new() };
        gc.set(CursorType::Grab);
        gc.set(CursorType::Click);
        gc.set(CursorType::Right);
        gc.reset();
        assert_eq!(gc.current(), CursorType::Standard);
        assert!(gc.history.is_empty());
    }

    #[test]
    fn hotspot_values_match_mulle_js() {
        // Verify hotspot values from mulle.js style.scss
        assert_eq!(CursorType::Standard.hotspot(), (5, 1));
        assert_eq!(CursorType::Grab.hotspot(), (13, 10));
        assert_eq!(CursorType::Click.hotspot(), (1, 1));
        assert_eq!(CursorType::Right.hotspot(), (27, 5));
    }
}
