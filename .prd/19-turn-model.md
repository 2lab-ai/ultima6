---
title: ultima6 — 19 the turn model (research and options)
type: spec
tags: [ultima6, timing, turns, input, research]
---

# 19 — The turn model

Status: planned (2026-10-27; research done, O1 building in round 2 as unit K1, O2 with spec 20). Item 4 of `objects-menu-fog-motion/ssot.md`; the owner's words there: the turn handling feels like a very old game. Round 1 unit T1 drafted it at main `587d973` (the round's common file names `cca7e3e`; the tree read is `587d973`). Related: spec 06 (the fixed tick and the paces timed from the original), spec 20 (smooth motion, unit W1).

Evidence labels used below: **measured** = printed by the T1 probe (`~/Games/ultima6/work/dispatch/r1027-t1/probe/src/main.rs`, outputs `probe-castle.txt`, `probe-fight.txt`, `probe-held.txt`, `probe-visible.txt`, `probe-busy.txt` beside it; game-first boot, 20 ms engine steps, the method of `combat/pace_real.rs:121-196`; it prints engine ms, PAL frames and counts only; built with `CARGO_TARGET_DIR=~/Games/ultima6/work/target-r1027-t1 CARGO_BUILD_JOBS=4 scripts/cargo.sh build --offline` in `probe/`, run as `t1-probe castle pass`, `fight`, `held tap`, `visible`, `busy`); **cited** = a number another spec or note measured; **read** = a `file:line` or an original address; `[추정]` = inference.

## Measured AS-IS

### One turn outside combat, from the key to the next prompt (code path)

1. The key. The page queues every `keydown`, auto-repeats included: `gameKey` (`site/app.js:3804-3816`) → `Game::key_down` pushes one `InputEvent::Key` per call (`crates/u6-web/src/lib.rs:1239-1249`). `Engine::tick` appends keys to the engine queue until it holds `KEY_RING` = 49 (`crates/u6-core/src/engine.rs:583`, `:715-724`); a key past that is dropped.
2. The prompt reads one key: `command_loop` → `read_key` (`main_loop/commands.rs:169`, `ui/wait.rs:135-195`) → `idle_poll` / `poll` (`ui/keyread.rs:48-73`, `:85-169`). The poll takes the queue's front key, in arrival order. The original's drain (A4-0x7a46: empty the ring, keep the last code) is not applied (`keyread.rs:34-41`; the flag is kept but unread, `ui/globals.rs:22-25`). While no key is queued, each poll draws the cursor, refreshes the view once and waits one display frame (`keyread.rs:146-159`).
3. The step: `dispatch` WALK → `paced_player_step` (`commands.rs:377-380`, `actors/walk.rs:76-88`): `player_step` 0x11d4ee moves the leader and runs the followers' pass 0x11c910, all members in one pass (`actors/party.rs:187-239`; each follower pays 5 points, `:235`). Then one wait of `STEP_MS` 24 + `FOLLOW_MS` 41 ms (`walk.rs:31`, `:37`). Nothing is drawn yet (`walk.rs:73-75`).
4. The world turn: `end_of_turn` → `world_turn` 0x1213de (`commands.rs:665-683`, `actors/turn.rs:553-627`). Bookkeeping, formation, schedules, then picks one actor at a time by movement points (`pick_actor`, `turn.rs:426-481`): each pick owes `PICK_MS` 11; a refill when nobody near has points (`turn.rs:580`) advances the clock one minute (`:584`) and waits `MINUTE_MS` 186; an NPC in a combat routine waits `COMBAT_NPC_MS` 46; then, if any object moved inside the view (`objects/moves.rs:76-84` sets `view_dirty`), `AFTER_TURN_MS` 427 and a full window refresh (`turn.rs:617-620`, `main_loop/main_hunk.rs:41-48`). That refresh is the first picture of the step. The turn returns when the pick is the player's character (`leader_picked`, `turn.rs:677`), paying `SETUP_MS` 16 last.
5. The prompt again; a step reprints none, any other command reprints it (`commands.rs:676-690`).

Every wait is `combat::present::pace` (`combat/present.rs:108-117`): it counts the wait (`schedule`, `:143-155`), awaits display frames until due (`display_frame`, `:165-170`) and records the due time (`finish`, `:158-162`). It reads and writes no RNG and no rule state; only `combat.paced`, `due_ms` and `paced_at_ms` change.

### Numbers at 1× (engine ms; PAL frame = 20 ms)

| What | Measured | Cited |
|---|---|---|
| Castle step, no clock minute in the turn | 620–640 ms = 31–32 frames, every seed (0, 5, 1000) | port 31, original 33 (README, spec 06) |
| Castle step with a clock minute (every second step here) | 860 ms = 43 frames | — |
| Castle, all 21 moving steps | median 32.0 frames | — |
| Castle step, key → first published frame | 540 ms (27 frames); the prompt at 640 ms | — |
| Castle step, waits in one turn | 65 (step + followers), 438 (refresh + one pass), 104 (setup + 8 passes); with a minute 27, 307, 438 | — |
| Pass (space) in the castle | 900–980 ms (45–49 frames) when followers move back; 460 ms (23 frames) when none moves | — |
| Start fight, one prompt to the next | 217–499 frames, median 326.0 (n 21; one blocked step 3 frames) | port 301, original 277 (README) |
| Start fight, what the 20 moving turns waited | after-turn refresh 28.7% (95 frames a turn), flight steps and combat NPC turns 17.4%, flights' first render 14.7%, hit flash 13.9%, refresh in a flight 11.8%, passes/steps/minutes 8.2%, messages 2.8%, object names 2.0% (mean turn 332 frames) | oracle per step: refresh 101.5, flight 75.9, flash 33.9 (`re/notes/oracle.md` §E) |
| Start fight, frames published in one turn | 26 in a 4460 ms turn, 41 in a 7400 ms turn (each actor's action is its own picture) | — |
| In-view NPC moves in the quiet castle | 0 in 180 turns (so no extra 427 ms refreshes there); in the fight 3–6 refreshes a turn | — |

### Input while a turn runs (measured, castle, the owner's key repeat)

The owner's macOS repeat: `defaults read -g InitialKeyRepeat` = 15, `KeyRepeat` = 2 (units of 15 ms: first repeat after 225 ms, then one press every 30 ms). The probe sends those presses for a hold of H ms, then counts the leader's steps (`held()` in the probe):

| Hold, south (open for about 10 cells) | Presses sent | Queued at release | Steps while held | Steps after release | Quiet prompt after release |
|---|---|---|---|---|---|
| 1000 ms | 27 | 25 | 2 | 8 | 6720 ms |
| 3000 ms | 94 | 49 (the ring's limit) | 5 | 5, then blocked by a wall | 5460 ms |
| 6000 ms | 194 | 49 | 9 | 1, then blocked | 2560 ms |
| 1000 ms, one press per prompt only (the original's model) | 2 | 0 | 2 | 0 | 500 ms (the turn in progress) |

- A key pressed during a turn is kept and starts the next turn at once: two presses 100 ms apart give steps at 40 and 640 ms with no prompt wait between (`tap`, measured).
- On open ground every queued press is one more step after the release: 49 queued × 32 frames ≈ 31 s of walking after a hold of 3 s or more `[추정: 49 × the measured step]`.
- A held mouse button does not overrun: its walk samples the pointer every `HELD_WALK_MS` = 660 ms (33 frames, from the original, `ui/target.rs:25-42`) and stops at the release (`target.rs:122-164`).
- Speed (spec 06) multiplies every engine step, the prompt's own animation included: `Pacer` presets 1×..8× and Max (`pacer.rs:23`, `:28`; `site/app.js:1217`), and the idle poll animates once per engine step (`keyread.rs:146-159`). There is no setting that shortens a turn but leaves the ambient animation at 1×.

## The original's design (Ultima VI, Amiga 1990)

### Who acts when

- **The unit is movement points against dexterity.** Every actor's points become its dexterity, plus any debt carried over, at a refill (`turn.rs:484-494`; 0x1213de's refill loop). Each refill is one game minute: the scheduler calls the clock 0x10368a with 1 (`turn.rs:584`; the decompile of 0x1213de calls `thunk_FUN_0010368a` right after the refill). There is no fixed round: a turn is "until the player's character is picked again".
- **The pick.** One pass over the 256 actors picks the one with the highest points/dexterity ratio, ties to the higher dexterity; an actor whose points reach its dexterity is taken at once (`turn.rs:426-481`; 0x1213de's loop, the test `mpts * dex_prev - dex * mpts_prev`). Only actors on the party's level inside the 40-tile window around the party take part; those outside lose their points, walkers there jump home (`turn.rs:443-457`; 0x1213de's `< 0x28` window test). When nobody near has points, everyone is refilled and the clock runs one minute.
- **Costs.** A step costs 5 plus the terrain nibble of the tile left and of the objects on it (0x11ef84, `walk.rs:217-232`), diagonals 5 more for the player (`party.rs:255`); party haste halves, slow doubles, never below 1 (0x11c650, `walk.rs:179-196`). An NPC whose turn spent nothing pays 10 (`npc.rs:540-548`). Pass sets the leader's points to 0 (`commands.rs:461-480`), so the world runs to the next refill.
- **The party.** Followers (worktype 1) never act in the scheduler (`turn.rs:458-460`); they move in the leader's step, all in one pass behind it (0x11c910, `party.rs:187-239`). In solo mode only the chosen member is player-controlled (`commands.rs:523-585`).
- **NPCs.** Every other pick runs its worktype (0x1200b0, `npc.rs:401-475`): schedule walking, loitering, guards, combat routines; then the bookkeeping 0x121056 again.
- **The player's turn.** The world turn returns when the pick is in worktype 2, the player-controlled mode, making that actor the leader if it is another member (`turn.rs:677-697`; 0x1213de's tail calls 0x103fd4 and sets A4-0x7afd).
- **A fight turn.** Combat mode gives each member a combat worktype; a member in the controlled mode is prompted on its own turn, the others act by their routines (`turn.rs:507-527`, `npc.rs:385-391`). Community descriptions agree: "command" lets you control a member each turn, the other modes fight on their own (CRPG Addict, https://crpgaddict.blogspot.com/2014/03/ultima-vi-combat-equipment-and-magic.html; StrategyWiki, https://strategywiki.org/wiki/Ultima_VI:_The_False_Prophet/Controls). So between two of the player's prompts every faster or ready combatant takes its turn, one after another.
- **Cross-check.** Nuvie's Ultima VI script implements the same scheduler: the same ratio test and early exit, `advance_time(1)` when nobody has points, the refill with debt carried, and the loop ends on the player-controlled worktype (`actor_update_all` and `advance_time` in https://raw.githubusercontent.com/scummvm/scummvm/master/devtools/create_ultima/files/ultima6/scripts/u6/actor.lua). Nuvie models the PC version; the rules match, the Amiga timings do not transfer.

### What paces it

- **Nothing in a turn waits on purpose.** The VBlank server only rewrites a display register, clears a flag and moves the pointer; the one routine that spins on that flag is reached from the shutdown and the Use machine only (`re/notes/oracle.md` §E; `combat/present.rs:11-18`). A turn is as slow as the 68000 and the blitter draw it.
- **One actor, one redraw.** After every pick that is not swallowed, the scheduler calls 0x102f2a, which redraws and copies the whole window when anything in the view changed (decompile of 0x1213de: `thunk_FUN_00102f2a()` inside the pick loop; `turn.rs:617-620`). Each moving actor in view is therefore its own picture, about 21 PAL frames apart (traced median 21.336, `turn.rs:12-19`). The port reproduces that as waits.
- **The picture comes at the end.** The step itself draws nothing; the first picture is the refresh after the first pick (`walk.rs:73-75`). Measured in the port: 540 ms from the key, out of 640 (above).

### Input in the original (read from the image)

- **The ring.** The input handler 0x151b08 maps each RAWKEY event with 0x151d00 and pushes the result into a 50-slot ring (0x14f0ac). For a key release 0x151d00 returns 0 (codes ≥ 0x80 fall into `if (uVar3 == 0) return 0`), and the handler pushes it with no test: the bytes at 0x151c32 are `20 07 61 00 00 ca 4e ba 02 96` (`move.l d7,d0; bsr 0x151d00; jsr` the ring push), so a release takes a slot as a 0.
- **The drain.** The idle key poll 0x105c8e loads D0 from A4-0x7a46 before the key take: bytes at 0x105d06 are `30 2c 85 ba 4e ba` (`move.w -0x7a46(a4),d0; jsr`), return address 0x105d0e. The key take 0x134e7c, with D0 ≠ 0, takes every code in the ring and keeps the last. The flag's initial word is 1 (image word at 0x153caa = `0001`); the line editor 0x107cba clears it while a line is typed and sets it back (bytes at 0x107cd6 `42 6c 85 ba`, at 0x107d5a `39 40 85 ba`).
- **What a player feels, derived.** At the prompt the last event decides: a key still down (its auto-repeat keeps adding presses) gives one step a turn and walks on; a key released before the poll leaves a 0 last, and the press is lost. A tap during a turn is dropped, a held key walks, and a release stops the walk at the next prompt. The Amiga sends repeats as key events with the repeat qualifier set ("Repeat key events are sent with the IEQUALIFIER_REPEAT bit set", Amiga ROM Kernel Reference Manual: Libraries, https://www.amigang.fr/rkm/Libraries_Manual_guide/node01F3.html); the handler does not test that qualifier (it reads only the bits at +8 & 0x10 and +9 & 0x20). `[추정]`: no oracle run held a key across a turn and released it before the prompt; the oracle's fight harness held each key until the game took it (`re/notes/oracle.md` §E, "held until A4-0x7a2c clears"), which is consistent with this reading.
- **Short taps at the prompt.** Each idle poll of the original also refreshes the view (0x105c8e calls 0x102ef0 when A4-0x7a56 is set), and the render alone takes 8.90 PAL frames timed at that poll (`re/notes/oracle.md` §E). A tap whose press and release fall between two polls leaves a 0 last and is lost `[추정]`. The port polls every 20 ms and keeps every press (`target.rs:161-164` for the pointer; the queue for keys): better than the original, and worth keeping.
- **The mouse.** A held button walks one cell per 33 PAL frames, sampled from the press (oracle runs b1..b6, `target.rs:25-42`); the release ends it.

## Other games: the specific mechanisms

| Game / source | Mechanism | Source |
|---|---|---|
| Nuvie (ScummVM), Ultima VI | Same scheduler as the original (above). All NPC turns between two player moves run inside one `ActorManager::moveActors` call, which returns at once while `wait_for_player` is set and wraps the script call in `pause_user` / `unpause_user`; `Game::play` then draws the map window once per loop iteration. So every NPC move between two prompts appears in the next frame, together, with no per-actor redraw | https://raw.githubusercontent.com/scummvm/scummvm/master/engines/ultima/nuvie/actors/actor_manager.cpp, https://raw.githubusercontent.com/scummvm/scummvm/master/engines/ultima/nuvie/core/game.cpp |
| Nuvie, timed sequences | Multi-step scripted moves are timed events: the party walking to a moongate or into a dungeon and gathering around a campfire step every 50 ms; resting advances the clock in real time (`TimedAdvance`, `TIMEADVANCE_PER_SECOND` 1000); visual effects are awaited by callback, not by fixed delays | https://raw.githubusercontent.com/scummvm/scummvm/master/engines/ultima/nuvie/core/timed_event.cpp |
| Nuvie, held keys | `Events::move` moves only when `Player::check_walk_delay()` passes; the delay is 125 ms on foot in Ultima VI (horse 50, ship 20, raft 100, balloon 10) and restarts at each step. Presses that come before the delay ends are consumed without a move, not queued; key-up events are ignored | https://raw.githubusercontent.com/scummvm/scummvm/master/engines/ultima/nuvie/core/player.cpp, https://raw.githubusercontent.com/scummvm/scummvm/master/engines/ultima/nuvie/core/events.cpp |
| Exult, Ultima VII (real time) | Each actor's walk is its own event in `Time_queue`, kept sorted by due time; `Path_walking_actor_action::handle_event` takes one step and returns the actor's frame time as the delay to its next step (a blocked step waits 100–600 ms at random). Actors therefore move concurrently, each on its own clock. The standard delay is `1000 / fps` with fps 5 by default (200 ms). NPC behaviour is a `Schedule` object per NPC whose `now_what()` picks the next task; schedule changes are keyed to 3-hour slots | https://raw.githubusercontent.com/exult/exult/master/actions.cc, https://raw.githubusercontent.com/exult/exult/master/tqueue.h, https://raw.githubusercontent.com/exult/exult/master/gamewin.cc, https://raw.githubusercontent.com/exult/exult/master/schedule.h |
| Exult, smooth scrolling | Since the 1.4 snapshot (2007): the screen position is interpolated at the camera actor's animation rate, falling back to the standard delay; the avatar and party are smoothed, other objects are not; a percentage setting. In current `gamewin.cc` it is `config/gameplay/smooth_scrolling`, default 0 | https://www.oldgamesitalia.net/forum/index.php?showtopic=12571&mode=threaded&pid=172499, https://raw.githubusercontent.com/exult/exult/master/gamewin.cc |
| Roguelike time systems | Four families: player-centric (`move_monsters()` after the player), fixed turns, a priority queue keyed by each actor's next action time, and energy (each tick adds energy by speed; an actor acts at a threshold and pays the action's cost). Ultima VI's points-against-dexterity is an energy system with a refill per minute | https://www.roguebasin.com/index.php/Time_Systems |
| A turn-based loop in a browser (Nystrom) | The engine processes one step and returns to the UI; the hero's turn returns at once when no action is queued, so the browser never blocks; a failed action does not spend the turn; speed is energy gained per pass | https://journal.stuffwithstuff.com/2014/07/15/a-turn-based-game-loop/ |
| Shattered Pixel Dungeon | `Actor.process` picks the lowest next-act time; before a character acts it waits only if that character's own sprite is still moving. A visible move starts the sprite's tween and returns at once (other characters keep acting, so moves overlap); a move out of the hero's sight is placed instantly. A visible attack returns false and resolves when its animation completes | https://raw.githubusercontent.com/00-Evan/shattered-pixel-dungeon/master/core/src/main/java/com/shatteredpixel/shatteredpixeldungeon/actors/Actor.java, https://raw.githubusercontent.com/00-Evan/shattered-pixel-dungeon/master/core/src/main/java/com/shatteredpixel/shatteredpixeldungeon/actors/Char.java, https://raw.githubusercontent.com/00-Evan/shattered-pixel-dungeon/master/core/src/main/java/com/shatteredpixel/shatteredpixeldungeon/actors/mobs/Mob.java |
| Dungeon Crawl Stone Soup | Waits are options in ms: `travel_delay = 20` after each travel step, `explore_delay = -1`, `rest_delay = 0`, `view_delay = 600` for ranged and spell animations, `use_animations` picks which animations run; `tile_key_repeat_delay = 200` before a held key repeats; `flush.command` (default false) dumps the whole input buffer whenever the game is about to read the next command, `flush.failure` (default true) on a miscast; `travel_key_stop = true` stops travel at any keypress | https://raw.githubusercontent.com/crawl/crawl/master/crawl-ref/docs/options_guide.txt |
| Brogue CE | Travel steps one cell at a time with `pauseAnimation(25, …)` between steps (500 for `travelMap`); when the pause reports an interruption the trip stops (`rogue.disturbed`) | https://raw.githubusercontent.com/tmewett/BrogueCE/master/src/brogue/Movement.c |
| Cogmind | Moves and most actions resolve fast enough to read as simultaneous for all units; an event neither seen nor heard applies instantly, without animation; the game pauses only for visible or audible effects. Keyboard running and path moves wait `movementDelayInterval`, 100 ms by default, and any input stops the move; new threats in view block it | https://www.gridsagegames.com/blog/2014/01/animation-vs-pacing-dilemma-modern-roguelikes/, https://www.gridsagegames.com/blog/2019/11/movement-qol/ |
| Input buffering for grid games (pancelor) | One buffered input: a newer press overwrites the older; it is consumed when the current move animation ends. A held direction is read when no tap is buffered, and the platform's auto-repeat is ignored, so a held key walks without flooding and a release stops it | https://pancelor.bearblog.dev/input-buffering-for-grid-games/ |

What they share, against our AS-IS: the world's other turns resolve without a pause each (Nuvie, Cogmind, Shattered Pixel Dungeon), moves overlap in time (Exult, Shattered Pixel Dungeon), a held key walks at a pace set by the game, not by the OS's repeat (Nuvie's walk delay, DCSS's repeat delay, pancelor), at most one input waits (pancelor) or the buffer is flushed (DCSS `flush.command`), and every remaining wait is a named, shortenable number (DCSS, Brogue, Cogmind).

## Diagnosis: what makes it feel old, ranked by the effect on play

Not a cause: the rules (the scheduler is the original's, and Nuvie's matches it, above), and compute (about 1% of a core, spec 06).

1. **A held key keeps walking after it is let go (port only).** Every OS repeat is queued as one more step (`app.js:3804-3816`, `lib.rs:1239-1249`, `engine.rs:715-724`) and the poll never drains (`keyread.rs:34-41`). Measured in the castle: a 1 s hold gives 2 steps while held and 8 after the release, the last 6.4 s after it; a hold of 3 s or more fills the 49-key ring. The original keeps only the last code at each prompt, a release included, so a held key walks one step a turn and stops at the release (read above). Walking by keyboard is most of play, so this ranks first. The same queue makes a tap during a turn start the next step unseen (two taps 100 ms apart: steps at 40 and 640 ms, measured).
2. **The screen freezes, then jumps.** After a step key nothing changes for 540 ms (27 frames), then the whole step appears 100 ms before the prompt (measured, castle). In a fight each actor's action is a separate picture, 26 and 41 of them in two measured turns. Faithful: the original shows nothing until its redraw is copied (`turn.rs:12-19`, 0x1213de → 0x102f2a). The motion the eye expects in that half second is item 5's slide; the delay before the first picture is this spec's.
3. **One actor at a time, each with a full redraw.** Every pick that changed the view pays 427 ms and redraws the window (`turn.rs:617-620`); in the start fight that is 3–6 refreshes a turn, 95 frames, 29% of a turn (measured). Faithful (0x1213de). In a busy scene each NPC walking in view adds 427 ms to the player's wait `[추정: not measured; the quiet castle had 0 in-view NPC moves in 180 turns]`.
4. **Long fight turns.** 217–499 frames from one prompt to the next, median 326 (4.3–10 s at 1×). About 69% of a turn is the original's drawing time the port waits out (after-turn refreshes, flights' first renders, hit flashes, refreshes in flights), 17% more is the 46 ms constant that serves both a flight's drawn step and an NPC's combat routine, the rest messages, names, passes and minutes (measured shares above). Faithful to the oracle's pace (277 median).
5. **Cell-to-cell jumps.** Actors teleport one cell (item 5, spec 20).
6. **Dead time with nothing to show.** A Pass waits 460 ms when nothing moves and 900–980 ms when the followers step back (measured); steps alternate 640 and 860 ms because every second one carries a clock minute (measured; `MINUTE_MS`, `turn.rs:35-41`). An uneven beat reads as stutter. Faithful.
7. **Speed is all or nothing.** The speed control scales every engine step (`pacer.rs`), so quick turns also mean a quick spinning cursor and quick water at the prompt (`keyread.rs:146-159`). No setting shortens only the original's drawing waits.

Items 2–4 and 6 are the original's pace reproduced on purpose; only item 1 is a port deviation. Items 2, 3, 4 and 6 are pure `pace()` time, which touches no RNG and no rule state (`present.rs:108-170`), and the only other game code that reads the engine clock is the held-pointer walk (`ui/target.rs`; every other `now_ms()` reader in `u6-core` is a test or the engine itself, by `grep -rn "now_ms()" crates/u6-core/src`). So an option that shortens, reorders or merges those waits keeps the rules, the RNG order and the savegame by construction; the held walk keeps its 660 ms cadence, because it waits for its own due time. (Spec 06's Max is not that receipt: it runs the same engine time faster in wall time.)

## Options that keep the rules, the RNG order and the savegame

Each changes input or presentation only. "Receipts" are the fidelity checks of spec 06 (castle step, start fight, held walk) and the T1 probe's scenarios.

### O1 — Input: the original's drain, with the release

- **What changes.**
  - The page also forwards `keyup` and the `e.repeat` flag of each `keydown` (a new `Game::key_up(code, key)` beside `key_down`, `lib.rs:1239`; native key-up likewise). The engine queue then holds releases and marks repeats.
  - At a key wait with the drain flag set (A4-0x7a46: every wait but the line editor, as in the original), the poll takes the newest queued press and drops the older ones: the original's 0x134e7c drain (`keyread.rs:48-73`).
  - A release removes that key's queued presses. O1a (as the original, where the release's 0 comes last): all of them, except that a press and its release arriving in the same engine step still count as one tap at the prompt (the port keeps the short taps the original could lose). O1b (one slot, the modern rule, pancelor): only its repeats, so a tap during a turn runs once.
  - So a held key walks one step a turn, the prompt itself gating the cadence (castle 640/860 ms), and stops at the release; the line editor (drain off) keeps every key and repeat, as now. No new pace constant: the mouse keeps its 660 ms sampler because 0x106bbc samples the button on every pass (`target.rs:129-131`); keys need none.
- **Fidelity.** Rules, RNG and savegame untouched (no RNG on the key path). O1a matches the original's input as read from the image; O1b keeps one tap the original drops. Other drained waits (a more prompt, the cross-hair) also see only the newest press, as in the original.
- **Cost.** Small: `lib.rs` (`key_up`, the repeat flag), `site/app.js` (`keyup`), `u6-native` (key-up in the window; headless `--keys` scripts presses only, so each counts as a press and its release in the same step), `engine.rs` (release and repeat in `InputEvent`, the ring count unchanged), `keyread.rs` (drain and cancel at the poll). Receipts: the probe's held scenarios become tests: steps while held = the prompts reached while the key is down (±1), steps after the release = 0 (O1a) or at most 1 (O1b); the probe's "one press per prompt" row is that model (2 steps for a 1 s hold, 0 after).

### O2 — Show the step at once

- **What changes.** After `player_step` (`walk.rs:76-88`), refresh and present the window before the step's waits, instead of at the first pick's refresh (`turn.rs:617-620`). The waits keep their sum, so the prompt comes at the same engine time (castle 620–640 ms); only the first picture moves from 540 ms to the key's own frame.
- **Fidelity.** As first written (a refresh in the real `State`) this is not pure: `update_view` draws from the game RNG when object type 0x188 first enters the view (`objects/draw.rs:265-269`), so the draw would move before the first pick. The pure form builds the early scene on a clone of `State` and hands it to the presenter only (spec 20 Design 4); then pace, rules, RNG and the key-to-prompt receipts are unchanged.
- **Cost.** Small. It is also what item 5 needs: a slide can only use the interval if the step is shown when the interval starts.

### O3 — A turn-speed factor for the original's drawing waits only

- **What changes.** A factor k (original 1, then 2, 4, instant) divides every `pace()` wait (`present.rs:108`) and `HELD_WALK_MS`, nothing else: vblank waits, the idle prompt, music and the opening keep 1×. Spec 06's speed control stays as it is and multiplies on top.
- **Fidelity.** Rules, RNG, savegame unchanged (above). The pace becomes the player's choice; at k = 1 the receipts hold.
- **Cost.** Small to medium: the factor in `pace`'s schedule (`present.rs:143-162`) and in the held walk's due time (`target.rs`); a setting beside Speed on the page; receipts at k = 1 unchanged, plus one per k (a fight turn = 1/k of its waits).

### O4 — Merge a world turn's quiet NPC moves into one picture

- **What changes.** Inside `world_turn` (`turn.rs:553-627`), consecutive NPC picks that show nothing of their own (no recorded frame, no line, no arrest, no conversation: `shows`, `turn.rs:669-675`) do not refresh one by one; their moves are drawn by one refresh and one `AFTER_TURN_MS` (or k-scaled) wait, and spec 20 slides them together. Any pick that shows something flushes the batch first, so a strike's picture still comes after the moves before it.
- **Fidelity.** Rules and RNG unchanged (picks run in the same order; only waits and draws merge). The pace and the picture count change: a scene with N NPCs walking in view waits one refresh instead of N, and the original's one-at-a-time pictures are gone.
- **Cost.** Medium: the world turn's pay and refresh logic and its interplay with `pay` and `owe_first` (`turn.rs:631-667`); receipts: the start fight's turns barely change (its NPCs mostly show flights), a busy-town save is needed to measure the gain.

### O5 — A key skips the rest of the current playback

- **What changes.** During a world turn's playback (`play_effects`, NPC turns, flights), a press of Space or Escape ends the remaining `pace()` waits of that turn at once; the pictures still come in order, faster (Brogue, DCSS `travel_key_stop`, Cogmind).
- **Fidelity.** Rules and RNG unchanged; the skipping key is consumed and is not a command.
- **Cost.** Small once O3 exists (k = instant for the rest of the turn); it needs O1's key state to see the press without taking it as the next command.

| Option | Fixes (diagnosis item) | Fidelity cost | Build cost |
|---|---|---|---|
| O1 | 1 | none to rules; O1a is the original's input | small (no new pace constant) |
| O2 | 2 (and enables item 5) | one picture's order | small |
| O3 | 2, 3, 4, 6, 7 at the player's choice | pace at k ≠ 1 | small–medium |
| O4 | 3 | picture count and pace | medium |
| O5 | 4 | pace of the skipped turn | small after O3 |

## Recommendation

1. **Round 2, the cheapest step (built beside item 5): O1, as O1b.** It is the one port deviation, the largest measured effect (8 extra steps from a 1 s hold; a full 49-press ring from 3 s), and the cheapest fix. O1b over O1a: a press the player gave and the game silently drops reads as unresponsive, which is the complaint; the original dropped it only as a side effect of its drain keeping a release's 0 (read above). Reversible: O1a is one condition away.
2. **Round 2, with item 5: O2.** Item 5's slide needs the step on screen when its interval starts; today the first picture comes 540 ms into a 640 ms step, which would leave a slide 100 ms or push the prompt late.
3. **Round 3, after the owner has played 1 and 2 with the slides at the original pace: O3** as a setting beside Speed, "Turns: original / quick (k = 2) / fast (k = 4) / instant", default original (spec 06 kept 1× as the default for the same reason: the oracle is the baseline). **O5** with it.
4. **O4 only on request**, and only in a non-original turn setting: it removes the original's one-at-a-time pictures, which is the U7 look the owner named for item 5, but it changes what the player sees, not only when.

What spec 20 (unit W1) must take from this:

- **The intervals to slide in, at 1× (measured):** a castle step, key to prompt, 620–640 ms, or 860 ms when the turn carries a clock minute; a held walk, one step a turn by key with O1 (the same 640/860 ms) and one step per 660 ms by mouse; a Pass's followers stepping back, inside 900–980 ms; a blocked step, 27–60 ms and no move, so no slide. Divide by the speed (spec 06) and, with O3, by k.
- **Where a slide starts:** at the key, which needs O2. Without O2 the step is published 540 ms after the key.
- **Who moves together:** the leader and the followers move in one pass (0x11c910) and slide together. NPCs move one per pick, each published by its own refresh about 427 ms after the previous picture; they slide one after another inside those windows, unless O4 merges them. In a fight every action is its own picture, 40–500 ms apart in the measured turns.
- **Input during a slide:** with O1 at most one press waits and a held key steps once a turn; without O1 a held key queues up to 49 presses, so up to 49 slides would play after the release. O1 has to land before or with spec 20.
- **Max and instant:** spec 20's collapse rule applies to both spec 06's Max and O3's instant.

## Open questions

1. **Confirm the original's held and tapped keys in the oracle.** Hold a key across a castle turn and release it before the prompt; tap a key during a turn. The read above predicts: the held key walks one step a turn and stops at the release, the tap is lost. The oracle's harness so far held every key until the game took it (`re/notes/oracle.md` §E).
2. **A busy scene.** The quiet castle had no in-view NPC moves in 180 turns; O4's gain needs a save with NPCs walking in view (a town by day).
3. **The default turn speed** after O1 and O2: the owner's taste.
4. **The prompt's own animation rate** (for W1 and a fidelity check). The port's idle poll waits one PAL frame (`keyread.rs:159`); the original's poll also refreshes the view (0x105c8e → 0x102ef0), and the render alone took 8.90 PAL frames at that poll (`re/notes/oracle.md` §E), so the port's water and cursor at the prompt may cycle up to about 9× the original's rate `[추정]`. An oracle count of overlay frames per second at the prompt settles it.
5. **The held cadence.** With O1 a held key steps once a turn, the original's cadence (the castle's 640/860 ms; the mouse's 660 ms sampler is the same pace there). Nuvie walks at most every 125 ms on foot; a faster held walk is a pace change, so it would belong to O3, not O1.

## Tensions

- O2's purity (2026-10-27, dispatcher): T1 drafted O2 as a refresh in the real `State` and called it pure; W1 rejected that form, because of the RNG draw in the view build (`objects/draw.rs:265-269`). Settled by spec 20 Design 4: the early scene is built on a clone and never touches the live state. That clone's cost is measured before the form is kept.

- `engine.rs:577-582` and `keyread.rs:34-41` say the drain was left out because the platform sends presses only, "no key-up codes that the drain would otherwise skip past", and that the port gives about twice the original's type-ahead for tapped keys. The image says the drain keeps the last code, a release's 0 included (`0x151c32`, `0x105d06`, `0x134e7c`, word `0x153caa`): the original has no type-ahead for a tap during a turn, and one step a turn for a held key. Not resolved here; O1 decides, and open question 1 tests it.
- The castle step: README and spec 06 say 31 PAL frames in the port; the probe measures 31–32 for a step without a clock minute and 43 with one, median 32.0 over 21 moving steps (every second step carries the minute here). The start fight: README 301; the probe 326 median over 3 seeds against the receipt's 9 `[추정: seed set]`.
