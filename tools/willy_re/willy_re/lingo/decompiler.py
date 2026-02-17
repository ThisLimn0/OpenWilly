"""Stack-based Lingo bytecode decompiler.

Converts LingoInstructions (bytecode) into AST nodes, then renders
them as readable Lingo source text.

Approach based on LingoDec from ProjectorRays (MPL-2.0):
- Walk bytecode instructions sequentially
- Maintain an expression stack
- Build AST nodes for statements and control flow
- Handle if/else, repeat, tell blocks via jump target analysis
"""

from __future__ import annotations

import logging
from dataclasses import dataclass
from typing import Any

from .ast_nodes import (
    AssignStmt,
    BinaryOp,
    CallStmt,
    ChunkExpr,
    FloatLiteral,
    FunctionCall,
    GlobalRef,
    GlobalStmt,
    HandlerNode,
    Identifier,
    IfStmt,
    IntLiteral,
    ListLiteral,
    MemberAccess,
    MethodCall,
    Node,
    PropListLiteral,
    PropertyRef,
    PutStmt,
    RepeatWhileStmt,
    ReturnStmt,
    ScriptNode,
    StringLiteral,
    SymbolLiteral,
    TellStmt,
    TheExpr,
    UnaryOp,
)
from .bytecode import (
    LingoHandler,
    LingoInstruction,
    LingoScript,
    OpCode,
)
from .names import MEMBER_PROPERTIES, SPRITE_PROPERTIES, THE_BUILTINS

log = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# Control-flow block types identified by the pre-pass
# ---------------------------------------------------------------------------


@dataclass
class _IfBlock:
    """An if/else block identified by jump analysis."""

    cond_idx: int  # instruction index of kOpJmpIfZ
    else_idx: int  # first instruction of else branch (or end)
    end_idx: int  # first instruction after the entire if/else
    has_else: bool = False


@dataclass
class _RepeatBlock:
    """A repeat block identified by jump analysis."""

    start_idx: int  # first instruction of the loop body
    end_idx: int  # instruction index of kOpEndRepeat
    back_jmp_idx: int  # instruction index of the kOpJmp back to condition


class DecompileError(Exception):
    pass


# Director 5+ constant entry size (u32 type + u32 value = 8 bytes).
# Verified against Willy32 binary: stack push/pop operates in 8-byte steps
# (DAT_00535d02 += 8), each element is 4-byte type + 4-byte value.
CONST_ENTRY_SIZE = 8
# Director 5+ variable stride (locals / params indexed by raw_arg // VAR_STRIDE).
# Confirmed: Willy32 uses the same 8-byte stride for variable indexing.
VAR_STRIDE = 8


class Decompiler:
    """Decompile a LingoScript into Lingo source text."""

    def __init__(self, script: LingoScript):
        self.script = script
        self.names = script.names
        self.constants = script.constants

    def decompile(self) -> str:
        """Decompile the entire script to Lingo source text."""
        node = self.decompile_ast()
        return node.to_lingo()

    def decompile_ast(self) -> ScriptNode:
        """Decompile the entire script to an AST."""
        handlers = []
        for handler in self.script.handlers:
            try:
                h_node = self._decompile_handler(handler)
                handlers.append(h_node)
            except Exception as e:
                log.warning("Failed to decompile handler '%s': %s", handler.name, e)
                # Create a stub with a comment
                h_node = HandlerNode(
                    name=handler.name or f"handler_{handler.name_id}",
                    body=[],
                )
                handlers.append(h_node)
        return ScriptNode(handlers=handlers)

    def _decompile_handler(self, handler: LingoHandler) -> HandlerNode:
        """Decompile a single handler into a HandlerNode."""
        instrs = handler.instructions

        # Build offset→index map for jump target resolution
        offset_to_idx: dict[int, int] = {}
        for i, ins in enumerate(instrs):
            offset_to_idx[ins.offset] = i

        # Pre-pass: identify control-flow blocks
        if_blocks = self._find_if_blocks(instrs, offset_to_idx)
        repeat_blocks = self._find_repeat_blocks(instrs, offset_to_idx)

        stmts = self._decompile_range(
            handler, instrs, 0, len(instrs), if_blocks, repeat_blocks, offset_to_idx
        )

        return HandlerNode(
            name=handler.name or f"handler_{handler.name_id}",
            args=handler.arg_names,
            body=stmts,
        )

    def _find_if_blocks(
        self, instrs: list[LingoInstruction], offset_to_idx: dict[int, int]
    ) -> dict[int, _IfBlock]:
        """Identify if/else blocks from kOpJmpIfZ instructions."""
        blocks: dict[int, _IfBlock] = {}  # keyed by instruction index of JmpIfZ

        for i, instr in enumerate(instrs):
            base = instr.opcode - 0x40 if instr.opcode >= 0x80 else instr.opcode
            if base != OpCode.kOpJmpIfZ:
                continue

            # JmpIfZ arg is the bytecode offset to jump to (the else/end)
            target_offset = instr.arg
            else_idx = offset_to_idx.get(target_offset, len(instrs))

            # Check if there's a kOpJmp just before else_idx — that's the end-of-then jump
            end_idx = else_idx
            has_else = False
            if else_idx > 0 and else_idx - 1 < len(instrs):
                prev = instrs[else_idx - 1]
                prev_base = prev.opcode - 0x40 if prev.opcode >= 0x80 else prev.opcode
                if prev_base == OpCode.kOpJmp:
                    jmp_target = prev.arg
                    real_end = offset_to_idx.get(jmp_target, len(instrs))
                    if real_end > else_idx:
                        end_idx = real_end
                        has_else = True

            blocks[i] = _IfBlock(cond_idx=i, else_idx=else_idx, end_idx=end_idx, has_else=has_else)

        return blocks

    def _find_repeat_blocks(
        self, instrs: list[LingoInstruction], offset_to_idx: dict[int, int]
    ) -> dict[int, _RepeatBlock]:
        """Identify repeat blocks from kOpEndRepeat + kOpJmp pairs."""
        blocks: dict[int, _RepeatBlock] = {}

        for i, instr in enumerate(instrs):
            base = instr.opcode - 0x40 if instr.opcode >= 0x80 else instr.opcode
            if base != OpCode.kOpEndRepeat:
                continue

            # kOpEndRepeat arg = offset to jump back to (start of loop / condition)
            loop_start_offset = instr.arg
            loop_start_idx = offset_to_idx.get(loop_start_offset, 0)

            # The body starts after the JmpIfZ that guards the loop
            body_start = loop_start_idx
            # Find the JmpIfZ right before: walk from loop_start forward
            for k in range(loop_start_idx, i):
                kb = instrs[k].opcode - 0x40 if instrs[k].opcode >= 0x80 else instrs[k].opcode
                if kb == OpCode.kOpJmpIfZ:
                    body_start = k + 1
                    break

            blocks[i] = _RepeatBlock(
                start_idx=body_start,
                end_idx=i,
                back_jmp_idx=i,
            )

        return blocks

    def _decompile_range(
        self,
        handler: LingoHandler,
        instrs: list[LingoInstruction],
        start: int,
        end: int,
        if_blocks: dict[int, _IfBlock],
        repeat_blocks: dict[int, _RepeatBlock],
        offset_to_idx: dict[int, int],
        _depth: int = 0,
    ) -> list[Node]:
        """Decompile a range of instructions [start, end) into statements."""
        if _depth > 15:
            # Too deeply nested — fall back to linear processing
            return self._decompile_range_linear(handler, instrs, start, end)
        stack: list[Node] = []
        statements: list[Node] = []
        i = start
        max_iterations = (end - start + 1) * 3  # safety: never loop more than 3x range size
        iteration = 0

        while i < end:
            iteration += 1
            if iteration > max_iterations:
                log.debug("Infinite loop safety: breaking at i=%d range=[%d,%d)", i, start, end)
                break
            instr = instrs[i]
            op = instr.opcode
            base_op = op - 0x40 if op >= 0x80 else op

            # --- Check if this is a known if-block entry ---
            if base_op == OpCode.kOpJmpIfZ and i in if_blocks:
                block = if_blocks[i]
                cond = self._pop(stack)

                then_stmts = self._decompile_range(
                    handler,
                    instrs,
                    i + 1,
                    block.else_idx - 1 if block.has_else else block.else_idx,
                    if_blocks,
                    repeat_blocks,
                    offset_to_idx,
                    _depth + 1,
                )

                else_stmts: list[Node] = []
                if block.has_else:
                    else_stmts = self._decompile_range(
                        handler,
                        instrs,
                        block.else_idx,
                        block.end_idx,
                        if_blocks,
                        repeat_blocks,
                        offset_to_idx,
                        _depth + 1,
                    )

                statements.append(IfStmt(cond, then_stmts, else_stmts))
                i = block.end_idx
                continue

            # --- Check if this is a repeat-block EndRepeat ---
            if base_op == OpCode.kOpEndRepeat and i in repeat_blocks:
                # This should only be hit if we're inside the loop body
                # and reach the end — skip it (handled by parent range)
                i += 1
                continue

            # --- Skip bare Jmp (else-terminator or loop-back) ---
            if base_op == OpCode.kOpJmp:
                # Check if it's a loop-back (target before current)
                target_offset = instr.arg
                target_idx = offset_to_idx.get(target_offset, i + 1)
                if target_idx <= i:
                    # This is a repeat loop — find the condition range
                    # The condition was evaluated between target_idx and i
                    cond = self._pop(stack) if stack else IntLiteral(1)
                    # Find the matching EndRepeat after this Jmp
                    er_idx = i + 1
                    for k in range(i + 1, end):
                        kb = (
                            instrs[k].opcode - 0x40
                            if instrs[k].opcode >= 0x80
                            else instrs[k].opcode
                        )
                        if kb == OpCode.kOpEndRepeat:
                            er_idx = k
                            break

                    # The JmpIfZ between target and i defines the body start
                    body_start = i + 1  # fallback
                    for k in range(target_idx, i):
                        kb = (
                            instrs[k].opcode - 0x40
                            if instrs[k].opcode >= 0x80
                            else instrs[k].opcode
                        )
                        if kb == OpCode.kOpJmpIfZ:
                            body_start = k + 1
                            cond = self._pop(stack) if stack else IntLiteral(1)
                            break

                    body_stmts = self._decompile_range(
                        handler,
                        instrs,
                        body_start,
                        i,
                        if_blocks,
                        repeat_blocks,
                        offset_to_idx,
                        _depth + 1,
                    )
                    statements.append(RepeatWhileStmt(cond, body_stmts))
                    i = er_idx + 1 if er_idx < end else end
                    continue
                # Forward jump — likely end-of-then in if/else; skip
                i += 1
                continue

            try:
                i = self._process_instruction(base_op, instr, instrs, i, stack, statements, handler)
            except Exception as e:
                log.debug("Instruction error at %04X op=%02X: %s", instr.offset, op, e)
                i += 1

        return statements

    def _decompile_range_linear(
        self,
        handler: LingoHandler,
        instrs: list[LingoInstruction],
        start: int,
        end: int,
    ) -> list[Node]:
        """Fallback: decompile a range without control-flow analysis."""
        stack: list[Node] = []
        statements: list[Node] = []
        i = start
        while i < end:
            instr = instrs[i]
            op = instr.opcode
            base_op = op - 0x40 if op >= 0x80 else op
            try:
                new_i = self._process_instruction(
                    base_op, instr, instrs, i, stack, statements, handler
                )
                if new_i <= i:
                    i += 1  # Force progress
                else:
                    i = new_i
            except Exception:
                i += 1
        return statements

    def _process_instruction(
        self,
        base_op: int,
        instr: LingoInstruction,
        instrs: list[LingoInstruction],
        idx: int,
        stack: list[Node],
        stmts: list[Node],
        handler: LingoHandler,
    ) -> int:
        """Process a single instruction. Returns next instruction index."""
        arg = instr.arg

        # --- Stack push operations ---
        if base_op == OpCode.kOpPushZero:
            stack.append(IntLiteral(0))

        elif base_op == OpCode.kOpPushInt8:
            stack.append(IntLiteral(arg))

        elif base_op == OpCode.kOpPushInt16:
            # arg is already a 16-bit value
            stack.append(IntLiteral(arg))

        elif base_op == OpCode.kOpPushInt32:
            stack.append(IntLiteral(arg))

        elif base_op == OpCode.kOpPushFloat32:
            # instr.float_arg contains the decoded float value
            float_val = instr.float_arg if instr.float_arg is not None else float(arg)
            stack.append(FloatLiteral(float_val))

        elif base_op == OpCode.kOpPushCons:
            # D5+: arg is a byte offset into the constants index;
            # divide by entry size (8) to get the actual index.
            node = self._resolve_constant(arg // CONST_ENTRY_SIZE)
            stack.append(node)

        elif base_op == OpCode.kOpPushSymb:
            name = self._name(arg)
            stack.append(SymbolLiteral(name))

        # --- Variable access ---
        elif base_op == OpCode.kOpGetLocal:
            name = self._local_name(arg // VAR_STRIDE, handler)
            stack.append(Identifier(name))

        elif base_op == OpCode.kOpGetParam:
            name = self._param_name(arg // VAR_STRIDE, handler)
            stack.append(Identifier(name))

        elif base_op in (OpCode.kOpGetGlobal, OpCode.kOpGetGlobal2):
            name = self._name(arg)
            stack.append(GlobalRef(name))

        elif base_op == OpCode.kOpGetProp:
            name = self._name(arg)
            stack.append(PropertyRef(name))

        # --- Variable set ---
        elif base_op == OpCode.kOpSetLocal:
            val = self._pop(stack)
            name = self._local_name(arg // VAR_STRIDE, handler)
            stmts.append(AssignStmt(Identifier(name), val))

        elif base_op == OpCode.kOpSetParam:
            val = self._pop(stack)
            name = self._param_name(arg // VAR_STRIDE, handler)
            stmts.append(AssignStmt(Identifier(name), val))

        elif base_op in (OpCode.kOpSetGlobal, OpCode.kOpSetGlobal2):
            val = self._pop(stack)
            name = self._name(arg)
            stmts.append(AssignStmt(GlobalRef(name), val))

        elif base_op == OpCode.kOpSetProp:
            val = self._pop(stack)
            name = self._name(arg)
            stmts.append(AssignStmt(PropertyRef(name), val))

        # --- Arithmetic / comparison ---
        elif base_op == OpCode.kOpAdd:
            right = self._pop(stack)
            left = self._pop(stack)
            stack.append(BinaryOp("+", left, right))

        elif base_op == OpCode.kOpSub:
            right = self._pop(stack)
            left = self._pop(stack)
            stack.append(BinaryOp("-", left, right))

        elif base_op == OpCode.kOpMul:
            right = self._pop(stack)
            left = self._pop(stack)
            stack.append(BinaryOp("*", left, right))

        elif base_op == OpCode.kOpDiv:
            right = self._pop(stack)
            left = self._pop(stack)
            stack.append(BinaryOp("/", left, right))

        elif base_op == OpCode.kOpMod:
            right = self._pop(stack)
            left = self._pop(stack)
            stack.append(BinaryOp("mod", left, right))

        elif base_op == OpCode.kOpInv:
            operand = self._pop(stack)
            stack.append(UnaryOp("-", operand))

        elif base_op == OpCode.kOpNot:
            operand = self._pop(stack)
            stack.append(UnaryOp("not ", operand))

        elif base_op == OpCode.kOpEq:
            right = self._pop(stack)
            left = self._pop(stack)
            stack.append(BinaryOp("=", left, right))

        elif base_op == OpCode.kOpNtEq:
            right = self._pop(stack)
            left = self._pop(stack)
            stack.append(BinaryOp("<>", left, right))

        elif base_op == OpCode.kOpLt:
            right = self._pop(stack)
            left = self._pop(stack)
            stack.append(BinaryOp("<", left, right))

        elif base_op == OpCode.kOpLtEq:
            right = self._pop(stack)
            left = self._pop(stack)
            stack.append(BinaryOp("<=", left, right))

        elif base_op == OpCode.kOpGt:
            right = self._pop(stack)
            left = self._pop(stack)
            stack.append(BinaryOp(">", left, right))

        elif base_op == OpCode.kOpGtEq:
            right = self._pop(stack)
            left = self._pop(stack)
            stack.append(BinaryOp(">=", left, right))

        elif base_op == OpCode.kOpAnd:
            right = self._pop(stack)
            left = self._pop(stack)
            stack.append(BinaryOp("and", left, right))

        elif base_op == OpCode.kOpOr:
            right = self._pop(stack)
            left = self._pop(stack)
            stack.append(BinaryOp("or", left, right))

        elif base_op == OpCode.kOpJoinStr:
            right = self._pop(stack)
            left = self._pop(stack)
            stack.append(BinaryOp("&", left, right))

        elif base_op == OpCode.kOpJoinPadStr:
            right = self._pop(stack)
            left = self._pop(stack)
            stack.append(BinaryOp("&&", left, right))

        elif base_op == OpCode.kOpContainsStr:
            right = self._pop(stack)
            left = self._pop(stack)
            stack.append(BinaryOp("contains", left, right))

        elif base_op == OpCode.kOpContains0Str:
            right = self._pop(stack)
            left = self._pop(stack)
            stack.append(BinaryOp("starts", left, right))

        elif base_op == OpCode.kOpHiliteChunk:
            chunk_types = {1: "char", 2: "word", 3: "item", 4: "line"}
            ct = chunk_types.get(arg, "char")
            source = self._pop(stack)
            end = self._pop(stack)
            start = self._pop(stack)
            stmts.append(CallStmt(FunctionCall("hilite", [ChunkExpr(ct, start, end, source)])))

        elif base_op == OpCode.kOpOntoSpr:
            right = self._pop(stack)
            left = self._pop(stack)
            stack.append(BinaryOp("intersects", left, right))

        elif base_op == OpCode.kOpIntoSpr:
            right = self._pop(stack)
            left = self._pop(stack)
            stack.append(BinaryOp("within", left, right))

        # --- D4→D6 remapper (should never appear in pre-processed D6 bytecode) ---
        elif base_op == OpCode.kOpD4Translate:
            log.warning(
                "D4 opcode remapping (0x20) at offset %04X — "
                "bytecode was not pre-fixed during Lscr loading",
                instr.offset,
            )

        # --- Sprite operations (D6 replacements for D4 opcodes) ---
        elif base_op == OpCode.kOpSpriteOp:
            obj = self._pop(stack)
            stmts.append(CallStmt(FunctionCall("spriteOp", [obj])))

        elif base_op == OpCode.kOpGetSprProp:
            obj = self._pop(stack)
            prop = SPRITE_PROPERTIES.get(arg, f"sprProp_{arg}")
            stack.append(MemberAccess(obj, prop))

        elif base_op == OpCode.kOpStartTell:
            obj = self._pop(stack)
            stmts.append(TellStmt(obj, []))

        elif base_op == OpCode.kOpEndTell:
            pass  # End of tell block — structural, handled by control flow

        elif base_op == OpCode.kOpTellCall:
            name = self._name(arg)
            nargs = self._peek_arg_count(instrs, idx)
            args = self._pop_n(stack, nargs)
            call = FunctionCall(name, args)
            if self._was_no_ret(instrs, idx):
                stmts.append(CallStmt(call))
            else:
                stack.append(call)

        # --- List/PropList ---
        elif base_op == OpCode.kOpPushList:
            items = self._pop_n(stack, arg)
            stack.append(ListLiteral(items))

        elif base_op == OpCode.kOpPushPropList:
            count = arg
            pairs = []
            for _ in range(count):
                val = self._pop(stack)
                key = self._pop(stack)
                pairs.insert(0, (key, val))
            stack.append(PropListLiteral(pairs))

        elif base_op == OpCode.kOpPushArgList:
            # arg list for function calls — leave items on stack
            pass

        elif base_op == OpCode.kOpPushArgListNoRet:
            pass

        # --- Function calls ---
        elif base_op == OpCode.kOpExtCall:
            name = self._name(arg)
            # Previous instruction should be arglist with count
            nargs = self._peek_arg_count(instrs, idx)
            args = self._pop_n(stack, nargs)
            call = FunctionCall(name, args)
            # If there was a PushArgListNoRet, it's a statement
            if self._was_no_ret(instrs, idx):
                stmts.append(CallStmt(call))
            else:
                stack.append(call)

        elif base_op == OpCode.kOpLocalCall:
            nargs = self._peek_arg_count(instrs, idx)
            args = self._pop_n(stack, nargs)
            name = self._handler_name(arg)
            call = FunctionCall(name, args)
            if self._was_no_ret(instrs, idx):
                stmts.append(CallStmt(call))
            else:
                stack.append(call)

        elif base_op == OpCode.kOpObjCall:
            name = self._name(arg)
            nargs = self._peek_arg_count(instrs, idx)
            args = self._pop_n(stack, nargs)
            if args:
                obj = args[0]
                rest = args[1:]
                call_node = MethodCall(obj, name, rest)
            else:
                call_node = FunctionCall(name, [])
            if self._was_no_ret(instrs, idx):
                stmts.append(CallStmt(call_node))
            else:
                stack.append(call_node)

        elif base_op == OpCode.kOpObjCallV4:
            # Director 4 style object call — same semantics as kOpObjCall
            name = self._name(arg)
            nargs = self._peek_arg_count(instrs, idx)
            args = self._pop_n(stack, nargs)
            if args:
                obj = args[0]
                rest = args[1:]
                call_node = MethodCall(obj, name, rest)
            else:
                call_node = FunctionCall(name, [])
            if self._was_no_ret(instrs, idx):
                stmts.append(CallStmt(call_node))
            else:
                stack.append(call_node)

        # --- The builtins ---
        elif base_op == OpCode.kOpTheBuiltin:
            prop = THE_BUILTINS.get(arg, f"unknown_{arg}")
            stack.append(TheExpr(prop))

        # --- Get/Set movie/obj props ---
        elif base_op == OpCode.kOpGetMovieProp:
            name = self._name(arg)
            stack.append(TheExpr(name))

        elif base_op == OpCode.kOpSetMovieProp:
            val = self._pop(stack)
            name = self._name(arg)
            stmts.append(AssignStmt(TheExpr(name), val))

        elif base_op == OpCode.kOpGetObjProp:
            obj = self._pop(stack)
            name = self._name(arg)
            stack.append(MemberAccess(obj, name))

        elif base_op == OpCode.kOpSetObjProp:
            val = self._pop(stack)
            obj = self._pop(stack)
            name = self._name(arg)
            stmts.append(AssignStmt(MemberAccess(obj, name), val))

        # --- Put ---
        elif base_op == OpCode.kOpPut:
            modes = {1: "into", 2: "before", 3: "after"}
            mode = modes.get(arg, "into")
            target = self._pop(stack)
            value = self._pop(stack)
            stmts.append(PutStmt(mode, value, target))

        # --- Return ---
        elif base_op == OpCode.kOpRet:
            if stack:
                stmts.append(ReturnStmt(self._pop(stack)))
            # else: implicit return, skip

        elif base_op == OpCode.kOpRetFactory:
            if stack:
                stmts.append(ReturnStmt(self._pop(stack)))

        # --- Jump (control flow) ---
        # Handled by _decompile_range pre-pass; if we reach here, emit comment
        elif base_op == OpCode.kOpJmp:
            pass  # Handled in _decompile_range

        elif base_op == OpCode.kOpJmpIfZ:
            # If we get here, the if-block wasn't identified in pre-pass
            if stack:
                cond = self._pop(stack)
                stmts.append(IfStmt(cond, [], []))

        elif base_op == OpCode.kOpEndRepeat:
            pass  # Handled in _decompile_range

        # --- Chunk expressions ---
        elif base_op == OpCode.kOpGetChunk:
            chunk_types = {1: "char", 2: "word", 3: "item", 4: "line"}
            ct = chunk_types.get(arg, "char")
            source = self._pop(stack)
            end = self._pop(stack)
            start = self._pop(stack)
            stack.append(ChunkExpr(ct, start, end, source))

        # --- Pop (discard top of stack) ---
        elif base_op == OpCode.kOpPop:
            if stack:
                # Popped value might be a call expression used as statement
                val = stack.pop()
                if isinstance(val, (FunctionCall, MethodCall)):
                    stmts.append(CallStmt(val))

        elif base_op == OpCode.kOpPeek:
            if stack:
                stack.append(stack[-1])

        # --- Get/Set using the "get/set" opcodes ---
        elif base_op == OpCode.kOpGet:
            name = self._name(arg)
            stack.append(Identifier(name))

        elif base_op == OpCode.kOpSet:
            val = self._pop(stack)
            name = self._name(arg)
            stmts.append(AssignStmt(Identifier(name), val))

        elif base_op == OpCode.kOpGetField:
            field_num = self._pop(stack)
            stack.append(FunctionCall("field", [field_num]))

        elif base_op == OpCode.kOpPushVarRef:
            name = self._name(arg)
            stack.append(Identifier(name))

        # --- Global variant opcodes (identical to kOpGet/SetGlobal) ---
        elif base_op == OpCode.kOpGetGlobal2:
            name = self._name(arg)
            stack.append(Identifier(name))

        elif base_op == OpCode.kOpSetGlobal2:
            val = self._pop(stack)
            name = self._name(arg)
            stmts.append(AssignStmt(Identifier(name), val))

        # --- Chunk mutation ---
        elif base_op == OpCode.kOpPutChunk:
            chunk_types = {1: "char", 2: "word", 3: "item", 4: "line"}
            ct = chunk_types.get(arg, "char")
            source = self._pop(stack)
            end = self._pop(stack)
            start = self._pop(stack)
            value = self._pop(stack)
            stmts.append(PutStmt("into", value, ChunkExpr(ct, start, end, source)))

        elif base_op == OpCode.kOpDeleteChunk:
            chunk_types = {1: "char", 2: "word", 3: "item", 4: "line"}
            ct = chunk_types.get(arg, "char")
            source = self._pop(stack)
            end = self._pop(stack)
            start = self._pop(stack)
            stmts.append(CallStmt(FunctionCall("delete", [ChunkExpr(ct, start, end, source)])))

        # --- Chunk variable reference ---
        elif base_op == OpCode.kOpPushChunkVarRef:
            chunk_types = {1: "char", 2: "word", 3: "item", 4: "line"}
            ct = chunk_types.get(arg, "char")
            source = self._pop(stack)
            end = self._pop(stack)
            start = self._pop(stack)
            stack.append(ChunkExpr(ct, start, end, source))

        # --- Push 16-bit int ---
        elif base_op == OpCode.kOpPushInt16:
            stack.append(IntLiteral(arg))

        # --- Chained property access ---
        elif base_op == OpCode.kOpGetChainedProp:
            obj = self._pop(stack)
            name = self._name(arg)
            stack.append(MemberAccess(obj, name))

        # --- Top-level property access ---
        elif base_op == OpCode.kOpGetTopLevelProp:
            name = self._name(arg)
            stack.append(TheExpr(name))

        elif base_op == OpCode.kOpSetTopLevelProp:
            val = self._pop(stack)
            name = self._name(arg)
            stmts.append(AssignStmt(TheExpr(name), val))

        # Default: unknown opcode
        else:
            log.debug("Unhandled opcode: 0x%02X at offset %04X", instr.opcode, instr.offset)

        return idx + 1

    # -- Helper methods -------------------------------------------------------

    def _pop(self, stack: list[Node]) -> Node:
        if stack:
            return stack.pop()
        return IntLiteral(0)  # fallback

    def _pop_n(self, stack: list[Node], n: int) -> list[Node]:
        if n <= 0:
            return []
        items = []
        for _ in range(n):
            items.insert(0, self._pop(stack))
        return items

    def _name(self, idx: int) -> str:
        if 0 <= idx < len(self.names):
            return self.names[idx]
        log.debug(
            "Name index %d out of range (table has %d entries), using fallback",
            idx,
            len(self.names),
        )
        return f"name_{idx}"

    def _local_name(self, idx: int, handler: LingoHandler) -> str:
        if handler.local_names and idx < len(handler.local_names):
            return handler.local_names[idx]
        return f"local_{idx}"

    def _param_name(self, idx: int, handler: LingoHandler) -> str:
        if handler.arg_names and idx < len(handler.arg_names):
            return handler.arg_names[idx]
        return f"param_{idx}"

    def _handler_name(self, idx: int) -> str:
        if idx < len(self.script.handlers):
            h = self.script.handlers[idx]
            return h.name or f"handler_{idx}"
        return f"handler_{idx}"

    def _resolve_constant(self, idx: int) -> Node:
        if idx < len(self.constants):
            c = self.constants[idx]
            if c.type == 1:
                return StringLiteral(c.value)
            elif c.type == 4:
                return IntLiteral(c.value)
            elif c.type == 9:
                return FloatLiteral(c.value)
        return IntLiteral(0)

    def _peek_arg_count(self, instrs: list[LingoInstruction], idx: int) -> int:
        """Look back to find the PushArgList instruction that precedes a call."""
        for j in range(idx - 1, max(idx - 10, -1), -1):
            base = instrs[j].opcode
            if base >= 0x80:
                base -= 0x40
            if base in (OpCode.kOpPushArgList, OpCode.kOpPushArgListNoRet):
                return instrs[j].arg
        return 0

    def _was_no_ret(self, instrs: list[LingoInstruction], idx: int) -> bool:
        """Check if the preceding arglist was a no-return variant."""
        for j in range(idx - 1, max(idx - 10, -1), -1):
            base = instrs[j].opcode
            if base >= 0x80:
                base -= 0x40
            if base == OpCode.kOpPushArgListNoRet:
                return True
            if base == OpCode.kOpPushArgList:
                return False
        return True  # default: treat as statement


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------


def decompile_script(script: LingoScript) -> str:
    """Decompile a LingoScript to Lingo source text."""
    return Decompiler(script).decompile()


def decompile_handler(handler: LingoHandler, script: LingoScript) -> str:
    """Decompile a single handler to Lingo source text."""
    d = Decompiler(script)
    node = d._decompile_handler(handler)
    return node.to_lingo()
