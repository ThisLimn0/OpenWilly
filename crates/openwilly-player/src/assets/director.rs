//! Parser for Macromedia Director 6 RIFX/XFIR file format
//!
//! Supports .DIR, .DXR, .CXT, .CST files.
//! Based on ShockwaveParser.py analysis + Director 6 format documentation.

use std::collections::HashMap;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use anyhow::{bail, Context, Result};
use byteorder::{BigEndian, LittleEndian, ReadBytesExt};

// ============================================================================
// Data types
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastType {
    Bitmap = 1,
    FilmLoop = 2,
    Field = 3,
    Palette = 4,
    Pict = 5,
    Sound = 6,
    Button = 7,
    Shape = 8,
    Movie = 9,
    DigitalVideo = 10,
    Script = 11,
    Text = 12,
    Ole = 13,
    Transition = 14,
    Unknown = 255,
}

impl From<u32> for CastType {
    fn from(v: u32) -> Self {
        match v {
            1 => CastType::Bitmap,
            2 => CastType::FilmLoop,
            3 => CastType::Field,
            4 => CastType::Palette,
            5 => CastType::Pict,
            6 => CastType::Sound,
            7 => CastType::Button,
            8 => CastType::Shape,
            9 => CastType::Movie,
            10 => CastType::DigitalVideo,
            11 => CastType::Script,
            12 => CastType::Text,
            13 => CastType::Ole,
            14 => CastType::Transition,
            _ => CastType::Unknown,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BitmapInfo {
    pub width: u16,
    pub height: u16,
    pub reg_x: i16,
    pub reg_y: i16,
    pub pos_x: i16,
    pub pos_y: i16,
    pub bit_depth: u8,
    pub bit_alpha: u8,
    pub palette_ref: i16,
}

impl BitmapInfo {
    /// Whether this bitmap has an alpha channel
    pub fn has_alpha(&self) -> bool {
        self.bit_alpha > 0
    }
}

/// A cue point embedded in a Director sound cast member (`cupt` chunk).
/// Used for lip-sync: "talk" = open mouth, "silence" = close mouth.
#[derive(Debug, Clone)]
pub struct CuePoint {
    /// Time offset in milliseconds (derived from sample offset)
    pub time_ms: u32,
    /// Cue name: "talk", "silence", "point", etc.
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct SoundInfo {
    pub sample_rate: u32,
    pub sample_size: u16,
    pub data_length: u32,
    pub looped: bool,
    pub codec: String,
    /// Cue points for lip-sync (from linked `cupt` chunk)
    pub cue_points: Vec<CuePoint>,
}

#[derive(Debug, Clone)]
pub struct CastMember {
    pub num: u32,
    pub name: String,
    pub cast_type: CastType,
    pub bitmap_info: Option<BitmapInfo>,
    pub sound_info: Option<SoundInfo>,
    pub palette_data: Option<Vec<[u8; 3]>>,
    pub text_content: Option<String>,
    /// Linked data chunks: "BITD" -> raw bytes, "sndS" -> raw bytes, etc.
    pub linked_data: HashMap<String, Vec<u8>>,
}

/// A parsed Director file
#[derive(Debug)]
pub struct DirectorFile {
    pub filename: String,
    pub big_endian: bool,
    pub version: String,
    pub movie_width: u16,
    pub movie_height: u16,
    pub created_by: String,
    pub modified_by: String,
    pub cast_members: HashMap<u32, CastMember>,
    /// Raw chunk directory
    chunks: Vec<ChunkEntry>,
}

impl DirectorFile {
    /// Summary string for logging — uses all metadata fields
    pub fn info_line(&self) -> String {
        format!(
            "{} v{} {}×{} ({}) chunks={} members={} by='{}' mod='{}'",
            self.filename,
            self.version,
            self.movie_width,
            self.movie_height,
            if self.big_endian { "BE" } else { "LE" },
            self.chunks.len(),
            self.cast_members.len(),
            self.created_by,
            self.modified_by,
        )
    }
}

#[derive(Debug, Clone)]
struct ChunkEntry {
    fourcc: String,
    length: u32,
    offset: u32,
    linked_entries: Vec<usize>,
}

// ============================================================================
// Parser
// ============================================================================

/// Endianness-aware reader wrapper
struct DirReader<R: Read + Seek> {
    inner: R,
    /// If true, multi-byte reads use big-endian (RIFX files)
    big_endian: bool,
}

impl<R: Read + Seek> DirReader<R> {
    fn new(inner: R, big_endian: bool) -> Self {
        Self { inner, big_endian }
    }

    fn read_u16(&mut self) -> io::Result<u16> {
        if self.big_endian {
            self.inner.read_u16::<BigEndian>()
        } else {
            self.inner.read_u16::<LittleEndian>()
        }
    }

    fn read_i16(&mut self) -> io::Result<i16> {
        if self.big_endian {
            self.inner.read_i16::<BigEndian>()
        } else {
            self.inner.read_i16::<LittleEndian>()
        }
    }

    fn read_u32(&mut self) -> io::Result<u32> {
        if self.big_endian {
            self.inner.read_u32::<BigEndian>()
        } else {
            self.inner.read_u32::<LittleEndian>()
        }
    }

    fn read_i32(&mut self) -> io::Result<i32> {
        if self.big_endian {
            self.inner.read_i32::<BigEndian>()
        } else {
            self.inner.read_i32::<LittleEndian>()
        }
    }

    fn read_u8(&mut self) -> io::Result<u8> {
        self.inner.read_u8()
    }

    fn read_fourcc(&mut self) -> io::Result<String> {
        let mut buf = [0u8; 4];
        self.inner.read_exact(&mut buf)?;
        if self.big_endian {
            Ok(String::from_utf8_lossy(&buf).into_owned())
        } else {
            // Little-endian files store FourCC reversed
            buf.reverse();
            Ok(String::from_utf8_lossy(&buf).into_owned())
        }
    }

    fn read_len_string(&mut self) -> io::Result<String> {
        let len = self.read_u8()? as usize;
        let mut buf = vec![0u8; len];
        self.inner.read_exact(&mut buf)?;
        // Padding byte for big-endian files
        if self.big_endian {
            let _ = self.inner.read_u8();
        }
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    fn read_bytes(&mut self, len: usize) -> io::Result<Vec<u8>> {
        let mut buf = vec![0u8; len];
        self.inner.read_exact(&mut buf)?;
        Ok(buf)
    }

    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }

    fn pos(&mut self) -> io::Result<u64> {
        self.inner.stream_position()
    }

    fn skip(&mut self, n: i64) -> io::Result<u64> {
        self.inner.seek(SeekFrom::Current(n))
    }

    // Force Big-Endian reads (for CASt, CAS*, MCsL, VWCF, VWFI, DRCF, sndH)
    fn read_u16_be(&mut self) -> io::Result<u16> {
        self.inner.read_u16::<BigEndian>()
    }

    fn read_i16_be(&mut self) -> io::Result<i16> {
        self.inner.read_i16::<BigEndian>()
    }

    fn read_u32_be(&mut self) -> io::Result<u32> {
        self.inner.read_u32::<BigEndian>()
    }

    fn read_i32_be(&mut self) -> io::Result<i32> {
        self.inner.read_i32::<BigEndian>()
    }
}

impl DirectorFile {
    pub fn parse(path: &Path) -> Result<Self> {
        let filename = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();

        let data = std::fs::read(path)
            .with_context(|| format!("Reading {}", path.display()))?;

        let cursor = io::Cursor::new(data);

        // Read magic to determine endianness
        let mut peek = io::Cursor::new(&cursor.get_ref()[..4]);
        let mut magic = [0u8; 4];
        peek.read_exact(&mut magic)?;
        let magic_str = String::from_utf8_lossy(&magic);

        let big_endian = match magic_str.as_ref() {
            "RIFX" => true,
            "XFIR" => false,
            _ => bail!("Not a Director file: magic={}", magic_str),
        };

        let mut reader = DirReader::new(cursor, big_endian);

        // Skip magic (already read)
        reader.skip(4)?;

        // File size
        let _file_size = reader.read_i32()?;

        // File signature (e.g. "MV93", "MC95")
        let file_sign = reader.read_fourcc()?;
        tracing::debug!("  Sign: {} (big_endian={})", file_sign, big_endian);

        // Read IMAP -> get MMAP offset
        // Note: IMAP/MMAP chunk headers (fourcc + length) are ALWAYS Little-Endian
        let _imap_fourcc = reader.read_fourcc()?;
        let _imap_len = reader.inner.read_u32::<LittleEndian>()?; // Always LE
        let _imap_unknown = reader.read_i16()?; // signed for possible sentinel values
        let _imap_unknown2 = reader.read_i16()?;
        let mmap_offset = reader.read_u32()?;

        // Read MMAP (chunk directory)
        reader.seek(SeekFrom::Start(mmap_offset as u64))?;
        let _mmap_fourcc = reader.read_fourcc()?;
        let _mmap_len = reader.inner.read_u32::<LittleEndian>()?; // Always LE
        let _version = reader.read_u32()?; // File endianness
        let _something1 = reader.inner.read_u32::<LittleEndian>()?; // Always LE
        let file_num = reader.read_u32()?; // File endianness
        let _something2 = reader.inner.read_u32::<LittleEndian>()?; // Always LE
        let _something3 = reader.inner.read_u32::<LittleEndian>()?; // Always LE
        let _something4 = reader.inner.read_u32::<LittleEndian>()?; // Always LE

        tracing::debug!("  MMAP: {} chunks", file_num);

        // Read chunk entries
        let mut chunks = Vec::with_capacity(file_num as usize);
        for _ in 0..file_num {
            let fourcc = reader.read_fourcc()?;
            let length = reader.read_u32()?;
            let offset = reader.read_u32()?;
            let _unknown1 = reader.read_u32()?;
            let _unknown2 = reader.read_u32()?;
            chunks.push(ChunkEntry {
                fourcc,
                length,
                offset,
                linked_entries: Vec::new(),
            });
        }

        // First pass: read metadata chunks
        let mut version = String::new();
        let mut movie_width = 640u16;
        let mut movie_height = 480u16;
        let mut created_by = String::new();
        let mut modified_by = String::new();

        for i in 0..chunks.len() {
            let chunk = &chunks[i];
            if chunk.offset == 0 || chunk.fourcc == "free" {
                continue;
            }

            match chunk.fourcc.as_str() {
                "DRCF" => {
                    reader.seek(SeekFrom::Start(chunk.offset as u64 + 8))?;
                    let v0 = reader.read_u8()?;
                    let v1 = reader.read_u8()?;
                    let hex = format!("{:02x}{:02x}", v0, v1);
                    version = match hex.as_str() {
                        "04c7" => "6.0".into(),
                        "057e" => "7.0".into(),
                        "0640" => "8.0".into(),
                        "073a" => "8.5/9.0".into(),
                        _ => hex,
                    };
                    tracing::debug!("  Version: {}", version);
                }
                "VWCF" => {
                    reader.seek(SeekFrom::Start(chunk.offset as u64 + 8))?;
                    reader.skip(8)?;
                    movie_height = reader.read_u16_be()?;
                    movie_width = reader.read_u16_be()?;
                }
                "VWFI" => {
                    reader.seek(SeekFrom::Start(chunk.offset as u64 + 8))?;
                    let skip_len = reader.read_u32_be()?;
                    reader.skip(skip_len as i64 - 4)?;
                    let field_num = reader.read_u16_be()? as usize;
                    reader.skip(4)?;
                    let mut offsets = Vec::with_capacity(field_num);
                    for _ in 0..field_num {
                        offsets.push(reader.read_u32_be()?);
                    }
                    let data_pos = reader.pos()?;
                    if field_num > 0 {
                        reader.seek(SeekFrom::Start(data_pos + offsets[0] as u64))?;
                        created_by = reader.read_len_string().unwrap_or_default();
                    }
                    if field_num > 1 {
                        reader.seek(SeekFrom::Start(data_pos + offsets[1] as u64))?;
                        modified_by = reader.read_len_string().unwrap_or_default();
                    }
                }
                _ => {}
            }
        }

        // Second pass: read KEY* table (links data chunks to cast members)
        for i in 0..chunks.len() {
            if chunks[i].fourcc != "KEY*" || chunks[i].offset == 0 {
                continue;
            }
            reader.seek(SeekFrom::Start(chunks[i].offset as u64 + 8))?;
            let _unknown1 = reader.read_u16()?;
            let _unknown2 = reader.read_u16()?;
            let _unknown3 = reader.read_u32()?;
            let entry_num = reader.read_u32()?;

            for _ in 0..entry_num {
                let cast_file_slot = reader.read_u32()? as usize;
                let cast_slot = reader.read_u32()? as usize;
                let _cast_type = reader.read_fourcc()?;

                if cast_slot < chunks.len() && cast_file_slot < chunks.len() {
                    chunks[cast_slot].linked_entries.push(cast_file_slot);
                }
            }
        }

        // Third pass: read CAS* (cast member lists) and CASt (cast member data)
        let mut cast_members = HashMap::new();

        // Find CAS* chunks — they list the cast member slots
        // CAS* data is ALWAYS Big-Endian regardless of container endianness
        let mut cas_star_members: HashMap<usize, u32> = HashMap::new(); // slot -> member_num
        let mut cas_star_count = 0u32;
        for i in 0..chunks.len() {
            if chunks[i].fourcc != "CAS*" || chunks[i].offset == 0 {
                continue;
            }
            cas_star_count += 1;
            reader.seek(SeekFrom::Start(chunks[i].offset as u64 + 8))?;
            let count = chunks[i].length / 4;
            tracing::debug!("  CAS* #{} at chunk {}: {} entries", cas_star_count, i, count);
            for j in 0..count {
                let slot = reader.read_u32_be()? as usize; // Always BE
                let member_num = j + 1;
                if slot > 0 && slot < chunks.len() {
                    if let Some(old) = cas_star_members.insert(slot, member_num) {
                        tracing::warn!("  CAS* collision: slot {} was member {} now member {}", slot, old, member_num);
                    }
                }
            }
        }
        if cas_star_count > 1 {
            tracing::warn!("  {} CAS* chunks found — member numbering may be incorrect!", cas_star_count);
        }

        // Parse each CASt chunk
        for (&slot, &member_num) in &cas_star_members {
            if chunks[slot].fourcc != "CASt" || chunks[slot].offset == 0 {
                continue;
            }

            match Self::parse_cast_member(&mut reader, &chunks, slot, member_num) {
                Ok(member) => {
                    cast_members.insert(member_num, member);
                }
                Err(e) => {
                    tracing::trace!("  Skip member {}: {}", member_num, e);
                }
            }
        }

        tracing::debug!(
            "  Parsed {} cast members ({} bitmaps, {} sounds)",
            cast_members.len(),
            cast_members.values().filter(|m| m.cast_type == CastType::Bitmap).count(),
            cast_members.values().filter(|m| m.cast_type == CastType::Sound).count(),
        );

        Ok(DirectorFile {
            filename,
            big_endian,
            version,
            movie_width,
            movie_height,
            created_by,
            modified_by,
            cast_members,
            chunks,
        })
    }

    fn parse_cast_member<R: Read + Seek>(
        reader: &mut DirReader<R>,
        chunks: &[ChunkEntry],
        slot: usize,
        member_num: u32,
    ) -> Result<CastMember> {
        let chunk = &chunks[slot];
        reader.seek(SeekFrom::Start(chunk.offset as u64 + 8))?;

        // CASt data is ALWAYS Big-Endian regardless of container endianness
        let cast_type_raw = reader.read_u32_be()?;
        let cast_type = CastType::from(cast_type_raw);
        let cast_data_length = reader.read_u32_be()?;
        let cast_end_data_length = reader.read_i32_be()?;
        tracing::trace!("  CASt member {}: type={:?} data_len={} end_data_len={}",
            member_num, cast_type, cast_data_length, cast_end_data_length);

        // Parse variable data block (name, flags)
        let mut name = String::new();
        let mut sound_looped = false;
        let mut sound_codec = String::new();

        if cast_data_length > 0 {
            // 16 × i16 unknown values (CASt is always BE)
            let mut unknowns = [0i16; 16];
            for u in &mut unknowns {
                *u = reader.read_i16_be()?;
            }
            sound_looped = unknowns[7] == 0;

            let field_num = reader.read_u16_be()? as usize;
            let mut field_offsets = Vec::with_capacity(field_num);
            for _ in 0..field_num {
                field_offsets.push(reader.read_u32_be()?);
            }
            let _field_data_length = reader.read_u32_be()?;

            // Read field strings
            let mut field_data = Vec::new();
            for _ in 0..field_num {
                let str_len = reader.read_u8()? as usize;
                let s = if str_len > 0 {
                    let bytes = reader.read_bytes(str_len)?;
                    String::from_utf8_lossy(&bytes).into_owned()
                } else {
                    String::new()
                };
                field_data.push(s);
            }

            if !field_data.is_empty() {
                name = field_data[0].clone();
            }
            if field_data.len() > 2 {
                sound_codec = field_data[2].clone();
            }
        }

        // Parse type-specific end data
        let mut bitmap_info = None;
        let mut sound_info = None;
        let mut palette_data = None;

        match cast_type {
            CastType::Bitmap => {
                let _unknown1 = reader.read_u16_be()?;
                let pos_y = reader.read_i16_be()?;
                let pos_x = reader.read_i16_be()?;
                let height_raw = reader.read_i16_be()?;
                let width_raw = reader.read_i16_be()?;
                let _unknown2 = reader.read_u32_be()?;
                let _unknown3 = reader.read_u32_be()?;
                let reg_y_raw = reader.read_i16_be()?;
                let reg_x_raw = reader.read_i16_be()?;
                let bit_alpha = reader.read_u8()?;
                let bit_depth = reader.read_u8()?;
                let _unknown4 = reader.read_u16_be()?;
                let palette_ref = reader.read_i16_be()?;

                bitmap_info = Some(BitmapInfo {
                    width: (width_raw - pos_x) as u16,
                    height: (height_raw - pos_y) as u16,
                    reg_x: reg_x_raw - pos_x,
                    reg_y: reg_y_raw - pos_y,
                    pos_x,
                    pos_y,
                    bit_depth,
                    bit_alpha,
                    palette_ref,
                });
            }
            CastType::Palette => {
                // Palette data is in a linked CLUT chunk — parsed later
            }
            CastType::Sound => {
                sound_info = Some(SoundInfo {
                    sample_rate: 22050, // Default, will be overridden from sndH
                    sample_size: 8,
                    data_length: 0,
                    looped: sound_looped,
                    codec: sound_codec,
                    cue_points: Vec::new(),
                });
            }
            _ => {}
        }

        // Read linked data chunks (BITD, sndS, sndH, CLUT, STXT, etc.)
        let mut linked_data = HashMap::new();
        let mut text_content = None;

        for &linked_idx in &chunk.linked_entries {
            if linked_idx >= chunks.len() {
                continue;
            }
            let linked = &chunks[linked_idx];
            if linked.offset == 0 {
                continue;
            }

            match linked.fourcc.as_str() {
                "BITD" => {
                    reader.seek(SeekFrom::Start(linked.offset as u64 + 8))?;
                    let data = reader.read_bytes(linked.length as usize)?;
                    linked_data.insert("BITD".to_string(), data);
                }
                "sndS" => {
                    reader.seek(SeekFrom::Start(linked.offset as u64 + 8))?;
                    let data = reader.read_bytes(linked.length as usize)?;
                    linked_data.insert("sndS".to_string(), data);
                }
                "sndH" => {
                    reader.seek(SeekFrom::Start(linked.offset as u64 + 8))?;
                    reader.skip(4)?;
                    let sound_length = reader.read_u32_be()?;
                    reader.skip(4)?;
                    reader.skip(20)?;
                    reader.skip(4)?;
                    reader.skip(4)?;
                    reader.skip(4)?;
                    let sample_rate = reader.read_u32_be()?;

                    if let Some(si) = &mut sound_info {
                        si.sample_rate = sample_rate;
                        si.data_length = sound_length;
                    }
                }
                "CLUT" => {
                    // Parse color lookup table
                    reader.seek(SeekFrom::Start(linked.offset as u64 + 8))?;
                    let num = (linked.length / 6) as usize;
                    let mut colors = Vec::with_capacity(num);
                    for _ in 0..num {
                        let r1 = reader.read_u8()?;
                        let _r2 = reader.read_u8()?;
                        let g1 = reader.read_u8()?;
                        let _g2 = reader.read_u8()?;
                        let b1 = reader.read_u8()?;
                        let _b2 = reader.read_u8()?;
                        colors.push([r1, g1, b1]);
                    }
                    colors.reverse(); // Director stores palettes reversed
                    palette_data = Some(colors);
                }
                "STXT" => {
                    reader.seek(SeekFrom::Start(linked.offset as u64 + 8))?;
                    reader.skip(4)?;
                    let text_len = reader.read_u32_be()? as usize;
                    let _style_len = reader.read_u32_be()?;
                    if text_len > 0 && text_len < 1_000_000 {
                        let bytes = reader.read_bytes(text_len)?;
                        text_content =
                            Some(String::from_utf8_lossy(&bytes).into_owned());
                    }
                }
                "cupt" => {
                    // Cue point chunk — lip-sync markers for sound cast members.
                    // Format: u32 entry_count, then per entry:
                    //   u16 unknown, u16 sample_offset, u8 name_len,
                    //   name_len bytes name, (31 - name_len) bytes padding
                    reader.seek(SeekFrom::Start(linked.offset as u64 + 8))?;
                    let entry_count = reader.read_u32_be()?;
                    let sample_rate = sound_info.as_ref().map(|s| s.sample_rate).unwrap_or(22050);
                    let mut cue_points = Vec::with_capacity(entry_count as usize);
                    for _ in 0..entry_count {
                        let _unknown = reader.read_u16_be()?;
                        let sample_offset = reader.read_u16_be()?;
                        let name_len = reader.read_u8()?;
                        let name = if name_len > 0 {
                            let bytes = reader.read_bytes(name_len as usize)?;
                            String::from_utf8_lossy(&bytes).into_owned()
                        } else {
                            String::new()
                        };
                        let pad = 31u8.saturating_sub(name_len) as usize;
                        if pad > 0 {
                            reader.skip(pad as i64)?;
                        }
                        // Convert sample offset to milliseconds
                        let time_ms = if sample_rate > 0 {
                            (sample_offset as u64 * 1000 / sample_rate as u64) as u32
                        } else {
                            sample_offset as u32
                        };
                        cue_points.push(CuePoint { time_ms, name });
                    }
                    if let Some(si) = &mut sound_info {
                        si.cue_points = cue_points;
                    }
                }
                _ => {}
            }
        }

        Ok(CastMember {
            num: member_num,
            name,
            cast_type,
            bitmap_info,
            sound_info,
            palette_data,
            text_content,
            linked_data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Diagnose cursor members 109-117 in 00.CXT
    ///
    /// Prints width, height, bit_depth, bit_alpha, palette_ref,
    /// registration point, and BITD data size for each member.
    #[test]
    fn diagnose_cursor_members_109_117() {
        // Try multiple paths to find 00.CXT
        let candidates = [
            "game/Movies/00.CXT",
            "../game/Movies/00.CXT",
            "../../game/Movies/00.CXT",
        ];
        let path = candidates.iter()
            .map(Path::new)
            .find(|p| p.exists());
        let path = match path {
            Some(p) => p,
            None => {
                // Try absolute path as last resort
                let abs = Path::new(r"D:\Projekte\OpenWilly\game\Movies\00.CXT");
                if abs.exists() {
                    abs
                } else {
                    eprintln!("SKIP: 00.CXT not found in any candidate path");
                    return;
                }
            }
        };

        let df = DirectorFile::parse(path).expect("Failed to parse 00.CXT");
        eprintln!("=== 00.CXT: {} ===", df.info_line());
        eprintln!();

        // Also scan for any members with cursor-like names
        let cursor_names = ["C_standard", "C_Grab", "C_Left", "C_Click",
                            "C_Back", "C_Right", "C_MoveLeft", "C_MoveRight", "C_MoveIn"];

        eprintln!("--- Members 109-117 ---");
        for num in 109..=117 {
            match df.cast_members.get(&num) {
                Some(member) => {
                    eprintln!("  Member {}: name='{}' type={:?}", num, member.name, member.cast_type);
                    if let Some(bi) = &member.bitmap_info {
                        eprintln!("    width={} height={} bit_depth={} bit_alpha={}",
                            bi.width, bi.height, bi.bit_depth, bi.bit_alpha);
                        eprintln!("    reg=({},{}) pos=({},{}) palette_ref={}",
                            bi.reg_x, bi.reg_y, bi.pos_x, bi.pos_y, bi.palette_ref);
                        if bi.bit_depth == 1 {
                            eprintln!("    *** WARNING: 1-bit depth — may cause decode issues for small cursors ***");
                        }
                    } else {
                        eprintln!("    (no bitmap_info)");
                    }
                    if let Some(bitd) = member.linked_data.get("BITD") {
                        eprintln!("    BITD data: {} bytes", bitd.len());
                        // Show first 32 bytes hex
                        let preview: Vec<String> = bitd.iter().take(32).map(|b| format!("{:02x}", b)).collect();
                        eprintln!("    BITD hex: {}", preview.join(" "));
                    } else {
                        eprintln!("    (no BITD data)  linked_keys={:?}", member.linked_data.keys().collect::<Vec<_>>());
                    }
                    eprintln!();
                }
                None => {
                    eprintln!("  Member {}: NOT FOUND", num);
                    eprintln!();
                }
            }
        }

        // Also search by cursor names
        eprintln!("--- Name-based cursor search ---");
        for name in &cursor_names {
            let found: Vec<_> = df.cast_members.iter()
                .filter(|(_, m)| m.name.eq_ignore_ascii_case(name))
                .collect();
            if found.is_empty() {
                eprintln!("  '{}': NOT FOUND", name);
            } else {
                for (&num, member) in &found {
                    eprintln!("  '{}' → member {} type={:?}", name, num, member.cast_type);
                    if let Some(bi) = &member.bitmap_info {
                        eprintln!("    width={} height={} bit_depth={} bit_alpha={} palette_ref={}",
                            bi.width, bi.height, bi.bit_depth, bi.bit_alpha, bi.palette_ref);
                    }
                }
            }
        }

        // Summary: list ALL bitmap members in range 100-130 for context
        eprintln!();
        eprintln!("--- All bitmap members 100-130 ---");
        for num in 100..=130 {
            if let Some(m) = df.cast_members.get(&num) {
                if m.cast_type == CastType::Bitmap {
                    let bi = m.bitmap_info.as_ref().unwrap();
                    eprintln!("  #{}: '{}' {}×{} depth={} alpha={} pal={}",
                        num, m.name, bi.width, bi.height, bi.bit_depth, bi.bit_alpha, bi.palette_ref);
                }
            }
        }
    }
}
