---
title: ultima6 — 04 P3 refactor (AI NPCs, editable systems, mods)
type: spec
tags: [ultima6, rust, refactor, ai, modding]
---

# 04 — P3 refactor: AI-driven NPCs, editable systems, mods

Status: planned (rev 10, 2026-10-16 — trinity rounds 1–3 on rev 1–4: gpt-6-astra and fable MUST-FIX folded in, both APPROVE at round 3. Rev 5 added an HD/4K seam; rev 6 removed it, after the grok-4.7 slot's round 1 on rev 5, because spec 07 built and shipped that seam in another form. Rev 7 folded in round 2 of all three slots: acceptance 7 keeps only what a v1 mod can affect and is gated by W6b, and §3.a records the transport decided 3 of 3. Rev 8 folds in round 3: grok-4.7 and fable APPROVE; gpt-6-astra's transport items (keys never in a URL or a log; gateway authentication; the owner gate with a real model) and fable's notes (the llmux half of W7b split into W7c, acceptance 7's last sentence); rev 9 folds in round 4: gpt-6-astra and grok-4.7 APPROVE; fable's two W7c items (an `Origin`-bearing request is never keyless even from loopback, since llmux exempts keyless loopback today; one allowed origin set in the gateway's config, so the preview gate can run) Rev 10 folds in round 5: gpt-6-astra and grok-4.7 APPROVE; fable's W7c gate item (a key that is not an issued key is refused too, since llmux serves an unknown loopback key as `local`) and the preview's stable alias origin. Starts after the M2 port is deployed (P2).

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
  in memory unless the user keeps them, never in a URL or a log; changing the destination or the field set asks again.
- **Transport:** decided by trinity on 2026-10-16, 3 of 3 in round 2 (grok-4.7, gpt-6-astra, fable). The CSP widening is a security change to a public site, so it ships only with the user's approval, asked when W7b is ready.
  - **Web = loopback only:** `connect-src 'self' http://localhost:* http://127.0.0.1:*`, every other directive unchanged. `http://[::1]:*` is added only if the owner's measurement shows it is needed. No `https:`: today the header is the one control that blocks third-party egress for every visitor, whatever the page's code does (`site/_headers:12`, a static origin with no receiver; pinned by `tools/web/test-web.mjs:189,199-202`). Reverting a header does not recall what left while it was open. No same-origin proxy: it would make our server the processor of game-derived payloads and keys (01 §Rights).
  - **Owner now = u6-native**, over plain HTTP to the owner's gateway. That needs no header change.
  - **Gate before the web path is offered:** one owner browser and one gateway, end to end with a real model. A public HTTPS page calls the owner's gateway at `http://127.0.0.1:<port>`, the gateway gets a reply from a real model, and the chosen action is applied to that NPC's turn. The stub receipts below are regression tests; they do not pass this gate.
    - The gateway binds loopback only and allows exactly one origin, set in the gateway's own config: the preview branch's stable alias `https://preview.ultima6.pages.dev` during the gate (each deployment's own hash subdomain changes), `https://ultima6.2lab.ai` after approval. It answers the CORS preflight (allowing the key header and `content-type`) and checks authentication on every request that carries an `Origin`, even from a loopback peer. The key is an issued data-plane key (llmux `lmk-`), never the admin key. Loopback, origin and CORS are not authentication: a simple POST is sent without a preflight. Whatever extra preflight header the browser's local-network policy asks for is set by that measurement, not named in advance. llmux has no CORS today; adding it is a llmux change.
    - "Loopback" means the machine the browser runs on. A remote gateway is mapped there (e.g. `ssh -L`).
    - If the call fails, the public page shows web AI as unsupported and keeps it off. The fallbacks are u6-native and the page served locally by `tools/web/serve.mjs`, which is loopback to loopback.
    - The page never widens to `https:` and never tells users to disable browser protections. A native or locally served success is not reported as public-web success.
  - **Consent screen:** it shows the address the browser connects to apart from where the model runs. If the gateway can forward to a hosted model, it says so; if that cannot be checked, it says "may forward to an outside provider; final location unconfirmed". The payload boundary above applies unchanged.
  - **Receipts:**
    - The `test-web.mjs` header assertion is updated to the decided value, and the `https://example.com/` probe stays refused.
    - A behavioural receipt is added next to it: AI off → zero off-origin requests during boot and play. AI on against a loopback stub → exactly the allowed fields are sent, the key appears in no URL, and the chosen id is applied. Invalid, oversized, timed-out and stale replies → the original routine decides and the reply is logged, without payload, reply bodies or the key.
  - **Docs:** the `site/_headers` comment and `workflow/repos.md` say "no third party; only the loopback endpoint the user enables".
  - Hosted models straight from the browser with a visitor's own key are not a requirement today. If the user asks for that, it is a new decision on a separate origin or a separate path with its own header.

## Code that is easy to change

- One codec per format, shared by engine, definitions and saves (objlist profiles unified; the
  test-only `ConverseLib` / `Portraits` / `Books` either become the engine's readers or go).
- Rules extraction as above.
- Rights cleanup: the ~110 quoted game strings in `re/notes/functions-*.md` and module doc
  listings become role descriptions.

## Output for HD / 4K

The user's 2026-10-15 requirement (x2/x4 resources, an HD/4K map view, more of the map shown; "not now") is owned by spec 07 and, for the wider view, by [`05-hd-output.md`](05-hd-output.md) §4. Spec 07 built the seam and shipped it: a per-pixel source plane in the frame buffers, the HD compositor reading the shown frame, the pointer on the logical 320×200, and the `u6-hdpack/1` format keyed by the classic image's content hash (`07-hd-art.md` Design 1–2; `u6-render/src/lib.rs:89`, `u6-render/src/hd.rs:499`, `u6-hdpack/src/lib.rs:58`). P3 does not reimplement any of it, adds no scene output, and changes neither `VIEW`, `WINDOW` nor `WINDOW_TILES` (05 §4 keeps the wider view as research). What P3 owes the HD output is acceptance 7. A v1 mod changes which records are drawn (map cells, object types, rules), never a record's pixels: art is out of v1, and the tile, portrait and bitmap files are not in the definition set. So the plane and the compositor apply unchanged, and the source receipt must still pass with a mod set loaded. Looking up a pack image by content hash belongs to 07 (its unit H2) and is tested there. The parity frame stays unchanged.

## Work units (one agent each)

| WU | Scope | Needs | Gate |
|---|---|---|---|
| W0 | contracts: definition / override schema, stable ids, codec API, save sidecar, load-order and conflict rules; the mod package's manifest `format` id, distinct from `u6-hdpack/1`, so that the page's drop classifier (07 H2) takes it as a third branch, not a second switch | — | spec review |
| W1 | map + chunks + basetile + tileflag + animdata writers, look.lzd table writer through the existing LZW encoder, text forms | W0 | local byte-identity |
| W2 | objlist codec unification (both profiles) + schedule writer + actor files | W0 | parity suite + byte-identity of both profiles |
| W3a | converse disassembler (opcode table from the interpreter's arms; data-in-code tables kept as data) + relocating rebuild (jump and table operands **and** the addresses of runtime script writes, `converse/exec.rs:268`, `converse/eval.rs:245` → `script.rs:41`) + name / price edits; a rebuilt script longer than `SCRIPT_LEN` 0x2800 is refused | W0 | identity rebuild of every script, **and** every name lengthened by N bytes → interpreter receipts identical except the names |
| W4 | executable tables as `rules/*.toml` (in-place patch) | W0 | unchanged values → identical `game` bytes |
| W5a | `u6-defs` library: export / build / merge / validate / sparse diff, wasm-callable (browser and CLI both call it; neither re-implements merge or validation) | W1–W4 | full export → build identical for every file the engine reads |
| W5b | CLI adapter: file I/O and the commands export, build, validate, new-mod, pack | W5a | each command on the user's files |
| W6a | mod resolver + save sidecar + package limits (read through `u6_intake::zip`: its budget, entry cap, inflate bound and CRC checks; no second zip reader) | W5a | acceptance 4–5 |
| W6b | page mod manager + end-to-end in native and in the browser; the source receipt (`main_loop/plane_receipt.rs`) run once more with acceptance 3's mod set as a `Files::overlay` layer | W6a, W5b, W8 (first family) | acceptance 3 (native and browser) and 7 (native, owner-local, counts only) |
| W7a | NPC seam + OriginalBrain | — | acceptance 1 + RNG-free enumeration test |
| W7b | ModelBrain + budget + transport (loopback, as decided in §3.a) | W7a (stub half); W7c (real-model half) | acceptance 2 with a loopback stub (before the user's approval); the §3.a owner-browser gate with a real model (after it, on a Pages preview carrying the widened header) |
| W7c | llmux (its own repository, its own PR and gates): loopback bind; one allowed origin from config; the preflight answer; a request carrying an `Origin` is never keyless, even from a loopback peer (keyless loopback requests without `Origin`, such as local CLI clients, keep today's `local` tenant) | — | from 127.0.0.1, on the API path W7b uses: a preflight and a request keyed with an issued data-plane key (`lmk-`) from the configured origin answered; any other origin refused; a request carrying the configured `Origin` refused when it has no key and when its key is not an issued key (llmux today serves an unknown key from loopback as `local`, `src/proxy/keys.rs:162-167`); a keyless request without `Origin` still served |
| W8 | rules extraction, one family per dispatch (missile weapons first) | W4 | behaviour test + parity |
| W9 | rights cleanup of notes and docs | — | grep: no quoted game strings |
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
7. With acceptance 3's mod set loaded as a `Files` layer, spec 07's source receipt still passes. Every unflagged tile, portrait or bitmap pixel with a current version equals its image's classic pixel; flagged, stale and classic pixels keep the classic output. The receipt runs native and owner-local and prints counts only. With HD off, the 1× frame is drawn by the same code as before HD whether or not a mod is loaded (its content differs only by what the mod changes).
