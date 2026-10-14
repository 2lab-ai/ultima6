---
title: ultima6 — 04 P3 refactor (AI NPCs, editable systems, mods)
type: spec
tags: [ultima6, rust, refactor, ai, modding]
---

# 04 — P3 refactor: AI-driven NPCs, editable systems, mods

Status: planned (2026-10-14). Starts after the P1 port reaches parity on the M2 scope.

User command (verbatim, `browser-port/ssot.md`): "3. rust 코드를 고치기 좋도록 리팩토링 /
3.a npc의 가능한 행동을 ai model에게 주고 응답 받을수 있도록 / 3.b 캐릭터 월드맵 등 전체적인
시스템 쉽게 수정할 수 있도록 / 3.c 스팀 게임처럼 mod를 쉽게 만들수 있도록".

## Invariant: the original stays the default and stays reproducible

Every P3 change is behavior-preserving with no mod and no AI brain loaded: the parity receipts of
P1 (headless frame sequences + deterministic state traces for the scripted scenarios in
`03-core-port.md` §7) must stay bit-identical. Refactors land behind that gate.

## 3.a — NPC actions offered to an AI model

Boundary (spec 03 §5): the NPC turn 0x1200b0 keeps one entry point. P3 splits it into

1. **Perceive** — build an owned `NpcView` (self stats, schedule entry, worktype, nearby actors
   and objects within the view window, time, party relation, recent dialogue flags). No borrows
   escape.
2. **Enumerate** — `legal_actions(&State, npc) -> Vec<NpcAction>`: every action the original
   routine could take this turn (step in each passable direction, follow schedule, open door,
   sit/sleep/eat at work object, attack target, flee, talk-to-party, idle), each with a stable id
   and a one-line description. Enumeration consumes no simulation RNG and mutates nothing.
3. **Decide** — `trait NpcBrain { async fn decide(&mut self, req: DecisionRequest) -> Decision; }`
   `OriginalBrain` reproduces the original choice bit-exactly (it runs the original decision code
   on the view). `ModelBrain` serializes the request to JSON and awaits a reply through a
   platform `Oracle` port (web: a user-configured HTTPS endpoint or a local model; native: an
   HTTP client) — **opt-in per NPC or per area, off by default**.
4. **Validate + apply** — the reply must name one id from `legal`; anything else, a timeout
   (budget per turn), or a stale generation falls back to `OriginalBrain` for that turn and is
   logged. Application goes through the original action execution path.

Free-text NPC speech: an optional `ModelConverse` hook may answer keywords the NPC's script does
not cover, constrained to that NPC's persona extracted from its own script; the script's
flags/items side effects stay script-only.

Privacy (01-spec rights): requests never include game files or extracted script text beyond what
the user explicitly enables; the page shows exactly what is sent; no endpoint is preconfigured.

## 3.b — systems editable as data

Every table the engine reads moves behind typed **definitions** loaded from a definition set:

| Definition | Today's source | Editable form |
|---|---|---|
| Characters/actors (stats, portrait, schedule, worktype, talk script id, alignment) | objlist + schedule + converse index | `actors/*.toml` (one per NPC) |
| Object types (tile, weight, flags, stackable, article, container, light) | basetile + tileflag + look.lzd + game tables | `objects/types.toml` |
| World map + chunks + object placement | map + chunks + OBJBLK?? | `world/` region files (chunk refs as text grid, objects as records) |
| Spells, reagents, creature types, combat tables | game hunk tables | `rules/*.toml` |
| Conversations | converse.a/b bytecode | decompiled text form ↔ bytecode (round-trip tested) |

`u6-defs` crate: schema (serde), validation with precise errors, and a two-way converter from the
user's original files (`u6 export <disks> <dir>` / import). The engine reads only definitions;
the original files are one importer. Round-trip gate: export → import → engine → identical P1
receipts.

## 3.c — Steam-style mods

- **Package**: a folder or zip with `mod.toml` (id, name, version, author, engine semver range,
  depends/conflicts, load-after) + any subset of definition files + optional new tiles/portraits
  (PNG with palette mapping) + optional Lua-free behavior via data only (no script runtime in
  v1; a WASM-component plugin ABI is a later decision).
- **Overrides**: by stable id (actor 5, object type 0x199, spell "Light"), field-level merge;
  deterministic load order (dependencies, then load-after, then id); conflicts reported.
- **Mod manager** in the page: drop a mod zip, enable/disable/reorder, per-mod validation
  report; enabled set recorded in each save; a save with a missing mod refuses to load with the
  list.
- **Authoring**: `u6 new-mod <id>` scaffold, `u6 validate`, and the export of the base game
  definitions as the starting point; an in-page editor is a later phase.

## Acceptance (P3)

1. No mods, OriginalBrain: all P1 receipts identical.
2. An NPC set to `ModelBrain` with a stub endpoint receives its legal list and its chosen action
   is applied; invalid/timeout replies fall back and are logged.
3. A sample mod renames an NPC, changes a shop price and moves a chest; enabling it changes those
   in game without rebuilding the engine; disabling restores the original.
4. Export → import of the base game round-trips byte-identically for every file the engine reads.
