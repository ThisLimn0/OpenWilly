"""Shape, Button, and Transition cast member support.

Director shapes (CastType=8) are vector graphics: rectangles, rounded
rectangles, ovals, and lines. They're drawn procedurally rather than
from bitmap data.

Buttons (CastType=7) are similar to shapes but carry click/press state.

Transitions (CastType=14) define cross-frame visual effects like wipes,
dissolves, and fades used during Score playback.

Digital Video (CastType=10) members reference external AVI/QuickTime
files or embedded video data.

Picture (CastType=5) members contain PICT-format images (Mac) or
metafile data.
"""

from __future__ import annotations

import logging
import struct
from dataclasses import dataclass, field
from enum import IntEnum

log = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# Shapes
# ---------------------------------------------------------------------------


class ShapeType(IntEnum):
    """Director shape sub-types."""

    RECTANGLE = 1
    ROUND_RECT = 2
    OVAL = 3
    LINE = 4


SHAPE_TYPE_NAMES: dict[int, str] = {
    1: "Rectangle",
    2: "Rounded Rectangle",
    3: "Oval",
    4: "Line",
}


@dataclass
class ShapeInfo:
    """Parsed shape cast member data."""

    shape_type: int = ShapeType.RECTANGLE
    line_size: int = 1
    pattern: int = 0  # fill pattern index
    fore_color: int = 0
    back_color: int = 255
    filled: bool = True


def parse_shape(data: bytes) -> ShapeInfo:
    """Parse a shape cast member's type-specific data.

    Parameters
    ----------
    data : bytes
        The shape-specific portion of the CASt data.
    """
    shape = ShapeInfo()

    if len(data) < 4:
        return shape

    shape.shape_type = struct.unpack_from(">H", data, 0)[0]
    if len(data) >= 6:
        shape.line_size = struct.unpack_from(">H", data, 2)[0]
    if len(data) >= 8:
        shape.pattern = struct.unpack_from(">H", data, 4)[0]
    if len(data) >= 10:
        shape.fore_color = data[6]
        shape.back_color = data[7]
    if len(data) >= 12:
        shape.filled = bool(struct.unpack_from(">H", data, 8)[0])

    log.debug(
        "Shape: type=%s lineSize=%d", SHAPE_TYPE_NAMES.get(shape.shape_type, "?"), shape.line_size
    )
    return shape


# ---------------------------------------------------------------------------
# Buttons
# ---------------------------------------------------------------------------


class ButtonType(IntEnum):
    PUSH = 1
    CHECK_BOX = 2
    RADIO = 3


@dataclass
class ButtonInfo:
    """Parsed button cast member data."""

    button_type: int = ButtonType.PUSH
    label: str = ""


def parse_button(data: bytes) -> ButtonInfo:
    """Parse a button cast member's type-specific data."""
    btn = ButtonInfo()

    if len(data) < 2:
        return btn

    btn.button_type = struct.unpack_from(">H", data, 0)[0]

    # Label follows as a length-prefixed string
    offset = 2
    if offset < len(data):
        label_len = data[offset]
        offset += 1
        if offset + label_len <= len(data):
            btn.label = data[offset : offset + label_len].decode("latin-1", errors="replace")

    return btn


# ---------------------------------------------------------------------------
# Transitions
# ---------------------------------------------------------------------------


class TransitionType(IntEnum):
    """Common Director transition types."""

    NONE = 0
    WIPE_RIGHT = 1
    WIPE_LEFT = 2
    WIPE_DOWN = 3
    WIPE_UP = 4
    CENTER_OUT_HORIZ = 5
    EDGES_IN_HORIZ = 6
    CENTER_OUT_VERT = 7
    EDGES_IN_VERT = 8
    CENTER_OUT_SQUARE = 9
    EDGES_IN_SQUARE = 10
    PUSH_LEFT = 11
    PUSH_RIGHT = 12
    PUSH_DOWN = 13
    PUSH_UP = 14
    REVEAL_UP = 15
    REVEAL_UP_RIGHT = 16
    REVEAL_RIGHT = 17
    REVEAL_DOWN_RIGHT = 18
    REVEAL_DOWN = 19
    REVEAL_DOWN_LEFT = 20
    REVEAL_LEFT = 21
    REVEAL_UP_LEFT = 22
    DISSOLVE_PIXELS_FAST = 23
    DISSOLVE_BOXY_RECTS = 24
    DISSOLVE_BOXY_SQUARES = 25
    DISSOLVE_PATTERNS = 26
    RANDOM_ROWS = 27
    RANDOM_COLUMNS = 28
    COVER_DOWN = 29
    COVER_DOWN_LEFT = 30
    COVER_DOWN_RIGHT = 31
    COVER_LEFT = 32
    COVER_RIGHT = 33
    COVER_UP = 34
    COVER_UP_LEFT = 35
    COVER_UP_RIGHT = 36
    VENETIAN_BLINDS = 37
    CHECKERBOARD = 38
    STRIPS_BOTTOM_LEFT = 39
    STRIPS_BOTTOM_RIGHT = 40
    STRIPS_TOP_LEFT = 41
    STRIPS_TOP_RIGHT = 42
    DISSOLVE_BITS_FAST = 50
    DISSOLVE_PIXELS = 51
    DISSOLVE_BITS = 52


TRANSITION_NAMES: dict[int, str] = {
    0: "None",
    1: "Wipe Right",
    2: "Wipe Left",
    3: "Wipe Down",
    4: "Wipe Up",
    5: "Center Out, Horizontal",
    6: "Edges In, Horizontal",
    7: "Center Out, Vertical",
    8: "Edges In, Vertical",
    9: "Center Out, Square",
    10: "Edges In, Square",
    11: "Push Left",
    12: "Push Right",
    13: "Push Down",
    14: "Push Up",
    23: "Dissolve, Pixels Fast",
    24: "Dissolve, Boxy Rectangles",
    25: "Dissolve, Boxy Squares",
    26: "Dissolve, Patterns",
    50: "Dissolve, Bits Fast",
    51: "Dissolve, Pixels",
    52: "Dissolve, Bits",
}


@dataclass
class TransitionInfo:
    """Parsed transition cast member data."""

    transition_type: int = 0
    duration: int = 250  # milliseconds
    chunk_size: int = 1  # pixels per step
    smoothness: int = 0
    area: int = 0  # 0=whole stage, 1=changing area only

    @property
    def name(self) -> str:
        return TRANSITION_NAMES.get(self.transition_type, f"Custom({self.transition_type})")


def parse_transition(data: bytes) -> TransitionInfo:
    """Parse a transition cast member's type-specific data."""
    tr = TransitionInfo()

    if len(data) < 4:
        return tr

    offset = 0
    tr.chunk_size = struct.unpack_from(">H", data, offset)[0]
    offset += 2
    tr.transition_type = struct.unpack_from(">H", data, offset)[0]
    offset += 2

    if len(data) >= 6:
        tr.duration = struct.unpack_from(">H", data, offset)[0]
        offset += 2

    if len(data) >= 8:
        tr.area = data[offset]
        offset += 1
        tr.smoothness = data[offset] if offset < len(data) else 0

    log.debug("Transition: %s (%dms, chunk=%d)", tr.name, tr.duration, tr.chunk_size)
    return tr


# ---------------------------------------------------------------------------
# Digital Video
# ---------------------------------------------------------------------------


@dataclass
class DigitalVideoInfo:
    """Parsed digital video cast member data."""

    video_type: str = ""  # "quicktime", "avi", "unknown"
    filename: str = ""
    frame_rate: int = 0
    direct_to_stage: bool = False
    loop: bool = False
    paused: bool = False
    controller: bool = True
    crop: bool = False


def parse_digital_video(data: bytes) -> DigitalVideoInfo:
    """Parse a digital video cast member's type-specific data."""
    dv = DigitalVideoInfo()

    if len(data) < 8:
        return dv

    offset = 0
    flags = struct.unpack_from(">I", data, offset)[0]
    offset += 4

    dv.direct_to_stage = bool(flags & 0x01)
    dv.loop = bool(flags & 0x02)
    dv.paused = bool(flags & 0x04)
    dv.controller = bool(flags & 0x08)
    dv.crop = bool(flags & 0x10)

    # Video type
    video_type_id = struct.unpack_from(">H", data, offset)[0]
    offset += 2
    dv.video_type = {0: "quicktime", 1: "avi"}.get(video_type_id, "unknown")

    dv.frame_rate = struct.unpack_from(">H", data, offset)[0]
    offset += 2

    # Filename (if embedded)
    if offset < len(data):
        name_len = data[offset]
        offset += 1
        if offset + name_len <= len(data):
            dv.filename = data[offset : offset + name_len].decode("latin-1", errors="replace")

    log.debug("DigitalVideo: type=%s fps=%d file=%s", dv.video_type, dv.frame_rate, dv.filename)
    return dv


# ---------------------------------------------------------------------------
# Picture (PICT)
# ---------------------------------------------------------------------------


@dataclass
class PictureInfo:
    """Parsed picture cast member metadata."""

    width: int = 0
    height: int = 0
    bit_depth: int = 0
    has_pict_data: bool = False


def parse_picture(data: bytes) -> PictureInfo:
    """Parse a picture cast member's type-specific data."""
    pic = PictureInfo()

    if len(data) < 8:
        return pic

    # Picture members store a PICT bounding rect followed by the PICT data
    # Bounding rect: top, left, bottom, right (2 bytes each)
    top = struct.unpack_from(">h", data, 0)[0]
    left = struct.unpack_from(">h", data, 2)[0]
    bottom = struct.unpack_from(">h", data, 4)[0]
    right = struct.unpack_from(">h", data, 6)[0]

    pic.width = right - left
    pic.height = bottom - top
    pic.has_pict_data = len(data) > 8

    log.debug("Picture: %dx%d, has_data=%s", pic.width, pic.height, pic.has_pict_data)
    return pic
