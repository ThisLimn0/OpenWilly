"""Sound decoder: sndS (raw PCM) and snd  (Mac sound resource) → WAV.

Also handles sndH (sound header) and cupt (cue points) metadata.
"""

from __future__ import annotations

import io
import logging
import struct
import wave
from pathlib import Path
from typing import BinaryIO

log = logging.getLogger(__name__)


def extract_snds_wav(
    f: BinaryIO,
    offset: int,
    length: int,
    sample_rate: int,
    channels: int = 1,
    sample_width: int = 1,
) -> bytes:
    """Extract raw PCM from sndS chunk and wrap in WAV format.

    Returns WAV file bytes.
    """
    f.seek(offset + 8)  # skip FourCC + length
    raw_data = f.read(length)

    buf = io.BytesIO()
    with wave.open(buf, "wb") as wav:
        wav.setnchannels(channels)
        wav.setsampwidth(sample_width)
        wav.setframerate(sample_rate)
        wav.writeframes(raw_data)

    return buf.getvalue()


def _swap_16bit(data: bytes) -> bytes:
    """Byte-swap 16-bit big-endian PCM samples to little-endian."""
    # Swap pairs of bytes
    ba = bytearray(data)
    for i in range(0, len(ba) - 1, 2):
        ba[i], ba[i + 1] = ba[i + 1], ba[i]
    return bytes(ba)


def _find_snd_data_offset(f: BinaryIO, base: int) -> int:
    """Parse Mac 'snd ' resource header to find the offset of raw sample data.

    Handles Type 1 and Type 2 snd resources with standard, extended (0xFF)
    and compressed (0xFE) sound data headers.
    Returns absolute file offset of the first sample byte.
    """
    f.seek(base)
    snd_type = struct.unpack(">H", f.read(2))[0]

    if snd_type == 1:
        num_data_types = struct.unpack(">H", f.read(2))[0]
        # Each data type entry: 2 bytes type + 4 bytes initOption
        f.seek(6 * num_data_types, 1)
        num_commands = struct.unpack(">H", f.read(2))[0]
    elif snd_type == 2:
        _ref_count = struct.unpack(">H", f.read(2))[0]
        num_commands = struct.unpack(">H", f.read(2))[0]
    else:
        log.warning("Unknown snd type %d, falling back to 78-byte header", snd_type)
        return base + 78

    # Scan commands for bufferCmd (0x8051) or soundCmd (0x8050)
    data_header_offset = None
    for _ in range(num_commands):
        cmd = struct.unpack(">H", f.read(2))[0]
        _param1 = struct.unpack(">H", f.read(2))[0]
        param2 = struct.unpack(">I", f.read(4))[0]
        if cmd in (0x8051, 0x8050):
            data_header_offset = param2

    if data_header_offset is None:
        log.warning("No bufferCmd/soundCmd in snd resource, falling back")
        return base + 78

    # Read sound data header at the indicated offset (relative to resource start)
    header_pos = base + data_header_offset
    f.seek(header_pos)
    _sample_ptr = struct.unpack(">I", f.read(4))[0]
    _field2 = struct.unpack(">I", f.read(4))[0]  # numSamples or numChannels
    _sr_fixed = struct.unpack(">I", f.read(4))[0]
    _loop_start = struct.unpack(">I", f.read(4))[0]
    _loop_end = struct.unpack(">I", f.read(4))[0]
    encode = struct.unpack(">B", f.read(1))[0]
    _base_freq = struct.unpack(">B", f.read(1))[0]

    if encode == 0x00:
        # Standard header (22 bytes total) — data follows immediately
        return f.tell()
    elif encode == 0xFF:
        # Extended header — 22 + 20 additional bytes = 42 total
        f.read(4)  # numFrames
        f.read(10)  # aiffSampleRate (80-bit extended)
        f.read(4)  # markerChunk
        f.read(4)  # instrumentChunks
        f.read(4)  # AESRecording
        f.read(2)  # sampleSize
        f.read(14)  # futureUse
        return f.tell()
    elif encode == 0xFE:
        # Compressed header — 42 + 8 additional bytes
        f.read(4)  # numFrames
        f.read(10)  # aiffSampleRate
        f.read(4)  # markerChunk
        f.read(4)  # instrumentChunks
        f.read(4)  # AESRecording
        f.read(2)  # sampleSize
        f.read(14)  # futureUse
        f.read(4)  # format
        f.read(4)  # reserved
        return f.tell()
    else:
        log.warning("Unknown snd encode type 0x%02X, assuming standard", encode)
        return f.tell()


def extract_snd_wav(
    f: BinaryIO,
    offset: int,
    data_length: int,
    sample_rate: int,
    sample_size: int,
    sound_data_length: int,
    channels: int = 1,
) -> bytes:
    """Extract Mac 'snd ' resource and wrap in WAV.

    Dynamically parses the Mac snd resource header to find sample data.
    Big-endian sample data is byte-swapped for WAV.
    """
    base = offset + 8  # skip chunk header (FourCC + length)
    data_start = _find_snd_data_offset(f, base)

    f.seek(data_start)
    raw_data = f.read(sound_data_length)

    sample_width = max(1, sample_size // 8)

    # Byte-swap 16-bit samples from big-endian to little-endian
    if sample_width == 2:
        raw_data = _swap_16bit(raw_data)

    buf = io.BytesIO()
    with wave.open(buf, "wb") as wav:
        wav.setnchannels(channels)
        wav.setsampwidth(sample_width)
        wav.setframerate(sample_rate)
        wav.writeframes(raw_data)

    return buf.getvalue()


def save_wav(wav_bytes: bytes, path: Path) -> None:
    """Write WAV bytes to a file."""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(wav_bytes)
    log.info("Saved WAV: %s (%d bytes)", path, len(wav_bytes))
