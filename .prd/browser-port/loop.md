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
- REA on the real binary: `tools/hunk2raw.py game` → `~/Games/ultima6/re/game.bin` (35 hunks,
  base 0x100000) → `rea inspect … --architecture m68k` = 1,383 procedures, 515 strings
  (41 s). Bulk decompile: `node tools/rea/dump-all.mjs` (one MCP session; first batch 30 s
  for import, then fast) → `~/Games/ultima6/re/dumps/game/` 1,383/1,383 status ok, 6.5 MB.
- Rust gates: `scripts/cargo.sh fmt --all --check` · `clippy --workspace --all-targets -D warnings`
  · `test --workspace` · `bash scripts/build-web.sh` (wasm32 via rustup stable 1.96.1).
- Deploy channel: CF Pages project `ultima6` (account b6eba338…, created via API — `wrangler
  pages project create` autoconfigures a Worker instead; that Worker was deleted) →
  `npx wrangler@4 pages deploy site --project-name ultima6 --branch <b>`; preview alias
  https://preview.ultima6.pages.dev. Custom domain + Vercel CNAME `ultima6` at milestone 1.

## Round 1 — open work units (rev 2: Rust main source)

Gate before any Rust logic dispatch: `re/notes/game-layout.md` records (a) symbol
count, (b) OS-call vs custom-chip access, (c) C runtime identity.

| WU | Scope | Owner files | State |
|---|---|---|---|
| WU-1 | rea `m68k` raw-binary architecture + tests + reinstall | rea `feat/m68k-raw-binary` | in flight |
| WU-2 | hunk → flat image + map; layout notes | `tools/hunk2raw.py`, `re/notes/game-layout.md` | in flight |
| WU-3 | Amiga data format spec (byte-level, for Rust parsers) | `re/notes/formats.md` (+ optional `tools/amiga2dos.py`) | in flight |
| WU-6 | TS intake (oracle for the Rust port; deleted once `u6-intake` lands) | `web/` | in flight |
| WU-R0 | Rust workspace skeleton: crates, native window, wasm shell, framebuffer | `Cargo.toml`, `crates/*` | open |
| WU-O | Oracle: original game running in an Amiga emulator (AROS ROM) + screenshot capture | `re/notes/oracle.md`, `tools/oracle/` | open |
| WU-R1 | `u6-intake` Rust port of WU-6 with the same fixture tests | `crates/u6-intake` | blocked on WU-6, WU-R0 |
| WU-R2 | `u6-formats` parsers from formats.md | `crates/u6-formats` | blocked on WU-3, WU-R0 |
| WU-4 | REA decompile: function inventory + classification; graphics loaders | `re/notes/functions.md`, `re/notes/graphics.md` | blocked on WU-1, WU-2 |
| WU-P* | Per-subsystem logic port (from WU-4 inventory) | `crates/u6-core/src/<subsystem>` | blocked on WU-4 |
| WU-D | Deploy: CF Pages `ultima6` + DNS CNAME | `site/`, deploy script | blocked on milestone 1 |
