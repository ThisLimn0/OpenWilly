"""BITD (bitmap data) decoder for Director 5/6.

Supports 1-bit, 8-bit (paletted), 16-bit, and 32-bit bitmaps.
8-bit uses PackBits RLE compression; 1-bit is raw bit-packed.
"""

from __future__ import annotations

import logging
import struct
from typing import BinaryIO

from PIL import Image

log = logging.getLogger(__name__)


def decode_bitd(
    f: BinaryIO,
    offset: int,
    length: int,
    width: int,
    height: int,
    bit_depth: int,
) -> list[list]:
    """Decode a BITD chunk into a 2D pixel array.

    For 32-bit images, each pixel is [A, R, G, B].
    For 8-bit paletted, each pixel is a palette index (0-255).
    For 1-bit, each pixel is 0 or 1.

    Parameters
    ----------
    f : BinaryIO
        File object positioned at start of the BITD chunk data (after FourCC + length).
    offset : int
        Absolute file offset of the BITD entry (before FourCC).
    length : int
        Length of the BITD data.
    width, height : int
        Image dimensions from the CASt member.
    bit_depth : int
        Bit depth: 1, 8, 16, or 32.
    """
    if width <= 0 or height <= 0:
        return []

    f.seek(offset + 8)  # skip FourCC + length

    if bit_depth == 32:
        return _decode_32bit(f, offset, length, width, height)
    elif bit_depth >= 33:
        # 1-bit stored as bit_depth > 32 in Director quirk
        return _decode_1bit(f, offset, length, width, height)
    elif bit_depth == 16:
        return _decode_16bit(f, offset, length, width, height)
    elif bit_depth == 4:
        return _decode_4bit(f, offset, length, width, height)
    elif bit_depth == 2:
        return _decode_2bit(f, offset, length, width, height)
    else:
        return _decode_paletted(f, offset, length, width, height)


def _decode_2bit(f: BinaryIO, offset: int, length: int, width: int, height: int) -> list[list[int]]:
    """Decode 2-bit paletted image: 4 pixels per byte, MSB first."""
    pixels = [[0] * width for _ in range(height)]
    x, y = 0, 0

    end = offset + length + 8
    while f.tell() < end and y < height:
        byte = f.read(1)
        if not byte:
            break
        val = byte[0]
        for shift in (6, 4, 2, 0):
            if x < width:
                pixels[y][x] = 0x03 - ((val >> shift) & 0x03)
            x += 1
            if x >= width:
                x = 0
                y += 1
                if y >= height:
                    return pixels
    return pixels


def _decode_4bit(f: BinaryIO, offset: int, length: int, width: int, height: int) -> list[list[int]]:
    """Decode 4-bit paletted image: 2 pixels per byte, high nibble first."""
    pixels = [[0] * width for _ in range(height)]
    x, y = 0, 0

    end = offset + length + 8
    while f.tell() < end and y < height:
        byte = f.read(1)
        if not byte:
            break
        val = byte[0]
        for shift in (4, 0):
            if x < width:
                pixels[y][x] = 0x0F - ((val >> shift) & 0x0F)
            x += 1
            if x >= width:
                x = 0
                y += 1
                if y >= height:
                    return pixels
    return pixels


def _decode_1bit(f: BinaryIO, offset: int, length: int, width: int, height: int) -> list[list[int]]:
    """Decode 1-bit image: 8 pixels per byte, MSB first."""
    pixels = [[0] * width for _ in range(height)]
    x, y = 0, 0

    while f.tell() < offset + length + 8 and y < height:
        byte = f.read(1)
        if not byte:
            break
        val = byte[0]
        for bit in range(7, -1, -1):
            if x < width:
                pixels[y][x] = 1 - ((val >> bit) & 1)
            x += 1
            if x >= width:
                x = 0
                y += 1
                if y >= height:
                    return pixels
    return pixels


def _decode_16bit(
    f: BinaryIO, offset: int, length: int, width: int, height: int
) -> list[list[list[int]]]:
    """Decode 16-bit RGB555 image (1 or 0 padding bits, 5 bits per channel).

    Director 16-bit BMPs use RGB555 (or occasionally RGB565).
    Returns pixels as [R, G, B] lists.
    """
    row_bytes = width * 2
    # Rows are padded to even byte boundaries (already even for 16-bit)
    if row_bytes % 2:
        row_bytes += 1

    pixels = [[[0, 0, 0] for _ in range(width)] for _ in range(height)]

    # Check if data is RLE-compressed or raw
    raw_size = row_bytes * height
    if raw_size <= length:
        # Raw / uncompressed
        for y in range(height):
            for x in range(width):
                word_bytes = f.read(2)
                if len(word_bytes) < 2:
                    return pixels
                word = struct.unpack_from(">H", word_bytes)[0]
                r = ((word >> 10) & 0x1F) << 3
                g = ((word >> 5) & 0x1F) << 3
                b = (word & 0x1F) << 3
                pixels[y][x] = [r, g, b]
            # Skip row padding
            pad_bytes = row_bytes - width * 2
            if pad_bytes > 0:
                f.read(pad_bytes)
    else:
        # PackBits RLE — decode into raw 16-bit pixel buffer
        end = offset + length + 8
        x, y = 0, 0
        while f.tell() < end and y < height:
            raw = f.read(1)
            if not raw:
                break
            n = raw[0]

            if n == 0x80:
                continue  # PackBits no-op
            elif n > 0x80:
                # RLE: repeat next 2 bytes (0x101 - n) times
                run_len = 0x101 - n
                word_bytes = f.read(2)
                if len(word_bytes) < 2:
                    break
                word = struct.unpack_from(">H", word_bytes)[0]
                r = ((word >> 10) & 0x1F) << 3
                g = ((word >> 5) & 0x1F) << 3
                b = (word & 0x1F) << 3
                for _ in range(run_len):
                    if y >= height:
                        return pixels
                    pixels[y][x] = [r, g, b]
                    x += 1
                    if x >= width:
                        x = 0
                        y += 1
            else:
                # Literal: copy (n+1) 16-bit pixels
                copy_len = n + 1
                for _ in range(copy_len):
                    word_bytes = f.read(2)
                    if len(word_bytes) < 2:
                        break
                    if y >= height:
                        return pixels
                    word = struct.unpack_from(">H", word_bytes)[0]
                    r = ((word >> 10) & 0x1F) << 3
                    g = ((word >> 5) & 0x1F) << 3
                    b = (word & 0x1F) << 3
                    pixels[y][x] = [r, g, b]
                    x += 1
                    if x >= width:
                        x = 0
                        y += 1

    return pixels


def _decode_paletted(
    f: BinaryIO, offset: int, length: int, width: int, height: int
) -> list[list[int]]:
    """Decode 8-bit paletted image (PackBits RLE or raw)."""
    pixels = [[0] * width for _ in range(height)]
    x, y = 0, 0

    # Determine whether data is raw (uncompressed) or RLE
    pad = height if width % 2 else 0
    raw_size = width * height + pad

    if raw_size == length:
        # Direct / uncompressed mode
        return _decode_raw_paletted(f, offset, length, width, height, pad > 0)

    # RLE mode (PackBits)
    # Row stride includes a padding byte for odd widths
    row_stride = width + (width % 2)
    end = offset + length + 8
    while f.tell() < end and y < height:
        raw = f.read(1)
        if not raw:
            break
        n = raw[0]

        if n == 0x80:
            # PackBits no-op byte — skip
            continue
        elif n > 0x7F:
            # Run-length: repeat next byte (0x101 - n) times
            run_len = 0x101 - n
            val_byte = f.read(1)
            if not val_byte:
                break
            val = 0xFF - val_byte[0]
            for _ in range(run_len):
                if y >= height:
                    return pixels
                if x < width:
                    pixels[y][x] = val
                x += 1
                if x >= row_stride:
                    x = 0
                    y += 1
        else:
            # Literal: copy next (n + 1) bytes
            copy_len = n + 1
            for _ in range(copy_len):
                val_byte = f.read(1)
                if not val_byte:
                    break
                if y >= height:
                    return pixels
                if x < width:
                    pixels[y][x] = 0xFF - val_byte[0]
                x += 1
                if x >= row_stride:
                    x = 0
                    y += 1

    return pixels


def _decode_raw_paletted(
    f: BinaryIO, offset: int, length: int, width: int, height: int, has_pad: bool
) -> list[list[int]]:
    """Decode raw (uncompressed) 8-bit paletted image."""
    pixels = [[0] * width for _ in range(height)]
    for y in range(height):
        for x in range(width):
            b = f.read(1)
            if not b:
                return pixels
            pixels[y][x] = 0xFF - b[0]
        if has_pad:
            f.read(1)  # skip padding byte
    return pixels


def _decode_32bit(
    f: BinaryIO, offset: int, length: int, width: int, height: int
) -> list[list[list[int]]]:
    """Decode 32-bit ARGB image (PackBits RLE, channel-planar per row)."""
    pixels = [[[0, 0, 0, 255] for _ in range(width)] for _ in range(height)]
    x, y = 0, 0
    channel = 0  # 0=A, 1=R, 2=G, 3=B

    end = offset + length + 8
    while f.tell() < end and y < height:
        raw = f.read(1)
        if not raw:
            break
        n = raw[0]

        if n > 0x7F:
            run_len = 0x101 - n
            val_byte = f.read(1)
            if not val_byte:
                break
            val = val_byte[0]
            for _ in range(run_len):
                if y >= height:
                    return pixels
                pixels[y][x][channel] = val
                x += 1
                if x >= width:
                    channel = (channel + 1) % 4
                    x = 0
                    if channel == 0:
                        y += 1
        else:
            copy_len = n + 1
            for _ in range(copy_len):
                val_byte = f.read(1)
                if not val_byte:
                    break
                if y >= height:
                    return pixels
                pixels[y][x][channel] = val_byte[0]
                x += 1
                if x >= width:
                    channel = (channel + 1) % 4
                    x = 0
                    if channel == 0:
                        y += 1

    return pixels


# ---------------------------------------------------------------------------
# High-level: BITD → PIL Image
# ---------------------------------------------------------------------------


def bitd_to_image(
    f: BinaryIO,
    offset: int,
    length: int,
    width: int,
    height: int,
    bit_depth: int,
    palette: list[tuple[int, int, int]] | None = None,
    palette_id: int = 0,
    transparent_white: bool = True,
    is_windows: bool = False,
) -> Image.Image | None:
    """Decode BITD to a PIL Image.

    Parameters
    ----------
    palette : optional list of (R,G,B) tuples for 8-bit images.
    palette_id : built-in palette selector (negative = system palette).
    transparent_white : if True, white pixels become transparent (for sprite compositing).
    is_windows : if True, use Windows system palette as default instead of Mac.
    """
    if width <= 0 or height <= 0:
        return None

    pixels = decode_bitd(f, offset, length, width, height, bit_depth)
    if not pixels:
        return None

    if bit_depth == 32:
        img = Image.new("RGBA", (width, height))
        for y in range(height):
            for x in range(width):
                p = pixels[y][x]
                img.putpixel((x, y), (p[1], p[2], p[3], 255 - p[0]))
        return img

    elif bit_depth >= 33:
        # 1-bit
        img = Image.new("1", (width, height))
        for y in range(height):
            for x in range(width):
                img.putpixel((x, y), pixels[y][x])
        return img

    elif bit_depth == 16:
        # 16-bit RGB555 → RGB image
        img = Image.new("RGB", (width, height))
        for y in range(height):
            for x in range(width):
                p = pixels[y][x]
                img.putpixel((x, y), (p[0], p[1], p[2]))
        return img

    else:
        # 8-bit paletted
        img = Image.new("P", (width, height))
        flat_palette = _build_flat_palette(palette, palette_id, is_windows=is_windows)
        if flat_palette:
            img.putpalette(flat_palette)

        for y in range(height):
            for x in range(width):
                img.putpixel((x, y), pixels[y][x])

        if transparent_white:
            return img.convert("RGBA")
        return img


def _build_flat_palette(
    palette: list[tuple[int, int, int]] | None,
    palette_id: int,
    *,
    is_windows: bool = False,
) -> list[int] | None:
    """Build a flat [R,G,B,R,G,B,...] palette list for PIL."""
    if palette:
        flat: list[int] = []
        for r, g, b in palette:
            flat.extend([r, g, b])
        # Pad to 256 entries
        while len(flat) < 768:
            flat.extend([0, 0, 0])
        return flat

    # Use system palette — pick Windows default for XFIR files
    from .palette import get_system_palette

    effective_id = palette_id
    if effective_id == 0 and is_windows:
        effective_id = -100  # Windows system palette

    sys_pal = get_system_palette(effective_id)
    if sys_pal:
        flat = []
        for r, g, b in sys_pal:
            flat.extend([r, g, b])
        while len(flat) < 768:
            flat.extend([0, 0, 0])
        return flat

    return None
