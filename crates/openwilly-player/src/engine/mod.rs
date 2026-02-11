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

const ESCAPE_MENU_COUNT: usize = 4; // resume, fullscreen, detail noise, quit

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

/// Scale the 640x480 framebuffer to any target size using per-axis nearest-
/// neighbor sampling. Horizontal and vertical scale factors are independent,
/// enabling PAR adjustment (e.g. 640x480 -> 1920x1080: 3.0x H, 2.25x V).
///
/// When `detail_noise` is true, pixels that were NOT directly sampled from
/// the source (i.e. duplicated neighbors) receive a subtle random brightness
/// perturbation of +/-0.2 (mapped to +/-51 on the 0-255 channel range).
/// This increases perceived sharpness and adds natural-looking micro-detail
/// at higher resolutions without affecting the original source pixels.
fn scale_to_size(src: &[u32], dst: &mut [u32], dst_w: usize, dst_h: usize, detail_noise: bool, frame: u64) {
    // Simple LCG-based fast noise — NOT crypto-quality, just visual dither
    let mut rng_state: u32 = (frame as u32).wrapping_mul(2654435761);

    for dy in 0..dst_h {
        let sy = (dy * SCREEN_HEIGHT) / dst_h;
        let dst_row = dy * dst_w;
        let src_row = sy * SCREEN_WIDTH;

        // Track which source column the previous dst pixel came from,
        // so we can detect "duplicate" (non-original) samples.
        let mut prev_sx: usize = usize::MAX;

        for dx in 0..dst_w {
            let sx = (dx * SCREEN_WIDTH) / dst_w;
            let pixel = src[src_row + sx];

            if !detail_noise || sx != prev_sx {
                // Original sample — output unchanged
                dst[dst_row + dx] = pixel;
            } else {
                // Duplicated neighbor — apply brightness noise +/-0.2
                // Advance the LCG
                rng_state = rng_state.wrapping_mul(1103515245).wrapping_add(12345);
                // Map to range [-51, +51]  (0.2 * 255 ~ 51)
                let noise = ((rng_state >> 16) % 103) as i32 - 51;

                let r = ((pixel >> 16) & 0xFF) as i32;
                let g = ((pixel >> 8) & 0xFF) as i32;
                let b = (pixel & 0xFF) as i32;

                let r2 = (r + noise).clamp(0, 255) as u32;
                let g2 = (g + noise).clamp(0, 255) as u32;
                let b2 = (b + noise).clamp(0, 255) as u32;

                dst[dst_row + dx] = 0xFF000000 | (r2 << 16) | (g2 << 8) | b2;
            }
            prev_sx = sx;
        }
    }
}

/// Draw semi-transparent escape/pause menu overlay onto the 640x480 framebuffer
fn draw_escape_menu(fb: &mut [u32], selected: usize, detail_noise: bool, lang: crate::game::i18n::Language) {
    // Darken the entire framebuffer
    for pixel in fb.iter_mut() {
        let r = (*pixel >> 16) & 0xFF;
        let g = (*pixel >> 8) & 0xFF;
        let b = *pixel & 0xFF;
        *pixel = 0xFF000000 | ((r / 3) << 16) | ((g / 3) << 8) | (b / 3);
    }

    let box_w: i32 = 280;
    let box_h: i32 = 180;
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

    let item_keys = ["menu_resume", "menu_fullscreen", "menu_detail_noise", "menu_quit"];
    for (i, key) in item_keys.iter().enumerate() {
        let label = t(lang, key);
        let iy = box_y + 46 + i as i32 * 26;
        let color = if i == selected { 0xFFFFFF00 } else { 0xFFBBBBBB };
        if i == selected {
            font::draw_rect(fb, box_x + 6, iy - 2, box_w - 12, 20, 0xFF333366);
        }
        let prefix = if i == selected { "> " } else { "  " };
        // Show toggle state for Detail-Rauschen (index 2)
        let suffix = if i == 2 {
            if detail_noise { " [ON]" } else { " [OFF]" }
        } else {
            ""
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
    let mut frame_count: u64 = 0;

    tracing::info!("Engine initialized, entering game loop");
    tracing::info!("Controls: F1-F9=Szene | Esc=Menü | F11=Vollbild");

    // Outer loop: window (re)creation on fullscreen toggle
    loop {
        let (win_w, win_h) = if fullscreen {
            (1920usize, 1080usize)
        } else {
            (SCREEN_WIDTH * 2, SCREEN_HEIGHT * 2)
        };

        let options = WindowOptions {
            resize: !fullscreen,
            borderless: fullscreen,
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

        // Inner loop: game frames
        while window.is_open() {
            // Track window size changes (for resizable windowed mode)
            let (actual_w, actual_h) = window.get_size();
            if actual_w > 0 && actual_h > 0 && (actual_w != out_w || actual_h != out_h) {
                out_w = actual_w;
                out_h = actual_h;
                scaled_buf.resize(out_w * out_h, 0);
            }

            // Mouse → logical 640×480
            let (mouse_x, mouse_y) = window
                .get_mouse_pos(MouseMode::Clamp)
                .unwrap_or((0.0, 0.0));
            let mx = ((mouse_x as usize) * SCREEN_WIDTH / out_w.max(1)) as i32;
            let my = ((mouse_y as usize) * SCREEN_HEIGHT / out_h.max(1)) as i32;
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

                        if window.get_mouse_down(MouseButton::Right) {
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
                        let box_x = (SCREEN_WIDTH as i32 - 280) / 2;
                        let box_y = (SCREEN_HEIGHT as i32 - 180) / 2;
                        if mx >= box_x + 6 && mx < box_x + 274 {
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
                        if mouse_clicked && mx >= box_x + 6 && mx < box_x + 274 {
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
                                    // Toggle detail noise
                                    game.dev_menu.detail_noise = !game.dev_menu.detail_noise;
                                    tracing::info!("Detail noise → {}", game.dev_menu.detail_noise);
                                }
                                3 => {
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

            // Render
            framebuffer.fill(0xFF000000);

            let sprites = game.get_all_sprites();
            for sprite in &sprites {
                if !sprite.visible || sprite.width == 0 || sprite.height == 0 {
                    continue;
                }
                blit_sprite(&mut framebuffer, sprite);
            }

            let hover_name = game.get_hover_info(mx, my);
            game.draw_ui(&mut framebuffer);

            // Draw escape menu overlay if paused
            if let EngineState::EscapeMenu { selected } = engine_state {
                draw_escape_menu(&mut framebuffer, selected, game.dev_menu.detail_noise, game.language);
            }

            // Software cursor (drawn last, always on top)
            game.cursor.blit(&mut framebuffer, SCREEN_WIDTH, SCREEN_HEIGHT, mx, my);

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

            // Scale to output size and present
            scale_to_size(&framebuffer, &mut scaled_buf, out_w, out_h,
                          game.dev_menu.detail_noise, frame_count);
            window
                .update_with_buffer(&scaled_buf, out_w, out_h)
                .map_err(|e| anyhow::anyhow!("Display error: {}", e))?;
        }

        if toggle_fs {
            fullscreen = !fullscreen;
            tracing::info!(
                "Fullscreen → {}",
                if fullscreen { "ON (1920×1080)" } else { "OFF (1280×960)" }
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