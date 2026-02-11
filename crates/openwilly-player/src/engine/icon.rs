//! Extract the Willy Werkel icon from WILLY32.EXE at runtime
//!
//! Parses the PE resource section to find RT_GROUP_ICON / RT_ICON entries,
//! builds a standard .ico file, writes it to a temp location, and sets
//! the minifb window icon.

use std::path::{Path, PathBuf};

/// Try to set the window icon from game data.
///
/// Search order:
/// 1. `MULLE.ICO` in game dir (or `Data/MULLE.ICO`)
/// 2. Extract from `WILLY32.EXE` PE resources
///
/// Returns the path to the icon file (for logging), or None if not found.
pub fn set_window_icon(window: &mut minifb::Window, game_dir: &Path) -> Option<PathBuf> {
    // 1. Try existing .ico file
    for candidate in &["MULLE.ICO", "Data/MULLE.ICO", "mulle.ico", "data/mulle.ico"] {
        let ico_path = game_dir.join(candidate);
        if ico_path.is_file() {
            if try_set_icon(window, &ico_path) {
                return Some(ico_path);
            }
        }
    }

    // 2. Extract from WILLY32.EXE
    for exe_name in &["WILLY32.EXE", "willy32.exe", "Willy32.exe"] {
        let exe_path = game_dir.join(exe_name);
        if exe_path.is_file() {
            match extract_icon_from_pe(&exe_path) {
                Ok(ico_data) => {
                    // Write to temp file
                    let tmp = std::env::temp_dir().join("openwilly_icon.ico");
                    if std::fs::write(&tmp, &ico_data).is_ok() {
                        if try_set_icon(window, &tmp) {
                            tracing::info!("Icon extracted from {}", exe_path.display());
                            return Some(tmp);
                        }
                    }
                }
                Err(e) => {
                    tracing::debug!("Failed to extract icon from {}: {}", exe_path.display(), e);
                }
            }
        }
    }

    tracing::debug!("No icon found in game directory");
    None
}

/// Set window icon from an .ico file path.
/// Uses minifb's set_icon API — we keep the wide string alive for the call.
fn try_set_icon(window: &mut minifb::Window, ico_path: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        let path_str = ico_path.to_string_lossy();
        let wide: Vec<u16> = OsStr::new(path_str.as_ref())
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // Construct Icon::Path with our pointer — wide vec stays alive
        let icon = minifb::Icon::Path(wide.as_ptr());
        window.set_icon(icon);
        true
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, ico_path);
        false
    }
}

// ============================================================================
// PE Resource Parsing — extract icon from WILLY32.EXE
// ============================================================================

/// Extract all icons from a PE file and build a .ico file
fn extract_icon_from_pe(exe_path: &Path) -> Result<Vec<u8>, String> {
    let data = std::fs::read(exe_path).map_err(|e| format!("read: {}", e))?;

    // Check MZ signature
    if data.len() < 0x40 || &data[0..2] != b"MZ" {
        return Err("Not a valid PE file (no MZ)".into());
    }

    let pe_offset = read_u32(&data, 0x3C) as usize;
    if pe_offset + 4 > data.len() || &data[pe_offset..pe_offset + 4] != b"PE\0\0" {
        return Err("Not a valid PE file (no PE sig)".into());
    }

    // COFF header
    let num_sections = read_u16(&data, pe_offset + 6) as usize;
    let opt_header_size = read_u16(&data, pe_offset + 20) as usize;
    let magic = read_u16(&data, pe_offset + 24);
    if magic != 0x10B {
        return Err(format!("Not PE32 (magic=0x{:X})", magic));
    }

    // Resource directory RVA (data directory entry 2)
    let rsrc_rva = read_u32(&data, pe_offset + 24 + 112) as usize;
    if rsrc_rva == 0 {
        return Err("No resource section".into());
    }

    // Find .rsrc section
    let section_start = pe_offset + 24 + opt_header_size;
    let mut rsrc_file_offset = 0usize;
    for i in 0..num_sections {
        let s = section_start + i * 40;
        let s_rva = read_u32(&data, s + 12) as usize;
        let s_raw = read_u32(&data, s + 20) as usize;
        let s_rawsz = read_u32(&data, s + 16) as usize;
        if rsrc_rva >= s_rva && rsrc_rva < s_rva + s_rawsz {
            rsrc_file_offset = s_raw + (rsrc_rva - s_rva);
            break;
        }
    }
    if rsrc_file_offset == 0 {
        return Err("Could not map .rsrc RVA to file offset".into());
    }

    // Parse resource directory tree
    let rsrc_base = rsrc_file_offset;

    // Find RT_GROUP_ICON (type 14) and RT_ICON (type 3)
    let root_entries = parse_res_dir(&data, rsrc_base)?;

    let group_icon_dir = root_entries
        .iter()
        .find(|e| e.id == 14)
        .ok_or("No RT_GROUP_ICON resource")?;

    let icon_dir = root_entries
        .iter()
        .find(|e| e.id == 3)
        .ok_or("No RT_ICON resource")?;

    // Get first GROUP_ICON resource
    let gi_subs = parse_res_dir(&data, rsrc_base + (group_icon_dir.offset & 0x7FFF_FFFF) as usize)?;
    if gi_subs.is_empty() {
        return Err("No GROUP_ICON entries".into());
    }

    // Get first sub-entry, then first language
    let gi_first = &gi_subs[0];
    let gi_langs = parse_res_dir(&data, rsrc_base + (gi_first.offset & 0x7FFF_FFFF) as usize)?;
    if gi_langs.is_empty() {
        return Err("No GROUP_ICON language entries".into());
    }

    // Read data entry (RVA, size, codepage, reserved)
    let data_entry_off = rsrc_base + (gi_langs[0].offset & 0x7FFF_FFFF) as usize;
    let gi_data_rva = read_u32(&data, data_entry_off) as usize;
    let gi_data_size = read_u32(&data, data_entry_off + 4) as usize;
    let gi_file_off = rva_to_offset(&data, pe_offset, num_sections, section_start, gi_data_rva)
        .ok_or("Could not map GROUP_ICON data RVA")?;

    // Parse GRPICONDIRHEADER
    if gi_file_off + 6 > data.len() {
        return Err("GROUP_ICON data truncated".into());
    }
    let _reserved = read_u16(&data, gi_file_off);
    let icon_type = read_u16(&data, gi_file_off + 2);
    let icon_count = read_u16(&data, gi_file_off + 4) as usize;
    if icon_type != 1 || icon_count == 0 {
        return Err(format!("Invalid GROUP_ICON: type={}, count={}", icon_type, icon_count));
    }

    // Parse GRPICONDIRENTRY array (14 bytes each)
    struct GrpIconEntry {
        width: u8,
        height: u8,
        color_count: u8,
        planes: u16,
        bit_count: u16,
        _bytes_in_res: u32,
        icon_id: u16,
    }

    let mut entries = Vec::new();
    for i in 0..icon_count {
        let e = gi_file_off + 6 + i * 14;
        if e + 14 > data.len() {
            break;
        }
        entries.push(GrpIconEntry {
            width: data[e],
            height: data[e + 1],
            color_count: data[e + 2],
            planes: read_u16(&data, e + 4),
            bit_count: read_u16(&data, e + 6),
            _bytes_in_res: read_u32(&data, e + 8),
            icon_id: read_u16(&data, e + 12),
        });
    }

    // Now find each RT_ICON by ID and extract raw data
    let icon_subs = parse_res_dir(&data, rsrc_base + (icon_dir.offset & 0x7FFF_FFFF) as usize)?;

    struct IconData {
        width: u8,
        height: u8,
        color_count: u8,
        planes: u16,
        bit_count: u16,
        raw: Vec<u8>,
    }

    let mut icons = Vec::new();
    for entry in &entries {
        // Find matching RT_ICON sub-entry by ID
        let icon_sub = icon_subs.iter().find(|s| (s.id & 0x7FFF_FFFF) == entry.icon_id as u32);
        let icon_sub = match icon_sub {
            Some(s) => s,
            None => continue,
        };

        let lang_entries = parse_res_dir(&data, rsrc_base + (icon_sub.offset & 0x7FFF_FFFF) as usize)?;
        if lang_entries.is_empty() {
            continue;
        }

        let de_off = rsrc_base + (lang_entries[0].offset & 0x7FFF_FFFF) as usize;
        let icon_data_rva = read_u32(&data, de_off) as usize;
        let icon_data_size = read_u32(&data, de_off + 4) as usize;
        let icon_file_off = match rva_to_offset(&data, pe_offset, num_sections, section_start, icon_data_rva) {
            Some(o) => o,
            None => continue,
        };

        if icon_file_off + icon_data_size > data.len() {
            continue;
        }

        icons.push(IconData {
            width: entry.width,
            height: entry.height,
            color_count: entry.color_count,
            planes: entry.planes,
            bit_count: entry.bit_count,
            raw: data[icon_file_off..icon_file_off + icon_data_size].to_vec(),
        });
    }

    if icons.is_empty() {
        return Err("No icon data extracted".into());
    }

    // Build .ico file
    // ICONDIR header (6 bytes) + ICONDIRENTRY per icon (16 bytes each) + raw data
    let header_size = 6 + icons.len() * 16;
    let mut ico = Vec::with_capacity(header_size + icons.iter().map(|i| i.raw.len()).sum::<usize>());

    // ICONDIR
    ico.extend_from_slice(&0u16.to_le_bytes()); // reserved
    ico.extend_from_slice(&1u16.to_le_bytes()); // type = icon
    ico.extend_from_slice(&(icons.len() as u16).to_le_bytes()); // count

    // Calculate data offsets
    let mut data_offset = header_size as u32;
    for icon in &icons {
        // ICONDIRENTRY (16 bytes)
        ico.push(icon.width);
        ico.push(if icon.height > 0 && icon.height != 255 {
            // In GROUP_ICON entries, the height might be doubled (XOR+AND mask)
            // For the .ico ICONDIRENTRY, use the actual display height
            // We can get it from the BITMAPINFOHEADER in the raw data
            if icon.raw.len() >= 8 {
                let bmp_height = read_u32(&icon.raw, 4) as u16;
                // BITMAPINFOHEADER height is doubled for icon (XOR mask + AND mask)
                (bmp_height / 2) as u8
            } else {
                icon.height
            }
        } else {
            icon.height
        });
        ico.push(icon.color_count);
        ico.push(0); // reserved
        ico.extend_from_slice(&icon.planes.to_le_bytes());
        ico.extend_from_slice(&icon.bit_count.to_le_bytes());
        ico.extend_from_slice(&(icon.raw.len() as u32).to_le_bytes());
        ico.extend_from_slice(&data_offset.to_le_bytes());
        data_offset += icon.raw.len() as u32;
    }

    // Append raw icon data
    for icon in &icons {
        ico.extend_from_slice(&icon.raw);
    }

    let _ = gi_data_size; // used above

    Ok(ico)
}

// --- PE helper functions ---

struct ResEntry {
    id: u32,
    offset: u32,
}

fn parse_res_dir(data: &[u8], off: usize) -> Result<Vec<ResEntry>, String> {
    if off + 16 > data.len() {
        return Err("Resource directory truncated".into());
    }
    let num_named = read_u16(data, off + 12) as usize;
    let num_id = read_u16(data, off + 14) as usize;
    let total = num_named + num_id;
    let mut entries = Vec::with_capacity(total);
    for i in 0..total {
        let e = off + 16 + i * 8;
        if e + 8 > data.len() {
            break;
        }
        entries.push(ResEntry {
            id: read_u32(data, e),
            offset: read_u32(data, e + 4),
        });
    }
    Ok(entries)
}

fn rva_to_offset(
    data: &[u8],
    pe_offset: usize,
    num_sections: usize,
    section_start: usize,
    rva: usize,
) -> Option<usize> {
    let _ = pe_offset;
    for i in 0..num_sections {
        let s = section_start + i * 40;
        let s_rva = read_u32(data, s + 12) as usize;
        let s_raw = read_u32(data, s + 20) as usize;
        let s_vsize = read_u32(data, s + 8) as usize;
        if rva >= s_rva && rva < s_rva + s_vsize {
            return Some(s_raw + (rva - s_rva));
        }
    }
    None
}

fn read_u16(data: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([data[off], data[off + 1]])
}

fn read_u32(data: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_helpers() {
        let data = [0x01, 0x02, 0x03, 0x04];
        assert_eq!(read_u16(&data, 0), 0x0201);
        assert_eq!(read_u32(&data, 0), 0x04030201);
    }

    #[test]
    fn test_extract_from_willy32() {
        // Only run if WILLY32.EXE is available
        let candidates = [
            PathBuf::from("../../game/WILLY32.EXE"),
            PathBuf::from("game/WILLY32.EXE"),
        ];
        let exe = candidates.iter().find(|p| p.is_file());
        if let Some(exe_path) = exe {
            let ico_data = extract_icon_from_pe(exe_path)
                .expect("Should extract icon from WILLY32.EXE");

            // Validate ICO header
            assert_eq!(&ico_data[0..2], &[0, 0]); // reserved
            assert_eq!(&ico_data[2..4], &[1, 0]); // type = icon
            let count = read_u16(&ico_data, 4);
            assert!(count >= 1, "Should have at least 1 icon, got {}", count);
            assert!(ico_data.len() > 100, "ICO should be substantial");

            eprintln!(
                "Extracted {} icon(s), total {} bytes",
                count,
                ico_data.len()
            );
        } else {
            eprintln!("WILLY32.EXE not found, skipping icon extraction test");
        }
    }
}
