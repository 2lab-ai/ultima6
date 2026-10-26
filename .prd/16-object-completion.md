--
---
title: ultima6 — 16 object completion (stubs, stale seams, wrong ports, untranslated lines)
type: spec
tags: [ultima6, objects, port, i18n]
---

# 16 — Object completion

Status: planned (2026-10-27). Item 1 of `objects-menu-fog-motion/ssot.md`; findings from round 1 units O1, O2 and O3 (`objects-menu-fog-motion/loop.md`).

## Measured facts (main 88eb5b0)

- `port-map.md` reports 3 stubbed game functions. `tools/rea/port_map.py` ranks ported above stubbed (`:40`) and records a `todo_stub!` only under its own address (`:112`). A stub whose address is ported elsewhere therefore disappears from the map. The game code holds such stale seams:
  - `actors/walk.rs:531`: the portcullis (0x1301e2, ported as `objects/effects.rs::toggle_passthrough`);
  - `actors/party.rs:441`: a carried object of a type (0x10b18c, ported as `objects/iter.rs::find_child`), which by the stub's own comment means swamp boots never protect (the code's claim; O1 traces the callers);
  - `actors/turn.rs:139`: the end of solo mode (0x104094, ported as `main_loop/main_hunk.rs::end_solo`);
  - `combat/seams.rs:95`: a staff or wand cast (0x11a586, ported as `magic/dispatch.rs::dispatch`, but async).
- Stubs whose routines are not ported:
  - `objects/commands/push.rs:117`: Move of a carried object to someone (0x12ae04);
  - `usecmd/misc.rs:435`: the serpent figure (0x12ff50);
  - `actors/npc.rs:585`: the confiscation on arrest (its stub names 0x10c928, which the map lists as `contained_in`; the address is to be checked);
  - `combat/seams.rs:68`: the remains (0x10c6a6);
  - `combat/seams.rs:140`: the overview sight, mode 2 of 0x109c12;
  - `usecmd/seams.rs:233`: the ending (0x133d1e, 0x13513a).
- Out of scope: the debug teleport (`main_loop/commands.rs:436`, 0x104140).
