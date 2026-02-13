//! Bitmap decoder for Director BITD chunks
//!
//! Supports:
//! - 1-bit bitmaps (bitfield)
//! - 8-bit uncompressed (palette-indexed, 0xFF-inverted)
//! - 8-bit PackBits RLE (palette-indexed, 0xFF-inverted)
//! - 16-bit uncompressed/RLE
//! - 32-bit planar RLE (A,R,G,B channels, no inversion)

/// Decoded bitmap in RGBA format, ready for rendering
#[derive(Debug, Clone)]
pub struct DecodedBitmap {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>, // RGBA, 4 bytes per pixel
}

/// Decode a Director BITD chunk into an RGBA bitmap
///
/// `transparent_color`: which palette index should be fully transparent.
///   - `Some(255)` for overlay sprites (Director default: white = transparent)
///   - `None` for backgrounds (no transparency)
pub fn decode_bitd(
    data: &[u8],
    width: u16,
    height: u16,
    bit_depth: u8,
    palette: &[[u8; 3]],
    transparent_color: Option<u8>,
) -> DecodedBitmap {
    let w = width as u32;
    let h = height as u32;
    let mut pixels = vec![0u8; (w * h * 4) as usize];

    match bit_depth {
        1 => decode_1bit(data, w, h, &mut pixels),
        8 => decode_8bit(data, w, h, palette, transparent_color, &mut pixels),
        16 => decode_16bit(data, w, h, &mut pixels),
        32 => decode_32bit(data, w, h, &mut pixels),
        _ => {
            tracing::warn!("Unsupported bit depth: {}", bit_depth);
        }
    }

    DecodedBitmap {
        width: w,
        height: h,
        pixels,
    }
}

// ============================================================================
// 1-bit decode
// ============================================================================

fn decode_1bit(data: &[u8], w: u32, h: u32, pixels: &mut [u8]) {
    // Row stride: ceil(width / 16) * 2 bytes (16-bit aligned)
    let row_bytes = ((w + 15) / 16 * 2) as usize;

    for y in 0..h as usize {
        for x in 0..w as usize {
            let byte_idx = y * row_bytes + x / 8;
            let bit_idx = 7 - (x % 8);
            let bit = if byte_idx < data.len() {
                (data[byte_idx] >> bit_idx) & 1
            } else {
                0
            };
            // 1-bit: 0 = white, 1 = black (color inverted)
            let color = if bit == 0 { 255u8 } else { 0u8 };
            let px = (y as u32 * w + x as u32) as usize * 4;
            if px + 3 < pixels.len() {
                pixels[px] = color;
                pixels[px + 1] = color;
                pixels[px + 2] = color;
                pixels[px + 3] = 255;
            }
        }
    }
}

// ============================================================================
// 8-bit decode (uncompressed + PackBits RLE)
// ============================================================================

fn decode_8bit(data: &[u8], w: u32, h: u32, palette: &[[u8; 3]], transparent_color: Option<u8>, pixels: &mut [u8]) {
    // Row stride: width padded to even
    let row_bytes = if w % 2 == 0 { w } else { w + 1 } as usize;
    let expected_uncompressed = row_bytes * h as usize;

    if data.len() >= expected_uncompressed {
        // Uncompressed
        decode_8bit_uncompressed(data, w, h, row_bytes, palette, transparent_color, pixels);
    } else {
        // PackBits RLE
        decode_8bit_rle(data, w, h, row_bytes, palette, transparent_color, pixels);
    }
}

fn decode_8bit_uncompressed(
    data: &[u8],
    w: u32,
    h: u32,
    row_bytes: usize,
    palette: &[[u8; 3]],
    transparent_color: Option<u8>,
    pixels: &mut [u8],
) {
    for y in 0..h as usize {
        for x in 0..w as usize {
            let idx = y * row_bytes + x;
            if idx < data.len() {
                let color_idx = 0xFF ^ data[idx]; // Director inverts with 0xFF XOR
                let px_offset = (y as u32 * w + x as u32) as usize * 4;
                if px_offset + 3 < pixels.len() && (color_idx as usize) < palette.len() {
                    let c = palette[color_idx as usize];
                    pixels[px_offset] = c[0];
                    pixels[px_offset + 1] = c[1];
                    pixels[px_offset + 2] = c[2];
                    // Transparent if matches the designated background color
                    pixels[px_offset + 3] = match transparent_color {
                        Some(tc) if color_idx == tc => 0,
                        _ => 255,
                    };
                }
            }
        }
    }
}

fn decode_8bit_rle(
    data: &[u8],
    w: u32,
    h: u32,
    row_bytes: usize,
    palette: &[[u8; 3]],
    transparent_color: Option<u8>,
    pixels: &mut [u8],
) {
    // PackBits RLE decompression
    // Decompress entire data into row-aligned buffer, then apply palette
    let mut decompressed = vec![0u8; row_bytes * h as usize];
    let mut src = 0usize;
    let mut dst = 0usize;

    while src < data.len() && dst < decompressed.len() {
        let run_len = data[src] as i8;
        src += 1;

        if run_len >= 0 {
            // Literal run: copy next (run_len + 1) bytes
            let count = run_len as usize + 1;
            for _ in 0..count {
                if src < data.len() && dst < decompressed.len() {
                    decompressed[dst] = data[src];
                    src += 1;
                    dst += 1;
                }
            }
        } else if run_len as u8 != 0x80 {
            // Repeat run: repeat next byte (1 - run_len) times
            // run_len is negative, so (1 - run_len) = (1 + |run_len|)
            let count = (1i16 - run_len as i16) as usize;
            let val = if src < data.len() {
                let v = data[src];
                src += 1;
                v
            } else {
                0
            };
            for _ in 0..count {
                if dst < decompressed.len() {
                    decompressed[dst] = val;
                    dst += 1;
                }
            }
        }
        // 0x80 = no-op
    }

    // Apply palette from decompressed buffer
    decode_8bit_uncompressed(&decompressed, w, h, row_bytes, palette, transparent_color, pixels);
}

// ============================================================================
// 16-bit decode
// ============================================================================

fn decode_16bit(data: &[u8], w: u32, h: u32, pixels: &mut [u8]) {
    let row_bytes = w as usize * 2;
    let expected_uncompressed = row_bytes * h as usize;

    let decompressed = if data.len() >= expected_uncompressed {
        data.to_vec()
    } else {
        packbits_decompress(data, row_bytes * h as usize)
    };

    for y in 0..h as usize {
        for x in 0..w as usize {
            let idx = (y * w as usize + x) * 2;
            if idx + 1 < decompressed.len() {
                let pixel16 = (decompressed[idx] as u16) << 8 | decompressed[idx + 1] as u16;
                let r = ((pixel16 >> 10) & 0x1F) as u8;
                let g = ((pixel16 >> 5) & 0x1F) as u8;
                let b = (pixel16 & 0x1F) as u8;
                let px_offset = (y as u32 * w + x as u32) as usize * 4;
                if px_offset + 3 < pixels.len() {
                    pixels[px_offset] = (r << 3) | (r >> 2);
                    pixels[px_offset + 1] = (g << 3) | (g >> 2);
                    pixels[px_offset + 2] = (b << 3) | (b >> 2);
                    pixels[px_offset + 3] = 255;
                }
            }
        }
    }
}

// ============================================================================
// 32-bit decode (planar ARGB)
// ============================================================================

fn decode_32bit(data: &[u8], w: u32, h: u32, pixels: &mut [u8]) {
    // 32-bit bitmaps use planar channel layout:
    // Each row is stored as: [A0, A1, ..., An, R0, R1, ..., Rn, G0, ..., Gn, B0, ..., Bn]
    // No color inversion for 32-bit

    let row_bytes = w as usize * 4; // 4 channels per row
    let total = row_bytes * h as usize;

    let decompressed = if data.len() >= total {
        data.to_vec()
    } else {
        packbits_decompress(data, total)
    };

    for y in 0..h as usize {
        let row_start = y * row_bytes;
        let channel_stride = w as usize;
        for x in 0..w as usize {
            let a_idx = row_start + x;
            let r_idx = row_start + channel_stride + x;
            let g_idx = row_start + channel_stride * 2 + x;
            let b_idx = row_start + channel_stride * 3 + x;

            let px_offset = (y as u32 * w + x as u32) as usize * 4;
            if px_offset + 3 < pixels.len() && b_idx < decompressed.len() {
                pixels[px_offset] = decompressed[r_idx];
                pixels[px_offset + 1] = decompressed[g_idx];
                pixels[px_offset + 2] = decompressed[b_idx];
                pixels[px_offset + 3] = decompressed[a_idx];
            }
        }
    }
}

// ============================================================================
// PackBits decompression (generic)
// ============================================================================

fn packbits_decompress(data: &[u8], expected_len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(expected_len);
    let mut src = 0usize;

    while src < data.len() && out.len() < expected_len {
        let run_len = data[src] as i8;
        src += 1;

        if run_len >= 0 {
            let count = run_len as usize + 1;
            for _ in 0..count {
                if src < data.len() && out.len() < expected_len {
                    out.push(data[src]);
                    src += 1;
                }
            }
        } else if run_len as u8 != 0x80 {
            let count = (1i16 - run_len as i16) as usize;
            let val = if src < data.len() {
                let v = data[src];
                src += 1;
                v
            } else {
                0
            };
            for _ in 0..count {
                if out.len() < expected_len {
                    out.push(val);
                }
            }
        }
    }

    out.resize(expected_len, 0);
    out
}
