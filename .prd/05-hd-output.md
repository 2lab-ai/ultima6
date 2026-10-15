---
title: ultima6 — 05 output for HD / 4K
type: spec
tags: [ultima6, rendering, hd, ui]
---

# 05 — Output for HD / 4K: designed in P3, built later

Status: planned (rev 1, 2026-10-15). The seam is built in P3 (04 W0, W10, acceptance 7). The HD phase follows the research listed at the end.

The user's words are in `browser-port/ssot.md`: x2 or x4 resources, an HD or 4K map view, and much more of the map shown, "not now"; P3 should take the UI and the output into account in advance. The HD phase itself is not part of P3. P3 keeps that phase possible and leaves the parity output unchanged.

Measured (main `3349a83`):
- **Frame buffers.** The engine draws palette indices into two 320×200 frame buffers (`u6-render/src/lib.rs:7-9`; front and back, `engine.rs:251-262`). The page shows a 320×200 canvas, scaled with `image-rendering: pixelated` (`site/index.html:29,61`).
- **Map view.** The view is 11×11 cells of 16 px, of which pixels 8..168 are shown (`gfx/view.rs:3-8`; `map/mod.rs:61` `VIEW`).
- **Window cache.** The 40×40-tile window cache (`map/mod.rs:57`) also bounds:
  - sight and the light flood (`map/view.rs`);
  - which NPCs walk by steps and which are sent home (`actors/path.rs:528`).
- **Screen coordinates.** Panels and hit tests use fixed screen-pixel coordinates (e.g. `ui/panels.rs:16`, `ui/pointer.rs:172`).

Design:
1. **Two outputs.**
   - The 320×200 indexed frame stays the parity output (04 §Invariant).
   - Alongside it, the engine publishes a **scene** each time it presents:
     - the map view as cells: tile id, animation frame, object tiles in draw order, light and outline bits (what `ViewWindow` already holds, `gfx/view.rs:27`), plus the view centre;
     - the UI as widgets: kind, rect in logical units, and content ids.
   - A renderer at any scale draws from the scene instead of scaling pixels.
   - The scene lives only in memory, in the user's browser or process.
2. **Logical input.** Pointer events enter in logical units: the 320×200 space of the classic layout. An HD layout maps its own screen onto them, so the hit tests (`ui/click.rs`, `ui/pointer.rs`) do not change in P3.
3. **Assets by id.** Tiles, sprites, portraits and font glyphs are read through one provider keyed by kind, id and frame, at a scale.
   - The base is the user's files at 1×.
   - An **HD pack** is a mod content type (04 W0 schema, W6a limits): images per id at 2× or 4×, either drawn by an author or generated in the user's browser from the user's own files.
   - Art upscaled from the game's tiles is extracted content (04 §Rights). It is never hosted, never committed, and never part of a shareable package; only author-drawn images are.
4. **View size as a parameter.**
   - P3 changes neither `VIEW`, `WINDOW` nor `WINDOW_TILES`.
   - New code (scene, renderer, layout) takes the view size from state, not from a constant.
   - A wider view changes game behaviour: sight, light, the window cache and which NPCs walk by steps. Whether simulation follows the view is the HD phase's first research question.

Research after P4 (queued in `browser-port/loop.md`):
- pixel-art scalers and learned upscalers that run in the browser;
- prior art in other Ultima VI engines and remakes [추정 — to be checked];
- wide-view game semantics;
- a 16:9 UI layout and fonts;
- GPU rendering cost at 4K and 60 Hz;
- the rights of HD packs.
