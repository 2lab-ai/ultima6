---
title: ultima6 — 04 P3 refactor (AI NPCs, editable systems, mods)
type: spec
tags: [ultima6, rust, refactor, ai, modding]
---

# 04 — P3 refactor: AI-driven NPCs, editable systems, mods

Status: planned (rev 2, 2026-10-15 — rebuilt on the measured data-source inventory; rev 1 assumed
the engine would be rewritten to read definitions). Starts after the M2 port is deployed (P2).

User command (verbatim, `browser-port/ssot.md`): "3. rust 코드를 고치기 좋도록 리팩토링 /
3.a npc의 가능한 행동을 ai model에게 주고 응답 받을수 있도록 / 3.b 캐릭터 월드맵 등 전체적인
시스템 쉽게 수정할 수 있도록 / 3.c 스팀 게임처럼 mod를 쉽게 만들수 있도록".

## Invariant: the original stays the default and stays reproducible

With no mod and no AI brain, every P1/M2 parity receipt (headless frame sequences + state traces of
03 §7 and the M2 receipts) stays bit-identical. Each P3 unit lands behind that gate.

## Measured facts (inventory of integrate/m2 `b312930`, 2026-10-15)

- Every game datum reaches the engine through `Files` (`crates/u6-core/src/files.rs`): either a
  user data file (map, chunks, objblk??, objlist, schedule, basetile, tileflag, animdata, look.lzd,
  converse.a/b, portrait.*, book.dat, tiles, bitmaps, MED modules) or the user's `game` executable
  read at fixed image addresses (palette, spells/reagents, creature and combat tables, map/moongate
  tables, object defs, inventory and UI tables, engine strings). Nothing reads `u` yet (the intro
  merge adds it). No game text, art or bulk table is compiled in.
- Compiled-in game knowledge exists only as object-type / tile / worktype IDs taken from compare
  instructions of the original code (e.g. `combat/attack.rs:346` `MISSILE_WEAPONS`, the Use
  command's 58-arm type switch in `usecmd/cases.rs`, ~150 scalar IDs).
- `Files` is already a layer stack: `overlay` pushes a layer (`files.rs:84`), `get` searches
  top-down; a test stacks two overlays (`files.rs:205`). Layers must be in place before boot
  (`Engine::with_seed` loads every table, `engine.rs:297`). The save layer is pushed by
  `saveload::attach`; the platforms build `base` then `save`.
- Write-back exists for objblk (byte-identical on all 69 real blocks), objlist (core encoder) and
  single schedule records. No writer exists for map, chunks, basetile, tileflag, animdata, look,
  converse, portrait, book or tiles.
- Non-party NPC names and shop prices live inside each NPC's converse bytecode (name in the script
  header; prices as operands / script tables read by op 0xb4, `converse/eval.rs:209`).
- B-class readers refuse any `game` that is not the measured build (length and hunk layout), so
  executable data can only be patched in place.

## Decision 1 — how definitions reach the engine (3b, 3c)

| Option | What | Parity risk | Cost |
|---|---|---|---|
| **F. Definitions compile to files** (recommended) | A definition = the typed, editable form of one original file or one executable table. `export` parses the user's files into text; `build` serializes definitions back to the original byte formats into a `Files` layer. The engine keeps reading files exactly as today. | none without mods (engine untouched); with mods, only data differs | one writer per format (map, chunks, tileflag, basetile, animdata, look, converse, schedule, executable tables) |
| R. Engine reads definitions | Rewrite every loader to read typed definitions; the original files become one importer. | every loader changes → whole parity suite at risk | rewrite of ~20 loaders plus the writers anyway for round trips |

Recommendation: **F**. It keeps the invariant structurally true, turns acceptance 4 (byte-identical
round trip) into the central mechanical gate, and makes a mod nothing more than a `Files` layer
between `base` and `save`. The cost of F is bounded by the original formats (fixed actor count 256,
fixed map size, fixed executable layout); edits that break those bounds are rejected by validation
with the bound named, not silently truncated.

Compiled-in IDs (class C) become editable in a second stage: each list moves to a `rules`
definition whose default is read from the instruction operand in the user's `game` at its address
(so no game content enters the source), one list per change, each behind the parity gate.

## Definition set (F)

| Definition | Original source | Text form | Writer needed |
|---|---|---|---|
| actors (party names, stats, alignment, flags, location, worktype) | objlist | `actors/<slot>.toml` | exists (core encoder) |
| schedules | schedule | inside each actor file | whole-file writer |
| object placement | objblk?? | `world/objects/<block>.toml` | exists |
| world map | map + chunks | `world/map.txt` (chunk ids as a grid) + `world/chunks.txt` | new |
| object types | basetile + tileflag + animdata + look.lzd | `objects/types.toml` | new (look needs the LZW encoder to match byte-for-byte) |
| conversations (NPC names, dialogue, prices) | converse.a/b | `converse/<slot>.u6c` (disassembled text) | assembler; round trip on all scripts |
| spells, reagents, creatures, combat, moongates | `game` at fixed addresses | `rules/*.toml` | in-place patcher of the executable bytes |
| art (tiles, portraits) | *.ega, portrait.* | PNG + palette map | later stage |

Exports are extracted content: they stay on the user's machine or in the user's browser; they are
never fixtures, test inputs, build artifacts or published files. Tests use synthetic definitions.

## 3.a — NPC actions offered to an AI model

Engine seam unchanged from rev 1 (03 §5): the NPC turn 0x1200b0 keeps one entry point and splits into
Perceive (owned `NpcView`) → Enumerate (`legal_actions`, no RNG, no mutation) → Decide
(`trait NpcBrain`, async, with a turn/state generation) → Validate + apply through the original
action path. `OriginalBrain` reproduces the original choice and is the default; `ModelBrain` is
opt-in per NPC or per area; invalid, oversized, timed-out or stale replies fall back to the
original routine for that turn and are logged; a slow reply never stalls the game loop (bounded
budget per turn, one request in flight per NPC).

Payload boundary: structured state and legal action ids only; no file bytes, no dialogue text
unless the user enables it per NPC; the exact JSON is shown before the first request and on
demand; no endpoint is preconfigured; keys stay in memory unless the user chooses to keep them.

Transport (open — security decision, trinity in progress 2026-10-15): the public page's CSP is
`connect-src 'self'`. Candidates: widen to `https:` + loopback for a user-entered endpoint; widen
to loopback only; native runner only. The browser receipt is behavioural, not header-based: with
AI off, zero off-origin requests during play; with AI on against a stub endpoint, exactly the
allowed fields are sent and the chosen id is applied.

## 3.c — mod packages

- Package: folder or zip with `mod.toml` (id, name, version, author, engine semver range,
  depends / conflicts / load-after) plus any subset of definition files; data only (no script
  runtime; a later plugin ABI gets no host network import).
- Apply: base definitions ← mods in deterministic order (dependencies, load-after, id), field-level
  merge by stable id (actor slot, object type, chunk, script slot, spell); conflicts reported; the
  result is built to bytes and pushed as layer `mods` between `base` and `save`. Bare-name lookup
  in `Files` means a mod may only contain the canonical paths the builder emits.
- Saves record the enabled set (a sidecar file in the save layer the original ignores); a save
  whose mods are missing refuses to load and lists them.
- Tools: `u6 export <disks> <dir>`, `u6 build <dir>`, `u6 validate`, `u6 new-mod <id>`; the page
  gets a mod manager (drop a mod zip, enable / disable / reorder, validation report).

## 3 — "고치기 좋도록" (code that is easy to change)

- One parser per format: delete the duplicates (u6-formats `ObjList` vs the core encoder;
  `ConverseLib` / `Portraits` / `Books` used only by tests) — the engine and the definition layer use
  the same parser and writer.
- Rights cleanup of the notes and module docs: replace the ~110 quoted game strings in
  `re/notes/functions-*.md` and module doc listings with role descriptions.
- Module docs list the definitions each module reads, so a change to a rule starts from its
  definition, not from an address hunt.

## Work units (one agent each, disjoint files)

| WU | Scope | Gate |
|---|---|---|
| P3-W1 | map + chunks writer and text form | export → build = identical bytes on the user's files |
| P3-W2 | basetile, tileflag, animdata, look.lzd writers | same |
| P3-W3 | converse disassembler / assembler (`.u6c`) | every script round-trips byte-identically; the interpreter runs the rebuilt bytes with identical receipts |
| P3-W4 | executable tables as `rules/*.toml` (in-place patch) | patch of unchanged values = identical `game` bytes |
| P3-W5 | `u6-defs` crate + CLI (export / build / validate / new-mod), schedule writer, actor files | full export → build of every file the engine reads = identical bytes |
| P3-W6 | mod packages, `mods` layer, save record, page mod manager | acceptance 3 |
| P3-W7 | NPC brain seam + OriginalBrain + ModelBrain + transport | acceptance 1, 2 |
| P3-W8 | duplicate-parser deletion, rights cleanup of notes, module docs | parity suite identical |

W1–W4 and W7–W8 run in parallel; W5 needs W1–W4; W6 needs W5.

## Acceptance (P3)

1. No mods, OriginalBrain: all P1/M2 receipts identical.
2. An NPC set to `ModelBrain` with a stub endpoint receives its legal list; its chosen action is
   applied; invalid / timeout / stale replies fall back and are logged.
3. A sample mod (synthetic values only) renames an NPC, changes a shop price and moves a chest;
   enabling it changes those in game without rebuilding the engine; disabling restores the
   original; a save made with it refuses to load without it.
4. Export → build of the base game is byte-identical for every file the engine reads.
