---
title: ultima6 — 14 Actor windows in Wide (party, attributes, paper doll and backpack open at once)
type: spec
tags: [ultima6, hd, ui, layout, wide]
---

# 14 — Actor windows: the side panel's views open at once, U7-style

Status: rev 2, 2026-10-24. Trinity review (grok-4.7, gpt-6-astra, fable) reached a unanimous verdict in round 2: C, rev 2. The briefs and positions are kept outside the repo, under `work/dispatch/r1023-wide/trinity-spec14-*`.

This spec builds on spec 13 (Wide mode). Original mode is unchanged. The owner's words are in `hd-layout-font-register/ssot.md` §Round 2. The reference image is nuvie.png; its right half shows one member's paper-doll window, portrait-and-attributes window and backpack window open at once over the map.

## Clause map

| Owner, verbatim | Acceptance |
|---|---|
| "nuvie.png 여기처럼 구현해줘 울티마7 처럼 ui 변경 저기 확인해서" | B1: in Wide, a live 1920 × 1080 capture in game shows these at the same time, each in its own framed window: the party list with the sky bar; one member's portrait with attributes; that member's paper doll; that member's backpack list with its weights |
| (same) | B2: every window stays live. A hit-point change shows in the party window while the doll and backpack windows are open. Readying a weapon, by click or by key, shows in the doll window in the same turn |
| (same) | B3: a click in any window does what the same click does in the original panel in that view. A member's row in the party window opens that member's views. A backpack item is used, readied or dropped. The list scrolls. A bag inside the backpack opens and closes back. The attributes window's icons step to the previous or next member |
| Round 2, "원본 모드랑 신규 모드 둘다 지원하도록해줘" | B4: Original and HD 16:9 (the `pkg` build) pass every gate they pass today, unchanged |

## Today (main 7cc8d5d)

- **One panel area, one mode at a time.** The side panel is one 144 × 98 area (`ui/panels.rs:16` `PANEL` = (0xa8, 6)..(0x138, 0x67); page region `[168, 6, 144, 98]`, `site/app.js:1519`). `redraw_panel` (`ui/panels.rs:112-139`) draws one mode of it, and only when `dirty`. The modes are:
  - 0x91 roster;
  - 0x90 stats;
  - 0x92 inventory;
  - 0x9e another actor's inventory;
  - also the spellbook and the conversation portrait.
- **Drawing reads the mode.**
  - `sky_bar` returns unless the mode is 0x91 or 0x9e (`ui/panels.rs:403-406`).
  - `draw_list` returns in 0x91 (`inventory/panel.rs:215-219`).
  - `inventory()` sets `inventory_first = 0` (`ui/panels.rs:339`) and re-derives `container` from `open_container` or the member (`:364-376`).
- **Clicks read the mode and the drawn state.**
  - `panel_click` (`ui/click.rs:144-173`) does the press check, then the right-click repeat (0xa9), then the mode's handler, then the target-pending confirmation (0x8e).
  - `inventory_click` resolves an item through `target_from_pointer` → `ui/cursor.rs:76-88`, only in mode 0x92.
  - It reads the cells `recount` and `draw_list` filled (`inventory/panel.rs:72-80`, `:270-282`, `:364-375`).
- **Some panel codes draw in place without `dirty`.**
  - Scrolling: 0x95 and 0x96, `ui/wait.rs:137-147` → `inventory/panel.rs:320-357`.
  - A bag inside the inventory: 0x9f and 0xa0 (`ui/wait.rs:95-121`). These set `container` and leave `open_container` as it is.
  - Ready and unready: 0xa7 and 0xa8 (`ui/wait.rs:152-161`).
  - Some commands refresh the inventory only when the mode is 0x92 (`usecmd/food.rs:54`, `objects/commands/world.rs:468`, `objects/commands/get.rs:79`).
- **The inventory view's layout, in panel pixels.**
  - Doll cells: x 0xb0..0xef, y 0x10..0x4f (`inventory/panel.rs:27-36`).
  - Container picture at (0x110, 0x10). The list is 4 × 3 cells (`:270-275`), with the list area at 0xf8..0x137 × 0x10..0x4f (`:228`).
  - List arrows: column 8, x 240..247.
  - Weight lines on row 9 (`:289-316`): Wielded at x 192..239, Carried from x 248.
  - Icon row at y 0x58: member icons at x 176..239; the combat-mode icon at 240..255 (`ui/click.rs:358-377`, `ui/panels.rs:20`).
  - The name line is row 0, centred in the Status window, x 176..311 (`ui/text.rs:100-101`).
- **Wide's frame and copies.** The Wide frame is 912 × 336 (`u6-render/src/lib.rs:28-33`), with the UI's 320 × 200 at its top left.
  - `copy_rect` keeps a copy into the UI's columns inside 320 × 200 (`gfx/blit.rs` `stays`).
  - `Framebuffer::copy_px_within` (`u6-render/src/lib.rs:373-383`) moves a pixel and its source within one buffer.
  - In Wide, the page shows the one panel area as the 파티 window (`site/app.js:1937-1947`).

## Design

1. **The frame grows to 912 × 436 in Wide.** Three fixed slots, each 144 × 98, hold three live views:

   | Slot | Frame origin |
   |---|---|
   | roster | (8, 202) |
   | stats | (160, 202) |
   | inventory | (8, 338) |

   - Nothing else draws in these rows or rects.
   - Original keeps 320 × 200. `HEIGHT` is the only frame constant; the page and native read the frame's size from the engine.
   - The frame area grows by 30%. The compositor receipt (`test-web.mjs` "Wide layout (spec 13 Design 6)") is measured again; above 8 ms the page asks S 2 by its own rule.
2. **A Wide redraw draws all three views.** On each `redraw_panel`, Wide draws each of roster, stats and inventory into the panel area, the current mode last. Each view is drawn with that view's `panel.mode` set, and the mode is restored after. After each draw, the panel rect (pixels and source plane, through `copy_px_within`) is copied into that view's slot. The copy of the current view is made after `panel_cursor`.
   - The extra draws leave every other state as a current-mode-only draw would leave it. The exception is the inventory context in Design 3.
   - A mode other than the three (spellbook, conversation, 0x9e) is drawn as today. The slots then keep their last views (see Design 6).
3. **The inventory context is kept.** In Wide, `inventory()` keeps `inventory_first` and `container`.
   - It resets the page and re-derives the container only when the shown member changes: 0x90, 0x92 with a selection, 0x99, 0x9a, 0x91.
   - It also resets on 0x9f and 0xa0, as today.
   - So the slot's pixels and the cells a click reads are always the same page and container.
4. **Every change reaches the slots.** In Wide, one place sets `panel.dirty = true`: after each panel code is handled (`ui/wait.rs` `read_key`) and after each command completes. This covers the in-place draws and the mode-gated refreshes listed above. Under Design 3, the redraw keeps the page and the open bag.
5. **A click in a slot goes through `panel_click`.** Wide adds three click regions, one per slot, after the side panel's region. A press in a slot:
   1. moves the pointer to the same place in the panel area;
   2. sets `panel.mode` to the slot's view and the inventory context of Design 3;
   3. calls `recount` for the inventory slot;
   4. calls `panel_click`, so the press check, the right-click repeat and the target-pending confirmation all run;
   5. keeps that mode until `read_key` has handled the code it returns;
   6. puts the pointer back where the press was;
   7. sets `dirty`.

   The codes switch the panel as today.
6. **The active view, and other modes.**
   - The current mode is the active view; the page marks its window (title strip and border).
   - While the mode is none of the three, a 패널 window shows the real panel area. The slots show their last views, dimmed, and a press in one is ignored until the mode is one of the three again.
7. **The page's windows** (spec 13's frames, drag and memory):

   | Window | Shows | Frame rect |
   |---|---|---|
   | 파티 | the roster slot | (8, 202, 144, 98) |
   | 능력치 | the stats slot | (160, 202, 144, 98) |
   | 인형 | the inventory slot's doll part: equipment, figure, Wielded, member icons | slot (0..71, 10..97) = frame (8, 348, 72, 88) |
   | 배낭 | the inventory slot's list part: container picture, 4 × 3 list, arrows, Carried, combat-mode icon | slot (72..143, 10..97) = frame (80, 348, 72, 88) |
   | 패널 | the panel area | only while Design 6 says |

   - The name row (rows 0..9) is left out; the 능력치, 인형 and 배낭 titles carry the shown member's name.
   - 날짜, 메시지 and 명령 stay as in spec 13.
   - Default places do not overlap on 1920 × 1080: 파티 under 날짜 at the top right, 능력치 to its left, 인형 and 배낭 side by side under 능력치.
   - The engine adds one read for the page, `panel_view()`: [mode, member, actor], plus the shown member's name.
8. **The combat-mode icon** sits at the bottom of the 배낭 window this round. Moving it to the doll window (an L-shaped window) and showing several members' bags at once are later work.

## Tests

- **Engine, Wide:**
  - after a redraw in each of the three modes, each slot holds its view, pixels and source plane, and roster's sky bar is drawn;
  - a redraw's state equals a current-mode-only redraw's, field by field, except Design 3's context;
  - scroll past 12 items, then any dirty redraw: the slot shows page N, and `inventory_first == N`;
  - open a bag in the 배낭 slot, Use an item in it: the bag stays open;
  - ready or unready by key while the roster is current: the inventory slot shows it in the same turn;
  - a press in each slot selects the same object, and gives the same ready/unready/use/drop result, as the same press in the panel in that mode;
  - the pointer is back after a slot press;
  - in another mode, a slot press is ignored.
- **Engine, Original:** no slot, no region, no frame change. Every existing test passes unchanged.
- **Web:**
  - the Wide layout lists 파티, 능력치, 인형, 배낭, 메시지, 명령 and 날짜, none overlapping at their default places, with the frame at 912 × 436 (the spec 13 literals updated);
  - 패널 shows only in another mode;
  - the active window is marked;
  - a click in each window reaches the engine at its frame rect's pixel;
  - the compositor receipt is re-measured.
- **Live:** B1–B3 on ultima6.2lab.ai as a fresh visitor at 1920 × 1080.

## Units (one agent each, DEV §3; the coordinates above are the contract)

| Unit | Scope |
|---|---|
| G1 | engine: `u6-render/src/lib.rs` (HEIGHT), `geom.rs` (slots, the frame assert), `ui/panels.rs`, `ui/click.rs`, `ui/pointer.rs`, `ui/wait.rs`, `ui/cursor.rs`, `inventory/panel.rs`, the command-completion hook; Designs 1–6 |
| G2 | page: `site/app.js`, `site/i18n.js`, `crates/u6-web/src/lib.rs` (`panel_view`), `tools/web/test-web.mjs`; Designs 6–7 |
