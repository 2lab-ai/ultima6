---
title: ultima6 — 22 conversation by click (Job and Name buttons, red keywords)
type: spec
tags: [ultima6, ui, input, converse]
---

# 22 — Conversation by click

Status: in progress (2026-10-28; built in the worktree, not merged). Round r1027-r2 unit T1 (worktree `feat/talk-clicks` from integ/r1027 60f92c6). The
message scrollback of the same message is spec 23 (unit T2).

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
   colour on cells of the command colour A4-0x6572 (the colour of 0x106506's mark), a key cap. The pair is centred in the
   window's 17 cells, one blank cell between them; were they wider than the row, the blank cells at their ends go. They
   are drawn when the read begins, only if every pixel of that row is the paper colour then (the portrait panel leaves it
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
   prompt blanks its row in the ink of a keyword it broke). A cell drawn over (the assembler, the more prompt's blank
   row, the echo, the key wait's cursor) forgets the span under it; a scroll moves every span up a row and hands the
   spans of the row that leaves to its caller (spec 23 keeps them with its history); opening the window again forgets
   them all. A span's word is its cells as typed: ASCII as itself (a runic glyph as its letter), a Hangul syllable as
   its character; any other cell is left out.
5. **A click.** A left click on a button or on a span (the left button down on it and up again on it), while 1 holds,
   types its word as the keyboard would: one Backspace for each cell already typed, the word's characters, Enter, put
   at the front of the engine's queue at the release, so the line editor takes them next through the keymap, the echo
   and its budget exactly as typed keys. The pointer events in front of the queue are taken in order in one pass of the
   editor's wait (each still one press or release a take), so a press and its release queued together type the word in
   that pass and leave no pointer event behind it: the reads after see what typing leaves. A release elsewhere than its
   press, a press anywhere else, a right press and a move do what they did (nothing).
6. **Hover.** While 1 holds, the button or span under the pointer is drawn inverse: its ink and paper swapped (a key cap
   shows its word in the command colour on the paper; a keyword shows in the paper colour on alert cells). Its cells
   stay whole glyphs, so the HD font lists them. The hover goes before each key the editor takes and when the read ends,
   so nothing moves while a cell is inverse.
7. **Input only.** Nothing is saved. Besides what typing the word causes, the state keeps only the span record
   (presentation, as the transcript) and, during a read, the buttons and the hover, both empty again when it ends.
8. **Wide and HD.** The same pixels in the same frame places; the page shows them through its windows and the HD
   compositor; the pointer reaches them through the page's map, which needs no change.

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
     `KeySpan::word()` (UTF-8) → `[Key::Backspace × cells typed, Key::Char(c) | Key::Space | InputEvent::Text(syllable),
     Key::Enter]` → `poll` → `input::key_code` (the keymap) → `edit_line`'s line → `echo` → `converse.input` → `keywords()`
     (`converse/run.rs:236-301`) matches it as typed.
   - 3d The record: `print::end_line` → `print::cell` forgets the cell's span, then records an alert cell
     (`KeySpans::draw`); `end_line`'s and `echo`'s scroll → `KeySpans::scroll`.
4. **Side effects.** The panel area's row 11 (drawn at the read's start, filled with the paper at its end); a target's
   cells drawn inverse and back; `TextState.keys`; `UiState.talk` (buttons, hover) during the read; the injected keys.
   No savegame field, no RNG draw, no turn beyond what the typed keys cause.
5. **Error paths.** No target under a press: nothing. Not the f7 read: the old loop (`Io::take_pointer`) runs. Row 11
   not blank: no buttons (keywords still work). A span scrolled off or drawn over: gone, no target. A word longer than
   the editor's budget: cut as typing it would be.
6. **Output.** The frame: the echo of the word, the answer the script gives it, the buttons gone; the same frame and
   state as typing the word (and moving the pointer to the same place) gives.
7. **Observability.** Tests print counts and pass/fail only.

## Tests

- Engine (`converse/tests_talk.rs`, synthetic script, made-up words): the buttons are drawn only in the f7 read
  (pixels during it, the row paper again after; none in f9 and fb, where a press types nothing; none over a row not all
  paper); a press on Job and on Name gives the same state and frame as typing the word + Enter (two runs from one
  state); a press on Name with a pack gives its alias, the same as typing it; the hit test maps a press inside a printed
  span (`@word`, and `@{…}` with a space) to its word and beside it to nothing, a scroll moves it up a row and the scroll
  that takes its row away forgets it; the hover inverts a span's and a button's cells and puts them back. Units
  (`ui/talk.rs`): spans join, forget whole, leave with the top row; a span's word (ASCII, runic, Hangul, the rest left
  out) and the keys of a word.
- Real data (`ui/tests/pointer_real.rs`, the user's zip; counts and equality only): Talk to member 1 by the pointer,
  press Name, type a letter and press Job (the press takes the letter back with the keymap's Backspace), press a keyword
  of the answers; the same run with the pointer moved to the same places and the keys typed in one tick ends each answer
  with equal words, scroll, frame and state. Frames of the buttons and a hovered keyword, Original and Wide, outside the
  repo (`U6_TALK_FRAMES`).
- Browser (`tools/web/test-web.mjs`, the user's disks, a made-up conversation in place of the user's): the Name button
  drawn in the frame; a press on it in the canvas sends no key and ends the conversation with the same message-window
  pixels as typing `name` + Enter, Original and Wide (frames with `U6_FRAMES_DIR`).
- RED: each test fails with its part of the feature removed.

## Files

`crates/u6-core/src/ui/talk.rs` (new), `ui/mod.rs`, `ui/textstate.rs`, `ui/print.rs`, `ui/keyread.rs`, `engine.rs`
(`Io::push_front`), `converse/seams.rs`, `converse/start.rs`, `converse/mod.rs`, `converse/tests_talk.rs` (new),
`ui/tests/pointer_real.rs`, `tools/web/test-web.mjs`, this file. (`re/notes/port-map.md` regenerates unchanged: every
new function is port-only.)

## Trace deviations

- The engine tests live in `converse/tests_talk.rs`, not `ui/tests/talk_tests.rs`: they run conversations, whose
  harness is the converse module's.
- Contract 5 (a design choice against the brief's "left press"): the word is typed at the release of a press on the same
  target, a click (the owner's 클릭시), so a press released elsewhere types nothing, and the injected keys are the last of
  the click: no release is left queued behind them for the reads after (a more prompt's key wait among them) to take as
  something more than typing. (The difference the real-data receipt showed on the way was the test's own: one run had a
  pointer move the other did not.)
- Contract 3's shown word: capitalised for a pack's ASCII alias too (the identity pack's real-data check,
  `converse/disasm/tests_real.rs`, needs its frames to be English's).
