---
title: ultima6 — 23 Message scrollback (the wheel shows past lines, a scrollbar on the right)
type: spec
tags: [ultima6, ui, hd, wide, input]
---

# 23 — Message scrollback

Status: implemented, not merged (2026-10-28, unit T2 / D2, branch `feat/message-scroll` from integ/r1027 60f92c6; round 1 06b1ab0, round 2 on top of it). The same owner message asked for the Job and Name buttons and the clickable keywords: that is unit T1 (spec 22, branch `feat/talk-clicks`, D1 8f48620). D2 merges after D1 (decision 12).

Owner's words (2026-10-28, verbatim): "그리고 메세지창에서 스크롤로 지난 대화 볼수 있게 해줘 오른족에 스크롤바도 추가해줘". In the same message the owner corrected that UI must be the game's own (in-game), not web-page elements (the brief's summary): everything here is drawn by the engine into the frame, in the game's colours; the page adds no element.

## Clause map

| Owner, verbatim | Acceptance |
|---|---|
| "메세지창에서 스크롤로 지난 대화 볼수 있게 해줘" | S1: after a conversation long enough to scroll, the wheel over the message window shows the lines that scrolled off, each as it looked before it scrolled off (real data: pixel equality of every line seen, counts only); the wheel elsewhere does nothing |
| "오른족에 스크롤바도 추가해줘" | S2: while the view is scrolled back, a scrollbar is drawn by the engine at the window's right edge: a track and a thumb whose length is the visible fraction and whose place is the fraction of lines above; dragging the thumb and pressing the track scroll |
| (the original stays the original) | S3: with no history view the frame is byte-identical to the frame without the feature, in Original and Wide, with and without the source plane: the shown, published and HD frames of a scripted run and of three real conversations equal digests recorded at 60f92c6 |
| (the correction: in-game UI) | S4: no DOM element; the page only passes the wheel and the pointer to the engine |

## Measured facts (60f92c6)

- The message window is the record `WindowId::Messages`, cells 22..=38 × 14..=23 (`ui/textstate.rs:170`), so pixels 176..=311 × 112..=191, in both builds (spec 13: the UI keeps its 320 × 200 coordinates; the page crops exactly this rect, `site/app.js:1588` `messages: from [176, 112, 136, 80]`, and Wide's 메시지 window shows the same region, `site/app.js:2127`).
- Text reaches the window's cells at five draw sites, all through `text::draw_cell` (`ui/text.rs:276`): `print::cell` (`ui/print.rs:29-35`: the assembled line, the centre padding, the more prompt's blank row), the key wait's cursor symbol and its erase (`ui/keyread.rs:149`, `:179`), the line editor's Backspace blank and its typed echo (`ui/keyread.rs:223`, `:225`).
- The window scrolls at two sites, both `text::scroll_window` of the current window (`ui/print.rs:276`, `ui/keyread.rs:235`; `ui/text.rs:446`): the top row moves out of the rect and is lost (0x10525c), the bottom row is filled with the window's paper.
- The paper under the rect, as the boot draws it, is the window's paper colour on 10 878 of its 10 880 pixels (probe, counts only).
- `u6.ch` inks the rightmost pixel column of 13 of its 95 printable glyphs and the next one of 76; the site's Hangul table never inks the rightmost column (probe, counts only).
- In a real conversation run (12 NPCs near the start, name / job / bye), 42 of 477 window rows hold text in their 17th cell (probe, counts only). A scrollbar that took the last cell column would hide a letter on those rows.
- The game's own scroll indicators are the arrow glyphs 0x18 / 0x19 printed in the inventory list (`inventory/panel.rs:302-305`), one cell each; its 1-pixel ink rules on the paper are the command underline (`ui/cursor.rs:33`, `:39`).
- The frame the player sees is composed by the engine: the published frame (`Io::published`, synced from the screen at a present) and the pointer sprite over it into `Engine::shown` (`engine.rs:854-860`; the source-only publish `:861-883`). The HD frame is composed from the published frame without the sprite (`hd.rs:150-169`). Game code reads the published frame (`combat/present.rs:376`), so the view is never drawn into it.

## Decisions (reversible; the owner vetoes on review)

1. **The history is cells, kept as the game draws them.** A shadow of the message window's rows (`ui::scrollback::Scrollback`, in `UiState`) records each cell a draw site writes inside the rect: code, ink, paper and kind (character, typed, symbol). When the message window scrolls, its top row goes to the history before the move, with the pixels of the screen row the cells do not reproduce (a colour patch; it makes every kept line colour-exact whatever drew there; rare: in the real-data receipt 36 of 1058 kept lines carry one, 2 pixels each). At most **500 lines**, oldest dropped first, each shared (`Rc<Line>`): a copy of the state, and the view's lines (2), copy a pointer a line. The memory is decision 11. It is presentation only: never saved, kept across a Restore, the same in every build.
2. **The view is the engine's, not the game's.** The offset (lines back from the live bottom, 0 = live, at most the lines it holds), the gesture and the kept lines as of the last publish (the shared lines, taken at each publish where lines were kept since the one before) live in `Engine` (`View`), not in `State`: viewing changes no state the game reads. Between publishes the view draws its own lines, so lines kept or dropped after the published frame do not move it (a source-only publish at the bound included). Another history at a publish (a new game's state: the view's newest line is not in it where the count says) returns the view to the live bottom. At offset 0 nothing is drawn, so the frame is the original's by construction (S3).
3. **The view is drawn between the published frame and the pointer sprite**, into `Engine::shown`, after its sync and before the sprite; the next sync puts back what it drew (the overlay's writes mark their chunks). For HD a second frame is built only while the offset is not 0, on demand: the published frame copied and the view drawn over it (`Engine::with_view_frame`); at offset 0 HD composes the published frame as before.
4. **The wheel.** `Engine::wheel(x, y, dy)`, outside the game's input queue (no new `InputEvent`): `dy` < 0 shows older lines, by |dy| lines; > 0 newer; 0 moves nothing and answers whether the wheel is taken there. It acts only over the message window's pixel rect, only while the program is `game`, and clamps to the lines kept. The page: one notch is one line (a line- or page-mode event is one line in its direction; pixel deltas, a trackpad's, add up, 100 CSS pixels a line). The carry takes only events the engine takes (the page asks with `dy` 0 first, and cancels only those), and is dropped when the direction turns, at a wheel the engine does not take, when the pointer leaves the message window or the canvas, and at a key; Ctrl-wheel is the browser's zoom. Over a window in front of the message window, a window's frame or a gap, the page sends the rest pixel (`layoutToGame` / `wideToGame` with `held`), which is outside the rect. Whole-screen frames: in `game` none covers the rect in the port (2026-10-28): the defeat (`actors/turn.rs:95-104`, `darkness`) prints its lines in the message window over a full redraw of the usual screen, and the end sequence (the launcher's `end`) is not ported (`main_loop.rs:246`); the gate is the program and the rect.
5. **New text while scrolled back does not move the view.** At each publish the offset grows by the lines that scrolled off since the last one, so the same lines stay in view; only the thumb moves (the history grew). The thumb above the bottom is the sign that newer lines follow. The view leaves the history only by the player (6) or the wheel and the bar.
6. **Leaving the history.** In `Engine::tick`, before it is queued for the game: a key press, a key repeat or typed text, and a pointer press (either button) anywhere but on the scrollbar, set the offset to 0 in that tick. A move without a button, a release and the keys the page keeps for itself (the speed keys) do not. A press inside the message window off the scrollbar while scrolled back (no button held before it) returns to the live bottom and is not the game's (the dispatcher's decision, 2026-10-28: a click on the history must not act on the live window; the keywords in the history become clickable with D1's merge, 10). A press with a button already held (the game's, a walk) is the game's whatever it lands on: the game sees every change of the buttons it holds.
7. **The scrollbar.** Drawn only while the offset is not 0, inside the window's last cell column (Original x 304..=311), over the window's 80 rows:
   - the track: a dotted 1-pixel line at the column's x 6 (Original x 310), every other row, in the text ink;
   - the thumb: 3 pixels wide at the column's x 5..=7 (Original x 309..=311), solid, in the text ink; its length is the visible fraction of the 80-pixel track, rows / (history + rows), at least 8 pixels; its top is (80 − length) × (lines above the view) / (history);
   - both are drawn only in the column's cells that show one colour (no letter, whatever the paper under them is): a cell that shows a letter keeps it whole, and the bar yields to it.

   Why thin and yielding: a scrollbar in its own cell column would hide the 17th cell, which holds text on about one row in eleven (facts); the arrow glyphs need a cell each. Why it yields to a letter cell instead of drawing on its paper pixels: in the HD font mode a cell is drawn in the HD font only when all 64 of its pixels are its glyph's (`u6-render/src/hd.rs:679-715`); a bar pixel in it would turn that letter back to the pixel font. The ink rule matches the game's own 1-pixel rules on the paper.
8. **The text area does not narrow.** The game wraps in `end_line` at its window's width, 17 (`ui/print.rs:200-216`), and the view never touches the window record or the line: live text always wraps as the original's. The view shows kept lines as they were wrapped.
9. **The bar's gestures.** The bar's hit area is the last cell column over the window's rows, only while it is drawn. A gesture starts only at a press with no button held before it (6). A left press on the thumb grabs it: moves with the button held set the thumb's top, and the offset follows (the inverse of 7); a left press on the track above the thumb shows a window of rows (10) older, below it 10 newer, at most to the live bottom. The gesture lasts until every button is up, wherever the pointer goes; a key, a repeat or text during it returns to the live bottom and ends the drag, and the press stays the view's until the buttons are up. Its events still reach the game, with no button held: the pointer sprite follows the pointer (it reads the queued positions, `engine.rs:217`), and the game sees moves only. While a gesture lasts, the platforms pace the steps as with a button held (`Engine::history_holding`, spec 06's 1×).
10. **Keyword spans: D1's.** D2 keeps no spans (round 2 deleted its alert-ink runs: they counted space cells in the alert ink, which the more prompt's blank row draws, and kept fragments of overdrawn keywords, against spec 22 Contract 4). D1 (8f48620, spec 22) brings its own spans; at D1's merge its record of a row (`KeySpans::scroll()`, the lines D1 also edits: `ui/print.rs:31-38`, `:275-282`, `ui/keyread.rs:150-151`, `:181-182`, `:226-230`, `:239-242`, `ui/mod.rs:140-143`) is handed to `scrolled()` and kept with the line, and a press on a kept span while scrolled back is D1's to route; until then a press on the history returns to live (6).
11. **Memory.** A line of the window's 17 cells holds 102 bytes of cells, its patch 4 bytes a pixel (made to its length), and a shared header of 72 bytes. Typical: 500 lines with no patch, 92 676 bytes (`history_keeps_…`); the real-data receipt's largest history, 17 750 bytes. Worst: every pixel of every line patch (a row the game filled in a colour the cells do not draw), 1088 pixels a line, 2 268 676 bytes at the bound (`a_bound_of_lines_all_patch_keeps_the_stated_size`, measured with `Scrollback::heap_bytes`). The bound is kept as stated, not capped: the patch is what makes a kept line colour-exact, and the lines are shared, so neither a state copy nor the view copies them. Patch sources (a limitation, stated): a patch pixel is drawn back in its colour alone, naming the paper at its new place as `scroll_rect` renames a scrolled pixel (`Paper::over`) and no image elsewhere, so a patch pixel of a tile, a portrait or a glyph would show its classic colour in HD. None was seen: with the source plane on, every one of the receipt's 72 patch pixels named the paper on the live frame (a probe, counts only).
12. **Merge plan (D1, M3).** D2 merges after D1. The second of D2 and M3 to merge makes one composition: published frame → view (kept lines from the view's lines, live rows from the published frame) → menu → sprite, into `shown`, and the HD key includes the view stamp; D2's separate HD frame (`Engine::with_view_frame`, which copies the published frame and its source plane at each publish while scrolled back) goes away then.

## Vertical trace (one scenario per section group; parameter arrows are as built)

### S-A: a line scrolls off and is kept (game draw path)

0. **Client surface.** None: the game prints.
1. **Entry.** `print::end_line` (`ui/print.rs:194`) at the row past the window's last, with `SCROLL` set; or `keyread::echo` (`ui/keyread.rs:195`) at the end of a typed row.
2. **Input.** The current window `w: TextWindow`; the front buffer `fb`; the shadow `ui.scrollback`.
3. **Layer flow.**
   - 3a. Every cell drawn inside the Messages rect: `(code c, w.fg, w.bg, kind)` → `Scrollback::rows[row − 14].cells[col − 22]` (`scrollback::note`, called after the draw at the five sites).
   - 3b. Before `text::scroll_window(fb, &w, paper)`: `scrollback::scrolled(ui, fb, &w)`: if `w`'s rect is the Messages rect, `rows[0]` → `Line { cells, bg, patch: fb row 112..119 pixels ≠ the cells' drawing, spans: alert-ink runs }` → `Scrollback::lines.push_back`, the oldest dropped past 500, `total += 1`; `rows` move up one, the last becomes blank with paper `w.bg`. Another window whose rect overlaps it clears the shadow (its next kept lines take their colours from the patch).
4. **Side effects.** `UiState::scrollback` only. No other state, no frame write.
5. **Error paths.** No font: nothing is drawn and nothing is noted. A window of another size than the shadow's: the shadow starts blank at the new size.
6. **Output.** `Scrollback::lines`, `Scrollback::total`.

### S-B: the wheel shows older lines (view path)

0. **Client surface.** `site/app.js`: a `wheel` event on the canvas → `wheelPixel(e)` (as `pointerPixel`, `site/app.js:3903`, with `held`) → game pixel (x, y) → `Game::wheel(x, y, 0)`: false → the carry dropped, the event the browser's; true → `preventDefault`, `wheelLines(e)` → `dy` (decision 4) → `Game::wheel(x, y, dy)` when not 0.
1. **Entry.** `Game::wheel` (`u6-web/src/lib.rs`) → `Engine::wheel(x as i16, y as i16, dy)`.
2. **Input.** (x, y) inside 176..=311 × 112..=191 (the Messages record × 8), program `game`, else false and nothing changes.
3. **Layer flow.** `dy` → `View::offset = clamp(offset − dy, 0, shown)` where `shown` = `View::lines.len()`, the lines kept as of the last publish; a change bumps `View::stamp`. `Engine::wheel` composes at once (trace deviation); `Engine::tick` composes whenever `stamp ≠ shown_stamp`: `shown.sync_from(published)` → `scrollback::draw_view(&ui, &view, &published, &mut shown)` → sprite → `shown_version += 1`. `draw_view`: view row r (0..10) shows combined line `shown − offset + r` of (`View::lines` ++ the published window's rows): a kept line → fill the row with its paper (`text::fill_rect`, naming the paper) → each cell through `draw_char` / `draw_typed` / `draw_symbol` → the patch; a live row → the published row r − (that index − shown) copied, paper sources renamed for the new row as `scroll_rect` does; then the bar (decision 7).
4. **Side effects.** `View` only. `State` unchanged.
5. **Error paths.** Offset already at an end: clamped, no stamp, nothing redrawn. No kept line: nothing.
6. **Output.** `Engine::frame()` (RGBA path: `Game::present` → `redraw`, `u6-web/src/lib.rs:966`, `:1728`) with the view; HD: `HdView::present` keys on `(published_version, view stamp)` and composes `Engine::with_view_frame` (`hd.rs:150`).
7. **Observability.** None logged (the lines are game text).

### S-C: the player's input returns to the live bottom

1. **Entry.** `Engine::tick(input)` (`engine.rs:776`), each event before it is queued.
3. **Layer flow.** `Key` / `Repeat` / `Text` → `offset = 0` (a gesture becomes `Gesture::Held`); `Pointer { buttons }` with a button newly pressed and one held before, or outside the message window, or at offset 0 → `offset = 0`, queued as given; with none held before, inside the window off the bar, at offset ≠ 0 → `offset = 0`, `Gesture::Held`, queued with `buttons = 0` until every button is up.
6. **Output.** The next composition draws no view: the frame is the live one.

### S-D: the scrollbar gestures

1. **Entry.** `Engine::tick(input)`, `InputEvent::Pointer`.
3. **Layer flow.** Offset ≠ 0, no button held before, a left press inside the last cell column × rows → on the thumb: `Gesture::Thumb { grab: y − top }`; above / below it: `offset ± 10` (clamped); a right press there: `Gesture::Held`; the event queued with `buttons = 0`. While the gesture lasts: `top = clamp(y − grab, 0, 80 − len)` → `offset = shown − round(top × shown / (80 − len))` (Thumb, left held); queued with `buttons = 0`; every button up ends it.
6. **Output.** As S-B.

### S-E: new text arrives while scrolled back

3. **Layer flow.** At a publish (`engine.rs:828` `presented`): `new = total − view.published_total`; `published_total = total`; new ≠ 0 → `View::lines` = the shared kept lines now; offset ≠ 0 → `offset = min(offset + new, shown)`, stamp bumped. A source-only publish (`Io::present_sources`) leaves the view as it is.
6. **Output.** The same kept lines in view; the thumb shorter and higher.

## Contract tests (each RED first: the feature removed, the test fails)

| Kind | Test | Source |
|---|---|---|
| Happy | `history_keeps_the_scrolled_off_lines_in_order_up_to_the_bound` | S-A §3b, D1 |
| Happy | `the_wheel_shows_a_kept_line_as_it_looked_before_it_scrolled_off` (pixels and sources, every row in view) | S-B §3, D3 |
| Contract | `with_no_history_view_the_frame_is_the_frame_without_the_feature` (RED: a mutant that draws at offset 0) | S3, D2 |
| Contract (independent) | `with_no_scroll_the_frames_are_the_parents` (`ui/tests/scrollback_baseline.rs`: the synthetic run prints 31 lines and one per event over a speck paper, presses on the bar's column and in the window, keys, a repeat, a release, text; 38 lines kept), `with_no_scroll_three_conversations_show_the_parents_frames` (`converse/tests_baseline.rs`: three real conversations on the boot's paper, two of them scroll); each the shown, published, HD 2× and 4× frames over every tick against FNV-1a digests the same file recorded at 60f92c6, which has no scrollback code, default and wide | S3 |
| Contract | `the_thumb_size_and_place_are_the_history_fraction` | D7 |
| Sad | `a_key_returns_the_view_to_the_live_bottom`, `a_press_outside_the_window_returns_and_reaches_the_game`, `a_press_inside_the_window_while_scrolled_back_returns_to_live_and_is_not_the_games` | S-C, D6 |
| Side effect | `viewing_changes_no_state` (a consumer that takes every event; at every step the State, what the game took and the published frame equal a run given the moves without a button, the shown frame outside the window too) | D2 |
| Happy | `dragging_the_thumb_and_pressing_the_track_scroll`, `a_bar_press_never_reaches_the_game` | S-D |
| Sad | `a_key_during_a_thumb_drag_…`, `a_repeat_during_a_thumb_drag_…`, `text_during_a_thumb_drag_never_hands_the_game_the_button` (the interrupt taken, `handler_pointer().buttons` 0 throughout), `a_thumb_drag_that_leaves_the_bar_never_hands_the_game_the_button`, `a_press_while_the_game_holds_a_button_is_the_games` | S-D, D9 |
| Happy | `new_text_while_scrolled_back_keeps_the_view`, `at_the_bound_a_source_only_publish_keeps_the_view` (no shift, the engine's colour assert quiet), `another_history_returns_the_view_to_the_live_bottom` (fewer lines, and as many) | S-E, D2, D5 |
| Contract | `the_bar_never_covers_a_letter` | D7 |
| Contract | `the_hd_frame_shows_the_history_rows_as_the_font_draws_them` (2× and 4×: each row of the window's first 16 cell columns against the words the synthetic font's rule gives, live, three back, at the oldest (rows of the first screen, never cleared) and live again) | S-B §6, D3 |
| Contract | `a_window_over_the_message_window_scrolling_clears_the_shadow` (the whole screen's window scrolls through `print::end_line`), `the_wheel_over_anything_but_the_window_does_nothing` (0 lines too) | S-A §3b, D4 |
| Contract | `a_kept_line_keeps_each_cell_in_its_colours`, `a_bound_of_lines_all_patch_keeps_the_stated_size` | D1, D11 |
| Real data | `receipt_a_conversation_scrolled_back_shows_each_line_as_it_was` (converse, counts only; every line wheeled to compared) | S1 |
| Browser | `the message history (spec 23)`, Original, the HD layout and Wide: the wheel over the message window scrolls it a line a notch, over the map not, the bar drawn in one ink, the window's text columns and the HD frame over them changed (the bar's column aside), a key back to live | S1, S2, S4 |
| Browser | `the message history (spec 23): the wheel's pixel deltas add up only over the message window …`: outside then inside no jump, fractional deltas, a turn, a leave, a key, Ctrl, a line-mode notch, `defaultPrevented`, the page's scroll inside and outside | D4 |

## File map

`.prd/23-message-scrollback.md`; `crates/u6-core/src/ui/scrollback.rs` (new), `ui/mod.rs`, `ui/print.rs`, `ui/keyread.rs`, `engine.rs`, `hd.rs`; tests `ui/tests/scrollback_tests.rs`, `ui/tests/scrollback_baseline.rs`, `converse/tests_baseline.rs` (new), `ui/tests.rs`, `converse/mod.rs`, `converse/tests_real.rs`; `crates/u6-web/src/lib.rs`; `site/app.js`; `tools/web/test-web.mjs`; `re/notes/port-map.md` (regenerated).

## Trace deviations

- S-B §3: `Engine::wheel` composes the shown frame at once (`Engine::compose_shown`), so the frame a page draws right after the wheel shows it; `Engine::tick` still composes whenever the view's stamp moved (the bar's gestures, the anchoring of S-E).
- Decision 7 as first written drew the bar only over cells of the window's paper colour; the oldest kept lines can sit on another colour (a row cleared before the paper was drawn, a window cleared in another paper), so the rule is a cell of one colour, whatever it is. The letter rule is unchanged.
- S-A §6: the newest kept line never shows on the published frame's last row (the newline scrolls before the next text shows there), so a test reading the window over the run sees each kept line on the frames as one of rows 0..8.
- Round 2 (2026-10-28): the browser check of the HD frame, once its digest left out the bar's column, no longer saw the history in the HD layout and Wide: with the HD font on, the text is drawn by the page from the HD frame's list of cells (spec 11), not in the HD frame's pixels, so round 1's check had passed on the bar's pixels alone. The digest takes the listed cells in the window's text columns too. The RGBA check now reads the scrolled-back window with the pointer's sprite out of it, as at the live read (the sprite had made "changed" pass with no history row drawn, W5).
- Round 2: the view's lines as of the last publish are its own (`View::lines`, shared with the history), no longer counted back from the history's lines now: at the bound, lines kept and dropped after a publish had shifted the view on a source-only publish (and fired the engine's colour assert, `engine.rs:887`).

## Implementation status

| Scenario | Code | Tests | Verified |
|---|---|---|---|
| S-A capture | `ui/scrollback.rs` `note`, `scrolled`; `ui/print.rs` `cell`, `end_line`; `ui/keyread.rs` `idle_wait`, `echo` | `history_keeps_…`, `a_kept_line_keeps_…`, `a_window_over_…_scrolling_…`, `a_bound_of_lines_all_patch_…`, the real-data receipt | RED R1, O1 |
| S-B wheel view | `ui/scrollback.rs` `draw_view`, `draw_bar`, `thumb`, `View::wheel`; `engine.rs` `Engine::wheel`, `compose_shown`, `draw_view`, `with_view_frame`; `hd.rs` `HdView::present` | `the_wheel_shows_…`, `with_no_history_view_…`, `the_thumb_…`, `the_bar_never_…`, `the_hd_frame_shows_the_history_rows_…`, the two baselines, the browser tests | RED R2, R3, R4, R8, H1, H2, B1, W1, W5 |
| S-C snap back | `View::input` (keys, repeats, text, a press outside the window or with a button held: the game's; a press in the window: live, the view's) | `a_key_returns_…`, `a_press_outside_…`, `a_press_inside_…`, `a_press_while_the_game_holds_…`, `viewing_changes_no_state` | RED R5, B2, V1; on 06b1ab0 |
| S-D bar gestures | `View::input`, `View::leave`, `View::drag`, `View::holding`; `engine.rs` `history_holding`; `u6-web` `pointer_held` | `dragging_the_thumb_…`, `a_bar_press_never_…`, the three interrupted drags, `a_thumb_drag_that_leaves_…`, u6-web `a_press_the_message_history_holds_paces_at_1x` | RED R7, V1, N1, N1w; on 06b1ab0 |
| S-E anchoring | `View::publish`, `View::lines` | `new_text_while_scrolled_back_keeps_the_view`, `at_the_bound_a_source_only_publish_keeps_the_view`, `another_history_returns_…` | RED R6, P1; on 06b1ab0 |
| page wiring | `site/app.js` wheel, pointer-move, pointer-leave and key listeners; `u6-web` `Game::wheel`, `Game::history_offset` | `the message history (spec 23)` ×4 | RED W1, W2, W3, W4, W5 |

Verified 2026-10-28 on the branch (not merged): fmt; clippy `-D warnings` plain and `--features u6-core/wide`; u6-core lib 1356 (default) and 1381 (wide) passed; u6-web lib 51 and 51; port-map up to date; build-web; the web suite 157 ran, 156 passed, the one failure the known Wide compositor timing test. The real-data receipt: 38 conversations, 1058 kept lines wheeled to and compared with the rows they showed on the published frames, all equal (wide build 1057). Each RED (R1–R8 engine mutations, W1 the page's wheel call removed) fails its tests.

Verified round 2, 2026-10-28 (not merged): fmt; clippy `-D warnings` plain and `--features u6-core/wide` (u6-core, u6-web, all targets); u6-core lib gate profile 1366 passed and 1 failed (default; the failure `amplifying_scripts_are_refused_quickly_and_the_export_goes_on`, a 5 s wall-clock bound in a file this unit does not touch, at load average about 170; alone it passes in the gate profile) and 1392 passed (wide); u6-render 59 passed; u6-web lib 52 and 52; port-map up to date (no diff); build-web; the web suite 158 ran, 157 passed, the one failure the known Wide compositor timing test. The real-data receipt: 38 conversations, 1058 lines kept, wheeled to and compared, all equal; 36 lines with a patch; the largest history 17 750 bytes. The baselines equal 60f92c6's digests in both builds. REDs on 06b1ab0 (the round-2 tests on the round-1 code): the three interrupted drags, `a_press_inside_…`, `a_press_while_the_game_holds_…`, `at_the_bound_…` (the engine's colour assert); on the round-2 tree, H1, H2, B1, B2, V1, O1, N1, N1w, P1, P2 and the page's W2–W5 each fail their tests.
