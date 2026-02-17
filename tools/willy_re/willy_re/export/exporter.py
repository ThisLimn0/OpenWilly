"""Export pipeline: JSON metadata, PNG/WAV assets, and cross-reference index.

Provides functions to export all parsed data from a Director file into
a structured output directory:

  <output>/
    metadata.json          - File summary + all cast member metadata
    assets/
      bitmaps/             - Extracted PNGs
      sounds/              - Extracted WAVs
      texts/               - Extracted text fields
      palettes/            - Extracted palette files
    gamedata/
      parts.json
      missions.json
      objects.json
      maps.json
      worlds.json
    scripts/
      <handler>.lingo      - Decompiled Lingo scripts
    score/
      score.json           - Score timeline data
      labels.json          - Frame labels
    xref.json              - Cross-reference index
"""

from __future__ import annotations

import json
import logging
from pathlib import Path
from typing import Any

from ..director.bitmap import bitd_to_image
from ..director.chunks import CAST_TYPE_NAMES, CastType
from ..director.ink import INK_NAMES
from ..director.parser import CastMember, DirectorFile
from ..director.sound import extract_snds_wav, extract_snd_wav, save_wav
from ..director.text import parse_stxt

log = logging.getLogger(__name__)


def export_all(
    dir_file: DirectorFile,
    output_dir: Path,
    *,
    export_bitmaps: bool = True,
    export_sounds: bool = True,
    export_texts: bool = True,
    export_scripts: bool = True,
    export_score: bool = True,
    export_gamedata: bool = True,
) -> dict[str, Any]:
    """Export everything from a parsed Director file.

    Returns a cross-reference index mapping member names/IDs to exported files.
    """
    output_dir.mkdir(parents=True, exist_ok=True)
    xref: dict[str, Any] = {}

    # 0. Load external cast libraries (if any)
    from ..director.external_casts import load_external_casts

    try:
        dir_file.external_casts = load_external_casts(dir_file)
        if dir_file.external_casts:
            log.info(
                "Loaded %d external cast(s): %s",
                len(dir_file.external_casts),
                list(dir_file.external_casts.keys()),
            )
    except Exception as e:
        log.warning("Failed to load external casts: %s", e)

    # 1. Metadata
    summary = dir_file.summary()
    _write_json(output_dir / "metadata.json", summary)

    # 2. Assets
    if export_bitmaps:
        _export_bitmaps(dir_file, output_dir / "assets" / "bitmaps", xref)

    if export_sounds:
        _export_sounds(dir_file, output_dir / "assets" / "sounds", xref)

    if export_texts:
        _export_texts(dir_file, output_dir / "assets" / "texts", xref)

    # 3. Scripts (Lingo decompilation)
    if export_scripts:
        _export_scripts(dir_file, output_dir / "scripts", xref)

    # 4. Score
    if export_score:
        _export_score(dir_file, output_dir / "score", xref)

    # 5. Game data
    if export_gamedata:
        _export_gamedata(dir_file, output_dir / "gamedata", xref)

    # 6. Cross-reference
    _write_json(output_dir / "xref.json", xref)

    # 7. External cast assets
    for ext_name, ext_file in dir_file.external_casts.items():
        ext_dir = output_dir / "external" / _safe_filename(ext_name)
        try:
            if export_bitmaps:
                _export_bitmaps(ext_file, ext_dir / "assets" / "bitmaps", xref)
            if export_sounds:
                _export_sounds(ext_file, ext_dir / "assets" / "sounds", xref)
            if export_texts:
                _export_texts(ext_file, ext_dir / "assets" / "texts", xref)
        except Exception as e:
            log.warning("Failed to export external cast '%s': %s", ext_name, e)

    log.info("Export complete: %s", output_dir)
    return xref


# ---------------------------------------------------------------------------
# Bitmap export
# ---------------------------------------------------------------------------


def _export_bitmaps(dir_file: DirectorFile, out_dir: Path, xref: dict) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)

    for lib in dir_file.cast_libraries:
        for num, member in lib.members.items():
            if member.cast_type != CastType.BITMAP:
                continue
            if member.image_width <= 0 or member.image_height <= 0:
                continue

            # Find linked BITD
            for slot in member.linked_entries:
                if slot >= len(dir_file.entries):
                    continue
                entry = dir_file.entries[slot]
                if entry.type != "BITD":
                    continue

                name = member.name or str(num)
                filename = f"{lib.name}_{num}_{name}.png"
                filepath = out_dir / _safe_filename(filename)

                try:
                    with open(dir_file.path, "rb") as f:
                        # Get palette from cast member if available
                        palette = _resolve_palette(dir_file, lib.name, member.image_palette)

                        img = bitd_to_image(
                            f,
                            entry.data_offset,
                            entry.data_length,
                            member.image_width,
                            member.image_height,
                            member.image_bit_depth,
                            palette=palette,
                            palette_id=member.image_palette,
                            is_windows=dir_file.little_endian,
                        )
                        if img:
                            img.save(str(filepath), "PNG")
                            xref[f"bitmap:{lib.name}/{num}"] = {
                                "file": str(filepath.relative_to(filepath.parent.parent.parent)),
                                "name": member.name,
                                "reg_x": member.image_reg_x,
                                "reg_y": member.image_reg_y,
                                "width": member.image_width,
                                "height": member.image_height,
                            }
                            log.debug("Exported bitmap: %s", filepath.name)
                except Exception as e:
                    log.warning("Failed to export bitmap %s/%d: %s", lib.name, num, e)


# ---------------------------------------------------------------------------
# Sound export
# ---------------------------------------------------------------------------


def _export_sounds(dir_file: DirectorFile, out_dir: Path, xref: dict) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)

    for lib in dir_file.cast_libraries:
        for num, member in lib.members.items():
            if member.cast_type != CastType.SOUND:
                continue
            if member.sound_sample_rate <= 0:
                continue

            for slot in member.linked_entries:
                if slot >= len(dir_file.entries):
                    continue
                entry = dir_file.entries[slot]

                name = member.name or str(num)
                filename = f"{lib.name}_{num}_{name}.wav"
                filepath = out_dir / _safe_filename(filename)

                try:
                    with open(dir_file.path, "rb") as f:
                        if entry.type == "sndS":
                            wav = extract_snds_wav(
                                f,
                                entry.data_offset,
                                entry.data_length,
                                member.sound_sample_rate,
                                channels=member.sound_channels or 1,
                                sample_width=max(1, (member.sound_sample_size or 8) // 8),
                            )
                            save_wav(wav, filepath)
                        elif entry.type == "snd " and entry.data_length > 0:
                            wav = extract_snd_wav(
                                f,
                                entry.data_offset,
                                entry.data_length,
                                member.sound_sample_rate,
                                member.sound_sample_size,
                                member.sound_data_length,
                            )
                            save_wav(wav, filepath)

                    xref[f"sound:{lib.name}/{num}"] = {
                        "file": str(filepath.relative_to(filepath.parent.parent.parent)),
                        "name": member.name,
                        "sample_rate": member.sound_sample_rate,
                        "looped": member.sound_looped,
                        "cue_points": [(off, name) for off, name in member.sound_cue_points],
                    }
                except Exception as e:
                    log.warning("Failed to export sound %s/%d: %s", lib.name, num, e)


# ---------------------------------------------------------------------------
# Text export
# ---------------------------------------------------------------------------


def _export_texts(dir_file: DirectorFile, out_dir: Path, xref: dict) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)

    for lib in dir_file.cast_libraries:
        for num, member in lib.members.items():
            if member.cast_type not in (CastType.FIELD, CastType.TEXT):
                continue

            for slot in member.linked_entries:
                if slot >= len(dir_file.entries):
                    continue
                entry = dir_file.entries[slot]
                if entry.type != "STXT":
                    continue

                name = member.name or str(num)
                filename = f"{lib.name}_{num}_{name}.txt"
                filepath = out_dir / _safe_filename(filename)

                try:
                    raw = dir_file.get_entry_data(slot)
                    result = parse_stxt(raw)
                    text = result.text

                    filepath.write_text(text, encoding="utf-8")

                    # Export style runs with resolved font names
                    style_data = []
                    for srun in result.styles:
                        font_name = None
                        if hasattr(dir_file, "resolve_font"):
                            font_name = dir_file.resolve_font(srun.font_id)
                        style_data.append(
                            {
                                "start": srun.start_offset,
                                "font_id": srun.font_id,
                                "font_name": font_name or f"font_{srun.font_id}",
                                "size": srun.font_size,
                                "style_flags": srun.style_flags,
                                "color": [srun.color_r, srun.color_g, srun.color_b],
                            }
                        )

                    xref_entry: dict[str, Any] = {
                        "file": str(filepath.relative_to(filepath.parent.parent.parent)),
                        "name": member.name,
                        "length": len(text),
                        "style_runs": len(result.styles),
                    }
                    if style_data:
                        # Also write a JSON sidecar with style info
                        style_path = filepath.with_suffix(".styles.json")
                        _write_json(style_path, style_data)
                        xref_entry["styles_file"] = str(
                            style_path.relative_to(style_path.parent.parent.parent)
                        )
                    xref[f"text:{lib.name}/{num}"] = xref_entry
                except Exception as e:
                    log.warning("Failed to export text %s/%d: %s", lib.name, num, e)


# ---------------------------------------------------------------------------
# Script (Lingo) export
# ---------------------------------------------------------------------------


def _export_scripts(dir_file: DirectorFile, out_dir: Path, xref: dict) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)

    from ..lingo.bytecode import parse_lscr
    from ..lingo.decompiler import decompile_script

    # Use the name table already parsed on DirectorFile
    names = dir_file.name_table

    # Build a map from file-entry slot → (lib_name, num, member_name) for naming
    member_by_slot: dict[int, tuple[str, int, str]] = {}
    for lib in dir_file.cast_libraries:
        for num, member in lib.members.items():
            if member.cast_type != CastType.SCRIPT:
                continue
            label = (lib.name, num, member.name or str(num))
            member_by_slot[member.file_slot] = label
            for slot in member.linked_entries:
                member_by_slot[slot] = label

    # Iterate all Lscr entries directly (they are not in KEY*)
    for idx, entry in enumerate(dir_file.entries):
        if entry.type != "Lscr":
            continue

        lib_name, num, name = member_by_slot.get(idx, ("Internal", idx, str(idx)))
        filename = f"{lib_name}_{num}_{name}.lingo"
        filepath = out_dir / _safe_filename(filename)

        try:
            data = dir_file.get_entry_data(idx)
            script = parse_lscr(data, names)
            source = decompile_script(script)

            # Prepend script type annotation from LctX if available
            header_lines = []
            if hasattr(dir_file, "script_cast_map") and dir_file.script_cast_map:
                for ctx in dir_file.script_contexts:
                    if ctx.cast_id == num or (
                        ctx.id < len(dir_file.script_contexts)
                        and member_by_slot.get(idx, (None,))[1:2] == (num,)
                    ):
                        _STYPES = {1: "Movie Script", 3: "Score Script", 7: "Parent Script"}
                        stype = _STYPES.get(ctx.type, f"Script (type {ctx.type})")
                        header_lines.append(f"-- {stype}")
                        if name and name != str(num):
                            header_lines.append(f'-- Cast member: "{name}"')
                        break
            if not header_lines and not dir_file.name_table:
                header_lines.append("-- WARNING: name table unavailable, using fallback names")
            if header_lines:
                source = "\n".join(header_lines) + "\n\n" + source

            filepath.write_text(source, encoding="utf-8")
            xref[f"script:{lib_name}/{num}"] = {
                "file": str(filepath.relative_to(filepath.parent.parent)),
                "name": name,
                "handlers": [h.name for h in script.handlers],
            }
            log.debug("Decompiled script: %s", filepath.name)
        except Exception as e:
            log.warning("Failed to decompile script Lscr@%d: %s", idx, e)


# ---------------------------------------------------------------------------
# Score export
# ---------------------------------------------------------------------------


def _export_score(dir_file: DirectorFile, out_dir: Path, xref: dict) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)

    from ..director.labels import parse_vwlb
    from ..director.score import parse_vwsc

    # VWSC
    # Build cast_id → name mapping for resolving sprite references
    # Include all members (named or unnamed with type fallback)
    cast_name_map: dict[int, str] = {}
    for lib in dir_file.cast_libraries:
        for num, member in lib.members.items():
            if member.name:
                cast_name_map[num] = member.name
            else:
                type_name = CAST_TYPE_NAMES.get(member.cast_type, "unknown")
                cast_name_map[num] = f"{type_name}_{num}"
    # Also include external cast members
    for ext_name, ext_df in dir_file.external_casts.items():
        for elib in ext_df.cast_libraries:
            for num, member in elib.members.items():
                if num not in cast_name_map:
                    cast_name_map[num] = member.name if member.name else f"ext:{ext_name}/{num}"

    for entry in dir_file.entries:
        if entry.type == "VWSC":
            try:
                data = dir_file.get_entry_data(entry.id)
                score = parse_vwsc(data)
                score_data = {
                    "total_frames": score.total_frames,
                    "channels_per_frame": score.channels_per_frame,
                    "frames": [
                        {
                            "frame": f.frame_num,
                            "tempo": f.tempo,
                            "script_id": f.script_id,
                            "sprites": [
                                {
                                    "channel": s.channel_id,
                                    "cast_id": s.cast_id,
                                    "cast_name": cast_name_map.get(s.cast_id, ""),
                                    "sprite_type": s.sprite_type,
                                    "script_id": s.script_id,
                                    "x": s.start_x,
                                    "y": s.start_y,
                                    "w": s.width,
                                    "h": s.height,
                                    "end_x": s.end_x,
                                    "end_y": s.end_y,
                                    "ink": s.ink,
                                    "ink_name": INK_NAMES.get(s.ink, f"Ink({s.ink})"),
                                    "blend": s.blend,
                                    "fore_color": s.fore_color,
                                    "back_color": s.back_color,
                                }
                                for s in f.sprites
                            ],
                        }
                        for f in score.frames
                    ],
                }
                _write_json(out_dir / "score.json", score_data)
                xref["score"] = {"frames": score.total_frames}
            except Exception as e:
                log.warning("Failed to export score: %s", e)
            break

    # VWtk — tempo/timing data
    if dir_file.tempo_data:
        try:
            tempo_data = [
                {
                    "frame": t.frame,
                    "tempo": t.tempo,
                    "wait_type": t.wait_type,
                    "wait_time": t.wait_time,
                    "channel": t.channel,
                    "cue_point": t.cue_point,
                }
                for t in dir_file.tempo_data
            ]
            _write_json(out_dir / "tempo.json", tempo_data)
            xref["tempo"] = {"entries": len(dir_file.tempo_data)}
        except Exception as e:
            log.warning("Failed to export tempo data: %s", e)

    # SCRF — score frame references
    if dir_file.score_frame_refs:
        try:
            scrf_data = [
                {
                    "frame": r.frame,
                    "cast_id": r.cast_id,
                    "cast_name": cast_name_map.get(r.cast_id, ""),
                    "ref_type": r.ref_type,
                }
                for r in dir_file.score_frame_refs
            ]
            _write_json(out_dir / "frame_refs.json", scrf_data)
            xref["frame_refs"] = {"entries": len(dir_file.score_frame_refs)}
        except Exception as e:
            log.warning("Failed to export frame references: %s", e)

    # VWLB
    for entry in dir_file.entries:
        if entry.type == "VWLB":
            try:
                data = dir_file.get_entry_data(entry.id)
                labels = parse_vwlb(data)
                labels_data = [{"frame": l.frame, "name": l.name} for l in labels]
                _write_json(out_dir / "labels.json", labels_data)
                xref["labels"] = {"count": len(labels)}
            except Exception as e:
                log.warning("Failed to export labels: %s", e)
            break


# ---------------------------------------------------------------------------
# Game data export
# ---------------------------------------------------------------------------


def _export_gamedata(dir_file: DirectorFile, out_dir: Path, xref: dict) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)

    from ..gamedata.extractor import extract_game_data

    try:
        gd = extract_game_data(dir_file)

        for cat in ("parts", "missions", "objects", "maps", "worlds"):
            items = getattr(gd, cat)
            items_by_id = getattr(gd, f"{cat}_by_id")
            if items:
                _write_json(out_dir / f"{cat}.json", items)
                _write_json(out_dir / f"{cat}_by_id.json", items_by_id)
                xref[f"gamedata:{cat}"] = {"count": len(items)}
    except Exception as e:
        log.warning("Failed to export game data: %s", e)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _write_json(path: Path, data: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2, ensure_ascii=False, default=str), encoding="utf-8")


def _resolve_palette(
    dir_file: DirectorFile,
    lib_name: str,
    palette_id: int,
) -> list[tuple[int, int, int]] | None:
    """Resolve a palette for a bitmap member.

    Checks local libraries, then external casts, for a palette
    member matching *palette_id*.  Returns ``None`` for system
    palette IDs (≤ 0) — those are handled inside ``bitmap.py``.
    """
    if palette_id <= 0:
        return None

    # Same library first
    pal_member = dir_file.get_member(lib_name, palette_id)
    if pal_member and pal_member.palette_data:
        return pal_member.palette_data

    # Any library in the same file
    for lib in dir_file.cast_libraries:
        pm = lib.members.get(palette_id)
        if pm and pm.palette_data:
            return pm.palette_data

    # External casts
    for _ext_name, ext_df in dir_file.external_casts.items():
        for elib in ext_df.cast_libraries:
            pm = elib.members.get(palette_id)
            if pm and pm.palette_data:
                return pm.palette_data

    return None


def _safe_filename(name: str) -> str:
    """Sanitize a filename, replacing unsafe characters."""
    return "".join(c if c.isalnum() or c in "._-" else "_" for c in name)
