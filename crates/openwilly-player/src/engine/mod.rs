//! Game engine — minifb-based renderer, input, and game loop.
//!
//! Uses a 640×480 pixel framebuffer with 32-bit ARGB pixels.

pub mod font;
pub mod icon;
pub mod sound_engine;

use anyhow::Result;
use minifb::{Key, MouseButton, MouseMode, Window, WindowOptions};

use crate::assets::AssetStore;
use crate::game::GameState;

pub const SCREEN_WIDTH: usize = 640;
pub const SCREEN_HEIGHT: usize = 480;
const FPS: u64 = 30;

/// Engine display state
#[derive(Clone, Copy)]
enum EngineState {
    Playing,
    EscapeMenu { selected: usize },
}

const ESCAPE_MENU_COUNT: usize = 5; // resume, fullscreen, display mode, detail noise, quit

/// Display scaling mode
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DisplayMode {
    /// Fill entire window, may distort aspect ratio on non-4:3 displays
    Stretch,
    /// Maintain 4:3 aspect ratio with black bars (pillarbox/letterbox)
    Pillarbox,
    /// Integer scaling only (1×, 2×, 3×…) centered with black padding
    PixelPerfect,
}

impl DisplayMode {
    /// Cycle to the next display mode
    pub fn next(self) -> Self {
        match self {
            DisplayMode::Stretch => DisplayMode::Pillarbox,
            DisplayMode::Pillarbox => DisplayMode::PixelPerfect,
            DisplayMode::PixelPerfect => DisplayMode::Stretch,
        }
    }

    /// Short label for the current mode (used in escape menu)
    pub fn label(self) -> &'static str {
        match self {
            DisplayMode::Stretch => "Stretch",
            DisplayMode::Pillarbox => "Pillarbox",
            DisplayMode::PixelPerfect => "Pixel",
        }
    }
}

/// Sprite rendered by the engine
#[derive(Clone, Debug)]
pub struct Sprite {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>, // RGBA, 4 bytes per pixel
    pub visible: bool,
    pub z_order: i32,
    /// Name for debugging / hit detection logging
    pub name: String,
    /// If true, this sprite responds to clicks
    pub interactive: bool,
    /// Member number (for identification)
    #[allow(dead_code)]
    pub member_num: u32,
}

impl Sprite {
    /// Check if a point (px, py) falls within this sprite's bounding box
    /// AND hits a non-transparent pixel. Non-interactive sprites are skipped.
    pub fn hit_test(&self, px: i32, py: i32) -> bool {
        if !self.visible || !self.interactive {
            return false;
        }
        let lx = px - self.x;
        let ly = py - self.y;
        if lx < 0 || ly < 0 || lx >= self.width as i32 || ly >= self.height as i32 {
            return false;
        }
        // Check alpha at that pixel
        let idx = (ly as usize * self.width as usize + lx as usize) * 4;
        if idx + 3 < self.pixels.len() {
            self.pixels[idx + 3] > 0 // Non-transparent
        } else {
            false
        }
    }

    /// Check if a point is within the bounding box (ignoring alpha)
    pub fn bbox_hit(&self, px: i32, py: i32) -> bool {
        if !self.visible {
            return false;
        }
        let lx = px - self.x;
        let ly = py - self.y;
        lx >= 0 && ly >= 0 && lx < self.width as i32 && ly < self.height as i32
    }
}

/// Compute the viewport rectangle (offset + size) for a given display mode.
/// Returns `(x_offset, y_offset, viewport_width, viewport_height)`.
fn compute_viewport(out_w: usize, out_h: usize, mode: DisplayMode) -> (usize, usize, usize, usize) {
    match mode {
        DisplayMode::Stretch => (0, 0, out_w, out_h),
        DisplayMode::Pillarbox => {
            let scale_x = out_w as f64 / SCREEN_WIDTH as f64;
            let scale_y = out_h as f64 / SCREEN_HEIGHT as f64;
            let scale = scale_x.min(scale_y);
            let vw = (SCREEN_WIDTH as f64 * scale) as usize;
            let vh = (SCREEN_HEIGHT as f64 * scale) as usize;
            let ox = (out_w.saturating_sub(vw)) / 2;
            let oy = (out_h.saturating_sub(vh)) / 2;
            (ox, oy, vw, vh)
        }
        DisplayMode::PixelPerfect => {
            let factor_x = out_w / SCREEN_WIDTH;
            let factor_y = out_h / SCREEN_HEIGHT;
            let factor = factor_x.min(factor_y).max(1);
            let vw = SCREEN_WIDTH * factor;
            let vh = SCREEN_HEIGHT * factor;
            let ox = (out_w.saturating_sub(vw)) / 2;
            let oy = (out_h.saturating_sub(vh)) / 2;
            (ox, oy, vw, vh)
        }
    }
}

/// Pre-generated noise map for the detail-noise upscaling effect.
///
/// Generated once per viewport size / scene change.  Each entry is either
/// `0` (pixel is an original nearest-neighbor sample → pass through unchanged)
/// or a non-zero `i8` brightness offset in the range `[-13, +13]` (~5%)
/// that is applied to duplicated (non-original) pixels during scaling.
struct NoiseMap {
    /// Noise values, row-major, `vw × vh` entries
    data: Vec<i8>,
    /// Viewport dimensions this map was generated for
    vw: usize,
    vh: usize,
}

impl NoiseMap {
    /// Generate a new noise map for the given viewport size.
    fn generate(vw: usize, vh: usize) -> Self {
        let len = vw * vh;
        let mut data = vec![0i8; len];
        // Seed with a fixed-but-varied value so the pattern looks good
        let mut rng: u32 = 0xDEAD_BEEF;

        for dy in 0..vh {
            let _sy = (dy * SCREEN_HEIGHT) / vh;
            let mut prev_sx: usize = usize::MAX;

            for dx in 0..vw {
                let sx = (dx * SCREEN_WIDTH) / vw;
                if sx != prev_sx {
                    // Original sample — no noise
                    // data[dy * vw + dx] already 0
                } else {
                    // Duplicated neighbor — assign ±13 brightness offset (~5%)
                    rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
                    let noise = ((rng >> 16) % 27) as i8 - 13;
                    data[dy * vw + dx] = noise;
                }
                prev_sx = sx;
            }
        }

        Self { data, vw, vh }
    }

    /// Check whether this map matches the current viewport dimensions.
    fn matches(&self, vw: usize, vh: usize) -> bool {
        self.vw == vw && self.vh == vh
    }
}

/// Scale the 640×480 framebuffer into a viewport sub-region of the output
/// buffer.  The rest of the output is filled with black.  When `noise_map`
/// is provided, duplicated neighbor pixels receive the pre-generated
/// brightness perturbation for a film-grain-like sharpening effect.
/// The `ui_mask` (640×480, matching `src`) marks pixels drawn by the UI;
/// noise is NOT applied to those pixels so buttons, menus and overlays
/// stay crisp.
fn scale_to_viewport(
    src: &[u32],
    dst: &mut [u32],
    dst_w: usize,
    _dst_h: usize,
    vx: usize,
    vy: usize,
    vw: usize,
    vh: usize,
    noise_map: Option<&NoiseMap>,
    ui_mask: &[bool],
) {
    // Clear entire output to black
    dst.iter_mut().for_each(|p| *p = 0xFF000000);

    for dy in 0..vh {
        let sy = (dy * SCREEN_HEIGHT) / vh;
        let dst_row = (vy + dy) * dst_w;
        let src_row = sy * SCREEN_WIDTH;
        let noise_row = dy * vw;

        for dx in 0..vw {
            let sx = (dx * SCREEN_WIDTH) / vw;
            let pixel = src[src_row + sx];
            let dst_idx = dst_row + vx + dx;

            // Skip noise for UI pixels (buttons, menus, overlays)
            let is_ui = ui_mask[src_row + sx];
            let noise_val = if is_ui {
                0
            } else {
                noise_map
                    .map(|nm| nm.data[noise_row + dx] as i32)
                    .unwrap_or(0)
            };

            if noise_val == 0 {
                dst[dst_idx] = pixel;
            } else {
                let r = ((pixel >> 16) & 0xFF) as i32;
                let g = ((pixel >> 8) & 0xFF) as i32;
                let b = (pixel & 0xFF) as i32;

                let r2 = (r + noise_val).clamp(0, 255) as u32;
                let g2 = (g + noise_val).clamp(0, 255) as u32;
                let b2 = (b + noise_val).clamp(0, 255) as u32;

                dst[dst_idx] = 0xFF000000 | (r2 << 16) | (g2 << 8) | b2;
            }
        }
    }
}

/// Draw semi-transparent escape/pause menu overlay onto the 640x480 framebuffer
fn draw_escape_menu(
    fb: &mut [u32],
    selected: usize,
    detail_noise: bool,
    display_mode: DisplayMode,
    lang: crate::game::i18n::Language,
) {
    // Darken the entire framebuffer
    for pixel in fb.iter_mut() {
        let r = (*pixel >> 16) & 0xFF;
        let g = (*pixel >> 8) & 0xFF;
        let b = *pixel & 0xFF;
        *pixel = 0xFF000000 | ((r / 3) << 16) | ((g / 3) << 8) | (b / 3);
    }

    let box_w: i32 = 300;
    let box_h: i32 = 210;
    let box_x = (SCREEN_WIDTH as i32 - box_w) / 2;
    let box_y = (SCREEN_HEIGHT as i32 - box_h) / 2;

    font::draw_rect(fb, box_x, box_y, box_w, box_h, 0xFF1a1a2e);
    font::draw_rect_outline(fb, box_x, box_y, box_w, box_h, 0xFF6666CC);
    font::draw_rect_outline(fb, box_x + 2, box_y + 2, box_w - 4, box_h - 4, 0xFF444488);

    use crate::game::i18n::t;
    let title = t(lang, "pause_title");
    font::draw_text_shadow(fb,
        box_x + (box_w - font::text_width(title)) / 2,
        box_y + 14, title, 0xFFFFFF00);

    let item_keys = [
        "menu_resume",
        "menu_fullscreen",
        "menu_display_mode",
        "menu_detail_noise",
        "menu_quit",
    ];
    for (i, key) in item_keys.iter().enumerate() {
        let label = t(lang, key);
        let iy = box_y + 46 + i as i32 * 26;
        let color = if i == selected { 0xFFFFFF00 } else { 0xFFBBBBBB };
        if i == selected {
            font::draw_rect(fb, box_x + 6, iy - 2, box_w - 12, 20, 0xFF333366);
        }
        let prefix = if i == selected { "> " } else { "  " };
        let mode_label = format!(" [{}]", display_mode.label());
        let suffix: &str = match i {
            2 => &mode_label,
            3 => if detail_noise { " [ON]" } else { " [OFF]" },
            _ => "",
        };
        let text = format!("{}{}{}", prefix, label, suffix);
        font::draw_text_shadow(fb, box_x + 20, iy + 2, &text, color);
    }

    font::draw_text(fb, box_x + 14, box_y + box_h - 22,
        t(lang, "pause_hint"), 0xFF777799);
}

/// Run the game engine
pub fn run(assets: AssetStore) -> Result<()> {
    let mut game = GameState::new(assets);
    let mut fullscreen = false;
    let mut engine_state = EngineState::Playing;
    let mut prev_mouse_down = false;
    let mut prev_right_down = false;
    let mut frame_count: u64 = 0;

    tracing::info!("Engine initialized, entering game loop");
    tracing::info!("Controls: F1-F9=Szene | Esc=Menü | F11=Vollbild");

    // Outer loop: window (re)creation on fullscreen toggle
    loop {
        let (win_w, win_h) = if fullscreen {
            // Query primary monitor resolution for true borderless fullscreen
            #[cfg(target_os = "windows")]
            {
                extern "system" {
                    fn GetSystemMetrics(nIndex: i32) -> i32;
                }
                let w = unsafe { GetSystemMetrics(0) } as usize; // SM_CXSCREEN
                let h = unsafe { GetSystemMetrics(1) } as usize; // SM_CYSCREEN
                if w > 0 && h > 0 { (w, h) } else { (1920, 1080) }
            }
            #[cfg(not(target_os = "windows"))]
            {
                (1920usize, 1080usize)
            }
        } else {
            (SCREEN_WIDTH * 2, SCREEN_HEIGHT * 2)
        };

        let options = WindowOptions {
            resize: false,
            borderless: fullscreen,
            topmost: fullscreen,
            scale_mode: minifb::ScaleMode::AspectRatioStretch,
            ..Default::default()
        };

        let mut window = Window::new("OpenWilly – Willy Werkel", win_w, win_h, options)
            .map_err(|e| anyhow::anyhow!("Window creation failed: {}", e))?;
        window.set_target_fps(FPS as usize);
        window.set_cursor_visibility(false); // Software cursor rendered on framebuffer

        // Set window icon from game data (WILLY32.EXE icon or MULLE.ICO)
        icon::set_window_icon(&mut window, &game.assets.game_dir);

        // Internal framebuffer at native resolution
        let mut framebuffer = vec![0u32; SCREEN_WIDTH * SCREEN_HEIGHT];

        // Output buffer — sized to match window
        let mut out_w = win_w;
        let mut out_h = win_h;
        let mut scaled_buf = vec![0u32; out_w * out_h];
        let mut toggle_fs = false;

        // Pre-generate noise map for the initial viewport
        let init_mode = game.dev_menu.display_mode;
        let (_, _, init_vw, init_vh) = compute_viewport(out_w, out_h, init_mode);
        let mut noise_map = NoiseMap::generate(init_vw, init_vh);
        let mut prev_scene = game.current_scene;

        // Inner loop: game frames
        while window.is_open() {
            // Track window size changes (for resizable windowed mode)
            let (actual_w, actual_h) = window.get_size();
            if actual_w > 0 && actual_h > 0 && (actual_w != out_w || actual_h != out_h) {
                out_w = actual_w;
                out_h = actual_h;
                scaled_buf.resize(out_w * out_h, 0);
            }

            // Compute viewport for current display mode
            let display_mode = game.dev_menu.display_mode;
            let (vx, vy, vw, vh) = compute_viewport(out_w, out_h, display_mode);

            // Mouse → logical 640×480 (accounting for viewport offset)
            let (mouse_x, mouse_y) = window
                .get_mouse_pos(MouseMode::Clamp)
                .unwrap_or((0.0, 0.0));
            let raw_mx = mouse_x as usize;
            let raw_my = mouse_y as usize;
            let mx = if raw_mx >= vx && raw_mx < vx + vw {
                (((raw_mx - vx) * SCREEN_WIDTH) / vw.max(1)) as i32
            } else {
                if raw_mx < vx { 0 } else { SCREEN_WIDTH as i32 - 1 }
            };
            let my = if raw_my >= vy && raw_my < vy + vh {
                (((raw_my - vy) * SCREEN_HEIGHT) / vh.max(1)) as i32
            } else {
                if raw_my < vy { 0 } else { SCREEN_HEIGHT as i32 - 1 }
            };
            let mx = mx.clamp(0, SCREEN_WIDTH as i32 - 1);
            let my = my.clamp(0, SCREEN_HEIGHT as i32 - 1);

            // F11 → fullscreen toggle (in any state)
            if window.is_key_pressed(Key::F11, minifb::KeyRepeat::No) {
                toggle_fs = true;
                break;
            }

            // Input state
            let esc_pressed = window.is_key_pressed(Key::Escape, minifb::KeyRepeat::No);
            let mouse_down = window.get_mouse_down(MouseButton::Left);
            let mouse_clicked = mouse_down && !prev_mouse_down;

            match engine_state {
                EngineState::Playing => {
                    if esc_pressed {
                        engine_state = EngineState::EscapeMenu { selected: 0 };
                    } else {
                        // Unified mouse state handling (includes drag & drop)
                        game.on_mouse_state(mx, my, mouse_down);

                        if mouse_clicked {
                            game.on_click(mx, my);
                        }

                        let right_down = window.get_mouse_down(MouseButton::Right);
                        let right_clicked = right_down && !prev_right_down;
                        if right_clicked {
                            game.on_right_click(mx, my);
                        }

                        let keys = window.get_keys_pressed(minifb::KeyRepeat::No);
                        for key in keys {
                            if let Some(ch) = key_to_char(
                                key,
                                window.is_key_down(Key::LeftShift)
                                    || window.is_key_down(Key::RightShift),
                            ) {
                                game.on_char_input(ch);
                            }
                            game.on_key_down(key);
                        }

                        // Poll driving keys (continuous, not event-based)
                        game.update_drive_keys(
                            window.is_key_down(Key::Up),
                            window.is_key_down(Key::Down),
                            window.is_key_down(Key::Left),
                            window.is_key_down(Key::Right),
                        );

                        game.update();
                    }
                }
                EngineState::EscapeMenu { selected } => {
                    if esc_pressed {
                        engine_state = EngineState::Playing;
                    } else {
                        let mut sel = selected;

                        // Keyboard navigation
                        if window.is_key_pressed(Key::Up, minifb::KeyRepeat::Yes) && sel > 0 {
                            sel -= 1;
                        }
                        if window.is_key_pressed(Key::Down, minifb::KeyRepeat::Yes)
                            && sel < ESCAPE_MENU_COUNT - 1
                        {
                            sel += 1;
                        }

                        // Mouse hover over menu items
                        let box_x = (SCREEN_WIDTH as i32 - 300) / 2;
                        let box_y = (SCREEN_HEIGHT as i32 - 210) / 2;
                        if mx >= box_x + 6 && mx < box_x + 294 {
                            let rel_y = my - (box_y + 44);
                            if rel_y >= 0 {
                                let idx = (rel_y / 26) as usize;
                                if idx < ESCAPE_MENU_COUNT {
                                    sel = idx;
                                }
                            }
                        }

                        engine_state = EngineState::EscapeMenu { selected: sel };

                        // Activate via Enter or mouse click
                        let mut action: Option<usize> = None;
                        if window.is_key_pressed(Key::Enter, minifb::KeyRepeat::No) {
                            action = Some(sel);
                        }
                        if mouse_clicked && mx >= box_x + 6 && mx < box_x + 294 {
                            let rel_y = my - (box_y + 44);
                            if rel_y >= 0 {
                                let idx = (rel_y / 26) as usize;
                                if idx < ESCAPE_MENU_COUNT {
                                    action = Some(idx);
                                }
                            }
                        }

                        if let Some(act) = action {
                            match act {
                                0 => engine_state = EngineState::Playing,
                                1 => toggle_fs = true,
                                2 => {
                                    // Cycle display mode
                                    game.dev_menu.display_mode = game.dev_menu.display_mode.next();
                                    tracing::info!("Display mode → {:?}", game.dev_menu.display_mode);
                                }
                                3 => {
                                    // Toggle detail noise
                                    game.dev_menu.detail_noise = !game.dev_menu.detail_noise;
                                    tracing::info!("Detail noise → {}", game.dev_menu.detail_noise);
                                }
                                4 => {
                                    tracing::info!("Engine shutdown (menu)");
                                    return Ok(());
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }

            if toggle_fs {
                break;
            }
            prev_mouse_down = mouse_down;
            prev_right_down = window.get_mouse_down(MouseButton::Right);

            // Render
            framebuffer.fill(0xFF000000);

            let sprites = game.get_all_sprites();
            for sprite in &sprites {
                if !sprite.visible || sprite.width == 0 || sprite.height == 0 {
                    continue;
                }
                blit_sprite(&mut framebuffer, sprite);
            }

            // Debug: draw bounding boxes when enabled via dev menu
            if game.dev_menu.show_hitboxes {
                for sprite in &sprites {
                    if !sprite.visible || sprite.width == 0 || sprite.height == 0 {
                        continue;
                    }
                    let color = if sprite.interactive { 0xFF00FF00 } else { 0xFF888888 }; // green for interactive, gray for passive
                    font::draw_rect_outline(
                        &mut framebuffer,
                        sprite.x, sprite.y,
                        sprite.width as i32, sprite.height as i32,
                        color,
                    );
                    // Label: name + position
                    let label = format!("{} ({},{}) z{}", sprite.name, sprite.x, sprite.y, sprite.z_order);
                    // Draw label background for readability
                    let tw = font::text_width(&label);
                    let lx = sprite.x.max(0);
                    let ly = (sprite.y - 12).max(0);
                    font::draw_rect(&mut framebuffer, lx, ly, tw + 2, 11, 0xCC000000);
                    font::draw_text(&mut framebuffer, lx + 1, ly + 1, &label, color);
                }
            }

            let hover_name = game.get_hover_info(mx, my);

            // Snapshot the scene-only framebuffer before UI overlays
            let scene_snap: Vec<u32> = framebuffer.clone();

            game.draw_ui(&mut framebuffer);

            // Debug: draw UI element hitboxes (after draw_ui so they appear on top)
            if game.dev_menu.show_hitboxes {
                let ui_color = 0xFF00FFFF; // cyan to distinguish from sprite hitboxes
                for (rx, ry, rw, rh, label) in game.scene_handler.get_ui_rects() {
                    font::draw_rect_outline(&mut framebuffer, rx, ry, rw, rh, ui_color);
                    let tag = format!("{} ({},{} {}x{})", label, rx, ry, rw, rh);
                    let tw = font::text_width(&tag);
                    let lx = rx.max(0);
                    let ly = (ry - 12).max(0);
                    font::draw_rect(&mut framebuffer, lx, ly, tw + 2, 11, 0xCC000000);
                    font::draw_text(&mut framebuffer, lx + 1, ly + 1, &tag, ui_color);
                }
            }

            // Draw escape menu overlay if paused
            if let EngineState::EscapeMenu { selected } = engine_state {
                draw_escape_menu(&mut framebuffer, selected, game.dev_menu.detail_noise,
                                 game.dev_menu.display_mode, game.language);
            }

            // Software cursor (drawn last, always on top)
            game.cursor.blit(&mut framebuffer, SCREEN_WIDTH, SCREEN_HEIGHT, mx, my);

            // Build UI mask: true where UI changed a pixel vs the scene snapshot
            let ui_mask: Vec<bool> = framebuffer.iter()
                .zip(scene_snap.iter())
                .map(|(cur, snap)| cur != snap)
                .collect();

            // Update window title
            frame_count += 1;
            if frame_count % 5 == 0 {
                let fs_label = if fullscreen { "FS" } else { "Win" };
                let title = format!(
                    "OpenWilly – {:?} | {} {}×{} | ({},{}) | {}",
                    game.current_scene,
                    fs_label,
                    out_w,
                    out_h,
                    mx,
                    my,
                    if hover_name.is_empty() { "-" } else { &hover_name },
                );
                window.set_title(&title);
            }

            // Regenerate noise map when viewport or scene changes
            if !noise_map.matches(vw, vh) || game.current_scene != prev_scene {
                noise_map = NoiseMap::generate(vw, vh);
                prev_scene = game.current_scene;
            }

            // Scale to output size and present
            let nm = if game.dev_menu.detail_noise { Some(&noise_map) } else { None };
            scale_to_viewport(&framebuffer, &mut scaled_buf, out_w, out_h,
                              vx, vy, vw, vh, nm, &ui_mask);
            window
                .update_with_buffer(&scaled_buf, out_w, out_h)
                .map_err(|e| anyhow::anyhow!("Display error: {}", e))?;
        }

        if toggle_fs {
            fullscreen = !fullscreen;
            tracing::info!(
                "Fullscreen → {}",
                if fullscreen { "ON (borderless)" } else { "OFF (1280×960)" }
            );
            engine_state = EngineState::Playing;
            continue;
        }

        break; // Window was closed
    }

    tracing::info!("Engine shutdown");
    Ok(())
}

/// Blit an RGBA sprite onto the u32 ARGB framebuffer with alpha blending
fn blit_sprite(fb: &mut [u32], sprite: &Sprite) {
    let sw = sprite.width as i32;
    let sh = sprite.height as i32;

    for sy in 0..sh {
        let dy = sprite.y + sy;
        if dy < 0 || dy >= SCREEN_HEIGHT as i32 {
            continue;
        }
        for sx in 0..sw {
            let dx = sprite.x + sx;
            if dx < 0 || dx >= SCREEN_WIDTH as i32 {
                continue;
            }

            let src_idx = (sy * sw + sx) as usize * 4;
            if src_idx + 3 >= sprite.pixels.len() {
                continue;
            }

            let r = sprite.pixels[src_idx] as u32;
            let g = sprite.pixels[src_idx + 1] as u32;
            let b = sprite.pixels[src_idx + 2] as u32;
            let a = sprite.pixels[src_idx + 3] as u32;

            if a == 0 {
                continue; // Fully transparent
            }

            let dst_idx = (dy as usize) * SCREEN_WIDTH + dx as usize;

            if a >= 255 {
                // Fully opaque — no blending needed
                fb[dst_idx] = 0xFF000000 | (r << 16) | (g << 8) | b;
            } else {
                // Alpha blend
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

/// Convert minifb Key to ASCII char for text input
fn key_to_char(key: Key, shift: bool) -> Option<char> {
    let ch = match key {
        Key::A => 'a', Key::B => 'b', Key::C => 'c', Key::D => 'd',
        Key::E => 'e', Key::F => 'f', Key::G => 'g', Key::H => 'h',
        Key::I => 'i', Key::J => 'j', Key::K => 'k', Key::L => 'l',
        Key::M => 'm', Key::N => 'n', Key::O => 'o', Key::P => 'p',
        Key::Q => 'q', Key::R => 'r', Key::S => 's', Key::T => 't',
        Key::U => 'u', Key::V => 'v', Key::W => 'w', Key::X => 'x',
        Key::Y => 'y', Key::Z => 'z',
        Key::Key0 | Key::NumPad0 => '0',
        Key::Key1 | Key::NumPad1 => '1',
        Key::Key2 | Key::NumPad2 => '2',
        Key::Key3 | Key::NumPad3 => '3',
        Key::Key4 | Key::NumPad4 => '4',
        Key::Key5 | Key::NumPad5 => '5',
        Key::Key6 | Key::NumPad6 => '6',
        Key::Key7 | Key::NumPad7 => '7',
        Key::Key8 | Key::NumPad8 => '8',
        Key::Key9 | Key::NumPad9 => '9',
        Key::Space => ' ',
        Key::Period => '.',
        Key::Minus => '-',
        Key::Backslash => '#', // German keyboard: # key is at US backslash position
        _ => return None,
    };
    if shift && ch.is_ascii_lowercase() {
        Some(ch.to_ascii_uppercase())
    } else {
        Some(ch)
    }
}