---
title: ultima6 browser-port — loop
type: loop
tags: [ultima6]
---

# Loop — browser-port

Status: in-progress

## Build facts (measured 2026-10-14)

- rea 6.4.0 built from 767fe0f0c (`npm ci`, `npm run build:cached`), installed via
  `npm pack` + `npm install -g`; Ghidra 12.1.4 + JDK 21 provider healthy
  (`rea doctor --provider ghidra`); arm64 decompiler built from source.
- `rea inspect game` → `unsupported_target / file_format_rejected` (Amiga hunk);
  raw-binary admits x86/x86_64/arm/arm64/6502 only.
- Ghidra ships `Processors/68000` (68000.ldefs).
- ScummVM web build: `./dists/emscripten/build.sh setup configure make dist
  --disable-all-engines --enable-engine=ultima6` (emsdk 4.0.10), log
  `~/Games/ultima6/scummvm-wasm-build.log`.
- Local data (never committed): `~/Games/ultima6/{original,img,files,unpacked}`.

## Round 1 — open work units

| WU | Scope | Owner files | State |
|---|---|---|---|
| WU-1 | rea `m68k` raw-binary architecture (Ghidra `68000:BE:32:default`) + tests | rea `src/domain/rawBinary*`, `src/ghidra/GhidraRawBinary*`, schemas | open |
| WU-2 | hunk → flat raw image with relocations applied + symbol/section map | this repo `tools/hunk2raw.py` | open |
| WU-3 | Format adapter for byte-order files vs Nuvie readers | this repo `tools/amiga2dos.py`, `re/notes/formats.md` | open |
| WU-4 | Graphics formats (`*.ega`) from REA decompile of the loaders | `re/notes/graphics.md`, adapter extension | blocked on WU-1, WU-2 |
| WU-5 | ScummVM fork: Amiga detection + any engine hook | `2lab-ai/scummvm` `engines/ultima/` | blocked on WU-3/4 |
| WU-6 | Browser intake: zip → HFE → ADF → OFS → PP20 (TS) with tests | this repo `web/src/intake/` | open |
| WU-7 | Shell UI + engine launch + IndexedDB cache | this repo `web/` | blocked on wasm build |
| WU-8 | Deploy: CF Pages `ultima6` + DNS CNAME | — | blocked on WU-7 |
