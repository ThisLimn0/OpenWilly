"""RIFX / XFIR container parser for Director 5/6 files.

Reads the memory map, cast libraries, KEY* table, and all cast members
with their associated resource chunks (BITD, sndS, CLUT, STXT, Lscr, etc.).
"""

from __future__ import annotations

import logging
import struct
from dataclasses import dataclass, field
from pathlib import Path
from typing import IO, Any, BinaryIO

from .chunks import CAST_TYPE_NAMES, VERSION_TABLE, CastType, ChunkType

log = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# Data classes
# ---------------------------------------------------------------------------


@dataclass
class FileEntry:
    """A single entry in the MMAP (memory map)."""

    id: int
    type: str
    data_length: int
    data_offset: int
    pointer_offset: int
    linked_entries: list[int] = field(default_factory=list)


@dataclass
class KeyEntry:
    """A single entry in the KEY* table (resource linkage)."""

    cast_file_slot: int
    cast_slot: int
    cast_type: str  # FourCC


@dataclass
class CastMember:
    """Parsed CASt member with metadata and linked resources."""

    slot: int
    library: str
    file_slot: int
    cast_type: int
    name: str = ""
    fields: list[str] = field(default_factory=list)

    # Bitmap specific
    image_width: int = 0
    image_height: int = 0
    image_pos_x: int = 0
    image_pos_y: int = 0
    image_reg_x: int = 0
    image_reg_y: int = 0
    image_bit_depth: int = 0
    image_palette: int = 0
    image_alpha: int = 0

    # Sound specific
    sound_looped: bool = False
    sound_codec: str = ""
    sound_sample_rate: int = 0
    sound_length: int = 0
    sound_sample_size: int = 8
    sound_data_length: int = 0
    sound_channels: int = 1
    sound_cue_points: list[tuple[int, str]] = field(default_factory=list)

    @property
    def sound_duration_seconds(self) -> float:
        """Sound duration in seconds (computed from length + sample rate)."""
        if self.sound_sample_rate > 0 and self.sound_length > 0:
            return self.sound_length / self.sound_sample_rate
        return 0.0

    # Palette specific
    palette_data: list[tuple[int, int, int]] = field(default_factory=list)

    # Shape specific (CastType 8)
    shape_data: Any = None
    # Button specific (CastType 7)
    button_data: Any = None
    # Transition specific (CastType 14)
    transition_data: Any = None
    # Digital Video specific (CastType 10)
    digital_video_data: Any = None
    # Picture specific (CastType 5)
    picture_data: Any = None
    # Film Loop specific (CastType 2)
    film_loop_data: Any = None

    # Links
    linked_entries: list[int] = field(default_factory=list)
    data_offset: int = 0
    data_length: int = 0

    # Raw unknown fields (for debugging / future parsing)
    unknown_fields: list[int] = field(default_factory=list)

    @property
    def type_name(self) -> str:
        return CAST_TYPE_NAMES.get(self.cast_type, f"Unknown({self.cast_type})")


@dataclass
class CastLibrary:
    """A cast library containing multiple CastMembers."""

    id: int
    name: str
    path: str = ""
    external: bool = False
    member_count: int = 0
    lib_slot: int = -1
    members: dict[int, CastMember] = field(default_factory=dict)


# ---------------------------------------------------------------------------
# Binary reader helpers
# ---------------------------------------------------------------------------


class BinaryReader:
    """Wraps a file handle with endian-aware read methods."""

    def __init__(self, f: BinaryIO, little_endian: bool = False):
        self.f = f
        self.little_endian = little_endian

    @property
    def pos(self) -> int:
        return self.f.tell()

    def seek(self, offset: int, whence: int = 0) -> None:
        self.f.seek(offset, whence)

    def skip(self, n: int) -> None:
        self.f.seek(n, 1)

    def read_bytes(self, n: int) -> bytes:
        return self.f.read(n)

    def _fmt(self, char: str) -> str:
        prefix = "<" if self.little_endian else ">"
        return f"{prefix}{char}"

    def read_uint8(self) -> int:
        return struct.unpack("B", self.f.read(1))[0]

    def read_int8(self) -> int:
        return struct.unpack("b", self.f.read(1))[0]

    def read_uint16(self) -> int:
        return struct.unpack(self._fmt("H"), self.f.read(2))[0]

    def read_int16(self) -> int:
        return struct.unpack(self._fmt("h"), self.f.read(2))[0]

    def read_uint32(self) -> int:
        return struct.unpack(self._fmt("I"), self.f.read(4))[0]

    def read_int32(self) -> int:
        return struct.unpack(self._fmt("i"), self.f.read(4))[0]

    def read_fourcc(self) -> str:
        """Read a 4-byte FourCC string, flipped if little-endian."""
        raw = self.f.read(4)
        if self.little_endian:
            return raw[::-1].decode("latin-1")
        return raw.decode("latin-1")

    def read_fourcc_raw(self) -> str:
        """Read a 4-byte FourCC without endian flip."""
        return self.f.read(4).decode("latin-1")

    def read_len_string(self) -> str:
        """Read a Pascal-style length-prefixed string."""
        length = self.read_uint8()
        if length == 0:
            # Consume padding byte for XFIR (little-endian) Director files
            if self.little_endian:
                self.skip(1)
            return ""
        text = self.f.read(length).decode("latin-1")
        if self.little_endian:
            self.skip(1)  # padding
        return text

    def read_string(self, length: int) -> str:
        return self.f.read(length).decode("latin-1")


# ---------------------------------------------------------------------------
# Director file parser
# ---------------------------------------------------------------------------


class DirectorFile:
    """Parses a Director 5/6 .DXR / .CXT / .CST / .DIR file."""

    def __init__(self, path: str | Path):
        self.path = Path(path)
        self.basename = self.path.name

        # Header
        self.header: str = ""
        self.little_endian: bool = False
        self.file_size: int = 0
        self.signature: str = ""

        # Version info
        self.version: int = 0
        self.shockwave_version: str = ""
        self.created_by: str = ""
        self.modified_by: str = ""
        self.file_path: str = ""

        # Movie config
        self.movie_width: int = 0
        self.movie_height: int = 0

        # Internal structures
        self.entries: list[FileEntry] = []
        self.cast_libraries: list[CastLibrary] = []
        self.text_contents: dict[str, dict[int, str]] = {}
        self.key_table: list[KeyEntry] = []
        self.name_table: list[str] = []
        self.script_contexts: list = []

        # Parsed additional chunk data
        self.font_map: Any = None  # VWFM / Fmap → FontMap
        self.tempo_data: list = []  # VWtk → [TempoEntry]
        self.score_frame_refs: list = []  # SCRF → [ScoreFrameRef]
        self.thumbnails: list = []  # THUM → [Thumbnail]
        self.cast_info: Any = None  # Cinf → CastInfo
        self.sort_order: list[int] = []  # Sord → [int]
        self.xtra_list: list = []  # XTRl → [XtraEntry]
        self.publ_data: bytes | None = None  # PUBL (raw)
        self.grid_data: bytes | None = None  # GRID (raw)
        self.fcol_data: bytes | None = None  # FCOL (raw)
        self.external_casts: dict = {}  # loaded external DirectorFile instances
        self.script_cast_map: dict[int, tuple[str, str]] = {}  # LctX id → (name, type)

        # Unparsed chunk data (for Score, Lingo, etc.)
        self._raw_chunks: dict[int, bytes] = {}

        # Cached file handle for repeated reads
        self._cached_fh: BinaryIO | None = None

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()

    def close(self) -> None:
        """Close the cached file handle if open."""
        if self._cached_fh is not None:
            self._cached_fh.close()
            self._cached_fh = None

    def _get_fh(self):
        """Return a cached file handle for read operations."""
        if self._cached_fh is None:
            self._cached_fh = open(self.path, "rb")
        return self._cached_fh

    def parse(self) -> None:
        """Parse the entire Director file."""
        with open(self.path, "rb") as f:
            self._reader = BinaryReader(f)
            self._parse_header()
            self._parse_imap()
            self._parse_mmap()
            self._parse_metadata()
            self._parse_key_table()
            self._parse_cast_libraries()
            self._parse_lnam()
            self._parse_lctx()

    # -- Header ---------------------------------------------------------------

    def _parse_header(self) -> None:
        r = self._reader
        self.header = r.read_fourcc_raw()
        if self.header not in ("RIFX", "XFIR"):
            raise ValueError(f"Not a Director file: header={self.header!r}")

        self.little_endian = self.header == "XFIR"
        r.little_endian = self.little_endian

        self.file_size = r.read_int32()
        self.signature = r.read_fourcc()

        log.info(
            "Header=%s  LittleEndian=%s  Size=%d  Sign=%s",
            self.header,
            self.little_endian,
            self.file_size,
            self.signature,
        )

    # -- imap -----------------------------------------------------------------

    def _parse_imap(self) -> None:
        r = self._reader
        _imap_tag = r.read_fourcc()
        _imap_len = r.read_int32()
        _unknown = r.read_int32()
        self._mmap_offset = r.read_int32()
        log.debug("imap: mmap_offset=%d", self._mmap_offset)

    # -- mmap (memory map) ---------------------------------------------------

    def _parse_mmap(self) -> None:
        r = self._reader
        r.seek(self._mmap_offset)

        _mmap_tag = r.read_fourcc()
        _mmap_len = r.read_int32()

        self.version = 0xF000 + r.read_int32()
        _something1 = r.read_int32()
        file_num = r.read_int32()
        _something2 = r.read_int32()
        _something3 = r.read_int32()
        _something4 = r.read_int32()

        log.info("Version=0x%X  Entries=%d", self.version, file_num)

        self.entries.clear()
        for i in range(file_num):
            pointer_offset = r.pos
            entry_type = r.read_fourcc()
            entry_length = r.read_int32()
            entry_offset = r.read_int32()
            _unk1 = r.read_int32()
            _unk2 = r.read_int32()

            self.entries.append(
                FileEntry(
                    id=i,
                    type=entry_type,
                    data_length=entry_length,
                    data_offset=entry_offset,
                    pointer_offset=pointer_offset,
                )
            )

    # -- Metadata (DRCF, VWCF, VWFI, MCsL) ----------------------------------

    def _parse_metadata(self) -> None:
        from .fonts import parse_vwfm, parse_fmap
        from .misc_chunks import (
            parse_vwtk,
            parse_scrf,
            parse_thum,
            parse_cinf,
            parse_xtrl,
            parse_sord,
        )

        for i, e in enumerate(self.entries):
            if e.type == "DRCF":
                self._parse_drcf(e)
            elif e.type == "MCsL":
                self._parse_mcsl(e)
            elif e.type == "VWFI":
                self._parse_vwfi(e)
            elif e.type == "VWCF":
                self._parse_vwcf(e)
            elif e.type == "VWFM":
                try:
                    data = self.get_entry_data(e.id)
                    self.font_map = parse_vwfm(data, little_endian=self.little_endian)
                    log.debug("VWFM: %d entries", len(self.font_map.entries))
                except Exception as ex:
                    log.warning("Failed to parse VWFM: %s", ex)
            elif e.type == "Fmap":
                try:
                    data = self.get_entry_data(e.id)
                    fmap = parse_fmap(data, little_endian=self.little_endian)
                    # Merge into font_map (VWFM has priority, Fmap supplements)
                    if self.font_map is None:
                        self.font_map = fmap
                    else:
                        existing_ids = {fe.font_id for fe in self.font_map.entries}
                        for fe in fmap.entries:
                            if fe.font_id not in existing_ids:
                                self.font_map.entries.append(fe)
                    log.debug("Fmap: %d entries", len(fmap.entries))
                except Exception as ex:
                    log.warning("Failed to parse Fmap: %s", ex)
            elif e.type == "VWtk":
                try:
                    data = self.get_entry_data(e.id)
                    self.tempo_data = parse_vwtk(data)
                except Exception as ex:
                    log.warning("Failed to parse VWtk: %s", ex)
            elif e.type == "SCRF":
                try:
                    data = self.get_entry_data(e.id)
                    self.score_frame_refs = parse_scrf(data)
                except Exception as ex:
                    log.warning("Failed to parse SCRF: %s", ex)
            elif e.type == "THUM":
                try:
                    data = self.get_entry_data(e.id)
                    thum = parse_thum(data)
                    if thum:
                        self.thumbnails.append(thum)
                except Exception as ex:
                    log.warning("Failed to parse THUM: %s", ex)
            elif e.type == "Cinf":
                try:
                    data = self.get_entry_data(e.id)
                    self.cast_info = parse_cinf(data)
                except Exception as ex:
                    log.warning("Failed to parse Cinf: %s", ex)
            elif e.type == "Sord":
                try:
                    data = self.get_entry_data(e.id)
                    self.sort_order = parse_sord(data)
                except Exception as ex:
                    log.warning("Failed to parse Sord: %s", ex)
            elif e.type == "XTRl":
                try:
                    data = self.get_entry_data(e.id)
                    self.xtra_list = parse_xtrl(data)
                except Exception as ex:
                    log.warning("Failed to parse XTRl: %s", ex)
            elif e.type == "PUBL":
                try:
                    self.publ_data = self.get_entry_data(e.id)
                    log.debug("PUBL: %d bytes", len(self.publ_data))
                except Exception as ex:
                    log.warning("Failed to read PUBL: %s", ex)
            elif e.type == "GRID":
                try:
                    self.grid_data = self.get_entry_data(e.id)
                    log.debug("GRID: %d bytes", len(self.grid_data))
                except Exception as ex:
                    log.warning("Failed to read GRID: %s", ex)
            elif e.type == "FCOL":
                try:
                    self.fcol_data = self.get_entry_data(e.id)
                    log.debug("FCOL: %d bytes", len(self.fcol_data))
                except Exception as ex:
                    log.warning("Failed to read FCOL: %s", ex)

        # Fallback: standalone file with no MCsL
        if not self.cast_libraries:
            self.cast_libraries.append(CastLibrary(id=0, name="Standalone", member_count=-1))

        log.info(
            "Movie=%dx%d  Version=%s  CastLibs=%d",
            self.movie_width,
            self.movie_height,
            self.shockwave_version,
            len(self.cast_libraries),
        )

    def _parse_drcf(self, entry: FileEntry) -> None:
        r = self._reader
        r.seek(entry.data_offset + 8)  # skip FourCC + length
        r.skip(36)
        version_bytes = r.read_bytes(2)
        version_hex = version_bytes[0:1].hex() + version_bytes[1:2].hex()
        self.shockwave_version = VERSION_TABLE.get(version_hex, f"unknown({version_hex})")

    def _parse_mcsl(self, entry: FileEntry) -> None:
        r = self._reader
        r.seek(entry.data_offset + 8)

        _unk1 = struct.unpack(">I", r.read_bytes(4))[0]
        cast_count = struct.unpack(">I", r.read_bytes(4))[0]
        _unk2 = struct.unpack(">I", r.read_bytes(4))[0]
        array_size = struct.unpack(">I", r.read_bytes(4))[0]

        # Skip offset array
        for _ in range(cast_count):
            r.skip(16)

        _unk3 = struct.unpack(">H", r.read_bytes(2))[0]
        _lib_len = struct.unpack(">I", r.read_bytes(4))[0]

        self.cast_libraries.clear()
        for j in range(cast_count):
            lib = CastLibrary(id=j, name="")

            lib.name = self._read_len_string_be()
            lib.path = self._read_len_string_be()
            if lib.path:
                lib.external = True
                r.skip(2)

            _preload = r.read_uint8()
            _storage = r.read_uint8()
            lib.member_count = struct.unpack(">h", r.read_bytes(2))[0]
            _num_id = struct.unpack(">I", r.read_bytes(4))[0]

            if lib.external:
                r.skip(1)  # external entries have one extra trailing byte

            self.cast_libraries.append(lib)

    def _read_len_string_be(self) -> str:
        """Read a Pascal string (always big-endian for MCsL)."""
        r = self._reader
        length = r.read_uint8()
        if length == 0:
            r.skip(1)  # align to even boundary
            return ""
        text = r.read_string(length)
        # Pad to even boundary: total = 1 (length byte) + length (data)
        if length % 2 == 0:  # total is odd when length is even
            r.skip(1)
        return text

    def _parse_vwfi(self, entry: FileEntry) -> None:
        r = self._reader
        r.seek(entry.data_offset + 8)
        skip_len = struct.unpack(">I", r.read_bytes(4))[0]
        r.skip(skip_len - 4)
        field_num = struct.unpack(">H", r.read_bytes(2))[0]
        r.skip(4)

        offsets = [struct.unpack(">I", r.read_bytes(4))[0] for _ in range(field_num)]
        data_pos = r.pos

        if len(offsets) > 0:
            r.seek(data_pos + offsets[0])
            self.created_by = self._read_len_string_be()
        if len(offsets) > 1:
            r.seek(data_pos + offsets[1])
            self.modified_by = self._read_len_string_be()
        if len(offsets) > 2:
            r.seek(data_pos + offsets[2])
            self.file_path = self._read_len_string_be()

    def _parse_vwcf(self, entry: FileEntry) -> None:
        r = self._reader
        r.seek(entry.data_offset + 8)
        r.skip(8)
        self.movie_height = struct.unpack(">H", r.read_bytes(2))[0]
        self.movie_width = struct.unpack(">H", r.read_bytes(2))[0]

    # -- KEY* table -----------------------------------------------------------

    def _parse_key_table(self) -> None:
        for i, e in enumerate(self.entries):
            if e.type == "KEY*":
                self._parse_keys(e)
                return
        log.warning("No KEY* chunk found")

    def _parse_keys(self, entry: FileEntry) -> None:
        r = self._reader
        r.seek(entry.data_offset + 8)

        _unk1 = r.read_uint16()
        _unk2 = r.read_uint16()
        _unk3 = r.read_uint32()
        entry_num = r.read_uint32()

        self.key_table.clear()
        for _ in range(entry_num):
            cast_file_slot = r.read_uint32()
            cast_slot = r.read_uint32()
            cast_type = r.read_fourcc()

            self.key_table.append(KeyEntry(cast_file_slot, cast_slot, cast_type))

            if cast_slot >= 1024:
                cast_num = cast_slot - 1024
                if cast_type == "CAS*":
                    if cast_num < len(self.cast_libraries):
                        self.cast_libraries[cast_num].lib_slot = cast_file_slot
                else:
                    # Link to cast library or unhandled metadata chunks
                    if cast_slot < len(self.entries):
                        self.entries[cast_slot].linked_entries.append(cast_file_slot)
            else:
                if cast_slot < len(self.entries) and cast_file_slot < len(self.entries):
                    self.entries[cast_slot].linked_entries.append(cast_file_slot)

    # -- Cast libraries & members ---------------------------------------------

    def _parse_cast_libraries(self) -> None:
        for lib in self.cast_libraries:
            if lib.lib_slot < 0:
                continue
            self._parse_cast_lib(lib)

    def _parse_cast_lib(self, lib: CastLibrary) -> None:
        r = self._reader
        entry = self.entries[lib.lib_slot]
        r.seek(entry.data_offset + 8)

        num_slots = entry.data_length // 4
        slot_map: dict[int, int] = {}
        for i in range(num_slots):
            cast_slot = struct.unpack(">I", r.read_bytes(4))[0]
            if cast_slot != 0:
                slot_map[i + 1] = cast_slot

        for num, slot in slot_map.items():
            member = self._parse_cast_member(slot, num, lib.name)
            if member:
                lib.members[num] = member

        log.info("Library '%s': %d members parsed", lib.name, len(lib.members))

    def _parse_cast_member(self, slot: int, num: int, lib_name: str) -> CastMember | None:
        """Parse a single CASt entry and its linked resources."""
        r = self._reader
        entry = self.entries[slot]
        r.seek(entry.data_offset + 8)

        cast_type = struct.unpack(">I", r.read_bytes(4))[0]
        cast_data_len = struct.unpack(">I", r.read_bytes(4))[0]
        cast_end_data_len = struct.unpack(">I", r.read_bytes(4))[0]

        member = CastMember(
            slot=num,
            library=lib_name,
            file_slot=slot,
            cast_type=cast_type,
            data_offset=entry.data_offset,
            data_length=entry.data_length,
            linked_entries=list(entry.linked_entries),
        )

        # Parse common field data
        if cast_data_len > 0:
            field_start = r.pos
            for _ in range(16):
                member.unknown_fields.append(struct.unpack(">H", r.read_bytes(2))[0])

            field_num = struct.unpack(">H", r.read_bytes(2))[0]
            offsets = [struct.unpack(">I", r.read_bytes(4))[0] for _ in range(field_num)]
            _field_data_len = struct.unpack(">I", r.read_bytes(4))[0]
            data_pos = r.pos

            for k in range(field_num):
                r.seek(data_pos + offsets[k])
                str_len = r.read_uint8()
                if r.pos + str_len > entry.data_offset + entry.data_length:
                    break
                member.fields.append(r.read_string(str_len))

            if member.fields:
                member.name = member.fields[0]

            r.seek(field_start + cast_data_len)

        # Parse type-specific end data
        self._parse_cast_end_data(member, cast_type, cast_end_data_len)

        # Parse linked resources
        self._parse_linked_resources(member)

        return member

    def _parse_cast_end_data(self, member: CastMember, cast_type: int, end_len: int) -> None:
        """Parse the type-specific data following the generic field block."""
        r = self._reader

        if end_len <= 0:
            return

        if cast_type == CastType.BITMAP:
            r.skip(2)  # unknown
            member.image_pos_y = struct.unpack(">h", r.read_bytes(2))[0]
            member.image_pos_x = struct.unpack(">h", r.read_bytes(2))[0]
            h_raw = struct.unpack(">h", r.read_bytes(2))[0]
            w_raw = struct.unpack(">h", r.read_bytes(2))[0]
            member.image_height = h_raw - member.image_pos_y
            member.image_width = w_raw - member.image_pos_x
            r.skip(4)  # unknown
            r.skip(4)  # unknown
            reg_y_raw = struct.unpack(">h", r.read_bytes(2))[0]
            reg_x_raw = struct.unpack(">h", r.read_bytes(2))[0]
            member.image_reg_y = reg_y_raw - member.image_pos_y
            member.image_reg_x = reg_x_raw - member.image_pos_x
            # Remaining fields may not exist for 1-bit bitmaps
            try:
                member.image_alpha = r.read_uint8()
                member.image_bit_depth = r.read_uint8()
                r.skip(2)
                member.image_palette = struct.unpack(">h", r.read_bytes(2))[0]
            except struct.error:
                member.image_bit_depth = 1

        elif cast_type == CastType.SOUND:
            if len(member.fields) >= 3:
                member.sound_codec = member.fields[2]
            if len(member.unknown_fields) >= 8:
                member.sound_looped = member.unknown_fields[7] == 0
            # Parse K4 end-data for channels / sample_size fallback
            try:
                end_data = r.read_bytes(min(end_len, 26))
                if len(end_data) >= 6:
                    _snd_type = struct.unpack_from(">H", end_data, 0)[0]
                    # K4 fields: channels at offset 8, sample_size at offset 10
                    if len(end_data) >= 12:
                        k4_channels = struct.unpack_from(">H", end_data, 8)[0]
                        k4_sample_size = struct.unpack_from(">H", end_data, 10)[0]
                        # Only use as fallback if not already set from sndH/snd
                        if member.sound_channels <= 1 and k4_channels > 0:
                            member.sound_channels = k4_channels
                        if member.sound_sample_size <= 8 and k4_sample_size > 0:
                            member.sound_sample_size = k4_sample_size
            except (struct.error, Exception):
                pass

        elif cast_type == CastType.PALETTE:
            # Parse palette metadata from end-data (name/type info)
            try:
                end_data = r.read_bytes(min(end_len, 8))
                # Palette end-data is minimal; CLUT comes from linked entries
                log.debug("Palette end-data: %d bytes", len(end_data))
            except (struct.error, Exception):
                pass

        elif cast_type == CastType.SCRIPT:
            pass  # Script bytecode is in linked Lscr chunk

        elif cast_type == CastType.FILMLOOP:
            from .filmloop import parse_film_loop

            try:
                end_data = r.read_bytes(end_len)
                member.film_loop_data = parse_film_loop(end_data)
                log.debug(
                    "FilmLoop '%s': %d frames",
                    member.name,
                    member.film_loop_data.total_frames,
                )
            except Exception as ex:
                log.warning("Failed to parse FilmLoop end-data for '%s': %s", member.name, ex)

        elif cast_type == CastType.BUTTON:
            from .cast_types import parse_button

            try:
                end_data = r.read_bytes(end_len)
                member.button_data = parse_button(end_data)
            except Exception as ex:
                log.warning("Failed to parse Button end-data for '%s': %s", member.name, ex)

        elif cast_type == CastType.SHAPE:
            from .cast_types import parse_shape

            try:
                end_data = r.read_bytes(end_len)
                member.shape_data = parse_shape(end_data)
            except Exception as ex:
                log.warning("Failed to parse Shape end-data for '%s': %s", member.name, ex)

        elif cast_type == CastType.DIGITAL_VIDEO:
            from .cast_types import parse_digital_video

            try:
                end_data = r.read_bytes(end_len)
                member.digital_video_data = parse_digital_video(end_data)
            except Exception as ex:
                log.warning("Failed to parse DigitalVideo end-data for '%s': %s", member.name, ex)

        elif cast_type == CastType.TRANSITION:
            from .cast_types import parse_transition

            try:
                end_data = r.read_bytes(end_len)
                member.transition_data = parse_transition(end_data)
            except Exception as ex:
                log.warning("Failed to parse Transition end-data for '%s': %s", member.name, ex)

        elif cast_type == CastType.PICTURE:
            from .cast_types import parse_picture

            try:
                end_data = r.read_bytes(end_len)
                member.picture_data = parse_picture(end_data)
            except Exception as ex:
                log.warning("Failed to parse Picture end-data for '%s': %s", member.name, ex)

        elif cast_type in (CastType.FIELD, CastType.TEXT):
            pass  # Text content is in linked STXT chunk

        elif cast_type == CastType.MOVIE:
            pass  # Movie reference — no specific end-data parser yet

        elif cast_type == CastType.OLE:
            pass  # OLE object — no specific end-data parser yet

        else:
            if cast_type != CastType.NULL:
                log.debug("Unhandled cast type %d end-data (%d bytes)", cast_type, end_len)

    def _parse_linked_resources(self, member: CastMember) -> None:
        """Parse linked resource entries (sndH, snd , cupt, CLUT)."""
        for linked_slot in member.linked_entries:
            if linked_slot >= len(self.entries):
                continue
            linked_entry = self.entries[linked_slot]

            if linked_entry.type == "sndH":
                self._parse_sndh(member, linked_entry)
            elif linked_entry.type == "snd ":
                self._parse_snd(member, linked_entry)
            elif linked_entry.type == "cupt":
                self._parse_cupt(member, linked_entry)
            elif linked_entry.type == "CLUT":
                self._parse_clut(member, linked_entry)

    def _parse_sndh(self, member: CastMember, entry: FileEntry) -> None:
        r = self._reader
        r.seek(entry.data_offset + 8)
        r.skip(4)
        member.sound_length = struct.unpack(">I", r.read_bytes(4))[0]
        r.skip(4)
        member.sound_channels = struct.unpack(">H", r.read_bytes(2))[0]
        r.skip(18 + 4 + 4 + 4)
        member.sound_sample_rate = struct.unpack(">I", r.read_bytes(4))[0]

    def _parse_snd(self, member: CastMember, entry: FileEntry) -> None:
        if entry.data_length == 0:
            return
        r = self._reader
        r.seek(entry.data_offset + 8)
        format_number = struct.unpack(">H", r.read_bytes(2))[0]
        offset = entry.data_offset + 8
        if format_number == 2:
            offset += 4
        r.seek(offset)
        has_sound_cmd = struct.unpack(">H", r.read_bytes(2))[0]
        if has_sound_cmd != 1:
            return
        r.skip(2)  # sound command
        _buffer_cmd = struct.unpack(">H", r.read_bytes(2))[0]
        sound_header_offset = struct.unpack(">I", r.read_bytes(4))[0]
        r.seek(entry.data_offset + 8 + sound_header_offset)
        member.sound_sample_rate = struct.unpack(">H", r.read_bytes(2))[0]
        r.skip(6)
        member.sound_data_length = struct.unpack(">I", r.read_bytes(4))[0]
        r.skip(28)
        member.sound_sample_size = struct.unpack(">H", r.read_bytes(2))[0]

    def _parse_cupt(self, member: CastMember, entry: FileEntry) -> None:
        r = self._reader
        r.seek(entry.data_offset + 8)
        count = struct.unpack(">I", r.read_bytes(4))[0]
        member.sound_cue_points = []
        for _ in range(count):
            _something = struct.unpack(">h", r.read_bytes(2))[0]
            sample_offset = struct.unpack(">h", r.read_bytes(2))[0]
            text_len = r.read_uint8()
            cue_name = r.read_string(text_len) if text_len > 0 else ""
            r.skip(max(0, 31 - text_len))  # padding to 32 bytes
            member.sound_cue_points.append((sample_offset, cue_name))

    def _parse_clut(self, member: CastMember, entry: FileEntry) -> None:
        r = self._reader
        r.seek(entry.data_offset + 8)
        num = entry.data_length // 6
        palette: list[tuple[int, int, int]] = []
        for _ in range(num):
            r1 = r.read_uint8()
            _r2 = r.read_uint8()
            g1 = r.read_uint8()
            _g2 = r.read_uint8()
            b1 = r.read_uint8()
            _b2 = r.read_uint8()
            palette.append((r1, g1, b1))
        member.palette_data = palette

    # -- Lnam / LctX (Lingo name table & script context) ---------------------

    def _parse_lnam(self) -> None:
        """Find and parse the Lnam (name table) chunk."""
        from ..lingo.bytecode import parse_lnam

        for entry in self.entries:
            if entry.type == "Lnam":
                try:
                    data = self.get_entry_data(entry.id)
                    names = parse_lnam(data)
                    # Validate parsed names
                    valid = []
                    for i, name in enumerate(names):
                        if len(name) > 256 or any(ord(c) == 0 for c in name):
                            log.warning(
                                "Lnam entry %d suspicious: len=%d, skipping",
                                i,
                                len(name),
                            )
                            valid.append(f"name_{i}")
                        else:
                            valid.append(name)
                    self.name_table = valid
                    if not self.name_table:
                        log.warning(
                            "Lnam parsing returned 0 names "
                            "\u2014 decompiler will use fallback names (name_0, name_1, ...)"
                        )
                    else:
                        log.debug("Lnam: %d names", len(self.name_table))
                except Exception as e:
                    log.warning(
                        "Failed to parse Lnam: %s \u2014 decompiler will use fallback names",
                        e,
                    )
                break

    def _parse_lctx(self) -> None:
        """Find and parse the LctX (script context) chunk."""
        from ..lingo.bytecode import parse_lctx

        for entry in self.entries:
            if entry.type == "LctX":
                try:
                    data = self.get_entry_data(entry.id)
                    self.script_contexts = parse_lctx(data)
                    log.debug("LctX: %d entries", len(self.script_contexts))
                except Exception as e:
                    log.debug("Failed to parse LctX: %s", e)
                break

        # Build the script→cast mapping for decompiler integration
        self._build_script_cast_map()

    def _build_script_cast_map(self) -> None:
        """Build a mapping from LctX script entries to cast member info.

        Maps ``script_context.cast_id`` → (member_name, script_type_str)
        so the decompiler can annotate scripts with their cast member
        name and type (movie / score / parent).
        """
        SCRIPT_TYPE_NAMES = {1: "movie", 3: "score", 7: "parent"}
        self.script_cast_map: dict[int, tuple[str, str]] = {}

        # Build a flat cast_id → name lookup
        member_lookup: dict[int, str] = {}
        for lib in self.cast_libraries:
            for num, m in lib.members.items():
                member_lookup[num] = m.name or f"cast_{num}"

        for ctx in self.script_contexts:
            cast_id = ctx.cast_id
            type_str = SCRIPT_TYPE_NAMES.get(ctx.type, f"type_{ctx.type}")
            member_name = member_lookup.get(cast_id, f"cast_{cast_id}")
            self.script_cast_map[ctx.id] = (member_name, type_str)

        if self.script_cast_map:
            log.debug("Script→Cast map: %d entries", len(self.script_cast_map))

    # -- Public access API ----------------------------------------------------

    def resolve_font(self, font_id: int) -> str | None:
        """Resolve a font ID to a font name via VWFM/Fmap."""
        if self.font_map is not None:
            return self.font_map.get_name(font_id)
        return None

    def get_member(self, lib_name: str, num: int) -> CastMember | None:
        """Look up a cast member by library name and slot number."""
        for lib in self.cast_libraries:
            if lib.name == lib_name:
                return lib.members.get(num)
        return None

    def get_member_by_name(self, name: str) -> CastMember | None:
        """Find first member matching the given name."""
        for lib in self.cast_libraries:
            for m in lib.members.values():
                if m.name == name:
                    return m
        return None

    def all_members(self) -> list[CastMember]:
        """Return a flat list of all cast members."""
        result = []
        for lib in self.cast_libraries:
            result.extend(lib.members.values())
        return result

    def get_entry_data(self, slot: int) -> bytes:
        """Read raw data bytes for a file entry (skipping FourCC + length)."""
        entry = self.entries[slot]
        f = self._get_fh()
        f.seek(entry.data_offset + 8)
        return f.read(entry.data_length)

    def get_raw_chunk(self, slot: int) -> tuple[str, bytes]:
        """Read FourCC type and raw data for a file entry."""
        entry = self.entries[slot]
        f = self._get_fh()
        f.seek(entry.data_offset)
        fourcc = f.read(4).decode("latin-1")
        _length = struct.unpack(">I", f.read(4))[0]
        data = f.read(entry.data_length)
        return (fourcc, data)

    def find_entries_by_type(self, type_str: str) -> list[FileEntry]:
        """Find all file entries matching a given FourCC type."""
        return [e for e in self.entries if e.type == type_str]

    def summary(self) -> dict[str, Any]:
        """Return a summary dict suitable for JSON export."""
        libs = []
        for lib in self.cast_libraries:
            members = {}
            for num, m in sorted(lib.members.items()):
                members[num] = {
                    "name": m.name,
                    "type": m.type_name,
                    "cast_type": m.cast_type,
                }
                if m.cast_type == CastType.BITMAP:
                    members[num].update(
                        {
                            "width": m.image_width,
                            "height": m.image_height,
                            "bit_depth": m.image_bit_depth,
                            "reg_x": m.image_reg_x,
                            "reg_y": m.image_reg_y,
                            "pos_x": m.image_pos_x,
                            "pos_y": m.image_pos_y,
                            "palette": m.image_palette,
                            "alpha": m.image_alpha,
                        }
                    )
                elif m.cast_type == CastType.SOUND:
                    members[num].update(
                        {
                            "sample_rate": m.sound_sample_rate,
                            "sample_size": m.sound_sample_size,
                            "channels": m.sound_channels,
                            "length": m.sound_length,
                            "data_length": m.sound_data_length,
                            "duration_seconds": m.sound_duration_seconds,
                            "looped": m.sound_looped,
                            "codec": m.sound_codec,
                            "cue_points": m.sound_cue_points,
                        }
                    )
                elif m.cast_type == CastType.SHAPE and m.shape_data:
                    members[num]["shape_type"] = m.shape_data.shape_type
                    members[num]["line_size"] = m.shape_data.line_size
                elif m.cast_type == CastType.BUTTON and m.button_data:
                    members[num]["button_type"] = m.button_data.button_type
                    members[num]["label"] = m.button_data.label
                elif m.cast_type == CastType.TRANSITION and m.transition_data:
                    members[num]["transition_type"] = m.transition_data.transition_type
                    members[num]["transition_name"] = m.transition_data.name
                    members[num]["duration_ms"] = m.transition_data.duration
                elif m.cast_type == CastType.DIGITAL_VIDEO and m.digital_video_data:
                    members[num]["video_type"] = m.digital_video_data.video_type
                    members[num]["filename"] = m.digital_video_data.filename
                    members[num]["frame_rate"] = m.digital_video_data.frame_rate
                elif m.cast_type == CastType.FILMLOOP and m.film_loop_data:
                    members[num]["total_frames"] = m.film_loop_data.total_frames
                    members[num]["loop"] = m.film_loop_data.loop
                elif m.cast_type == CastType.PICTURE and m.picture_data:
                    members[num]["width"] = m.picture_data.width
                    members[num]["height"] = m.picture_data.height
            libs.append(
                {
                    "name": lib.name,
                    "member_count": lib.member_count,
                    "members": members,
                }
            )

        return {
            "file": str(self.path),
            "header": self.header,
            "version": f"0x{self.version:X}",
            "little_endian": self.little_endian,
            "shockwave_version": self.shockwave_version,
            "dimensions": f"{self.movie_width}x{self.movie_height}",
            "created_by": self.created_by,
            "modified_by": self.modified_by,
            "entries": len(self.entries),
            "name_table_size": len(self.name_table),
            "key_table_size": len(self.key_table),
            "libraries": libs,
            "font_map_entries": len(self.font_map.entries) if self.font_map else 0,
            "tempo_entries": len(self.tempo_data),
            "score_frame_refs": len(self.score_frame_refs),
            "thumbnails": len(self.thumbnails),
            "sort_order_entries": len(self.sort_order),
            "xtra_list": [{"name": x.name, "version": x.version} for x in self.xtra_list],
            "cast_info": (
                {"name": self.cast_info.name, "path": self.cast_info.file_path}
                if self.cast_info
                else None
            ),
            "script_contexts": len(self.script_contexts),
        }
