---
title: ultima6 — 17 context menu on a right press
type: spec
tags: [ultima6, ui, input]
---

# 17 — Context menu

Status: in progress (2026-10-28). Item 2 of `objects-menu-fog-motion/ssot.md`. Research: round 1 unit M1
(`~/Games/ultima6/work/dispatch/r1027-m1/menu.md`, code at `587d973`); build: round 2 unit M2 (shipped in deploy 30
as a page overlay); in-game rework: unit M3 (branch `feat/ingame-menu`, from `60f92c6`). The entry "Go here" starts
unit M4's travel (spec 21): wired in M3's round 4, the merge with integ/r1027 9e8f133 (M4 travel, T1, K2, D2), where
the menu, the travel and spec 23's history view share one tick and one composition.

Owner, 2026-10-28, verbatim: "문맥 메뉴 인게임 ui로 해줘 시발 왜 웹페이지 ui로 만듬?" and "우클릭시 여기로 이동도
하나 추가해줘 해당 위치까지 a*로 검색해서 이동하는 명령".

## Measured facts (main 88eb5b0, re-read at 587d973)

- The page sends the right button to the engine (`site/app.js:3870`, `buttons & 3`) and suppresses the browser menu
  on the canvas (`:3896-3898`).
- In the original a right press on the map with a command underlined repeats that command on the cell (0xa9); with
  none it walks like the left button (`crates/u6-core/src/ui/click.rs:116-148`). On the command bar a right press only
  underlines (`:167-179`); on the panels it repeats the underlined command on the object under the pointer
  (`:181-197`).
- Every map command takes its selection from 0x105f7c (`ui/cursor.rs:55-75`): the cell under the pointer, then
  0x1225c0 (`combat/leaves.rs:159-192`): the first visible live actor in the cell's location list, else the first
  visible object, else the first hidden one; -1 for a cell that shows nothing. Each command then resolves that
  selection its own way: Talk, Look, Move and Attack act on it (a horse's rider for Talk and Look); Get and Use act on
  the object under an actor or a cover (`get.rs:114-117`, `usecmd/dispatch.rs:100-128`).
- Use's type gate is the 85-word table A4-0x66dc (0x128b6a, `objects/commands/keys.rs:27-37`); it equals the type set
  of the port's Use switch (`usecmd/cases.rs:13-123`), both directions.
- Commands that refuse before reading a target (aboard: Cast, Get, Drop, Move `main_loop/commands.rs:588-598`; Attack
  `combat/attack.rs:166-187`; Cast without a book, with the negate timer, with no spells `magic/cast.rs:72-96`,
  `:171-191`) return to the prompt; a target press queued in advance would then walk the party (`ui/click.rs:141-149`).
- Wide (spec 13 Decision 4): a non-Look command takes a map target only inside the 11 × 11 window round the leader
  (`ui/click.rs:127`, `:156-165`); Look takes any cell of the view.
- Escape at the prompt passes a turn (`main_loop/commands.rs:376`).

### Measured for the in-game menu (unit M3, 60f92c6)

- The deploy-30 menu is a page element: `site/index.html:214` (`#context-menu`, styles `:78-92`), opened by
  `openContextMenu` (`site/app.js:4004`) from the canvas's `pointerdown` (`:3934`) with the labels and reasons of
  `site/i18n.js` (`cmd*`, `why*`, `theft`, en `:75-110`, ko `:297-332`) and the names of `Game::context_menu`'s JSON
  (`crates/u6-web/src/lib.rs:820`, `menu_json` `:1837`). The engine never drew it; the HD compositor never saw it.
- The engine owns the menu's meaning: `ui/menu.rs:219` `query`, `:694` `Run::start`, `:705` `Run::step`. The run was
  held by the page's `Game` (`u6-web/src/lib.rs:515`, `step_run` `:1457`).
- The original's text windows: ink A4-0x656a (6 in the owner's game) on paper A4-0x656c (15) (`ui/text.rs:77`,
  `ui/textstate.rs:152`, 0x107d7c), glyphs through 0x105228 (`text.rs:186`) on the 40 × 25 grid of 8 × 8 cells
  (`text.rs:5`). The frame round the message scroll is the paper bitmap 0x1350d8 draws at the boot
  (`main_loop.rs:415-429`, kept as `ui.paper`): next to the window's first column, a few one-pixel lines of other
  colours, then the paper's own colour again (5 lines in the owner's paper, read at run time, never stored).
- The original marks the current line of a list with glyph 0x1a in column 0, the names from column 1 (the spellbook
  page, 0x116218, `magic/book.rs:329-364`). Its only dither is 0x134044's checker, `(x + y) & 1` of a cell's pixels
  (`~/Games/ultima6/re/dumps/game/0x134044.json`).
- The pointer sprite is an overlay of the published frame, never part of the screen buffers (`ui/pointer.rs:8-10`);
  the shown frame is synced from the published one (`engine.rs:855`, `:869`), and the HD frame is composed from the
  published frame without the sprite (`hd.rs:163`). A frame plane syncs from one other only (`u6-render` `sync_from`).
- The original copies and scrolls rectangles of its screen (`combat/present.rs:4`, `ui/text.rs:385`): a menu drawn
  into the screen buffers would be taken as game screen.
- The HD layout and Wide cut the frame into regions (`site/app.js:1583-1589`, `:2121-2130`): the map, the side panel,
  and in Wide the inventory view split at its x 72 into the paper doll and the backpack list (panel x 0xf0: the
  equipment cells end there, the list cells start at 0xf8, `inventory/panel.rs:372-420`).
- The original's command letters at the prompt: A, B (Combat), C, D, G, L, M, R, T, U (`main_loop/commands.rs:234-290`).

## Contract

**When.** With the engine's context menu on (`Engine::set_context_menu`; the web build turns it on at every start,
the native build and the engine's own tests keep it off, so the original's right press stays the default), a right
press (the right button alone going down) on the map or on an inventory or equipment slot (Wide's slots included)
opens the menu only at the command prompt: A4-0x7a2c is 1, nothing is queued for the game and no chosen entry is
still running. Everywhere else the right press reaches the game as before: it confirms a pending target, underlines
on the command bar, passes on the rest of the screen, and keeps its current effect in a conversation, at a more
prompt, at Drop's count or spot and at Move's direction. Left-button walking and the arrows are unchanged;
right-button walking on the map at the prompt is gone (the menu replaces it).

**What it lists.** One section per resolved object, headed by its display name (`objects::shown_name`, locale aware;
never an actor's own name), in cell order: the selection's section, then the object under it. Each entry is a command
that resolves to that object. Map: Attack, Cast (the book's current spell, on this cell), Talk, Look, Get, Move, Use.
Panel: Look, Drop, Move, Use, and the slot's own left-press action (ready, open, take off). The underlined command,
when there is one, is the first entry. Rest and Combat appear only as that first entry. A map menu ends with **Go
here** (port-only code 0x100) for the pressed cell, a bare tile included, except on the leader's own cell; it starts
unit M4's travel (spec 21) and is disabled, reason Unreachable, when M4's reachability query refuses the cell.

**Tiers** (M1 §2): a condition of the target's kind hides the entry (no Use on a type outside the 85, no Talk to
oneself, no Get of an actor, no Move, Drop or Ready of type 0x4c, which the object moves refuse at the prompt:
`move_to` always, `move_into` unless a body transfer sets its flag, objects/moves.rs). A situational, pure condition
(a read of the current state: out of reach, too heavy, aboard, outside Wide's window, no free hand, no book, negate
timer, no spells, level, points, reagents, solo, a blocked door, a locked door or chest (Use, reason Locked), readied
by another member, a case's own refusal, no way there) disables it with a reason. A random draw or a later prompt
never disables. Get of what is not the party's is marked as theft. An invisible selection gets no section (the cell
shows as its tile with Look only); a cover keeps its section.

**Running a choice.** The original's own mouse flow: a left press and release on the command's icon, then a left
press and release at the target, the target press released only once the command reads its target: A4-0x7a32 is 1,
A4-0x7a2e is the code, `ui.input.accept` is true, no queued events, and the engine is blocked inside the target
read (a port-only flag set at `wait_event`'s entry, cleared on return). If the engine comes back to the prompt
first, the target press is dropped. No underline is written and the savegame is untouched. Rest, Combat and the
slot's own action are a single press (the icon's, or a left click on the slot). The engine holds the run
(`ui::popup::Popup`), stepped at the start of every tick before that tick's input is queued.

**Engine API (port-only).**
- `Engine::set_context_menu(on)`: the menu on or off (off closes an open one).
- `Engine::set_context_menu_hidden(rects)`: the frame rects (x0, y0, x1, y1) the platform does not show; a menu keeps
  off them where it can, and an open one is laid out again (Where it is placed). `Engine::context_menu_hidden()`:
  the rects it holds now (tests).
- `Engine::dismiss_context_menu() -> bool`: closes an open menu for a press of the platform's that reaches no frame
  pixel, at once; `Engine::tick_dismissing(input, dismissals, ms)`: a tick with such presses at their places among
  `input`'s events. `Engine::set_wall_clock(ms)`: the platform's wall clock, which a double click is timed on.
  `Engine::pointer_at() -> Pointer`: the menu's pointer while it takes it, else the game's (K3's OS cursor).
  `Engine::game_presses()` / `Engine::game_keys()`: the presses and the key events the menu let through to the game,
  counted (tests). `Engine::go_here_asked()`: the cells Go here asked travel to, each given to `Engine::travel_to`
  (spec 21) as chosen, unless a menu is open after the input that chose it (then none starts; tests).
  `Engine::context_menu_gate(pending)`: the gate a tick puts before a right press that would open the menu (§2), for
  a press after `pending`, the platform's events not yet given to a tick (tests: `Game::context_menu`).
- `Engine::context_menu() -> Option<View>`: the open menu as drawn: its box and text area in frame pixels, and per
  row its kind (heading, entry, note), its code, whether it can run, its reason, its theft mark and its pixel rows,
  the highlighted entry, and whether it is in the Korean words (`korean`); no text (tests, the page's tests).
- `Engine::screen_frame()` / `Engine::screen_version()`: the published frame with spec 23's history view and the menu
  over it, without the pointer sprite, and its version (it moves when the published frame, the view or the drawn menu
  changes). The shown frame (`Engine::frame`) is it with the sprite; the HD frame is composed from it, keyed on its
  version (spec 23 Decision 12).
- `u6-web`: `Game::context_menu(x, y) -> String` (read only, for tests): `""` unless the engine's gate lets a right
  press after the page's queued events open one (`Engine::context_menu_gate`) and on the map or a slot; else JSON
  `{kind, cell, underlined, sections: [{object, kind, entries: [{code, enabled, reason, theft}]}], go_here: {code,
  enabled, reason, theft} | null}`, no names. `Game::game_buttons()`: the buttons the game's input handler has (what
  the engine let through; the page's tests), `Game::game_presses()`, `Game::game_keys()` and `Game::message_lines()`
  (counts, for the page's tests); `Game::published_rgba()`: the published frame as RGBA (the page's tests tell the
  game's own screen changing from what a closed menu left). `Game::set_menu_hidden(rects)`: x, y, w, h in turn,
  passed to the engine (kept for later starts; `Game::menu_hidden()` reads back the rects the engine holds, none
  without a game, for the page's tests); the page gives them before every step whenever they change
  (`hiddenFrameRects` in `site/app.js`: in every view the canvas's parts outside the viewport and under the badge
  and the Exit button through each region it shows; in Wide's game view also each window's frame and the map's parts
  off the canvas). `Game::dismiss_menu()`: the page's press off the canvas, queued behind the events waiting for the
  next step (`Engine::tick_dismissing`), at once when none wait. `Game::set_clock(now)` gives the engine the page's
  clock as its wall clock at each step. `Game::go_here_asked()`: Go here's travel requests, counted.
  `Game::menu_view() -> String`: `""` while closed, else JSON `{rect: [x, y, w, h], text: [x, y, w, h], hover,
  korean, colours: [paper, ink, heading], rows: [{kind, code, enabled, reason, theft, y}]}` (frame pixels, 0xRRGGBB
  colours for the page's pixel checks; `korean` true while the menu is in the Korean words, no words; codes only). `Game::command_at` and the page's run are gone.

**Predicates.** Each situational condition is a small function beside its handler citing the original's lines, with
a test that runs the real handler on a clone and checks enabled ⇔ the handler reached its action.

## Presentation (in-game, unit M3; replaces the deploy-30 page element)

The menu is drawn by the engine into the game's frame, in the game's own font, colours and window look; the page
draws nothing of it and has no element for it.

- **Where it is drawn.** Over the published frame, into a frame of its own (`Engine::screen_frame`, the published
  frame with the menu over it), which the shown frame and the HD frame are made from, the pointer sprite over it last.
  Never into the screen buffers: the original copies and scrolls rectangles of its screen, which would take the menu
  for game screen, and closing it would need the screen restored. Shown in Original, HD (through the compositor, like
  every engine frame: its glyph cells name their glyphs, so the HD font draws them) and Wide.
- **The look** (the original's text window): the text area is paper (A4-0x656c) with ink (A4-0x656a) glyphs drawn as
  0x105228 draws a cell (`ui::text::draw_char`), the game's `u6.ch` for codes below 0x100 and the Hangul table for a
  syllable. Round it, the frame of the paper's message scroll window: the colours of the paper bitmap (0x1350d8) on
  the row through the scroll's middle, from the pixel left of its first column outward up to the paper's own colour
  again (at most 6), drawn as one-pixel rings on all four sides; with no paper, one ring of ink. Section headings start
  in column 0 in the alert colour (A4-0x6576, the colour of the `@` keywords, 0x108376); entries start in column 1;
  the highlighted entry has glyph 0x1a in column 0, as the spellbook marks its current line (0x116218).
- **Disabled entries** are drawn in 0x134044's checker: of the label's ink pixels only those with `(x + y) & 1` = 0
  (the others paper). The original greys nothing; the checker is its only half-tone. Each cell is still named as its
  glyph, flagged half-toned (`u6_render::FLAG_HALFTONE` on its glyph sources, `HdGlyph::halftone`, `Game::hd_glyphs`
  code | 0x40000), so the HD font draws the label and the page then paints every second canvas pixel of the cell
  (`(x + y)` even) in the paper again: the same half-tone at the HD screen's pixel grid. Pixel mode shows the classic
  checker. A note row at the bottom (present when some entry has a reason) shows the highlighted entry's reason in
  ink; it is as wide as the reasons of the entries shown need, no wider. An entry's theft mark follows its label on
  its own row, in the alert colour, highlighted or not. With two or more sections the underlined command's entry
  (first) has its section's name after it, in the alert colour (its own heading comes later).
- **Words.** The port's own words, in English or, while the installed locale pack is Korean (`state.locale.pack`'s
  language, which `locale::install` sets: `ui::popup::korean`, the one reading of it for the layout, the words, the
  note, the theft mark and the relayout check), in Korean drawn with the game's Hangul glyphs: one table in
  `ui/popup.rs` (the page's copies in `site/i18n.js` are deleted). Not the text layer's `ui.text.language`: that is
  the language of the string being printed, which `puts`, `printf`, Look and the conversation set for one pack string
  and put back, English at rest; round 1 to 4 read it, so with the pack installed the live menu (deploy 37, ko-KR)
  drew English rows under a Korean section name (round 5, MUST 4). Section names are the engine's display names. Each row is cut to the text area's
  width, which only a frame too small for the widest row makes narrower than the rows ask.
- **Where it is placed.** Beside the pressed point: the text area's top left one cell right of and below the point's
  cell, flipped to the other side where it would not fit, then held inside the bounds of the region pressed, rings
  included: the shown map (`geom::SHOWN`), the side panel (click region 2), or a Wide slot (its rect; the inventory
  slot's doll part left of the slot's x 72, its list part right of it, both below its name row). Where that region
  cannot hold the widest row (a Wide inventory slot's 72-pixel part), the shown map instead, then the region as it
  can, then the whole frame. Text cells stay on the 8-pixel grid (the HD font draws a cell only there). Of the four
  places beside the press (right and below first, then left, then above, then both) the first whose box meets none
  of the frame rects the platform does not show (`Engine::set_context_menu_hidden`, given by the page from its layout
  before every step, in every layout: the frame's parts the canvas shows outside the viewport, as a phone held
  sideways scrolls the page past the canvas's bottom, and outside the visual viewport of a pinch-zoom, a canvas
  pixel partly outside counted outside; under the page's elements over the canvas, the speed badge and fullscreen's
  Exit button, a press on which leaves fullscreen: in fullscreen both at all times, shown or hidden, else each while
  shown; in Wide also its windows over the map and the map's parts off the canvas); with none such, the place in
  the bounds whose box meets the fewest of their pixels, the nearest to the first place among those. An open menu
  is laid out again (the same entry highlighted) when those rects change and when the language changes.
- **Input.** All of it goes through the engine's own input path (`Engine::tick`); nothing the menu takes reaches the
  game's queue, and the game's pointer sample is not moved. A right press opens it only at the command prompt
  (`engine::command_prompt`: A4-0x7a2c set, no cross-hair or panel cursor, the game accepting input), with nothing
  before it in the tick or queued for the game but events that change nothing it reads (a button-less move, a
  release, a platform's repeat of a direction key, which the prompt takes back; a paced press is a press). The
  pointer: a move over an entry highlights it; a left press on an entry that
  can run chooses it; a left press on a disabled entry, a heading, the note or the frame does nothing; a left press
  outside or any right press closes it, and so does a press of the platform's that reaches no frame pixel (off the
  canvas, on fullscreen's black sides; a press in the HD layout's gaps reaches the rest of the screen, outside the
  box), in order with the events queued before it: with some waiting for the next tick it closes the menu there,
  after them (`Engine::tick_dismissing`), else at once (`Engine::dismiss_context_menu`). With no menu open such a
  press only ends a double click (below): it is not a press on the game, so it is no new input and stops no travel;
  only closing an open menu makes it new input. Every press the menu takes is the menu's up to its release, and so
  is the opening press: a held right button moved over the map sends nothing. After a choice, the double click's
  second press is the menu's with its release: the choosing press's button again, within 500 ms of it on the
  player's clock and 4 frame pixels of it, no press of any kind (the pointer's, a key, typed text, the platform's
  off the frame) having reached the game between them. The player's clock is the platform's wall clock
  (`Engine::set_wall_clock`; the web page's animation-frame time), not the engine's: a double click is the player's
  gesture, and the engine's clock runs as fast as the game is stepped (8× makes 200 ms of the player's 1600 of the
  engine's, and Max more), so on it every double click at speed would leak its second press. Without a wall clock (a
  platform that gives none) it is the engine's, which is the player's at 1×; so a platform that runs the game faster
  than 1× gives `Engine::set_wall_clock` (the native build has speed presets but no menu today). So is every press
  while a chosen command's target press waits: neither becomes the game's press or the command's target. Keys: Up
  and Down move the highlight over the entries (wrapping, disabled ones included), Enter or Space chooses the
  highlighted entry when it can run, a command letter (A, B, C, D, G, L, M, R, T, U, either case) chooses its first
  entry that can run, Escape closes; every other key and typed text is taken and does nothing. Every key the menu
  took (a set: a held Down and the Escape that closed it both) keeps its repeats the menu's until that key's own
  release, open or closed, so no held key passes a turn. A platform's paced press of a held key (spec 19 O1c:
  `InputEvent::Paced`) is that key held, not a press: it never ends the menu's hold on the key nor counts as its own
  press, never moves the highlight and is no new input; a platform adds none for a key the menu holds
  (`Engine::held_press`). The pointer sprite while it is open follows the pointer the menu took, in the arrow shape;
  otherwise the game's own shape (the cross-hair of a target read stays while the choosing button is held).
  `Engine::pointer_at` is the pointer as the screen shows it: the menu's while it takes it, else the game's.
- **Not state.** The menu is not part of `State` or the savegame: presentation and input only. It closes when the
  prompt goes (A4-0x7a2c no longer 1 after a tick) and when the menu is turned off, which also forgets the pointer,
  the press and the keys it took.

## Trace (STV, written before the code)

### §0 Client surface (the page)
`canvas` `pointerdown`/`pointermove`/`pointerup` → `sendPointer` → `Game::pointer(x, y, buttons)` (a right press
included; no page handler opens anything) → `Game::pending` → `Game::step` → `Engine::tick(&pending)`. `keydown` →
`gameKey` → `Game::key_down`/`key_repeat` → `InputEvent::Key`/`Repeat`; `keyup` → `Game::key_up` → `Release`. Render:
`Game::present` → `Engine::frame` (classic) or `HdView::present` → `Engine::screen_frame` (HD) → canvas. The canvas's
`contextmenu` default stays suppressed.

### §1 Entry
`Engine::tick_dismissing(input, dismissals, ms)` (`engine.rs`; `Engine::tick` with none), the menu on: before anything
is queued, `Popup::input_with(&State, &Io, input, quiet, now, dismissals)` with `quiet` = every event in the game's
queue one the prompt takes back (`taken_back`; none at all elsewhere) and `now` the wall clock
(`Engine::set_wall_clock`, else the engine's).

### §2 Input and its checks
- The run held: `Run::step(state, quiet && input all inert)` (inert = a button-less pointer event, a key release, or
  at the command prompt a repeat of a direction key, which the prompt takes back; never a key, a paced press or text)
  → `Release` appends the target press to the game's events, `Drop` forgets it.
- A right press opens only if: the menu is closed, no run is held, `engine::command_prompt(state)` (A4-0x7a2c 1, no
  cross-hair or panel cursor, `ui.input.accept`), `quiet`, every earlier event of this tick's input inert,
  `menu::query(state, io, x, y)` is
  `Some`, and the laid-out menu has at least one entry (a query whose sections all came out empty opens nothing).
  Else the press is the game's.

### §3 Layer flow
- Open: `Pointer{x, y, 2}` → `query` → `Menu` → `Open::new(&menu, (x, y), &State, &hidden)` → rows (`Row::Head(cells)`,
  `Row::Entry(Item{code, object, reason, theft})`, `Row::Note`), the text area (`Open::text`, cells), the rings, the
  highlight → `Popup.open`; `version += 1`.
- Hover: `Pointer{x, y, _}` inside a row of an entry → `hover = row`; `version += 1` when it changed.
- Choose: `Pointer{x, y, 1}` (left newly down) on an enabled entry, or Enter/Space/letter → `Popup::choose(row)` →
  `Go here` → `Popup.go_here = cell` (the engine then `take_go_here` → the record, and `travel_to` when no menu is
  open after the input: a right press after the choice in the same input opened one, spec 21's merge contract); slot
  action → `menu::left_press(at)`; Rest/Combat → `menu::icon_press`; else `Run::start(state, code, at)` → icon press
  to the game's events, `Popup.run = Some(run)`. The menu closes (`version += 1`).
- Close: Escape, a right press, a left press outside → `open = None` (`version += 1`), the press swallowed to its
  release, the key kept in the taken set to its own release; `dismiss` (a press off the canvas, at its place among
  the tick's events) → the double click ended, and with a menu open `open = None` and new input; with none open it
  is no new input (not a press on the game, so no travel stops for it).
- Relay: new hidden rects (`set_hidden`) or a language change after the poll while open → `Open::relaid` (the same
  entry highlighted); `version += 1`, the engine drawing again at once (`Engine::redraw` → `Engine::compose`, the one
  composition of the screen and the shown frame a tick uses too, with the pointer sprite as it is then: a dismissed
  menu's arrow gives way to the game's pointer in that frame).
- Draw (after the poll, `Engine::compose`): when the game presented, the menu's version or the message history's
  view (its stamp, spec 23) moved, `layer.sync_from(published)`; `Engine::draw_view` (spec 23 Decision 3: the kept
  lines and the bar over the message window while scrolled back, nothing at the live bottom); open →
  `Open::draw(&mut layer, &State)` (the paper filled, the rings, `draw_char` per cell, `draw_halftone` for disabled
  labels, `draw_symbol` of glyph 0x1a at the highlight), `screen_version += 1`; `shown.sync_from(layer)`; the sprite
  (the arrow at the menu's pointer while the menu is open; the game's own shape at the menu's pointer while it still
  takes a press after closing) over it.

### §4 Side effects
The game's queue receives only the events the menu let through plus the run's presses. `State` is never written by
the menu. No savegame write.

### §5 Error paths
No query result → the press goes to the game. A run whose prompt came back → `Drop`, no target press. No font →
nothing drawn but the paper and the rings. No paper → one ring of ink. The prompt gone while open → closed after the
tick. Off → closed, every event the game's.

### §6 Output
The frame with the menu (`Engine::frame`, `screen_frame`), `Engine::context_menu()`'s view, the web JSON.

### File map
`crates/u6-core/src/ui/popup.rs` (new), `ui/mod.rs`, `ui/menu.rs` (Go here, Unreachable), `ui/pointer.rs` (a sprite of
a given shape), `engine.rs` (layer, input, run), `hd.rs` (composes the screen frame), `ui/tests.rs`,
`ui/tests/popup_tests.rs`, `ui/tests/popup_real.rs` (new); `crates/u6-web/src/lib.rs`; `site/app.js`,
`site/index.html`, `site/i18n.js`; `tools/web/harness.mjs`, `tools/web/test-web.mjs`. Also `ui/tests/real_tests.rs`
(a PNG of RGBA) and `objects/commands/real_data.rs` (`closed_door` shared). `re/notes/port-map.md` is unchanged: the
module is port-only.

## Replaced (2026-10-28)

The deploy-30 page menu (`#context-menu`, its styles, `openContextMenu` and its handlers in `site/app.js`, the
`cmd*`/`why*`/`theft` words in `site/i18n.js`, `Game::command_at` and the page-held run) is removed. Reason: the
owner's goal item 2 asked for a right-click menu in the game, and a game's right-click menu is the game's own UI; the
page element was a web page's menu laid over the game (owner, 2026-10-28, above). Migration: the right press goes to
the engine, which opens, draws and runs the menu; `Game::context_menu` stays read only without names, and
`Game::menu_view` reports the drawn menu.

What the page menu had that the engine's does not (recorded 2026-10-28, round 2):
- Home and End (first and last entry): not kept; Up and Down wrap.
- Any press outside it closed it: kept in the frame, and for presses of the page's that reach no frame pixel
  (`Game::dismiss_menu`).
- Screen-reader roles (`role="menu"`, `menuitem`, `aria-disabled`): lost. A mirror of the menu in the page would put
  the game's words back in the page; not done (dispatcher, round 2; reversible).
- Placement inside the visible part of the viewport: restored, by the hidden rects the page gives in every layout
  (Where it is placed).
- A text size that followed the phone's (CSS px): the menu is in the game's font at the frame's scale; on a small
  screen it is as small as the game's own text.

## Unit M4 wiring (spec 21, done in M3's round 4)

The menu does not start travel itself. Choosing Go here records the cell in the menu (`Popup::choose`), and the
engine hands it out in the same tick (`Popup::take_go_here`, `Engine::tick_dismissing`) to M4's
`Engine::travel_to(target: (u16, u16))`, which starts travel to the map cell (world x, y on the leader's level; the
cell is the `i16` pair of the menu's `cell`, M4's `u16` coordinates by an `as` cast, masked by M4 to the level), and
the engine keeps the cell in its record (`Engine::go_here_asked`, `ui::popup::Travel`; M4's accessor of the travel is
`Engine::travel_progress`). Order in a tick (spec 21's merge contract): the menu's filter first
(`Popup::input_with`); M4's `travel_press` sees only what passes it, plus `Popup::took_new_input()` as a stop (the
menu took an event that is not the tail of a gesture it already held; a press off the canvas is new only when it
closed an open menu; without the user's zip, `a_key_the_menu_takes_ends_a_travel_under_way_by_its_flag`); the chosen
Go here is applied after `travel_press`, so the new travel's first step is the next tick's, and only when no menu is
open after the input (round 5: one input of the choice, its release and a right press opening a new menu starts no
travel under it; spec 21's merge contract); M4's own step key is appended after the filter and never fed to it, nor
to spec 23's `View::input`, which the player's events of the filter's output go through next; then K2's take-back,
the rings, the queue. The tail of a choice is the menu's and not new: its press's held moves, a double click's press
and release, the repeats and release of a key taken while open; the press's own release reaches the game as a move
with no button, which ends no travel (`the_tail_of_a_choice_is_not_new_input`,
`a_double_click_on_an_entry_chooses_it_once_and_leaks_no_press`, and with the real travel `travel_tests.rs`'s three
`a_travel_the_menu_started_walks_through_…`, each against a control of as many ticks with no input since round 5: the
same cells walked and keys pressed at the same ticks, Go here chosen once, none of the tail the game's). Whether Go
here can run: M4's `travel::reachable(s, target)`; `ui::popup::go_here_reason` maps its `Reason` onto the menu's:
`Here`, `NoLeader` and `NoGame` hide the entry, `Aboard`, `Unreachable`, `Far`, `Combat` and `Foe` disable it with
that reason (en and ko words in `ui/popup.rs`; `Reason::ALL` holds them). The stubs (`travel_reason`, `Refusal`,
`popup::travel_to`) are gone. Pace: at the prompt a travel steps at most once a `MIN_STEP_MS` of the platform's clock
(spec 21 S3, `Engine::set_travel_due`, given by the page at every step).

## Acceptance

1. Right press on a closed door next to the leader → a menu with the door's name, Look and Use enabled (Attack last);
   choosing Use opens the door exactly as `U` then a left press on it does (same state after, same RNG; measured
   2026-10-28: all of `State` equal at the same clock but the keyboard's own target cursor, `ui.input.cursor` and
   `range`, which a command the pointer started never sets).
2. Right press on a door three cells away → Use shown, disabled, reason "far"; nothing is printed and no turn passes.
3. Right press on a villager on a chair → two sections: the villager (Talk, Look, Move, Cast, Attack; Cast is the
   cell's, as for every map selection), the chair (Get). The
   chair has no Use: its type (0xfc) is not among Use's 85, a condition of the kind, so the tier rule hides it
   (amended 2026-10-27 after M2 r3 measured the real chair; the first draft listed Use against its own rule).
4. Right press on an inventory object → the panel menu with its ready/open/take-off action, Look, Drop, Move, Use.
5. Cast without a book: the entry is disabled; if forced through the engine, the target press is dropped and the
   party does not walk.
6. Escape, a right press or a left press outside closes the menu and passes no turn; a held right button moving over
   the map sends no press to the game; the frame outside the menu is the frame without it.
7. Wide: a non-Look entry on a cell outside the 11 × 11 window is disabled, reason "window".
8. (M3) The menu is in the frame's pixels, in Original, HD and Wide; the page has no element for it; a left press on
   Look runs Look; the menu's pixels are paper and ink inside its text area and the frame's own outside its box.
9. (M3) Right press on bare ground → the tile's Look and Go here; choosing Go here calls `travel_to` with the cell,
   and the party walks there (round 4: the real travel, arriving on the cell).

Receipt on the user's data (`ui/tests/popup_real.rs`): each test prints `ran` or `skipped`; the release receipt is
`U6_REQUIRE_ARCHIVE=1 U6_FRAMES_DIR=<outside the repository> scripts/cargo.sh test -p u6-core --profile gate --lib
[--features wide] -- popup_real --nocapture` in both builds, which fails without the archive and writes the frames;
with `U6_KO_PACK=<the user's ko.u6lp>` the bare-ground receipt installs the user's Korean pack as the page does
(`Engine::install_locale`), else a Korean pack of no runs (the menu's words are the port's own either way; round 5).

## Not settled

- A Go here whose cell `Engine::travel_to` refuses at the moment of choosing (its `Err`, dropped by the engine's
  `let _ = self.travel_to(...)`) closes the menu with no sign: the entry was enabled by `travel::reachable` on the
  state as the menu opened, and the choice runs the travel's start again on the state then (the two differ only if
  the game changed its state while the prompt waited; not measured). Nothing tells the player why the party did not
  walk; the record (`go_here_asked`) still holds the cell. A sign would be the port's own text in the frame (a word in
  both languages and a place to show it), a presentation decision of its own: not settled, and no new text in round
  5 (dispatcher, 2026-10-29).
- The cost of one `State` clone per right press (estimate: a few MB, once per press, not per frame), and since
  round 4 Go here's `travel::reachable` on a map cell (spec 21's search: one more clone and at most two passes of
  `WINDOW²` expansions, once per press; not measured apart: the whole u6-web lib suite, the every-view-cell query
  sweep among its 88 tests, took about 2 s in the gate profile).
- Talk's script-presence test without loading the script: until an index lookup exists, the slot/type part decides
  and a scriptless NPC prints the original's refusal.
- Wide's page windows lie over the map: the page gives the engine the rects they hide and the menu keeps off them;
  when every place in the map meets one (the windows dragged over most of it), the menu takes the place least hidden.
  So does a viewport that shows too little of the frame to hold the menu.
