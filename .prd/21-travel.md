---
title: ultima6 — 21 travel to a pressed cell
type: spec
tags: [ultima6, input, pathfinding, ui]
---

# 21 — Travel ("여기로 이동")

Status: planned (2026-10-28; built in round 2 as unit M4 on `feat/travel` from integ `60f92c6`, shipped when it is
merged). The owner, 2026-10-28, verbatim: "그리고 우클릭시 여기로 이동도 하나 추가해줘 해당 위치까지 a*로 검색해서
이동하는 명령". Spec 17's context menu (unit M3, `feat/ingame-menu`) draws the entry; this spec owns the search, the
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

- **The step rule is the player's step 0x11d4ee** (`actors/party.rs:284` `player_step`; lines on this branch): the leader may enter (x, y)
  when 0x11b1c0 allows it (`actors/walk.rs:322` `can_move_to`: the map tile, then each object there; party members
  swap); a diagonal also needs one of its two orthogonal neighbours enterable (`party.rs:399` `step_test`, which `player_step` runs at `:324`; before this unit it was
  inline there). With a clicked
  target (the mouse walk only, `work.mouse_walk`) a blocked diagonal slides along an axis (`party.rs:417` `slide`);
  a key never sets that flag (`ui/target.rs:313-315` clears the target on any key).
- **The cost** (`party.rs:472` `pay_step`): on foot the step pays the move cost of the tile it leaves, 0x11ef84
  (`walk.rs:217` `spend_tile_cost`: 5, plus the high nibble of flags1 of the map tile and of every non-impassable
  object tile there), and 5 more on a diagonal. 0x11c650 (`walk.rs:179` `spend_mp`) then halves it for type 0x1af or
  the party haste, doubles it under the slow timer, and floors it at 1: per-actor factors, the same for every step of
  a walk. A turn ends when the scheduler picks the leader again (spec 19), so the points are the walk's game time.
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
  only where the step rule does: open (frames 0..3; 4..7 closed, 8.. locked, `walk.rs:510` `door_at_actor` writes
  4 × closed + side). It plans the player's steps, and the player opens a closed door with Use first. The two differ
  on purpose.

## Contract

**Where.** `crates/u6-core/src/actors/travel.rs` (the search, the per-prompt step, the stops); the engine hook in
`Engine::tick` (`engine.rs`); the shared step test and costs in `actors/party.rs` and `actors/walk.rs`; the diagonal
keys in `engine.rs` `Key` and `ui/input.rs` `key_code`; the exports in `crates/u6-web/src/lib.rs`. Other modules named
travel are unrelated: `map::travel` (level moves), `usecmd::travel` (vessels), `magic::travel` (gates).

**The search.** A* from the leader's cell to the target cell on the party's level.
- A step from cell c in direction d (eight, 0 = north clockwise, `walk.rs:19-20`) is valid exactly when the player's
  step would take it: `party::step_test`, the test `player_step` itself runs, fed `can_move_to` for the leader. The
  search runs it on one copy of the state with the hiccup flag (A4-0x6a9c `work.hic`) clear, so no line is queued,
  and keeps each cell's answer for the search.
- Its cost is `walk::tile_cost` of c plus `party::DIAGONAL_COST` on a diagonal: what `pay_step` charges on foot,
  before 0x11c650's per-actor factors. Ties go to fewer steps. The estimate is 5 × (|dx| + |dy|) points and the
  Chebyshev distance in steps: neither overestimates, so the path found is a cheapest one.
- Harm first avoided: the search runs once with the rule's own damaging-tile switch set (A4-0x6a8a `avoid_damage`,
  the switch NPC steps use, `walk.rs:544` `step`, set at `:558`), which refuses a tile or object with flags1 0x08; only if that finds no path
  it runs again without it, as a key would walk.
- Bound: the window, `geom::WINDOW` cells a side round its origin (40 × 40 in Original, 60 × 60 in Wide), the tiles
  and objects the step rule reads there being loaded; each cell is expanded at most once, so at most `WINDOW²`
  expansions. A target outside the window is `Far`; no path inside it is `Unreachable`.
- Reads the state only: the real `State` is unchanged (compared `==` in a test), no RNG is drawn (the copy's `rng`
  compared too), nothing is saved.

**Refusals** (`travel::reachable(&State, (x, y)) -> Option<Reason>`, also what `travel_to` returns):
`Here` (the leader stands there), `Aboard` (a vessel carries the party: its moves follow the wind and current, not
the key), `Combat` (combat mode), `Foe` (a hostile in view, below), `Far`, `Unreachable`, `NoLeader` (no leader on
the map).

**The walk.** One step per command prompt, entered as a direction key.
- The command prompt = the command loop reads a command key with keys going through command processing: A4-0x7a2c
  set (`main.reading == 1`, `main_loop/commands.rs:163-172`) and A4-0x7a42 set (`ui.input.accept`, which the more
  prompt clears, `ui/print.rs:65-72`).
- At each tick, before the platform's input is queued, `Engine::tick` asks the travel for a press: when the command
  prompt is up and no key waits (neither queued nor in this tick's input), it returns the next step's key
  (`Key::Up`..`Key::UpLeft`), which the tick appends to the input. The same tick's poll takes it: the poll, the drain
  (A4-0x7a46), 0x151d00 (arrows) or the keypad code (diagonals), 0x106c6c's `DIRECTION_KEYS`, the walk command
  (`commands.rs:379-382`), `paced_player_step`, the followers' pass, the level test, the world turn and its NPC
  turns, the refresh and the slide of spec 20: everything a key press runs. No state is written by the travel.
- Before it presses, the travel checks the step on a copy (the same test as the search): a step refused now is a
  blocked step (below), not a press.

**Stops** (`travel::End`; the walk ends, nothing is pressed after it):
1. `Arrived`: the leader stands on the target at a prompt.
2. `Blocked`: the next step is refused, or the leader did not reach the cell the last step aimed at (refused after
   paying, no wind, a drunk stagger, a swap that went elsewhere): one new plan from where the leader stands; if it
   finds none, or its first step is refused too, stop. The budget is one plan per blockage: a step that lands where
   it aimed restores it.
3. `Combat`: combat mode (A4-0x7b84 + 0x78, `work.rs:241`) at a prompt.
4. `Foe`: a hostile in view at a prompt. Hostile = an actor slot not in the party (status 0x80 clear), not dead
   (status 0x10 clear), aligned evil (status & 0x60 = 0x20) or chaotic (0x60): the alignments 0x12500a lets take a
   good (0x40) actor as a foe (`combat/npc.rs:136-187`). In view = on a view cell the leader sees (`map/view.rs:38`
   `is_visible`, flags bit 0x80) and seen by 0x1224ea (`actors/leaves.rs:118` `can_see`: not invisible, same level).
5. `Input`: any key, repeat or text in a tick's input (K2's held repeat included), or a pointer button pressed (a
   bit set that the handler's buttons before the tick did not have). A release or a pointer move is not a stop.
6. `Prompt`: a key wait other than the command prompt: the event wait 0x106bbc (`ui.input.in_wait`) or the line
   editor (`ui.input.line_input`) while the command prompt is not up (a more prompt, a question, a conversation).
7. `Party`: a member the followers' pass moves (worktype 1, on the party's level, free to step: `party::following`)
   stands more than 8 cells (Chebyshev) from the leader at a prompt; 8 is how far 0x11f0a0 lets a party member end
   from the leader in combat mode (`walk.rs:552-557`), the original's one leash.
8. `Displaced`: at a prompt the leader is more than one cell from where it stood when the last step was pressed, or
   on another level, or another actor leads: not a step's result (a level change on the way, a load, a teleport).
9. `Gone`: no leader, or the game left the command loop's program.

**Determinism and saves.** The travel is engine state (`Engine.travel`), not `State`: never saved, never cloned into a
save, and a new engine (a load from the page) has none. The game's own Restore takes keys (Ctrl-R and its question),
which stop it (5, 6). The search and the checks draw no RNG.

**API** (for M3's entry; signatures in the report):
- `u6_core::actors::travel::reachable(s: &State, target: (u16, u16)) -> Option<Reason>`;
- `Engine::travel_to(&mut self, target: (u16, u16)) -> Result<(), Reason>`, `Engine::travel_active(&self) -> bool`,
  `Engine::travel_end(&self) -> Option<End>`;
- u6-web `Game::travel_to(x, y) -> bool` (map cell, the party's level), `Game::travel_active() -> bool`.
- `Reason::code()` and `End::code()` give stable ASCII codes for the page's words.

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
     z, leader, path, pressed: None, replanned: false, steps, presses, plans }` → `Engine.travel = Some(..)`,
     `Engine.travel_end = None`.
   - 3b each `Engine::tick(input, ms)`: `travel_press(input)` → `Travel::next(&State, Polled { input facts, queued,
     buttons })` → `Hold` | `Press(Key)` | `Stop(End)`; on `Press(k)`: `input ++ [InputEvent::Key(k)]` → `io.events`.
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
   a diagonal only past an open side (the corner rule), returns `Unreachable` for a walled-off cell, stays within
   `WINDOW²` expansions, and leaves the `State` and its RNG as they were.
2. Every planned path, walked by `player_step` on a copy step by step, ends on the target (the path = the keys).
3. Each stop (1–9) ends the walk in an engine test, and each refusal shows in `reachable`.
4. The four diagonal keys read as the walk command in their direction, past the raw keymap
   (`ui/tests/input_tests.rs`); a diagonal into a shut corner is refused by `player_step` with no points spent, no
   move and no line printed.
5. Real data (the castle start state of spec 19's held-key tests): the party travels across a room to a closed door,
   opens it with `u` and a press on it, and travels through it to a cell beyond; each leg arrives with one key a
   planned step and one move a key; the same inputs given at the same ticks from the same start leave an equal
   `State` and frame (no shortcut); steps, turns (prompts), clock minutes and engine ms printed as counts.
6. The page's `Game::travel_to` starts exactly what `reachable` allows, and a refusal leaves a travel under way as it
   was (`crates/u6-web/src/lib.rs` test `travel_to_starts_exactly_what_reachable_allows`).
7. RED: each guard removed fails its test.

## Not settled

- A step whose cell an NPC just entered replans at once; Exult waits and retries first. If replans feel jumpy in towns,
  a one-turn retry (a Pass) is the alternative, at the cost of a command the player did not enter.
- No travel across a level change (ladders, stairs, gates): the walk ends `Displaced` there.
