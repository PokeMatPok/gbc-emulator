# GBC Emulator

A Game Boy Color (GBC/DMG) emulator written in **Rust**, compiled to **WebAssembly**, with a **WebGPU** (Canvas 2D fallback) web frontend.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                        Web Frontend                      │
│  index.html  ·  style.css  ·  app.js                    │
│                                                          │
│  • WebGPU renderer (nearest-neighbour pixel scaling)    │
│  • Canvas 2D fallback                                    │
│  • Web Audio API (48 kHz stereo)                        │
│  • Keyboard / touch / gamepad input                     │
│  • ROM drag-and-drop                                     │
└──────────────────────┬──────────────────────────────────┘
                       │  wasm-bindgen JS glue
┌──────────────────────▼──────────────────────────────────┐
│                    WASM Core (Rust)                      │
│                                                          │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌────────┐  │
│  │   CPU    │  │   PPU    │  │   APU    │  │ Timer  │  │
│  │  SM83    │  │ 160×144  │  │ 4-ch PCM │  │ DIV/   │  │
│  │ 512 ops  │  │ scanline │  │ 48 kHz   │  │ TIMA   │  │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └───┬────┘  │
│       │             │             │             │        │
│  ┌────▼─────────────▼─────────────▼─────────────▼────┐  │
│  │                       MMU                         │  │
│  │  ROM banks · WRAM banks · I/O · OAM DMA · HDMA   │  │
│  └────────────────────────┬──────────────────────────┘  │
│                           │                              │
│  ┌────────────────────────▼──────────────────────────┐  │
│  │                    Cartridge                      │  │
│  │  ROM-Only · MBC1 · MBC2 · MBC3 (+RTC) · MBC5     │  │
│  └───────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
```

## Hardware Emulated

| Component | Details |
|-----------|---------|
| **CPU** | Sharp SM83 (LR35902) – full 512-instruction set, interrupts, HALT/STOP, double-speed mode |
| **PPU** | Scanline renderer – BG, Window, OBJ; DMG 4-shade palettes; CGB BGR555 colour palettes (8 BG + 8 OBJ); VRAM bank switching |
| **APU** | 4 channels: pulse (ch1 sweep + ch2), wave (ch3), noise (ch4); frame sequencer; stereo mixing; 48 kHz output |
| **Timer** | DIV/TIMA/TMA/TAC with falling-edge and reload accuracy |
| **Joypad** | P1 register; joypad interrupt |
| **Serial** | Stub (no link cable) |
| **Cartridge** | ROM-Only, MBC1, MBC2, MBC3 + RTC, MBC5 |
| **MMU** | Full 16-bit address space; OAM DMA; HDMA/GDMA (CGB); WRAM banking; echo RAM |
| **GBC extras** | CGB mode detection, 2×VRAM banks, 8×WRAM banks, color palettes, double-speed mode |

## Prerequisites

| Tool | Install |
|------|---------|
| **Rust** ≥ 1.70 | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` |
| **wasm-pack** | `cargo install wasm-pack` |
| **Node.js** ≥ 18 | [nodejs.org](https://nodejs.org) |

Add the WASM target once:
```bash
rustup target add wasm32-unknown-unknown
```

## Building

```bash
# Install dependencies (no npm deps currently needed)
npm install

# Build WASM + JS glue (release mode)
npm run build

# Build in debug mode (faster compile, larger WASM)
npm run build:debug
```

The build produces `web/pkg/gbc_emulator.js` and `web/pkg/gbc_emulator_bg.wasm`.

## Running

```bash
# Start the dev server at http://localhost:8080
npm run serve

# Or build + serve in one step
npm run dev
```

Then open [http://localhost:8080](http://localhost:8080) and drag-and-drop a `.gb` or `.gbc` ROM file.

> **Note:** The server sets `Cross-Origin-Opener-Policy: same-origin` and
> `Cross-Origin-Embedder-Policy: require-corp` headers, required for
> `SharedArrayBuffer` / high-resolution timers used by the WASM runtime.

## Controls

| Keyboard | Gamepad | Function |
|----------|---------|----------|
| Z | Button 0 | A |
| X | Button 1 | B |
| Enter | Button 9 | Start |
| Space / Shift | Button 8 | Select |
| Arrow keys | D-pad / Axis | D-Pad |

On-screen touch buttons are also displayed.

## CLI (headless)

```bash
cargo build --release
./target/release/gbc-emulator path/to/game.gb [frames]
```

Runs the ROM headlessly for the given number of frames (default 60) and prints cycle counts.

## Running Tests

```bash
cargo test
```

## Project Structure

```
gbc-emulator/
├── src/
│   ├── lib.rs           ← WASM bindings (wasm-bindgen)
│   ├── main.rs          ← CLI entry point
│   ├── emulator.rs      ← Top-level Emulator struct
│   ├── cpu/
│   │   ├── mod.rs       ← SM83 CPU + full instruction set
│   │   └── registers.rs ← Register file
│   ├── mmu/
│   │   └── mod.rs       ← Memory Management Unit
│   ├── ppu/
│   │   └── mod.rs       ← Picture Processing Unit
│   ├── apu/
│   │   └── mod.rs       ← Audio Processing Unit
│   ├── cartridge/
│   │   └── mod.rs       ← MBC implementations
│   ├── timer.rs
│   ├── joypad.rs
│   └── serial.rs
├── web/
│   ├── index.html
│   ├── style.css
│   ├── app.js           ← WebGPU/Canvas 2D + Web Audio frontend
│   └── pkg/             ← Generated by wasm-pack (git-ignored)
├── scripts/
│   ├── server.js        ← Dev HTTP server
│   └── copy-web.js      ← Post-build validation
├── Cargo.toml
└── package.json
```

## ROM Compatibility

Tested ROM types: MBC1, MBC3, MBC5 cartridges and ROM-only games.
The PPU uses a scanline-based renderer (not pixel FIFO), so some games with
mid-scanline register changes may render slightly differently from hardware.

## Licence

MIT
