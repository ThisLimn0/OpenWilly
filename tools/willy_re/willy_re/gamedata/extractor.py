"""Game data extraction: parts, missions, maps, objects, worlds.

Reads Field cast members from the data CXT file (e.g. CDDATA.CXT),
parses their Lingo property list contents, and organizes them by
type (parts, missions, etc.) using name-prefix matching.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass, field
from typing import Any

from ..director.chunks import CastType
from ..director.parser import CastMember, DirectorFile
from ..lingo.listparser import parse_lingo_list

log = logging.getLogger(__name__)

# Name prefix → data category
DATA_PREFIXES: dict[str, str] = {
    "part": "parts",
    "mission": "missions",
    "object": "objects",
    "map": "maps",
    "world": "worlds",
}

# ID field names per category (for building hash maps)
ID_FIELDS: dict[str, str] = {
    "parts": "partId",
    "missions": "missionId",
    "objects": "objectId",
    "maps": "mapId",
    "worlds": "worldId",
}


@dataclass
class GameData:
    """Extracted and parsed game data from a cast file."""

    parts: list[dict[str, Any]] = field(default_factory=list)
    missions: list[dict[str, Any]] = field(default_factory=list)
    objects: list[dict[str, Any]] = field(default_factory=list)
    maps: list[dict[str, Any]] = field(default_factory=list)
    worlds: list[dict[str, Any]] = field(default_factory=list)

    # Hash maps keyed by the type-specific ID field
    parts_by_id: dict[Any, dict] = field(default_factory=dict)
    missions_by_id: dict[Any, dict] = field(default_factory=dict)
    objects_by_id: dict[Any, dict] = field(default_factory=dict)
    maps_by_id: dict[Any, dict] = field(default_factory=dict)
    worlds_by_id: dict[Any, dict] = field(default_factory=dict)


def extract_game_data(dir_file: DirectorFile) -> GameData:
    """Extract game data from a parsed Director file.

    Looks for Field/Text cast members whose names start with known
    prefixes (part*, mission*, object*, map*, world*), reads their
    text content, parses it as Lingo property lists, and categorizes
    the results.
    """
    data = GameData()

    for member in dir_file.all_members():
        if member.cast_type not in (CastType.FIELD, CastType.TEXT):
            continue
        if not member.name:
            continue

        # Determine category by name prefix
        category = None
        for prefix, cat in DATA_PREFIXES.items():
            if member.name.lower().startswith(prefix):
                category = cat
                break

        if not category:
            continue

        # Read the text content from the linked STXT chunk
        text = _read_stxt(dir_file, member)
        if not text:
            continue

        # Parse the Lingo property list
        try:
            parsed = parse_lingo_list(text)
        except Exception as e:
            log.warning("Failed to parse %s (%s): %s", member.name, category, e)
            continue

        if not isinstance(parsed, dict):
            log.debug("Non-dict result for %s: %s", member.name, type(parsed))
            continue

        # Add to appropriate category
        target_list = getattr(data, category)
        target_list.append(parsed)

        # Build hash map
        id_field = ID_FIELDS.get(category, "")
        if id_field and id_field in parsed:
            target_hash = getattr(data, f"{category}_by_id")
            target_hash[parsed[id_field]] = parsed

        log.debug("Extracted %s: %s", category, member.name)

    log.info(
        "Game data: %d parts, %d missions, %d objects, %d maps, %d worlds",
        len(data.parts),
        len(data.missions),
        len(data.objects),
        len(data.maps),
        len(data.worlds),
    )
    return data


def _read_stxt(dir_file: DirectorFile, member: CastMember) -> str | None:
    """Read the STXT text content linked to a cast member."""
    from ..director.text import parse_stxt

    for slot in member.linked_entries:
        if slot >= len(dir_file.entries):
            continue
        entry = dir_file.entries[slot]
        if entry.type != "STXT":
            continue

        raw = dir_file.get_entry_data(slot)
        result = parse_stxt(raw)
        return result.text if result.text else None

    return None
