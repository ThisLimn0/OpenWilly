/// OpenWilly Player — Modern Rust recreation of Willy Werkel (Director 6)
///
/// Architecture:
///   assets/   — Director file parser (.CXT, .DXR, .CST)
///   engine/   — Renderer, audio, input, scene system
///   game/     — Game logic (scenes, car building, driving, actors)

mod assets;
mod engine;
mod game;

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("openwilly=debug".parse()?))
        .init();

    tracing::info!("OpenWilly Player v{}", env!("CARGO_PKG_VERSION"));

    // Find game data — supports: extracted dir, ISO file, or mounted ISO
    let game_dir = find_game_data()?;
    tracing::info!("Game data: {}", game_dir.display());

    // Load game assets from Director files
    let asset_store = assets::AssetStore::load(&game_dir)?;
    tracing::info!(
        "Loaded {} cast members from {} files",
        asset_store.total_members(),
        asset_store.total_files()
    );

    // Start game engine
    engine::run(asset_store)
}

/// Locate game data. Priority:
/// 1. Command-line argument (directory or .iso file)
/// 2. Extracted game files in well-known directories
/// 3. ISO file in current directory or nearby
/// 4. Mounted drive letters (D:–Z:) with Willy Werkel signature files
fn find_game_data() -> Result<PathBuf> {
    // --- 1. Command-line argument ---
    if let Some(arg) = std::env::args().nth(1) {
        let path = PathBuf::from(&arg);
        if path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("iso")) == Some(true) {
            if path.is_file() {
                tracing::info!("ISO file specified: {}", path.display());
                return extract_iso_to_cache(&path);
            }
        }
        if path.is_dir() && is_game_dir(&path) {
            return Ok(path);
        }
        // Maybe it's a directory that contains an ISO
        if path.is_dir() {
            if let Some(iso) = find_iso_in_dir(&path) {
                tracing::info!("Found ISO in specified directory: {}", iso.display());
                return extract_iso_to_cache(&iso);
            }
        }
        if path.exists() {
            // Try it anyway (maybe it's a game dir with unusual layout)
            return Ok(path);
        }
        tracing::warn!("Specified path not found: {}", arg);
    }

    // --- 2. Well-known extracted directories ---
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let base_dirs = [
        cwd.clone(),
        exe_dir.clone(),
        cwd.join("game"),
        exe_dir.join("game"),
        cwd.join("game_data"),
        exe_dir.join("game_data"),
    ];

    for dir in &base_dirs {
        if dir.is_dir() && is_game_dir(dir) {
            return Ok(dir.clone());
        }
    }

    // --- 3. ISO file nearby ---
    let search_dirs = [&cwd, &exe_dir];
    for dir in &search_dirs {
        if let Some(iso) = find_iso_in_dir(dir) {
            tracing::info!("Found ISO: {}", iso.display());
            return extract_iso_to_cache(&iso);
        }
    }

    // --- 4. Mounted drives (D:–Z:) ---
    for letter in b'D'..=b'Z' {
        let drive = format!("{}:\\", letter as char);
        let drive_path = PathBuf::from(&drive);
        if drive_path.exists() && is_game_dir(&drive_path) {
            tracing::info!("Found game on mounted drive {}", drive);
            return Ok(drive_path);
        }
    }

    anyhow::bail!(
        "Game data not found!\n\n\
         Place one of the following next to openwilly.exe:\n\
         • An .iso file of 'Autos bauen mit Willy Werkel'\n\
         • A 'game/' or 'game_data/' folder with extracted game files\n\n\
         Or pass the path as argument:  openwilly.exe <path-to-iso-or-folder>\n\n\
         Expected signature files: DATA.CST, Startcd.dir, AUTOBAU.HLP"
    )
}

/// Check if a directory looks like it contains Willy Werkel game files
fn is_game_dir(dir: &Path) -> bool {
    // Check for signature files (case-insensitive on Windows)
    let signatures = [
        "DATA.CST", "Startcd.dir", "AUTOBAU.HLP",
        "WILLY32.EXE", "Data/DATA.CST",
    ];
    for sig in &signatures {
        if dir.join(sig).exists() {
            return true;
        }
    }
    // Also check for Movies/ with Director files
    let movies = dir.join("Movies");
    if movies.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&movies) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_uppercase();
                if name.ends_with(".DXR") || name.ends_with(".CXT") {
                    return true;
                }
            }
        }
    }
    false
}

/// Find an ISO file in a directory (first .iso file found)
fn find_iso_in_dir(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if ext.eq_ignore_ascii_case("iso") {
                    return Some(path);
                }
            }
        }
    }
    None
}

/// Extract ISO contents to a cache directory next to the ISO/exe
/// Returns the path to the extracted game data
fn extract_iso_to_cache(iso_path: &Path) -> Result<PathBuf> {
    // Determine cache location: next to the executable, or next to the ISO
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()));
    let iso_dir = iso_path.parent().map(|p| p.to_path_buf());

    let cache_dir = exe_dir
        .or(iso_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("game_data");

    // If already extracted with enough files, reuse
    if cache_dir.is_dir() && is_game_dir(&cache_dir) {
        let file_count = count_game_files(&cache_dir);
        if file_count >= 10 {
            tracing::info!(
                "Using cached extraction at {} ({} files)",
                cache_dir.display(),
                file_count
            );
            return Ok(cache_dir);
        }
    }

    tracing::info!(
        "Extracting ISO {} → {}",
        iso_path.display(),
        cache_dir.display()
    );
    println!("Extracting game data from ISO...");
    println!("  Source: {}", iso_path.display());
    println!("  Target: {}", cache_dir.display());

    std::fs::create_dir_all(&cache_dir)
        .with_context(|| format!("Failed to create {}", cache_dir.display()))?;

    extract_iso_contents(iso_path, &cache_dir)?;

    let file_count = count_game_files(&cache_dir);
    println!("  Extracted {} game files.", file_count);

    if !is_game_dir(&cache_dir) {
        anyhow::bail!(
            "ISO extraction completed but no game files found.\n\
             The ISO may not be a valid Willy Werkel game disc."
        );
    }

    Ok(cache_dir)
}

/// Extract ISO9660 contents into target directory using the iso9660 crate
fn extract_iso_contents(iso_path: &Path, target: &Path) -> Result<()> {
    use iso9660::{ISO9660, DirectoryEntry};
    use std::io::{Read, Seek};

    let file = std::fs::File::open(iso_path)
        .with_context(|| format!("Failed to open ISO: {}", iso_path.display()))?;

    match ISO9660::new(file) {
        Ok(iso) => {
            fn extract_dir<T: Read + Seek>(
                dir: &iso9660::ISODirectory<T>,
                target: &Path,
                prefix: &str,
            ) -> Result<()> {
                for entry in dir.contents() {
                    let entry = entry?;
                    let name = entry.identifier().to_string();

                    // Skip . and ..
                    if name == "\0" || name == "\x01" || name.is_empty() {
                        continue;
                    }

                    // Clean version suffix (";1")
                    let clean = if let Some(idx) = name.find(';') {
                        &name[..idx]
                    } else {
                        &name
                    };

                    let rel_path = if prefix.is_empty() {
                        clean.to_string()
                    } else {
                        format!("{}/{}", prefix, clean)
                    };

                    match entry {
                        DirectoryEntry::Directory(subdir) => {
                            let dst = target.join(&rel_path);
                            std::fs::create_dir_all(&dst)?;
                            extract_dir(&subdir, target, &rel_path)?;
                        }
                        DirectoryEntry::File(iso_file) => {
                            let dst = target.join(&rel_path);
                            if let Some(parent) = dst.parent() {
                                std::fs::create_dir_all(parent)?;
                            }

                            // Skip installer/autorun files
                            let upper = clean.to_uppercase();
                            if upper == "AUTORUN.INF"
                                || upper == "SETUP.EXE"
                                || upper == "INSTALL.EXE"
                                || upper.ends_with(".INI")
                                    && (upper.contains("SETUP") || upper.contains("INSTALL"))
                            {
                                continue;
                            }

                            let mut reader = iso_file.read();
                            let mut out = std::fs::File::create(&dst)
                                .with_context(|| format!("Failed to create: {}", dst.display()))?;
                            std::io::copy(&mut reader, &mut out)?;

                            let size = iso_file.size();
                            if size > 1_000_000 {
                                println!(
                                    "  {} ({:.1} MB)",
                                    rel_path,
                                    size as f64 / 1_000_000.0
                                );
                            }
                        }
                    }
                }
                Ok(())
            }

            extract_dir(&iso.root, target, "")
        }
        Err(e) => {
            // iso9660 crate fails on some ISOs (e.g. null-filled timestamps).
            // Fall back to our own robust raw parser.
            tracing::warn!("iso9660 crate failed ({}), using fallback parser", e);
            println!("  Note: using fallback ISO parser...");
            extract_iso_raw(iso_path, target)
        }
    }
}

// ─── Fallback raw ISO9660 parser ────────────────────────────────────────────

/// Robust fallback ISO extractor that reads ISO9660 structures manually.
/// Handles ISOs where the `iso9660` crate fails (null timestamps, non-UTF8, etc.)
fn extract_iso_raw(iso_path: &Path, target: &Path) -> Result<()> {
    use std::io::{Read, Seek, SeekFrom};

    const SECTOR_SIZE: u64 = 2048;

    let mut file = std::fs::File::open(iso_path)?;

    // Read Primary Volume Descriptor (sector 16)
    file.seek(SeekFrom::Start(16 * SECTOR_SIZE))?;
    let mut pvd = [0u8; 2048];
    file.read_exact(&mut pvd)?;

    // Verify PVD signature: byte 0 = type 1, bytes 1..6 = "CD001"
    if &pvd[1..6] != b"CD001" {
        anyhow::bail!("Not a valid ISO 9660 image (missing CD001 signature)");
    }

    // Root directory record is at PVD offset 156, 34 bytes long
    // LBA of root directory extent (little-endian u32 at offset 158 in PVD)
    let root_lba = u32::from_le_bytes([pvd[158], pvd[159], pvd[160], pvd[161]]) as u64;
    // Data length of root directory (little-endian u32 at offset 166 in PVD)
    let root_size = u32::from_le_bytes([pvd[166], pvd[167], pvd[168], pvd[169]]) as u64;

    tracing::info!("ISO PVD: root directory at LBA {}, size {} bytes", root_lba, root_size);

    extract_iso_directory_raw(&mut file, root_lba, root_size, target, "")
}

/// Recursively extract files from an ISO directory using raw sector reading
fn extract_iso_directory_raw(
    file: &mut std::fs::File,
    dir_lba: u64,
    dir_size: u64,
    target: &Path,
    current_path: &str,
) -> Result<()> {
    use std::io::{Read, Seek, SeekFrom, Write};

    const SECTOR_SIZE: u64 = 2048;

    let file_len = file.metadata()?.len();
    let max_lba = file_len / SECTOR_SIZE;

    if dir_lba >= max_lba {
        anyhow::bail!("Directory LBA {} beyond ISO end (max {})", dir_lba, max_lba);
    }

    // Read directory data (may span multiple sectors)
    let sectors_needed = (dir_size + SECTOR_SIZE - 1) / SECTOR_SIZE;
    let sectors_to_read = std::cmp::min(sectors_needed, max_lba.saturating_sub(dir_lba));

    let mut dir_data = vec![0u8; (sectors_to_read * SECTOR_SIZE) as usize];
    file.seek(SeekFrom::Start(dir_lba * SECTOR_SIZE))?;
    let bytes_read = file.read(&mut dir_data)?;
    dir_data.truncate(bytes_read);

    if dir_data.is_empty() {
        return Ok(());
    }

    let mut offset = 0usize;

    while offset < dir_size as usize && offset < dir_data.len() {
        let record_len = dir_data[offset] as usize;
        if record_len == 0 {
            // Padding at sector boundary — advance to next sector
            let next_sector = ((offset / SECTOR_SIZE as usize) + 1) * SECTOR_SIZE as usize;
            if next_sector >= dir_data.len() {
                break;
            }
            offset = next_sector;
            continue;
        }

        if offset + record_len > dir_data.len() {
            break;
        }

        let record = &dir_data[offset..offset + record_len];

        // ISO 9660 directory entry layout:
        //   [2..6]   extent LBA (LE u32)
        //   [10..14] data length (LE u32)
        //   [25]     file flags
        //   [26]     file unit size (interleave)
        //   [28]     interleave gap size
        //   [32]     name length
        //   [33..]   name
        let extent_lba = u32::from_le_bytes([record[2], record[3], record[4], record[5]]) as u64;
        let data_length = u32::from_le_bytes([record[10], record[11], record[12], record[13]]) as u64;
        let file_flags = record[25];
        let file_unit_size = record[26];
        let interleave_gap = record[28];
        let name_len = record[32] as usize;

        if name_len == 0 || offset + 33 + name_len > dir_data.len() {
            offset += record_len;
            continue;
        }

        let name_bytes = &record[33..33 + name_len];

        // Skip . and .. entries (encoded as 0x00 and 0x01)
        if name_bytes == [0x00] || name_bytes == [0x01] {
            offset += record_len;
            continue;
        }

        // Decode name (lossy for non-UTF8 compatibility)
        let name = String::from_utf8_lossy(name_bytes).to_string();

        // Remove version suffix (";1")
        let clean_name = if let Some(idx) = name.find(';') {
            &name[..idx]
        } else {
            &name
        };

        let entry_path = if current_path.is_empty() {
            clean_name.to_string()
        } else {
            format!("{}/{}", current_path, clean_name)
        };

        let is_directory = (file_flags & 0x02) != 0;
        let is_interleaved = file_unit_size > 0 && interleave_gap > 0;

        if is_directory {
            let dst_dir = target.join(&entry_path);
            std::fs::create_dir_all(&dst_dir)?;
            extract_iso_directory_raw(file, extent_lba, data_length, target, &entry_path)?;
        } else {
            // Skip installer/autorun files
            let upper = clean_name.to_uppercase();
            if upper == "AUTORUN.INF"
                || upper == "SETUP.EXE"
                || upper == "INSTALL.EXE"
                || upper.starts_with("_INST")
                || upper.starts_with("_SETUP")
                || upper.starts_with("_ISDEL")
                || upper.starts_with("_ISRES")
                || (upper.ends_with(".INI") && (upper.contains("SETUP") || upper.contains("INSTALL")))
            {
                offset += record_len;
                continue;
            }

            let dst_path = target.join(&entry_path);
            if let Some(parent) = dst_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            if !dst_path.exists() {
                if is_interleaved {
                    tracing::debug!(
                        "Extracting interleaved file: {} (unit={}, gap={})",
                        entry_path, file_unit_size, interleave_gap
                    );
                    extract_interleaved_file(
                        file, extent_lba, data_length,
                        file_unit_size, interleave_gap, &dst_path,
                    )?;
                } else {
                    // Normal file — sequential read
                    if extent_lba >= max_lba {
                        tracing::warn!("Skipping {} — LBA {} beyond ISO end", entry_path, extent_lba);
                        offset += record_len;
                        continue;
                    }

                    file.seek(SeekFrom::Start(extent_lba * SECTOR_SIZE))?;

                    let available = file_len.saturating_sub(extent_lba * SECTOR_SIZE);
                    let to_read = std::cmp::min(data_length, available) as usize;

                    let mut data = vec![0u8; to_read];
                    let n = file.read(&mut data)?;
                    data.truncate(n);

                    let mut out = std::fs::File::create(&dst_path)?;
                    out.write_all(&data)?;

                    if data_length > 1_000_000 {
                        println!(
                            "  {} ({:.1} MB)",
                            entry_path,
                            data_length as f64 / 1_000_000.0
                        );
                    }
                }
            }
        }

        offset += record_len;
    }

    Ok(())
}

/// Extract an interleaved file by reading data units and skipping gap sectors
fn extract_interleaved_file(
    file: &mut std::fs::File,
    start_lba: u64,
    data_length: u64,
    file_unit_size: u8,
    interleave_gap: u8,
    dst_path: &Path,
) -> Result<()> {
    use std::io::{Read, Seek, SeekFrom, Write};

    const SECTOR_SIZE: u64 = 2048;

    let file_len = file.metadata()?.len();
    let max_lba = file_len / SECTOR_SIZE;

    if start_lba >= max_lba {
        anyhow::bail!("Interleaved file start LBA {} beyond ISO end", start_lba);
    }

    let unit_sectors = file_unit_size as u64;
    let gap_sectors = interleave_gap as u64;
    let bytes_per_unit = unit_sectors * SECTOR_SIZE;

    let mut output = std::fs::File::create(dst_path)?;
    let mut bytes_written = 0u64;
    let mut current_lba = start_lba;

    while bytes_written < data_length {
        if current_lba >= max_lba {
            tracing::warn!(
                "Interleaved extraction partial: {} of {} bytes at LBA {}",
                bytes_written, data_length, current_lba
            );
            break;
        }

        file.seek(SeekFrom::Start(current_lba * SECTOR_SIZE))?;

        let bytes_to_read = std::cmp::min(bytes_per_unit, data_length - bytes_written);
        let available = file_len.saturating_sub(current_lba * SECTOR_SIZE);
        let actual = std::cmp::min(bytes_to_read, available) as usize;

        if actual == 0 {
            break;
        }

        let mut buffer = vec![0u8; actual];
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        buffer.truncate(n);
        output.write_all(&buffer)?;
        bytes_written += n as u64;

        // Skip to next unit (data unit + gap)
        current_lba += unit_sectors + gap_sectors;
    }

    Ok(())
}

/// Count Director game files in a directory (recursive)
fn count_game_files(dir: &Path) -> usize {
    let mut count = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                count += count_game_files(&path);
            } else if path.is_file() {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_uppercase();
                if matches!(
                    ext.as_str(),
                    "DXR" | "CXT" | "CST" | "DIR" | "HLP" | "EXE" | "X32"
                ) {
                    count += 1;
                }
            }
        }
    }
    count
}
