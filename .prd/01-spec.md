---
title: ultima6 — 01 spec
type: spec
tags: [ultima6, rea, rust, wasm, amiga]
---

# 01 — Spec: Ultima VI (Amiga) reverse-engineered and ported to Rust, running in the browser

Status: in-progress (rev 2, 2026-10-14 — main source is Rust; ScummVM is no longer the runtime)

## Problem

The user wants Ultima VI reverse-engineered with REA (morluto/rea), ported to Rust
following the original code's intent, and served at `ultima6.2lab.ai`; then refactored
so NPCs can be driven by an AI model, systems are editable, and mods are easy
(see `browser-port/ssot.md` for the verbatim commands).

The input is the user's own download: `ultima-vi.zip` from archive.org item
`ultima-vi`, a Pauline flux dump of the **Amiga 3.5"** release (disks INSTALL, DATA1,
DATA2). The executable is `game` (PowerPacker-crunched AmigaOS hunk file, 267,332
bytes decrunched, Motorola 68000).

Rights boundary (fixed 2026-10-14): Ultima VI is still a commercial Electronic Arts
title (GOG "Ultima 4+5+6", "© 1992, 2011 Electronic Arts Inc."); the archive.org item
carries no license field. Therefore:

- the public site serves **our Rust engine only**; game bytes are supplied by the user
  in their own browser and never uploaded or hosted;
- decompiler output of `game` stays in `re/dumps/` (gitignored); committed Rust code
  never contains pasted decompiler output — it cites the original function address and
  describes behavior in prose;
- no game data or derived files are committed.

These restrictions are distribution controls, not a legal determination or a claim of clean-room
development. The archive item's availability and the user's possession of files do not
establish a license. The translated engine is derived from analysis of EA's executable; whether
publishing it is permitted depends on facts and jurisdiction this project has not assessed
(reported to the user 2026-10-14; deploy proceeds on the user's instruction). Extracted strings,
tables, artwork and executable fragments never go into source, fixtures, build artifacts, logs or
review captures. Original files and extracted content stay browser-local and are excluded from
telemetry and error reports. A later AI-model integration needs its own explicit payload/consent
boundary and must not upload dialogue, files or extracted content silently. Public branding must
not imply EA endorsement.

## Phases

| Phase | Outcome |
|---|---|
| P1 | Rust port of the original game logic, derived from the REA decompile of `game` |
| P2 | Deploy P1 to `ultima6.2lab.ai` (wasm) |
| P3 | Refactor: 3a NPC action set offered to an AI model, reply applied · 3b characters, world map and systems editable as data · 3c Steam-style mod packages |
| P4 | Redeploy |

Deploys follow milestones. Milestone 1 = the game boots into Britannia from the
user's disks; the party walks; NPCs follow schedules; talking works. Combat, magic,
inventory and the rest ride later milestones, each deployed when verified.

## Non-goals

- Hosting any game data.
- The intro/endgame sequences and MED music in milestone 1.
- Using Nuvie/ScummVM as the runtime (cross-reference only).

## Acceptance scenarios

1. REA opens the flattened `game` image as m68k and decompiles game functions
   (`re/notes/` holds the command evidence; dumps stay local).
2. `https://ultima6.2lab.ai` with no files → the page asks for the user's Ultima VI
   files and loads no game bytes from the network.
3. Drop `ultima-vi.zip` (sha256 `a65e911d…22e8a2`) → disks decode in-browser → the Rust
   engine renders Britannia with party and UI; keyboard movement works. Evidence:
   screenshot of the live site next to the same scene from the original game running
   in an Amiga emulator (parity sheet).
4. Reload → files persist (IndexedDB) and the game starts without re-dropping.
5. P3: an NPC configured for AI control receives its legal action list, the model's
   choice is applied in-game; a mod package (manifest + data overrides) changes a
   character/map entry without rebuilding the engine.
