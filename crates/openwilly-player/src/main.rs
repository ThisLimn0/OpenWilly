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

    let iso = ISO9660::new(file)
        .with_context(|| format!("Failed to parse ISO9660: {}", iso_path.display()))?;

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
