"""Ink effect compositing for Director sprites.

Director supports multiple ink effects that control how sprites are
composited onto the stage:

  Ink  Value  Description
  ──── ────── ──────────────────────────────────────────────────
  Copy     0  Opaque: source replaces destination
  Transp   1  Transparent: white pixels are transparent
  Reverse  2  XOR with destination
  Ghost    3  Invisible (only outline)
  NotCopy  4  Inverted copy
  NotTrnsp 5  Inverted transparent
  NotRev   6  Inverted reverse
  NotGhost 7  Inverted ghost
  Matte    8  Matte: use alpha/mask for blending
  Mask     9  Mask: use separate mask bitmap
  Blend   32  Alpha blend (uses blend %)
  AddPin  33  Additive clamped to white
  Add     34  Additive blending
  SubPin  35  Subtractive clamped to black
  Sub     36  Subtractive blending
  Darkest 37  Darken: min(src, dst)
  Lightest38  Lighten: max(src, dst)
  BgTrans 39  Background transparent

This module provides compositing functions for each ink type.
All functions operate on Pillow Images (RGBA mode).
"""

from __future__ import annotations

import logging
from enum import IntEnum

from PIL import Image, ImageChops

log = logging.getLogger(__name__)


class InkType(IntEnum):
    COPY = 0
    TRANSPARENT = 1
    REVERSE = 2
    GHOST = 3
    NOT_COPY = 4
    NOT_TRANSPARENT = 5
    NOT_REVERSE = 6
    NOT_GHOST = 7
    MATTE = 8
    MASK = 9
    BLEND = 32
    ADD_PIN = 33
    ADD = 34
    SUB_PIN = 35
    SUB = 36
    DARKEST = 37
    LIGHTEST = 38
    BG_TRANSPARENT = 39


INK_NAMES: dict[int, str] = {
    0: "Copy",
    1: "Transparent",
    2: "Reverse",
    3: "Ghost",
    4: "Not Copy",
    5: "Not Transparent",
    6: "Not Reverse",
    7: "Not Ghost",
    8: "Matte",
    9: "Mask",
    32: "Blend",
    33: "Add Pin",
    34: "Add",
    35: "Sub Pin",
    36: "Sub",
    37: "Darkest",
    38: "Lightest",
    39: "Background Transparent",
}


def composite_sprite(
    stage: Image.Image,
    sprite: Image.Image,
    x: int,
    y: int,
    ink: int = 0,
    blend: int = 255,
    fore_color: tuple[int, int, int] = (0, 0, 0),
    back_color: tuple[int, int, int] = (255, 255, 255),
) -> Image.Image:
    """Composite a sprite onto the stage using the given ink effect.

    Parameters
    ----------
    stage : Image
        The destination stage image (RGBA).
    sprite : Image
        The source sprite image (RGBA or RGB).
    x, y : int
        Position to place the sprite.
    ink : int
        Ink type (see InkType enum).
    blend : int
        Blend amount 0-255 (used by Blend ink).
    fore_color, back_color : tuple
        Foreground and background colors for certain ink types.

    Returns
    -------
    Image
        The modified stage image.
    """
    # Ensure RGBA mode
    if stage.mode != "RGBA":
        stage = stage.convert("RGBA")
    if sprite.mode != "RGBA":
        sprite = sprite.convert("RGBA")

    # Crop sprite to stage bounds
    sw, sh = sprite.size
    stw, sth = stage.size
    if x >= stw or y >= sth or x + sw <= 0 or y + sh <= 0:
        return stage  # completely off-stage

    try:
        if ink == InkType.COPY:
            stage.paste(sprite, (x, y))

        elif ink == InkType.TRANSPARENT:
            # White pixels are transparent
            _composite_transparent(stage, sprite, x, y, back_color)

        elif ink == InkType.MATTE:
            # Use sprite's alpha channel as mask
            stage.paste(sprite, (x, y), sprite)

        elif ink == InkType.BLEND:
            _composite_blend(stage, sprite, x, y, blend)

        elif ink == InkType.ADD or ink == InkType.ADD_PIN:
            _composite_additive(stage, sprite, x, y, clamped=(ink == InkType.ADD_PIN))

        elif ink == InkType.SUB or ink == InkType.SUB_PIN:
            _composite_subtractive(stage, sprite, x, y, clamped=(ink == InkType.SUB_PIN))

        elif ink == InkType.DARKEST:
            _composite_darkest(stage, sprite, x, y)

        elif ink == InkType.LIGHTEST:
            _composite_lightest(stage, sprite, x, y)

        elif ink == InkType.REVERSE:
            _composite_reverse(stage, sprite, x, y)

        elif ink == InkType.NOT_COPY:
            inverted = ImageChops.invert(sprite.convert("RGB")).convert("RGBA")
            stage.paste(inverted, (x, y))

        elif ink == InkType.BG_TRANSPARENT:
            # Background color pixels are transparent
            _composite_transparent(stage, sprite, x, y, back_color)

        elif ink == InkType.GHOST:
            # Ghost: invisible (keep destination)
            pass

        else:
            # Fallback: simple paste with alpha
            stage.paste(sprite, (x, y), sprite)

    except Exception as e:
        log.warning("Ink compositing error (ink=%d): %s", ink, e)
        # Fallback: try simple paste
        try:
            stage.paste(sprite, (x, y), sprite)
        except Exception:
            pass

    return stage


def _composite_transparent(
    stage: Image.Image,
    sprite: Image.Image,
    x: int,
    y: int,
    transparent_color: tuple[int, int, int],
) -> None:
    """Paste sprite, treating pixels matching transparent_color as see-through."""
    sr, sg, sb, sa = sprite.split()
    rgb = sprite.convert("RGB")

    # Create mask: pixels NOT matching the transparent color
    mask = Image.new("L", sprite.size, 255)
    pixels = rgb.load()
    mask_pixels = mask.load()
    if pixels is not None and mask_pixels is not None:
        for py in range(sprite.height):
            for px in range(sprite.width):
                r, g, b = pixels[px, py]
                if (r, g, b) == transparent_color or (
                    abs(r - transparent_color[0])
                    + abs(g - transparent_color[1])
                    + abs(b - transparent_color[2])
                ) < 10:
                    mask_pixels[px, py] = 0

    stage.paste(sprite, (x, y), mask)


def _composite_blend(
    stage: Image.Image,
    sprite: Image.Image,
    x: int,
    y: int,
    blend: int,
) -> None:
    """Alpha blend sprite at given opacity."""
    alpha = blend / 255.0
    overlay = sprite.copy()
    # Adjust alpha channel
    _r, _g, _b, a = overlay.split()
    a = a.point(lambda p: int(p * alpha))
    overlay.putalpha(a)
    stage.paste(overlay, (x, y), overlay)


def _composite_additive(
    stage: Image.Image,
    sprite: Image.Image,
    x: int,
    y: int,
    clamped: bool = False,
) -> None:
    """Additive blending: dst = dst + src."""
    # Extract the region under the sprite
    region = stage.crop((x, y, x + sprite.width, y + sprite.height))
    result = ImageChops.add(region.convert("RGB"), sprite.convert("RGB"))
    result_rgba = result.convert("RGBA")
    stage.paste(result_rgba, (x, y))


def _composite_subtractive(
    stage: Image.Image,
    sprite: Image.Image,
    x: int,
    y: int,
    clamped: bool = False,
) -> None:
    """Subtractive blending: dst = dst - src."""
    region = stage.crop((x, y, x + sprite.width, y + sprite.height))
    result = ImageChops.subtract(region.convert("RGB"), sprite.convert("RGB"))
    result_rgba = result.convert("RGBA")
    stage.paste(result_rgba, (x, y))


def _composite_darkest(
    stage: Image.Image,
    sprite: Image.Image,
    x: int,
    y: int,
) -> None:
    """Darken: dst = min(dst, src)."""
    region = stage.crop((x, y, x + sprite.width, y + sprite.height))
    result = ImageChops.darker(region.convert("RGB"), sprite.convert("RGB"))
    result_rgba = result.convert("RGBA")
    stage.paste(result_rgba, (x, y))


def _composite_lightest(
    stage: Image.Image,
    sprite: Image.Image,
    x: int,
    y: int,
) -> None:
    """Lighten: dst = max(dst, src)."""
    region = stage.crop((x, y, x + sprite.width, y + sprite.height))
    result = ImageChops.lighter(region.convert("RGB"), sprite.convert("RGB"))
    result_rgba = result.convert("RGBA")
    stage.paste(result_rgba, (x, y))


def _composite_reverse(
    stage: Image.Image,
    sprite: Image.Image,
    x: int,
    y: int,
) -> None:
    """Reverse (XOR): dst = dst XOR src."""
    region = stage.crop((x, y, x + sprite.width, y + sprite.height))
    # True per-byte XOR — difference() is |a-b|, not a^b
    r_data = region.convert("RGB").tobytes()
    s_data = sprite.convert("RGB").tobytes()
    xor_data = bytes(a ^ b for a, b in zip(r_data, s_data))
    result = Image.frombytes("RGB", region.convert("RGB").size, xor_data).convert("RGBA")
    stage.paste(result, (x, y))
