---
title: ultima6 — 18 remembered sight (grey memory of cells out of sight)
type: spec
tags: [ultima6, render, hd]
---

# 18 — Remembered sight

Status: shipped (2026-10-27, deploy 27). Item 3 of `objects-menu-fog-motion/ssot.md`. Round-1 findings come from unit S1; the build is unit C1 (branch `feat/remembered-sight`). This document describes the implemented contract. It is not a future target.

Owner's words (item 3, verbatim): "보이다가 시야에서 안보이면 지금 안보이는 부분이 그냥 사라지는데, 회색 표시로 전환해줘. (움직이는건 안보이고 안보이게 됐을때의 모습으로 b/w 어둡게 표시하도록 개선. (화면 밖으로 나가면 당연히 렌더링에서 제거."

## Measured facts (main 88eb5b0)

- The sight flood (0x109c12, `map/view.rs`) runs in mode Sight from the leader (`map/view.rs:389`) and adds light. Mode Sight also sets `CELL_VISIBLE` on each cell it reaches (`:309-311`). When the ambient light is below full daylight, light sources add light in a second flood (`:390-391`).
- A cell the sight flood never reached draws `NOT_VISIBLE`. A cell it reached but that has no light draws `DARK` (`map/view.rs:555-565`). Both look black on screen.
- While a light burns, the clock raises the ambient light to at least 4 (`actors/clock.rs:425-427`). So a view with a light source has no `DARK` cells. At night without one, most cells are `DARK` (S1: 37 of 121 lit in Original).
- The HD compositor receives a frame with a per-pixel source plane, not a tile list (`crates/u6-core/src/hd.rs:160`).

## Contract

- **What is remembered.** Each map-window cell that was in sight keeps its look when it leaves sight: its ground, and its objects that do not move. That look is the one from its last sight. Actors are never remembered, and neither are objects that move with an actor (`objects/moves.rs:29` `moves_with_actor`: parted objects, 0x1a9, the serpent). The remembered look is drawn in dark grey, both in the window and through the HD compositor.
- **Where it is kept.** `gfx::cache::SightMemory` (`gfx/cache.rs:157`) is keyed by world cell and level. It is never saved. These clear it all:
  - load state (0x105350);
  - the window reset 0x133a96 (death, arrest, the healer): `RenderCache::forget_sight`, `gfx/cache.rs:116-118`;
  - restore.

  A level change drops the cells of the old level. A cell that leaves the window is dropped, so off-screen cells are never drawn.
- **Frozen at last sight.** The shore merge 0x152dac rewrites map records in place (`gfx/anim.rs:121`, the only writer of record pixels after the load). A remembered layer therefore keeps the record version it was drawn at. For a record in the merge set, or any record already rewritten (version not 0), it keeps a copy of the record's pixels at that version (`Frozen`, `gfx/cache.rs:200-207`).
  - The copies are shared per (record, version).
  - A copy is dropped with the last layer that names it.
  - Each copy's image id is `FROZEN_BASE` + slot, at version 0 (`gfx/cache.rs:195`). `FROZEN_BASE` is 2048, past every record.

  On real data, 64 ticks rewrite 32 records, none of them outside the shore set. A test asserts it.
- **The grey.** One function serves classic and HD: `u6_render::grey` (`u6-render/src/lib.rs:125`), the luma `(77r + 150g + 29b + 128) >> 8` × `GREY_LEVEL` 115 / 256, rounded. In classic, palette entries `GREY + i` (0x20..0x3f) hold the grey of game colour `i`, written by the map window render only while the switch is on. With the switch off, entries 32..63 are left alone, so the frames match the original pixel for pixel (visual parity, not 256-entry parity). In HD, a pixel flagged `FLAG_MEMORY` (0x80) is drawn from the grey copy of its HD image.
- **HD pack art.** With an owner's HD pack loaded, the pack's picture is looked up by the record a copy came from (`Images::pack_id`, `u6-core/src/hd.rs:347`). It is shown only when the copy's own pixels hash to the pack picture's key (`HdPack::hd`, `u6-core/src/hd.rs:500`). Otherwise the default upscaled art is shown, in grey, which is what the live record showed at that sight.
- **Darkness.** An unlit cell that is in sight (0x1bc) also shows its remembered look (`MEMORY_IN_DARKNESS`, one constant to veto it). An all-dark window that is not a reset keeps the memory.
- **In sight is unchanged.** A cell in sight draws exactly as it did before, whether the switch is on or off. The test `real_a_cell_in_sight_is_the_same_with_the_switch_on_and_off` renders one state on two engines in both builds.
- **The switch.** `Io::set_sight_memory` defaults to off, so tests, oracle comparisons and receipts see the original's frames. The page turns it on for each game. A visitor who stores `localStorage['u6.memory'] = 'off'` keeps it off.
- **Missile redraw.** A missile's restore pass draws the remembered layers too.

## Cost (measured by the coder, re-read by the reviewers)

- Tick: 2.56 ms with the memory on, against 2.53 ms off.
- HD peak: 8.08 MiB against 7.69 MiB, out of a 24 MiB limit.
- Copies: at most one per remembered cell, about 0.5 KiB each, so about 66 KiB in Original and 430 KiB in Wide. In-sight churn is at most 32 copies per merge.

## Residual

- A live wall piece next to a remembered cell may not match the frozen look next to it.

## Receipts

The receipts are in `objects-menu-fog-motion/loop.md`, under C1 and deploy 27. They cover:
- reviews in round 2 and round 3, plus round 3b;
- frames at `~/Games/ultima6/work/dispatch/r1027-r2/c1-frames/`, by day and night, and for shore and flight;
- the live check after deploy.
