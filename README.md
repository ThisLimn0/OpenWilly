# OpenWilly

🎮 **Compatibility wrapper for classic Willy Werkel (Mulle Meck) games on modern Windows systems**

![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)
![Rust](https://img.shields.io/badge/rust-1.75%2B-orange)

## About

OpenWilly is a Rust-based compatibility layer that allows you to play classic Willy Werkel educational games from the late 1990s on modern Windows 10/11 systems. These games use **Macromedia Director 6** as their engine – the Director Player (WILLY32.EXE) still runs on modern Windows, but the 32-bit **Xtras** (MOA plugins) and CD-ROM requirements cause the games to fail.

### Supported Games

- 🚗 **Autos bauen mit Willy Werkel** (1997)
- ✈️ **Flugzeuge bauen mit Willy Werkel**
- 🏠 **Häuser bauen mit Willy Werkel**
- 🚀 **Raumschiffe bauen mit Willy Werkel**
- 🚢 **Schiffe bauen mit Willy Werkel**

## Features (Planned)

- ✅ ISO mounting and extraction
- ✅ CD-ROM check bypass
- ✅ Macromedia Director compatibility (GDI32-based rendering)
- ✅ Legacy API compatibility layer
- ✅ Xtra plugin support (FILEIO, KEYPOLL, PMATIC)
- ✅ Windowed and fullscreen modes
- ✅ Modern resolution support
- ✅ No external dependencies (dgVoodoo2, etc.)
- ✅ User-friendly launcher GUI

## Architecture

OpenWilly consists of several modular components:

- **openwilly-launcher**: Main GUI application for game management
- **openwilly-fileio**: FILEIO.X32 replacement – file I/O + CD-path bypass
- **openwilly-keypoll**: KEYPOLL.X32 replacement – keyboard state polling
- **openwilly-pmatic**: PMATIC.X32 stub – PrintOMatic (unused by games)
- **openwilly-iso**: ISO9660 parsing and virtual filesystem
- **openwilly-patcher**: Runtime DLL injection and API hooking
- **openwilly-director**: Macromedia Director engine support (Xtras, DXR files)
- **openwilly-media**: Video and audio playback (AVI, MCI)
- **openwilly-common**: Shared utilities and types

See [MASTERPLAN.md](MASTERPLAN.md) for detailed architecture documentation.

## Quick Start

### Prerequisites

- Windows 10/11 (x64)
- Rust 1.75 or later
- Visual Studio Build Tools (for Windows SDK)
- Original game ISOs

### Building

```powershell
# Clone repository
git clone https://github.com/yourusername/openwilly.git
cd openwilly

# Build all components
cargo build --release

# Run launcher
cargo run --release -p openwilly-launcher
```

### Usage

1. Launch the OpenWilly application
2. Add your game ISOs to the library
3. Select a game and click "Play"
4. Enjoy!

## Development Status

🚧 **Phase 1 – Making it Playable** (Active)

- [x] Project planning and architecture
- [x] Basic workspace setup  
- [x] Game executable analysis (WILLY32.EXE is GDI32, not DirectDraw)
- [x] Ghidra decompilation of all Xtras
- [x] FILEIO.X32 Rust replacement (25 handlers, CD-path bypass)
- [x] KEYPOLL.X32 Rust replacement (keyboard polling via MOA)
- [x] PMATIC.X32 stub (PrintOMatic, unused by game scripts)
- [ ] 32-bit DLL build & runtime test with Director
- [ ] MOA value marshalling validation
- [ ] Launcher GUI
- [ ] First playable game

See [MASTERPLAN.md](MASTERPLAN.md) for detailed roadmap.

## How It Works

### The Problem

These games were designed for Windows 95/98 and rely on:
- **Macromedia Director 6 Player** (WILLY32.EXE) – the engine itself still runs on modern Windows
- **Director Xtras** (FILEIO.X32, KEYPOLL.X32) – 32-bit MOA plugins compiled for Win95
- **CD-ROM checks** – game expects data on a CD drive
- **Hardcoded paths** – absolute paths to CD-ROM (D:\, E:\, etc.)

### The Solution

OpenWilly provides:

1. **Xtra Replacements**: Drop-in Rust DLLs that replace the original Director Xtras (FILEIO.X32, KEYPOLL.X32, PMATIC.X32) with modern, working versions
2. **CD-ROM Bypass**: Integrated path redirection from CD drive paths to local game directory
3. **API Hooks**: Runtime hooks for file paths, registry access, and drive detection
4. **ISO Extraction**: Extract game ISOs to eliminate CD-ROM requirement
5. **Launcher**: User-friendly GUI for game management

```
┌──────────────────────┐
│  WILLY32.EXE         │  (Original Director 6 Player)
│  (Macromedia Director)│
└──────┬───────────────┘
       │ loads
       ▼
┌──────────────────┐ ┌──────────────┐ ┌──────────────┐
│ FILEIO.X32  ✅   │ │ KEYPOLL.X32  │ │ PMATIC.X32   │
│ (Rust replacement)│ │ (Rust)       │ │ (Rust stub)  │
│ + CD-path bypass │ │ + Input      │ │ + Print stub │
└──────────────────┘ └──────────────┘ └──────────────┘
       │
       ▼
┌──────────────────┐
│ GDI32 Rendering  │  (Native Windows – just works!)
│ (no wrapper needed)│
└──────────────────┘
```

## Project Structure

```
OpenWilly/
├── crates/
│   ├── openwilly-launcher/     # GUI launcher
│   ├── openwilly-fileio/       # FILEIO.X32 replacement (CD bypass)
│   ├── openwilly-keypoll/      # KEYPOLL.X32 replacement (keyboard)
│   ├── openwilly-pmatic/       # PMATIC.X32 stub (print, unused)
│   ├── openwilly-director/     # Director engine support
│   ├── openwilly-patcher/      # API hooks & path redirection
│   ├── openwilly-iso/          # ISO handling
│   ├── openwilly-media/        # Video/Audio
│   └── openwilly-common/       # Shared types
├── tools/                      # Analysis & development tools
├── docs/                       # Documentation
├── mulle.js/                   # JS reference implementation (Phaser)
├── autos-bauen/decomps/        # Ghidra decompilations
├── export/                     # Extracted Director data
├── MASTERPLAN.md               # Current development plan
└── README.md                   # This file
```

## Contributing

Contributions are welcome! This is a passion project to preserve classic educational games.

### Areas Where Help Is Needed

- 🔍 **Reverse Engineering**: MOA interface analysis, Director internals
- 🔌 **Xtra Development**: Improving FILEIO/KEYPOLL implementations
- 🎬 **Media Codecs**: AVI playback, MCI audio
- 🎨 **GUI Design**: Launcher interface improvements
- 📚 **Documentation**: Writing guides and documentation
- 🐛 **Testing**: Playing games and reporting issues

### Development Setup

```powershell
# Build all crates (host architecture)
cargo build --workspace

# Build 32-bit Xtra DLLs for Director
cargo build --release --target i686-pc-windows-msvc -p openwilly-fileio -p openwilly-keypoll -p openwilly-pmatic
```

See [MASTERPLAN.md](MASTERPLAN.md) for detailed development information.

## Legal & Licensing

### Project License

OpenWilly is dual-licensed under:
- MIT License
- Apache License 2.0

Choose whichever license works best for your use case.

### Game Rights

**Important**: This project does NOT include any game files, assets, or copyrighted content. You must own the original games to use this software. OpenWilly is purely a compatibility layer to run software you already legally own on modern systems.

The Willy Werkel games are copyrighted by their respective owners (Möllers & Bellinghausen Verlag GmbH, Terzio, Levande Böcker).

## Credits

- **Original Games**: Created by the talented developers at Levande Böcker
- **Inspiration**: Projects like dgVoodoo2, Wine, and DXVK
- **Community**: The game preservation community

## Resources

- [MASTERPLAN.md](MASTERPLAN.md) - Current development plan and architecture
- [mulle.js/](mulle.js/) - JavaScript reference implementation (Phaser CE)
- [autos-bauen/decomps/](autos-bauen/decomps/) - Ghidra decompilations of Xtras

## Frequently Asked Questions

### Do I need the original game CDs?

Yes, you need to own the original games. OpenWilly works with ISO images of your legally owned game CDs.

### Will this work on Linux/macOS?

Currently Windows-only due to the nature of the games being Windows executables. Future support for Wine/Proton is possible but not planned.

### Why Rust?

Rust provides:
- Memory safety (critical for hooking/injection)
- Great Windows API bindings
- Modern tooling and package management
- Performance on par with C/C++
- Active community

### Can I help?

Absolutely! See the Contributing section above.

## Disclaimer

This is a fan project for game preservation and education. It is not affiliated with or endorsed by the original game developers or publishers.

---

Made with ❤️ for preserving childhood memories
