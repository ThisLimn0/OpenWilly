"""Font map parsers for Director files.

Handles VWFM (movie font map) and Fmap (font mapping) chunks.
These map platform-specific font IDs to font names and attributes.

VWFM layout (Director 6):
  - 2 bytes: entry count
  - Per entry:
    - 2 bytes: font ID
    - 1 byte:  font name length
    - N bytes: font name (Latin-1)
    - padding to even boundary

Fmap layout:
  - Variable-length entries mapping Director's internal font IDs
    to platform font names for cross-platform compatibility.
"""

from __future__ import annotations

import logging
import struct
from dataclasses import dataclass, field

log = logging.getLogger(__name__)


@dataclass
class FontMapEntry:
    """A single entry in a font map."""

    font_id: int = 0
    font_name: str = ""
    font_style: int = 0  # 0=normal, 1=bold, 2=italic, etc.
    font_platform: str = ""  # "mac" or "win"


@dataclass
class FontMap:
    """Parsed font map data."""

    entries: list[FontMapEntry] = field(default_factory=list)

    def get_name(self, font_id: int) -> str | None:
        """Look up a font name by ID."""
        for entry in self.entries:
            if entry.font_id == font_id:
                return entry.font_name
        return None


def parse_vwfm(data: bytes, *, little_endian: bool = False) -> FontMap:
    """Parse a VWFM (movie font map) chunk.

    Parameters
    ----------
    data : bytes
        Raw chunk payload (after FourCC + length header).
    little_endian : bool
        True for XFIR (little-endian) Director files.
    """
    font_map = FontMap()
    bo = "<" if little_endian else ">"

    if len(data) < 2:
        return font_map

    count = struct.unpack_from(f"{bo}H", data, 0)[0]
    offset = 2

    for _ in range(count):
        if offset + 3 > len(data):
            break

        font_id = struct.unpack_from(f"{bo}H", data, offset)[0]
        offset += 2

        name_len = data[offset]
        offset += 1

        if offset + name_len > len(data):
            break

        font_name = data[offset : offset + name_len].decode("latin-1", errors="replace")
        offset += name_len

        # Align to even boundary
        if offset % 2:
            offset += 1

        font_map.entries.append(FontMapEntry(font_id=font_id, font_name=font_name))

    log.debug("VWFM: %d font entries", len(font_map.entries))
    return font_map


def parse_fmap(data: bytes, *, little_endian: bool = False) -> FontMap:
    """Parse an Fmap (font mapping) chunk.

    The Fmap chunk maps fonts between platforms.  Fmap data is always stored
    in big-endian byte order, even inside XFIR (little-endian) containers.

    Format:
      - 4 bytes: header/version
      - 4 bytes: entry count
      - Per entry:
        - 2 bytes: original font ID
        - 1 byte: original name length
        - N bytes: original font name
        - 1 byte: mapped name length
        - N bytes: mapped font name
        - 2 bytes: style/flags
    """
    font_map = FontMap()
    # Fmap is always big-endian regardless of container endianness
    bo = ">"

    if len(data) < 8:
        return font_map

    _version = struct.unpack_from(f"{bo}I", data, 0)[0]
    count = struct.unpack_from(f"{bo}I", data, 4)[0]
    offset = 8

    # Sanity check
    if count > 10000:
        log.warning("Fmap entry count %d seems too large", count)
        return font_map

    for _ in range(count):
        if offset + 3 > len(data):
            break

        font_id = struct.unpack_from(f"{bo}H", data, offset)[0]
        offset += 2

        # Original font name
        if offset >= len(data):
            break
        name_len = data[offset]
        offset += 1
        if offset + name_len > len(data):
            break
        orig_name = data[offset : offset + name_len].decode("latin-1", errors="replace")
        offset += name_len

        # Mapped font name
        if offset >= len(data):
            break
        mapped_len = data[offset]
        offset += 1
        if offset + mapped_len > len(data):
            break
        mapped_name = data[offset : offset + mapped_len].decode("latin-1", errors="replace")
        offset += mapped_len

        # Style flags
        style = 0
        if offset + 2 <= len(data):
            style = struct.unpack_from(f"{bo}H", data, offset)[0]
            offset += 2

        font_map.entries.append(
            FontMapEntry(
                font_id=font_id,
                font_name=orig_name if orig_name else mapped_name,
                font_style=style,
            )
        )

    log.debug("Fmap: %d font mapping entries", len(font_map.entries))
    return font_map
