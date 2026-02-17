"""CLI entry point for willy-re.

Usage:
    willy-re parse <file>             Parse a Director file and print summary
    willy-re extract <file> -o <dir>  Full extract (assets, scripts, game data)
    willy-re info <dir>               Detect game edition from a directory
    willy-re list <file>              List all cast members
    willy-re decompile <file>         Decompile all Lingo scripts
    willy-re gamedata <file>          Extract game data (parts, missions, etc.)
    willy-re score <file>             Extract Score timeline + labels
    willy-re lingo-parse <text>       Parse a Lingo property list literal
"""

from __future__ import annotations

import json
import logging
import sys
from pathlib import Path

import click

from . import __version__


@click.group()
@click.version_option(__version__)
@click.option("-v", "--verbose", is_flag=True, help="Enable debug logging")
def main(verbose: bool) -> None:
    """Willy Werkel / Mulle Meck Director game reverse engineering toolkit."""
    level = logging.DEBUG if verbose else logging.INFO
    logging.basicConfig(
        level=level,
        format="%(name)s %(levelname)s: %(message)s",
    )


@main.command()
@click.argument("file", type=click.Path(exists=True))
def parse(file: str) -> None:
    """Parse a Director file and print a JSON summary."""
    from .director.parser import DirectorFile

    with DirectorFile(file) as df:
        df.parse()
        out = json.dumps(df.summary(), indent=2, ensure_ascii=False)
        sys.stdout.buffer.write(out.encode("utf-8"))
        sys.stdout.buffer.write(b"\n")


@main.command(name="list-members")
@click.argument("file", type=click.Path(exists=True))
def list_members(file: str) -> None:
    """List all cast members in a Director file."""
    from .director.parser import DirectorFile
    from .director.chunks import CAST_TYPE_NAMES

    with DirectorFile(file) as df:
        df.parse()

        for lib in df.cast_libraries:
            click.echo(f"\n=== Library: {lib.name} ({len(lib.members)} members) ===")
            for num, m in sorted(lib.members.items()):
                type_name = CAST_TYPE_NAMES.get(m.cast_type, f"Type({m.cast_type})")
                extra = ""
                if m.cast_type == 1:  # bitmap
                    extra = f"  {m.image_width}x{m.image_height} @{m.image_bit_depth}bpp"
                elif m.cast_type == 6:  # sound
                    extra = (
                        f"  {m.sound_sample_rate}Hz {m.sound_channels}ch {m.sound_sample_size}bit"
                    )
                elif m.cast_type == 2 and m.film_loop_data:  # film loop
                    extra = f"  {m.film_loop_data.total_frames} frames"
                elif m.cast_type == 7 and m.button_data:  # button
                    extra = f"  {m.button_data.label!r}"
                elif m.cast_type == 8 and m.shape_data:  # shape
                    from .director.cast_types import SHAPE_TYPE_NAMES

                    extra = f"  {SHAPE_TYPE_NAMES.get(m.shape_data.shape_type, '?')}"
                elif m.cast_type == 10 and m.digital_video_data:  # digital video
                    extra = f"  {m.digital_video_data.video_type} {m.digital_video_data.filename}"
                elif m.cast_type == 14 and m.transition_data:  # transition
                    extra = f"  {m.transition_data.name}"
                click.echo(f"  [{num:4d}] {type_name:12s}  {m.name}{extra}")


# Alias: `willy-re list` works the same as `willy-re list-members`
main.add_command(list_members, name="list")


@main.command()
@click.argument("file", type=click.Path(exists=True))
@click.option(
    "-o",
    "--output",
    type=click.Path(),
    default=None,
    help="Output directory (default: <file>_export/)",
)
@click.option("--no-bitmaps", is_flag=True, help="Skip bitmap extraction")
@click.option("--no-sounds", is_flag=True, help="Skip sound extraction")
@click.option("--no-texts", is_flag=True, help="Skip text extraction")
@click.option("--no-scripts", is_flag=True, help="Skip Lingo decompilation")
@click.option("--no-score", is_flag=True, help="Skip Score extraction")
@click.option("--no-gamedata", is_flag=True, help="Skip game data extraction")
def extract(
    file: str,
    output: str | None,
    no_bitmaps: bool,
    no_sounds: bool,
    no_texts: bool,
    no_scripts: bool,
    no_score: bool,
    no_gamedata: bool,
) -> None:
    """Extract everything from a Director file."""
    from .director.parser import DirectorFile
    from .export.exporter import export_all

    with DirectorFile(file) as df:
        df.parse()

        out_dir = Path(output) if output else Path(file).with_suffix("") / "_export"

        xref = export_all(
            df,
            out_dir,
            export_bitmaps=not no_bitmaps,
            export_sounds=not no_sounds,
            export_texts=not no_texts,
            export_scripts=not no_scripts,
            export_score=not no_score,
            export_gamedata=not no_gamedata,
        )

    click.echo(f"Exported to {out_dir}")
    click.echo(f"Cross-reference entries: {len(xref)}")


@main.command()
@click.argument("dir", type=click.Path(exists=True))
def info(dir: str) -> None:
    """Detect game edition from a directory."""
    from .gamedata.detector import detect_edition, list_director_files

    game_dir = Path(dir)
    edition = detect_edition(game_dir)

    click.echo(f"Edition:    {edition.edition.value}")
    click.echo(f"Title (DE): {edition.title_de}")
    click.echo(f"Title (SV): {edition.title_sv}")
    click.echo(f"Year:       {edition.year}")
    click.echo(f"Resolution: {edition.resolution[0]}x{edition.resolution[1]}")
    click.echo(f"Colors:     {edition.color_depth}-bit")
    click.echo(f"Director:   {edition.director_version}")

    files = list_director_files(game_dir)
    if files:
        click.echo(f"\nDirector files ({len(files)}):")
        for f in files:
            click.echo(f"  {f.relative_to(game_dir)}")


@main.command()
@click.argument("file", type=click.Path(exists=True))
@click.option("-o", "--output", type=click.Path(), default=None)
def decompile(file: str, output: str | None) -> None:
    """Decompile all Lingo scripts in a Director file."""
    from .director.parser import DirectorFile

    with DirectorFile(file) as df:
        df.parse()

        out_dir = Path(output) if output else Path(file).with_suffix("") / "_scripts"
        out_dir.mkdir(parents=True, exist_ok=True)

        from .lingo.bytecode import parse_lscr
        from .lingo.decompiler import decompile_script
        from .director.chunks import CastType

        # Use the name table already parsed on DirectorFile
        names = df.name_table

        # Build a map from file-entry slot → cast member name for naming output files
        member_by_slot: dict[int, str] = {}
        for lib in df.cast_libraries:
            for num, member in lib.members.items():
                if member.cast_type == CastType.SCRIPT:
                    member_by_slot[member.file_slot] = member.name or str(num)
                    # Also register linked entry slots
                    for slot in member.linked_entries:
                        member_by_slot[slot] = member.name or str(num)

        # Iterate all Lscr entries directly (they are not in KEY*)
        count = 0
        for idx, entry in enumerate(df.entries):
            if entry.type != "Lscr":
                continue
            try:
                data = df.get_entry_data(idx)
                script = parse_lscr(data, names)
                # Try to find a name from the cast member map or use entry index
                name = member_by_slot.get(idx, str(idx))
                source = decompile_script(script)

                # Prepend script type annotation from LctX
                header_lines: list[str] = []
                if hasattr(df, "script_cast_map") and df.script_cast_map:
                    _STYPES = {1: "Movie Script", 3: "Score Script", 7: "Parent Script"}
                    for ctx in df.script_contexts:
                        slot_name = member_by_slot.get(idx)
                        if slot_name is not None:
                            stype = _STYPES.get(ctx.type, f"Script (type {ctx.type})")
                            # Match by cast_id with member slot lookup
                            for lib in df.cast_libraries:
                                for mnum, mem in lib.members.items():
                                    if ctx.cast_id == mnum and (
                                        mem.name == slot_name or str(mnum) == slot_name
                                    ):
                                        header_lines.append(f"-- {stype}")
                                        if mem.name:
                                            header_lines.append(f'-- Cast member: "{mem.name}"')
                                        break
                                if header_lines:
                                    break
                        if header_lines:
                            break
                if not header_lines and not df.name_table:
                    header_lines.append("-- WARNING: name table unavailable, using fallback names")
                if header_lines:
                    source = "\n".join(header_lines) + "\n\n" + source

                safe = "".join(c if c.isalnum() or c in "._-" else "_" for c in name)
                (out_dir / f"{safe}.lingo").write_text(source, encoding="utf-8")
                count += 1
            except Exception as e:
                click.echo(f"  Failed Lscr@{idx}: {e}", err=True)

    click.echo(f"Decompiled {count} scripts to {out_dir}")


@main.command()
@click.argument("file", type=click.Path(exists=True))
@click.option("-o", "--output", type=click.Path(), default=None)
def gamedata(file: str, output: str | None) -> None:
    """Extract game data (parts, missions, maps, etc.)."""
    from .director.parser import DirectorFile
    from .gamedata.extractor import extract_game_data

    with DirectorFile(file) as df:
        df.parse()

        data = extract_game_data(df)
        out_dir = Path(output) if output else Path(file).with_suffix("") / "_gamedata"
        out_dir.mkdir(parents=True, exist_ok=True)

        for cat in ("parts", "missions", "objects", "maps", "worlds"):
            items = getattr(data, cat)
            by_id = getattr(data, f"{cat}_by_id")
            if items:
                (out_dir / f"{cat}.json").write_text(
                    json.dumps(items, indent=2, ensure_ascii=False, default=str),
                    encoding="utf-8",
                )
                (out_dir / f"{cat}_by_id.json").write_text(
                    json.dumps(by_id, indent=2, ensure_ascii=False, default=str),
                    encoding="utf-8",
                )
                click.echo(f"  {cat}: {len(items)} entries")

    click.echo(f"Exported to {out_dir}")


@main.command()
@click.argument("file", type=click.Path(exists=True))
@click.option("-o", "--output", type=click.Path(), default=None)
def score(file: str, output: str | None) -> None:
    """Extract Score timeline and frame labels."""
    from .director.parser import DirectorFile
    from .director.score import parse_vwsc
    from .director.labels import parse_vwlb

    with DirectorFile(file) as df:
        df.parse()

        out_dir = Path(output) if output else Path(file).with_suffix("") / "_score"
        out_dir.mkdir(parents=True, exist_ok=True)

        for entry in df.entries:
            if entry.type == "VWSC":
                data = df.get_entry_data(entry.id)
                sc = parse_vwsc(data)
                click.echo(f"Score: {sc.total_frames} frames, {sc.channels_per_frame} channels")
                score_out = {
                    "total_frames": sc.total_frames,
                    "channels_per_frame": sc.channels_per_frame,
                    "frames": len(sc.frames),
                }
                # Include VWtk tempo data if parsed
                if df.tempo_data:
                    score_out["tempo_entries"] = [
                        {
                            "frame": t.frame,
                            "tempo": t.tempo,
                            "wait_type": t.wait_type,
                            "wait_time": t.wait_time,
                        }
                        for t in df.tempo_data
                    ]
                (out_dir / "score_summary.json").write_text(
                    json.dumps(score_out, indent=2), encoding="utf-8"
                )
                break

        for entry in df.entries:
            if entry.type == "VWLB":
                data = df.get_entry_data(entry.id)
                labels = parse_vwlb(data)
                click.echo(f"Labels: {len(labels)}")
                labels_out = [{"frame": l.frame, "name": l.name} for l in labels]
                (out_dir / "labels.json").write_text(
                    json.dumps(labels_out, indent=2, ensure_ascii=False), encoding="utf-8"
                )
                break


@main.command("lingo-parse")
@click.argument("text")
def lingo_parse(text: str) -> None:
    """Parse a Lingo property list literal string."""
    from .lingo.listparser import parse_lingo_list

    result = parse_lingo_list(text)
    click.echo(json.dumps(result, indent=2, ensure_ascii=False, default=str))


@main.command("batch-extract")
@click.argument("dir", type=click.Path(exists=True))
@click.option("-o", "--output", type=click.Path(), default=None)
def batch_extract(dir: str, output: str | None) -> None:
    """Extract all Director files found in a game directory."""
    from .gamedata.detector import list_director_files
    from .director.parser import DirectorFile
    from .export.exporter import export_all

    game_dir = Path(dir)
    out_base = Path(output) if output else game_dir / "_re_export"

    files = list_director_files(game_dir)
    click.echo(f"Found {len(files)} Director files")

    for f in files:
        click.echo(f"\n--- {f.name} ---")
        try:
            with DirectorFile(f) as df:
                df.parse()
                out_dir = out_base / f.stem
                export_all(df, out_dir)
                click.echo(f"  -> {out_dir}")
        except Exception as e:
            click.echo(f"  FAILED: {e}", err=True)

    click.echo(f"\nDone. Output in {out_base}")


if __name__ == "__main__":
    main()
