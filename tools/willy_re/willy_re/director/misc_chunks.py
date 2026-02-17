"""Miscellaneous Director chunk parsers.

Handles lesser-used chunk types:
  - VWtk: Score tempo / timing data
  - SCRF: Score frame references
  - THUM: Thumbnail image data
  - Cinf: Cast info / metadata
  - XTRl: Xtra (plugin) list
  - Sord: Sort order for cast members
"""

from __future__ import annotations

import logging
import struct
from dataclasses import dataclass, field

log = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# VWtk — Score tempo / timing entries
# ---------------------------------------------------------------------------


@dataclass
class TempoEntry:
    """A tempo (timing) entry from the VWtk chunk."""

    frame: int = 0
    tempo: int = 0  # frames per second (0 = use default)
    wait_type: int = 0  # 0=none, 1=wait, 2=waitForCuePoint, 3=waitForMouse, 4=waitForKey
    wait_time: int = 0  # seconds to wait (for wait_type=1)
    channel: int = 0
    cue_point: int = 0


def parse_vwtk(data: bytes) -> list[TempoEntry]:
    """Parse a VWtk (score tempo) chunk.

    Returns a list of tempo entries that control frame playback speed
    and wait conditions.
    """
    entries: list[TempoEntry] = []

    if len(data) < 4:
        return entries

    # Header: entry count or total size
    offset = 0
    count = struct.unpack_from(">I", data, offset)[0]
    offset += 4

    # Each entry is typically 6-8 bytes
    entry_size = 6
    if count > 0 and (len(data) - 4) // count >= 8:
        entry_size = 8

    for i in range(count):
        if offset + entry_size > len(data):
            break
        entry = TempoEntry(frame=i)
        entry.tempo = struct.unpack_from(">H", data, offset)[0]
        entry.wait_type = data[offset + 2] if offset + 3 <= len(data) else 0
        entry.wait_time = data[offset + 3] if offset + 4 <= len(data) else 0
        entry.channel = (
            struct.unpack_from(">H", data, offset + 4)[0] if offset + 6 <= len(data) else 0
        )
        entries.append(entry)
        offset += entry_size

    log.debug("VWtk: %d tempo entries", len(entries))
    return entries


# ---------------------------------------------------------------------------
# SCRF — Score frame references
# ---------------------------------------------------------------------------


@dataclass
class ScoreFrameRef:
    """A reference entry from the SCRF chunk."""

    frame: int = 0
    cast_id: int = 0
    ref_type: int = 0


def parse_scrf(data: bytes) -> list[ScoreFrameRef]:
    """Parse an SCRF (score frame reference) chunk.

    These map frames to cast members used in scripts.
    """
    entries: list[ScoreFrameRef] = []

    if len(data) < 4:
        return entries

    offset = 0
    count = struct.unpack_from(">I", data, offset)[0]
    offset += 4

    for _ in range(count):
        if offset + 6 > len(data):
            break
        frame = struct.unpack_from(">H", data, offset)[0]
        cast_id = struct.unpack_from(">H", data, offset + 2)[0]
        ref_type = struct.unpack_from(">H", data, offset + 4)[0]
        entries.append(ScoreFrameRef(frame=frame, cast_id=cast_id, ref_type=ref_type))
        offset += 6

    log.debug("SCRF: %d frame references", len(entries))
    return entries


# ---------------------------------------------------------------------------
# THUM — Thumbnail
# ---------------------------------------------------------------------------


@dataclass
class Thumbnail:
    """Parsed thumbnail data."""

    width: int = 0
    height: int = 0
    bit_depth: int = 0
    pixel_data: bytes = b""


def parse_thum(data: bytes) -> Thumbnail | None:
    """Parse a THUM (thumbnail) chunk.

    Thumbnails are small preview images stored for cast members.
    """
    if len(data) < 8:
        return None

    thum = Thumbnail()
    thum.width = struct.unpack_from(">H", data, 0)[0]
    thum.height = struct.unpack_from(">H", data, 2)[0]
    thum.bit_depth = struct.unpack_from(">H", data, 4)[0]
    # bytes 6-7: reserved
    thum.pixel_data = data[8:]

    log.debug("THUM: %dx%d @ %d-bit", thum.width, thum.height, thum.bit_depth)
    return thum


# ---------------------------------------------------------------------------
# Cinf — Cast info / metadata
# ---------------------------------------------------------------------------


@dataclass
class CastInfo:
    """Cast info metadata from a Cinf chunk."""

    script_text: str = ""
    name: str = ""
    file_path: str = ""
    entries: list[tuple[int, str]] = field(default_factory=list)


def parse_cinf(data: bytes) -> CastInfo:
    """Parse a Cinf (cast info) chunk.

    Contains metadata about the cast file: paths, names, etc.
    """
    info = CastInfo()

    if len(data) < 4:
        return info

    offset = 0
    # Cinf typically contains a series of length-prefixed strings
    strings: list[str] = []
    while offset + 2 <= len(data):
        str_len = struct.unpack_from(">H", data, offset)[0]
        offset += 2
        if str_len == 0 or offset + str_len > len(data):
            break
        s = data[offset : offset + str_len].decode("latin-1", errors="replace")
        strings.append(s)
        offset += str_len
        # Align to even
        if offset % 2:
            offset += 1

    if len(strings) >= 1:
        info.name = strings[0]
    if len(strings) >= 2:
        info.file_path = strings[1]

    for i, s in enumerate(strings):
        info.entries.append((i, s))

    log.debug("Cinf: %d string entries", len(strings))
    return info


# ---------------------------------------------------------------------------
# XTRl — Xtra (plugin) list
# ---------------------------------------------------------------------------


@dataclass
class XtraEntry:
    """An entry in the Xtra list."""

    name: str = ""
    version: int = 0
    flags: int = 0


def parse_xtrl(data: bytes) -> list[XtraEntry]:
    """Parse an XTRl (Xtra list) chunk.

    Lists the Xtras (plugins) required by the Director file.
    """
    entries: list[XtraEntry] = []

    if len(data) < 4:
        return entries

    offset = 0
    count = struct.unpack_from(">I", data, offset)[0]
    offset += 4

    # Sanity
    if count > 1000:
        log.warning("XTRl count %d too large", count)
        return entries

    for _ in range(count):
        if offset + 8 > len(data):
            break
        # Each entry: 4 bytes flags/version, then length-prefixed name
        version = struct.unpack_from(">I", data, offset)[0]
        offset += 4
        flags = struct.unpack_from(">I", data, offset)[0]
        offset += 4

        if offset >= len(data):
            break
        name_len = data[offset]
        offset += 1
        if offset + name_len > len(data):
            break
        name = data[offset : offset + name_len].decode("latin-1", errors="replace")
        offset += name_len

        # Align to even
        if offset % 2:
            offset += 1

        entries.append(XtraEntry(name=name, version=version, flags=flags))

    log.debug("XTRl: %d xtra entries", len(entries))
    return entries


# ---------------------------------------------------------------------------
# Sord — Sort order
# ---------------------------------------------------------------------------


def parse_sord(data: bytes) -> list[int]:
    """Parse a Sord (sort order) chunk.

    Returns a list of cast member IDs in display sort order.
    """
    if len(data) < 8:
        return []

    offset = 0
    _header = struct.unpack_from(">I", data, offset)[0]
    offset += 4
    count = struct.unpack_from(">I", data, offset)[0]
    offset += 4

    if count > 100000:
        log.warning("Sord count %d too large", count)
        return []

    order: list[int] = []
    for _ in range(count):
        if offset + 2 > len(data):
            break
        member_id = struct.unpack_from(">H", data, offset)[0]
        order.append(member_id)
        offset += 2

    log.debug("Sord: %d entries in sort order", len(order))
    return order
