---
title: ultima6 — 13 Wide mode (60 × 60 simulation, a map that fills the screen, floating windows)
type: spec
tags: [ultima6, hd, ui, layout, simulation]
---

# 13 — Wide mode: the map grows, the simulation grows with it, the panels float

Status: rev 1, 2026-10-23. Supersedes spec 10's layout for the new mode; spec 10 stays the layout of Original mode. Answers spec 05 §4 (the wider view, left open). Owner's words: `hd-layout-font-register/ssot.md` §Round 2.

## Clause map (owner's words → what proves it)

| Owner, verbatim | Acceptance |
|---|---|
| "지금 11x11 표시하고 있나 그걸 늘려달라고" | A1: Wide live capture at 1920 × 1080 shows 36 × 20 whole map cells plus the half cells at the edges, counted from the engine's view export, and the map fills the canvas |
| "시물레이션을 60x60으로 늘리고" | A2: the engine's window side is 60; an NPC 25 cells east and south of the leader, and one 25 cells west and north, each walks steps in Wide and does not in Original |
| "뷰포트를 2배로 늘려줘 21x21 정도로" | A1 (21 rows), Decision 1 |
| "원본 모드랑 신규 모드 둘다 지원하도록해줘" | A3: the Original build passes every gate it passes today, unchanged; a live capture of Original matches deploy 9's layout; the setting switches between them |
| "nuvie.png 여기처럼 구현해줘 울티마7 처럼 ui 변경" | A4: full-frame capture: the map under at least four framed, titled windows (side panel, message scroll, command bar, date and sun bar) |
| Round 1, item 1: "전체 화면 모드 지원 … 화면이 크면 알아서 스트레칭" | A5: fullscreen at 2560 × 1440 fills the screen in both modes |

## Decisions (reversible; the owner vetoes on review)

1. **The Wide view is 37 × 21 cells, not 21 × 21.** The owner said "21x21 정도" and pointed at nuvie.png, where the map fills a 16:9 screen; 21 rows at 16:9 is 37 columns. `VIEW_W` and `VIEW_H` are separate constants, so 21 × 21 is a one-line change.
2. **The mode is a build, chosen at boot.** A cargo feature `wide` on u6-core, u6-render, u6-web and u6-native; the deploy builds `site/pkg` (Original) and `site/pkg-wide`. Settings and the first-run chooser carry "화면: 와이드 / 원본"; a new visitor gets Wide. Switching reloads the page. Saves carry over: no window- or view-sized state is saved (`saveload/save.rs:101`, `restore.rs:88` force a window rebuild). Why a build and not a runtime switch: Original stays value-identical by construction (the constants keep today's values), and the 238 size sites need no geometry passed through them.
3. **Spell effect areas stay the original 11 × 11 box round the leader** (tremor, slime, wing strike, codex, a party member's "near", rest's others-near, all-foes ±7): what the game calls "the screen" for balance. The player will see a tremor spare monsters at the far edge.
4. **Targeting stays in the original 11 × 11 box round the leader.** The cursor stops at ±5 cells; ranges and the 11 × 11 spread table (`combat/tables.rs:61-63`) stay valid. The player will see the cursor stop short of a visible enemy.
5. **Visibility follows the wide view.** Anything the game does "where the player can't see": ambush spawns, eggs not hatching on screen (`map/region.rs:23` 8 → half view + 3 per axis), sending a walker home only when unseen (`actors/path.rs:287-297`), idle frames, hit flashes, dissolves, scatter-until-unseen, the dragon entering at the view's edge, and moves marking the view dirty.
6. **Simulation radii follow the window.** In-window tests (0x27, 0x28, 39, 20), temporaries dropped beyond `WINDOW/2`, `prune_far`'s `KEEP`. The view lies inside the window, so an actor on screen always takes its turns. Nuvie leaves the active range at 40 and its actors can stand still on screen (research below); this mode does not.
7. **Durations stay; rates scale.** Dissolves, flashes and refreshes take the same time over the bigger map.

## Geometry (one module: `u6-core/src/geom.rs`; the frame size in `u6-render`)

| | Original | Wide |
|---|---|---|
| view (`VIEW_W` × `VIEW_H`) | 11 × 11 | 37 × 21 |
| half view (`HALF_W`, `HALF_H`) | 5, 5 | 18, 10 |
| window (`WINDOW`) | 40 | 60 |
| window lead (`LEAD`): origin = `(c − LEAD) & 0x3f8` | 16 | 26 |
| leader's window cell | 16..23 | 26..33 |
| view-offset mask | 0x1f | 0x3f |
| inner sight block, per axis: lo = `(LEAD − HALF − 1) & !7`, hi = `(LEAD + 7 + HALF + 1 + 8) & !7` | 8..32 | x 0..56, y 8..48 |
| frame (`WIDTH` × `HEIGHT`) | 320 × 200 | 912 × 336 |
| map cell (0, 0) at frame pixel (`MAP_X`, `MAP_Y`) | (0, 0) | (320, 0) |
| shown map rect (`SHOWN`) | (8, 8)–(167, 167) | (328, 8)–(903, 327) |
| sight queue | 256, u8 | 1024, u16 |

- The UI keeps its 320 × 200 coordinates in both modes; in Wide it is the frame's left 320 columns and the map is beside it. Only map writers learn `MAP_X`/`MAP_Y`. Nothing else draws where Original's map was.
- `WINDOW` = 60 is not a multiple of the 8-cell chunk: `fill_window` (`map/region.rs:138-150`) copies the last column and row of chunks clipped to 60.
- Dungeon levels (z ≥ 1, 256 wide): keep today's edge behaviour (`chunk_ref`, `world_mask`) and test it at a level's edge in Wide.
- The origin formula appears six times (`map/region.rs:91-92`, `actors/path.rs:36-38`, `actors/turn.rs:427-428`, `actors/rest/mod.rs:198-199`, `main_loop/main_hunk.rs:77-78`, `map/view.rs:316`); all six call one `geom` function.

## Design

1. **Sizes become constants.** Every site the inventory (`~/Games/ultima6/work/dispatch/r1023-wide/inventory.md`) marks Y reads `geom`. A site of Decision 3 or 4 written leader-relative (`cx − 5..=cx + 5`) keeps its ±5 (named `RULE_HALF`); a site in view-cell coordinates (`(5, 5)`, `0..11`, `row * 11 + col`) becomes centre-relative.
2. **Silent limits scale too.** The draw-list pool (`objects/draw.rs:13,26-28`, 256 nodes, u8 links) and the path pool (`actors/work.rs:150`) would drop objects and paths without a panic: Wide sizes them by cell count with u16 links. The light table (`map/view.rs:130-141`, 8 × 8) saturates beyond 7, which Original never reaches. Walk steps from the game image (`map/tables.rs:74-89`, checked against `dy * 40 + dx`) are decoded with stride 40 and re-encoded with `WINDOW`.
3. **Wider types.** `pointer_origin` (`inventory/mod.rs:83`) and the push target pixel (`objects/commands/push.rs:135-136`) become u16; map pixels in Wide reach 903.
4. **The UI clip.** In Wide a UI writer that ran past x = 319 would draw into the map; each unit checks its writers near x 312..320.
5. **The page.** A Wide layout table beside spec 10's: the shown map rect covers the whole canvas (1920 × 1080, cover); the engine's side panel, message scroll, command bar and date line are cropped from the UI area into framed windows over the map, U7-style (a frame, a title, no Nuvie art). The pointer maps per window through spec 10's region machinery. Whole-screen frames (save, death, ending) show the 320 × 200 UI area centred, as `LAYOUT_WHOLE` does. Drag and collapse, with positions kept in the layout key, ship when ready and do not hold deploy 10.
6. **HD.** The compositor composes the 912 × 336 frame at S = 4 (3648 × 1344; it is incremental, `u6-render/src/hd.rs:856-868`). Measure the time per frame on the live check; above 8 ms median, Wide composes at S = 2.
7. **Deploy 11** (not this spec's acceptance): Nuvie's new style proper: separate paper-doll, attributes and backpack windows open at once, which needs the engine to draw those views into their own regions.

## Tests

- Tests that assert a size derive it from `geom` and run in both builds; only comparisons with the real game (parity, oracle, `plane_parity`) stay Original-only.
- Each unit adds one test that fails if `VIEW_W` and `VIEW_H` are swapped.
- The gate runs twice: `scripts/cargo.sh test --workspace --profile gate` and the same with `--features wide`; the web tests run against both packages.
- Rights as everywhere (`01-spec.md` §Rights): expectations come from the owner's files; tests print labels and counts only.

## Units (one agent each; `hd-layout-font-register/loop.md` §Round 2)

| Unit | Files | Needs |
|---|---|---|
| W0 | `geom.rs`, features, frame size, gate lines | — |
| W1 | `map/*` | W0 |
| W2 | `objects/*`, `actors/*`, `main_loop/main_hunk.rs` | W0 |
| W3 | `gfx/*`, `combat/*`, `magic/*` | W0 |
| W4 | `ui/*`, `inventory/*`, `main_loop/commands.rs`, `converse/*`, `usecmd/*` | W0 |
| W5 | `u6-render` compose, `u6-web`, `u6-native`, `site/*`, `scripts/build-web.sh`, `scripts/deploy-web.sh` | W0 (page work can start at once) |

## Prior art: Nuvie / ScummVM (scummvm@e0ddc60, read 2026-10-23)

- `game_style` original / new / original+ / original+_full_map; the map window is the game width / 16, rounded to odd; original+ keeps the 320 × 200 panel at the right edge, full_map draws it over the map; new style has only the sun and moon ribbon plus gumps (`core/game.cpp:107-121,448-459`, `gui/widgets/map_window.cpp:163-190`, `views/view_manager.cpp:82-122`).
- Actors take turns within 0x27 of `(player − 16) & !7`, whatever the view (`scripts/u6/actor.lua:1878-1899`). A walker outside that box is sent home only when its home is off screen, so in a wide view it can stand still on screen. No report of it found; derived from the code.
- Eggs hatch within 20 tiles; the "not within 8" guard is dropped for every wide style (`core/egg_manager.cpp:126-132`), so monsters appear on screen. Sight floods the window + 3 cells; the light map is window-sized (`map_window.cpp:723,762-775,1334-1377`).
