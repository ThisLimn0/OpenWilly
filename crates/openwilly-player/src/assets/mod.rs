//! Director 6 file format parser and asset store
//!
//! Parses .DIR, .DXR, .CXT, .CST files (RIFX/XFIR container format)
//! Extracts bitmaps, sounds, palettes, text, and scripts.

pub mod director;
pub mod bitmap;
pub mod palette;
pub mod sound;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use anyhow::Result;

/// Central asset store — loads all Director files and provides access to cast members
pub struct AssetStore {
    /// Parsed Director files, keyed by filename (e.g. "00.CXT", "03.DXR")
    pub files: HashMap<String, director::DirectorFile>,
    /// Base path to game data
    pub game_dir: PathBuf,
}

impl AssetStore {
    /// Load all Director files from the game directory
    pub fn load(game_dir: &Path) -> Result<Self> {
        let mut files = HashMap::new();

        for entry in std::fs::read_dir(game_dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_uppercase();

            if matches!(ext.as_str(), "CXT" | "DXR" | "CST" | "DIR") {
                let name = path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                tracing::info!("Parsing: {}", name);
                match director::DirectorFile::parse(&path) {
                    Ok(df) => {
                        tracing::info!("  {}", df.info_line());
                        files.insert(name, df);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse {}: {}", path.display(), e);
                    }
                }
            }
        }

        // Also scan Data/ and Movies/ subdirectories
        for subdir_name in &["Data", "Movies", "Autos"] {
            let subdir = game_dir.join(subdir_name);
            if !subdir.is_dir() {
                continue;
            }
            for entry in std::fs::read_dir(&subdir)? {
                let entry = entry?;
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_uppercase();

                if matches!(ext.as_str(), "CXT" | "DXR" | "CST" | "DIR") {
                    let name = path
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned();
                    if files.contains_key(&name) {
                        continue; // Skip duplicates
                    }
                    tracing::info!("Parsing: {}/{}", subdir_name, name);
                    match director::DirectorFile::parse(&path) {
                        Ok(df) => {
                            tracing::info!("  {}", df.info_line());
                            files.insert(name, df);
                        }
                        Err(e) => {
                            tracing::warn!("Failed to parse {}: {}", path.display(), e);
                        }
                    }
                }
            }
        }

        Ok(Self {
            files,
            game_dir: game_dir.to_path_buf(),
        })
    }

    pub fn total_files(&self) -> usize {
        self.files.len()
    }

    pub fn total_members(&self) -> usize {
        self.files.values().map(|f| f.cast_members.len()).sum()
    }

    /// Get a cast member by file name and member number
    pub fn get_member(&self, file: &str, num: u32) -> Option<&director::CastMember> {
        self.files.get(file).and_then(|f| f.cast_members.get(&num))
    }

    /// Decode a bitmap cast member to RGBA pixels (opaque, for backgrounds)
    pub fn decode_bitmap(
        &self,
        file: &str,
        num: u32,
    ) -> Option<bitmap::DecodedBitmap> {
        self.decode_bitmap_inner(file, num, None)
    }

    /// Decode a bitmap cast member to RGBA with white (idx 255) as transparent
    /// (for overlay sprites)
    pub fn decode_bitmap_transparent(
        &self,
        file: &str,
        num: u32,
    ) -> Option<bitmap::DecodedBitmap> {
        self.decode_bitmap_inner(file, num, Some(255))
    }

    fn decode_bitmap_inner(
        &self,
        file: &str,
        num: u32,
        transparent_color: Option<u8>,
    ) -> Option<bitmap::DecodedBitmap> {
        let df = self.files.get(file)?;
        let member = df.cast_members.get(&num)?;
        if member.cast_type != director::CastType::Bitmap {
            return None;
        }
        let bitmap_info = member.bitmap_info.as_ref()?;
        let bitd_data = member.linked_data.get("BITD")?;

        if bitmap_info.has_alpha() {
            tracing::trace!("Bitmap #{} has alpha channel (bit_alpha={})", num, bitmap_info.bit_alpha);
        }

        let palette = self.resolve_palette(file, bitmap_info.palette_ref);

        Some(bitmap::decode_bitd(
            bitd_data,
            bitmap_info.width,
            bitmap_info.height,
            bitmap_info.bit_depth,
            &palette,
            transparent_color,
        ))
    }

    /// Decode a sound cast member to a DecodedSound
    pub fn decode_sound(&self, file: &str, num: u32) -> Option<sound::DecodedSound> {
        let df = self.files.get(file)?;
        let member = df.cast_members.get(&num)?;
        if member.cast_type != director::CastType::Sound {
            return None;
        }
        let snd_data = member.linked_data.get("sndS")?;
        let si = member.sound_info.as_ref()?;

        tracing::trace!("Decode sound #{}: rate={}, codec='{}', looped={}, size={}",
            num, si.sample_rate, si.codec, si.looped, si.data_length);

        let bits = if si.sample_size > 0 { si.sample_size } else { 8 };
        Some(sound::DecodedSound::from_raw_pcm(snd_data, si.sample_rate, bits))
    }

    /// Get the duration of a named sound in milliseconds.
    pub fn sound_duration_ms(&self, name: &str) -> u32 {
        if let Some((file, num)) = self.find_sound_by_name(name) {
            if let Some(decoded) = self.decode_sound(&file, num) {
                return decoded.duration_ms();
            }
        }
        0
    }

    /// Find a sound cast member by name across all files.
    /// Returns (filename, member_num) if found.
    pub fn find_sound_by_name(&self, name: &str) -> Option<(String, u32)> {
        for (fname, df) in &self.files {
            for (num, member) in &df.cast_members {
                if member.cast_type == director::CastType::Sound && member.name == name {
                    return Some((fname.clone(), *num));
                }
            }
        }
        None
    }

    /// Find cue points for a sound cast member by name.
    /// Returns the cue point list (empty if none found).
    pub fn find_cue_points(&self, name: &str) -> Vec<director::CuePoint> {
        for (_fname, df) in &self.files {
            for (_num, member) in &df.cast_members {
                if member.cast_type == director::CastType::Sound && member.name == name {
                    if let Some(si) = &member.sound_info {
                        return si.cue_points.clone();
                    }
                }
            }
        }
        Vec::new()
    }

    /// Find a bitmap cast member by name across all files,
    /// decode it as transparent, and return the decoded bitmap.
    /// Member names are like "20b001v2" (found mainly in CDDATA.CXT).
    pub fn find_bitmap_by_name(&self, name: &str) -> Option<bitmap::DecodedBitmap> {
        for (fname, df) in &self.files {
            for (num, member) in &df.cast_members {
                if member.cast_type == director::CastType::Bitmap && member.name == name {
                    return self.decode_bitmap_transparent(fname, *num);
                }
            }
        }
        None
    }

    /// Find a bitmap cast member by name across all files.
    /// Returns (filename, member_num, BitmapInfo) if found.
    pub fn find_bitmap_info_by_name(&self, name: &str) -> Option<(String, u32, &director::BitmapInfo)> {
        for (fname, df) in &self.files {
            for (num, member) in &df.cast_members {
                if member.cast_type == director::CastType::Bitmap && member.name == name {
                    if let Some(bi) = &member.bitmap_info {
                        return Some((fname.clone(), *num, bi));
                    }
                }
            }
        }
        None
    }

    /// Resolve a palette reference to actual RGB data
    fn resolve_palette(&self, file: &str, palette_ref: i16) -> Vec<[u8; 3]> {
        if palette_ref >= 1 {
            // Custom palette from cast member
            if let Some(df) = self.files.get(file) {
                if let Some(member) = df.cast_members.get(&(palette_ref as u32)) {
                    if let Some(pal) = &member.palette_data {
                        return pal.clone();
                    }
                    // Fallback: if no parsed palette_data but raw CLUT linked data exists
                    if let Some(clut_data) = member.linked_data.get("CLUT") {
                        return palette::parse_clut(clut_data);
                    }
                }
            }
            // Fallback: check shared cast (00.CXT)
            if let Some(df) = self.files.get("00.CXT") {
                if let Some(member) = df.cast_members.get(&(palette_ref as u32)) {
                    if let Some(pal) = &member.palette_data {
                        return pal.clone();
                    }
                    if let Some(clut_data) = member.linked_data.get("CLUT") {
                        return palette::parse_clut(clut_data);
                    }
                }
            }
        }

        match -palette_ref {
            100 => palette::windows_palette(),
            0 => palette::mac_palette(),
            _ => palette::windows_palette(), // Default
        }
    }
}
