---
title: ultima6 objects-menu-fog-motion — loop
type: loop
tags: [ultima6]
---

# Loop: objects-menu-fog-motion

Status: in progress (2026-10-27). One production deploy per item as soon as its receipt is green, then one per
follow-up (`ssot.md`).

Merge rule (2026-10-28): a merge into a unit branch is built (clippy plain and wide) before the dispatcher commits it. d082854 (integ into `feat/smooth-motion`) was committed unbuilt and does not compile; fixed forward in W2 r5.

Owner page (using-epic-issue, opened 2026-10-27): http://192.168.50.49:8763/ultima6-improve-20261027.html, rendered
from `~/.local/share/ultima6-dashboard/state.json`, refreshed every 15 minutes and at each event.

## Round 1 (2026-10-27): research and audit, read-only

Every unit writes its findings under `~/Games/ultima6/work/dispatch/r1027-<unit>/`. A finding cites `file:line` and
original addresses; none quotes game text or names. Decompiles: `~/Games/ultima6/re/dumps/game/<addr>.json`; the
original under emulation: `tools/oracle`. Research notes that are not specs: `~/Games/ultima6/research/2026-10-27/`.

| Unit | Item | Spec | Output | Question |
|---|---|---|---|---|
| O1 | 1a | 16 | `stubs.md` | Every game-side `todo_stub!` site, plus stale seams (a callee ported elsewhere that the caller never calls): contract, who reaches it in play, fix size |
| O2 | 1b | 16 | `wrong-ports.md` | Every object type with its own case in Use, Look, Move, Get, the eggs, the gatehouse, the parts and the blocks: is there a real-data receipt? A case without one is a candidate, which the oracle decides |
| O3 | 1c | 16 | `lines.md` | Every string address that object code prints but no `PRINTED`, `LABELS` or `SHOWN` list holds; addresses only |
| M1 | 2 | 17 | `menu.md` | For each object kind, which commands the original allows, from its own checks; where the menu lives (engine, page or both); the right-button decision in `ssot.md` checked against `ui/click.rs` |
| S1 | 3 | 18 | `sight.md` | The paths from `view_flags` bit 7 to the black cell, in the classic renderer (`gfx/view.rs`) and the HD compositor (`u6-render`); a world-keyed memory design; cost per frame |
| T1 | 4 | 19 | draft `19-turn-model.md` | How a turn runs now (AS-IS, `file:line`), how the original designed it, how Ultima VII, Nuvie and modern turn-based games do it, what makes ours feel old, options with a recommendation |
| W1 | 5 | 20 | `motion.md` | Step intervals by actor and speed (measured), how Ultima VII and Exult and modern games interpolate, slide length, smooth scroll, compositor budget, speed multiplier and Max |

Results: all seven written (2026-10-27): T1's draft landed as `19-turn-model.md`, W1's design as `20-smooth-motion.md`. The dispatcher re-read the claims the round 2
briefs rest on (port side by `file:line`; the mantra centre from the bytes at 0x12edca..0x12ee54; the arrest's x and y
from the bytes at 0x11fd8c and decompile 0x1092fc).

## Round 2 (2026-10-27): build

Briefs: `~/Games/ultima6/work/dispatch/r1027-r2/` (`common.md` + one per unit). Base `587d973`. Each unit: RED first,
then two reviewers; the dispatcher merges reviewed units into one integration branch, runs `merge-gate.sh` once, then
deploys.

| Unit | Item | Branch / worktree | Scope | Needs |
|---|---|---|---|---|
| B1 | 1a | `fix/swamp-boots-readied` | `actors/party.rs` readied → `find_child` (O1 row 1) | — |
| B2 | 1a | `fix/print-queue-text-switch` | the print queue keeps the text switch (O1 N3); portcullis seam (row 2) | — |
| B3 | 1a | `fix/end-solo-wiring` | `actors/turn.rs` → `end_solo` (row 3) | — |
| B4 | 1a, 1b | `fix/arrest-confiscation` | arrest centre x/y swap; the confiscation loop (row 4, N1) | — |
| B5 | 1a | `feat/remains-overview-flood` | remains marker (row 6); overview flood mode 2 (row 7) | — |
| B6 | 1a, 1b | `feat/serpents-mantra` | serpent figure 0x12ff50 (row 8); mantra centred on the leader (O2 1) | — |
| B7 | 1a | `feat/move-carried-object` | Move of a carried object, 0x12ae04 + 0x12adc4 (row 9) | — |
| B8 | 1b | `fix/potion-target-word` | potion reads the full target word (O2 2) | — |
| B9 | 1b | `fix/lockpick-quantity-byte` | lock pick: quantity byte only (O2 3) | — |
| B10 | 1c | `feat/catalogue-object-lines` | O3's uncatalogued lines; static-scan test; uncatalogued counter | — |
| B11 | 1a | `tools/port-map-stale-seams` | `port_map.py` shows stale seams and multi-line stubs | — |
| C1 | 3 | `feat/remembered-sight` | spec 18 from S1 §3–§5 | — |
| K1 | 4 | `fix/held-key-drain` | spec 19 O1b: the poll keeps the newest press, a release cancels its repeats | — |
| B12 | 1a | `feat/staff-wand-casts` (from main bce1bfe) | staff and wand casts, monsters included (row 5) | B2, B4 merged: done |
| P1 | 1c | `~/Games/ultima6/i18n/ko-1027a` (a copy of ko-1026a) | Korean lines for B10's new catalogue entries, on top of the live pack. Export at main e0bc087: 469 game strings, 56 not in the live pack, all new since 1026a (`exports/ultima6-strings-1027-new.json`); dry run: 1 request, 931 characters. The `--send` run was refused by the session's auto-mode classifier; the owner allowed it in chat on 2026-10-28 ("해줘 허용함"). Sent 2026-10-28: 1 request in 22.6 s, 56/56 translated, 0 left English; the base is byte-identical to the live ko-1026a (d53e5234); the new pack is 6219fe23. Committed to ultima6-deploy 794e0a5 (index ko.file → ko-1027a.u6lp); zbrain-43 told. Live in deploy 31 (2026-10-28; `packs/ko-1027a.u6lp` served 200, 1 642 334 bytes; live19 with it LIVE OK) | B10 merged (done); message the row-25 session before the pack deploy |
| M2 | 2 | `feat/context-menu` (from `integ/r1027` 41e8209) | spec 17: the query on a State clone, the icon-then-target run with its gate, the page menu. Shipped in deploy 30 (main bd3e670) | B6, B7 merged: done |
| W2 | 5 | `feat/smooth-motion` (from `integ/r1027` ec3c4f6) | spec 20: scene, presenter, early picture on a clone. r1 (6ca722b; `integ/r1027` merged in, 6c180c3): purity holds (State and RNG equal, publications byte-identical with the switch on and off); end-of-slide frames equal the engine's picture in 420 scenes; in-between offsets measured in all three modes; doorway frames read by the dispatcher (half-cell scroll, the mover under the doorway top). Deviation: HD recomposes the whole frame each display frame, which costs Wide 5.18 ms median / 9.01 p95 / 14.9 max per present. Decided: Wide stays on by default, and r2 builds Design 5's per-publication layers. r2 also explains the early-picture spikes and writes the kept deviations into spec 20 (`w2-r2.md`). r2+r3 (27ca698, `integ/r1027` merged in, 063907d): per-generation layers sent to the page as block changes; Wide build frame fight 10.5 / 14.8 ms, out of combat 9.4 / 12.1 ms median / p95 under load (main 10.3–10.6 ms on the same 8 ms test). r3 review (gpt ×5, opus; `review/w2-r3-findings.md`): no production defect, the six r2 MUSTs closed in code; r4 (`w2-r4.md`) = switch-resume and return-step test holes, the Wide cost written into spec 20 as a deviation (accepted: once per step, under one game pixel), frame-layer scan skip, one invalidation mechanism, presenter pins. Gate r3 green through core (1334 / 1359), render 77, port-map; stopped before the web suite for r4 | C1, B2, K1 merged (done) |
| W3 | 5 | (after W2 ships) | held-walk continuity: the slide is fixed at 360 ms while a held step takes 620–660 ms quiet (58–120 ticks with NPCs in view), so a held walk glides then stops each cell (opus W2 r3). Make a held walk move without a stop, neither slower nor faster than the steps; research the rule first (Ultima VII walk, the step cadence), then spec 20 | W2 merged |
| K3 | 2 | `fix/hide-os-cursor` (from `integ/r1027` 60f92c6) | hide the OS cursor over the game screen, where the game draws its pointer (owner 2026-10-28). Brief `k3.md`. r1 8d1a303, r3 a5d1b17 (the page follows the engine's own pointer, mouse only), r4 65e8c39 (the invariant checked with HD off; the fullscreen Exit button shows the OS cursor). r4 reviews gpt and opus APPROVE, no MUST (open: assert the menu visible before Escape, and the hold-bound comment wording at app.js:3979-3982 — carried into M3 r2b, which rewrites those tests). Merged into integ (9bdead8, built); merge gate: core 1826 / wide 1851, 0 failed, web 165/166 ×3 (the known Wide timing test, 10.6–11.0 ms), oracles and port-map ok. Shipped in deploy 32 (main 52cc339, 2026-10-28; live32-cursor LIVE OK: data-pointer game and cursor none over the map, page and auto off the canvas; live30-menu and live19 LIVE OK, page errors 0). M3 must drop `swallowing` from `gameUnder` when it deletes the page menu (opus K3 r4) | done |
| M3 | 2 | `feat/ingame-menu` (from `integ/r1027` 60f92c6) | the context menu drawn in-game by the engine (font, window style, pointer and keys); the deploy-30 DOM menu deleted; spec 17 reworked; the "여기로 이동" entry calls M4's `travel_to` (owner 2026-10-28: "인게임 ui로"). Brief `m3.md` | M4 merged, for the entry |
| M4 | 2 | `feat/travel` (from `integ/r1027` 60f92c6) | "여기로 이동": A* over the leader's step rule, walked as real key moves, with stops; spec 21. Brief `m4.md` | K2 merged (input hook) |
| D1 | 2 | `feat/talk-clicks` (from `integ/r1027` 60f92c6) | Job and Name buttons under the conversation portrait, and a click on a coloured keyword types it; in-game UI; spec 22. Brief `t1.md` | — |
| D2 | 2 | `feat/message-scroll` (from `integ/r1027` 60f92c6) | message-window scrollback by wheel, with a scrollbar on the right; in-game UI; the live bottom stays the original's frame; spec 23. Brief `t2.md` | D1 merged, for keyword clicks in history |
| B13 | 1c | (later, after the units in flight) | replace the static scan with a compiler rule: clippy `disallowed-methods` on the image text readers, one string-registry module the catalogue is built from (opus, B10 r4). About 100+ production reader sites (`cstr` 32, `game_string` 31, `printf` 46) | B10, M2, W2, B12 merged |
| E1 | 1a | (round 3) | the `end` program (O1 row 10): decompile, port, launcher state 2 | — |
| C2 | 3 | `feat/sight-fade` (from `integ/r1027` b8ed317) | the owner's item-3 follow-up (`ssot.md`): remembered cells kept off the view and on other levels; fade by game days, 2^-d of the pixels through a 4 × 4 ordered pattern, black and forgotten from day 5; the window reset keeps the memory. Brief `c2.md`. r1 committed 86902a4, integ merged (80b3588); dispatcher gate: u6-core 1334 / wide 1359, u6-web 51 + 51 wide, render 59; fade frames read (day 0 grey, day 1 checkerboard, then 25 / 12.5 / 6.25 %, day 5 black). Review r1: gpt part b APPROVE; gpt part a REVISE (at MAX_COPIES a remembered layer falls back to the live record and can show later rewrites, confirmed cache.rs:402/306); opus REVISE (the window cache's fade key, the default live path, has no test). r2 brief `c2-r2.md` (also: copy ownership tests, a measured switch to an append-only copy store if ≤ 2048 contents, faded overlay and missile redraw tests, heap_bytes) r2 ed40e7b: the append-only copy store (255 (record, content) pairs measured against 8192), forget at the cap, the fade through the window cache; reviews gpt ×2 + opus APPROVE, no MUST (`review/c2-r2-findings.md`); gate re-run green (core 1340 / 1365, web 153/154, the known Wide timing test only); frames read (day 0 grey → days 1–4 dither → day 5 black). Merged into integ (60f92c6, tree = ed40e7b); shipped in deploy 31 (main 234fc39, 2026-10-28; live19 LIVE OK with ko-1027a, live30-menu LIVE OK, page errors 0). r3 (`c2-r3.md`): MAX_CELLS measured against 5 days of Wide exploration, the copy-bound receipt, the memo clear, a deterministic cap tie-break, test pins r3 30118e2 (measured cap, compact cells, memo clear, tie-break), r4 30961a0 (heap cap at 7<<15 cells, the minute horizon, lifecycle), r5 73ef5bc (self-checking cell bound, peak measured, stack share), r6 7349468 (tests and wording: peak before the cap, stack-count bound, in-place rehash): r6 reviews gpt ×2 release-ready and opus APPROVE, no MUST (`review/c2-r6-findings.md`; 16 MiB holds up to 3 326 stacks in the test's state, about 3 300 in play [computed]); dispatcher gate green (`c2-r6-gate.txt`). Merged into integ (30d01d5); merged-state gate (`deploy34/gate.txt` GATE_END): u6-core 1866 / wide 1891, 0 failed; web 172/173 twice and 171/173 once. The extra failure was the HD full-pack frame-time test while the 1-minute load was 131–170; a paired re-run (`deploy34/hdperf-pair.txt`) passed 3/3 on integ (S 4 p95 1.2–1.3 ms) and 2/3 on main (p95 1.2–1.4 ms; one run failed its walk check under load). Shipped in deploy 34 (main 97d437b, 2026-10-28): live files match the build (app.js d64ff9b2e0e6858b, pkg 0aca1fd9e164ee43, pkg-wide b2a23d7dde20bb7e); packs unchanged (ultima6-deploy 794e0a5); live19, live30-menu, live32-cursor and live33-held at 8× LIVE OK, page errors 0. r7 after the merge, tests and wording only: SHOULD 1, SHOULD 2, NITs a–e | C1 shipped: done |
| K2 | 4 | `fix/held-key-immediate` (from `integ/r1027` ba6cd09) | the owner's report (2026-10-27): a held arrow steps once, waits for the OS repeat delay, then walks on. A held direction key is taken again at every command prompt at once (one step a turn); other waits keep the browser's repeat. Brief `k2.md`. r1 (`k2-delta.diff`, 572 lines, uncommitted on ba6cd09): `Engine::held_press` adds a repeat of the newest held direction key when the command loop reads a command key (A4-0x7a2c) and no key waits; web and native; REDs for the held arrow with no browser repeats, the letter and line-editor controls, native. Coder's gate: u6-core 1327 / wide 1352, u6-web 57 both, native 23, web 154/155 (the Wide timing test). Dispatcher found one gap: a held arrow keeps walking under an opened context menu; the follow-up releases held keys when the menu opens (RED: 3 moves under the menu). Committed e98d469, integ merged (0fc44e9). Dispatcher gate (partial, stopped for r2): u6-core 1329 / wide 1354. Review r1: gpt REVISE (native Alt/Super-arrow becomes eligible after the modifier is let go, confirmed main.rs:432/491/615-630), opus REVISE (above 1× a tap walks several cells; every focus-taker, not just the menu, must release held keys). Decided: a 150 ms wall-clock tap gate (not 1× pacing, which would slow a held walk at 8×); release on the canvas blur; native press-time eligibility. r2 brief `k2-r2.md`. r4 03793fa (one step source at the prompt, paced presses only: `InputEvent::Paced`, at least `MIN_STEP_MS` 150 ms after the take before); r4 reviews gpt ×4 no MUST (gpt c's MUST rejected as a behaviour change, wording kept), opus APPROVE (`review/k2-r4-findings.md`). Merged into integ (099334f, built; d059d8e with main); merge gate: core 1858 / wide 1883, 0 failed, web 172/173 ×3 (the known Wide timing test, 9.8–10.8 ms), oracles and port-map ok. Shipped in deploy 33 (main d059d8e, 2026-10-28; live19, live30-menu, live32-cursor LIVE OK, page errors 0). live33-held (one keydown, no browser repeat, held 6 s, a step hook in the test browser only): at 8× LIVE OK — the first step 29–59 ms after the keydown, 4–8 cells, every prompt taken within 188 ms, nothing after the keyup; at 1× in the opening castle the first step 16–38 ms and every prompt taken within 38 ms, but the turn between prompts is 2.6–4.3 s (measured while two gates ran on the same machine), so 1–2 cells in 6 s: the held walk now waits on the turn, not the OS repeat (re-measure unloaded; W3 / spec 19 O2). r5 after the merge: opus S1 (a zip-free prompt-read test), S2 (an explicit predicate instead of Weak State in Io), N1/N2/N4 spec wording; gpt a-1, b-1, b-2, c-2, c-3, c-4, d-1, d-2, d-3 | K1 shipped: done |
| C3 | 3 | (after C2) | the remembered sight written beside the savegame as a port-only file and read back with it | C2 |
| M2-F | 2 | `feat/context-menu` | F0 (bd0eb13) + F0b (7345d88): the u6-web menu tests' press points from the shown map, so they hold in the wide build (the deploy-30 gate failed 3 there); both reviewers approve, merged into `integ/r1027` ba6cd09. F1 (cca6c84, test-only follow-ups): r1 gpt and opus REVISE; r2 (`m2-f1r2.diff`: the fresh-copy counter in `Press::check`, the said queue built by the engine's own `say`, a state check after each query, the panel count from the inventory) both APPROVE; r3 (`m2-f1r3.diff`: each slot checked against its own occupancy, RED by swapping the two flags; two comment NITs) both APPROVE. Dispatcher's gate on r3: fmt ok, clippy 0 both, u6-core 1314 / wide 1339, u6-web 51 both builds, render 59, port-map up to date, the keepFrame web test 1/1. Committed (4a61705), merged into `integ/r1027` (4e837a2) for deploy 31; worktree removed | M2 |

### Review state (2026-10-27)

| Unit | RED → GREEN (dispatcher re-run) | gpt-6-astra | opus | Open |
|---|---|---|---|---|
| B1 | 2 tests ok, fmt ok, clippy ok | APPROVE (after evidence file) | APPROVE | committed on its branch (17a697b) |
| B11 | test_port_map 10 OK, classify 24 OK, `--check` up to date | APPROVE (r2) | APPROVE (r2) | committed on its branch (e683da4) |
| B3 | turn tests 25 ok, fmt ok; agent full lib 1141 ok | APPROVE | APPROVE | committed on its branch (8c59cc0) |
| B9 | 1 test ok, fmt ok | APPROVE | APPROVE | committed on its branch (6a1330a) |
| B2 | 5 tests ok, fmt ok | APPROVE (r2) | APPROVE (r2) | M1 (`say` checks the switch) + SHOULD + NITs done — committed on its branch (6a5ce88) |
| B5 | 2 tests ok, fmt ok | APPROVE (r2, with opus trace) | APPROVE | light assert, docs, address NITs done — committed on its branch (177a629) |
| B8 | 1 test ok, fmt ok | APPROVE (r2) | APPROVE (r2) | bounded read kept port-only, documented; bit-7 tests — committed on its branch (06c5113) |
| B4 | 1 test ok, fmt ok | APPROVE | APPROVE | 2 SHOULDs + `obj_quality` doc done — committed on its branch (e95a936) |
| B6 | lib 1144 ok, fmt ok, clippy ok (scripts/cargo.sh) | APPROVE (r2) | APPROVE (r2) | M1 note from the asm, S1 six-cell test, N1, N2 done — committed on its branch (19e5b8d) |
| B7 | r3: B7 tests 11 ok default, 12 ok wide, fmt ok; clippy ok both | APPROVE (code r2, tests r3) | APPROVE (r2) | committed on its branch (97e3d10), merged into `integ/r1027` |
| B10 | r4: fmt ok, clippy 0 both, lib 1193, wide subset 195, port-map up to date (dispatcher) | r4: t APPROVE; w, e, r, f, x1, x2 REVISE (12 MUSTs, every one needing code the crate does not contain) | r4 APPROVE (3 SHOULDs) | Production code approved. The scan now has a written contract: exact for the crate's own idioms; exotic forms (`include!`, `#[path]`, leading `::`, `no_implicit_prelude`, user `From`/`Into`, block generic arguments, `use … as <module>`) are findings outright; a MUST needs a current site or an idiom the crate uses. The dispatcher's grep found no production text read outside `READERS`. r5 (`b10-r5.md`): binding scopes (closure, arm, `for`, `if let`), width-correct arithmetic, `named` bound to its const, empty reasons rejected, `static`, the rejection list, inline literals at byte accessors, `NOT_IMPLEMENTED` as English. r5 built: all ten items with RED fixtures; `tests_scan.rs` 5213 lines; lib 1212, wide subset 214 (agent). r5 review against the contract: gpt p1, p2, p3 APPROVE; opus APPROVE (every r4 item closed; header contract matches the code; production unchanged). SHOULDs and NITs for B13: the `<T>::f::<{1}>()` qself check (p1); the const arm tested only on empty `Consts` (p3); an unresolved site's arguments never checked; A4-offset literals at byte accessors not swept; a `Logic` reason goes false if that message is later printed (opus). Committed 115ed0c, merged into `integ/r1027` (2ec05d8). On the merged tree the scan finds 4 sites from other units: B7's two `print_at(line)` through `if let Some(line) = refusal` (`push.rs:438`, `:566`), a block-tailed `print(line)` (`usecmd/misc.rs:521`), and C1's `FROZEN_BASE = TILE_COUNT as u16` (`gfx/cache.rs:195`). Every const they print is listed, so only the site resolution is missing. r6: resolve those idioms in the scan, and give `FROZEN_BASE` a reason. r6 (`tests_scan.rs` only): immutable local bindings of `Some(CONST)` / `None` / blocks / `if` / crate fns returning `Option` resolve; `match`, `mut`, parameters and closures stay unresolved. r6 review: gpt p1, p2 APPROVE; opus APPROVE. Committed ed43cfa, merged, deployed (28). Every unit merged after it must keep its print sites resolvable. |
| K1 | lib 1147, u6-web 44+1, u6-native 22, fmt ok, clippy ok both | a APPROVE; b APPROVE (r4) | APPROVE (r3) | committed (05765c9), merged, deployed (26) |
| C1 | r3b: fmt ok, clippy 0 both, lib 1159, wide 1182, u6-render 59, u6-web 44+1, port-map up to date (dispatcher) | r3: a, b, c, d APPROVE; r3b b APPROVE | r3 APPROVE; r3b APPROVE | committed (08925ae), merged, deployed (27); spec 18 shipped. Live: a door closed on a corridor leaves it in dark grey. |
| M2 | r1: fmt ok, clippy 0 both, lib 1186, wide 1211, u6-web 46+1, render 53, port-map up to date (dispatcher) | r1: a, c, d1, d2 REVISE; b conditional | r1 REVISE (2 MUSTs) | r1 committed on its branch (29e513d); `integ/r1027` merged in (ce911d4) so K1 is in its base. r2 (`m2-r2.md`): the Release passes the gate; 0xba/0xc0 Use stays enabled (the original searches anyway); one source for the Use refusals; the held press dropped when the prompt returns; deterministic refusals judged on the clone; one clone per press; the menu bounded to the screen; the pointer swallow; exact acceptance tests 1, 3, 4, 5, 7 and predicate equivalence both ways. Two review claims checked and dropped (underlined first is in the page; the solo reason means "not allowed in solo mode"). r2 gate (dispatcher): fmt ok, clippy 0 both, lib 1206, wide 1231, u6-web 50+1, render 53, port-map up to date. r2 review: gpt a APPROVE; b, c1, c2, d1, d2 REVISE; opus REVISE. MUSTs: `InWait::drop` skips the restore silently when State is borrowed; r2 deleted the spec-19 held-arrow browser test (checked: absent in the worktree, present on main); the menu's `min-width` beats a narrower bound; the scroll and refocus claims untested; acceptance 3 derives its expected set from the predicates; Use-family equivalence one direction only; the aboard Cast pair not matched; Stays without an allowed case; and (opus) `magic/cast.rs:240` prints through a `match` the B10 scan cannot read. The dispatcher committed r2 (9590bbb), merged `integ/r1027` with B10 (ca5b14f), and ran the scan there: exactly that one finding. Locked doors and chests on the map get `Reason::Locked` (decided). r3 brief: `m2-r3.md`. r3 (items 0-22) and r4: the scan site rewritten; `InWait` restores loudly; the held-arrow test restored byte-identical; menu sizing, scroll, refocus and button-transition tests; literal chair pin; Use families in both directions with action checkpoints (11 families, 20 cases); the aboard Cast pair; one shared copy kept with an equality proof over 141 cells. Two spec findings, decided and written into spec 17 (main 29323c3): the real chair has no Use (type 0xfc is not among Use's 85, so the tier rule hides it; acceptance 3 amended), and type 0x4c hides Drop as it hides Move (the object move refuses it every time, `objects/moves.rs:11`). Coder's gate: u6-core 1310, wide 1335, u6-web 51, u6-render 59, web menu 9/9, held arrow 1/1. Frames read by the dispatcher (`m2-frames/menu.png`, `menu-phone.png`): the door offers Look and Use, with Cast greyed and its reason shown; at phone width the menu fits inside the box. r3+r4 review: four gpt parts (`m2-r4-prod`, `-rd-a`, `-rd-b`, `-web`) and opus on the whole. |
| B12 | r2: fmt ok, clippy 0 both, lib 1190 (dispatcher) | r1: a, b REVISE; r2: a REVISE (1 MUST, 2 NITs), b APPROVE (1 SHOULD) | r1 REVISE (1 MUST, 4 SHOULDs); r2 REVISE (1 MUST, 1 SHOULD, 3 NITs) | r2 closed: one index-16 accessor (`actors::party::roster_byte`, `leader_actor`), the Attack tail after the cast and its report, movement points paid after the cast, a test that fails if the text-off is deleted, pinned RNG and target. The Gate Travel premise was wrong: 0x104124 `moveq #0,d0; bsr 0x103fd4` resets the leader index to 0 before the follow loop, so the party forms round member 0 (dispatcher's byte check; opus withdrew its round-1 claim). r2 MUST (both reviewers): a deferred cast reorders `retreat`'s tail. The original 0x126738 runs the assault (line 51), then the recovery roll (line 63-64); the port rolls first. r3: an ordered after-cast list, every NPC caller of the hit audited, `Party::leader()` on the same model (u6-web `data-leader`, the pointer's viewer), damage.rs routed, member locations in the Gate Travel test, headers and wording. Then `integ/r1027` is merged in, so the string scan checks its print sites. r3 (9ccedae) and r4 (the report as an `if` chain the scan reads): APPROVE both reviewers; `integ/r1027` merged in (a959891). r5 NITs: "a wand's charge" wording; `bury` reads slot 16 through `member_actor`; `Port:` notes at the three unbounded `member_flags` writes (0x112052, 0x1123d6, 0x12a226 write A4+0x1370 + member; at 16 that is the high byte of `open_container`, which the port skips). Dispatcher gate: fmt ok, clippy 0 both, lib 1293, wide subset 241, port-map up to date. Committed b6b3449 on its branch, merged, deployed (29). |

Carried (from K1 opus, outside K1): `main_loop/test_support.rs:89-97` builds test states with the key drain off,
unlike the image word 0x153caa = 1, so tests that queue several keys at once (`actors/rest/tests.rs:136,189,215`)
read them in order where a drained wait would merge them.

Integration: `integ/r1027` (worktree `.worktrees/integ-r1027`) holds B1, B3, B9, B11, B4, B2, B5, B8, B6, B7, K1, C1, B10 and B12 (all deployed, 26-29). Two kinds of merge conflict came up: two appended tests (both kept) and `port-map.md` (regenerated; `--check` up to date).

Deploy 26 (2026-10-27): main 7a87fd2, fast-forward of `integ/r1027`, pushed.
- `merge-gate.sh`:
  - fmt and clippy pass in both builds.
  - Workspace tests: 1652 passed, 1676 with wide, 0 failed.
  - Oracles 8/8; port-map up to date; loop-lint 0 FAIL.
  - Web: 142 of 143, three runs. The one failure is the Wide compositor time-per-frame test at S 4: 10.40–12.00 ms against an 8 ms limit.
  - Control for that failure: run alone, alternating main and the integration branch at load ~100, it measured main 11.70 / 11.10 ms and integration 11.60 / 10.95 ms. The failure is machine load.
- Packs unchanged: `ultima6-deploy` 72aa055, `site/packs` byte-identical to deploy 25.
- Production deploy (wrangler): https://780de565.ultima6.pages.dev.
- Live check:
  - The live wasm (both builds) and `app.js` match the local build by SHA-256.
  - `live19.mjs`, run as a fresh visitor: LIVE OK; packs 200; Korean hits 23, misses 0; 0 page errors.
- Receipts: `~/Games/ultima6/work/dispatch/r1027-r2/deploy26/`.

Deploy 27 (2026-10-27): main ec3c4f6, which is deploy 26 plus C1 (remembered sight, spec 18 shipped), pushed.
- `merge-gate.sh`:
  - fmt and clippy pass in both builds.
  - Workspace tests: 1679 passed, 1703 with wide, 0 failed.
  - Web: 143 of 144, three runs. The one failure is the Wide compositor time-per-frame test at S 4, at 10.50 / 10.55 ms against an 8 ms limit.
  - Oracles 8/8; port-map up to date; loop-lint 0 FAIL.
- Control for the Wide failure: run alone, alternating main (deploy 26 code) and the integration branch, at load ~170-190. It measured main 10.75 / 11.90 ms and integration 10.90 / 11.60 ms. The failure is load, and C1 does not raise it.
- Packs unchanged: `ultima6-deploy` 72aa055, 20 files, byte-identical.
- Production deploy (wrangler): https://c59d3d3b.ultima6.pages.dev.
- Live check:
  - The live wasm (both builds) and `app.js` match the local build by SHA-256.
  - `live19.mjs`: LIVE OK; Korean hits 23, misses 0; 0 page errors.
  - `live-sight.mjs`, a fresh visitor in the default Wide HD layout: walk south 10, open the south door, close it. Open: the corridor beyond shows in colour. Closed: the same corridor shows in dark grey with its statues and walls, the look it had when it left sight. 0 page errors.
- Receipts: `~/Games/ultima6/work/dispatch/r1027-r2/deploy27/` (`sight-opened.png`, `sight-t1.png`).

Deploy 28 (2026-10-27): main e0bc087, which is deploy 27 plus B10 (the catalogued object lines and the string scan), pushed.
- `merge-gate.sh`:
  - fmt and clippy pass in both builds.
  - Workspace tests: 1757 passed, 1781 with wide, 0 failed.
  - Web: 143 of 144, three runs. The one failure is the Wide compositor time-per-frame test at S 4, at 9.30–10.70 ms against an 8 ms limit.
  - Oracles 8/8; port-map up to date; loop-lint 0 FAIL.
- Control for the Wide failure: run alone, alternating main (deploy 27 code) and the integration branch, at load ~25-27. It measured main 10.50 / 10.40 ms and integration 10.30 / 10.40 ms. B10 does not raise it. At this lower load main still measures above 8 ms, so load alone may not explain it (open, not part of this goal's items).
- Packs unchanged: `ultima6-deploy` 72aa055, 20 files, byte-identical.
- Production deploy (wrangler): https://90ead2b0.ultima6.pages.dev.
- Live check:
  - The live wasm (both builds) and `app.js` match the local build by SHA-256.
  - `live19.mjs`: LIVE OK; Korean hits 23, misses 0; 0 page errors.
  - The catalogue the engine exports grows from 413 to 469 game strings (56 new object lines, 0 removed). The live Korean pack holds the 413, so in Korean mode the 56 show in English until P1's pack ships.
- Receipts: `~/Games/ultima6/work/dispatch/r1027-r2/deploy28/`.

Deploy 29 (2026-10-27): main 87ae3f8, which is deploy 28 plus B12 (staff and wand casts), pushed. The main-into-integ merge after the gate touches `.prd` only.
- `merge-gate.sh` on b6b3449:
  - fmt and clippy pass in both builds.
  - Workspace tests: 1772 passed, 1796 with wide, 0 failed.
  - Web: 143 of 144, three runs. The one failure is the Wide compositor time-per-frame test at S 4, at 10.40–12.10 ms against an 8 ms limit.
  - Oracles 8/8; port-map up to date; loop-lint 0 FAIL.
- Control for the Wide failure: run alone, alternating main (deploy 28 code) and the integration branch, at load ~34. Both measured 10.40 ms, twice.
- Packs unchanged: `ultima6-deploy` 72aa055, byte-identical.
- Production deploy (wrangler): https://31043240.ultima6.pages.dev.
- Live check:
  - The live wasm (both builds) and `app.js` match the local build by SHA-256.
  - `live19.mjs`: LIVE OK; 0 page errors.
- Receipts: `~/Games/ultima6/work/dispatch/r1027-r2/deploy29/`.

Deploy 30 (2026-10-27): main bd3e670, which is deploy 29 plus M2 (the context menu, spec 17 shipped) and its test fixes F0 and F0b, pushed. The main-into-integ merge after the gate touches `.prd` only.
- `merge-gate.sh` on ba6cd09 (`deploy30b/`; the first try, `deploy30/`, failed 3 u6-web menu tests in the wide build, fixed by F0 and F0b):
  - fmt and clippy pass in both builds.
  - Workspace tests: 1812 passed, 1837 with wide, 0 failed.
  - Web: 153 of 154, three runs. The one failure is the Wide compositor time-per-frame test at S 4, at 10.40–10.90 ms against an 8 ms limit.
  - Oracles 8/8; port-map up to date; loop-lint 0 FAIL.
- Control for the Wide failure: run alone, alternating main (deploy 29 code) and the integration branch, at load ~69-79. It measured main 10.60 / 10.50 ms and integration 10.60 / 10.50 ms. The menu does not raise it.
- Packs unchanged: `ultima6-deploy` 72aa055, byte-identical.
- Production deploy (wrangler): https://629c8a76.ultima6.pages.dev.
- Live check:
  - The live wasm (both builds) and `app.js` match the local build by SHA-256.
  - `live19.mjs`: LIVE OK; 0 page errors.
  - `live30-menu.mjs` (production, Wide layout, ko-KR): a right press near the map's centre at the prompt opens the menu with the focus; 1 section, 2 entries (Look enabled, Cast disabled with its reason); the menu inside the screen box; the held right button moved over the map walks no one; Escape closes it with the leader where he was and the canvas focused; 0 page errors. Frame `d30-menu.png` read by the dispatcher: the leader's section, Look highlighted, Cast greyed with "no spellbook in hand" under it, Talk absent (no Talk to oneself).
- Receipts: `~/Games/ultima6/work/dispatch/r1027-r2/deploy30/`.

Carried to B12: when the staff cast (row 5) turns the text switch off, check every line it prints goes through
`queue_print` or `combat::present::say` (both test the switch since B2). The forced-on writes once noted here were
withdrawn: those addresses are thunks of 0x1231be, not switch writes. Only 0x124232 turns the switch off around
0x11a586.
