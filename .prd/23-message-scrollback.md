---
title: ultima6 — 23 Message scrollback (the wheel shows past lines, a scrollbar on the right)
type: spec
tags: [ultima6, ui, hd, wide, input]
---

# 23 — Message scrollback

Status: implemented, not merged (2026-10-28, unit T2, branch `feat/message-scroll` from integ/r1027 60f92c6). The same owner message asked for the Job and Name buttons and the clickable keywords: that is unit T1 (spec 22, branch `feat/talk-clicks`).

Owner's words (2026-10-28, verbatim): "그리고 메세지창에서 스크롤로 지난 대화 볼수 있게 해줘 오른족에 스크롤바도 추가해줘". In the same message the owner corrected that UI must be the game's own (in-game), not web-page elements (the brief's summary): everything here is drawn by the engine into the frame, in the game's colours; the page adds no element.

## Clause map

| Owner, verbatim | Acceptance |
|---|---|
| "메세지창에서 스크롤로 지난 대화 볼수 있게 해줘" | S1: after a conversation long enough to scroll, the wheel over the message window shows the lines that scrolled off, each as it looked before it scrolled off (real data: pixel equality of every line seen, counts only); the wheel elsewhere does nothing |
| "오른족에 스크롤바도 추가해줘" | S2: while the view is scrolled back, a scrollbar is drawn by the engine at the window's right edge: a track and a thumb whose length is the visible fraction and whose place is the fraction of lines above; dragging the thumb and pressing the track scroll |
| (the original stays the original) | S3: with no history view the frame is byte-identical to the frame without the feature, in Original and Wide, with and without the source plane |
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

1. **The history is cells, kept as the game draws them.** A shadow of the message window's rows (`ui::scrollback::Scrollback`, in `UiState`) records each cell a draw site writes inside the rect: code, ink, paper and kind (character, typed, symbol). When the message window scrolls, its top row goes to the history before the move, with two additions: the pixels of the screen row the cells do not reproduce (a colour patch, empty in practice; it makes every kept line colour-exact whatever drew there) and its keyword spans (the runs of cells drawn in the alert ink, which is how braced words print, `ui/print.rs:137`). At most **500 lines**, oldest dropped first: at 17 cells a line holds 102 bytes of cells and three vector headers, about 0.1 MB at the bound (`Scrollback::heap_bytes`, tested). It is presentation only: never saved, kept across a Restore, the same in every build.
2. **The view is the engine's, not the game's.** The offset (lines back from the live bottom, 0 = live), the gesture on the scrollbar and the history count as of the last publish live in `Engine`, not in `State`: viewing changes no state the game reads. At offset 0 nothing is drawn, so the frame is the original's by construction (S3).
3. **The view is drawn between the published frame and the pointer sprite**, into `Engine::shown`, after its sync and before the sprite; the next sync puts back what it drew (the overlay's writes mark their chunks). For HD a second frame is built only while the offset is not 0, on demand: the published frame copied and the view drawn over it (`Engine::with_view_frame`); at offset 0 HD composes the published frame as before.
4. **The wheel.** `Engine::wheel(x, y, dy)`, outside the game's input queue (no new `InputEvent`): `dy` < 0 shows older lines, by |dy| lines; > 0 newer. It acts only over the message window's pixel rect, only while the program is `game`, and clamps to the lines kept. The page: one notch is one line (a line- or page-mode event is one line in its direction; pixel deltas, a trackpad's, add up, 100 CSS pixels a line, the carry dropped when the direction turns). Over a window in front of the message window, a window's frame or a gap, the page sends the rest pixel (`layoutToGame` / `wideToGame` with `held`), which is outside the rect.
5. **New text while scrolled back does not move the view.** At each publish the offset grows by the lines that scrolled off since the last one, so the same lines stay in view; only the thumb moves (the history grew). The thumb above the bottom is the sign that newer lines follow. The view leaves the history only by the player (6) or the wheel and the bar.
6. **Leaving the history.** In `Engine::tick`, before it is queued for the game: a key press, a key repeat or typed text, and a pointer press (either button) anywhere but on the scrollbar, set the offset to 0 in that tick. A move without a button, a release and the keys the page keeps for itself (the speed keys) do not.
7. **The scrollbar.** Drawn only while the offset is not 0, inside the window's last cell column (Original x 304..=311), over the window's 80 rows:
   - the track: a dotted 1-pixel line at the column's x 6 (Original x 310), every other row, in the text ink;
   - the thumb: 3 pixels wide at the column's x 5..=7 (Original x 309..=311), solid, in the text ink; its length is the visible fraction of the 80-pixel track, rows / (history + rows), at least 8 pixels; its top is (80 − length) × (lines above the view) / (history);
   - both are drawn only in the column's cells that show one colour (no letter, whatever the paper under them is): a cell that shows a letter keeps it whole, and the bar yields to it.

   Why thin and yielding: a scrollbar in its own cell column would hide the 17th cell, which holds text on about one row in eleven (facts); the arrow glyphs need a cell each. Why it yields to a letter cell instead of drawing on its paper pixels: in the HD font mode a cell is drawn in the HD font only when all 64 of its pixels are its glyph's (`u6-render/src/hd.rs:679-715`); a bar pixel in it would turn that letter back to the pixel font. The ink rule matches the game's own 1-pixel rules on the paper.
8. **The text area does not narrow.** The game wraps in `end_line` at its window's width, 17 (`ui/print.rs:200-216`), and the view never touches the window record or the line: live text always wraps as the original's. The view shows kept lines as they were wrapped.
9. **The bar's gestures.** The bar's hit area is the last cell column over the window's rows, only while it is drawn. A left press on the thumb grabs it: moves with the button held set the thumb's top, and the offset follows (the inverse of 7); a left press on the track above the thumb shows a window of rows (10) older, below it 10 newer, at most to the live bottom. The gesture lasts until every button is up. Its events still reach the game, with no button held: the pointer sprite follows the pointer (it reads the queued positions, `engine.rs:217`), and the game sees moves only.
10. **Keyword spans for T1.** Each kept line carries its spans as column ranges. When T1's span record (spec 22) merges, its record of a row replaces the alert-ink runs at the capture, and a press on a kept span (while scrolled back) is T1's to route; until then a press on the history is a press anywhere (6).

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

0. **Client surface.** `site/app.js`: a `wheel` event on the canvas → `wheelPixel(e)` (as `pointerPixel`, `site/app.js:3903`, with `held`) → game pixel (x, y); `wheelLines(e)` → `dy` (decision 4) → `Game::wheel(x, y, dy)`; `preventDefault` when it returns true.
1. **Entry.** `Game::wheel` (`u6-web/src/lib.rs`) → `Engine::wheel(x as i16, y as i16, dy)`.
2. **Input.** (x, y) inside 176..=311 × 112..=191 (the Messages record × 8), program `game`, else false and nothing changes.
3. **Layer flow.** `dy` → `View::offset = clamp(offset − dy, 0, shown)` where `shown` = lines kept as of the last publish; a change bumps `View::stamp`. Next `Engine::tick`: `stamp ≠ shown_stamp` → `shown.sync_from(published)` → `scrollback::draw_view(&ui, &view, &published, &mut shown)` → sprite → `shown_version += 1`. `draw_view`: view row r (0..10) shows combined line `shown − offset + r` of (kept lines ++ the published window's rows): a kept line → fill the row with its paper (`text::fill_rect`, naming the paper) → each cell through `draw_char` / `draw_typed` / `draw_symbol` → the patch; a live row → the published row r − (that index − shown) copied, paper sources renamed for the new row as `scroll_rect` does; then the bar (decision 7).
4. **Side effects.** `View` only. `State` unchanged.
5. **Error paths.** Offset already at an end: clamped, no stamp, nothing redrawn. No kept line: nothing.
6. **Output.** `Engine::frame()` (RGBA path: `Game::present` → `redraw`, `u6-web/src/lib.rs:966`, `:1728`) with the view; HD: `HdView::present` keys on `(published_version, view stamp)` and composes `Engine::with_view_frame` (`hd.rs:150`).
7. **Observability.** None logged (the lines are game text).

### S-C: the player's input returns to the live bottom

1. **Entry.** `Engine::tick(input)` (`engine.rs:776`), each event before it is queued.
3. **Layer flow.** `Key` / `Repeat` / `Text` → `offset = 0`; `Pointer { buttons }` with a button newly pressed, not on the bar (or no bar) → `offset = 0`; queued as given.
6. **Output.** The next composition draws no view: the frame is the live one.

### S-D: the scrollbar gestures

1. **Entry.** `Engine::tick(input)`, `InputEvent::Pointer`.
3. **Layer flow.** Offset ≠ 0, a left press inside the last cell column × rows → on the thumb: `Gesture::Thumb { grab: y − top }`; above / below it: `offset ± 10` (clamped); the event queued with `buttons = 0`. While the gesture lasts: `top = clamp(y − grab, 0, 80 − len)` → `offset = shown − round(top × shown / (80 − len))`; queued with `buttons = 0`; every button up ends it.
6. **Output.** As S-B.

### S-E: new text arrives while scrolled back

3. **Layer flow.** At a publish (`engine.rs:828` `presented`): `new = total − view.published_total`; `published_total = total`; offset ≠ 0 → `offset = min(offset + new, shown)`, stamp bumped.
6. **Output.** The same kept lines in view; the thumb shorter and higher.

## Contract tests (each RED first: the feature removed, the test fails)

| Kind | Test | Source |
|---|---|---|
| Happy | `history_keeps_the_scrolled_off_lines_in_order_up_to_the_bound` | S-A §3b, D1 |
| Happy | `the_wheel_shows_a_kept_line_as_it_looked_before_it_scrolled_off` (pixels and sources, every row in view) | S-B §3, D3 |
| Contract | `with_no_history_view_the_frame_is_the_frame_without_the_feature` (RED: a mutant that draws at offset 0) | S3, D2 |
| Contract | `the_thumb_size_and_place_are_the_history_fraction` | D7 |
| Sad | `a_key_returns_the_view_to_the_live_bottom`, `a_press_off_the_bar_returns_and_reaches_the_game` | S-C |
| Side effect | `viewing_changes_no_state` (State equal to a run without the wheel) | D2 |
| Happy | `dragging_the_thumb_and_pressing_the_track_scroll`, `a_bar_press_never_reaches_the_game` | S-D |
| Happy | `new_text_while_scrolled_back_keeps_the_view` | S-E, D5 |
| Contract | `the_bar_never_covers_a_letter` | D7 |
| Contract | `the_hd_frame_shows_the_view_and_at_the_live_bottom_is_the_frame_without_it` (both scales; RED: HD composing the published frame alone) | S-B §6, D3 |
| Contract | `a_window_over_the_message_window_clears_the_shadow`, `the_wheel_over_anything_but_the_window_does_nothing` | S-A §3b, D4 |
| Real data | `receipt_a_conversation_scrolled_back_shows_each_line_as_it_was` (converse, counts only) | S1 |
| Browser | `the message history (spec 23)`, Original, the HD layout and Wide: the wheel over the message window scrolls it a line a notch, over the map not, the bar drawn in one ink, the HD frame changed, a key back to live | S1, S2, S4 |

## File map

`.prd/23-message-scrollback.md`; `crates/u6-core/src/ui/scrollback.rs` (new), `ui/mod.rs`, `ui/print.rs`, `ui/keyread.rs`, `engine.rs`, `hd.rs`; tests `ui/tests/scrollback_tests.rs` (new), `ui/tests.rs`, `converse/tests_real.rs`; `crates/u6-web/src/lib.rs`; `site/app.js`; `tools/web/test-web.mjs`; `re/notes/port-map.md` (regenerated).

## Trace deviations

- S-B §3: `Engine::wheel` composes the shown frame at once (`Engine::compose_shown`), so the frame a page draws right after the wheel shows it; `Engine::tick` still composes whenever the view's stamp moved (the bar's gestures, the anchoring of S-E).
- Decision 7 as first written drew the bar only over cells of the window's paper colour; the oldest kept lines can sit on another colour (a row cleared before the paper was drawn, a window cleared in another paper), so the rule is a cell of one colour, whatever it is. The letter rule is unchanged.
- S-A §6: the newest kept line never shows on the published frame's last row (the newline scrolls before the next text shows there), so a test reading the window over the run sees each kept line on the frames as one of rows 0..8.

## Implementation status

| Scenario | Code | Tests | Verified |
|---|---|---|---|
| S-A capture | `ui/scrollback.rs` `note`, `scrolled`; `ui/print.rs` `cell`, `end_line`; `ui/keyread.rs` `idle_wait`, `echo` | `history_keeps_…`, `a_kept_line_keeps_…`, `a_window_over_…`, the real-data receipt | RED R1 |
| S-B wheel view | `ui/scrollback.rs` `draw_view`, `draw_bar`, `thumb`, `View::wheel`; `engine.rs` `Engine::wheel`, `compose_shown`, `draw_view`, `with_view_frame`; `hd.rs` `HdView::present` | `the_wheel_shows_…`, `with_no_history_view_…`, `the_thumb_…`, `the_bar_never_…`, `the_hd_frame_…`, the browser tests | RED R2, R3, R4, R8, W1 |
| S-C snap back | `View::input` (keys, repeats, text, a press off the bar) | `a_key_returns_…`, `a_press_off_the_bar_…` | RED R5 |
| S-D bar gestures | `View::input`, `View::drag` | `dragging_the_thumb_…`, `a_bar_press_never_…` | RED R7 |
| S-E anchoring | `View::publish`, `View::kept` | `new_text_while_scrolled_back_keeps_the_view` | RED R6 |
| page wiring | `site/app.js` wheel listener; `u6-web` `Game::wheel`, `Game::history_offset` | `the message history (spec 23)` ×3 | RED W1 |

Verified 2026-10-28 on the branch (not merged): fmt; clippy `-D warnings` plain and `--features u6-core/wide`; u6-core lib 1356 (default) and 1381 (wide) passed; u6-web lib 51 and 51; port-map up to date; build-web; the web suite 157 ran, 156 passed, the one failure the known Wide compositor timing test. The real-data receipt: 38 conversations, 1058 kept lines wheeled to and compared with the rows they showed on the published frames, all equal (wide build 1057). Each RED (R1–R8 engine mutations, W1 the page's wheel call removed) fails its tests.
