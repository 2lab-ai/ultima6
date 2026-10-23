---
title: ultima6 hd-layout-font-register — loop done
type: loop
tags: [ultima6]
---

# Loop done: hd-layout-font-register

Append-only. The loop closed on 2026-10-23 after 9 production deploys; each deploy's scope, main commit and live receipt is a row of [loop.md](loop.md) §Deploys. Specs: item 1 = 10 (with 07's P1), item 2 = 11, item 3 = 12 (with 08 revs 15-19); all three are `Status: shipped`.

## Closed units

| Unit | Item | Spec | Merged | Deploy |
|---|---|---|---|---|
| R1 | 3 | 12 | `49a0be2` (+ guide fixes to `41ff1c8`) | 1 |
| L1 | 1 | 10 | `c5d8c39` | 2 |
| F1 | 2 | 11 | `ba20bf9` | 3 |
| F1c | 2 | 11 Design 3 rev. | `ba3a639` | 4 |
| K1, yes/no words, R2 | 3 | 08 rev 14-15, 12 | `0c080e8`, `82a9ab3` | 5 |
| L2 | 1 | 10 Design 2 | `4936c59` | 6 |
| block words, quotes, R3 | 3 | 08 revs 16-17 | `c321b5b`, `c10f9f9` | 7 |
| P1 | 1 | 07, 11 | `87c4814` | 8 |
| N1, N1c, R4 | 3 | 08 revs 18-19 | `6d757a1` | 9 |

Each unit's merge had the project gate re-run by the orchestrator and two external reviews (gpt-6-astra and opus) before it landed; review findings marked MUST-FIX were fixed and re-reviewed before the merge.

## Kept as they are (owner may veto)

- The HD layout draws no paper frame round the message scroll: the right column has no room for it.
- The opening and character creation (`u`) are letterboxed and keep the pixel font; they draw through another path.
- At first load a fragment of the pointer can show at the right edge until the mouse moves.
- Latin text in the HD font runs at its natural width and ends short of the original's cells.
- Where the game writes another colour over the paper (a clear's white on panel rows 6-7, the command underline), those pixels show classic, as every flagged pixel does.
- Korean pack ko-1023f: 3 lines still fail a check after two asks (names 2, quotes 1) and keep the earlier pack's text; 7 blocks share one word for a synonym pair of keys (sing/song and the like).
