"""VWLB (frame labels) parser for Director 5/6.

Frame labels associate names with Score frame numbers, used by Lingo
scripts for navigation (e.g., `go to frame "start"`).
"""

from __future__ import annotations

import logging
import struct
from dataclasses import dataclass

log = logging.getLogger(__name__)


@dataclass
class FrameLabel:
    """A named label on a Score frame."""

    frame: int
    name: str


def parse_vwlb(data: bytes) -> list[FrameLabel]:
    """Parse a VWLB chunk into a list of FrameLabels.

    Parameters
    ----------
    data : bytes
        Raw bytes of the VWLB chunk (after FourCC + length header).
    """
    if len(data) < 4:
        return []

    labels: list[FrameLabel] = []
    offset = 0

    # Number of labels
    count = struct.unpack_from(">H", data, offset)[0]
    offset += 2

    # Read label entries: each is (frame_num: u16, name_offset: u16)
    entries: list[tuple[int, int]] = []
    for _ in range(count):
        if offset + 4 > len(data):
            break
        frame_num = struct.unpack_from(">H", data, offset)[0]
        name_offset = struct.unpack_from(">H", data, offset + 2)[0]
        entries.append((frame_num, name_offset))
        offset += 4

    # String table follows the entries
    string_base = offset
    for frame_num, name_offset in entries:
        str_pos = string_base + name_offset
        if str_pos >= len(data):
            continue
        # Read Pascal string: length byte + chars
        str_len = data[str_pos]
        if str_pos + 1 + str_len > len(data):
            continue
        name = data[str_pos + 1 : str_pos + 1 + str_len].decode("latin-1")
        labels.append(FrameLabel(frame=frame_num, name=name))

    log.info("Parsed %d frame labels", len(labels))
    return labels
