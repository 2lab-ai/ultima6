---
title: ultima6 objects-menu-fog-motion — loop
type: loop
tags: [ultima6]
---

# Loop: objects-menu-fog-motion

Status: in progress (2026-10-27). One production deploy per item as soon as its receipt is green, then one per
follow-up (`ssot.md`).

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
| P1 | 1c | (later) | Korean lines for B10's new catalogue entries, on top of the live pack | B10 merged; message the row-25 session |
| M2 | 2 | `feat/context-menu` (from `integ/r1027` 41e8209) | spec 17: the query on a State clone, the icon-then-target run with its gate, the page menu | B6, B7 merged: done |
| W2 | 5 | (later) | spec 20: scene, presenter, early picture on a clone | C1, B2, K1 merged (gfx, walk.rs, engine.rs) |
| B13 | 1c | (later, after the units in flight) | replace the static scan with a compiler rule: clippy `disallowed-methods` on the image text readers, one string-registry module the catalogue is built from (opus, B10 r4). About 100+ production reader sites (`cstr` 32, `game_string` 31, `printf` 46) | B10, M2, W2, B12 merged |
| E1 | 1a | (round 3) | the `end` program (O1 row 10): decompile, port, launcher state 2 | — |

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
| B10 | r4: fmt ok, clippy 0 both, lib 1193, wide subset 195, port-map up to date (dispatcher) | r4: t APPROVE; w, e, r, f, x1, x2 REVISE (12 MUSTs, every one needing code the crate does not contain) | r4 APPROVE (3 SHOULDs) | Production code approved. The scan now has a written contract: exact for the crate's own idioms; exotic forms (`include!`, `#[path]`, leading `::`, `no_implicit_prelude`, user `From`/`Into`, block generic arguments, `use … as <module>`) are findings outright; a MUST needs a current site or an idiom the crate uses. The dispatcher's grep found no production text read outside `READERS`. r5 (`b10-r5.md`): binding scopes (closure, arm, `for`, `if let`), width-correct arithmetic, `named` bound to its const, empty reasons rejected, `static`, the rejection list, inline literals at byte accessors, `NOT_IMPLEMENTED` as English. r5 built: all ten items with RED fixtures; `tests_scan.rs` 5213 lines; lib 1212, wide subset 214 (agent). Next: the r5 review, ruled against the contract. |
| K1 | lib 1147, u6-web 44+1, u6-native 22, fmt ok, clippy ok both | a APPROVE; b APPROVE (r4) | APPROVE (r3) | committed (05765c9), merged, deployed (26) |
| C1 | r3b: fmt ok, clippy 0 both, lib 1159, wide 1182, u6-render 59, u6-web 44+1, port-map up to date (dispatcher) | r3: a, b, c, d APPROVE; r3b b APPROVE | r3 APPROVE; r3b APPROVE | committed (08925ae), merged, deployed (27); spec 18 shipped. Live: a door closed on a corridor leaves it in dark grey. |
| M2 | r1: fmt ok, clippy 0 both, lib 1186, wide 1211, u6-web 46+1, render 53, port-map up to date (dispatcher) | r1: a, c, d1, d2 REVISE; b conditional | r1 REVISE (2 MUSTs) | r1 committed on its branch (29e513d); `integ/r1027` merged in (ce911d4) so K1 is in its base. r2 (`m2-r2.md`): the Release passes the gate; 0xba/0xc0 Use stays enabled (the original searches anyway); one source for the Use refusals; the held press dropped when the prompt returns; deterministic refusals judged on the clone; one clone per press; the menu bounded to the screen; the pointer swallow; exact acceptance tests 1, 3, 4, 5, 7 and predicate equivalence both ways. Two review claims checked and dropped (underlined first is in the page; the solo reason means "not allowed in solo mode"). |

Carried (from K1 opus, outside K1): `main_loop/test_support.rs:89-97` builds test states with the key drain off,
unlike the image word 0x153caa = 1, so tests that queue several keys at once (`actors/rest/tests.rs:136,189,215`)
read them in order where a drained wait would merge them.

Integration: `integ/r1027` (worktree `.worktrees/integ-r1027`) holds B1, B3, B9, B11, B4, B2, B5, B8, B6, B7 and K1. Two kinds of merge conflict came up: two appended tests (both kept) and `port-map.md` (regenerated; `--check` up to date).

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

Carried to B12: when the staff cast (row 5) turns the text switch off, check every line it prints goes through
`queue_print` or `combat::present::say` (both test the switch since B2). The forced-on writes once noted here were
withdrawn: those addresses are thunks of 0x1231be, not switch writes. Only 0x124232 turns the switch off around
0x11a586.
