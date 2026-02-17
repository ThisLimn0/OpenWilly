"""VWSC (Score) parser for Director 5/6.

The Score contains per-frame channel data describing which cast members
appear, their positions, inks, blend levels, and script triggers.

Format reference: ScummVM Director engine readChannelD6() (GPLv2, used only
as format documentation — no code copied).

Director 6 Score layout:
  - Header: total frames, channels per frame, channel size
  - Per-frame data: array of channels, each with fixed-size fields
  - Channel 0: frame tempo/timing data
  - Channel 1: palette info
  - Channels 2+: sprite channels

Each sprite channel (Dir 6, 24 bytes):
  Offset  Size  Field
  0       2     Script cast member ID
  2       1     Sprite type / ink + flags
  3       1     Fore color
  4       2     Cast member ID
  6       2     startY (top)
  8       2     startX (left)
  10      2     height
  12      2     width
  14      1     Script ID?
  15      1     Blend amount
  16      2     Back color / line size
  18      2     endY (bottom delta?)
  20      2     endX (right delta?)
  22      2     (unused / flags)
"""

from __future__ import annotations

import logging
import struct
from dataclasses import dataclass, field
from typing import BinaryIO

log = logging.getLogger(__name__)

# Director 6 channel size
D6_CHANNEL_SIZE = 24


@dataclass
class SpriteChannel:
    """A single channel's data for one Score frame."""

    channel_id: int = 0
    script_id: int = 0
    sprite_type: int = 0
    ink: int = 0
    fore_color: int = 0
    cast_id: int = 0
    start_x: int = 0
    start_y: int = 0
    width: int = 0
    height: int = 0
    blend: int = 0
    back_color: int = 0
    end_x: int = 0
    end_y: int = 0

    @property
    def is_empty(self) -> bool:
        return self.cast_id == 0 and self.sprite_type == 0


@dataclass
class ScoreFrame:
    """One frame of the Score timeline."""

    frame_num: int
    tempo: int = 0
    palette_id: int = 0
    transition_id: int = 0
    sound1_id: int = 0
    sound2_id: int = 0
    script_id: int = 0
    sprites: list[SpriteChannel] = field(default_factory=list)


@dataclass
class Score:
    """Complete parsed Score (VWSC) data."""

    total_frames: int = 0
    channels_per_frame: int = 0
    channel_size: int = D6_CHANNEL_SIZE
    frames: list[ScoreFrame] = field(default_factory=list)


def parse_vwsc(data: bytes, version: int = 0xF4C7) -> Score:
    """Parse a VWSC chunk into a Score object.

    Parameters
    ----------
    data : bytes
        Raw bytes of the VWSC chunk (after FourCC + length header).
    version : int
        Director version code (default: Director 6 = 0xF4C7).
    """
    if len(data) < 12:
        log.warning("VWSC chunk too small: %d bytes", len(data))
        return Score()

    score = Score()

    # Header
    offset = 0
    score.total_frames = struct.unpack_from(">I", data, offset)[0]
    offset += 4

    # Sanity check — protect against corrupt data
    MAX_FRAMES = 100_000
    if score.total_frames > MAX_FRAMES:
        log.warning(
            "Score total_frames %d exceeds max, clamping to %d",
            score.total_frames,
            MAX_FRAMES,
        )
        score.total_frames = MAX_FRAMES
    score.channels_per_frame = struct.unpack_from(">H", data, offset)[0]
    offset += 2
    score.channel_size = struct.unpack_from(">H", data, offset)[0]
    offset += 2

    # Some versions have extra header bytes
    # Skip the rest of a 20-byte header if present
    if len(data) > 20 and score.channel_size == D6_CHANNEL_SIZE:
        header_size = struct.unpack_from(">I", data, offset)[0]
        offset += 4
        if header_size > 12:
            offset = header_size

    log.info(
        "Score: %d frames, %d channels, channel_size=%d",
        score.total_frames,
        score.channels_per_frame,
        score.channel_size,
    )

    # Parse frames
    ch_size = score.channel_size if score.channel_size > 0 else D6_CHANNEL_SIZE
    frame_size = score.channels_per_frame * ch_size

    for frame_idx in range(score.total_frames):
        if offset + frame_size > len(data):
            log.warning("Score data truncated at frame %d", frame_idx)
            break

        frame = ScoreFrame(frame_num=frame_idx + 1)

        # Channel 0: tempo/timing
        if score.channels_per_frame > 0:
            ch0_data = data[offset : offset + ch_size]
            if len(ch0_data) >= 6:
                frame.tempo = struct.unpack_from(">H", ch0_data, 0)[0]
                frame.transition_id = struct.unpack_from(">H", ch0_data, 2)[0]
                frame.script_id = struct.unpack_from(">H", ch0_data, 4)[0]

        # Channel 1: palette/sound
        if score.channels_per_frame > 1:
            ch1_off = offset + ch_size
            ch1_data = data[ch1_off : ch1_off + ch_size]
            if len(ch1_data) >= 6:
                frame.palette_id = struct.unpack_from(">H", ch1_data, 0)[0]
                frame.sound1_id = struct.unpack_from(">H", ch1_data, 2)[0]
                frame.sound2_id = struct.unpack_from(">H", ch1_data, 4)[0]

        # Sprite channels (2+)
        for ch_idx in range(2, score.channels_per_frame):
            ch_off = offset + ch_idx * ch_size
            ch_data = data[ch_off : ch_off + ch_size]
            sprite = _parse_sprite_channel_d6(ch_data, ch_idx)
            if not sprite.is_empty:
                frame.sprites.append(sprite)

        score.frames.append(frame)
        offset += frame_size

    return score


def _parse_sprite_channel_d6(data: bytes, channel_id: int) -> SpriteChannel:
    """Parse a single Director 6 sprite channel (24 bytes)."""
    sprite = SpriteChannel(channel_id=channel_id)
    if len(data) < D6_CHANNEL_SIZE:
        return sprite

    sprite.script_id = struct.unpack_from(">H", data, 0)[0]
    type_ink = data[2]
    sprite.sprite_type = (type_ink >> 4) & 0x0F
    sprite.ink = type_ink & 0x0F
    sprite.fore_color = data[3]
    sprite.cast_id = struct.unpack_from(">H", data, 4)[0]
    sprite.start_y = struct.unpack_from(">h", data, 6)[0]
    sprite.start_x = struct.unpack_from(">h", data, 8)[0]
    sprite.height = struct.unpack_from(">H", data, 10)[0]
    sprite.width = struct.unpack_from(">H", data, 12)[0]
    sprite.blend = data[15]
    sprite.back_color = struct.unpack_from(">H", data, 16)[0]
    sprite.end_y = struct.unpack_from(">h", data, 18)[0]
    sprite.end_x = struct.unpack_from(">h", data, 20)[0]

    return sprite
