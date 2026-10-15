---
title: ultima6 browser-port — loop done
type: loop
tags: [ultima6]
---

# Loop done — browser-port (append-only)

## R1 — closed

Rust workspace, six M1 subsystems, boot and command loop, oracle, M1 deploy.

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
| WU-C0 | u6-core engine/State/RNG/seams (03 rev 2) | `65b421c`, `063b0f1`; 127 tests (re-run by dispatcher); zip→intake→State→Engine frame | done |
| WU-O | Amiga emulator oracle (AROS) | `6486e6b`, `7bf6d1a`, `9fc76d2`; floppies built from installu6's copy table; original reaches the world map via `--game-first` (launcher 1-byte patch + objlist 0x1C71/0x1C72 = 1); intro program `u` still stalls under AROS (not root-caused) | done (game path) |
| WU-P1 | gfx world view | port/gfx `57d6c8a`; 18 fns; throne-room frame read by dispatcher | merged → integrate/m1 |
| WU-P2 | map | port/map `43bb7a4`; 16 ported + 3 replaced; view/light grids from real data | merged → integrate/m1 |
| WU-P3 | objects | port/objects `8ec66c3`; 60/70 + 4 helpers; 16838 records load + write back | merged → integrate/m1 |
| WU-P4 | ui | port/ui `7faa720`; 25 ported, 3 partial, 10 stubs; roster + stats panels render | merged → integrate/m1 |
| WU-P5 | actors | port/actors `7004468`; 52/52; NPCs walk to schedules at 12:00 | merged → integrate/m1 |
| WU-P6 | converse | port/converse `086b9bf`; 20/20; 200 scripts; real NPC dialogue coherent | merged → integrate/m1 |
| WU-P7 | boot + command loop + seam reconciliation + web IndexedDB | port/main `6dce550`; 315 tests (re-run by dispatcher); start/walk/talk/web/reload receipts read by dispatcher; M1 406: ported 219, replaced 76, stubbed 65, planned 34 | done |
| WU-P7b | external review CHANGES (lifecycle race, unsupported-command honesty, canvas focus, memory, Attack targeting, error boundary) + `_headers` CSP | `a0f7443`, `ec598e0` (MUST-1/2/3, NICE-3, input limits, CSP); 10/10 browser tests (re-run by dispatcher); agent stopped by API 429 mid NICE-2 — WIP `0864d8b` on port/main | done — `0864d8b` (page notice: party/solo need leaving combat) merged `aa145b8`, production `a549b08a` |
| WU-D | Deploy ultima6.2lab.ai | main `a28f4c2` (merge of ec598e0) → Pages production `108af585`; custom domain ultima6.2lab.ai active (Vercel CNAME rec_b5896faf… → ultima6.pages.dev); live playthrough in headless Chromium: drop → walk → reload from IndexedDB, 10 requests all same-origin, 0 console errors | done (M1 interim) |

Open review items: gpt6 spec review CHANGES ×8 applied in `d749513`; WU-C0 notified.
TS intake deleted in `1a414a6`. Port worktrees gfx/map/objects/ui/actors/converse removed after the clean+on-remote+merged gate.

## M1 parity receipt — start frame (2026-10-14, dispatcher-read)

Oracle `~/Games/ultima6/oracle/shots/play-20261014-190647/ingame.png` (crop at (93,44), 320×200) vs
port `port/main` headless start frame. Receipts kept outside the repo:
`~/Games/ultima6/receipts/parity-start.png` (sha256 dd74b474…), `parity-diff2.png` (f769019e…).

- Raw RGB agreement is 4.2% because the emulator expands 4-bit channels non-linearly (15→240,
  mid values one nibble low) while the port uses n×17. Under a palette-index mapping
  (each port colour → majority oracle colour) agreement is **98.80% whole screen, 99.23% outside
  the map window**.

| Gap | Where | Cause | State |
|---|---|---|---|
| G1 | gargoyles in the throne room (oracle) absent in port | egg hatching 0x1319b2 + monster spawn 0x13121a are stubs | open (M2 with combat) |
| G2 | last command icon differs | icon reflects the combat flag set in the shipped DATA1 state | open |
| G3 | one NPC sprite + brazier flames | animation phase / idle-pass RNG timing | not a defect (timing) |
| G4 | pointer at (0,0) | emulator mouse sprite | not a defect |

## Interim deploy receipt — 2026-10-14 (user: "중간 보고하고 중간 산출물 정리해서 한번 먼저 올려줘")

- main `a28f4c2` = merge of port/main `ec598e0` (+ deploy script). Gates on the merge: fmt, clippy
  -D warnings, 323 tests, wasm 706,192 bytes, `node tools/web/test-web.mjs` 10/10.
- Cloudflare Pages: preview `7bf16d98` (checked first), production `108af585`; headers on every
  path: CSP `default-src 'self'; connect-src 'self'; script-src 'self' 'wasm-unsafe-eval' …`,
  nosniff.
- https://ultima6.2lab.ai: 200; headless Chromium with the user's zip → game started → 3 steps
  south → reload → restarted from IndexedDB; requests 10, foreign 0, console errors 0.

## Round 2 status — API rate limit 2026-10-14

All M2 agents and WU-P7b stopped on a server-side HTTP 429 ("retry in ~12,600 s"). Partial work
committed as WIP and pushed (not built): m2/combat `283a748`, m2/magic `c224a2a`, m2/usecmd
`d967d6a`, m2/inventory `ac4924a`, m2/saveload `269cec6`, m2/sound `260c2a6`, m2/intro
`3d4f493`, port/main `0864d8b`; m2/objcmds had no uncommitted files. Resume each unit from its
worktree when the limit clears.

## Redeploy — 2026-10-14

main `aa145b8` (merge of port/main `0864d8b`): 323 tests, web tests 10/10. Production deployment
`a549b08a`; https://ultima6.2lab.ai serves the corrected notice (party/solo listed under "not yet":
the shipped state starts in combat mode and leaving combat is not ported). Live playthrough:
requests 10, foreign 0, console errors 0.

## Round 2 — closed 2026-10-15

Milestone 2 shipped: main `624477d` (merge of integrate/m2 `093f583`), production deployment `332fdd9e`, https://ultima6.2lab.ai. Gates on 093f583: fmt/clippy clean, 762 tests (`--profile gate`), web 28/28, test_amifs + test_fight OK. Live check on production: 10 requests, 0 foreign, 0 console errors; frames read by the dispatcher (creation question screen, start fight at the oracle-measured pace, Journey Onward).

Review rounds after M2-review: round 1 = 10 fix units (both engines re-reviewed, APPROVE); round 2 = web, usecmd, objects, sound, tests, state, flash, pace, intro; round 3/4 = ready, sound (first launch is a new process), tests (no game values in tests, strict output rule, `--profile gate`), pace (held-walk cadence on the engine clock, release/move ordering, bounded take), webrace, combat-pace (fight and effects on the engine clock from oracle traces: start fight 277 vs 301 PAL frames per step, castle 33 vs 31). Ledgers in the session scratchpad review/rr2 and review/rr3.

## Round 2 — milestone 2 (rest of the game), dispatched 2026-10-14

Base origin/port/main `6dce550`. One worktree per unit (`.worktrees/m2-<unit>`, branch `m2/<unit>`).
`u` (intro executable) REA: 479 procedures, 317 strings, 479/479 decompiled (`~/Games/ultima6/re/dumps/u/`).

| WU | Scope | State |
|---|---|---|
| M2-combat | hunk 13 + Attack + combat mode + NPC combat worktypes + missile effects | m2/combat `114621d`; 53 + 7 hunk-18 fns; 334 tests; gargoyle fight messages match oracle wording | done → M2-int-a |
| M2-magic | hunk 11 + Cast + spellbook + reagents | m2/magic `4c4ba6f` + m2/magic2 `4814fc5`; all 22 remaining spell routines ported; 384 tests; 17 spell receipts | done → M2-int-b |
| M2-objcmds | hunk 15 Look/Get/Drop/Move/doors/traps + eggs/monster spawn | m2/objcmds `5eea93a`; 339 tests; start frame 99.20% vs oracle (seed 0x1d3), 98.76% seed 0; Move-to-member partial | done → M2-int-a |
| M2-usecmd | hunk 16 Use + per-object handlers | m2/usecmd `73300c7`; 53/59; 329 tests; 8 object kinds; duplicated 10 hunk-15 fns (dedupe in int) | done → M2-int-a |
| M2-inventory | hunk 8 inventory/Ready/Unready + 0x128ce8 panel + hunk 14 Rest | m2/inventory `3227cc3`; 332 tests; inventory panel 100.00% vs oracle (the hero and a companion, controls 85.6%/86.0%); Rest 8 h receipt; Ready/Unready reachable only by click 0x10619c → M2-mouse | done → M2-int-b |
| M2-saveload | Save/Restore, byte-compatible savegame, SaveStore trait | m2/saveload `ad43c95`; 333 tests; original's save loads in port, port re-save = original re-save except frame bits + 1 stack-garbage byte | done → M2-int-a |
| M2-sound | MED player + Paula emulation (u6-audio) + music state | m2/sound `0f7958c`; 362 tests; start song = song 7 ULTIMA_Da; envelope vs oracle audio r 0.952, tempo drift +0.01 s / 7.5 s; host wiring (AudioDriver + AudioWorklet) + 4 commands.rs edits pending | done → M2-int-b |
| M2-intro | `u`: intro, menu, character creation, hand-off | m2/intro `355253d` + m2/intro2 `59d1008`; 351 tests; opening (storm/stones/moongate), Introduction story, Transfer (U4 rost.bin / U5 saved.gam) ported; `u` port map 110 ported / 3 stubbed (music, local time); oracle intro unblocked by patching OwnBlitter/DisownBlitter (recipe ~/Games/ultima6/oracle/patch-intro2/) → static frames 100% vs original | done → M2-int-c |
| M2-mouse+ui | pointer input, ui mouse subtree incl. 0x10619c, pointer sprite, remaining planned ui/gfx | m2/mouse `6cfc225`; 359 tests, web 11/11; whole click-only playthrough (talk, walk, inventory, Unready/Ready (14,2)↔(0,16)); turned flight sprites; sprite alignment vs oracle unverified | done → M2-int-c |
| M2-int-a | merge combat+objcmds+saveload+usecmd, dedupe hunk 15, wire seams + commands.rs, ctrl keys | integrate/m2 `b324737`; 396 tests, web 11/11; Attack/B/Look/Get/Drop/Move/Use/Ctrl-S/Ctrl-R/overview live in the command loop; port map ported 357/599 | done |
| M2-int-b | merge magic2+inventory+sound, wire Cast/Rest/sound, web IndexedDB saves + AudioWorklet | integrate/m2 `b312930`; 543 tests, web 14/14, port map ported 471/599 (gates re-run by dispatcher 2026-10-15); Light/Magic Arrow/Rest/inventory/save→reload frames read by dispatcher | done |
| M2-int-intro | merge m2/intro2, run `u` first as the launcher does, persist a created character | int/intro `630777e`; launcher ultcli 0x100360 states read from asm (u → game → u on no character); 583 tests, web 16/16 (re-run by dispatcher); menu 100% vs oracle; create → reload → Journey Onward frames read by dispatcher | done |
| M2-int-mouse | merge m2/mouse, reconcile pointer/sprite/fill/spellbook seams, first click = audio start + game click | int/mouse `561b10b`; trap seam RED→GREEN, sprite/fill to gfx, hail sprites ported; 565 tests, web 16/16 ×3; Look/walk/spellbook click frames read by dispatcher | done |
| M2-int-c | dispatcher merges int/intro + int/mouse into integrate/m2 | integrate/m2 `4ab938d`; 605 tests, web 18/18 ×3 (dispatcher) | done |
| M2-review | external merge-gate review, 10 modules × (gpt-6-astra + opus) | 20 reports; sound APPROVE ×2; 9 modules CHANGES: panics on malformed files/saves (sprite first_row, party size, actor tables by object slot, -1 slots), fishing BorrowMutError, held mouse button never yields (walk burst, y/n and more prompts freeze), intro TV loop, items lost on failed Animate/scatter/balloon/Codex, egg cursor, stale-session save race; ledger in the session scratchpad review/findings.md | done |
| M2-fix | 10 fix units (`.worktrees/fix-<unit>`, branch `fix/m2-<unit>` from `4ab938d`): combat, state (actors+saveload), objects, magic, usecmd, inventory, sound, intro (+ intro music), mouse (+ 0x132338 shared seam), web; RED test per MUST-FIX | dispatched 2026-10-15 | in flight |
| M2-ship | merge fix branches → gates → re-review of the fix diffs → main → preview → live check → production → live check | after M2-fix | next |

## M2 follow-ups — rights + cpnits closed 2026-10-15

| Unit | Branch | Result |
|---|---|---|
| cpnits | fix/m2r4-cpnits | merged 2b4551b: Time Stop turns charge no combat-NPC time (asm 0x1200e8); trace.json carries keys + complete and trace.py refuses partial runs; tracecore.mjs + test_trace.mjs; lateness repaid one tick per wait (no wait ends more than its ending tick early); docs without game values; hunk lengths from hunk2raw's JSON |
| rights | fix/m2r4-rights | merged 3c2741e: no character or place names, messages, song titles, tables, bytes or values read from the game files anywhere in the repo (sweep 0 in owned files); mechanics vocabulary allowed (01-spec §Rights); mkadf by regions + sha256 (ADFs byte-identical); classify.py by string address (0 classification differences / 1383); palette and conversation checks structural |

Redeploy: main 2b4551b → production 8999b6ca; 765 tests (--profile gate), web 28/28; live check 10 requests, 0 foreign, 0 errors; frames read by the dispatcher.

## WU-P7b carried item closed 2026-10-15

- "Panel keys inside `ui::wait::read_key` do nothing silently" was closed by inspection on main `3349a83`. read_key now dispatches inventory scroll, Ready/Unready, the spellbook page and containers (`crates/u6-core/src/ui/wait.rs:135-155`).

## M2 round 5 — closed 2026-10-15

Playback and walkstep merged into main `a99c2bf` and deployed to production `36aa4826` (https://ultima6.2lab.ai).

| Unit | Branch | Result |
|---|---|---|
| playback | fix/m2r5-playback | merged `3ed544c` (head `800767e`). An action inside a more prompt (Ready, Unready, a trapped chest) plays its own lines and frames in place, while the outer print, its frames and the held newline are set aside and put back. Frames come due by printed marks (`present::printed_one`). The idle poll shows due frames in every wait (`show_recorded` removed). The idle-refresh quake plays in its prompt. PRINT_MS 22 (pooled 22.495 ms). Reviews: gpt6 ×4 and opus ×3 rounds, all APPROVE at the end. |
| walkstep | fix/m2r5-walkstep | merged `a99c2bf` (head `b094f0b`). The step's own time is STEP_MS 24 plus FOLLOW_MS 41 when a follower is eligible. Constants are pooled from r4+r5. COMBAT_NPC_MS 46 is charged by the routine a worktype runs (`npc::runs_combat_routine`). Followers are checked with follow()'s own guards. Receipts: castle 34 vs 33, start fight 302 vs 277, quiet held walk 1/1/1/2/7 vs 1/1/1/2/6. Reviews: gpt6 + opus ×2 rounds, APPROVE. |

Gates on `a99c2bf`:
- fmt and clippy clean
- `--profile gate`: 793 passed, 0 failed, 6 ignored
- build-web: wasm 1,759,472 B
- web 28/28
- test_amifs, test_fight and test_trace pass
- port_map clean

Live check2 on preview `cf99e9f2` and on production: 10 requests, 0 foreign, 0 errors. Frames read by the dispatcher: menu, game start, walk into the start fight with wound lines, Journey Onward after reload.

## webpace — closed 2026-10-15

| Unit | Branch | Result |
|---|---|---|
| webpace | fix/m2r4-webpace | Merged as `517b2ba` (head `de4e98f`).<br>**Root cause:** the clock seed, not the hold length. About 7% of boots leave a foe in the walked cell; the step is blocked and 0 cells result.<br>**Fix:** holds are timed on the engine clock; a direction retry is added; the pass-turn after a blocked sideways try is added.<br>**Pace check:** the original's fastest repeat, 500 ms from `pace/run-b1..b6`, is an independent floor, both cumulative and consecutive. The harness guards are labelled as such. The results table is in `re/notes/oracle.md` §4, and the `HELD_WALK_MS` doc matches it.<br>**Receipts on de4e98f:** 3 runs under 8× `yes` and 3 under `U6_FRAME_DELAY=150`, all 29/29.<br>**Reviews:** gpt6 ×3 and opus ×2, APPROVE. |

Gates on `517b2ba`: fmt, clippy, 793 tests (`--profile gate`), web 29/29. The wasm is unchanged at 1,759,472 B, so no redeploy was needed.


## intake-budget — closed 2026-10-16

| Unit | Branch | Result |
|---|---|---|
| intake-budget | fix/m2r6-intake-budget | Merged as `dca61f5` (head `43c747d`). See the notes below. |

- **What was built:** the intake's whole output is bounded at 64 MiB.
  - Every kept byte is charged by capacity: disks, labels, loose files and their keys, the report lines.
  - The staging lists and the sort scratch are charged.
  - 4,096 B of first allocations are reserved.
  - Give-backs are capped at the starting amount, with a `debug_assert`.
  - Names are compared in place.
  - The worst case is 57.5 MB.
- **Review rounds:**
  - 3–4 (opus): uncharged label and key copies, a 351 MiB report.
  - 5 (gpt6): length charged instead of capacity.
  - 6–7 (both): the nice items.
  - Final: gpt6 APPROVE, and opus APPROVE with all its hostile probes stopping at the budget.
- **Gates on dca61f5:** fmt, clippy, 856 tests (`--profile gate`), web 41/41; amifs OK; real-data identity PASS against 3349a83.
- **Deploys:** preview 3080f5b7, then production f6c06291. live-check2 on both: 10 requests, 0 foreign, 0 errors. The dispatcher read the Journey Onward frame.

## zip-slice — closed 2026-10-16

| Unit | Branch | Result |
|---|---|---|
| zip-slice | fix/m2r6-zip-slice | Merged as `0fff6c3` (head `0939b17`). See the notes below. |

- **What was built:** a dropped zip is read with `Blob.slice`: the central directory and the wanted disk entries only.
  - The end record is chosen once, on the original bytes, by one documented rule: containment, then best fit, then EOF, then the last one.
  - A directory must start with a central record and lie before its end records before it can rank.
  - The rebuilt zip is padded with a 65,535-byte zero comment, so it cannot choose its end record again.
  - The end of the file is read at most 3 times.
- **Review rounds 2–7** (opus and gpt6 each round). Planted records, fake Zip64 locators, the rebuilt-offset attacks XRB0/XRB1, a declared directory that does not exist (XR2), the read bound (XF, XN1), the directory limit (XN4), a directory running past its record (XN5).
- **Final:** both reviewers APPROVE. Opus's 60k fuzz found 0 disagreements and 0 panics.
- **Tests:** an end-record table of 20+ rows, and every rule's mutation fails its own row.
- **Gates on 0fff6c3:** 887 tests (`--profile gate`), web 45/45, amifs OK.
- **Memory, measured on the merged main:** wasm after start 192.2 → 13.9 MiB; ArrayBuffer peak 186.9 → 8.5 MiB; renderer RSS peak 517.6 → 157.9 MiB.
- **Deploys:** preview, then production 6847a75d. live-check2 found 0 foreign requests and 0 errors. The dispatcher read the walked-into-the-fight frame.
