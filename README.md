# OpenWilly

**Open-source reimplementation of the Willy Werkel (Mulle Meck) educational game engine in Rust**

![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)
![Rust](https://img.shields.io/badge/rust-1.75%2B-orange)

## About

OpenWilly is a clean-room reimplementation of the game engine powering the classic Willy Werkel (known internationally as Mulle Meck / Gary Gadget) educational games from the late 1990s. Rather than wrapping or patching the original Macromedia Director 6 player, OpenWilly parses the original game data files (DXR/CXT casts, Director bitmaps, sounds) directly and runs them in a new Rust-based engine with a minifb-backed 640x480 renderer.

The project draws heavily on the prior reverse-engineering work done by the **mulle.js** project (a Phaser.js-based reimplementation), which served as the primary reference for game logic, scene structure, part databases, cursor behavior, physics parameters, and many other behavioral details. Without mulle.js, this project would not exist in its current form.

### Supported Games

- **Autos bauen mit Willy Werkel** (1997) -- primary development target
- **Flugzeuge bauen mit Willy Werkel**
- **Haeuser bauen mit Willy Werkel**
- **Raumschiffe bauen mit Willy Werkel**
- **Schiffe bauen mit Willy Werkel**

## Approach

OpenWilly takes a **reimplementation** approach rather than a compatibility-wrapper approach:

1. **Parse original Director data** -- DXR/CXT cast files are read directly using a custom Director 6 parser. Bitmaps, palettes, sounds, and cast metadata are extracted at runtime.
2. **Reconstruct game logic from mulle.js** -- The mulle.js project reverse-engineered the Lingo scripts and game behavior into readable JavaScript. OpenWilly translates this logic into idiomatic Rust, including scene state machines, dialog systems, car building, driving physics, and quest progression.
3. **Native rendering** -- A minifb window provides the 640x480 framebuffer. Sprites are alpha-blended and z-ordered. Nearest-neighbor scaling with optional detail noise handles arbitrary output resolutions.
4. **No original executables needed** -- The original WILLY32.EXE (Director Player) and its Xtras are not used at runtime. Only the game data files (from the original CD/ISO) are required.

### What mulle.js provided

The [mulle.js](https://github.com/niclaslindstedt/mulle) project by Niclas Lindstedt is a JavaScript/Phaser.js reimplementation of "Bygg bilar med Mulle Meck" that decoded and documented large parts of the original game's behavior. OpenWilly uses mulle.js as a behavioral specification for:

- Scene flow and state machine transitions
- Parts database structure and snap-point geometry
- Cursor types and hotspot coordinates (from style.scss)
- Junkyard pile drop rectangles and bounce-back logic
- Driving physics, engine sound state machines, and fuel mechanics
- Destination scripts, NPC dialog chains, and quest flag conventions
- Car show rating formulas and animation sequences
- Dashboard HUD layout (fuel needle frames, speedometer z-ordering)
- Morph/snap preview switching (junkView to UseView within 40px)
- Gravity constants (800 px/s^2) and collision floors

All game logic was rewritten from scratch in Rust; no JavaScript code was translated line-by-line.

## Architecture

```
openwilly-player       Standalone game engine (minifb + rodio)
  src/
    engine/            Renderer, font, sound engine, icon loader
    game/              Game logic modules:
      mod.rs           Central GameState, scene switching, update loop
      scenes.rs        SceneHandler -- DXR loading, buttons, hotspots
      build_car.rs     Car assembly, snap points, road-legality checks
      driving.rs       DriveCar physics, engine sounds, mouse/key steering
      drag_drop.rs     Drag-and-drop engine, snap preview, part physics
      cursor.rs        Software cursor (9 types, stack-based management)
      dashboard.rs     Fuel needle + speedometer HUD
      toolbox.rs       Driving-view popup menu (5 buttons)
      dialog.rs        Subtitle rendering, quest flags, mission database
      scene_script.rs  Data-driven dialog/animation script chains
      dev_menu.rs      Hidden developer menu (5x # activation)
      parts_db.rs      Part properties, weights, categories
      save.rs          Save/load game state
    assets/            Director file parser, bitmap decoder, palette module
    director/          DXR/CXT cast parsing, member extraction

openwilly-iso          ISO 9660 parser for CD image mounting
openwilly-launcher     (planned) GUI launcher for game management
openwilly-fileio       FILEIO.X32 replacement DLL (legacy compatibility path)
openwilly-keypoll      KEYPOLL.X32 replacement DLL (legacy compatibility path)
```

The legacy DLL crates (fileio, keypoll) exist from an earlier compatibility-wrapper approach and are retained for reference but are not used by openwilly-player.

## Building and Running

### Prerequisites

- Windows 10/11 (x64)
- Rust 1.75 or later
- Original game files (from CD or ISO image)

### Build

```powershell
cargo build --release -p openwilly-player
```

### Run

```powershell
# Point to the directory containing the extracted game data
cargo run --release -p openwilly-player -- --game-dir "C:\path\to\game\files"
```

The game directory must contain the Director cast files (DATA.CST, *.DXR, *.CXT) and asset folders (Movies/, Data/, Autos/, Xtras/).

## Development Status

The player is functional for "Autos bauen mit Willy Werkel" with the following systems implemented:

- Director 6 DXR/CXT parser with bitmap decoding and palette support
- Scene state machine (Boot, Menu, Garage, Junkyard, Yard, World, Destinations, CarShow)
- Car building with snap points, attachment validation, and road-legality checks
- Drag-and-drop with morph/snap preview (junkView/UseView switching)
- Arcade-style part physics (gravity, floor collision, weight-based impact sounds)
- Driving with keyboard and mouse steering, engine sound state machine (9 types x 7 states)
- Dashboard HUD (16-frame fuel needle, speedometer)
- Toolbox popup menu in driving view
- Dialog system with cue-point sync, subtitle rendering, speaker color coding
- Quest flags and mission delivery (Figge workshop cutscene)
- Scene scripts for all destinations (82-94)
- Car show with score rating and judge animation
- Software cursor (9 types from 00.DXR, context-sensitive switching)
- Transition cutscenes with progress bar
- Save/load system
- Developer menu with cheats, scene warps, and triggers

See [docs/GAPS.md](docs/GAPS.md) for the detailed gap tracking table comparing the reimplementation against mulle.js.

## Legal

### Project License

Dual-licensed under MIT and Apache 2.0. Choose whichever fits your needs.

### Game Rights

This project contains no game files, assets, or copyrighted content. You must own the original games. OpenWilly reads the original data files at runtime and does not redistribute them.

The Willy Werkel games are copyrighted by their respective owners (Levande Boecker / Moellers & Bellinghausen Verlag GmbH / Terzio).

## Credits

- **Original games**: Levande Boecker (Sweden) -- game design, art, and Lingo scripting
- **mulle.js**: Niclas Lindstedt -- JavaScript/Phaser reimplementation that served as the behavioral reference for this project. Repository: https://github.com/niclaslindstedt/mulle
