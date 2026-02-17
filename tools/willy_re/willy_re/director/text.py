"""STXT (Styled Text) chunk parser for Director files.

The STXT chunk stores text content plus run-based style information
(font, size, color, etc.).  The text is always encoded as Mac Roman /
Latin-1 in Director 5-7 files.

Layout
------
  Offset  Size  Description
  0       4     Header length (big-endian uint32, typically 12)
  4       4     Text length in bytes
  8       4     Padding / reserved (always 0)
  12      N     Text bytes (Latin-1)
  12+N    4     Style run count (big-endian uint32) -- optional tail
  16+N    ...   Style runs (each 20 bytes)

Each style run (20 bytes):
  Offset  Size  Description
  0       4     Start offset in text
  4       2     Font cast-member index
  6       1     Style flags (bold / italic / underline / …)
  7       1     (reserved)
  8       2     Font size (points)
  10      6     Foreground colour (RGB, 2 bytes each)
  16      4     (reserved / extra)
"""

from __future__ import annotations

import struct
from dataclasses import dataclass, field


@dataclass
class StyleRun:
    """A single style run inside an STXT chunk."""

    start_offset: int = 0
    font_id: int = 0
    style_flags: int = 0
    font_size: int = 12
    color_r: int = 0
    color_g: int = 0
    color_b: int = 0


@dataclass
class StyledText:
    """Parsed STXT result: text string plus optional style runs."""

    text: str = ""
    styles: list[StyleRun] = field(default_factory=list)


def parse_stxt(data: bytes) -> StyledText:
    """Parse an STXT chunk and return *StyledText*.

    Parameters
    ----------
    data:
        Raw chunk payload (after the outer RIFX envelope).

    Returns
    -------
    StyledText
        The decoded text and (if present) accompanying style runs.
    """
    if len(data) < 12:
        return StyledText()

    header_len = struct.unpack_from(">I", data, 0)[0]
    text_len = struct.unpack_from(">I", data, 4)[0]
    # bytes 8..11 are padding / reserved

    text_start = header_len if header_len >= 12 else 12
    text_end = text_start + text_len
    text = data[text_start:text_end].decode("latin-1", errors="replace")

    styles: list[StyleRun] = []
    style_offset = text_end

    # Style table is optional — only present if there is room.
    if style_offset + 4 <= len(data):
        run_count = struct.unpack_from(">I", data, style_offset)[0]
        style_offset += 4

        for _ in range(run_count):
            if style_offset + 20 > len(data):
                break
            (
                start,
                font_id,
                flags,
                _rsv,
                fsize,
                cr,
                cg,
                cb,
            ) = struct.unpack_from(">IHBBHHHH", data, style_offset)
            # last 4 bytes (reserved / extra) are consumed by fsize+HHh already
            styles.append(
                StyleRun(
                    start_offset=start,
                    font_id=font_id,
                    style_flags=flags,
                    font_size=fsize,
                    color_r=cr,
                    color_g=cg,
                    color_b=cb,
                )
            )
            style_offset += 20

    return StyledText(text=text, styles=styles)
