---
title: ultima6 — 18 remembered sight (grey memory of cells out of sight)
type: spec
tags: [ultima6, render, hd]
---

# 18 — Remembered sight

Status: shipped (2026-10-27, deploy 27); the owner's follow-up (kept off screen, fading by game days, unit C2) shipped 2026-10-28 in deploy 31 (main 234fc39). Saving the memory with the savegame is unit C3. Item 3 of `objects-menu-fog-motion/ssot.md`. Round-1 findings come from unit S1; the first build is unit C1 (branch `feat/remembered-sight`). This document describes the implemented contract. It is not a future target.

Owner's words (item 3, verbatim): "보이다가 시야에서 안보이면 지금 안보이는 부분이 그냥 사라지는데, 회색 표시로 전환해줘. (움직이는건 안보이고 안보이게 됐을때의 모습으로 b/w 어둡게 표시하도록 개선. (화면 밖으로 나가면 당연히 렌더링에서 제거."

Owner's follow-up (2026-10-27, verbatim): "화면에 안보이면 바로 없는게 아니고 한번이라도 본 지형은 계속 기억하도록해줘. 그리고 게임내 턴수에 따라서 천천히 없어지는데 1에서 0이 되는게 아니라 1에서 하루가 지날때마다 50%씩 까여 그럼 해당 타일의 50%만 그 디더링하듯 격자로 출력. 이틀 지나면 25% 남고 이렇게 3일 4일 6.25% 5일째되면 검게 표시하도록해줘 이렇게 구현"

## Measured facts (main 88eb5b0)

- The sight flood (0x109c12, `map/view.rs`) runs in mode Sight from the leader (`map/view.rs:389`) and adds light. Mode Sight also sets `CELL_VISIBLE` on each cell it reaches (`:309-311`). When the ambient light is below full daylight, light sources add light in a second flood (`:390-391`).
- A cell the sight flood never reached draws `NOT_VISIBLE`. A cell it reached but that has no light draws `DARK` (`map/view.rs:555-565`). Both look black on screen.
- While a light burns, the clock raises the ambient light to at least 4 (`actors/clock.rs:425-427`). So a view with a light source has no `DARK` cells. At night without one, most cells are `DARK` (S1: 37 of 121 lit in Original).
- The HD compositor receives a frame with a per-pixel source plane, not a tile list (`crates/u6-core/src/hd.rs:160`).

## Contract

- **What is remembered.** Each map-window cell that was in sight keeps its look from its last sight, with the game time of that sight: its ground, and its objects that do not move. Actors are never remembered, and neither are objects that move with an actor (`objects/moves.rs:29` `moves_with_actor`: parted objects, 0x1a9, the serpent). The look is drawn in dark grey, in the window and through the HD compositor.
- **Where it is kept.** `gfx::cache::SightMemory` (`gfx/cache.rs:164`), keyed by level and world cell. A cell stays remembered after it leaves the window and while the party is on another level. It shows again, in grey, when it is back in the window and out of sight. It is never saved.
  - Load state (0x105350, a restore) forgets it all: `RenderCache::forget_sight`, `gfx/cache.rs:118`, called at `saveload/restore.rs:90`. The saved memory is unit C3.
  - The window reset 0x133a96 (death, arrest, the healer) keeps it. The reset window shows the remembered cells.
- **Fading by game days.** The age is the game time since the cell's last sight, on the original's calendar: 60 minutes, 24 hours, days 1..28, months 1..12 (`minutes`, `gfx/cache.rs:245`). d is the whole days of 1440 minutes (`fade_of`, `:253`).

  | d (whole days) | 0 | 1 | 2 | 3 | 4 | 5 and later |
  |---|---|---|---|---|---|---|
  | pixels shown | 100 % | 50 % | 25 % | 12.5 % | 6.25 % | none: black, forgotten |
  | of a fully opaque 16 × 16 tile | 256 | 128 | 64 | 32 | 16 | 0 |

  A lit cell in sight is seen again at every framed render (its look taken again), so it is day 0. An unlit cell in sight (0x1bc) is not seen: it keeps the age of its last lit sight.
- **The pattern.** A fixed 4 × 4 ordered (Bayer) matrix on the original's pixel grid (`BAYER`, `gfx/cache.rs:232`; `keeps`, `:239`). It is anchored to the world: a cell is 16 pixels, so the cell's own column and row stand for the world's. Day d keeps the pixels whose threshold is below 16 >> d. Each day's pixels are a subset of the day before's, and 50 % is a checkerboard.
  - The window render first draws the cell's own tile (0xff or 0x1bc, black), then the remembered layers, writing only the kept pixels (`draw_remembered` and `blit_memory`, `gfx/view.rs:156` and `:107`). The overlay OR pass (masktype bit 2) obeys the same mask.
  - The missiles' redraw (`blit_remembered`, `gfx/view.rs:175`, called from `combat/present.rs`) and the cached window path draw the same way. The window cache keys a remembered box by its fade too.
  - In HD, only the kept pixels carry `FLAG_MEMORY`. The left-out pixels name the cell's own black tile, so the pattern holds at the original's pixel grid.
- **Frozen at last sight.** The shore merge 0x152dac rewrites map records in place (`gfx/anim.rs:121`, the only writer of record pixels after the load). A remembered layer of a record in the merge set, or of any record already rewritten (version not 0), keeps a copy of the record's pixels from that sight (`Frozen`).
  - The copies are an append-only store (`freeze`, `gfx/cache.rs:374`): one per record and content (pixels and transparency, compared in full behind a hash). A new version whose content was held before takes the copy it took then.
  - The owner's game makes 255 copies, of 32 records, over 10 000 animation ticks (the shore merges of the water's cycle; measured in round 2, counts only). So no copy is ever freed: a slot names one content until load state, and the window cache keys a copy by its slot.
  - A copy's image id is `FROZEN_BASE` + slot, at version 0 (`gfx/cache.rs:216`). `FROZEN_BASE` is 2048, past every record.
  - At most `MAX_COPIES` copies (8 192, `:223`), 32 times the measured need. When the store is full and a cell in sight needs a new copy, that cell is forgotten: it is black once out of sight. Its older look is not kept, because it is not the look of its last sight. A remembered layer never draws a rewritten record live.
- **Bounds.**
  - At most `MAX_CELLS` cells (131 072, `gfx/cache.rs:219`). Past it the oldest sights are forgotten first, down to seven eighths (`cap`, `:423`).
  - Cells of day 5 and later show nothing at once. A sweep forgets them (`sweep`, `:413`).
  - Two passes scan every cell, inside a framed render: the sweep, at most once per game hour, and the cap, only when the cells are over `MAX_CELLS` (then not again for the next 16 384 new cells). Every other render touches only the cells of the view.
- **The grey.** One function serves classic and HD: `u6_render::grey` (`u6-render/src/lib.rs:125`), the luma `(77r + 150g + 29b + 128) >> 8` × `GREY_LEVEL` 115 / 256, rounded.
  - In classic, palette entries `GREY + i` (0x20..0x3f) hold the grey of game colour `i`, written by the map window render only while the switch is on. With the switch off, entries 32..63 are left alone, so the frames match the original pixel for pixel (visual parity, not 256-entry parity).
  - In HD, a pixel flagged `FLAG_MEMORY` (0x80) is drawn from the grey copy of its HD image.
- **HD pack art.** With an owner's HD pack loaded, the pack's picture is looked up by the record a copy came from (`Images::pack_id`, `u6-core/src/hd.rs:347`). It is shown only when the copy's own pixels hash to the pack picture's key (`HdPack::hd`, `u6-core/src/hd.rs:498`).
- **Darkness.** An unlit cell that is in sight (0x1bc) also shows its remembered look (`MEMORY_IN_DARKNESS`, one constant to veto it). Its age runs from its last lit sight. An all-dark window that is not a reset keeps the memory.
- **In sight is unchanged.** A cell in sight draws exactly as it did before, whether the switch is on or off (`real_a_cell_in_sight_is_the_same_with_the_switch_on_and_off`, both builds).
- **The switch.** `Io::set_sight_memory` defaults to off, so tests, oracle comparisons and receipts see the original's frames. The page turns it on for each game. A visitor who stores `localStorage['u6.memory'] = 'off'` keeps it off.

## Cost (unit C2 round 2, measured on the owner's game, gate profile, source plane on, load average about 100 to 127)

- Tick at idle, after the door scenario (`receipt_the_tick_with_50k_remembered_cells`, run with `--ignored`), median of 600 ticks:

  | build | memory off | on | on, 50 000 more cells | the same at night |
  |---|---|---|---|---|
  | Wide (912 × 408) | 2.333 ms | 2.380 ms | 2.422 ms | 2.420 ms |
  | Original (320 × 200) | 0.389 ms | 0.400 ms | 0.412 ms | 0.415 ms |

  - The 50 000 more cells are off the view (one layer each, seen at the time of the injection). They are never drawn; the hourly sweep scans them.
  - The night case also sets the hour to 23 and the ambient light to 0, as the clock does then. The unlit cells in sight that the scenario remembered then draw their remembered look too: 10 885 grey pixels instead of 8 007 (Original), 30 036 instead of 23 722 (Wide). Cells never seen stay black, so this is not every cell of the window.
- Memory (`SightMemory::heap_bytes`: every table and buffer by its capacity, std's hashbrown bucket layout, not the allocator's overhead; native build):
  - 50 221 cells (Original) hold 3.09 MB, and 50 276 cells (Wide) hold 3.10 MB.
  - At the bounds (`the_bounds_hold_the_heap_under_24_mib`): `MAX_CELLS` cells of one layer each hold 11.8 MB, and 17.2 MB with `MAX_COPIES` copies.
  - On wasm32 the pointers are half the size, so the tables are smaller [추정: computed from the layouts, not measured in the page].
- Copies: about 0.5 KiB each. The owner's game makes 255 over 10 000 animation ticks, 0.13 MB.

## Residual

- A live wall piece next to a remembered cell may not match the frozen look next to it.

## Receipts

The receipts are in `objects-menu-fog-motion/loop.md`, under C1 and deploy 27. They cover:
- reviews in round 2 and round 3, plus round 3b;
- frames at `~/Games/ultima6/work/dispatch/r1027-r2/c1-frames/`, by day and night, and for shore and flight;
- the live check after deploy.

Unit C2's frames are at `~/Games/ultima6/work/dispatch/r1027-r2/c2-frames/` (round 2, the final code): one place on days 0..5 in classic and HD ×4, through the window cache, and a cell that leaves the window and comes back.
