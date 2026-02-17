"""Film Loop support for Director files.

A Film Loop is a cast member (CastType=2) that references a sequence of
Score frames, replaying them as an animated sprite. The data for a Film
Loop is stored as a mini-Score within the cast member's data.

This module extracts the frame list so that the RE framework can
reconstruct the original animation sequences.
"""

from __future__ import annotations

import logging
import struct
from dataclasses import dataclass, field

log = logging.getLogger(__name__)


@dataclass
class FilmLoopFrame:
    """A single frame in a Film Loop."""

    frame_num: int = 0
    cast_ids: list[int] = field(default_factory=list)
    duration: int = 1  # frames to hold

    @property
    def is_empty(self) -> bool:
        return not self.cast_ids


@dataclass
class FilmLoop:
    """Parsed Film Loop data."""

    total_frames: int = 0
    loop: bool = True
    frames: list[FilmLoopFrame] = field(default_factory=list)

    @property
    def cast_member_ids(self) -> set[int]:
        """All unique cast member IDs referenced by this film loop."""
        ids: set[int] = set()
        for frame in self.frames:
            ids.update(frame.cast_ids)
        return ids


def parse_film_loop(data: bytes) -> FilmLoop:
    """Parse Film Loop cast member data.

    Film Loops store a mini-Score structure similar to VWSC but simpler.
    The exact format varies by Director version; this handles the Dir 6
    layout.

    Parameters
    ----------
    data : bytes
        Raw cast member data (the Film Loop-specific portion after
        the common CASt header).
    """
    fl = FilmLoop()

    if len(data) < 8:
        return fl

    offset = 0

    # Header
    fl.total_frames = struct.unpack_from(">H", data, offset)[0]
    offset += 2

    channels = struct.unpack_from(">H", data, offset)[0]
    offset += 2

    _flags = struct.unpack_from(">H", data, offset)[0]
    offset += 2
    fl.loop = bool(_flags & 0x01)

    _reserved = struct.unpack_from(">H", data, offset)[0]
    offset += 2

    # Each frame: list of (channel, cast_id) pairs
    # Compact format: channels * 2 bytes per frame
    for frame_idx in range(fl.total_frames):
        frame = FilmLoopFrame(frame_num=frame_idx + 1)
        for _ch in range(channels):
            if offset + 2 > len(data):
                break
            cast_id = struct.unpack_from(">H", data, offset)[0]
            offset += 2
            if cast_id > 0:
                frame.cast_ids.append(cast_id)
        fl.frames.append(frame)

    log.debug("FilmLoop: %d frames, %d channels, loop=%s", fl.total_frames, channels, fl.loop)
    return fl
