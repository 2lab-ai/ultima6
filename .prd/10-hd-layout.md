---
title: ultima6 — 10 HD layout (1280 × 720, the pack one to one, fullscreen stretch)
type: spec
tags: [ultima6, hd, ui, layout]
---

# 10 — HD layout: the game's screen recomposed for 1280 × 720

Status: shipped (2026-10-23, in production at https://ultima6.2lab.ai from main 904ffd2; deploy 2 of `hd-layout-font-register/loop.md`). As built: the column's scale is 3.5, the only scale of 3.5..3.75 that puts every rect on whole pixels, with gaps of 30 and 31 px; a move with a button held in a gap is sent like a press (the rest of the screen). Follow-up L2 (the game's paper round the regions, Design 2) shipped as a later deploy of the same loop. User item 1 of `hd-layout-font-register/ssot.md`, verbatim: "hd 해상도에 맞게 화면 ui 재구성해주고 hd pack을 최대한 사용할 수 있도록 개선 (전체 화면 모드 지원; 최대 해상도는 hd고 그거보다 화면이 크면 알아서 스트레칭)". It builds on 07's compositor and pack and does not change the engine or the parity frame.

## Measured facts (main 6e4e337)

- The page draws the classic frame (320 × 200) or the HD frame (320·S × 200·S) on a canvas of that size (`site/app.js:1266-1295`). S is 4 when the canvas box holds at least 3 device pixels per game pixel each way, else 2 (`app.js:1314-1318`). The box is `aspect-ratio: 320 / 200` (`site/index.html:27`), 960 × 600 on a 1920-wide window, so the ×4 frame (1280 × 800) is shown at 0.75: the pack's pixels are never shown one to one. In fullscreen the box is `min(100vw, 160vh)` wide (`index.html:52`), 1728 × 1080 on a 1920 × 1080 screen.
- The pointer maps the canvas box onto the game's 320 × 200 pixels (`app.js:2425-2431`); the hit tests use those pixels (`ui/pointer.rs:19-23`, `ui/click.rs:37`).
- The in-game screen's parts, from the code (screen pixels, inclusive):
  - the map window 8..=167 × 8..=167 (`gfx/view.rs:3-8`);
  - the command bar: icons at x = 8 + 16·i, y = 0xb0 (`main_loop.rs:357-361`), the selected command's underline on row 0xc0 (`ui/cursor.rs:21-38`): 8..=167 × 176..=192;
  - the date line: the Screen window, cursor cell (2, 0x15) (`actors/seams.rs:249-258`): 16..=167 × 168..=175;
  - the side panel (party, inventory, spellbook, portraits): 0xa8..=0x137 × 6..=0x67 (`magic/book.rs:171`, `ui/panels.rs:16`);
  - the message scroll: the Messages window, cells 0x16..=0x26 × 0x0e..=0x17 (`ui/textstate.rs:167-171`), 176..=311 × 112..=191.
  Everything else is `paper.bmp`, drawn over the whole screen at the boot (`main_loop.rs:350-356`).
- `#screen[data-program]` says which program runs (`u` or `game`).

## Design

1. **Output.** With HD on and the layout set to HD (the default), the canvas is 1280 × 720 and the compositor always runs at S = 4 (`game.set_hd(true, 4)`). The ×4 frame (1280 × 800) is kept on an offscreen canvas, updated by its dirty rect as today, and the visible canvas is drawn from it with `drawImage` rect copies.
2. **The game screen (`data-program` = `game`).** Five regions are copied from the ×4 frame:
   - the map window at ×4 (640 × 640) and, under it, the command bar at ×4 (640 × 68): the pack's pixels one to one (a destination rect the same size as its source rect, at integer coordinates);
   - on the right, one column at a single scale s (3.5 ≤ s ≤ 3.75, the largest that fits): the date line, the side panel, then the message scroll, in that order, each centred in the column;
   - margins and gaps symmetric, at least 4 px, every destination rect at integer coordinates.
   The background is the game's paper, taken from the ×4 frame at each whole paint (never stored): the paper's band at the canvas edges, 9-sliced (24 px top and bottom, 32 px left and right at ×4; the top edge one to one, the left edge squashed to fit), and inside it one parchment colour, each channel the median of three paper pixels of the row under the side panel (one pixel would take the pointer's colour when it passes). The right edge, the bottom edge and the bottom-right corner are the left edge, the top edge and the bottom-left corner mirrored: the frame's own carry the paper's frame round the message scroll, and the pointer passes there. A dirty rect that meets a rect the band is taken from makes the paint whole. The parchment samples are not such sources (one bounding box per take spans their row whenever the map animates and the scroll changes); instead each partial paint reads the three samples from the ×4 frame and paints whole when their median is not the parchment drawn, so a covered sample the median absorbs (the pointer) repaints nothing and any change of the parchment repaints it. Every rect the band is taken from lies outside the regions (pinned by the L2 test). (v1, before L2: a dark fill and a thin frame round each region.) The paper's frame round the message scroll is not drawn in this layout: the column leaves no room for it.
3. **Every other screen** (`u`: the opening, the menu, creation; and any frame while the program is not `game`): the whole ×4 frame scaled to fit 1280 × 720 with its 16:10 shape (1152 × 720, centred), the sides dark.
4. **Pointer.** The page inverts the layout: a point inside a destination rect maps to the game pixel of its source rect. In a gap or margin, a move with no button maps to the nearest pixel inside the nearest region (so the game's pointer sprite stays where the player sees it), but a press or a release maps to a pixel of the screen that is in none of the map window, the command bar and the side panel, so it is the original's click on the rest of the screen, a cancel (`ui/click.rs:36-58`: region 3 gives `0x8f` and clears the target), never a walk or a command. The page exposes the map for tests (`globalThis.__u6Layout`: `toGame(px, py)` and `toCanvas(gx, gy)`, canvas pixels ↔ game pixels).
5. **Fit and stretch.** The canvas box keeps 16:9. In the page it is as wide as the column allows, at most 1280 CSS px. In fullscreen (the browser's or the page's own) it is as large as the screen allows with 16:9, centred on black, at any size ("알아서 스트레칭"): `image-rendering: pixelated` when the box is an integer multiple of 1280 × 720 in device pixels, smooth otherwise. The canvas never holds more than 1280 × 720 pixels ("최대 해상도는 hd").
6. **Setting.** Settings gains "Screen layout: HD 16:9 / Original 16:10" ("화면 배치: HD 16:9 / 원본 16:10"), kept in `localStorage` (`u6.layout`, any other value is HD). With Original, or with HD off, the page is as before this spec (07's canvas sizes and S choice).
7. **Tests.** `tools/web/harness.mjs` sets `u6.layout` to `original` before a page loads, unless a test asks for HD, so every click test keeps its 320 × 200 box; the HD layout tests click through `__u6Layout.toCanvas`.

## Acceptance

1. In game at 1280 × 720 (a 1400 × 900 window) the map rect of the visible canvas equals the map rect of the ×4 frame pixel for pixel, and so does the command bar.
2. Clicks through the layout: a map cell walks or targets that cell; each command icon selects its command; a party member in the side panel opens its inventory; a press in a gap is the original's click on the rest of the screen (no walk, no command), and a move there keeps the pointer sprite in the nearest region.
3. The opening and the menu show the whole frame letterboxed, and their clicks still land.
4. Fullscreen at 1920 × 1080 (smooth stretch), 2560 × 1440 (pixelated ×2) and 1728 × 1117 (16:9 with bars): the box size, the rendering mode and a key reaching the game are printed (the web test "HD layout (spec 10 Acceptance 4)", in the browser's and the page's own fullscreen).
5. Original layout and HD off are as before: the existing web tests pass unchanged.
6. Gates (`merge-gate.sh`) pass; the live site, a fresh visitor with the zip only, shows 1 to 4.
