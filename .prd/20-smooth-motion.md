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
2. **The scene.** At each framed map present the engine records a port-only `MapScene`:
   - the publication's engine time, the leader's cell and z, the view origin and size;
   - per cell the base word and the node words in list order, each with its rank (top, part, overlay) and its object
     slot (a `node_slot` table beside `node_tile`, filled in `draw_object`);
   - a "foreign" mark on a publication without a new scene: a missile, a flash, a quake, sparkle.

   The scene is a read-only copy; nothing in `State` or the RNG changes.
3. **The presenter** (`u6-render`, a `motion` module shared by web and native) holds the previous and the current
   scene.
   - **Camera:** if the leader moved by one cell (Chebyshev 1) on the same z, the camera slides with it. Otherwise it
     snaps: a teleport, a ladder, a level change, a switch of leader, a dissolve.
   - **Movers:** a slot drawn in both scenes, one cell apart, same z and same type, slides. Anything else snaps: it
     appears, disappears or changes type.
   - **Occlusion:** draw the static words without the movers, then the movers ordered by their in-between y then x, then
     the occluders over them. The occluders are the words after the mover in its old and new cells, and on a diagonal
     the side cells' top tiles, multi-tile parts and light overlays.
   - **Over the map:** the border and the rest of the UI come from the engine frame unchanged.
   - **New publications during a slide:**
     - a foreign one ends every slide;
     - a scene with the same positions lets slides run on;
     - a new move during a slide restarts from the drawn position, or snaps if that is over 1.5 cells away.
4. **The early picture (spec 19 O2, pure form).** Right after the leader's step (`actors/walk.rs:76-88`), the engine
   builds a scene on a clone of `State` (`update_view` and the draw lists on the clone; the clone is dropped). It hands
   that scene to the presenter at once.
   - The party's slide then starts at the key, not 540 ms later.
   - The authoritative publication at the first pick's refresh is then a scene with the same party positions, so
     their slides run on; NPCs that moved in that pick slide from there.
   - The preview never writes `Io`'s buffers or the window cache.
   - Its cost is one `State` clone per step, measured before it is kept (budget: under 1 ms at 8× with the held walk).
5. **Renderers.**
   - **Original, classic:** during a slide, re-render the map rect each display frame in software (about 12 × 12 cells).
   - **HD (Original and Wide):** per publication the engine blits the scene with a one-cell margin through
     `blit_back` (`gfx/blit.rs:98`) into scene-sized layers: static, movers and occluders. The compositor composes each
     layer once; each display frame then draws them with the camera offset, clipped to the shown rect, under the
     frame's border and UI.
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
| Actor entering or leaving sight | It is in one scene only, so it pops (spec 18's memory never holds actors). |
| A click during a slide | Mapped through the published cell grid. The held walk is relative to the centred leader, so it is unaffected. |
| Key type-ahead | Fixed by spec 19 O1 (unit K1), which lands before this. Otherwise one hold queues up to 49 slides. |
