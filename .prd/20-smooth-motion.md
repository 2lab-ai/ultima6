---
title: ultima6 — 20 smooth motion between cells
type: spec
tags: [ultima6, render, hd, timing]
---

# 20 — Smooth motion

Status: W2 integrated locally; W3 implementation and validation in progress (2026-10-29). Item 5 of
`objects-menu-fog-motion/ssot.md`. Research: round 1 unit W1 and spec 19 (T1).
The remaining acceptance includes owner play at 1× and 4×; automated receipts do not decide the
owner's preferred pace. This round finishes step-end, hold, foreign-picture and menu-reentry checks.
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
  (`draw` in `site/app.js`).
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
   - A held pointer button keeps 1×, so 360 ms (`Pacer::rate`).
   - At Max, or when D at the current speed is under two display frames, there is no slide: the move snaps.
   - Why 360: under the 427 ms floor between world-turn pictures; no sampled fight move was cut; 55–58 % of the quiet
     step, near Ultima VII's medium walk of 400 ms per 16 px cell. Modern tile games use 100–270 ms but block input
     during the slide; this port must not block. Sources: W1 §2–§3.
   - A held walk (spec 19 O1c) steps every max(turn / speed, `MIN_STEP_MS` = 150) wall ms, and every paced step
     slides. Both receipts (`a_held_walk_slides_on_every_paced_step_and_its_moving_share_by_speed`; in the page,
     test-web's "a held walk in the page", read from `Game.slide` per display frame, not from the page's draws, which
     it makes only when the slide changed) count per display frame where the map is drawn: natively the camera of
     the view shown, in the page the static layer's placement (the static and the frame layer found by their roles,
     `Game::slide_layer_role`). A frame is off rest when the map is off its place; moving when it is also between
     the step's two cells (on the step's side of rest, within one cell) and closer to rest than on the step's last
     frame that reached the screen, which lay between them too (in the page, drawn onto the screen as well);
     backward when farther, once the step's slide began (a frame at rest before its first frame off rest is its
     onset); stray when off rest anywhere else. Every paced step needs two moving frames or more, none backward or
     stray, and no jump over the bound (round 12): the jump is the largest drop of the map's offset between two
     frames that reached the screen, from 16 (the old cell, where the step starts) to the first, a frame of the
     engine's after the slide began counting as at rest (0), except a foreign picture over the old
     cell (16), and one before onset not counted; the bound is
     ceil(16 · Δ / D) (`jump_bound`), Δ the engine ms a display frame runs and D the slide's 360 ms: 1 px at 1×,
     3 at 4×, 6 at 8× (the presentation clock moves Δ a frame, a slide's first frame lies within Δ of its start, and
     the offsets are rounded). In the page, besides, every frame whose placement or layers changed since the last
     that drew the static layer onto the screen draws it, each draw where `Game.slide` puts it. At 8× a slide is 45
     wall ms, 2.7 display frames: two frames off rest are sure (`slides_shown` shows no slide under two), the first
     with no frame of the step before it, so there one moving frame (`moving_needed`). A slide frozen at its start
     (`Track::offset` read at `start`), run backward in time, overshooting rest by half a cell, drawn on the wrong
     side of rest, snapped (`SNAP_PX` under a cell), cut short (16, 15, 14, then the engine's frame: a jump of 14) or
     drawn in 4 px steps fails both receipts; a page that draws the static layer every 4th frame, or a device pixel
     off its place, fails the page's (`the_step_receipt_counts_the_jump_onset_and_frames_not_shown` checks the
     counting itself on such frames). Every step is closed at the next leader change, or after the
     last release and drain, and must reach rest: a slide that moves twice then freezes short of
     rest is `unfinished`. Backward motion is judged against the last frame actually shown, including
     an engine frame at rest; an unshown frame cannot make the count rise. Both twins run hand-made
     sequences that pin these rules. The receipts measure the map, which the camera moves, not the movers: a
     companion's or an NPC's own slide is not counted frame by frame (the frames and slide-end receipts draw them,
     Design 5, and the start room's hold receipt checks the presenter's newest scene on the leader's cell). The largest
     jump measured over the paced steps (2026-10-29): 1, 3 and 6 px at 1×, 4× and 8× in Original and in Wide; 1 px in
     the page's start room. The slide keeps its 360 engine
     ms, so the map moves on part of each step. Measured in the castle fixture (the assault's actors passed over),
     the down arrow held 6 s on 60 Hz frames, per step over the frames to the next, median and range; "at rest" is
     the presenter's picture with the map at its place (an early picture held until the turn's, or other objects
     sliding), the other frames show the engine's frame (2026-10-29):

     | speed | a step every (mean, Original / Wide) | build | off rest | moving | at rest |
     |---|---|---|---|---|---|
     | 1× | 746 / 757 wall ms | Original | 41 % (40–57) | 29 % (29–38) | 20 % (19–28) |
     | | | Wide | 50 % (40–51) | 33 % (27–37) | 24 % (19–24) |
     | 4× | 185 / 189 wall ms | Original | 50 % (38–60) | 40 % (31–50) | 23 % (15–30) |
     | | | Wide | 50 % (38–55) | 40 % (31–45) | 23 % (15–30) |
     | 8× | 156 / 156 wall ms | Original | 22 % (20–22) | 11 % (10–11) | 22 % (10–22) |
     | | | Wide | 22 % (20–22) | 11 % (10–11) | 22 % (10–22) |

     The fewest moving frames in a step: 14 at 1×, 4 at 4×, 1 at 8× (both builds). In the page's start room (HD on,
     1×, out of the start fight, from `Game.slide`), where a step takes 3.4 s and more, the two paced steps followed
     by another were off rest on 10 % of their frames, moving on 7 % and at rest on 5–7 %. (Round 9's 61–78 %
     counted the frames at rest as moving, round 10's the frames off rest, and its page figures the page's redraws.)
   - **Decision (W2's): D stays 360 ms of the presentation clock.** The constraints that fix it:
     - The floor: outside a fight a world turn's pictures come no sooner than `AFTER_TURN_MS` = 427 ms apart
       (Measured facts). A 360 ms slide ends before the next turn's picture, so each turn's moves are seen whole.
     - The next step is unknown when the slide starts. A held step's cadence is max(turn / speed, 150) wall ms, and
       the turn's length is known only when its prompt comes back, after its slide was published with its length.
     - A slide longer than the cadence lags. Each new step restarts it from where it is drawn, farther behind the
       leader each time, until the restart is more than `SNAP_PX` (1.5 cells) off and snaps (`far` in
       `Presenter::publish`): the jump smooth motion exists to remove. 360 ms is under the cadence at every speed a
       slide is shown at (at 8×, 45 wall ms against 150).
     A held walk that moves without a stop between its steps is unit W3's (`objects-menu-fog-motion/loop.md`, row
     W3); spec 19 O1c's "Measured live" is the same 1× symptom seen from the held key.
   - **The held walk on the real clock (rounds 10–11).** In the start room out of the start fight (HD on, 1×, the
     page's fixed seed), the virtual display's receipt above walks a paced step in 206 and 207 frames: 3433 and 3450
     ms (the paced steps followed by another). On the browser's own clock in the same room, K2's instrument
     (`watchWalk`, `walkReceipt`) with each step's time, the ms the pace dropped and combat mode (test-web's "the
     held walk on the real clock", the down arrow held with no browser repeat; four runs, 2026-10-29, two alone and
     two in the whole suite):
     - motion on: paced steps of 4450, 5967 and 3650 wall ms; 4517, 6033 and 4533; 4450, 5966 and 4617; 4517, 6033
       and 4533;
     - motion off: 4517, 5583 and 4533 wall ms; 4517, 7283 and 3733; 4517, 6033 and 4533; 4517, 6033 and 4533;
     - in every run the engine ms within a frame of the wall ms, 0 ms dropped, out of combat at every step, no late
       step, each paced press taken 17–33 ms (one or two frames) after the prompt came back.

     The receipt asserts, motion on and off, that no step leaves the prompt up with a paced press due (`late` 0) and
     that each paced step's engine ms lie within two display frames of its wall ms less the ms the pace clamped
     (`Game::clamped_ms`: a host stall past `CAP_MS`, pacer.rs), both ends read at the last log row of the display
     frame they lie in (round 12: a frame that runs more steps after the step's own row no longer reads as lag); it
     bounds neither the ms dropped nor the times. With a delay of 300 ms injected at each ready prompt (`U6_DELAY_TAKE_MS`, test-only) both fail.
     What was seen: no slowdown that comes with motion on, and no delay of a held press at a ready prompt; the time
     between two steps is the room's world turn (3.7–7.3 s on the real clock in these runs, the virtual display's
     two paced steps 3.4 s; the runs walked different steps, and a turn's length varies with what the room's actors
     do). That pace is the turn's (spec 19 O2), and W3's for the held walk's continuity.
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
     actually sweeps) the top tiles, multi-tile parts and light overlays.
   - **Over the map:** the border and the rest of the UI come from the engine frame unchanged.
   - **New publications during a slide:**
     - a foreign one ends every slide;
     - a scene with the same positions lets slides run on;
     - a new one-cell move during a slide restarts from the drawn position, even past 24px; only a logical teleport snaps;
     - an early picture stays held after the slide reaches rest until the turn's first publication
       (a scene or a foreign picture) or the command prompt, whichever comes first. There is no
       timer fallback: `PREVIEW_MS` is removed. The hold does not append scenes. W3 retains only source scenes that supply cells
       in the current camera sweep; ordinary one-cell slides use at most three, while unusual
       accumulated lag can require another departing strip until the next publication. `Motion::publish` delivers the command-prompt state to
       `Presenter::prompt`; `publish` and `foreign` end it on publications.
     - **Why an event:** the leader or a follower may carry action-point debt through repeated
       refills. No 2000 ms upper bound follows from the scheduler. Before the first pick, the step
       waits `STEP_MS + FOLLOW_MS = 65` ms (`actors/walk.rs`), each refill can add
       `PICK_MS + MINUTE_MS = 197` ms (`actors/turn.rs`), then the first pick and refresh wait.
       A swallowed pick (`turn.rs`, `swallowed` before the worktype decision) may consume that pick;
       queued combat lines also pay `PRINT_MS = 22` ms each (`combat/present.rs`, `print_paced`).
       These paths are not evidence of a small timeout. A command prompt ends every completed turn.
     - The long-first-pick receipt forces the leader to -128 action points after a step, with
       nonparty actors inactive. This is a synthetic debt on the user's runtime data: sampled
       vessel costs needed at most one refill and did not exercise the old 2000 ms hole.
       `an_early_picture_is_held_through_a_long_first_pick_until_the_turns_picture` checks every
       frame before the scene (2280 ms in the original-build fixture), no stale scene, and the
       presenter's image throughout. A second receipt drops all publications after the preview
       and proves that the prompt itself releases it. The start-room receipt separately requires
       some turn longer than its longest first-scene wait and counts old-cell foreign frames.
     - Foreign pictures still end a hold. To prevent preview → old published cell → new cell in
       a first-pick combat effect, Design 4 suppresses the preview when the copied scheduler picks
       a fighter first. Such a move starts its slide from the authoritative turn picture.
     - an open context menu (spec 17) ends every slide at once, and none runs while it is open (round 12): a slide
       running when the menu opens, or one a publication would start under it, ends (`Presenter::snap`, from
       u6-web `Game::slide_view`, which reads `Engine::context_menu_open`: a snap, not a publication, so the
       presenter keeps the scene it shows and its generation moves). The platform then shows the screen frame, the
       menu over the map at rest: the menu is never drawn over a moving map, nor a moving map under it, and nothing
       slides on after it closes (the next publication that moves something slides from the scene kept). Receipts:
       `a_context_menu_opened_during_a_slide_ends_it_and_it_does_not_resume` (HD off and on, Original and Wide: in
       the page's start room out of its fight, an NPC still sliding at the prompt; in its start fight, a member still
       sliding at the next member's prompt, with the history scrolled three lines back: no slide at the menu's first
       frame, none through the slide's own end and 200 ms past it, open or closed, each layer where the one
       composition puts it), then a fresh tapped step must slide and finish at rest. The page
       checks the full replacement map pixels on the first menu frame and at slide teardown,
       and requires the slide still to be active on every pre-menu frame. Test-web's "a right
       press during a slide opens the menu" also taps a step after Escape. A separate Go-here
       receipt opens a menu at rest and checks the travel's first step from its first frame.
4. **The early picture (spec 19 O2, pure form).** Right after the leader's step (`actors::walk::paced_player_step`),
   the engine builds a scene on a clone of `State` (`update_view` and the draw lists on the clone; the clone is
   dropped; the clone holds no remembered sight, so the early picture reads the live state's). It hands
   that scene to the presenter at once.
   - The party's slide then starts at the key, not 540 ms later.
   - The authoritative publication at the first pick's refresh is then a scene with the same party positions, so
     their slides run on; NPCs that moved in that pick slide from there.
   - The preview never writes `Io`'s buffers or the window cache.
   - Its cost is a prediction `State` clone and, when accepted, a second clone for the view and draw lists per step (budget: under 1 ms at 8× with the held
     walk; the cost per step does not depend on the speed). Before the conservative prediction added 2026-10-29, measured 2026-10-27: natively 0.19 ms median in Original
     and 0.29 ms in Wide (the early picture alone, 200 on the state after the start fight and the town steps, gate
     profile, `bench_the_state_clone_of_the_early_picture`); in Wide's wasm (the Game driven in Node, each step's whole
     tick, the least of 3 runs over 23 steps) a step's tick takes 1.35 ms median and 1.70 ms worst with smooth motion
     on against 0.94 and 1.24 ms off, so the early picture adds about 0.4–0.5 ms there.
     With the conservative prediction (2026-10-29), an accepted early picture measured
     0.459 ms median / 1.526 ms p95 in Original and 0.473 / 1.508 ms in Wide; maxima
     6.023 / 4.608 ms. These runs overlapped other gates and are not idle-host measurements.
     The median meets the 1 ms target; these results do not establish a sub-1 ms tail bound.
     Prediction and scene clones are sequential, so only one extra State copy is live at once.
   - There is no early picture in combat mode, or when `preview_blocked` finds that a
     fighter can publish the old cell before the first refresh. The prediction starts on its
     own State clone, before the separate preview view build consumes any RNG. It runs the real
     `leader_bookkeeping`, the shared `setup_after_raise` (`formation`, `go_to_schedules` and
     leader marking), scheduler picks/refills, and `clock::tick` for each refill.
     A noncombat NPC pick runs the real `npc_turn_sync` on the clone: any queued effects,
     lines or an arrest/conversation/cast wait also withhold the preview. In particular a
     guard-post worktype can strike even though it is not classified as a combat routine.
     `predict_without_io` prevents all synchronous access to the polling engine's I/O and
     treats any attempted access, including a read, as unknown. An unknown result withholds
     the preview. A pending resurrection, changed leader/centre, swallowed pick, or exhausted
     bounded search likewise withholds it. The swallowed branch can skip the first refresh;
     it is deliberately not treated as an ordinary harmless pick. The guard restores its scope
     on unwinding and propagates a nested attempted access to the outer prediction.
     A clock minute which only queues text still requires I/O to read that text, so it too
     conservatively waits for the authoritative picture. This sacrifices some early starts,
     while keeping safe first picks early and leaving authoritative state, RNG, time and input
     unchanged. Tests omit bookkeeping, clock ticking and the swallowed guard in turn: each
     omission must fail its named regression. The real-data first-fighter receipt also checks
     both widths (Original seed 25402, Wide seed 7), with the old-cell foreign-frame counter.
     A fighter which ultimately draws nothing may lose its early picture unnecessarily.
     The rule avoids the observed slide → old-cell effect → new-cell jump: fight playback
     restores a window published before the step. A broad "any fighter nearby" rule was
     rejected because it suppresses all start-room previews even on a safe first pick.
5. **Renderers.**
   - **Original, classic:** during a slide, re-render the map rect each display frame in software (about 12 × 12 cells).
   - **HD (Original and Wide):** once per generation of the slides the engine blits the scene with a one-cell margin
     through `blit_at` (`gfx/motion.rs`: the draws of `gfx::blit::blit_back`, at any pixel; equal to the engine's
     picture in the 422 scenes the slide-end receipt draws again from scratch) into scene-sized layers: static, movers
     and occluders. The compositor composes each layer once; each display frame then draws them with the camera
     offset, clipped to the shown rect, under the frame's border and UI.
     - The layers are the static words (the newest scene's view and a cell round it, one rect whatever cells the
       scenes hold; over colour 0, every pixel drawn), the cursor and border, and per mover its own words and the words
       over it. A layer pixel that the frame's composition wrote from the same colour and source takes that
       composition's block.
     - The static layer's rectangle follows the actual camera sweep, with at least one cell
       around the view. Source scenes are retained only when they supply a needed cell that a
       newer scene lacks. Idle publications cannot evict the departing strip. The former 24px
       compile-time bound/snap is removed; synthetic repeated-shortening and idle-publication
       receipts check every shown pixel has ground across the larger sweep.
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
     - A fade step (a remembered cell a game day older, or forgotten from day 5) is an in-place change: the scene of
       the render that shows it has the same places, so the slides run on. Classic shows it at the next present (each
       frame is drawn from the newest scene); HD at the slide's end (the layers stay those of their generation, and
       the page then shows the engine's HD frame).
     - The early picture's grey cells are read from the live remembered sight, unchanged, round the copy's view and
       at its time (`SightMemory::shown_in`): the copy's cache holds none, and the cells in sight are drawn live,
       so what that render would remember changes nothing it shows. One case differs: where the store is at its
       cap, the turn's render may forget the oldest remembered cells, which the early picture, reading the store as
       it is, still shows in grey; the turn's picture then shows them black.
     - No slide crosses a load (load state 0x105350) or a new process taking over: the remembered sight's copies
       are forgotten there, which the scenes kept before name, and the game shown is another. The engine drops its
       kept scenes, and until it queues a publication with a scene every scene it keeps snaps, the early picture's
       included, and an unchanged picture is published again (`Io::break_motion`). An early picture is such a
       publication: the presenter starts again from one that snaps whether or not it shows slides
       (`Presenter::preview`). The page makes the next layers whole after a scene that snaps.
   - **Page:** `draw()` also runs while a slide is active. The wasm exports the presentation clock.
6. **Switch.** Smooth motion on by default in the page, off in the engine and the web harness by default (as spec 18's
   switch). Off is today's picture exactly.
7. **Bounds.** What motion keeps and what bounds it. Spec 18's memory bounds count the remembered sight only, not
   these. Sizes are this build's layouts on the native build, from capacities, as `SightMemory::heap_bytes` counts.
   - The engine keeps at most `SHOTS` = 64 framed renders until a publication shows one (`engine::SHOTS`), with the
     one kept last and the early picture made in the poll. It queues at most `PUBLISHED` = 256 publications for the
     platform (`engine::PUBLISHED`). Past it the whole queue gives way to one foreign publication at that time, and
     the game shown breaks as at a load (`Engine::queue_motion`): the platform sees none of what was queued, its
     presenter ends every slide at the foreign one, and the scenes kept after it snap until one is queued
     (`a_full_queue_gives_way_to_a_foreign_publication_and_breaks_the_game_shown`,
     `a_presenter_fed_past_a_full_queue_never_slides_from_the_scene_before_it`).
   - The page never meets that cap, so a snapping publication that ends a load's break is never lost before the page
     sees it. A tick queues at most two: its early picture (in `Engine::tick`, after the poll) and its own
     (`Engine::match_motion`). The page runs at most `MAX_STEPS` = 64 ticks a display frame (`Pacer::due`, and the
     budget loop in site/app.js `frame`) and presents after any frame that ran one (`frame`'s `game.present()`).
     Each present takes the queue whole (`Game::present` → `Game::slide_view` → `Motion::take` →
     `Engine::take_motion`). 2 × 64 ≤ 256 is asserted at compile time beside `PUBLISHED`. A host that ticks longer
     without taking the queue meets the cap, and the break above.
   - W3 keeps only scenes supplying cells in the actual camera sweep, normally 2–3. It retains
     at most `MAX_HISTORY = 16` snapshots after a publication (17 transiently while examining
     the incoming one). Sixteen gives over three times the five-scene shortening stress
     fixture's measured need; the normal paired receipts retain at most three.
     Exceeding it resets presentation to the exact newest scene and increments `capacity_resets`;
     it changes no game state. This explicit resource fallback is independent of distance and
     never activates in the normal cadence/O3 receipts. Foreign pictures, teleports and clear
     purge old sources; completed tracks (including release settle) retain only the newest.
   - The static raster area budget is `min(1,048,576, (cols+8)*(rows+8)*256)` source pixels:
     four cells round each view edge as area headroom (`SWEEP_MARGIN_CELLS=4`), compared with
     the ordinary one-cell margin `(cols+2)*(rows+2)`. This is an area budget, not a 24px
     displacement threshold; a long narrow strip may fit where a square sweep would not.
     `MAX_STATIC_PIXELS = 1,048,576` is an additional absolute ceiling for malformed scenes.
     Coverage dimensions use
     checked arithmetic, checked before allocating the coverage cell set or raster. A larger
     sweep uses the same resource reset. The cell set is therefore at most 4096 entries, and
     each mover's occlusion sweep is intersected with it before enumeration. The raster holds
     `N * (sizeof(u8) + sizeof(Src) + sizeof(bool))` payload bytes; its HD RGBA canvas holds
     `4 * scale² * N` bytes. For an Original 11×11 view the budget is 92,416 source pixels
     (HD4 canvas 5.64 MiB); for a Wide 37×21 view it is 334,080 (20.39 MiB). At HD4, even
     three simultaneous full-size RGBA copies of the Wide static layer total 61.17 MiB,
     plus its source/index/drawn arrays and retained predecessor. Mover/occluder layers,
     whole-screen canvases and compositor caches are additional and measured separately;
     these figures are not a universal total-process RSS bound. The 1M absolute ceiling
     alone would allow 64 MiB per canvas, so the per-view budget is the operative limit.
   - Snapshot payloads vary. Retained history is bounded by `16 * B`, where B is the largest
     supplied snapshot heap footprint, not a universal constant inferred from a benchmark.
   - The most a shot held in the receipt's runs is 12 264 bytes in Original and 69 864 in Wide (`Shot::heap_bytes`,
     its accounting checked on a shot built by hand: `a_shots_heap_bytes_are_its_capacities_worked_by_hand`; the runs
     are the start fight, the town's walk, and the walk with the cells round the party remembered:
     `the_heap_bytes_of_a_shot`, 2026-10-28). These are measured figures, not a bound: a scene with more nodes or more
     remembered layers holds more. With W3's retained cap the snapshot count is at most
     66 + 256 + 16 = 338 (339 transiently while admitting one); with the page's at-most-128 queue,
     66 + 128 + 16 = 210 (211 transiently). Multiply by an observed B only as an estimate;
     the former 325/197 counts assumed W2's fixed three-scene history and are superseded.
   - A run with motion on (`the_shots_motion_holds_at_its_peak`, 2026-10-29: the start fight, and the town's walk
     with the cells round the party remembered, a presenter fed as a page at 8× feeds it, every 8 ticks; sampled
     after each tick and, on a tick that feeds it, before the feed too): the shots the engine and the presenter hold,
     each once, take at most 85 848 bytes in 7 shots in Original (the start fight, before a feed: the presenter's
     scenes beside the queued ones it has not taken) and 468 440 bytes in 7 shots in Wide. At Max a page presents
     after up to 64 ticks, so the old seven-shot observation is not a universal bound; use the W3 counts above.
   - The slide's layers at the page's largest scale, HD 4×, over four taps east, the most over the frames from the
     first tap on that drew a move's slide while it ran, every move having some (`the_slide_layer_buffers_at_hd_4x`,
     2026-10-28; measured again with this window 2026-10-29, the same figures). In Original: 1.05 MB of layers
     (colour indices, sources and drawn marks, in the wasm), 4.77 MB of composed words and changes (in the wasm),
     and 4.78 MB of canvases in the test's stand-in for the page (`Page`: the layers' canvases only; the page's
     screen canvas and Wide's compositing canvas are not counted). The sum of these peaks is 10.6 MB, and the most
     on one frame 10.58 MB. In Wide: 6.00, 27.4 and 27.4 MB; 60.9 MB both summed and on one frame. The words and
     canvases take 4 · s² bytes a layer pixel at scale s, so at 2× a quarter of these.

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
| The message history scrolled back (spec 23) | A slide keeps the history over the message window. Classic draws the slide into a copy of the engine's shown frame (the published frame, the history over it, the pointer sprite), HD over the HD frame of the screen frame (`Engine::screen_frame`: the published frame, the history, the menu); the slide's layers are clipped to the shown map (`a_slide_keeps_the_message_history_the_player_scrolled_back_to`: against a frame made in the test from the published frame and the history's view alone, the frames off rest, those holding the early picture at rest and the first after the slide; in HD the three share one frame off the map; in the page, test-web's "while the history is scrolled back"). With a context menu open over it no slide runs (Design 3). |
| A right press during a slide (spec 17's menu) | The slide ends at once and the menu is drawn over the map at rest; nothing slides on after it closes (Design 3). |
| Key type-ahead | Fixed by spec 19 O1 (unit K1), which lands before this. Otherwise one hold queues up to 49 slides. |


## W3 — held cadence (2026-10-29)

Research: Exult's [actor path action](https://raw.githubusercontent.com/exult/exult/master/actions.cc)
uses the actor's frame time for the next walking delay, while
[Nuvie's player](https://raw.githubusercontent.com/scummvm/scummvm/master/engines/ultima/nuvie/core/player.cpp)
keeps a walking delay separate from the display. The applicable principle is independent movement
progress, not a copy of either engine's numeric cadence. This port preserves its original turn rules.

1. **Authoritative time and publication time are different.** The web and native window hosts observe each party
   slot's cell and the camera centre after every engine poll, read-only. A W2-refused preview can
   arrive late as the authoritative picture, then another step's allowed preview only 40ms later.
   The latter is not a 40ms walking interval. Each observed target is paired with its own logical
   step interval; a bounded four-target history handles multiple publications in one display frame.
2. **Tap and held walk.** The first step and a physical input direction change use `360/k` engine ms (spec 19 O3).
   While an unmodified direction or an accepted pointer walk remains held, or an automatic route is active, each subsequent one-cell track uses that mover's
   own maximum of the **last two raw logical intervals** as the duration of the whole remaining
   journey. The first measured interval uses itself. A buffered maximum is never fed back as a
   raw sample: a long interval expires after one subsequent short leg. This handles the ordinary
   550/750ms date-update alternation without a stop every second cell; a steady cadence stays
   unchanged. Other
   movers' publications do not reset it. A follower's diagonal/straight formation adjustment keeps
   its own cadence while the same key remains held. Sparse NPCs use the base duration and retain independent
   start/progress; their idle time is not learned as a walking interval.
3. **Retarget and release.** A new track starts at the current drawn world coordinate. A shorter
   actual interval can require temporary catch-up speed: velocity is actual start-distance / D,
   not always 16 / 360. After a long→short transition there is one buffered leg, then the next
   uses the short interval and clears the lag by its endpoint; it never teleports at
   24px. Only the separate snapshot or per-view pixel budget in Bounds may reset presentation,
   explicitly counted and tested; ordinary runs must have zero resets. Releasing settles the
   camera and the previous held party's remaining distance at no slower than the base per-cell
   speed. Unrelated NPC tracks keep exactly their own start, duration and progress.
   Manual direction changes explicitly cancel outstanding targets that lie on the abandoned direction.
   An automatic route has a separate session identity: corners and replanning keep cadence,
   whereas a replacement route or manual takeover resets it. Route tracks keep at most 16
   pending waypoints in fixed arrays and pass them in order, including several in one display
   frame. Their distance/D bound uses the whole remaining polyline. Overflow restarts at the
   newest scene and increments the existing capacity-reset receipt; ordinary routes must not
   hit it. A route ending without replacement settles its remaining corners rather than cutting
   across them.
4. **O3 and frame budget.** Observed engine intervals already contain O3's shortened waits and
   the held input's wall-time minimum, so they are not divided by k again. A track shorter than
   two display frames snaps, but its observation remains: at high O3 a short first tap does not
   prevent a later 150-wall-ms held interval from animating. Instant and Max remain snaps, including a track already running when the setting changes
   without a new publication. A finite short-tap policy may suppress a new track but never
   discards an already active release tail. Display time is monotonic when a speed change
   clears the pacer carry; actual cell observations retain raw engine time, and per-tick
   observations never add carry that the pacer has not yet debited.
5. **Receipts.** A retarget is not reported as a finished step. Tests record actual target
   passage/cancellation, continuous world position, exact per-track distance/D jump bounds,
   actual changed pixel frames and longest unchanged-pixel run, and final release rest. At low
   speed a 16px journey lasts more than 16 display frames, so unchanged integer pixels between
   successive one-pixel advances are quantization, not fabricated motion. Native paired runs
   use the same input and fixed tail tick count to compare the entire authoritative `State`,
   including position, RNG, calendar, rules and UI fields. Ordinary no-NPC EN and owned-KO
   fixtures use their original movement points and require at most three consecutive unchanged
   display frames after the initial two steps, at engine 1/4/8 and O3 original/4.
   The KO checks use `U6_LOCALE`; a missing explicitly selected file fails. With no override,
   an available local default is checked and a missing default is a visible skip, even with
   `U6_REQUIRE_ARCHIVE=1`. No locale generation or model service is needed for an English gate.
6. **Date UI preview proof.** The clone-only predictor may omit the date UI queue when the
   configured Screen is the normal 40×25 window without scrolling/paging, the final Messages
   and Status windows are outside the map, and the actual locale date/wind strings do not
   request a key wait. Screen output starts on the lower UI row and clamps at its bottom;
   real EN, owned KO, long and multiline strings preserve the map's pixels and Src plane.
   The authoritative path still produces all four Pending operations, in the same order with
   the same `PRINT_MS`. Only that proven date output is omitted on the state copy; unknown I/O,
   other print queues, hour/sky changes and dangerous clock effects still refuse previews.
7. **Limits and acceptance.** No renderer can know a future longer NPC turn. A longer interval,
   or a W2 preview withheld for a potential foreign picture, can still leave a real pause. These
   are reported separately from a stable quiet cadence. The safe-preview quiet fixture explicitly
   grants movement-point headroom; the ordinary no-NPC and NPC fixtures keep their real remaining
   waits and preview refusals. Owner pace judgement and live Original/Wide browser receipts remain
   separate acceptance; an OS-blocked browser run is not a pass.

The 750→150ms and 2400→150ms synthetic receipts sample actual world positions every 10ms:
maximum lag 39.04px / 45.06px, maximum jump 2.60px / 3.00px. All eight prior targets are
traversed, the old long sample expires after one buffered leg, subsequent 150ms legs have no
rest, release settles, and resource resets remain zero. Repeated shortening can retain five
source scenes, within the unchanged 16-scene cap and the actual static pixel budget.

The old W2 discussion above that assumed every publication closes at rest, a universal 360ms D,
`SNAP_PX`, and a fixed three-scene budget is historical. W3 supersedes those assumptions only for
presentation continuity; W2's conservative preview guard, event-ended hold, foreign-picture teardown,
menu behavior and no-old-cell requirement remain.

### Native and pointer integration re-audit (2026-10-29)

The native window now invokes the common Presenter/painter on every display, including
zero-tick displays. It observes each engine step, restores the authoritative frame after
settling, keys its conversion cache by scene identity, and leaves menu/ending frames whole.
Go Here uses the same 150 ms wall-clock acceptance gate as held movement; this gate never
blocks a stop input. Tests of the actual window helpers use an injected clock and are not
physical-window evidence. The temporary app used for the first manual checks is an older
snapshot; final binary and final integration checks are recorded separately.

### Owner update: walking poses and vertical footstep motion

The owner's 2026-10-29 update asks for the existing humanoid walk frames only while moving,
with a small synchronized upward offset. The later instruction permits applying a reversible
best choice while feedback is pending. Three private comparisons use the same original avatar,
route and pace: A (1 pixel sine), B (2 pixels, a 15% planted interval at each end), and C
(3 pixels, a 5% planted interval and parabolic hop). B is the initial default. Original art
and the private comparison artifacts are not repository assets.

The common display policy samples each actor's displayed world position, excluding camera
motion. Every four travelled pixels advances the original phase sequence 1, 0, 1, 2; an
eight-pixel footstep drives the lift curve. Rest uses middle phase 1 and zero lift. A live
track with no integer-pixel advance keeps its pose, while an actual stop resets it. Only
standing single-tile humanoid families already handled by `walk::face` are eligible; sitting,
lying and multipart shapes retain their original presentation. Off restores the former pixels.

This policy changes copied draw words and body offsets only. It does not write actor frames,
logical positions, turn timing, speed, RNG or engine-published pixels. The original idle RNG
still runs. Ground anchors, draw-list order and occlusion are preserved; lift extends the
occluder coverage northward. Foreign pictures, rejected previews, menus and Max/Instant
transitions must not resurrect an old scene. HD pose layers invalidate for new source
publications as well as changed geometry, palette, scale and poses.
The web HD/font switch and scale change present the current state before their synchronous
draw, including posed classic pixels or refreshed HD layers. The first paint must be correct
without waiting for another animation callback or taking an engine step. A storage-refusal
browser regression retains default B and checks HD off/on, both scales and reload against
the full layer composition, with clock, phase and world position unchanged.

Both web and native window presenters use this policy. The legacy exact-engine interpolation
oracles explicitly select Off; separate B receipts check actual pose pixels, stationary
environment updates, cache reuse, menu/foreign reentry and State/RNG/publication equality.
Browser raster checks and the existing Wide performance budget must run with B enabled.
Passing an Off benchmark is not evidence of the new default's cost. Final build and physical
screen acceptance remain distinct from the headless renderer receipts.

## Display-only attacks (2026-10-29)

The approved body-check/recoil request uses B's 6 px forward impulse for a physical
melee attack and a 3 px impulse away from a projectile's target. The quick impulse
occupies 30% of the envelope and returns over the remaining 70%: 180 ms melee,
120 ms ranged at Original turn speed. These values live independently of gait in
`u6-render::motion::attacks`; O3 scales the presentation duration and Instant/Max
ends it. No extra game wait is inserted. The initial 440 ms comparison preview
was shortened to fit the attack's existing presentation waits.

The actual `hit` path checks range, ammunition and interference before recording
a cue; spellbook/item-spell casts do not use the physical envelope. Projectile
classification reuses `flies`, including its melee exceptions. The cue is attached
to the first existing combat effect and starts immediately before its original
wait; a physical attack with no effect emits its cue directly. This adds no effect,
engine publication, input, turn, RNG call or damage change. The transient cue is
not saved and both motion-on and motion-off States remain identical.

Classic, HD and native consume the same copied pose offsets. All parts of the
attacker move together; ground cells and occlusion order stay fixed. The swept
area includes the 6 px strike plus a possible 3 px gait lift. Foreign missile/flash
pictures retain priority. Context menus, focus/Settings release, new games,
scene discontinuities and Instant/Max discard active offsets. Web requests a
presentation even on a display frame with no logical step, including the last
frame that clears the offset.

Receipts exercise real Attack + target + Enter with melee and an ammunition-equipped
bow in Original and Wide builds, compare HD layer composition to the complete
classic raster, and compare the native window buffer and complete State/RNG/engine
publications every frame with motion off. Synthetic receipts cover refused range/
ammunition, all impulse directions, multipart occlusion, rest, speed, cancellation,
foreign-to-world return and implicit scene restarts. The preview remains local/private;
no original sprite or generated pack belongs in the public repository.
