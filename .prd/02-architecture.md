---
title: ultima6 — 02 architecture
type: spec
tags: [ultima6, rea, rust, wasm, amiga]
---

# 02 — Architecture

Status: in-progress (rev 2, 2026-10-14)

## Pieces

| Piece | Location | License |
|---|---|---|
| REA with an `m68k` raw-binary architecture | `~/2lab.ai/_ref/rea` branch `feat/m68k-raw-binary` | MIT |
| RE tools (PP20, hunk flattener) + format/behavior notes | `tools/`, `re/notes/` | MIT |
| Decompiler dumps of `game` | `re/dumps/` — gitignored, never published | — |
| Rust workspace (main source) | `crates/` | MIT |
| Web shell (static HTML + wasm-bindgen glue) | `crates/u6-web` → `site/` build output | MIT |
| Deploy | Cloudflare Pages project `ultima6` (account icedac@gmail.com) + Vercel DNS CNAME `ultima6` in the `2lab.ai` zone | — |

Before each build `scripts/deploy-web.sh` replaces the gitignored `site/packs/` (served under `/packs/` behind the owner key) with exactly the `packs/` of the private repository `2lab-ai/ultima6-deploy` (`U6_DEPLOY_REF`, default `main`), and stops the deploy, leaving `site/packs/` as it was, unless its `index.json` parses and every file it lists is a plain name present there (the owner, 2026-10-21; `hd-i18n-voice/ssot.md`).

## Rust workspace

```text
crates/
  u6-intake   zip → HFE → Amiga MFM → ADF → OFS files → PP20 (shared by native + wasm)
  u6-formats  parsers for the Amiga data files (spec: re/notes/formats.md)
  u6-core     platform-free game state + logic, ported per original function
  u6-render   platform-free framebuffer + blit primitives → RGBA (world-view orchestration lives in u6-core/gfx)
  u6-native   desktop runner (window + keyboard) for development and oracle comparison
  u6-web      wasm-bindgen entry: canvas, input, file drop, IndexedDB cache
```

Rules that keep P3 cheap: game tables load as data (no hard-coded consts for
characters/map/schedules); `u6-core` never touches I/O or time directly — the platform
passes input and time in, gets a framebuffer and events out; NPC decision points are
explicit functions so a later `NpcBrain` can replace them.

## Port method (P1)

1. Flatten `game` (`tools/hunk2raw.py`) → REA `--architecture m68k` → function inventory.
2. Classify each function: game logic / C runtime / OS stub / data. Only game logic is
   ported; runtime maps to Rust std; OS calls map to the platform layer.
3. Port per subsystem in dependency order, each function cited by original address.
4. Verify against the original running in an Amiga emulator (parity sheets).

## Data path (user's bytes → engine)

```text
ultima-vi.zip | *.adf | loose files  (user drop)
  → u6-intake → Amiga file set (PP20 decrunched)
  → u6-formats → u6-core world
```

## Measured format facts (2026-10-14)

- DOS-identical layout: `converse.a` (LE32 offsets), `portrait.a` (LE32 offsets),
  `look.lzd`, `book.dat` (LE16 offsets), `map` (32,256 bytes, LE 12-bit packing),
  `chunks` (65,536), `tileflag` (7,168).
- Big-endian: `basetile` (u16 per object), `animdata`, `OBJBLK*` object count.
- Amiga-specific graphics: `maptiles.ega` (76,612 decrunched), `objtiles.ega`
  (265,248), `animmask.ega`, `masktype.ega` — format from the `game` loaders via REA.

## Superseded (rev 1)

ScummVM `ultima6` engine + Amiga detection (fork `2lab-ai/scummvm`, unmodified).
The fork and the local builds in `~/2lab.ai/scummvm` are cross-reference only.
