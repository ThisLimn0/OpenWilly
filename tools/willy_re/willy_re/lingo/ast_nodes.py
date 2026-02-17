"""AST node definitions for the Lingo decompiler.

These nodes represent the structure of decompiled Lingo code
before rendering to source text.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass
class Node:
    """Base class for all AST nodes."""

    def to_lingo(self, indent: int = 0) -> str:
        """Render this node as Lingo source text."""
        raise NotImplementedError


# ---------------------------------------------------------------------------
# Literals
# ---------------------------------------------------------------------------


@dataclass
class IntLiteral(Node):
    value: int = 0

    def to_lingo(self, indent: int = 0) -> str:
        return str(self.value)


@dataclass
class FloatLiteral(Node):
    value: float = 0.0

    def to_lingo(self, indent: int = 0) -> str:
        return str(self.value)


@dataclass
class StringLiteral(Node):
    value: str = ""

    def to_lingo(self, indent: int = 0) -> str:
        if '"' not in self.value:
            return f'"{self.value}"'
        # Lingo has no string escape — use QUOTE constant for embedded quotes
        parts = self.value.split('"')
        return " & QUOTE & ".join(f'"{p}"' for p in parts)


@dataclass
class SymbolLiteral(Node):
    name: str = ""

    def to_lingo(self, indent: int = 0) -> str:
        return f"#{self.name}"


@dataclass
class ListLiteral(Node):
    items: list[Node] = field(default_factory=list)

    def to_lingo(self, indent: int = 0) -> str:
        inner = ", ".join(item.to_lingo() for item in self.items)
        return f"[{inner}]"


@dataclass
class PropListLiteral(Node):
    """Property list: [#key: value, ...]"""

    pairs: list[tuple[Node, Node]] = field(default_factory=list)

    def to_lingo(self, indent: int = 0) -> str:
        parts = []
        for key, val in self.pairs:
            parts.append(f"{key.to_lingo()}: {val.to_lingo()}")
        return f"[{', '.join(parts)}]"


# ---------------------------------------------------------------------------
# Identifiers & references
# ---------------------------------------------------------------------------


@dataclass
class Identifier(Node):
    name: str = ""

    def to_lingo(self, indent: int = 0) -> str:
        return self.name


@dataclass
class GlobalRef(Node):
    name: str = ""

    def to_lingo(self, indent: int = 0) -> str:
        return self.name


@dataclass
class PropertyRef(Node):
    name: str = ""

    def to_lingo(self, indent: int = 0) -> str:
        return self.name


# ---------------------------------------------------------------------------
# Expressions
# ---------------------------------------------------------------------------


# Lingo operator precedence (higher = binds tighter)
_OP_PRECEDENCE: dict[str, int] = {
    "or": 1,
    "and": 2,
    "not": 3,  # unary, but listed for reference
    "<": 4,
    "<=": 4,
    ">": 4,
    ">=": 4,
    "=": 4,
    "<>": 4,
    "contains": 4,
    "starts": 4,
    "&": 5,
    "&&": 5,
    "+": 6,
    "-": 6,
    "*": 7,
    "/": 7,
    "mod": 7,
    "intersects": 4,
    "within": 4,
}


@dataclass
class BinaryOp(Node):
    op: str = ""
    left: Node = field(default_factory=Node)
    right: Node = field(default_factory=Node)

    def to_lingo(self, indent: int = 0) -> str:
        my_prec = _OP_PRECEDENCE.get(self.op, 0)
        left_str = self.left.to_lingo()
        right_str = self.right.to_lingo()
        # Only parenthesise child BinaryOp with strictly lower precedence
        if isinstance(self.left, BinaryOp):
            child_prec = _OP_PRECEDENCE.get(self.left.op, 0)
            if child_prec < my_prec:
                left_str = f"({left_str})"
        if isinstance(self.right, BinaryOp):
            child_prec = _OP_PRECEDENCE.get(self.right.op, 0)
            if child_prec <= my_prec:  # right-assoc: use <=
                right_str = f"({right_str})"
        return f"{left_str} {self.op} {right_str}"


@dataclass
class UnaryOp(Node):
    op: str = ""
    operand: Node = field(default_factory=Node)

    def to_lingo(self, indent: int = 0) -> str:
        return f"{self.op}{self.operand.to_lingo()}"


@dataclass
class FunctionCall(Node):
    name: str = ""
    args: list[Node] = field(default_factory=list)

    def to_lingo(self, indent: int = 0) -> str:
        if not self.args:
            return self.name
        arg_str = ", ".join(a.to_lingo() for a in self.args)
        return f"{self.name}({arg_str})"


@dataclass
class MethodCall(Node):
    obj: Node = field(default_factory=Node)
    method: str = ""
    args: list[Node] = field(default_factory=list)

    def to_lingo(self, indent: int = 0) -> str:
        arg_str = ", ".join(a.to_lingo() for a in self.args)
        if arg_str:
            return f"{self.obj.to_lingo()}.{self.method}({arg_str})"
        return f"{self.obj.to_lingo()}.{self.method}()"


@dataclass
class MemberAccess(Node):
    obj: Node = field(default_factory=Node)
    prop: str = ""

    def to_lingo(self, indent: int = 0) -> str:
        return f"the {self.prop} of {self.obj.to_lingo()}"


@dataclass
class ChunkExpr(Node):
    """Chunk expression: char/word/item/line X of Y"""

    chunk_type: str = ""  # "char", "word", "item", "line"
    start: Node = field(default_factory=Node)
    end: Node | None = None
    source: Node = field(default_factory=Node)

    def to_lingo(self, indent: int = 0) -> str:
        if self.end:
            return f"{self.chunk_type} {self.start.to_lingo()} to {self.end.to_lingo()} of {self.source.to_lingo()}"
        return f"{self.chunk_type} {self.start.to_lingo()} of {self.source.to_lingo()}"


@dataclass
class TheExpr(Node):
    """The-expression: the <property>"""

    prop: str = ""

    def to_lingo(self, indent: int = 0) -> str:
        return f"the {self.prop}"


@dataclass
class SpriteRef(Node):
    sprite_num: Node = field(default_factory=Node)

    def to_lingo(self, indent: int = 0) -> str:
        return f"sprite {self.sprite_num.to_lingo()}"


# ---------------------------------------------------------------------------
# Statements
# ---------------------------------------------------------------------------


@dataclass
class AssignStmt(Node):
    target: Node = field(default_factory=Node)
    value: Node = field(default_factory=Node)

    def to_lingo(self, indent: int = 0) -> str:
        pad = "  " * indent
        return f"{pad}set {self.target.to_lingo()} = {self.value.to_lingo()}"


@dataclass
class PutStmt(Node):
    """put <expr> into/before/after <target>"""

    mode: str = "into"  # "into", "before", "after"
    value: Node = field(default_factory=Node)
    target: Node = field(default_factory=Node)

    def to_lingo(self, indent: int = 0) -> str:
        pad = "  " * indent
        return f"{pad}put {self.value.to_lingo()} {self.mode} {self.target.to_lingo()}"


@dataclass
class CallStmt(Node):
    call: Node = field(default_factory=FunctionCall)  # FunctionCall or MethodCall

    def to_lingo(self, indent: int = 0) -> str:
        pad = "  " * indent
        return f"{pad}{self.call.to_lingo()}"


@dataclass
class ReturnStmt(Node):
    value: Node | None = None

    def to_lingo(self, indent: int = 0) -> str:
        pad = "  " * indent
        if self.value:
            return f"{pad}return {self.value.to_lingo()}"
        return f"{pad}return"


@dataclass
class GlobalStmt(Node):
    names: list[str] = field(default_factory=list)

    def to_lingo(self, indent: int = 0) -> str:
        pad = "  " * indent
        return f"{pad}global {', '.join(self.names)}"


@dataclass
class IfStmt(Node):
    condition: Node = field(default_factory=Node)
    then_body: list[Node] = field(default_factory=list)
    else_body: list[Node] = field(default_factory=list)

    def to_lingo(self, indent: int = 0) -> str:
        pad = "  " * indent
        lines = [f"{pad}if {self.condition.to_lingo()} then"]
        for stmt in self.then_body:
            lines.append(stmt.to_lingo(indent + 1))
        if self.else_body:
            lines.append(f"{pad}else")
            for stmt in self.else_body:
                lines.append(stmt.to_lingo(indent + 1))
        lines.append(f"{pad}end if")
        return "\n".join(lines)


@dataclass
class RepeatWhileStmt(Node):
    condition: Node = field(default_factory=Node)
    body: list[Node] = field(default_factory=list)

    def to_lingo(self, indent: int = 0) -> str:
        pad = "  " * indent
        lines = [f"{pad}repeat while {self.condition.to_lingo()}"]
        for stmt in self.body:
            lines.append(stmt.to_lingo(indent + 1))
        lines.append(f"{pad}end repeat")
        return "\n".join(lines)


@dataclass
class RepeatWithStmt(Node):
    var: str = ""
    start: Node = field(default_factory=Node)
    end: Node = field(default_factory=Node)
    body: list[Node] = field(default_factory=list)

    def to_lingo(self, indent: int = 0) -> str:
        pad = "  " * indent
        lines = [f"{pad}repeat with {self.var} = {self.start.to_lingo()} to {self.end.to_lingo()}"]
        for stmt in self.body:
            lines.append(stmt.to_lingo(indent + 1))
        lines.append(f"{pad}end repeat")
        return "\n".join(lines)


@dataclass
class TellStmt(Node):
    target: Node = field(default_factory=Node)
    body: list[Node] = field(default_factory=list)

    def to_lingo(self, indent: int = 0) -> str:
        pad = "  " * indent
        lines = [f"{pad}tell {self.target.to_lingo()}"]
        for stmt in self.body:
            lines.append(stmt.to_lingo(indent + 1))
        lines.append(f"{pad}end tell")
        return "\n".join(lines)


# ---------------------------------------------------------------------------
# Handler (top-level function/event)
# ---------------------------------------------------------------------------


@dataclass
class HandlerNode(Node):
    name: str = ""
    args: list[str] = field(default_factory=list)
    body: list[Node] = field(default_factory=list)

    def to_lingo(self, indent: int = 0) -> str:
        pad = "  " * indent
        args_str = ", ".join(self.args)
        if args_str:
            lines = [f"{pad}on {self.name} {args_str}"]
        else:
            lines = [f"{pad}on {self.name}"]
        for stmt in self.body:
            lines.append(stmt.to_lingo(indent + 1))
        lines.append(f"{pad}end")
        return "\n".join(lines)


# ---------------------------------------------------------------------------
# Script (collection of handlers)
# ---------------------------------------------------------------------------


@dataclass
class ScriptNode(Node):
    handlers: list[HandlerNode] = field(default_factory=list)

    def to_lingo(self, indent: int = 0) -> str:
        return "\n\n".join(h.to_lingo(indent) for h in self.handlers)
