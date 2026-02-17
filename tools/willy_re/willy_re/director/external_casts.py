"""External cast file loader for Director projects.

Director files can reference external cast files (.CXT, .CST) that
contain additional cast members (bitmaps, sounds, scripts, etc.).
The MCsL (Cast Library List) chunk defines which external casts are
used and their file paths.

This module provides lazy-loading of external cast files so that
cross-file references can be resolved during analysis.
"""

from __future__ import annotations

import logging
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .parser import CastMember, DirectorFile

log = logging.getLogger(__name__)


def find_external_casts(dir_file: "DirectorFile") -> list[tuple[str, Path]]:
    """Find external cast files referenced by a Director file.

    Searches for cast library references in the parsed data and
    resolves their file paths relative to the main file.

    Returns a list of (library_name, resolved_path) tuples.
    """
    results: list[tuple[str, Path]] = []
    game_dir = dir_file.path.parent

    for lib in dir_file.cast_libraries:
        if lib.path:
            # External cast file path from MCsL
            cast_path = _resolve_cast_path(game_dir, lib.path)
            if cast_path and cast_path.exists():
                results.append((lib.name, cast_path))
                log.debug("Found external cast: %s -> %s", lib.name, cast_path)
            else:
                log.warning("External cast not found: %s (path: %s)", lib.name, lib.path)

    return results


def load_external_casts(
    dir_file: "DirectorFile",
) -> dict[str, "DirectorFile"]:
    """Load all external cast files referenced by a Director file.

    Returns a dict mapping library name to parsed DirectorFile instances.
    Lazy: only loads files that exist and haven't been loaded yet.
    """
    from .parser import DirectorFile as DFClass

    loaded: dict[str, "DirectorFile"] = {}

    for lib_name, cast_path in find_external_casts(dir_file):
        try:
            ext = DFClass(cast_path)
            ext.parse()
            loaded[lib_name] = ext
            log.info(
                "Loaded external cast: %s (%d members)",
                lib_name,
                sum(len(l.members) for l in ext.cast_libraries),
            )
        except Exception as e:
            log.warning("Failed to load external cast %s: %s", cast_path, e)

    return loaded


def resolve_external_member(
    member_id: int,
    lib_index: int,
    dir_file: "DirectorFile",
    external_casts: dict[str, "DirectorFile"],
) -> "CastMember | None":
    """Resolve a cast member that might be in an external cast.

    Parameters
    ----------
    member_id : int
        The cast member number.
    lib_index : int
        The cast library index (0 = internal, 1+ = external).
    dir_file : DirectorFile
        The main Director file.
    external_casts : dict
        Previously loaded external casts.
    """
    if lib_index == 0 or lib_index > len(dir_file.cast_libraries):
        # Internal cast
        for lib in dir_file.cast_libraries:
            member = lib.members.get(member_id)
            if member:
                return member
        return None

    # External cast
    lib = (
        dir_file.cast_libraries[lib_index - 1]
        if lib_index <= len(dir_file.cast_libraries)
        else None
    )
    if lib and lib.name in external_casts:
        ext_file = external_casts[lib.name]
        for ext_lib in ext_file.cast_libraries:
            member = ext_lib.members.get(member_id)
            if member:
                return member

    return None


def _resolve_cast_path(game_dir: Path, cast_ref: str) -> Path | None:
    """Resolve a cast file path reference to an actual file.

    Director stores paths in various formats (Mac OS 9 colons,
    Windows backslashes, relative or absolute). This normalises
    them and searches common locations via case-insensitive
    directory browsing — no hardcoded subdirectory names.
    """
    # Normalise separators
    normalized = cast_ref.replace(":", "/").replace("\\", "/")
    # Take just the filename if it's a full path
    filename = Path(normalized).name
    filename_upper = filename.upper()

    # 1. Direct match (case-insensitive) in game_dir
    for child in game_dir.iterdir():
        if child.is_file() and child.name.upper() == filename_upper:
            return child

    # 2. Try the normalised relative path
    candidate = game_dir / normalized
    if candidate.exists():
        return candidate

    # 3. Scan all immediate subdirectories (case-insensitive)
    for subdir in game_dir.iterdir():
        if subdir.is_dir():
            for child in subdir.iterdir():
                if child.is_file() and child.name.upper() == filename_upper:
                    return child

    return None
