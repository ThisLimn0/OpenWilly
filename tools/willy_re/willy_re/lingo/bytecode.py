"""Lingo bytecode definitions and Lscr chunk parser.

Based on the ProjectorRays / LingoDec opcode set (MPL-2.0).
Director 6 Lingo uses a stack-based bytecode with ~70 opcodes.
"""

from __future__ import annotations

import logging
import struct
from dataclasses import dataclass, field
from enum import IntEnum
from typing import Any

log = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# Opcode definitions
# ---------------------------------------------------------------------------


class OpCode(IntEnum):
    """Director 6 Lingo bytecodes."""

    # Stack operations
    kOpRet = 0x01
    kOpRetFactory = 0x02
    kOpPushZero = 0x03
    kOpMul = 0x04
    kOpAdd = 0x05
    kOpSub = 0x06
    kOpDiv = 0x07
    kOpMod = 0x08
    kOpInv = 0x09
    kOpJoinStr = 0x0A
    kOpJoinPadStr = 0x0B
    kOpLt = 0x0C
    kOpLtEq = 0x0D
    kOpNtEq = 0x0E
    kOpEq = 0x0F
    kOpGt = 0x10
    kOpGtEq = 0x11
    kOpAnd = 0x12
    kOpOr = 0x13
    kOpNot = 0x14
    kOpContainsStr = 0x15
    kOpContains0Str = 0x16
    kOpGetChunk = 0x17
    kOpHiliteChunk = 0x18
    kOpOntoSpr = 0x19
    kOpIntoSpr = 0x1A
    kOpGetField = 0x1B
    kOpStartTell = 0x1C
    kOpEndTell = 0x1D
    kOpPushList = 0x1E
    kOpPushPropList = 0x1F
    # D4→D6 opcode remapper: reads old D4 opcode byte, looks up D6
    # equivalent in the Lscr fixup table (section type 0x0b), then
    # re-dispatches.  Normally transparent — Director rewrites bytecode
    # at load time so we should never see this in D6 files.
    kOpD4Translate = 0x20
    # Sprite operations added in D6 (replacements for D4 opcodes via fixup).
    # kOpSpriteOp: performs a sprite operation with an implicit null argument.
    # kOpGetSprProp: reads a sprite property, pushes result.
    kOpSpriteOp = 0x21
    kOpGetSprProp = 0x22

    # Immediate value opcodes (1 byte arg or 2 byte arg if ≥ 0x40)
    kOpPushInt8 = 0x41  # push immediate byte
    kOpPushArgListNoRet = 0x42  # shares handler with kOpPushVarRef (0x46) in Willy32 binary
    kOpPushArgList = 0x43
    kOpPushCons = 0x44  # push constant from pool
    kOpPushSymb = 0x45  # push symbol (name table ref)
    kOpPushVarRef = 0x46  # shares handler with kOpPushArgListNoRet (0x42) in Willy32 binary
    kOpGetGlobal2 = 0x47
    kOpGetGlobal = 0x48
    kOpGetProp = 0x49
    kOpGetParam = 0x4A
    kOpGetLocal = 0x4B
    kOpSetGlobal2 = 0x4C
    kOpSetGlobal = 0x4D
    kOpSetProp = 0x4E
    kOpSetParam = 0x4F
    kOpSetLocal = 0x50
    kOpJmp = 0x51
    kOpEndRepeat = 0x52
    kOpJmpIfZ = 0x53
    kOpLocalCall = 0x54
    kOpExtCall = 0x55
    kOpObjCallV4 = 0x56
    kOpPut = 0x57
    kOpPutChunk = 0x58
    kOpDeleteChunk = 0x59
    kOpGet = 0x5A
    kOpSet = 0x5B
    kOpGetMovieProp = 0x5C
    kOpSetMovieProp = 0x5D
    # DEPRECATED: Not implemented in Willy32 / Director 6.0 — maps to
    # the error handler in the binary dispatch table.  Kept for compat
    # with ProjectorRays definitions; may exist in Director 7+.
    kOpGetObjProp = 0x5E
    kOpSetObjProp = 0x5F
    kOpTellCall = 0x60
    kOpPeek = 0x61
    kOpPop = 0x62
    kOpTheBuiltin = 0x63
    kOpObjCall = 0x64
    kOpPushChunkVarRef = 0x65
    kOpPushInt16 = 0x66
    kOpPushInt32 = 0x67
    kOpGetChainedProp = 0x68
    kOpPushFloat32 = 0x69
    kOpGetTopLevelProp = 0x6A
    kOpSetTopLevelProp = 0x6B  # partner to kOpGetTopLevelProp


# Opcode names for pretty-printing
OPCODE_NAMES: dict[int, str] = {op.value: op.name for op in OpCode}


def opcode_name(op: int) -> str:
    return OPCODE_NAMES.get(op, f"op_{op:02X}")


# ---------------------------------------------------------------------------
# Script data structures
# ---------------------------------------------------------------------------


@dataclass
class LingoInstruction:
    """A single decoded bytecode instruction."""

    offset: int  # Byte offset in the bytecode stream
    opcode: int  # Raw opcode byte
    arg: int = 0  # Immediate argument (0, 1, 2, or 4 bytes)
    arg_bytes: int = 0  # Number of argument bytes (0, 1, 2, or 4)
    float_arg: float | None = None  # For kOpPushFloat32

    @property
    def name(self) -> str:
        # For 2-byte arg versions (0x80+ range), map them back
        base = self.opcode
        if base >= 0x80:
            base = base - 0x40
        return opcode_name(base)

    def __repr__(self) -> str:
        if self.arg_bytes > 0:
            return f"<{self.offset:04X}: {self.name} {self.arg}>"
        return f"<{self.offset:04X}: {self.name}>"


@dataclass
class ScriptConstant:
    """A constant in the script constant pool.

    Datum type values (verified against Willy32 binary)::

        0 = Null      2 = Void     6 = Object
        1 = String     4 = Integer   8 = Symbol
                                    9 = Float
    """

    type: int  # 0=null, 1=string, 2=void, 4=int, 6=object, 8=symbol, 9=float
    value: Any


@dataclass
class LingoScript:
    """A parsed Lscr (Lingo script) chunk."""

    # Script metadata
    script_number: int = 0
    script_flags: int = 0
    handler_count: int = 0

    # Handlers
    handlers: list[LingoHandler] = field(default_factory=list)

    # Constant pool
    constants: list[ScriptConstant] = field(default_factory=list)

    # Name table (from Lnam)
    names: list[str] = field(default_factory=list)

    # Script-level property / global names
    property_names: list[str] = field(default_factory=list)
    global_names: list[str] = field(default_factory=list)


@dataclass
class LingoHandler:
    """A single handler (function) within a script."""

    name_id: int = 0
    bytecode_offset: int = 0
    bytecode_length: int = 0
    arg_count: int = 0
    local_count: int = 0
    global_count: int = 0

    # Decoded
    name: str = ""
    instructions: list[LingoInstruction] = field(default_factory=list)
    arg_names: list[str] = field(default_factory=list)
    local_names: list[str] = field(default_factory=list)
    global_names: list[str] = field(default_factory=list)


# ---------------------------------------------------------------------------
# Lscr chunk parser
# ---------------------------------------------------------------------------


def parse_lscr(data: bytes, names: list[str] | None = None) -> LingoScript:
    """Parse an Lscr chunk into a LingoScript.

    Based on the ScummVM Director engine ``LingoCompiler::compileLingoV4``.
    Reference: engines/director/lingo/lingo-bytecode.cpp

    Lscr header layout (always big-endian, 92 bytes / 0x5C):
        0x00   8 bytes  unknown (version magic)
        0x08   u32      totalLength
        0x0C   u32      totalLength2
        0x10   u16      codeStoreOffset  (= start of bytecodes, typ. 92)
        0x12   u16      scriptId
        0x14   s16      unknown
        0x16   s16      parentNumber
        0x18   12 bytes unknown
        0x24   u16      unknown
        0x26   u32      scriptFlags
        0x2A   4 bytes  unknown
        0x2E   u16      assemblyId
        0x30   s16      factoryNameId

    Contents map (offset 0x32 onwards, mixed u16 counts + u32 offsets):
        0x32   u16      eventMapCount
        0x34   u32      eventMapOffset
        0x38   u32      eventMapFlags
        0x3C   u16      propertiesCount
        0x3E   u32      propertiesOffset  (array of s16 name indices)
        0x42   u16      globalsCount
        0x44   u32      globalsOffset     (array of s16 name indices)
        0x48   u16      functionsCount    (number of handler records)
        0x4A   u32      functionsOffset   (42-byte handler records)
        0x4E   u16      constsCount
        0x50   u32      constsOffset      (constant index: u32 type + u32 value)
        0x54   u32      constsStoreCount
        0x58   u32      constsStoreOffset (raw constant data)

    Handler record (42 bytes, at functionsOffset):
        +0x00  s16      nameIndex
        +0x02  u16      unknown
        +0x04  u32      codeLength       (bytes of bytecode)
        +0x08  u32      codeStartOffset  (absolute, from chunk start)
        +0x0C  u16      argCount
        +0x0E  u32      argOffset        (absolute, to s16[] name indices)
        +0x12  u16      varCount         (local variables)
        +0x14  u32      varOffset        (absolute, to s16[] name indices)
        +0x18  18 bytes unknown (9 × u16)

    Parameters
    ----------
    data : bytes
        Raw Lscr chunk data (after FourCC + length).
    names : optional list[str]
        Name table from Lnam chunk.
    """
    if len(data) < 0x5C:
        log.warning("Lscr too small: %d bytes (need >= 0x5C)", len(data))
        return LingoScript()

    script = LingoScript()
    if names:
        script.names = names

    # --- Header (always big-endian) ---
    code_store_offset = struct.unpack_from(">H", data, 0x10)[0]
    script_number = struct.unpack_from(">H", data, 0x12)[0]
    script_flags = struct.unpack_from(">I", data, 0x26)[0]

    script.script_number = script_number
    script.script_flags = script_flags

    # --- Contents map ---
    properties_count = struct.unpack_from(">H", data, 0x3C)[0]
    properties_offset = struct.unpack_from(">I", data, 0x3E)[0]
    globals_count = struct.unpack_from(">H", data, 0x42)[0]
    globals_offset = struct.unpack_from(">I", data, 0x44)[0]
    functions_count = struct.unpack_from(">H", data, 0x48)[0]
    functions_offset = struct.unpack_from(">I", data, 0x4A)[0]
    consts_count = struct.unpack_from(">H", data, 0x4E)[0]
    consts_offset = struct.unpack_from(">I", data, 0x50)[0]
    consts_store_offset = struct.unpack_from(">I", data, 0x58)[0]

    script.handler_count = functions_count

    log.debug(
        "Lscr #%d: codeStoreOff=%d, flags=0x%X, props=%d@%d, "
        "globals=%d@%d, funcs=%d@%d, consts=%d@%d, constsStore@%d",
        script_number,
        code_store_offset,
        script_flags,
        properties_count,
        properties_offset,
        globals_count,
        globals_offset,
        functions_count,
        functions_offset,
        consts_count,
        consts_offset,
        consts_store_offset,
    )

    # --- Parse script-level property names ---
    script.property_names = _read_name_indices_s16(data, properties_offset, properties_count, names)

    # --- Parse script-level global names ---
    script.global_names = _read_name_indices_s16(data, globals_offset, globals_count, names)

    # --- Parse constants (D5+ format: 8-byte entries) ---
    _parse_constants(data, consts_offset, consts_store_offset, consts_count, script)

    # --- Parse handler records (42 bytes each) ---
    HANDLER_RECORD_SIZE = 42
    offset = functions_offset
    for h_idx in range(functions_count):
        if offset + HANDLER_RECORD_SIZE > len(data):
            log.warning(
                "Lscr: handler %d truncated at offset %d (need %d, have %d)",
                h_idx,
                offset,
                offset + HANDLER_RECORD_SIZE,
                len(data),
            )
            break

        handler = LingoHandler()
        handler.name_id = struct.unpack_from(">h", data, offset + 0x00)[0]  # s16
        # +0x02: u16 unknown (skip)
        handler.bytecode_length = struct.unpack_from(">I", data, offset + 0x04)[0]
        code_start_offset = struct.unpack_from(">I", data, offset + 0x08)[0]
        handler.arg_count = struct.unpack_from(">H", data, offset + 0x0C)[0]
        arg_offset = struct.unpack_from(">I", data, offset + 0x0E)[0]
        handler.local_count = struct.unpack_from(">H", data, offset + 0x12)[0]
        var_offset = struct.unpack_from(">I", data, offset + 0x14)[0]

        # bytecode_offset is the absolute start in the data
        handler.bytecode_offset = code_start_offset

        # Resolve handler name
        if names and 0 <= handler.name_id < len(names):
            handler.name = names[handler.name_id]
        else:
            handler.name = f"handler_{handler.name_id}"

        # Resolve arg name list (s16 indices within the code area)
        handler.arg_names = _read_name_indices_s16(data, arg_offset, handler.arg_count, names)

        # Resolve local variable name list
        handler.local_names = _read_name_indices_s16(data, var_offset, handler.local_count, names)

        # Globals are at the script level, not per-handler
        handler.global_names = script.global_names
        handler.global_count = globals_count

        # Decode bytecode
        handler.instructions = _decode_bytecode(
            data, handler.bytecode_offset, handler.bytecode_length
        )

        log.debug(
            "  Handler[%d] '%s': code=%d bytes @%d, args=%d, locals=%d",
            h_idx,
            handler.name,
            handler.bytecode_length,
            handler.bytecode_offset,
            handler.arg_count,
            handler.local_count,
        )

        script.handlers.append(handler)
        offset += HANDLER_RECORD_SIZE

    return script


def _parse_constants(
    data: bytes,
    consts_offset: int,
    consts_store_offset: int,
    count: int,
    script: LingoScript,
) -> None:
    """Parse the constant pool from an Lscr chunk (Director 5+ format).

    The constants index at *consts_offset* contains *count* entries of
    (u32 type, u32 value).

    - type 1 (string): value = offset into consts store.
      At consts_store[value]: u32 length, then string bytes.
    - type 4 (integer): value IS the integer directly.
    - type 9 (float): value = offset into consts store.
      At consts_store[value]: u32 length (8 or 10), then float bytes.
    """
    tbl = consts_offset
    for i in range(count):
        if tbl + 8 > len(data):
            break
        const_type = struct.unpack_from(">I", data, tbl)[0]
        value = struct.unpack_from(">I", data, tbl + 4)[0]
        tbl += 8

        if const_type == 1:  # string
            store_pos = consts_store_offset + value
            if store_pos + 4 > len(data):
                script.constants.append(ScriptConstant(type=1, value=""))
                continue
            str_len = struct.unpack_from(">I", data, store_pos)[0]
            raw = data[store_pos + 4 : store_pos + 4 + str_len]
            # Strip trailing NUL if present
            if raw and raw[-1:] == b"\x00":
                raw = raw[:-1]
            script.constants.append(ScriptConstant(type=1, value=raw.decode("latin-1")))

        elif const_type == 4:  # integer — value IS the integer
            script.constants.append(
                ScriptConstant(type=4, value=struct.unpack(">i", struct.pack(">I", value))[0])
            )

        elif const_type == 9:  # float
            store_pos = consts_store_offset + value
            if store_pos + 4 > len(data):
                script.constants.append(ScriptConstant(type=9, value=0.0))
                continue
            float_len = struct.unpack_from(">I", data, store_pos)[0]
            if float_len == 8 and store_pos + 4 + 8 <= len(data):
                val = struct.unpack_from(">d", data, store_pos + 4)[0]
            elif float_len == 10 and store_pos + 4 + 10 <= len(data):
                # 80-bit extended float (SANE) — approximate with struct
                val = _read_float80(data, store_pos + 4)
            else:
                log.warning("Unexpected float length %d at const %d", float_len, i)
                val = 0.0
            script.constants.append(ScriptConstant(type=9, value=val))

        else:
            log.debug("Unknown constant type %d at const %d", const_type, i)
            script.constants.append(ScriptConstant(type=const_type, value=None))


def _read_float80(data: bytes, offset: int) -> float:
    """Read an 80-bit extended precision float (Apple SANE format)."""
    # 80-bit: 1 sign + 15 exponent + 64 mantissa (no implicit bit)
    if offset + 10 > len(data):
        return 0.0
    exponent = struct.unpack_from(">H", data, offset)[0]
    mantissa = struct.unpack_from(">Q", data, offset + 2)[0]
    sign = -1.0 if (exponent & 0x8000) else 1.0
    exponent &= 0x7FFF
    if exponent == 0 and mantissa == 0:
        return 0.0
    if exponent == 0x7FFF:
        return float("inf") * sign if mantissa == 0 else float("nan")
    # Bias for 80-bit is 16383
    return sign * (mantissa / (1 << 63)) * (2.0 ** (exponent - 16383))


def _read_name_indices_s16(
    data: bytes, offset: int, count: int, names: list[str] | None
) -> list[str]:
    """Read a list of s16 name-table indices and resolve them to strings."""
    result: list[str] = []
    if offset == 0 or count == 0:
        return result
    for i in range(count):
        idx_off = offset + i * 2
        if idx_off + 2 > len(data):
            break
        name_idx = struct.unpack_from(">h", data, idx_off)[0]  # signed!
        if name_idx == -1:
            break  # end-of-list sentinel
        if names and 0 <= name_idx < len(names):
            result.append(names[name_idx])
        else:
            result.append(f"name_{name_idx}")
    return result


def _decode_bytecode(data: bytes, offset: int, length: int) -> list[LingoInstruction]:
    """Decode raw bytecode into a list of LingoInstructions."""
    instructions: list[LingoInstruction] = []
    pos = offset
    end = offset + length

    # Opcodes that require a 4-byte inline argument regardless of their range
    FOUR_BYTE_OPS = {OpCode.kOpPushInt32, OpCode.kOpPushFloat32}

    while pos < end and pos < len(data):
        op = data[pos]
        instr = LingoInstruction(offset=pos - offset, opcode=op)
        base_op = op - 0x40 if op >= 0x80 else op

        if base_op in FOUR_BYTE_OPS:
            # 4-byte inline argument (int32 or float32)
            if pos + 4 >= len(data):
                break
            raw_bytes = data[pos + 1 : pos + 5]
            if base_op == OpCode.kOpPushFloat32:
                instr.float_arg = struct.unpack_from(">f", raw_bytes)[0]
                instr.arg = struct.unpack_from(">I", raw_bytes)[0]  # raw bits
            else:
                instr.arg = struct.unpack_from(">i", raw_bytes)[0]
            instr.arg_bytes = 4
            pos += 5
        elif op >= 0x80:
            # 2-byte argument
            if pos + 2 >= len(data):
                break
            instr.arg = struct.unpack_from(">H", data, pos + 1)[0]
            instr.arg_bytes = 2
            pos += 3
        elif op >= 0x40:
            # 1-byte argument
            if pos + 1 >= len(data):
                break
            instr.arg = data[pos + 1]
            instr.arg_bytes = 1
            pos += 2
        else:
            # No argument
            pos += 1

        instructions.append(instr)

    return instructions


# ---------------------------------------------------------------------------
# Lnam (name table) parser
# ---------------------------------------------------------------------------


def parse_lnam(data: bytes) -> list[str]:
    """Parse an Lnam chunk into a list of name strings.

    Lnam layout (always big-endian regardless of container endianness):
        0x00  u32  unknown (version?)
        0x04  u32  unknown
        0x08  u32  unknown
        0x0C  u32  unknown
        0x10  u16  namesOffset  (typically 20 — start of Pascal strings)
        0x12  u16  nameCount
        [namesOffset ..] Pascal strings (1-byte length + chars)

    Parameters
    ----------
    data : bytes
        Raw Lnam chunk data (after FourCC + length).
    """
    if len(data) < 20:
        log.warning("Lnam too small: %d bytes", len(data))
        return []

    # Header: 4 × u32 + 2 × u16  (always big-endian)
    names_offset = struct.unpack_from(">H", data, 0x10)[0]
    name_count = struct.unpack_from(">H", data, 0x12)[0]

    if names_offset == 0:
        names_offset = 20  # default

    log.debug("Lnam: namesOffset=%d, nameCount=%d", names_offset, name_count)

    names: list[str] = []
    offset = names_offset
    for _ in range(name_count):
        if offset >= len(data):
            break
        str_len = data[offset]
        offset += 1
        if offset + str_len > len(data):
            log.warning("Lnam: string at offset %d truncated", offset - 1)
            break
        name = data[offset : offset + str_len].decode("latin-1")
        offset += str_len
        names.append(name)

    log.debug("Lnam: parsed %d names", len(names))
    return names


# ---------------------------------------------------------------------------
# LctX (script context) parser
# ---------------------------------------------------------------------------


@dataclass
class ScriptContextEntry:
    """An entry in the LctX script context table."""

    id: int
    type: int  # 1=movie, 3=score, 7=parent
    cast_id: int
    name: str = ""


def parse_lctx(data: bytes) -> list[ScriptContextEntry]:
    """Parse an LctX chunk.

    Parameters
    ----------
    data : bytes
        Raw LctX chunk data (after FourCC + length).
    """
    if len(data) < 12:
        return []

    offset = 0
    _unk1 = struct.unpack_from(">I", data, offset)[0]
    offset += 4
    _unk2 = struct.unpack_from(">I", data, offset)[0]
    offset += 4
    count = struct.unpack_from(">I", data, offset)[0]
    offset += 4

    entries: list[ScriptContextEntry] = []
    for i in range(count):
        if offset + 12 > len(data):
            break
        _unk = struct.unpack_from(">I", data, offset)[0]
        offset += 4
        script_type = struct.unpack_from(">H", data, offset)[0]
        offset += 2
        cast_id = struct.unpack_from(">H", data, offset)[0]
        offset += 2
        offset += 4  # skip

        entries.append(ScriptContextEntry(id=i, type=script_type, cast_id=cast_id))

    return entries
