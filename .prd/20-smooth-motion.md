---
title: ultima6 — 20 smooth motion between cells
type: spec
tags: [ultima6, render, hd, timing]
---

# 20 — Smooth motion

Status: planned (2026-10-27). Item 5 of `objects-menu-fog-motion/ssot.md`. Research and measurements: round 1 unit W1,
`~/Games/ultima6/work/dispatch/r1027-w1/motion.md` (runner and raw outputs in `runner/` beside it), and spec 19 (T1).
Presentation only: the engine keeps its cells, its RNG order and the original's timing (spec 06).

## Measured facts (main 587d973)

- The step is drawn late. A castle step publishes nothing for 540 ms after the key, then the whole step; the prompt
  returns at 620–640 ms, or 860 ms when the turn carries a clock minute (spec 19, measured). The first picture is the
  world turn's refresh after the first pick (`actors/turn.rs:617-620`, `main_loop/main_hunk.rs:41-48`).
- Every move publication outside combat comes after `AFTER_TURN_MS` = 427 ms (`actors/turn.rs:19`, paid at `:618`).
  In the start fight, no frame after a move came sooner than 18 PAL frames (81 sampled moves, W1 `fight-gaps.txt`).
- The quiet castle walk is 31–33 PAL frames a step (W1 and spec 06). A held key runs 58–120 ticks a step with NPCs in
  view; a held click 60–105 (W1).
- The compositor receives a flat per-pixel source plane (`crates/u6-render/src/lib.rs:130`) and composes the fixed frame
  (`u6-render/src/hd.rs:984`). The page draws only when the frame version, the view or the pointer changes
  (`site/app.js:1379`).
- The draw lists hold tile words only, no object slot (`crates/u6-core/src/objects/draw.rs:50-53`); `draw_object`
  knows the slot (`:274-275`).
- Building the view is not pure: when object type 0x188 first enters it, `update_view` draws from the game RNG
  (`objects/draw.rs:265-269`). So an extra view build in the real `State` (spec 19 O2 as first written) would move an
  RNG draw before the first pick.
- The engine's present already shows a margin: Original draws 8 px of the left outer column and 7 px of the right,
  Wide 7 px each side (W1 `border.txt`).

## Design

1. **Timing rule.** A slide starts when a move is published and runs linear for D = 18 PAL frames = 360 engine ms.
   - The clock is engine time plus the pacer's carried fraction, so 2× gives 180 ms and 8× 45 ms.
   - A held pointer button keeps 1×, so 360 ms (`pacer.rs:80-86`).
   - At Max, or when D at the current speed is under two display frames, there is no slide: the move snaps.
   - Why 360: under the 427 ms floor between world-turn pictures; no sampled fight move was cut; 55–58 % of the quiet
     step, near Ultima VII's medium walk of 400 ms per 16 px cell. Modern tile games use 100–270 ms but block input
     during the slide; this port must not block. Sources: W1 §2–§3.
2. **The scene.** At each framed map present the engine records a port-only `MapScene` (`Scene` in u6-render's
   `motion` module; u6-core's `Shot` (`gfx/motion.rs`) holds it with what the render drew in place of a base):
   - the publication's engine time, the leader's cell and z, the view origin and size;
   - per cell the base word and the node words in list order, each with its rank (top, part, overlay) and its object
     slot (a `node_slot` table beside `node_tile`, filled in `draw_object`);
   - a "foreign" mark on a publication without a new scene: a missile, a flash, a quake, sparkle.
   - Foreign pictures are found by the picture, not marked where they are drawn: each kept render carries a hash of
     the shown map's colours, and a publication whose shown map changed names the newest kept render of the same hash,
     or none (foreign). A fight's frames play back renders made earlier, so only the picture tells which render a
     publication shows; this covers missiles, flashes, quakes, the map overview and any later effect without a mark
     at each draw site. A render with the sparkle's XOR plots or presented dissolving holds a scene that is marked to
     snap: no slide into it.

   The scene is a read-only copy; nothing in `State` or the RNG changes.
3. **The presenter** (`u6-render`, a `motion` module shared by web and native) holds the previous and the current
   scene.
   - **Camera:** if the leader moved by one cell (Chebyshev 1) on the same z, the camera slides with it. Otherwise it
     snaps: a teleport, a ladder, a level change, a switch of leader, a dissolve.
   - **Movers:** a slot drawn in both scenes, one cell apart, same z and same type, slides. Anything else snaps: it
     appears, disappears or changes type.
   - **Occlusion:** draw the static words without the movers, then the movers ordered by their in-between y then x, then
     the occluders over them. The occluders are the words after the mover in a cell it is drawn in, and in every other
     cell its draws cover from the slide's start to its end (its old cell, a diagonal's side cells, the cells a restart
     up to 1.5 cells away sweeps) the top tiles, multi-tile parts and light overlays.
   - **Over the map:** the border and the rest of the UI come from the engine frame unchanged.
   - **New publications during a slide:**
     - a foreign one ends every slide;
     - a scene with the same positions lets slides run on;
     - a new move during a slide restarts from the drawn position, or snaps if that is over 1.5 cells away;
     - a preview held for 2000 ms (`PREVIEW_MS`) with no picture of the engine's, or a foreign picture after it, shows
       the engine's frame: the party is drawn where the engine's picture has it (it jumps back) until the turn's
       picture comes.
4. **The early picture (spec 19 O2, pure form).** Right after the leader's step (`actors/walk.rs:76-88`), the engine
   builds a scene on a clone of `State` (`update_view` and the draw lists on the clone; the clone is dropped). It hands
   that scene to the presenter at once.
   - The party's slide then starts at the key, not 540 ms later.
   - The authoritative publication at the first pick's refresh is then a scene with the same party positions, so
     their slides run on; NPCs that moved in that pick slide from there.
   - The preview never writes `Io`'s buffers or the window cache.
   - Its cost is one `State` clone and the view and draw lists on it per step (budget: under 1 ms at 8× with the held
     walk; the cost per step does not depend on the speed). Measured 2026-10-27: natively 0.19 ms median in Original
     and 0.29 ms in Wide (the early picture alone, 200 on the state after the start fight and the town steps, gate
     profile, `bench_the_state_clone_of_the_early_picture`); in Wide's wasm (the Game driven in Node, each step's whole
     tick, the least of 3 runs over 23 steps) a step's tick takes 1.35 ms median and 1.70 ms worst with smooth motion
     on against 0.94 and 1.24 ms off, so the early picture adds about 0.4–0.5 ms there.
   - There is no early picture in combat mode: a fight's frames (a missile's flight, a hit flash) can be published
     between the step and the turn's picture and show the party where it stood, so the party would slide, jump back
     and jump forward (seen in Wide at seed 7). In combat mode the party slides from the turn's picture.
5. **Renderers.**
   - **Original, classic:** during a slide, re-render the map rect each display frame in software (about 12 × 12 cells).
   - **HD (Original and Wide):** once per generation of the slides the engine blits the scene with a one-cell margin
     through `blit_at` (`gfx/motion.rs`: the draws of `blit_back`, `gfx/blit.rs:98`, at any pixel; equal to the engine's
     picture in the 422 scenes the slide-end receipt draws again from scratch) into scene-sized layers: static, movers
     and occluders. The compositor composes each layer once; each display frame then draws them with the camera
     offset, clipped to the shown rect, under the frame's border and UI.
     - The layers are the static words (the newest scene's view and a cell round it, one rect whatever cells the
       scenes hold; over colour 0, every pixel drawn), the cursor and border, and per mover its own words and the words
       over it. A layer pixel that the frame's composition wrote from the same colour and source takes that
       composition's block.
     - The static layer covers the shown map wherever the camera is drawn: the shown map lies half a cell inside the
       view and the layer reaches a cell past it, 24 px each side, and the camera is drawn at most 1.5 cells (24 px,
       `SNAP_PX`, `u6-render` `motion.rs`) from rest, since a slide starts one cell off and a restart farther than that
       snaps. `slide_layers` (`gfx/motion.rs`) asserts the bound at compile time.
     - The words over a mover are drawn over the static layer's pixels: where one ORs into the pixel under it (an
       animated overlay), the pixel takes the ground's colour, not the mover's, for the slide; its end shows the
       engine's frame.
     - The layers are made again on a new generation of the slides (see the edge case "Animation during a slide")
       and when the HD view's version moves (its switch, scale, pack or font; `u6-web` `place_slide`). A palette change
       alone makes nothing again: the layers of a running slide keep their colours until the next generation, and
       the slide's end shows the engine's frame.
     - The static and the frame layers go to the page as their changes from the ones the page holds: moved with the
       camera, then the 16 × 16 blocks that differ (a run of blocks under one of the same columns merged into it).
       Changes are made only against layers composed under the same HD view version and in the same palette
       (`SlideImages::make`); otherwise the game makes the layers whole, as after a pack, a scale or a switch change.
       The page reports the version it holds and applies changes only to that version; a page that holds none (a
       new canvas) asks for whole layers (`slide_reset`), and a report of layers made before the last ones, or
       before such a reset, holds nothing.
     - The HD frame itself stays the engine's; at a slide's end the page puts its map back.
     - **Cost in Wide (a measured deviation: the 8 ms target is not met).** At S 4 in the browser test (2026-10-28,
       load 24–28 on 18 cores; 20 frames each), a display frame that makes the layers as changes costs 10.7 ms median
       and 16.6 ms p95 in the fight and 8.9 / 9.5 ms out of combat (2026-10-27 at load 30–100: 10.5 / 14.8 and
       9.4 / 12.1). The first make after a start (the page holds no layers) is whole: 35.9 ms (31.6); so is the
       first after a pack, a scale or a switch change (the HD view's version moved). A frame that
       only draws the slide costs the page 2.6 / 3.4 ms in the fight and 2.5 / 3.0 ms out of combat. The timer leaves
       out the engine's step (1.35–1.70 ms a step in Wide's wasm), and p95 here is the 19th of 20 frames, not a
       maximum nor a whole frame's time. On main, without smooth motion, the Wide layout's compositor test (a whole
       composition of the Wide frame at S 4) measures 10.3–10.6 ms against the same 8 ms limit.
       It is accepted because it falls on a slide's first frame only, once a generation of the slides: out of combat
       about once a step (26 generations in 23 steps of the town runs, 2026-10-28), in a fight about seven times a
       step (216 in 31 steps of the fight runs: each turn's moves of the other combatants start their own), and a
       360 ms slide moves about 0.74 game pixel per 16.7 ms display frame, so a late frame shifts the picture by less
       than a game pixel. A
       later option is to rasterize only the cells whose draw lists changed (at most about 2.5 ms natively: the
       static layer's raster and its change scan).
   - **Remembered sight (spec 18) in slides.** Each scene keeps what its render drew in grey for each cell out of
     sight (the remembered layers and their fade, `cache::Shown`). A cell takes the look of the newest scene that holds
     it (the static layer), as the window render drew it then: its base word (black for a remembered cell), then the
     remembered layers in grey, each pixel only where the fade keeps it (`keeps`, as `draw_remembered`). So a slide's
     end is the engine's picture for faded cells too (the receipt ages remembered cells 0 to 4 game days).
     - The early picture's grey cells are read from the live remembered sight, unchanged, round the copy's view and
       at its time (`SightMemory::shown_in`): the copy's cache holds none, and the cells in sight are drawn live,
       so what that render would remember changes nothing it shows.
     - No slide crosses a load (load state 0x105350): the remembered sight's copies are forgotten there, which the
       scenes kept before name, so the engine drops its kept scenes and the next framed render's scene snaps
       (`Io::break_motion`).
   - **Page:** `draw()` also runs while a slide is active. The wasm exports the presentation clock.
6. **Switch.** Smooth motion on by default in the page, off in the engine and the web harness by default (as spec 18's
   switch). Off is today's picture exactly.

## Receipts

- **Purity.** With the switch on and off, after N castle turns and a start fight:
  - `State` is equal, the RNG included;
  - the authoritative publication stream is identical.

  The preview and the slides are a separate stream.
- **Timing.** At 1×, 2× and 8×, a step's slide length on the presentation clock (engine-time based) is 360/s ms within
  one display frame; at Max, no slide.
- **Frames.** A doorway pass and a diagonal step past a wall corner, as frame sequences (t0, mid, end): the mover stays
  under the doorway top and the corner.
- **Live.** A new visitor walks 5 cells by key and by click in Original, HD and Wide; the recorded frames show
  in-between positions, and the step-to-prompt time equals the switch-off run's.

## Edge cases (from W1 §5)

| Case | Behaviour |
|---|---|
| A blocked step | No cell changes, so no slide. |
| Boats, horses, two-tile creatures | Their slots slide. A turn in place or a type change snaps. |
| The party in a line | Moves in the leader's picture, all slide together. Tracking is by slot, so swaps work. |
| An object pushed with Move | Its slot slides. |
| Walk-cycle frame | The published frame is shown for the whole slide. A mid-slide stride frame is a later option. |
| Animation during a slide | The HD layers are made again only when what slides changes: a camera slide starts or restarts, a mover joins, restarts or leaves, the presenter starts again (a foreign picture, a snap, the switch). A publication that only lets the slides run on keeps them, so an animated tile or the clock keeps its frame for at most one slide; the slide's end shows the engine's frame. |
| Actor entering or leaving sight | It is in one scene only, so it pops (spec 18's memory never holds actors). |
| A click during a slide | Mapped through the published cell grid. The held walk is relative to the centred leader, so it is unaffected. |
| Key type-ahead | Fixed by spec 19 O1 (unit K1), which lands before this. Otherwise one hold queues up to 49 slides. |
