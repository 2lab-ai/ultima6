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
- **Where it is kept.** `gfx::cache::SightMemory` (`gfx/cache.rs:182`), keyed by level and world cell packed in 32 bits (`cell_key`, `:244`). A cell stays remembered after it leaves the window and while the party is on another level. It shows again, in grey, when it is back in the window and out of sight. It is never saved.
  - A cell is 16 bytes (`Remembered`, `:236`): the minute of its last sight, its first layer, and whether it has more. A cell with more layers keeps all of them in a side table (`stacks`).
  - The minute is kept as a `u32` (`minute32`, `:260`; `age`, `:266`). Its horizon, `u32::MAX`, falls in the calendar's year 8 876: 8 715 game years after the start of the owner's game, so the game's clock never gets there in play (`real_the_minute_horizon_is_thousands_of_game_years_after_the_start`). The time saturates at the horizon too (`a_sight_before_the_minute_horizon_ages_through_day_5`):
    - a sight five days or more before the horizon ages through day 5 as any;
    - a sight less than five days before it stops aging at the day it has at the horizon, so it never reaches day 5;
    - a sight at or past it (a calendar set there, an edited save) stays at day 0.
    Such sights go only at load state or by the cap.
  - Load state (0x105350, a restore) forgets it all: `RenderCache::forget_sight`, `gfx/cache.rs:120`, called at `saveload/restore.rs:90`. It also drops the memoized window, whose key names copies by slot. The saved memory is unit C3.
  - The window reset 0x133a96 (death, arrest, the healer) keeps it. The reset window shows the remembered cells.
- **Fading by game days.** The age is the game time since the cell's last sight, on the original's calendar: 60 minutes, 24 hours, days 1..28, months 1..12 (`minutes`, `gfx/cache.rs:328`). d is the whole days of 1440 minutes (`fade_of`, `:336`).

  | d (whole days) | 0 | 1 | 2 | 3 | 4 | 5 and later |
  |---|---|---|---|---|---|---|
  | pixels shown | 100 % | 50 % | 25 % | 12.5 % | 6.25 % | none: black, forgotten |
  | of a fully opaque 16 × 16 tile | 256 | 128 | 64 | 32 | 16 | 0 |

  A lit cell in sight is seen again at every framed render (its look taken again), so it is day 0. An unlit cell in sight (0x1bc) is not seen: it keeps the age of its last lit sight.
- **The pattern.** A fixed 4 × 4 ordered (Bayer) matrix on the original's pixel grid (`BAYER`, `gfx/cache.rs:315`; `keeps`, `:322`). It is anchored to the world: a cell is 16 pixels, so the cell's own column and row stand for the world's. Day d keeps the pixels whose threshold is below 16 >> d. Each day's pixels are a subset of the day before's, and 50 % is a checkerboard.
  - The window render first draws the cell's own tile (0xff or 0x1bc, black), then the remembered layers, writing only the kept pixels (`draw_remembered` and `blit_memory`, `gfx/view.rs:156` and `:107`). The overlay OR pass (masktype bit 2) obeys the same mask.
  - The missiles' redraw (`blit_remembered`, `gfx/view.rs:175`, called from `combat/present.rs`) and the cached window path draw the same way. The window cache keys a remembered box by its fade too.
  - In HD, only the kept pixels carry `FLAG_MEMORY`. The left-out pixels name the cell's own black tile, so the pattern holds at the original's pixel grid.
- **Frozen at last sight.** The shore merge 0x152dac rewrites map records in place (`gfx/anim.rs:121`, the only writer of record pixels after the load). A remembered layer of a record in the merge set, or of any record already rewritten (version not 0), keeps a copy of the record's pixels from that sight (`Frozen`).
  - The copies are an append-only store (`freeze`, `gfx/cache.rs:504`): one per record and content (pixels and transparency, compared in full behind a hash). A new version whose content was held before takes the copy it took then.
  - For each setting of the animation flags (`seams::anim_mode`, which picks how each animdata slot runs), the store's size is a constant of the game data, not of play. A merged record's content is its load content with the masked pixels of the records its source water tiles draw now (`gfx/anim.rs:111-117`). Those records come from a finite frame set: each animdata frame is the animation counter `& and_mask`, a byte, shifted (`gfx/anim.rs:147-156`), so the frames repeat every 256 ticks. So no copy is ever freed: a slot names one content until load state, and the window cache keys a copy by its slot.
  - A copy's image id is `FROZEN_BASE` + slot, at version 0 (`gfx/cache.rs:282`). `FROZEN_BASE` is 2048, past every record.
  - At most `MAX_COPIES` copies (8 192, `:306`), about 0.6 KiB each with the index, 5.39 MB in all. When the store is full and a cell in sight needs a new copy, that cell is forgotten: it is black once out of sight. Its older look is not kept, because it is not the look of its last sight.
  - A layer with no copy is of a record outside the shore-merge set at version 0, which nothing rewrites after the load. The animation's frames repeat every 256 ticks, and a whole cycle of 256 ticks rewrites no record outside that set (`real_a_rewritten_record_outside_the_shore_set_keeps_its_look`, `gfx/view.rs:1814`). So no remembered layer of the shore-merge set, or of a record rewritten before its sight, draws its record live.
- **Bounds.**
  - At most `MAX_CELLS` cells at any time (229 376, seven eighths of 2^18, `gfx/cache.rs:293`). The cap below keeps at most `MAX_CELLS` − 1 before any insert. `layers` asserts it before each `put` in debug builds (`:629`).
  - The cap (`cap`, `:556`) acts after a render once the next render's cells (`BOXES`, 121 in Original and 777 in Wide, at most that many new) could pass `MAX_CELLS`. It forgets the oldest sights first, down to seven eighths (200 704). Among the sights of the cutoff's own minute, the lowest keys go first, so the cells kept do not depend on the table's order. A walk passes the cap only above 228 599 cells in Wide (229 255 in Original).
  - The table. It starts small and doubles as cells come. It can reach 2^19 buckets (11.0 MB of cells retained), also below the cap: a sweep or the cap leaves deleted entries, and those can force the 2^18 → 2^19 growth with more than 114 688 cells. It never grows past 2^19 buckets. A cap never shrinks it, and hashbrown (behind std's `HashMap`) rehashes in place, allocating nothing, while the cells after an insert are at most half a table's full capacity (seven eighths of its buckets). That is 229 376 for 2^19 buckets, and the build checks it with a const assert (`:299`).
  - **Retained, not peak.** The figures below are what the memory retains (`heap_bytes`). The one growth to 2^19 buckets is the peak: for that moment the old table (2^18 buckets, 5.5 MB) and the new one (11.0 MB) live together, 16.5 MB of cell tables. Measured with a counting allocator (`crates/u6-core/tests/sight_memory_peak.rs`, the copies as in play): peak 16 824 576 bytes over the start in Wide and 17 249 040 in Original; then 11 352 800 and 11 732 364 retained. With a full copy store the peak would be about 21.9 MB [computed]. The cap's scratch (`seen`, 917 504 bytes at the cap) lives at another moment. wasm32 linear memory keeps its high-water mark, so the page keeps the peak's pages.
  - **The retained ceiling, and its condition.** With `MAX_COPIES` copies the memory retains under 16 MiB while at most about 1.5 % of the cells hold a stack (more than one layer). Past 3 584 stacks, about 1.56 % of the cap, the stacks' table steps to 8 192 buckets and the ceiling no longer holds. The land walks measured 0.51 % and 0.52 %, the town walk 1.26 % (Cost below), close to the condition: a memory of towns alone could pass it, and then the retained figure goes over 16 MiB by about 8 KB (16 785 488 bytes [computed]). `the_bounds_hold_the_heap_under_16_mib` (`:1526`) asserts the ceiling at one cell in 100 stacked (`STACKED_LAND`, `:1469`) and at one in 67, 1.5 % (`STACKED_MOST`, `:1474`).
  - Five game days of a Wide walk over unseen land hold at most 169 794 cells (with a light burning at night), 74 % of `MAX_CELLS`, and the cap forgot none over either run. That is a measured five-day count, not a bound. The walk's geometric bound is the start's view (777 cells) per start and teleport plus 37 cells a step (a new row of the Wide view): 777 × (1 + teleports) + 37 × steps, 357 864 and 368 816 for the two runs, above the cap. The receipt asserts the cells held stay within it. A walk that shows more than the measured count (faster travel, a ship or the carpet, is not measured) passes the cap, and the oldest, most faded sights go first.
  - Cells of day 5 and later show nothing at once. A sweep forgets them (`sweep`, `:543`).
  - Three passes scan every cell: the sweep, at most once per game hour, and the cap, only when it acts (then not again for at least the next 27 896 new cells in Wide), both inside a framed render; and hashbrown's in-place rehash, when deleted entries use up the table's room. Every other render touches only the cells of the view.
- **The grey.** One function serves classic and HD: `u6_render::grey` (`u6-render/src/lib.rs:125`), the luma `(77r + 150g + 29b + 128) >> 8` × `GREY_LEVEL` 115 / 256, rounded.
  - In classic, palette entries `GREY + i` (0x20..0x3f) hold the grey of game colour `i`, written by the map window render only while the switch is on. With the switch off, entries 32..63 are left alone, so the frames match the original pixel for pixel (visual parity, not 256-entry parity).
  - In HD, a pixel flagged `FLAG_MEMORY` (0x80) is drawn from the grey copy of its HD image.
- **HD pack art.** With an owner's HD pack loaded, the pack's picture is looked up by the record a copy came from (`Images::pack_id`, `u6-core/src/hd.rs:347`). It is shown only when the copy's own pixels hash to the pack picture's key (`HdPack::hd`, `u6-core/src/hd.rs:498`).
- **Darkness.** An unlit cell that is in sight (0x1bc) also shows its remembered look (`MEMORY_IN_DARKNESS`, one constant to veto it). Its age runs from its last lit sight. An all-dark window that is not a reset keeps the memory.
- **In sight is unchanged.** A cell in sight draws exactly as it did before, whether the switch is on or off (`real_a_cell_in_sight_is_the_same_with_the_switch_on_and_off`, both builds).
- **The switch.** `Io::set_sight_memory` defaults to off, so tests, oracle comparisons and receipts see the original's frames. The page turns it on for each game. A visitor who stores `localStorage['u6.memory'] = 'off'` keeps it off.

## Cost (unit C2 rounds 3 to 5, measured on the owner's game, gate profile, source plane on, load average about 100 to 320)

- **Cells over five game days** (`receipt_five_days_of_a_wide_walk_fit_the_remembered_cells`, Wide, run with `--ignored`). The game is played by its own keys from a place on the land with the view open. Each step is the arrow key toward the nearest cell not yet seen. The party's hit points are kept and other actors are kept off, so no fight stops the walk. The clock runs as the game runs it, 0.72 to 0.75 game minutes a step.

  | walk (five game days) | steps | teleports | geometric bound | cells after 5 days | most held after a step | forgotten by the cap | with more than one layer | heap retained | 131 072 passed at |
  |---|---|---|---|---|---|---|---|---|---|
  | no light | 9 651 | 0 | 357 864 | 149 585 | 149 595 | 0 | 756 (0.51 %) | 5.73 MB | minute 6 413 (4.5 days) |
  | a light burning (night light level 4) | 9 947 | 0 | 368 816 | 169 762 | 169 794 | 0 | 876 (0.52 %) | 5.77 MB | minute 5 383 (3.7 days) |

  - The geometric bound is 777 × (1 + teleports) + 37 × steps: the start's view, then a new row of the Wide view a step. The measured counts are 42 % and 46 % of it, because the land and the walls hide much of each new row.
  - The receipt asserts that the cells held stayed within the geometric bound and under `MAX_CELLS` after every step, and that the cap forgot no cell over the whole run.
  - The town walk (the same receipt, Wide): from the place round the start whose view shows the most cells with still objects, within 48 cells of the party, until nothing unseen was in reach after 15 game hours. 1 507 steps, 0 teleports, 38 963 cells, 491 of them with more than one layer (1.26 %), 1.57 MB retained, geometric bound 56 536, the cap forgot none.

  - A full daylight hour remembers 1 258 cells on average without a light and 1 417 with one. The busiest hour remembers 3 723.
  - The worst case, 169 794 cells held, is 74 % of `MAX_CELLS`. So five days of continuous exploration on foot fit, and the five-day fade, not the cap, forgets them. Faster travel (a ship, the carpet) is not measured; past the cap, the oldest and most faded sights go first.
- **The copy store** (`real_the_copies_of_the_merged_records_are_a_constant_of_the_game_data`, in the gate). A copy of every merged record is taken after each animation tick: 255 copies of 32 records after 512 ticks, and still 255 after 1 024. Each copy is about 0.6 KiB, 0.15 MB in all.
- **Tick at idle**, after the door scenario (`receipt_the_tick_with_50k_remembered_cells`, run with `--ignored`), median of 600 ticks:

  | build | memory off | on | on, 50 000 more cells | the same at night |
  |---|---|---|---|---|
  | Wide (912 × 408) | 2.568 ms | 2.627 ms | 2.648 ms | 2.660 ms |
  | Original (320 × 200) | 0.469 ms | 0.479 ms | 0.487 ms | 0.493 ms |

  - The 50 000 more cells are off the view (one layer each, seen at the time of the injection). They are never drawn; the hourly sweep scans them.
  - The night case also sets the hour to 23 and the ambient light to 0, as the clock does then. The unlit cells in sight that the scenario remembered then draw their remembered look too: 10 885 grey pixels instead of 8 007 (Original), 30 036 instead of 23 722 (Wide). Cells never seen stay black, so this is not every cell of the window.
- **Memory** (`SightMemory::heap_bytes`: every table and buffer by its capacity, std's hashbrown bucket layout, not the allocator's overhead; native build):
  - A cell is 21 bytes a bucket: 16 for `Remembered`, 4 for its key and 1 control byte. Round 2's cell, with its own vector of layers, took about 90 bytes.
  - 50 221 cells (Original) hold 1.38 MB, and 50 276 cells (Wide) hold 1.38 MB (round 2: 3.09 and 3.10 MB).
  - At the bounds (`the_bounds_hold_the_heap_under_16_mib`): renders of a view's worth of new cells each, each followed by the cap, until twice `MAX_CELLS` cells were remembered. The cells are at most 229 295 at any time in Original and 229 215 in Wide, and the table is 2^19 buckets in both (counted from its capacity, a lower bound). With `MAX_COPIES` copies and the side table the memory retains, under the test's ceiling of 16 MiB (16 777 216 bytes):
    - one cell in 100 stacked: 16 600 176 bytes (Original) and 16 601 840 (Wide);
    - one cell in 67 stacked (1.5 %): 16 769 584 bytes (Wide), 3 088 stacks.
  - Round 3's cap, `MAX_CELLS` = 458 752 acting only past it, let the table grow to 2^20 buckets: the same test then measures 27 827 568 bytes.
  - `heap_bytes` counts a group's width of control bytes as 16 (SSE2's). It is 8 with NEON, as on this aarch64 machine, and on wasm32. On wasm32 the pointers are half the size too, so the tables are no larger [추정: computed from the layouts, not measured in the page].

## Residual

- A live wall piece next to a remembered cell may not match the frozen look next to it.

## Receipts

The receipts are in `objects-menu-fog-motion/loop.md`, under C1 and deploy 27. They cover:
- reviews in round 2 and round 3, plus round 3b;
- frames at `~/Games/ultima6/work/dispatch/r1027-r2/c1-frames/`, by day and night, and for shore and flight;
- the live check after deploy.

Unit C2's frames are at `~/Games/ultima6/work/dispatch/r1027-r2/c2-frames/` (round 2): one place on days 0..5 in classic and HD ×4, through the window cache, and a cell that leaves the window and comes back.

Round 2 first measured the copy store with a scratch count over 10 000 animation ticks: 255 copies of 32 records. Round 3 keeps the receipt in the gate, with the bound's reason in the contract (Frozen at last sight).
