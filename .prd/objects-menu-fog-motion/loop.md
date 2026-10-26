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

Round 2 (implementation) is planned from these findings after the dispatcher has re-read every `file:line` claim.
