---
title: ultima6 — 04 P3 refactor (AI NPCs, editable systems, mods)
type: spec
tags: [ultima6, rust, refactor, ai, modding]
---

# 04 — P3 refactor: AI-driven NPCs, editable systems, mods

Status: planned (rev 5, 2026-10-15 — trinity rounds 1–2: gpt-6-astra and fable MUST-FIX folded in;
grok-4.7 slot pending; rev 5 adds the HD/4K seam (05-hd-output.md), W10 and acceptance 7, not yet reviewed). Starts after the M2 port is deployed (P2).

User command (verbatim, `browser-port/ssot.md`): "3. rust 코드를 고치기 좋도록 리팩토링 /
3.a npc의 가능한 행동을 ai model에게 주고 응답 받을수 있도록 / 3.b 캐릭터 월드맵 등 전체적인
시스템 쉽게 수정할 수 있도록 / 3.c 스팀 게임처럼 mod를 쉽게 만들수 있도록".

## Invariant

With no mod and no AI brain, every P1/M2 parity receipt (headless frame sequences + state traces of
03 §7 and the M2 receipts) stays bit-identical. Units that change engine code paths (rules
extraction, the NPC seam, codec unification) land only behind that gate.

## Measured facts (integrate/m2 `b312930`, 2026-10-15)

- Every game datum reaches the engine through `Files` (`u6-core/src/files.rs`): user data files
  (map, chunks, objblk??, objlist, schedule, basetile, tileflag, animdata, look.lzd, converse.a/b,
  portrait.*, book.dat, tiles, bitmaps, MED modules) or the user's `game` executable at fixed image
  addresses (palette, spell / creature / combat / map / object / inventory / UI tables). No game
  text, art or bulk table is compiled in.
- Typed tables already exist after parsing (`state.rs:169-236`), but conversations, portraits,
  books, bitmaps and songs are read lazily from `Files` after boot (`converse/script.rs:145,216`,
  `main_loop.rs:267`, `magic/book.rs:176`, `sound/driver.rs:81`), so an edit must exist as bytes.
- Compiled-in game knowledge = object-type / tile / worktype IDs from compare instructions
  (e.g. `combat/attack.rs:346` `MISSILE_WEAPONS`, the 58-arm Use switch in `usecmd/cases.rs`,
  ~150 scalar IDs).
- `Files` is a layer stack (`overlay` `files.rs:84`, top-down `get`); layers precede boot
  (`engine.rs:297`). `saveload::attach` puts `savegame/objlist` and `objblk??` on top
  (`saveload/store.rs:67-72`), so a save shadows any mod edit of actors or object placement.
- Writers: objblk (byte-identical on all 69 real blocks); objlist in two profiles — the shipped
  file is 0x1D73 bytes (`u6-formats/src/objlist.rs:14`), the game's save writes 0x1C73
  (`u6-core/src/saveload/objlist.rs`); single schedule records. The existing `lzw::encode`
  reproduces every real LZW stream byte-for-byte (look.lzd 1/1, converse.a 97/97, converse.b
  102/102, portrait.a/b/z 98/96/12; trinity measurement, counts only).
- Conversations: every script's name is terminated by an opcode (no slack byte, 200/200); jump and
  table operands are absolute script addresses (`converse/exec.rs:81`, `converse/eval.rs:211`);
  op 0xb4 reads word tables that sit inside the code. Shop prices are script operands / such
  tables. Renaming an NPC therefore means relocating the script.

## Decision 1 — definitions compile to files, plus a rules boundary (F+)

The editable form of each original file or executable table is a definition; `build` serializes
definitions back to the original byte formats into a `Files` layer, and the engine keeps reading
files. Rewriting loaders to read definitions (R) is rejected: the lazy readers above would still need
bytes, and it puts every loader behind the parity gate at once.

F alone does not deliver "고치기 좋도록": compiled-in rules stay address hunts. So P3 also extracts
rules into typed, immutable inputs (`CombatRules`, `UseRules`, …) whose defaults are read from the
user's `game` at the instruction operand addresses (no content enters the source), consumed by the
engine through one path that mod overrides also feed. First family: missile weapons
(`MISSILE_WEAPONS`), then the Use switch, then the scalar IDs by module. Each family is one unit
with a behaviour test (changing the definition changes the game) and the parity gate.

Bounds of F (validated, never truncated): 256 actor slots, fixed map size, fixed executable
layout. A rule lifted from code is overridden by patching its instruction operands, so its
cardinality stays the original's (e.g. 10 missile weapons; more is refused). Out of scope for v1:
new actor slots, larger maps, longer rule lists, new Use behaviours, new combat algorithms, and
art (tiles, portraits).

## Definition set

| Definition | Source | Text form | Writer |
|---|---|---|---|
| actors (names, stats, alignment, flags, location, worktype) + schedules | objlist (both profiles) + schedule | `actors/<slot>.toml` | unify the two objlist codecs; whole-file schedule writer |
| object placement | objblk?? | `world/objects/<block>.toml` | exists |
| world map | map + chunks | `world/map.txt`, `world/chunks.txt` | inverse of the fixed-size parsers |
| object types | basetile + tileflag + animdata + look.lzd | `objects/types.toml` | fixed-size writers; look via the existing LZW encoder |
| conversations | converse.a/b | per script: name, price words and other addressable fields (v1); full text form (later) | disassembler + relocating rebuild |
| spells, reagents, creatures, combat, moongates | `game` at fixed addresses | `rules/*.toml` | in-place patcher |
| rules lifted from code | `game` instruction operands | `rules/*.toml` | in-place patcher of the operand |

Unchanged files, compressed blocks and unknown regions are preserved byte-for-byte; a changed
part is checked by value (decode(encode(v)) = v) and by game behaviour. Byte-identity gates need
the user's files, so they are local-only tests that skip without the zip, like the existing
real-data tests.

## Rights: local export vs. shareable mods

- `export` output is extracted content: it stays on the user's machine or browser; never a fixture,
  test input, artifact or published file.
- A shareable mod is a **sparse override** computed by `u6 pack <mod-dir>` against the author's
  local base: stable id + changed field + new value only. It carries no original bytes, no old
  values and no generated archives; the consumer's tool rebuilds bytes from the consumer's own
  base. `pack` refuses a package field equal to the base value; a test proves packages contain no
  base snapshot. Whole-script replacement is allowed only for author-written dialogue.
- Conversation edits produce bytecode the existing interpreter runs; "data only" means no new
  general plugin runtime (a later plugin ABI gets no host network import).
- Package input limits: normalized paths, duplicate canonical names refused, bounded decompressed
  size and file count, malformed packages fail with no change to the running game or saves.

## Mods and saves

- Load order: `base` ← `mods` (dependencies, then load-after, then id; the manager's user order
  only reorders within those constraints) ← `save`. Field-level merge by stable id; conflicts and
  dependency cycles are reported and refuse the set.
- v1 lifecycle: changing the enabled set = reboot + start a new game, or resume a save made with the
  same set. No hot reload, no migration of existing saves.
- Each save carries a sidecar (in the save layer, ignored by the original) with the base
  fingerprint, schema / engine version, and the ordered mod list (id, version, content hash). It is
  committed in the same save generation as the state. Missing, changed or reordered mods refuse
  the save before boot and list the difference; a save with no sidecar loads only with no mods.

## 3.a — NPC actions offered to an AI model

- **Seam (W7a):** the NPC turn 0x1200b0 splits into Perceive (owned `NpcView`) → Enumerate
  (`legal_actions`, zero RNG draws, no mutation — tested) → Decide (`NpcBrain`) → Validate +
  apply through the original action path. `OriginalBrain` reproduces the original choice including
  its RNG draws (the worktype routines in `actors/npc.rs` mix choice and RNG), bit-identically.
- **Model (W7b):** `ModelBrain` is opt-in per NPC or per area. A request carries a turn/state
  generation; requests for all ModelBrain NPCs of a world round go out together at the round's
  start and share one round budget (one game future serves the whole round, so per-NPC budgets
  would add up); when it runs out, the original routine decides each unanswered turn. The
  generation is the restore / cancel epoch (03 §5), not a per-change counter: a reply computed
  from the round-start view is applied on that NPC's turn only if its action id is in the legal
  set recomputed at that moment; otherwise the original routine decides
  (rendering stays live throughout; turn order is unchanged); invalid, oversized, timed-out or stale
  replies fall back and are logged; one request in flight per NPC; bounded reply size.
- **Payload boundary:** structured state and legal action ids only; dialogue text only if enabled
  per NPC; exact JSON shown before the first request and on demand; no endpoint preconfigured; keys
  in memory unless the user keeps them; changing the destination or the field set asks again.
- **Transport:** open security decision (trinity 2026-10-15, then the user): the page's CSP is
  `connect-src 'self'`; candidates are `https:` + loopback for a user-entered endpoint, loopback
  only, or native only. Whatever ships, the browser receipt is behavioural: AI off → zero
  off-origin requests during play; AI on against a stub endpoint → only the allowed fields sent and
  the chosen id applied. The `test-web.mjs` header assertion stays, updated to the decided CSP
value; the behavioural receipt is added next to it.

## Code that is easy to change

- One codec per format, shared by engine, definitions and saves (objlist profiles unified; the
  test-only `ConverseLib` / `Portraits` / `Books` either become the engine's readers or go).
- Rules extraction as above.
- Rights cleanup: the ~110 quoted game strings in `re/notes/functions-*.md` and module doc
  listings become role descriptions.

## Output for HD / 4K

The user's 2026-10-15 requirement (x2/x4 resources, an HD/4K map view, more of the map shown; "not now") is designed in [`05-hd-output.md`](05-hd-output.md). P3 builds only its seam: the scene and asset-provider contracts and HD-pack type in W0, the scene output in W10, and acceptance 7. The parity frame stays unchanged.

## Work units (one agent each)

| WU | Scope | Needs | Gate |
|---|---|---|---|
| W0 | contracts: definition / override schema, stable ids, codec API, save sidecar, load-order and conflict rules, scene and asset-provider contracts, HD-pack content type | — | spec review |
| W1 | map + chunks + basetile + tileflag + animdata writers, look.lzd table writer through the existing LZW encoder, text forms | W0 | local byte-identity |
| W2 | objlist codec unification (both profiles) + schedule writer + actor files | W0 | parity suite + byte-identity of both profiles |
| W3a | converse disassembler (opcode table from the interpreter's arms; data-in-code tables kept as data) + relocating rebuild (jump and table operands **and** the addresses of runtime script writes, `converse/exec.rs:268`, `converse/eval.rs:245` → `script.rs:41`) + name / price edits; a rebuilt script longer than `SCRIPT_LEN` 0x2800 is refused | W0 | identity rebuild of every script, **and** every name lengthened by N bytes → interpreter receipts identical except the names |
| W4 | executable tables as `rules/*.toml` (in-place patch) | W0 | unchanged values → identical `game` bytes |
| W5a | `u6-defs` library: export / build / merge / validate / sparse diff, wasm-callable (browser and CLI both call it; neither re-implements merge or validation) | W1–W4 | full export → build identical for every file the engine reads |
| W5b | CLI adapter: file I/O and the commands export, build, validate, new-mod, pack | W5a | each command on the user's files |
| W6a | mod resolver + save sidecar + package limits | W5a | acceptance 4–5 |
| W6b | page mod manager + end-to-end in native and in the browser | W6a, W5b, W8 (first family) | acceptance 3, in native and in the browser |
| W7a | NPC seam + OriginalBrain | — | acceptance 1 + RNG-free enumeration test |
| W7b | ModelBrain + budget + transport | W7a, transport decision | acceptance 2 |
| W8 | rules extraction, one family per dispatch (missile weapons first) | W4 | behaviour test + parity |
| W9 | rights cleanup of notes and docs | — | grep: no quoted game strings |
| W10 | scene output next to the frame (map cells + UI widgets, logical units); view size read from state in new code | W0 | parity identical + scene/frame agreement on synthetic state |
| W3b | full conversation text form | W3a | later |

## Acceptance (P3)

1. No mods, OriginalBrain: all P1/M2 receipts identical.
2. An NPC on `ModelBrain` with a stub endpoint receives its legal list; its chosen action is
   applied; invalid / timeout / stale replies fall back and are logged.
3. A sample mod made with `pack` from synthetic values renames an NPC, changes a shop price, moves
   a chest, changes one map tile and one lifted rule; in a new game with it enabled all five show;
   disabled (new game) the original returns.
4. Two non-conflicting mods compose; two conflicting mods are refused with the field named; a
   malformed package is refused with no change.
5. A save made with a mod set resumes with that set; with a mod missing, changed or reordered it is
   refused before boot with the difference listed; a save without a sidecar loads only with no mods.
6. Export → build of the base game is byte-identical for every file the engine reads.
7. Every presented frame has a scene, and drawing that scene at 1× with the base assets reproduces the frame's map window and panels (synthetic state, then the real-data receipts as pass/fail).
