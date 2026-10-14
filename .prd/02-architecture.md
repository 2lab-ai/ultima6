---
title: ultima6 — 02 architecture
type: spec
tags: [ultima6, rea, scummvm, wasm, amiga]
---

# 02 — Architecture

Status: in-progress (2026-10-14)

## Pieces and where they live

| Piece | Repo / path | License |
|---|---|---|
| REA with an `m68k` raw-binary architecture | `~/2lab.ai/_ref/rea` branch `feat/m68k-raw-binary` (fork of morluto/rea) | MIT |
| Hunk flattener, PP20 decruncher, format notes | this repo `tools/`, `re/notes/` | MIT (this repo) |
| ScummVM with Amiga Ultima VI support | `2lab-ai/scummvm` branch `feat/ultima6-amiga` (`~/2lab.ai/scummvm`) | GPL-3.0 |
| Browser shell (file intake, decoding, launch) | this repo `web/` | GPL-3.0 (links the GPL build) |
| Deploy | Cloudflare Pages project `ultima6` + Vercel DNS CNAME `ultima6` in the `2lab.ai` zone | — |

## Data path (user's bytes → engine)

```text
ultima-vi.zip (user drop)
  → unzip in browser (Disk N.hfe)
  → HFE → Amiga MFM decode → ADF (880 KB, 1760 sectors)
  → AmigaDOS OFS reader → files of INSTALL / DATA1 / DATA2
  → PP20 decrunch where the file starts with "PP20"
  → Amiga→engine adapter (byte-order swaps, graphics conversion — per re/notes)
  → Emscripten FS mount /games/ultima6 → ScummVM ultima6 engine
```

The ADF and plain-file inputs enter the same chain at the matching stage.
Converted files are cached in IndexedDB in the user's browser only.

## Measured format facts (2026-10-14, local unpack of the user's disks)

- DOS-identical layout: `converse.a` (LE32 offset table), `portrait.a` (LE32 offsets),
  `look.lzd`, `book.dat` (LE16 offsets), `map` (32,256 bytes, LE 12-bit packing),
  `chunks` (65,536 bytes), `tileflag` (7,168 bytes).
- Big-endian on Amiga: `basetile` (u16 per object), `animdata`, `OBJBLK*` object count.
- Amiga-specific graphics: `maptiles.ega` (76,612), `objtiles.ega` (265,248),
  `animmask.ega`, `masktype.ega` — format to be read from the `game` loaders via REA.
- ScummVM detection keys on `converse.a`; the Amiga file is 162,615 bytes like GOG DOS
  but md5 `cf65b2931d1bc89abcd06bce614aa495` (GOG DOS: `5065716423ef1389e3f7b4946d815c26`).

## Boundaries

- The adapter lives in the shell (TypeScript), with a Python reference in `tools/`
  used for native verification against the ScummVM desktop build.
- Engine changes stay minimal: Amiga detection entry, any palette/format hook the
  converted data cannot express.
- The ScummVM web build needs no SharedArrayBuffer unless the build log shows pthreads;
  if it does, Pages `_headers` sets COOP/COEP.
