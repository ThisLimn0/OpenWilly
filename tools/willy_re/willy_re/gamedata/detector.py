"""Game edition detector and game data configuration.

Identifies which Willy Werkel / Mulle Meck edition a Director file belongs to,
and provides edition-specific metadata needed for extraction.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path

log = logging.getLogger(__name__)

# Canonical set of Director file extensions (lower-case, with leading dot)
DIRECTOR_EXTENSIONS: frozenset[str] = frozenset({".dxr", ".cxt", ".cst", ".dir", ".dcr"})


class Edition(str, Enum):
    """Known Willy Werkel / Mulle Meck game editions."""

    AUTOS = "autos"  # Autos Bauen (1997) - 640x480, 256 col, Dir 5/6
    SCHIFFE = "schiffe"  # Schiffe Bauen (~1999) - 640x480, 256 col, Dir 5/6
    HAEUSER = "haeuser"  # Häuser Bauen (2003) - 800x600, 16-bit, Dir 8
    FLUGZEUGE = "flugzeuge"  # Flugzeuge Bauen (~2001) - 800x600, 16-bit, Dir 7/8
    RAUMSCHIFFE = "raumschiffe"  # Raumschiffe Bauen (2005) - 800x600, 16-bit+OpenGL
    UNKNOWN = "unknown"


@dataclass
class EditionInfo:
    """Metadata for a game edition."""

    edition: Edition
    title_de: str
    title_sv: str  # Swedish (Mulle Meck) title
    year: int
    resolution: tuple[int, int]
    color_depth: int
    director_version: str
    main_cast_files: list[str] = field(default_factory=list)
    data_cast_file: str = ""  # The CXT with game data (parts, missions, etc.)


# Edition databases
EDITIONS: dict[Edition, EditionInfo] = {
    Edition.AUTOS: EditionInfo(
        edition=Edition.AUTOS,
        title_de="Willy Werkel - Autos Bauen",
        title_sv="Mulle Meck bygger bilar",
        year=1997,
        resolution=(640, 480),
        color_depth=8,
        director_version="6.0",
        main_cast_files=["00.CXT", "Startcd.dir"],
        data_cast_file="CDDATA.CXT",
    ),
    Edition.SCHIFFE: EditionInfo(
        edition=Edition.SCHIFFE,
        title_de="Willy Werkel - Schiffe Bauen",
        title_sv="Mulle Meck bygger båtar",
        year=1999,
        resolution=(640, 480),
        color_depth=8,
        director_version="6.0",
        main_cast_files=["00.CXT", "Startcd.dir"],
        data_cast_file="CDDATA.CXT",
    ),
    Edition.HAEUSER: EditionInfo(
        edition=Edition.HAEUSER,
        title_de="Willy Werkel - Häuser Bauen",
        title_sv="Mulle Meck bygger hus",
        year=2003,
        resolution=(800, 600),
        color_depth=16,
        director_version="8.0",
        main_cast_files=["00.CXT", "Startcd.dir"],
        data_cast_file="CDDATA.CXT",
    ),
    Edition.FLUGZEUGE: EditionInfo(
        edition=Edition.FLUGZEUGE,
        title_de="Willy Werkel - Flugzeuge Bauen",
        title_sv="Mulle Meck bygger flygplan",
        year=2001,
        resolution=(800, 600),
        color_depth=16,
        director_version="7.0",
        main_cast_files=["00.CXT", "Startcd.dir"],
        data_cast_file="CDDATA.CXT",
    ),
    Edition.RAUMSCHIFFE: EditionInfo(
        edition=Edition.RAUMSCHIFFE,
        title_de="Willy Werkel - Raumschiffe Bauen",
        title_sv="Mulle Meck bygger rymdskepp",
        year=2005,
        resolution=(800, 600),
        color_depth=16,
        director_version="8.5/9.0",
        main_cast_files=["00.CXT", "Startcd.dir"],
        data_cast_file="CDDATA.CXT",
    ),
}


def detect_edition(game_dir: Path) -> EditionInfo:
    """Detect the game edition from a directory of game files.

    Heuristic: look for known filenames and directory structures.
    All comparisons are case-insensitive for cross-platform robustness.
    """
    files = {f.name.upper(): f for f in game_dir.rglob("*") if f.is_file()}
    dirs = {d.name.upper(): d for d in game_dir.rglob("*") if d.is_dir()}

    # ---- Check for edition-specific markers first ----

    # Autos Bauen: has AUTOBAU.HLP or "Autos" folder
    if "AUTOBAU.HLP" in files or "AUTOBAU.CNT" in files or "AUTOS" in dirs:
        return EDITIONS[Edition.AUTOS]

    # Schiffe Bauen: has SCHIFBAU.HLP or "Schiffe" folder
    if "SCHIFBAU.HLP" in files or "SCHIFFE" in dirs:
        return EDITIONS[Edition.SCHIFFE]

    # Häuser Bauen: has HAUSBAU.HLP or "Haeuser" / "Häuser" folder
    if "HAUSBAU.HLP" in files or "HAEUSER" in dirs or "HÄUSER" in dirs:
        return EDITIONS[Edition.HAEUSER]

    # Flugzeuge Bauen: has FLUGBAU.HLP or "Flugzeuge" folder
    if "FLUGBAU.HLP" in files or "FLUGZEUGE" in dirs:
        return EDITIONS[Edition.FLUGZEUGE]

    # Raumschiffe Bauen: has RAUMBAU.HLP or "Raumschiffe" folder
    if "RAUMBAU.HLP" in files or "RAUMSCHIFFE" in dirs:
        return EDITIONS[Edition.RAUMSCHIFFE]

    # ---- Fallback: try directory name patterns ----
    dir_name = game_dir.name.lower()
    for edition_key, keywords in [
        (Edition.AUTOS, ("auto", "bilar", "cars")),
        (Edition.SCHIFFE, ("schiff", "båtar", "boats", "ships")),
        (Edition.HAEUSER, ("haus", "häuser", "haeuser", "hus", "house")),
        (Edition.FLUGZEUGE, ("flug", "flygplan", "plane")),
        (Edition.RAUMSCHIFFE, ("raum", "rymd", "space")),
    ]:
        if any(kw in dir_name for kw in keywords):
            return EDITIONS[edition_key]

    # ---- Generic: has common Director structure ----
    if "CDDATA.CXT" in files and "00.CXT" in files:
        # Looks like a Willy game but can't tell which — default to AUTOS
        log.info("Found CDDATA.CXT + 00.CXT, defaulting to AUTOS edition")
        return EDITIONS[Edition.AUTOS]

    log.warning("Could not determine game edition from %s", game_dir)
    return EditionInfo(
        edition=Edition.UNKNOWN,
        title_de="Unknown",
        title_sv="Unknown",
        year=0,
        resolution=(0, 0),
        color_depth=0,
        director_version="unknown",
    )


def list_director_files(game_dir: Path) -> list[Path]:
    """Find all Director files in a game directory (case-insensitive).

    Uses the shared ``DIRECTOR_EXTENSIONS`` set so that the accepted
    file types are defined in exactly one place.
    """
    result = []
    for f in sorted(game_dir.rglob("*")):
        if f.is_file() and f.suffix.lower() in DIRECTOR_EXTENSIONS:
            result.append(f)
    return result
