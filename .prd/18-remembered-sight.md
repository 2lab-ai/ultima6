---
title: ultima6 — 18 remembered sight (grey memory of cells out of sight)
type: spec
tags: [ultima6, render, hd]
---

# 18 — Remembered sight

Status: shipped (2026-10-27, deploy 27); the owner's follow-up (kept off screen, fading by game days) is unit C2 (branch `feat/sight-fade`). Item 3 of `objects-menu-fog-motion/ssot.md`. Round-1 findings come from unit S1; the first build is unit C1 (branch `feat/remembered-sight`). This document describes the implemented contract. It is not a future target.

Owner's words (item 3, verbatim): "보이다가 시야에서 안보이면 지금 안보이는 부분이 그냥 사라지는데, 회색 표시로 전환해줘. (움직이는건 안보이고 안보이게 됐을때의 모습으로 b/w 어둡게 표시하도록 개선. (화면 밖으로 나가면 당연히 렌더링에서 제거."

Owner's follow-up (2026-10-27, verbatim): "화면에 안보이면 바로 없는게 아니고 한번이라도 본 지형은 계속 기억하도록해줘. 그리고 게임내 턴수에 따라서 천천히 없어지는데 1에서 0이 되는게 아니라 1에서 하루가 지날때마다 50%씩 까여 그럼 해당 타일의 50%만 그 디더링하듯 격자로 출력. 이틀 지나면 25% 남고 이렇게 3일 4일 6.25% 5일째되면 검게 표시하도록해줘 이렇게 구현"

## Measured facts (main 88eb5b0)

- The sight flood (0x109c12, `map/view.rs`) runs in mode Sight from the leader (`map/view.rs:389`) and adds light. Mode Sight also sets `CELL_VISIBLE` on each cell it reaches (`:309-311`). When the ambient light is below full daylight, light sources add light in a second flood (`:390-391`).
- A cell the sight flood never reached draws `NOT_VISIBLE`. A cell it reached but that has no light draws `DARK` (`map/view.rs:555-565`). Both look black on screen.
- While a light burns, the clock raises the ambient light to at least 4 (`actors/clock.rs:425-427`). So a view with a light source has no `DARK` cells. At night without one, most cells are `DARK` (S1: 37 of 121 lit in Original).
- The HD compositor receives a frame with a per-pixel source plane, not a tile list (`crates/u6-core/src/hd.rs:160`).

## Contract

- **What is remembered.** Each map-window cell that was in sight keeps its look from its last sight, with the game time of that sight: its ground, and its objects that do not move. Actors are never remembered, and neither are objects that move with an actor (`objects/moves.rs:29` `moves_with_actor`: parted objects, 0x1a9, the serpent). The look is drawn in dark grey, in the window and through the HD compositor.
- **Where it is kept.** `gfx::cache::SightMemory` (`gfx/cache.rs:162`), keyed by level and world cell. A cell stays remembered after it leaves the window and while the party is on another level. It shows again, in grey, when it is back in the window and out of sight. It is never saved.
  - Load state (0x105350, a restore) forgets it all: `RenderCache::forget_sight`, `gfx/cache.rs:118`, called at `saveload/restore.rs:90`. The saved memory is unit C3.
  - The window reset 0x133a96 (death, arrest, the healer) keeps it. The reset window shows the remembered cells.
- **Fading by game days.** The age is the game time since the cell's last sight, on the original's calendar: 60 minutes, 24 hours, days 1..28, months 1..12 (`minutes`, `gfx/cache.rs:251`). d is the whole days of 1440 minutes (`fade_of`, `:259`).

  | d (whole days) | 0 | 1 | 2 | 3 | 4 | 5 and later |
  |---|---|---|---|---|---|---|
  | pixels shown | 100 % | 50 % | 25 % | 12.5 % | 6.25 % | none: black, forgotten |
  | of a fully opaque 16 × 16 tile | 256 | 128 | 64 | 32 | 16 | 0 |

  A cell in sight is seen again at every framed render, so it is always day 0.
- **The pattern.** A fixed 4 × 4 ordered (Bayer) matrix on the original's pixel grid (`BAYER`, `gfx/cache.rs:238`; `keeps`, `:245`). It is anchored to the world: a cell is 16 pixels, so the cell's own column and row stand for the world's. Day d keeps the pixels whose threshold is below 16 >> d. Each day's pixels are a subset of the day before's, and 50 % is a checkerboard.
  - The window render first draws the cell's own tile (0xff or 0x1bc, black), then the remembered layers, writing only the kept pixels (`draw_remembered` and `blit_memory`, `gfx/view.rs:155` and `:106`). The overlay OR pass (masktype bit 2) obeys the same mask.
  - The missiles' redraw (`blit_remembered`, `gfx/view.rs:174`, called from `combat/present.rs`) and the cached window path draw the same way. The window cache keys a remembered box by its fade too.
  - In HD, only the kept pixels carry `FLAG_MEMORY`. The left-out pixels name the cell's own black tile, so the pattern holds at the original's pixel grid.
- **Frozen at last sight.** The shore merge 0x152dac rewrites map records in place (`gfx/anim.rs:121`, the only writer of record pixels after the load). A remembered layer of a record in the merge set, or of any record already rewritten (version not 0), keeps a copy of the record's pixels from that sight (`Frozen`).
  - Copies are shared by record and content: a new version with the same pixels reuses the copy.
  - Each copy counts the layers that name it, and is freed with the last one. Freeing costs O(the layers changed), not O(all cells).
  - A copy's image id is `FROZEN_BASE` + slot, at version 0 (`gfx/cache.rs:223`). `FROZEN_BASE` is 2048, past every record. Each copy has its own generation, which the window cache keys by.
- **Bounds.**
  - At most `MAX_CELLS` cells (131 072, `gfx/cache.rs:226`). Past it the oldest sights are forgotten first, down to seven eighths.
  - At most `MAX_COPIES` copies (8 192, `:229`). Past it a layer keeps the record itself.
  - Cells of day 5 and later show nothing at once. A sweep, at most once per game hour, forgets them and releases their copies. No render scans all cells.
- **The grey.** One function serves classic and HD: `u6_render::grey` (`u6-render/src/lib.rs:125`), the luma `(77r + 150g + 29b + 128) >> 8` × `GREY_LEVEL` 115 / 256, rounded.
  - In classic, palette entries `GREY + i` (0x20..0x3f) hold the grey of game colour `i`, written by the map window render only while the switch is on. With the switch off, entries 32..63 are left alone, so the frames match the original pixel for pixel (visual parity, not 256-entry parity).
  - In HD, a pixel flagged `FLAG_MEMORY` (0x80) is drawn from the grey copy of its HD image.
- **HD pack art.** With an owner's HD pack loaded, the pack's picture is looked up by the record a copy came from (`Images::pack_id`, `u6-core/src/hd.rs:347`). It is shown only when the copy's own pixels hash to the pack picture's key (`HdPack::hd`, `u6-core/src/hd.rs:498`).
- **Darkness.** An unlit cell that is in sight (0x1bc) also shows its remembered look (`MEMORY_IN_DARKNESS`, one constant to veto it). Its age runs from its last lit sight. An all-dark window that is not a reset keeps the memory.
- **In sight is unchanged.** A cell in sight draws exactly as it did before, whether the switch is on or off (`real_a_cell_in_sight_is_the_same_with_the_switch_on_and_off`, both builds).
- **The switch.** `Io::set_sight_memory` defaults to off, so tests, oracle comparisons and receipts see the original's frames. The page turns it on for each game. A visitor who stores `localStorage['u6.memory'] = 'off'` keeps it off.

## Cost (unit C2, measured on the owner's game, gate profile, source plane on, load average about 50)

- Tick at idle, after the door scenario (`receipt_the_tick_with_50k_remembered_cells`, run with `--ignored`):
  - Wide: 2.14 ms with the memory off, 2.19 ms on, 2.23 ms on with 50 000 more cells (C1: 2.56 ms on).
  - Original: 0.335, 0.344 and 0.354 ms.
- Memory: 50 276 cells hold about 2.96 MB of heap in the native build, by the tables' capacities. On wasm32 the pointers are half the size, about 2.5 MB [추정: computed from the layouts, not measured in the page].
- Copies: about 0.5 KiB each, at most `MAX_COPIES`.

## Residual

- A live wall piece next to a remembered cell may not match the frozen look next to it.

## Receipts

The receipts are in `objects-menu-fog-motion/loop.md`, under C1 and deploy 27. They cover:
- reviews in round 2 and round 3, plus round 3b;
- frames at `~/Games/ultima6/work/dispatch/r1027-r2/c1-frames/`, by day and night, and for shore and flight;
- the live check after deploy.

Unit C2's frames are at `~/Games/ultima6/work/dispatch/r1027-r2/c2-frames/`: one place on days 0..5 in classic and HD ×4, and a cell that leaves the window and comes back.
