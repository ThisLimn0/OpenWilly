"""Macromedia Director 5/6 file format parser."""

from .chunks import ChunkType, CastType
from .parser import DirectorFile
from .text import parse_stxt, StyledText
from .fonts import parse_vwfm, parse_fmap, FontMap, FontMapEntry
from .filmloop import parse_film_loop, FilmLoop
from .ink import InkType, INK_NAMES, composite_sprite
from .cast_types import (
    parse_shape,
    parse_button,
    parse_transition,
    parse_digital_video,
    parse_picture,
    ShapeInfo,
    ButtonInfo,
    TransitionInfo,
    DigitalVideoInfo,
    PictureInfo,
    ShapeType,
    TransitionType,
    TRANSITION_NAMES,
)
from .external_casts import find_external_casts, load_external_casts
from .misc_chunks import (
    parse_vwtk,
    parse_scrf,
    parse_thum,
    parse_cinf,
    parse_xtrl,
    parse_sord,
    TempoEntry,
    ScoreFrameRef,
    Thumbnail,
    CastInfo,
    XtraEntry,
)

__all__ = [
    "DirectorFile",
    "ChunkType",
    "CastType",
    "parse_stxt",
    "StyledText",
    "parse_vwfm",
    "parse_fmap",
    "FontMap",
    "FontMapEntry",
    "parse_film_loop",
    "FilmLoop",
    "InkType",
    "INK_NAMES",
    "composite_sprite",
    "parse_shape",
    "parse_button",
    "parse_transition",
    "parse_digital_video",
    "parse_picture",
    "ShapeInfo",
    "ButtonInfo",
    "TransitionInfo",
    "DigitalVideoInfo",
    "PictureInfo",
    "ShapeType",
    "TransitionType",
    "TRANSITION_NAMES",
    "find_external_casts",
    "load_external_casts",
    "parse_vwtk",
    "parse_scrf",
    "parse_thum",
    "parse_cinf",
    "parse_xtrl",
    "parse_sord",
    "TempoEntry",
    "ScoreFrameRef",
    "Thumbnail",
    "CastInfo",
    "XtraEntry",
]
