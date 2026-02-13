//! Dashboard HUD for the driving world view
//!
//! Renders:
//! - Fuel needle (05.DXR members 27-42, 16 frames)
//! - Speedometer  (05.DXR member 46, slides horizontally)
//!
//! The dashboard background (member 25) is loaded as a scene overlay
//! with z_order=50, so the speedometer sits *below* it (z=49) to
//! naturally mask the hidden portion.

use crate::assets::AssetStore;
use crate::engine::Sprite;

/// Pre-decoded dashboard sprite data
pub struct Dashboard {
    /// 16 fuel-needle frames: (x, y, w, h, pixels)
    fuel_frames: Vec<FrameData>,
    /// Speedometer bitmap
    speedo: FrameData,
}

struct FrameData {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

/// Fuel needle: member 27 = empty, member 42 = full → 16 frames
const FUEL_MEMBERS: [u32; 16] = [27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42];

/// Speedometer: member 46
const SPEEDO_MEMBER: u32 = 46;

/// Speedo resting x (speed = 0) and max travel distance in pixels
const SPEEDO_BASE_X: f32 = 100.0;
const SPEEDO_TRAVEL: f32 = 140.0;

impl Dashboard {
    /// Load all dashboard sprites from 05.DXR/.CXT.
    /// Returns `None` only if the DXR is completely missing.
    pub fn new(assets: &AssetStore) -> Option<Self> {
        let file = if assets.files.contains_key("05.DXR") {
            "05.DXR"
        } else if assets.files.contains_key("05.CXT") {
            "05.CXT"
        } else {
            tracing::warn!("Dashboard: no 05.DXR/CXT found");
            return None;
        };

        // Load 16 fuel-needle frames
        let mut fuel_frames = Vec::with_capacity(16);
        for &mem in &FUEL_MEMBERS {
            if let Some(bmp) = assets.decode_bitmap_transparent(file, mem) {
                // Fuel needle anchor: (491, 447) per mulle.js
                let (rx, ry) = reg_point(assets, file, mem);
                fuel_frames.push(FrameData {
                    x: 491 - rx,
                    y: 447 - ry,
                    width: bmp.width,
                    height: bmp.height,
                    pixels: bmp.pixels,
                });
            } else {
                tracing::warn!("Dashboard: missing fuel needle frame #{}", mem);
                // Push a 1×1 transparent stub so frame indexing stays valid
                fuel_frames.push(FrameData {
                    x: 0, y: 0, width: 1, height: 1,
                    pixels: vec![0, 0, 0, 0],
                });
            }
        }

        // Load speedometer
        let speedo = if let Some(bmp) = assets.decode_bitmap_transparent(file, SPEEDO_MEMBER) {
            // Speedometer anchor: (99, 446) per mulle.js
            let (rx, ry) = reg_point(assets, file, SPEEDO_MEMBER);
            FrameData { x: 99 - rx, y: 446 - ry, width: bmp.width, height: bmp.height, pixels: bmp.pixels }
        } else {
            tracing::warn!("Dashboard: missing speedometer #{}", SPEEDO_MEMBER);
            FrameData { x: 0, y: 0, width: 1, height: 1, pixels: vec![0, 0, 0, 0] }
        };

        Some(Dashboard {
            fuel_frames,
            speedo,
        })
    }

    /// Produce dashboard sprites for the current driving state.
    ///
    /// - `fuel_pct` — fuel as fraction [0.0, 1.0]
    /// - `speed`    — current speed (absolute)
    /// - `max_speed`— car's maximum speed
    pub fn sprites(&self, fuel_pct: f32, speed: f32, max_speed: f32) -> Vec<Sprite> {
        let mut out = Vec::with_capacity(2);

        // ── Fuel needle ────────────────────────────────────────────────
        // frame_index = clamp(0, 15, round(fuel_pct * 16))
        // Index 0 = empty (member 27), index 15 = full (member 42)
        let fi = (fuel_pct * 16.0).round() as usize;
        let clamped = fi.min(15);
        let f = &self.fuel_frames[clamped];
        out.push(Sprite {
            x: f.x,
            y: f.y,
            width: f.width,
            height: f.height,
            pixels: f.pixels.clone(),
            visible: true,
            z_order: 51, // above dashboard bg (z=50)
            name: format!("fuel_needle_{}", clamped),
            interactive: false,
            member_num: FUEL_MEMBERS[clamped],
        });

        // ── Speedometer ────────────────────────────────────────────────
        // x = 100 + |140 * (speed / max_speed)|
        let speed_ratio = if max_speed > 0.0 {
            (speed.abs() / max_speed).min(1.0)
        } else {
            0.0
        };
        let speedo_x = SPEEDO_BASE_X + (SPEEDO_TRAVEL * speed_ratio);
        out.push(Sprite {
            x: speedo_x as i32,
            y: self.speedo.y,
            width: self.speedo.width,
            height: self.speedo.height,
            pixels: self.speedo.pixels.clone(),
            visible: true,
            z_order: 49, // below dashboard bg (z=50) so edges are naturally masked
            name: "speedometer".into(),
            interactive: false,
            member_num: SPEEDO_MEMBER,
        });

        out
    }
}

fn reg_point(assets: &AssetStore, file: &str, num: u32) -> (i32, i32) {
    assets.files.get(file)
        .and_then(|df| df.cast_members.get(&num))
        .and_then(|m| m.bitmap_info.as_ref())
        .map(|bi| (bi.reg_x as i32, bi.reg_y as i32))
        .unwrap_or((0, 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuel_frame_index_clamped() {
        // Verify the clamping logic matches mulle.js: round(pct * 16), clamped 0-15
        let cases: [(f32, usize); 5] = [
            (0.0, 0),   // empty
            (0.03, 0),  // 0.03 * 16 = 0.48 → round = 0
            (0.5, 8),   // 0.5 * 16 = 8
            (1.0, 15),  // 1.0 * 16 = 16 → clamped to 15
            (0.95, 15), // 0.95 * 16 = 15.2 → round = 15
        ];
        for (pct, expected) in &cases {
            let fi = (pct * 16.0).round() as usize;
            let clamped = fi.min(15);
            assert_eq!(clamped, *expected, "pct={}", pct);
        }
    }

    #[test]
    fn speedo_x_range() {
        // speed=0 → x=100, speed=max → x=240
        let cases: [(f32, f32, f32); 3] = [
            (0.0, 4.0, 100.0),
            (4.0, 4.0, 240.0),  // max speed
            (2.0, 4.0, 170.0),  // half speed
        ];
        for (speed, max_speed, expected_x) in &cases {
            let ratio = if *max_speed > 0.0 { (speed.abs() / max_speed).min(1.0) } else { 0.0 };
            let x = SPEEDO_BASE_X + SPEEDO_TRAVEL * ratio;
            assert!((x - expected_x).abs() < 0.01, "speed={} max={} → x={} (expected {})",
                speed, max_speed, x, expected_x);
        }
    }
}
