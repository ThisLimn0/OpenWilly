"""Lingo property list parser.

Pure Python port of listparser.js from mulle.js — parses Lingo literal
syntax (as found in Field cast members) into Python dicts/lists.

Supported syntax:
  - Linear lists: [1, 2, 3]
  - Property lists: [#key: value, #key2: value2]
  - Nested lists
  - Strings: "hello"
  - Symbols: #someName
  - Numbers: 42, 3.14
  - Director functions: point(x, y) → {"x": x, "y": y}
"""

from __future__ import annotations

from typing import Any


class LingoListParserError(Exception):
    pass


# Built-in Director functions that can appear in list literals
_DIRECTOR_FUNCTIONS: dict[str, Any] = {
    "point": lambda x, y: {"x": x, "y": y},
    "rect": lambda l, t, r, b: {"left": l, "top": t, "right": r, "bottom": b},
    "rgb": lambda r, g, b: {"r": r, "g": g, "b": b},
    "color": lambda r, g, b: {"r": r, "g": g, "b": b},
}


def parse_lingo_list(text: str) -> Any:
    """Parse a Lingo list/property list literal into Python objects.

    Returns a dict for property lists, a list for linear lists,
    or a scalar value (str, int, float, symbol string) for atoms.
    """
    trimmed = _trim_whitespace(text)
    return _parse_segment(trimmed)


# ---------------------------------------------------------------------------
# Internal parser
# ---------------------------------------------------------------------------


def _trim_whitespace(s: str) -> str:
    """Remove whitespace outside of string literals."""
    out = []
    in_string = False
    quote_count = 0
    for ch in s:
        if ch in (" ", "\n", "\t", "\r"):
            if not in_string:
                continue
        if ch == '"':
            in_string = not in_string
            quote_count += 1
        out.append(ch)
    if quote_count % 2 != 0:
        raise LingoListParserError("Uneven quotes detected")
    return "".join(out)


def _parse_segment(seg: str) -> Any:
    """Parse a segment that may be a list, atom, or nested structure."""
    original_len = len(seg)
    inner = _trim_outer_brackets(seg)

    if inner is None:
        return []

    has_brackets = len(inner) < original_len

    if has_brackets:
        children = _get_children(inner)
        if not children:
            return []

        is_prop = _detect_property(children[0])
        if is_prop:
            result: dict[str, Any] = {}
            for child in children:
                key = _get_property_name(child)
                val_str = _get_property_value(child)
                result[key] = _parse_segment(val_str)
            return result
        else:
            return [_parse_segment(c) for c in children]
    else:
        # Atom
        if _detect_symbol(inner):
            return inner  # keep as e.g. "#symbolName"

        typ = _get_type(inner)
        if typ == "number":
            return _parse_number(inner)
        elif typ == "string":
            return inner[1:-1]  # strip quotes
        elif typ == "function":
            return _eval_function(inner)

        # Fallback: return as-is
        return inner


def _trim_outer_brackets(s: str) -> str | None:
    """If s is wrapped in [...], return the inner content. Otherwise return s."""
    if not s:
        return None

    bracket_pairs: list[dict] = []
    current_pair = -1

    for i, ch in enumerate(s):
        if ch == "[":
            bp_id = len(bracket_pairs)
            bracket_pairs.append({"s": i, "e": None, "p": current_pair})
            current_pair = bp_id
        elif ch == "]":
            if current_pair == -1:
                raise LingoListParserError("Incorrectly nested brackets")
            pair = bracket_pairs[current_pair]
            current_pair = pair["p"]
            pair["e"] = i

    last = len(s) - 1
    for pair in bracket_pairs:
        if pair["s"] == 0 and pair["e"] == last:
            return s[1:-1]

    return s


def _get_children(s: str) -> list[str]:
    """Split a comma-separated list, respecting nesting and strings."""
    children: list[str] = []
    in_string = False
    in_function = False
    nest_level = 0
    pending = ""

    for i, ch in enumerate(s):
        if ch == "[":
            nest_level += 1
        elif ch == "]":
            nest_level -= 1
        elif ch == '"':
            in_string = not in_string
        elif ch in ("(", ")"):
            in_function = not in_function

        if ch == "," and nest_level == 0 and not in_string and not in_function:
            children.append(pending)
            pending = ""
            continue

        pending += ch

    if pending:
        children.append(pending)

    return children


def _detect_property(s: str) -> bool:
    """Check if a segment looks like a property entry (e.g., #key:value or 1:value)."""
    if not s:
        return False
    first = s[0]
    if first == "#" or first.isdigit():
        for ch in s[1:]:
            if ch == ":":
                return True
            if ch in ("[", "#", "]", ","):
                return False
    return False


def _detect_symbol(s: str) -> bool:
    """Check if s is a Lingo symbol (starts with # but has no colon)."""
    if not s or s[0] != "#":
        return False
    return ":" not in s


def _get_type(s: str) -> str:
    if not s:
        return "unknown"
    if s[0] == '"':
        return "string"
    if "(" in s and ")" in s:
        return "function"
    try:
        float(s)
        return "number"
    except ValueError:
        return "unknown"


def _parse_number(s: str) -> int | float:
    try:
        return int(s)
    except ValueError:
        return float(s)


def _eval_function(s: str) -> Any:
    """Evaluate a Director function like point(x, y)."""
    # Extract function name and args
    paren_idx = s.index("(")
    func_name = s[:paren_idx]
    args_str = s[paren_idx + 1 : -1]  # strip parens

    # Parse args (comma-separated, may be nested)
    args = _get_children(args_str)
    parsed_args = [_parse_segment(a) for a in args]

    func = _DIRECTOR_FUNCTIONS.get(func_name)
    if func:
        try:
            return func(*parsed_args)
        except TypeError:
            return {"_func": func_name, "_args": parsed_args}

    return {"_func": func_name, "_args": parsed_args}


def _get_property_name(s: str) -> str:
    """Extract the key from a property entry like #key:value."""
    name = ""
    for ch in s:
        if ch == "#":
            continue
        if ch == ":":
            break
        name += ch
    if not name:
        raise LingoListParserError(f"Invalid property name in: {s}")
    return name


def _get_property_value(s: str) -> str:
    """Extract the value part from a property entry like #key:value."""
    idx = s.index(":")
    return s[idx + 1 :]
