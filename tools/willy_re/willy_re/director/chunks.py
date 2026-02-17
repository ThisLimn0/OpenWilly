"""Chunk type and cast type definitions for Director 5/6 files."""

from __future__ import annotations

from enum import Enum, IntEnum


class ChunkType(str, Enum):
    """Known Director chunk FourCC types."""

    # Container
    RIFX = "RIFX"
    XFIR = "XFIR"

    # Memory map
    IMAP = "imap"
    MMAP = "mmap"

    # Movie metadata
    DRCF = "DRCF"  # Director config (version info)
    VWCF = "VWCF"  # Movie config (dimensions, bg color)
    VWFI = "VWFI"  # Movie file info (creator, paths)
    VWFM = "VWFM"  # Movie font map

    # Cast
    MCsL = "MCsL"  # Cast library list
    CASs = "CAS*"  # Cast member index (cast → slot mapping)
    CASt = "CASt"  # Cast member data
    KEYs = "KEY*"  # Key table (links cast members to resources)
    Sord = "Sord"  # Sort order

    # Media resources
    BITD = "BITD"  # Bitmap data
    CLUT = "CLUT"  # Color look-up table (palette)
    STXT = "STXT"  # Styled text
    sndS = "sndS"  # Sound data (raw PCM)
    sndH = "sndH"  # Sound header
    snd_ = "snd "  # Mac sound resource
    cupt = "cupt"  # Cue points

    # Score / Timeline
    VWSC = "VWSC"  # Score data
    VWLB = "VWLB"  # Frame labels
    VWtk = "VWtk"  # Score tempo/wait

    # Lingo
    Lscr = "Lscr"  # Lingo script bytecode
    LctX = "LctX"  # Lingo script context
    Lnam = "Lnam"  # Lingo name table

    # Other
    SCRF = "SCRF"  # Score frame reference?
    THUM = "THUM"  # Thumbnail
    FXmp = "FXmp"  # Font map xtra
    Cinf = "Cinf"  # Cast info
    Fmap = "Fmap"  # Font map
    XTRl = "XTRl"  # Xtra list
    PUBL = "PUBL"  # Publisher settings
    GRID = "GRID"  # Stage grid settings
    FCOL = "FCOL"  # Favourite colours

    FREE = "free"  # Free/deleted slot


class CastType(IntEnum):
    """Director cast member types."""

    NULL = 0
    BITMAP = 1
    FILMLOOP = 2
    FIELD = 3
    PALETTE = 4
    PICTURE = 5
    SOUND = 6
    BUTTON = 7
    SHAPE = 8
    MOVIE = 9
    DIGITAL_VIDEO = 10
    SCRIPT = 11
    TEXT = 12
    OLE = 13
    TRANSITION = 14


# Cast type names for display
CAST_TYPE_NAMES: dict[int, str] = {
    0: "Null",
    1: "Bitmap",
    2: "Film Loop",
    3: "Field",
    4: "Palette",
    5: "Picture",
    6: "Sound",
    7: "Button",
    8: "Shape",
    9: "Movie",
    10: "Digital Video",
    11: "Script",
    12: "Text",
    13: "OLE",
    14: "Transition",
}


# Director version lookup
VERSION_TABLE: dict[str, str] = {
    "0304": "3.0",
    "0404": "4.0",
    "040c": "4.0.4",
    "0452": "5.0",
    "04c7": "6.0",
    "057e": "7.0",
    "0640": "8.0",
    "073a": "8.5/9.0",
    "0744": "10.1",
    "0782": "11.5.0r593",
    "0783": "11.5.8.612",
    "079f": "12",
}
