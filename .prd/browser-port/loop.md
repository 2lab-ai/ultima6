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

## Round 1 — work units (rev 2: Rust main source)

| WU | Scope | Result | State |
|---|---|---|---|
| WU-1 | rea `m68k` raw-binary | rea `9466e0712` (feat/m68k-raw-binary, local); 199 tests; fixture decompiles | done |
| WU-2 | hunk flattener + layout | `2725b02`; 30 tests; game = 35 hunks, no overlay, entry 0x100000 | done |
| WU-3 | Amiga format spec | `c142d64`, `0eae222` formats.md | done |
| WU-6 | TS intake (oracle for R1) | `8b02eda`; 33 tests; ADF sha = greaseweazle | done (delete after R1 parity — R1 done) |
| WU-R0 | Rust workspace skeleton | `cc620ab`; 14 tests; wasm 35 KB | done |
| WU-R1 | `u6-intake` Rust | `6b20c4e`; 30 tests incl. real zip 0.68 s (re-run by dispatcher) | done |
| WU-R2 | `u6-formats` parsers | `1c442b3`; 44+12 tests (re-run by dispatcher) | done |
| WU-4a | tile graphics + palette | `da9a115`, `1afd5f0`; 2048 tiles; sheet read by dispatcher = real U6 tiles | done |
| WU-4b | function inventory | `5630231`, `9d93cb4`; 1383/1383; M1 = 320 game fns + 41 crt, 45 stubs | done |
| WU-C0 | u6-core engine/State/RNG/seams (03 rev 2) | — | in flight |
| WU-O | Amiga emulator oracle (AROS) | — | in flight |
| WU-P1..P7 | gfx · map · objects · ui · actors · converse · main loop | — | blocked on WU-C0 |
| WU-D | Deploy ultima6.2lab.ai | CF Pages project + preview alias live (skeleton) | custom domain at M1 |

Open review items: gpt6 spec review CHANGES ×8 applied in `d749513`; WU-C0 notified.
Cleanup owed: delete `web/` (TS intake) now that u6-intake has parity.
