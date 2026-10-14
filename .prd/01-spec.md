---
title: ultima6 — 01 spec
type: spec
tags: [ultima6, rea, scummvm, wasm, amiga]
---

# 01 — Spec: Ultima VI (Amiga) reverse-engineered and running in the browser

Status: in-progress (2026-10-14)

## Problem

The user wants Ultima VI reverse-engineered with REA (morluto/rea) and rebuilt so it
runs in a web browser at `ultima6.2lab.ai`. The input is the user's own download:
`ultima-vi.zip` from archive.org item `ultima-vi`, a Pauline flux dump of the
**Amiga 3.5"** release (3 disks: INSTALL, DATA1, DATA2).

Rights boundary (fixed 2026-10-14): Ultima VI is still a commercial Electronic Arts
title (GOG "Ultima 4+5+6", "© 1992, 2011 Electronic Arts Inc."). The archive.org item
carries no license field. So:

- the public site serves **engine code only** (GPL ScummVM fork + this repo's shell);
- game bytes are supplied by the user in their own browser and never uploaded;
- no game data, decrunched files, or decompiler dumps of `game` are committed.

## Goals

1. REA can open the Amiga 68000 executable (`game`, PowerPacker-crunched hunk file,
   267,332 bytes decrunched) and decompile the routines that load the Amiga data files.
2. The Amiga-specific data formats are documented from that evidence and converted
   into the layout the ScummVM `ultima6` engine (Nuvie lineage) reads.
3. A browser build of that engine boots the game into Britannia from the user's files.
4. `https://ultima6.2lab.ai` serves the build; the user drops the archive.org zip,
   the three ADFs, or the extracted files and plays.

## Non-goals

- Bundling or hosting any game data.
- The Amiga intro/endgame sequences (`*.new`, `INTRO*`) and MED music on first ship —
  the first acceptance line skips the intro.
- A ground-up reimplementation of the game engine (the GPL ScummVM `ultima6` engine is
  the reconstruction target; this project adds Amiga data support to it).

## Acceptance scenarios

1. `rea inspect <flattened game image> --target-format raw-binary --architecture m68k …`
   returns functions; `rea decompile`/`rea function` on the tile loader returns
   pseudo-code. Evidence: command output saved under `re/notes/`.
2. Open `https://ultima6.2lab.ai` in Chrome with no files → the page asks for the
   user's Ultima VI files and loads no game bytes from the network
   (DevTools network list: engine assets only).
3. Drop `ultima-vi.zip` (sha256 `a65e911d…22e8a2`) → the page decodes the disks
   in-browser and starts the engine → the Britannia map view renders with party and
   UI, keyboard movement works. Evidence: screenshot of the live site.
4. Reload → the files persist (IndexedDB) and the game starts without re-dropping.
