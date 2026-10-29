---
title: ultima6 — 21 travel to a pressed cell
type: spec
tags: [ultima6, input, pathfinding, ui]
---

# 21 — Travel ("여기로 이동")

Status: shipped (2026-10-28, deploy 35, main 17c09b6): the search, the walk, the stops and the engine API below
are the implemented contract, not a future target (unit M4, `feat/travel`). The context-menu entry that calls it is wired
in unit M3's merge round (spec 17, 2026-10-29: "여기로 이동" calls `Engine::travel_to`, with S3's pace). The owner,
2026-10-28, verbatim: "그리고 우클릭시 여기로 이동도 하나 추가해줘 해당 위치까지 a*로 검색해서 이동하는 명령". Spec 17's context menu (unit M3, `feat/ingame-menu`) draws the entry; this spec owns the search, the
walk and the API the entry calls. Related: spec 19 (the turn model, O1c held keys = unit K2), spec 20 (smooth motion).

Evidence labels: **read** = `file:line` or an original address; **measured** = printed by a test named here (counts
only); **cited** = an outside source; `[추정]` = inference.

## Research

### Ultima VII: click-to-walk and how it stops (cited)

- In the original game a double right-click on a spot walks the Avatar there (the Black Gate player's guide, ["move the
  cursor … and click the right mouse button twice … You walk to that location"](http://bootstrike.com/Ultima7bg/Online/umanual.php));
  a right-click walks toward the pointer, a double right-click moves the Avatar to it "if possible, using a
  pathfinding algorithm" ([a summary of the controls](https://cyberartsweb.org/cpace/infotech/digerate/ULTIMA7.HTML)).
- Exult, the open reimplementation, documents the same right-click pathfinding (double click by default, single or off
  by `allow_right_pathfind`, [Exult docs §3.9, §6.1, §11.1](https://exult.sourceforge.io/docs.html)). Its search is
  an A* over the eight neighbours (`Neighbor_iterator::coords`, a step cost of −1 = blocked) that gives up past a cost
  bound, not a node count (`pathfinder/path.cc` `Find_path`, `max_cost`); on no path it prints that it found none and
  does not move (`gamewin.cc` `start_actor_along_path`).
- How Exult's walk stops (`actions.cc` `Path_walking_actor_action::handle_event`): at the path's end; when the next step
  is blocked it waits 0.1–0.6 s and tries the same step again up to `max_blocked` times, then, if an NPC blocks,
  plans once more from where it stands (`walk_to_tile`), and gives up when that fails or its persistence runs out.
  Exult is a reimplementation, so this is its design, not a measurement of the original's.
- What this port takes from it: the eight-neighbour search with a bound, the empty result when no path exists, the
  stop at the end, one fresh plan when the next step is blocked and then stop. It does not take Exult's waits: a turn
  in this port is a command, so waiting would be a Pass the player did not ask for.

### Ultima VI: move costs and diagonals (read)

- **The step rule is the player's step 0x11d4ee** (`actors/party.rs:284` `player_step`; lines on this branch): the
  leader may enter (x, y) when 0x11b1c0 allows it (`actors/walk.rs:326` `can_move_to`: the map tile, then each object
  there; party members swap); a diagonal also needs one of its two orthogonal neighbours enterable (`party.rs:399`
  `step_test`, which `player_step` runs at `:324` with the damaging-tile switch clear; before this unit it was inline
  there). While a pointer button walks (`work.mouse_walk`, set by a press only, `ui/target.rs:261-262`, cleared once
  the buttons are up, `:255-258`) a blocked diagonal slides along an axis (`party.rs:417` `slide`); a key clears the
  clicked target `work.mouse_target` (`ui/target.rs:313-315`).
- **The cost** (`party.rs:472` `pay_step`): on foot the step pays the move cost of the tile it leaves, 0x11ef84
  (`walk.rs:217` `spend_tile_cost`: 5, plus the high nibble of flags1 of the map tile and of every non-impassable
  object tile there), and 5 more on a diagonal. 0x11c650 (`walk.rs:179` `spend_mp`) then halves it for type 0x1af or
  the party haste, doubles it under the slow timer, and floors it at 1, per call: per-actor factors, the same for
  every step of a walk; a diagonal pays in two calls (`party.rs:487-488`). A turn ends when the scheduler picks the
  leader again (spec 19), so the points are the walk's game time.
- **The original's own pathfinder** is the NPCs': a bucket-queue Dijkstra over the cost map 0x1206e2 builds, four
  directions only, inside the window grid, 40 × 40 in Original (`actors/path.rs:1-8`, `:94`, `:18-21` `DX4`).
- **Keys and directions.** The command loop takes eight direction codes as the walk (0x106c6c, `ui/target.rs:62-71`
  `DIRECTION_KEYS`, `:322-324`): 0x148, 0x149, 0x14d, 0x151, 0x150, 0x14f, 0x14b, 0x147 for north clockwise, the PC
  keypad's codes the game was written for (`ui/input.rs:10-12`). Measured on the user's `game` (a probe over
  `UiState::load`'s keymap, 2026-10-28, not kept): the Amiga raw-key table 0x151d00 gives the four arrows' codes and
  gives none of the four diagonal codes in its plain, shift or control column; two of them stand in its Alt column,
  which builds the Alt + keypad number instead (`ui/input.rs:94-106`). So on the Amiga a diagonal walk comes only from
  the mouse (0x105ebe, `ui/click.rs:223`).

## Tensions

- **"The same input a direction key gives" vs "a diagonal rule matching the original".** The Amiga keyboard gives no
  diagonal (above). Resolved (reversible): the engine's `Key` gets four port-only diagonal keys, `UpRight`,
  `DownRight`, `DownLeft`, `UpLeft`, whose codes are the PC keypad's 0x149, 0x151, 0x14f, 0x147, given past the raw
  table (`ui/input.rs` `key_code`); from there on the press is an arrow's in every step of the game's code (the poll
  and its drain, 0x106c6c's direction table, the walk command, the step without the mouse's slide). Only travel
  presses them; no platform maps a key to them. The other reading, four-way travel on the arrows alone, would need no
  input change but takes two turns for each diagonal cell (up to twice the wall time on such routes); it stays one
  line away (drop the diagonals from the search's neighbours).
- **The search's cost vs the original's NPC cost map.** The NPC map (0x1206e2) charges nibble + 1 per cell, lets a
  door through below frame 8 (an NPC opens a closed door on its way) and blocks the cell before a door
  (`actors/path.rs:91-92`). The travel search charges what the player's step pays (above) and lets a door through
  only where the step rule does: open (frames 0..3; 4..7 closed, 8.. locked, `walk.rs:514` `door_at_actor` writes
  4 × closed + side). It plans the player's steps, and the player opens a closed door with Use first. The two differ
  on purpose.

## Contract

**Where.** `crates/u6-core/src/actors/travel.rs` (the search, the per-prompt step, the stops); the engine hook in
`Engine::tick` (`engine.rs`); the shared step test and costs in `actors/party.rs` and `actors/walk.rs`; the diagonal
keys in `engine.rs` `Key` and `ui/input.rs` `key_code`; the exports in `crates/u6-web/src/lib.rs`. Other modules named
travel are unrelated: `map::travel` (level moves), `usecmd::travel` (vessels), `magic::travel` (gates).

**The search.** A* from the leader's cell to the target cell on the party's level.
- A step from cell c in direction d (eight, 0 = north clockwise, `walk.rs:20-21`) is valid exactly when the player's
  step would take it: `party::step_test`, the test `player_step` itself runs, fed `can_move_to` for the leader. The
  search runs it on one copy of the state with the hiccup flag (A4-0x6a9c `work.hic`) clear, so no line is queued,
  and keeps each cell's answer for the search.
- Its cost is `walk::tile_cost` of c, plus the leader's own object tile where the leader does not stand now (0x11ef84
  counts every object tile on the cell it leaves, the leader's included: `walk::object_cost`), plus
  `party::DIAGONAL_COST` on a diagonal: what `pay_step` charges on foot before 0x11c650's factors. The factors are left
  out: under haste or slow each call rounds on its own, so a step pays within a point of the factor times the search's
  points, and the path is a cheapest one in unscaled points. Ties go to fewer steps. The estimate is 5 × (|dx| + |dy|)
  points and the Chebyshev distance in steps: neither overestimates, so the path found is a cheapest one.
- Harm first avoided: the search runs once with the rule's own damaging-tile switch set (A4-0x6a8a `avoid_damage`,
  the switch NPC steps use, `walk.rs:548` `step`, set at `:562`) on the cell a step enters, which refuses a tile or
  object with flags1 0x08; a diagonal's two side cells are tested without it, as the key's step tests them. Only if
  that finds no path it runs again without the switch, as a key would walk.
- Bound: the window, `geom::WINDOW` cells a side round its origin (40 × 40 in Original, 60 × 60 in Wide), the tiles
  and objects the step rule reads there being loaded; each cell is expanded at most once a pass, so at most `WINDOW²`
  expansions a pass and twice that a plan. A target outside the window is `Far`; no path inside it is `Unreachable`.
- Reads the state only, as observed: `reachable` leaves the caller's `State` unchanged (compared `==`, its RNG
  included), and `plan_on`, which `plan` and `reachable` run on a clone, draws no RNG and queues no line (its `rng`,
  `pending_print` and A4-0x7afd compared, with a barrier 0x1a0, whose refusal prints while the hiccup flag is set, on
  the search's way); the clone inside `reachable` is not observed itself. Nothing is saved. Each plan and each
  prompt's step check clone the state once.

**Refusals** (`travel::reachable(&State, (x, y)) -> Option<Reason>`, also what `travel_to` returns):
`Here` (the leader stands there), `Aboard` (a vessel carries the party: its moves follow the wind and current, not
the key), `Combat` (combat mode), `Foe` (a hostile in view, below), `Far`, `Unreachable`, `NoLeader` (no leader on
the map), `NoGame` (the command loop's program is not running: the opening, the main menu).

**The walk.** One step per command prompt, entered as a direction key.
- The command prompt = the command loop reads a command key with keys going through command processing: A4-0x7a2c
  set (`main.reading == 1`, `main_loop/commands.rs:163-172`), A4-0x7a42 set (`ui.input.accept`, which the more
  prompt clears, `ui/print.rs:65-72`), the line editor off, and no cursor up (`ui.input.targeting == 0`: with Tab's
  cross-hair, 1, or the panel cursor, 2, a direction key moves the cursor, `ui/target.rs:497`, `:549`).
- At each tick, before the platform's input is queued, `Engine::tick` asks the travel for a press: when the command
  prompt is up and no key waits (none in this tick's input; in the queue at most the travel's own last press, until the
  game reads it), it returns the next step's key
  (`Key::Up`..`Key::UpLeft`), which the tick appends to the input. The same tick's poll takes it: the poll, the drain
  (A4-0x7a46), 0x151d00 (arrows) or the keypad code (diagonals), 0x106c6c's `DIRECTION_KEYS`, the walk command
  (`commands.rs:379-382`), `paced_player_step`, the followers' pass, the level test, the world turn and its NPC
  turns, the refresh and the slide of spec 20: everything a key press runs. No state is written by the travel.
- Before it presses, the travel checks where the leader stands against the last press (before the first press,
  against the plan's start: the leader may have moved between the start and the first prompt) and the step on a copy
  (the same test as the search): a step refused now is a blocked step (below), not a press. Distances are Chebyshev,
  each axis taken round the level's mask (`map.world_mask`): the cells either side of the seam are one apart.

**Stops** (`travel::End`; the walk ends, nothing is pressed after it):
1. `Arrived`: the leader stands on the target at a prompt.
2. `Blocked`: the next step is refused, or the leader did not reach the cell the last step aimed at (refused after
   paying, no wind, a drunk stagger, a swap that went elsewhere): one new plan from where the leader stands; if it
   finds none, or its first step is refused too, stop. The budget is one plan per blockage: a step that lands where
   it aimed restores it. At the first press a leader one cell off the plan's start is a blockage.
3. `Combat`: combat mode (A4-0x7b84 + 0x78, `work.rs:241`) at a prompt.
4. `Foe`: a hostile in view at a prompt. Hostile = an actor slot not in the party (status 0x80 clear), not dead
   (status 0x10 clear), aligned evil (status & 0x60 = 0x20) or chaotic (0x60): the alignments 0x12500a lets take a
   good (0x40) actor as a foe (`combat/npc.rs:136-187`). In view = on a view cell the leader sees (`map/view.rs:38`
   `is_visible`, flags bit 0x80) and seen by 0x1224ea (`actors/leaves.rs:118` `can_see`: not invisible, same level).
   Narrower than the game's own enmity: neutrals that attack (worktype 0x9b), fleeing or guarding actors, an attacker
   out of sight and lost hit points are not read; those reach the travel as combat (3), a prompt (6) or not at all.
5. `Input`: any key, repeat or text in a tick's input (K2's held repeat included, which the page queues into the
   tick's input), a pointer button pressed (a bit set that the handler's buttons before the tick did not have), or a
   key in the queue the travel did not press (given before it started, or pressed by a travel it replaced and not
   read yet: `travel_to` clears the travel's own key). A release, a pointer move, or a button held down since before
   (1 then 1) is not a stop. Chosen: a direction key held when a travel starts stops it at its first tick, by the
   repeats it queues (those K2 would take back included: `travel_press` counts them before the take-back, below);
   the player walking by hand keeps the walk. The match on the event's kind has no wildcard arm
   (`travel.rs:489-497`), so a kind added later is decided there.
6. `Prompt`: a key wait other than the command prompt: the event wait 0x106bbc (`ui.input.in_wait`) or the line
   editor (`ui.input.line_input`) while the command prompt is not up (a more prompt, a question, a conversation, a
   cursor up).
7. `Party`: a member the followers' pass moves (worktype 1, on the party's level, free to step: `party::following`)
   stands more than 8 cells (Chebyshev, round the mask) from the leader at a prompt; 8 is how far 0x11f0a0 lets a
   party member end from the leader in combat mode (`walk.rs:555-560`), the original's one leash.
8. `Displaced`: at a prompt the leader is more than one cell from where it stood when the last step was pressed (from
   the plan's start before the first), or on another level, or another actor leads, or a Restore ran since the travel
   started (`SaveLoad::restores`, counted at the end of `saveload::restore`, `saveload/restore.rs`; port-only, never
   saved), wherever it left the leader: not a step's result (a level change on the way, a load, a teleport). A ladder,
   stairs or a gate as the target ends here, not as `Arrived`: the step onto it changes the level.
9. `Gone`: no leader, or the game left the command loop's program.

**Determinism and saves.** The travel is engine state (`Engine.travel`), not `State`: never saved, never cloned into a
save, and a new engine (a load from the page) has none. The game's own Restore takes keys (Ctrl-R and its question),
which stop it (5, 6); one run at a turn's end without a key (save mode 0x52, 0x104d98, `commands.rs:667-673`) ends it
by stop 8. The search and the checks draw no RNG.

**API** (for M3's entry; signatures in the report):
- `u6_core::actors::travel::reachable(s: &State, target: (u16, u16)) -> Option<Reason>`;
- `Engine::travel_to(&mut self, target: (u16, u16)) -> Result<(), Reason>`, `Engine::travel_active(&self) -> bool`,
  `Engine::travel_end(&self) -> Option<End>`, `Engine::travel_progress(&self) -> Option<&Travel>` (its target and
  receipts);
- u6-web `Game::travel_to(x, y) -> bool` (map cell, the party's level), `Game::travel_active() -> bool`.
- `Reason::code()` and `End::code()` give stable ASCII codes for the page's words.

**Merge contract with unit M3** (the menu; wired in M3's merge round, 2026-10-29, as below). M3's filter swallows three
things the raw input still holds (as the review read M3, `popup.rs:992-1007`, `:1051-1058`, `:1080-1093`,
`:1160-1169`; not read here): the choosing press's held moves (after the choice it queues a move with no button, so
the handler's buttons read 0 and a held move reads as a new press), a second click on the entry within 500 ms, and a
held Enter's or Space's repeats. Fed the raw input, a travel the menu just started would end `Input` on each. So:
- `popup.input` runs first. It returns `through`, the events it passes on, and a stop flag: it took an event that is
  not the tail of its last choice's gesture. Tails: the choosing press's held moves and release, the double click's
  press and release, the choosing key's repeats and release. Not tails: the press that opens the menu, a key or a
  press inside it.
- `travel_press` runs on `through`, the flag a stop when set (a travel under way ends when the menu opens).
- A chosen "여기로 이동" (`go_here`) calls `Engine::travel_to` after that tick's `travel_press`; the new travel's
  first press is the next tick's, and the stops after the choice in the same input end it at once (below).
- The travel's step is appended to `through` after the filter and never fed to `popup.input`: an open menu takes Up
  and Down, and M3 counts the game's keys there (M3 `engine.rs:821-822`, per the review).
- The wiring round's tests, each a travel the menu just started that keeps walking until a later press ends it: a
  swallowed choosing press with its held moves and its release; a double click on the entry; Enter held past its
  repeat delay. `travel_tests.rs:739-765` models a press the game saw, not one the menu swallowed. Done:
  `a_travel_the_menu_started_walks_through_the_choosing_presss_held_moves_and_release`, `…_a_double_click_on_the_entry`,
  `…_enter_held_past_its_repeat_delay` (`travel_tests.rs`, the stand-in with the menu on; RED: `travel_press` handed the
  raw input, each ends `Input` in its tail); the stop flag: `popup_real`'s
  `a_right_press_after_a_choice_or_a_repeat_taken_back_opens_the_menu` (a right press opening a menu in the tick after
  the choice ends that travel before its first step; RED: the flag ignored); the real walk:
  `a_right_press_on_bare_ground_offers_go_here_and_it_calls_travel_to` (the party arrives on the pressed cell).
  Round 5: each of the three tails against a control of as many ticks with no input (gpt c SHOULD 2: the same cells
  walked and keys pressed at the same ticks, Go here chosen once, no press or key of the tail the game's); the stop
  flag without the zip, `a_key_the_menu_takes_ends_a_travel_under_way_by_its_flag` (a menu shown over a travel under
  way, a key it takes: `Input` before the first step; RED: `travel_press` handed `false`). The stop-flag test feeds
  the choosing press's release and the right press in one tick, so alone it would pass a build that stopped on the
  release (gpt d SHOULD 2); the siblings exclude that: `the_tail_of_a_choice_is_not_new_input` (the release is no new
  input), `…_a_double_click_on_the_entry` (its tail starts with that release alone in its tick, and the travel walks
  on) and `a_release_or_a_move_does_not_stop_it` (a button-less move is no stop).
- A press off the frame (the page's `dismiss_menu`, a press that reaches no frame pixel) is never new input but when
  it closes an open menu (spec 17, Layer flow, Close): a travel under way walks on through one with no menu open.
  Harmless as long as an open menu means no travel runs: a menu opens only by new input (a right press at the command
  prompt), which ends a travel under way, and "여기로 이동" starts one only as its menu closes.
- The stops after the choice (round 6, opus MUST A and B): the outcome must not hang on how the page groups events
  into ticks. One page frame can hold the choice and more after it: a key, a left press on the map, a right press
  opening a new menu (and Escape closing it again). `travel_press` reads the whole input before the choice starts its
  travel, so these events would reach no travel, though one tick later they end it. So `Popup::take_go_here` gives
  each choice of the input (`GoHere`, every one, in order) with its place in `through` (`at`, the index after the move
  the choice gives the game) and whether the menu took new input after it (`later`, apart from the choosing press's
  or key's own: the menu notes each event's new input apart and gives it to the choices made before that event).
  `Engine::start_go_here` records the cell (`go_here_asked`), starts the travel (`travel_to`), then reads
  `Polled::stops` over `through[at..]` with the game's pointer buttons (`handler_pointer`), `menu` = `later`,
  `queued` and `foreign` false: on a stop no travel walks and `travel_end` is `End::Input`, as the next tick's press
  gives; the cell stays recorded. A second choice later in the input (after a menu opened again) starts its own
  travel the same way, the first having ended on that menu. The choosing gesture's own tail is no stop by
  construction: what of it the menu passes on (the press's release, a button-less move) lies after `at` but stops
  nothing, and the rest (held moves, a double click, the choosing key's repeats and release) the menu keeps. The
  round-5 guard (no `travel_to` while a menu is open after the input) is gone: it read whether a menu is open now,
  not whether new input came after the choice, so a menu opened and closed again (B), a key or a left press after the
  choice (A) left the travel walking. Tests: `travel_tests.rs`'s
  `a_stop_after_the_choice_in_its_tick_ends_the_travel_as_in_the_next` (the stand-in, the menu shown by the test:
  Enter then a direction key, a press on Go here with its release then a left press on the map, each in one tick and
  split at the choice into two, give the same walking, end, keys and their ticks, leader cell, game presses and keys,
  ending `Input` before the first step; controls the choice alone, with its release and Enter's repeat, which walk
  to arrival; RED on 6b09164: the two stops walked to arrival in one tick); `popup_real`'s
  `a_menu_opened_and_closed_in_the_input_that_chose_go_here_ends_its_travel` (B in one tick: the press on Go here,
  its release, a right press, its release, Escape: no menu, no travel, `End::Input`, the party standing over quiet
  ticks; RED on 6b09164: the travel walking), `a_menu_opened_in_the_input_that_chose_go_here_starts_no_travel` (the
  menu left open: no travel, now with `End::Input`; RED on 6b09164: `None`) and
  `every_go_here_chosen_in_one_input_is_asked_and_the_last_walks` (two choices in one input, both cells asked, the
  party on the second; RED on 6b09164: one asked, the Option the menu held overwritten). The arrow after Enter in the
  same tick: it reaches the game in that tick, the prompt takes it (`keyread.rs` `take_drained`) and the leader steps
  one cell; split, the same one tick later; both end `Input` with the leader one cell over.
- Names and types: M3 has no `Engine::travel()` (it asks with `go_here_asked`); this unit's accessor is
  `Engine::travel_progress()`. M3's cells are `i16`; `travel_to` takes them `as u16` and masks them with the level's
  mask, so a cell left of x 0 wraps as the map does.

**Merge rules with unit K2** (held keys, spec 19 O1c; `Paced` and the pacing are not on this branch):
- S1: `InputEvent::Paced(_)` is a stop: it joins the key arm of `Polled::stops` (`travel.rs:489-497`; no wildcard arm,
  so the merge must decide it), and the web test gets a `Paced` row beside its `Repeat` one.
- S2: `travel_press` runs before K2's take-back, its tick-input filter and its queue retain (K2 `engine.rs:919-926`,
  per the review). Taken back first, a direction held since before the start would lose its repeats at the prompt
  and the travel would press until a `Paced` came. A tick's order: `popup.input`, `travel_press`, the choices of Go
  here (`start_go_here`), K2's take-back, the queue.
- S3: one step in 150 ms at the prompt (K2's `MIN_STEP_MS`). Due-ness comes from the platform's clock through a
  setter (`Engine::set_travel_due` into `Polled.due`, true by default, so native, headless and this unit's tests are
  unchanged; the page gives it from its clock and `last_step` at every step once it has a clock, done in M3's merge
  round; since round 5 at every speed, u6-web
  `at_every_speed_a_travel_steps_at_most_once_a_min_step_of_the_pages_clock`: at 1×, 4×, 8× and Max no step of the
  travel comes sooner than 150 ms of the page clock after the press the prompt took last, the first after entering
  clocked mode from headless stepping (the last headless take at the page clock's 0; round 6, opus NIT 4: no page
  does that, `site/app.js` gives `set_clock` before the first `step` of every frame, so in production no take is
  stamped without the page's clock; the 160 ms holds only because `Game`'s clock starts at 0.0, the test's own
  start), each after the one before, and the first after an ordinary command (a pass); least gaps, round 5: 1× 640 ms (the castle's turn is longer than the
  interval), 4×, 8× and Max 160 ms, the first step 160 ms after the headless take at every speed, and after the pass
  848, 208, 160 and 160 ms; RED without the setter, gaps 0 and 16 ms at Max (round 4), and round 5's three: the
  setter always true (every part sooner at 8× and Max: the first step 16 ms after the headless take, the least gap 80
  and 0 ms, after the pass 96 and 16 ms), the page's take time stamped only once it has a clock (the first step 16 ms
  after the headless take at 4×, 8× and Max) and only while a travel walks (that, and after the pass 96 and 16 ms at
  8× and Max); the hold and the stops while not due: `a_step_not_due_holds_it_and_the_stops_stay_at_once`, with a
  press outstanding since round 5). `next` tests it in the Hold branch after the stops (`travel.rs:624`, after
  `:612-623`), never after `pressed.take()` (`:639`): there it would take `pressed` and pop the path with no press (a
  false blockage, or `Displaced`). `travel_press` still runs every tick while a step is not due, so the stops tested
  before the step (the input, the program, a Restore, another prompt) stay immediate; those read at the step (`Gone`
  with no leader, `Displaced` by another leader, level or cell, `Arrived`, `Combat`, `Foe`, `Party`, `Blocked`) wait
  for a due step, so the travel's end is deferred until the next due poll, subject to platform
  scheduling (round 5, opus NIT). The travel's key is a prompt
  take, so it already moves K2's `last_step`: the held walk and the travel share one step time; keep it so.
- N2: K2's own command-prompt test (K2 `engine.rs:81`) lacks `!line_input` beside `travel::command_prompt`
  (`travel.rs:450`); the merge keeps one predicate for both. Done in K2 round 6 (2026-10-28): the travel reads
  `engine::command_prompt`, and the line editor's flag stays only where it decides a result, in `other_prompt`
  (the line editor up is a `Prompt` stop whatever else is set). The line editor never runs while A4-0x7a2c is set
  (it is entered only after the command read cleared the flag), so no caller of the
  one predicate reads otherwise; `the_one_command_prompt_predicate_decides_the_travel_as_before` checks the
  travel's two decisions against the old predicate over all 48 flag combinations.

## Trace (STV, before code)

Scenario: the owner right-presses an open cell beyond a door and chooses "여기로 이동".

0. **Client surface.** M3's in-game menu (spec 17) shows the entry for a map cell, enabled when
   `reachable(state, cell)` is `None`, else disabled with `Reason::code()`. Choosing it calls `Engine::travel_to(cell)`
   (or the page's `Game::travel_to(x, y)`); the menu closes. Return leg: the party walks in the game frame, turn by
   turn; `travel_active()` turns false at the end and `travel_end()` names why.
1. **Entry.** `Engine::travel_to((x, y))` → `travel::Travel::start(&State, (x, y))`.
2. **Input.** `(x, y)` world cell; masked by `map.world_mask` (a view cell left of x 0 wraps as the map does). Refused
   by `reachable`'s reasons above, nothing changed then.
3. **Layer flow.**
   - 3a `Travel::start` → `plan(&State, leader, from, target)`: `State.clone()` → `work.hic = 0` → `search(copy,
     grid, from, target)` with `avoid_damage = 1`, else again with 0 → `Vec<dir>` (next step last); `Travel { target,
     z, leader, path, start, restores, pressed: None, replanned: false, steps, presses, plans }` →
     `Engine.travel = Some(..)`, `Engine.travel_end = None`.
   - 3b each `Engine::tick(input, ms)`: `travel_press(input)` → `Travel::next(&State, Polled { input, buttons, queued,
     foreign })` → `Hold` | `Press(Key)` | `Stop(End)`; on `Press(k)`: `input ++ [InputEvent::Key(k)]` → `io.events`,
     `Engine.travel_own = Some(k)` while it waits there.
   - 3c the game future, polled in the same tick: `command_loop` → `read_key` → `wait_event` → `idle_poll` → `poll`
     → `key_code(k)` → code → `map_key` → `DIRECTION_KEYS` → `ui.input.direction = d`, code WALK → `dispatch` →
     `paced_player_step(d)` → `player_step` (`step_test` → `can_move_to` …, `pay_step`, move, `follow`,
     `level_transition`) → `end_of_turn` → `world_turn` (NPC turns, refresh) → the prompt again (`reading = 1`).
   - 3d the next ticks: `Travel::next` sees the leader on the cell the pressed step aimed at → pops the step → stops
     1, 3, 4, 7, 8 → the next press; or at a non-command wait → stop 6.
4. **Side effects.** Only the walk command's own (the game's state as a key's step leaves it). The travel writes no
   `State` field; `Engine.travel` changes.
5. **Error paths.** A refusal at the start: `Err(Reason)`, nothing changed. A stop mid-walk: `Engine.travel = None`,
   `Engine.travel_end = Some(End)`, no further press; the step already pressed completes as any key's.
6. **Output.** `travel_active()`; `travel_end()`; the frames the steps publish.
7. **Observability.** Tests count steps, prompts, clock minutes and engine ms (counts only).

## Acceptance

1. Unit (crafted `State`): the search goes round obstacles and water, through an open door and not a closed one, takes
   a diagonal only past an open side (the corner rule) and past harm beside it as the key's one step, returns
   `Unreachable` for a walled-off cell, stays within `WINDOW²` expansions a pass, and leaves the `State` and its RNG
   as they were (the plan's copy too: no RNG, no line).
2. Every planned path, walked by `player_step` on a copy step by step, ends on the target (the path = the keys).
3. Each stop (1–9) ends the walk in an engine test, and each refusal shows in `reachable`. Each guard on its own: a
   more prompt with the read up, the line editor, the cross-hair, the panel cursor; another leader, no leader; a
   Restore leaving the leader where the last press found it; a key waiting that is not the travel's (its own waiting
   key holds it); a held button (1 then 1) does not stop it, a second button does; an input stops it in its own tick
   with no press, at the prompt and mid-turn; the first press checks the plan's start; each of two blockages gets its
   own plan; distances wrap at the level's mask (both masks, both axes, both ways; a stagger across the seam plans
   again, a member across it is beside the leader); a stopped travel's unread key is not the next one's own; no
   travel starts outside the game (`NoGame`).
4. The four diagonal keys read as the walk command in their direction, past the raw keymap
   (`ui/tests/input_tests.rs`); a diagonal into a shut corner is refused by `player_step` with no points spent, no
   move and no line printed or queued. The keys' oracle is a table of its own (key → step on the map), not
   `key_of`. Real data: the raw-key table gives no diagonal code in its plain, shift or control column (counts).
5. Real data (the castle start state of spec 19's held-key tests): the party travels across a room to a closed door,
   opens it with `u` and a press on it, and travels through it to a cell beyond; each leg arrives with one key a
   planned step and one move a key, diagonal keys among them; the same inputs given at the same ticks from the same
   start show an equal frame at every tick and leave an equal `State` (no shortcut); steps, turns (prompts), clock
   minutes and engine ms printed as counts, with `real receipt: ran` or `skipped`. Also real: a cursor up (Tab once,
   twice) ends a travel as `Prompt` with no key and the cursor where it was; a Restore at the first turn's end ends
   it as `Displaced` on the start cell; the real Restore of a save made on the leader's own cell, at the prompt after
   the first step, leaves the leader put and ends the travel by the Restore counter alone.
6. The page's `Game::travel_to` starts exactly what `reachable` allows (`crates/u6-web/src/lib.rs` test
   `travel_to_starts_exactly_what_reachable_allows`, in the castle fight: refusals only); past the fight a reachable
   cell starts, a refused request leaves that travel walking to its target, which it reaches, and a repeat queued as
   the page queues it ends the next (`travel_to_starts_a_reachable_cell_and_a_refusal_leaves_it_walking`).
7. RED: each guard removed fails its test.

## Not settled

- A step whose cell an NPC just entered replans at once; Exult waits and retries first. If replans feel jumpy in towns,
  a one-turn retry (a Pass) is the alternative, at the cost of a command the player did not enter.
- No travel across a level change (ladders, stairs, gates): the walk ends `Displaced` there.
