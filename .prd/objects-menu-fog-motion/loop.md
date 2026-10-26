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
| B12 | 1a | (later) | staff and wand casts, monsters included (row 5) | B2, B4 merged |
| P1 | 1c | (later) | Korean lines for B10's new catalogue entries, on top of the live pack | B10 merged; message the row-25 session |
| M2 | 2 | (later) | the right-press menu from M1 | B6, B7 merged (predicates read those handlers) |
| W2 | 5 | (later) | spec 20: scene, presenter, early picture on a clone | C1, B2, K1 merged (gfx, walk.rs, engine.rs) |
| E1 | 1a | (round 3) | the `end` program (O1 row 10): decompile, port, launcher state 2 | — |
