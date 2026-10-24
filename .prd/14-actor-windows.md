---
title: ultima6 — 14 Actor windows in Wide (party, attributes, paper doll and backpack open at once)
type: spec
tags: [ultima6, hd, ui, layout, wide]
---

# 14 — Actor windows: the side panel's views open at once, U7-style

Status: rev 5, 2026-10-24. Trinity review (grok-4.7, gpt-6-astra, fable) reached a unanimous verdict in round 2: C, rev 2. The briefs and positions are kept outside the repo, under `work/dispatch/r1023-wide/trinity-spec14-*`.

Rev 3 (during G1 and G2, from G2's report) changes two things:
- **Slot rows.** The slots move onto the HD font's 8-pixel grid, so the frame is 912 × 408. `hd.rs:613-616` `glyph_cell` draws a cell in the HD font only at `(col*8, row*8)`, and the Status window's text is on that grid (`ui/text.rs:99`, text rows 1..12). Rev 2's rows 202 and 338 moved panel text 196 and 332 rows, which is off the grid, so the actor windows would have shown the pixel font.
- **Conversations show 패널.** A conversation draws its portrait in the panel area with the mode at 0x9e only while drawing, then puts the mode back, usually to 0x91 (`inventory/portrait.rs:40-158`). Under rev 2's mode rule the portrait was invisible in Wide.

Rev 4 (from the G1 and G2 reviews, built that way) records four points:
- **Design 3's member check.** It compares the shown member's base with the last draw's base, not flags at five codes. It keeps the bag only while the bag's chain reaches that base. Restore clears the record.
- **Design 5 step 3.** The inventory slot uses the inventory view's saved cells instead of `recount`.
- **Design 4.** It also mirrors in-place cursor draws.
- **Design 7's titles.** They carry the member the slots were last drawn for.

Rev 5 (from the G1 round 3–5 reviews, built that way) adds three points to Design 3:
- **A Save clears the record.** A Save writes the region back to its blocks and reads it again, so the party's objects come back in other slots. The next redraw shows the member's actor from the first row, as Original's redraw after a Save does.
- **A kept bag is the same object, not the same kind.** A container other than the base is kept only while its slot has not been freed since the last draw. Each slot counts its frees: a block's write-back and `remove` bump the count. A type word is not identity. A block written back and read again can put another bag of the same type, held by the same member, in the old slot.
- **The chain walk stops at a freed link** (a type word of 0), where `inventory::weight::holder` would walk on.

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

1. **The frame grows to 912 × 408 in Wide.** Three fixed slots, each 144 × 98, hold three live views:

   | Slot | Frame origin |
   |---|---|
   | roster | (8, 206) |
   | stats | (160, 206) |
   | inventory | (8, 310) |

   - Each slot's offset from `PANEL` is a multiple of 8 in x and y, so text copied into a slot stays on the HD font's grid (rev 3).

   - Nothing else draws in these rows or rects.
   - Original keeps 320 × 200. `HEIGHT` is the only frame constant; the page and native read the frame's size from the engine.
   - The frame area grows by 21%. The compositor receipt (`test-web.mjs` "Wide layout (spec 13 Design 6)") is measured again; above 8 ms the page asks S 2 by its own rule.
2. **A Wide redraw draws all three views.** On each `redraw_panel`, Wide draws each of roster, stats and inventory into the panel area, the current mode last. Each view is drawn with that view's `panel.mode` set, and the mode is restored after. After each draw, the panel rect (pixels and source plane, through `copy_px_within`) is copied into that view's slot. The copy of the current view is made after `panel_cursor`.
   - The extra draws leave every other state as a current-mode-only draw would leave it. The exception is the inventory context in Design 3.
   - A mode other than the three (spellbook, conversation, 0x9e) is drawn as today. The slots then keep their last views (see Design 6).
3. **The inventory context is kept.** In Wide, `inventory()` keeps `inventory_first` and `container`.
   - It resets the page and re-derives the container only when the shown member's base (the member, or `open_container`) differs from the last draw's, or after a Save (rev 5). This covers 0x90, 0x92 with a selection, 0x99, 0x9a and 0x91, and also the solo override, Drop's or Tab's leader, a conversation and a restore. A code that names the member already shown keeps the page (rev 4).
   - The container is kept only while its slot has not been freed since the last draw, or since the 0x9f / 0xa0 that opened it (rev 5), and its chain of parents reaches that base, as `inventory::weight::holder` walks it but stopping at a freed link (rev 5). A chain that ends on the map, at another actor or at the walk's limit starts the page and container again. So does a missing last draw. Restore clears the record (rev 4).
   - It also resets on 0x9f and 0xa0, as today.
   - So the slot's pixels and the cells a click reads are always the same page and container.
4. **Every change reaches the slots.** In Wide, one place sets `panel.dirty = true`: after each panel code is handled (`ui/wait.rs` `read_key`) and after each command completes. The draws that need no code, the keyboard panel cursor and the combat-mode line, copy the current view into its slot where they draw (rev 4). This covers the in-place draws and the mode-gated refreshes listed above. Under Design 3, the redraw keeps the page and the open bag.
5. **A click in a slot goes through `panel_click`.** Wide adds three click regions, one per slot, after the side panel's region. A press in a slot:
   1. moves the pointer to the same place in the panel area;
   2. sets `panel.mode` to the slot's view and the inventory context of Design 3;
   3. for the inventory slot, puts in the inventory view's cells as its last draw left them (rev 4: `recount` alone zeroes the list cells, `inventory/panel.rs:74`);
   4. calls `panel_click`, so the press check, the right-click repeat and the target-pending confirmation all run;
   5. keeps that mode until `read_key` has handled the code it returns;
   6. puts the pointer back where the press was;
   7. sets `dirty`. A press that returns no code redraws at once (the combat-mode line draws in place).

   The codes switch the panel as today.
6. **The active view, and other modes.**
   - The current mode is the active view; the page marks its window (title strip and border).
   - While the mode is none of the three, **or a conversation is active**, a 패널 window shows the real panel area and is the active window. The slots show their last views, dimmed, and a press in one is ignored until the mode is one of the three again.
   - While a conversation is active, no Wide slot redraw draws over the portrait in the panel area. The redraw after the Talk command restores the panel as today (rev 3).
7. **The page's windows** (spec 13's frames, drag and memory):

   | Window | Shows | Frame rect |
   |---|---|---|
   | 파티 | the roster slot | (8, 206, 144, 98) |
   | 능력치 | the stats slot | (160, 206, 144, 98) |
   | 인형 | the inventory slot's doll part: equipment, figure, Wielded, member icons | slot (0..71, 10..97) = frame (8, 320, 72, 88) |
   | 배낭 | the inventory slot's list part: container picture, 4 × 3 list, arrows, Carried, combat-mode icon | slot (72..143, 10..97) = frame (80, 320, 72, 88) |
   | 패널 | the panel area | only while Design 6 says |

   - The name row (rows 0..9) is left out. The 능력치, 인형 and 배낭 titles carry the name of the member the slots were last drawn for. In another mode or a conversation the engine's panel member is the other actor, so the page keeps the name of its last actor-view paint (rev 4).
   - 날짜, 메시지 and 명령 stay as in spec 13.
   - Default places do not overlap on 1920 × 1080: 파티 under 날짜 at the top right, 능력치 to its left, 인형 and 배낭 side by side under 능력치.
   - The engine adds two reads for the page: `panel_view()`, which returns [mode, member, actor, conversation active], and `panel_name()`, the shown member's name.
8. **The combat-mode icon** sits at the bottom of the 배낭 window this round. Moving it to the doll window (an L-shaped window) and showing several members' bags at once are later work.

## Tests

- **Engine, Wide:**
  - after a redraw in each of the three modes, each slot holds its view, pixels and source plane, and roster's sky bar is drawn;
  - a redraw's state equals a current-mode-only redraw's, field by field, except Design 3's context;
  - scroll past 12 items, then any dirty redraw: the slot shows page N, and `inventory_first == N`;
  - open a bag in the 배낭 slot, Use an item in it: the bag stays open;
  - Use and Drop through the command loop give the same result from the slot as from the panel;
  - a bag taken by another actor, a bag left on the map, a Restore, a Save, a block written back under the open bag, and another bag of the same type read into its slot each start the page and container again; the member's own list stays on its page while the member walks;
  - ready or unready by key while the roster is current: the inventory slot shows it in the same turn;
  - a press in each slot selects the same object, and gives the same ready/unready/use/drop result, as the same press in the panel in that mode;
  - the pointer is back after a slot press;
  - in another mode, a slot press is ignored.
  - each slot's offset from `PANEL` is a multiple of 8 (a const assert), and panel text in a slot passes `glyph_cell`;
  - during Talk, the panel area keeps the portrait.
- **Engine, Original:** no slot, no region, no frame change. Every existing test passes unchanged.
- **Web:**
  - the Wide layout lists 파티, 능력치, 인형, 배낭, 메시지, 명령 and 날짜, none overlapping at their default places, with the frame at 912 × 408 (the spec 13 literals updated);
  - 패널 shows only in another mode or during a conversation;
  - the active window is marked;
  - a click in each window reaches the engine at its frame rect's pixel;
  - the compositor receipt is re-measured.
- **Live:** B1–B3 on ultima6.2lab.ai as a fresh visitor at 1920 × 1080.

## Units (one agent each, DEV §3; the coordinates above are the contract)

| Unit | Scope |
|---|---|
| G1 | engine: `u6-render/src/lib.rs` (HEIGHT), `geom.rs` (slots, the frame assert), `ui/panels.rs`, `ui/click.rs`, `ui/pointer.rs`, `ui/wait.rs`, `ui/cursor.rs`, `inventory/panel.rs`, the command-completion hook; Designs 1–6 |
| G2 | page: `site/app.js`, `site/i18n.js`, `crates/u6-web/src/lib.rs` (`panel_view`), `tools/web/test-web.mjs`; Designs 6–7 |
