---
title: ultima6 — 22 conversation by click (Job and Name buttons, red keywords)
type: spec
tags: [ultima6, ui, input, converse]
---

# 22 — Conversation by click

Status: in progress (2026-10-28; built in the worktree, not merged). Round r1027-r2 unit T1 (worktree `feat/talk-clicks` from integ/r1027 60f92c6;
round 1 committed as 8f48620, round 2 on it: the reviews' findings). The message scrollback of the same message is spec 23
(unit T2).

Owner, 2026-10-28, verbatim: "그리고 대화시 나오는 사람패널 밑에 job 대화랑 name 대화 버튼 만들고 (인게임 ui로) 클릭시 해당
대사 입력 그리고 대화창에서 빨간색 키워드 클릭시 해당 대사 하도록해줘." The same message corrected that deploy 30's context
menu was a web-page element where it should have been in-game UI: everything here is drawn by the engine, in the game's
font and colours, and is the same in Original, HD and Wide. No DOM element.

## Measured facts (60f92c6)

- **The talk portrait.** A conversation draws its panel with 0x128ce8 (`inventory/portrait.rs:37-158`), from Talk's start
  (`converse/start.rs:225-228`) and op 0xbf (`converse/exec.rs:118-122`): the panel area cleared to the paper
  (`:100-102`, (168, 6)..(311, 103)), the portrait at (216, 24), or at (248, 24) beside the figure frame and the
  equipment (`:104-116`), the name centred on row 10 of the Status window (`:138-144`). The Status window is columns
  22..=38, rows 1..=12 (`ui/text.rs:98-100`), so its row 11 (screen row 12, pixels 176..311 × 96..103) stays blank paper
  under the name: measured on the user's files, Talk to party member 1 by the pointer, the row all paper colour (probe of
  this round, counts only). (The brief's `ui/panels.rs:575` portrait at (176, 16) is the stats view 0x110d2c, not the
  conversation's.) Op 0xbe draws the inventory panel there instead (`converse/seams.rs:148-158`), whose icon row
  covers y 88..103 at x 176..255 (spec 14 "Today").
- **The red keywords are the original's.** `@` starts a keyword in the alert colour A4-0x6576 that ends at a character
  of the keyword-end set (0x108376, `ui/printf.rs:286-306`); the port's `@{…}` (spec 08, with a pack) colours exactly the
  braced text the same way (`ui/print.rs:137-139`). Both only while the help mode A4-0x7a50 is on (`ui/tables.rs:222`,
  toggled by Ctrl-H, `main_loop/commands.rs:353`); the user's files start with it on (probe). 193 of the 200 scripts hold
  an `@`; 183 have a `job` keyword block and 185 a `name` one (probe, counts only). In the message window the alert ink
  has no other writer (`ui/panels.rs:427` is the Status window's).
- **Text drawing.** 40 × 25 cells of 8 × 8 (`ui/text.rs:1-7`); a cell is drawn by 0x105228 `draw_char`
  (`ui/text.rs:186-196`) in a window's ink and paper; the line assembler 0x107e5a draws a finished part of a line cell by
  cell with each cell's own ink (`ui/print.rs:224-232`), and scrolls the window with 0x10525c (`:270-277`); the line
  editor's echo 0x105d78 draws typed cells and scrolls too (`ui/keyread.rs:197-241`). The colours are A4-0x656a text
  (6), A4-0x656c paper (15), A4-0x6576 alert (12), A4-0x6572 command (9), the colour 0x106506 underlines the selected
  command icon with (`ui/cursor.rs:22-41`, `ui/tables.rs:16-27`; values from the user's files).
- **HD.** The HD font mode lists a cell only when its 64 pixels are one glyph drawn whole in one ink and one paper
  (`u6-render/src/hd.rs:17-22`); a glyph pixel over the paper shows the HD paper only where its colour is the paper's
  there (`:1116-1130`). A cell with an extra pixel row (an underline) is no longer listed and falls back to the pixel
  font.
- **Typing.** The player's words are read by the line editor 0x107cba (`ui/keyread.rs:318-412`) from ops f7 (after
  "you say", `converse/start.rs:342-349`), f9 (a string answer) and fb (a number), through `seams::read_line`
  (`converse/seams.rs:72-78`). It takes keys and text in order from the engine's queue and applies the pointer events in
  front of them (`ui/keyread.rs:344-347`, `engine.rs:303-308`); nothing in it reads the pointer.
- **The page** sends every pointer move, press and release on the canvas, buttons or none (`site/app.js:3906-3937`,
  through the HD and Wide layouts' map back to frame pixels), to `Game::pointer` (`crates/u6-web/src/lib.rs:1353-1360`),
  which queues `InputEvent::Pointer`. In Wide, while a conversation runs, the panel area shows in its own window
  (spec 14 Design 6) and the message scroll in 메시지.

## Contract

1. **When.** Only while the conversation reads the player's words at op f7 (the "you say" prompt). The answers of f9
   and fb, every other read and every other moment are unchanged: no button, and a press on a keyword does nothing.
2. **The buttons.** Two, Job then Name, on row 11 of the Status window, below the portrait and its name, inside the
   panel area. Each is its word between two blank cells, drawn with 0x105228 in the game's font: the word in the paper
   colour on cells of the command colour A4-0x6572 (the colour of 0x106506's mark), a key cap. The pair is centred in
   the window's 17 cells, one blank cell between them; were they wider than the row, the blank cells at their ends go,
   and were they wider still, there are none (no cell is ever drawn outside the row). Main's row never takes the last
   step: two words of at most 8 cells and the cell between fill its 17 exactly; only a narrower window would. They are
   drawn when the read begins, only if every pixel of that row is the paper colour then (the portrait panel leaves it
   so; the inventory panel of op 0xbe does not, and then there are no buttons for that read), and the row is filled with
   the paper again (0x1521f4 with the paper, as the portrait panel's clear) when it ends, so the frame after the read is
   the frame without the feature.
3. **The buttons' words.** English: the script's keywords `job` and `name`. With a locale pack: the first word the pack
   gives that keyword of the script loaded (`locale::Pack::aliases`), else the English. Each is typed as it is and shown
   with its first letter a capital when that is a lower-case ASCII letter (so the identity pack, whose alias of a keyword
   is the keyword, shows what English shows); a word is shown in at most 8 cells. Each word is the one the script
   matches: an alias matches through `keywords()` (spec 08 Design 4), the English keyword always.
4. **The keywords.** The cells the line assembler draws in the message window in the alert colour are recorded as spans
   at the moment they are drawn: their window row and first column, their cell codes and ink. Consecutive alert cells on
   a row are one span; a space joins a span but starts none, and a span's word ends at its last other cell (the more
   prompt blanks its row in the ink of a keyword it broke). Every cell the assembler draws there first forgets the span
   under it, whatever its ink (`KeySpans::draw` does both, so no caller can record without forgetting); the more prompt's
   blank row, the echo and the key wait's cursor forget too. A scroll moves every span up a row and hands the spans of
   the row that leaves to its caller (spec 23 keeps them with its history), and moves the hovered and the pressed keyword
   with them (Contract 6). Setting the window up again forgets them all; that happens once, at the boot (`main_windows`,
   `ui/textstate.rs:175-178`, from `main_loop.rs:403`): a conversation's start does not set it up again, so spans of
   earlier text stay targets while their cells show. A keyword the word wrap breaks is one keyword: when the assembler
   wraps a row with its last drawn cell in the alert colour and what follows the break in it too (the space it drops
   there, or the next cell; inside `@{…}` a space is in the alert colour, after an `@` keyword it is not), the span the
   next row starts with at column 0 is that keyword's rest (the first cell drawn there takes the wrap, a blank aside, so
   a more prompt's blank row between keeps it), and its word is both parts' cells with the dropped cells between. A
   span's word is its cells as typed: ASCII as itself (a runic glyph as its letter), a Hangul syllable as its character;
   any other cell is left out.
5. **A click.** A left click on a button or on a keyword (the left button down on it and up again on it; a keyword's two
   parts are one target), while 1 holds, types its word as the keyboard would: one Backspace for each cell already
   typed, the word's characters, Enter, put at the front of the engine's queue at the release, so the line editor takes
   them next through the keymap, the echo and its budget exactly as typed keys. The word goes at the release, as a
   button of any interface does it: a press is taken back by moving off the target before letting go, and then nothing
   is typed. (Acting at the press would type before the player could change their mind; the release that came after it
   would find nothing to do.) A word that types nothing (a keyword of cells no key types) is no click: nothing is
   typed, not even Enter. A release elsewhere than its press, a press anywhere else, a right press and a move do what
   they did (nothing); so does a press the browser cancels, which the page sends as a release at frame pixel (-1, -1),
   which the engine's pointer clamps to the frame's corner (0, 0) (`engine.rs` `Pointer::apply`), off every target
   (Contract 8).
   - Timing: in the f7 read every pointer event at the front of the queue is taken in the same pass of the editor's
     wait, one press or release a take (`Io::take_pointer`), the hover drawn after each; elsewhere the original's event
     wait takes one a pass. So presses and releases queued together, on targets or off them, are all taken before the
     wait's next frame, and a press and its release queued together type the word in that pass and leave no pointer
     event behind it.
   - Positions: a take merges the moves after a press or a release that keep its buttons, so the press (or release) is
     hit-tested where the last of those moves left the pointer, not where its own event was.
   - The injected keys go in front of the queue without the input handler's ring limit (`KEY_RING`, 49 keys, applied
     to what `Engine::tick` queues): harmless, as the editor takes them all in that pass and its budget bounds the line
     as for typed keys.
6. **Hover.** While 1 holds, the button or keyword under the pointer is drawn inverse: its ink and paper swapped (a key
   cap shows its word in the command colour on the paper; a keyword, all its parts, shows in the paper colour on alert
   cells). Its cells stay whole glyphs, so the HD font lists them. Each pass of the wait hit-tests the pointer where it
   is, so a pointer resting over a button when the read begins shows it inverse at once. The hover goes before each
   key the editor takes and when the read ends. The one scroll that can come while a keyword is inverse, the line
   editor's entry put (`ui/keyread.rs` `idle_wait`: a prompt that filled its row puts its line out first), moves the
   inverse cells, and the hover and the press move up with them (`talk::scroll`), so the cells are drawn back where they
   went.
7. **Input only.** Nothing is saved. Besides what typing the word causes, the state keeps only the span record
   (presentation, as the transcript) and, during a read, the buttons and the hover, both empty again when it ends.
8. **Wide and HD.** The same pixels in the same frame places; the page shows them through its windows and the HD
   compositor; the pointer reaches them through the page's map. The page's one change: a cancelled press
   (`pointercancel`: a touch the browser takes for a scroll or a zoom) is released at frame pixel (-1, -1), the frame's
   corner once the engine clamps it, instead of where it was cancelled. The release `sendPointer` queued moves there
   (`Game::pointer` merges a pointer event into the one queued before it with the same buttons,
   `crates/u6-web/src/lib.rs` `queue_pointer`, pinned by its test), so the engine sees no release over the target.
   Chosen over `touch-action: none` on the canvas: that would change the page for every touch user (no scroll or zoom
   started on the canvas) and still leave the cancels the browser makes for other reasons. Elsewhere in the game a
   cancelled press now ends with a release at that corner: in the Original layout that is where a release off the
   canvas's top left lands (`pointerPixel` gives negative pixels there, the engine clamps them), and a Wide title-strip
   drag, whose releases `pointerPixel` sends at the layout's rest point `WIDE_REST`, now ends with the game's pointer at
   the corner instead. What the game makes of a release at (0, 0) during a drag of its own was not traced for this
   change.

## Trace: a click on Name (the same for Job and a keyword)

0. **Client surface.** `site/app.js` `pointerdown` → `sendPointer` → `pointerPixel` (Original: the canvas box onto the
   frame; HD and Wide: `layoutToGame`) → `game.pointer(x, y, 1)`; the release `game.pointer(x, y, 0)`. Moves without a
   button go the same way (hover). No DOM element is drawn or read.
1. **Entry.** `Game::pointer` (`crates/u6-web/src/lib.rs:1356`) → `queue_pointer` → `InputEvent::Pointer {x, y,
   buttons}` → `Engine::tick` queues it (`engine.rs:776-806`) and polls the game.
2. **Input.** Frame pixel (x, y), buttons bit 0. Valid only in the f7 read (Contract 1), as a click (bit 0 set, then
   clear, both inside the same target: a button's cells on screen row 12, or a span's cells in the message window).
3. **Layer flow.**
   - 3a `converse::start::input_loop` op f7 → `seams::read_said` → `keyread::read_said` → `edit_line(talk = true)`:
     `talk::begin` draws the buttons (Contract 2) into the panel area and records them.
   - 3b Each pass of the editor's wait, while a pointer event is in front (`Io::pointer_ready`): `talk::pointer` →
     `Io::pointer` (buttons before) → `Io::take_pointer` (one press or release at most) → `talk::target_at(x, y)`:
     `x / 8, y / 8` → screen cell → a button (row 12, its columns) or a span (`TextState.keys`, the message window's row
     and column) → `talk::hover` draws the change → a press keeps its target (`TalkClicks.pressed`); the release on the
     same target → `talk::keys(typed, word)` → `Io::push_front`.
   - 3c Arrows: release pixel → screen cell → `Target::Button(i)` / `Target::Key {row, col}` → `Button.word` /
     `KeySpans::word` (its parts, UTF-8) → `[Key::Backspace × cells typed, Key::Char(c) | Key::Space |
     InputEvent::Text(syllable), Key::Enter]` → `poll` → `input::key_code` (the keymap) → `edit_line`'s line → `echo` →
     `converse.input` → `keywords()` (`converse/run.rs:236-301`) matches it as typed.
   - 3d The record: `print::end_line` → `print::cell` → `KeySpans::draw` (forgets the cell's span, then records an
     alert cell); a wrap inside a keyword → `KeySpans::wrapped`; `end_line`'s and `echo`'s scroll → `talk::scroll` →
     `KeySpans::scroll` and the hover's and the press's rows.
4. **Side effects.** The panel area's row 11 (drawn at the read's start, filled with the paper at its end); a target's
   cells drawn inverse and back; `TextState.keys`; `UiState.talk` (buttons, hover) during the read; the injected keys.
   No savegame field, no RNG draw, no turn beyond what the typed keys cause.
5. **Error paths.** No target under a press: nothing. Not the f7 read: the old loop (`Io::take_pointer`) runs. Row 11
   not blank, or too narrow: no buttons (keywords still work). A span scrolled off or drawn over: gone, no target (a
   wrapped keyword whose first part left the window types its rest). A word that types nothing: no click. A word longer
   than the editor's budget: cut as typing it would be. A cancelled press: a release off every target.
6. **Output.** The frame: the echo of the word, the answer the script gives it, the buttons gone; the same frame and
   state as typing the word (and moving the pointer to the same place) gives.
7. **Observability.** Tests print counts and pass/fail only.

## Tests

- Engine (`converse/tests_talk.rs`, synthetic script, made-up words, no assets; Backspace mapped to 0x10e as the user's
  keymap maps it): the buttons are drawn only in the f7 read (pixels during it, the row paper again after; none in f9
  and fb; none over a row not all paper); they fit the row with or without their blank cells or are not drawn, never a
  cell outside it (narrowed windows, a pack's 8-syllable alias); in f9 and fb a click where Name is in f7 and a click on
  a shown keyword leave the state and frame of the same moves without the clicks, and the read takes exactly what is
  typed; a press on Job and on Name gives the same state and frame as typing the word + Enter (two runs from one
  state); a press on Name with a pack gives its alias; a press moved off before its release types nothing and the read
  goes on; a press and its release ticks apart type; after an ASCII letter and a Hangul syllable typed with a pack, a
  click on Name (a Hangul alias) and on a keyword each give the state and frame of two Backspaces, the word and Enter
  built by hand; a right click types nothing; a keyword of cells no key types is no click; the hit test maps a press
  inside a printed span (`@word`, and `@{…}` with a space) to its word and beside it to nothing; a scroll moves it up a
  row and the scroll that takes its row away forgets it; typing past the last row moves a keyword up a row with its
  cells, and a click there types it with the letters taken back; the more prompt's blank row and the echo (typed or
  Backspace) forget a keyword they draw over; a keyword hovered before the entry put's scroll is drawn back where it
  went; a braced keyword the wrap breaks is one target, inverse whole, typed whole from either part, and `@a`/`@b` on
  the two sides of a wrap stay two; the hover inverts a span's and a button's cells and puts them back. Units
  (`ui/talk.rs`): spans join, forget whole, leave with the top row; a cell of any ink drawn over a keyword forgets it
  and a keyword cell there starts a fresh one; a wrapped keyword's parts are one word and one target, a blank at
  column 0 keeps the wrap, a cell of another ink spends it; a span's word (ASCII, runic, Hangul, the rest left out)
  and the keys of a word. Web (`crates/u6-web/src/lib.rs`): a release queued again at once replaces the one before it.
- Real data (`ui/tests/pointer_real.rs`, the user's zip; counts and equality only; it prints `ran` or `skipped`): Talk
  to member 1 by the pointer, press Name, type a letter and press Job (the press takes the letter back with the keymap's
  Backspace), press a keyword of the answers; the same run with the pointer moved to the same places and the keys typed
  in one tick ends each answer with equal words, scroll, frame and state. At each read after the first the two buttons
  are back and their row (the frame without the pointer sprite) is the first read's, pixel for pixel: the row the
  read's end filled with the paper on the real paper map is all paper again. Frames of the buttons and a hovered
  keyword, Original and Wide, outside the repo (`U6_TALK_FRAMES`).
- Browser (`tools/web/test-web.mjs`, the user's disks, a made-up conversation in place of the user's): the Name button
  drawn in the frame; a touch pressed on it that the browser cancels types nothing and the words are still read; a
  press on it in the canvas sends no key and ends the conversation with the same message-window pixels as typing
  `name` + Enter, every cell but the prompt's turning cursor (the one cell that changes from frame to frame there,
  found in each run, the same in both), Original and Wide (frames with `U6_FRAMES_DIR`).
- RED: each test fails with its part of the feature removed.

## Files

`crates/u6-core/src/ui/talk.rs` (new), `ui/mod.rs`, `ui/textstate.rs`, `ui/print.rs`, `ui/keyread.rs`, `engine.rs`
(`Io::push_front`), `converse/seams.rs`, `converse/start.rs`, `converse/mod.rs`, `converse/tests_talk.rs` (new),
`ui/tests/pointer_real.rs`, `crates/u6-web/src/lib.rs` (a test), `site/app.js` (`pointercancel`),
`tools/web/test-web.mjs`, this file. (`re/notes/port-map.md` regenerates unchanged: every
new function is port-only.)

## Trace deviations

- The engine tests live in `converse/tests_talk.rs`, not `ui/tests/talk_tests.rs`: they run conversations, whose
  harness is the converse module's.
- Contract 5 (a design choice against the brief's "left press"): the word is typed at the release of a press on the same
  target, a click (the owner's 클릭시), as a button of any interface acts: a press can be taken back by moving off before
  letting go. (Round 1 gave another reason, that no release is left behind the keys; that holds, but a release left
  behind would do nothing either, so it is no reason. The difference the real-data receipt showed on the way was the
  test's own: one run had a pointer move the other did not.)
- Merge with spec 23 (unit T2): its round 2 swallows a press inside the message window while the history is scrolled
  back (it snaps to the live view and the press goes no further), so a press there never reaches Contract 5 and types
  the live keyword under that pixel. Keywords in the scrolled history become targets at that merge (spec 23 keeps the
  spans the scroll hands back), not here. The two specs change `print::end_line`'s and `keyread::echo`'s scroll and
  the line editor's wait: textual conflicts expected there.
- Contract 3's shown word: capitalised for a pack's ASCII alias too (the identity pack's real-data check,
  `converse/disasm/tests_real.rs`, needs its frames to be English's).
