---
title: ultima6 — 05 output for HD / 4K
type: spec
tags: [ultima6, rendering, hd, ui]
---

# 05 — Output for HD / 4K: what shipped in 07, and the wider view still to come

Status: rev 2, 2026-10-16. Rev 1 (2026-10-15) designed a seam for P3 to build (04 W0, W10, acceptance 7). It is superseded for resources ×2/×4: spec 07 built the seam in another form and shipped it (H0, H0b, H1, H3 in production 80cdffb2). Rev 1's scene output was not adopted; 07 rev 3 replaced it, measured, with a per-pixel source plane. This file now records how the shipped seam meets the user's three asks, and keeps the one ask still open: a wider map view (§4) and its research. Nothing here is built in P3.

The user's words are in `browser-port/ssot.md`: x2 or x4 resources, an HD or 4K map view, and much more of the map shown, "not now"; P3 should take the UI and the output into account in advance.

Measured (main `eb71371`):
- **Frame buffers.** The engine draws palette indices into 320×200 frame buffers (`u6-render/src/lib.rs`; back and front at `engine.rs:108-109`, the shown frame at `engine.rs:536`). With HD off the page shows that frame on a 320×200 canvas, scaled with `image-rendering: pixelated`.
- **Map view.** The view is 11×11 cells of 16 px, of which pixels 8..168 are shown (`gfx/view.rs:3-8`; `map/mod.rs:61` `VIEW`).
- **Window cache.** The 40×40-tile window cache (`map/mod.rs:57`) also bounds:
  - sight and the light flood (`map/view.rs`);
  - which NPCs walk by steps and which are sent home (`actors/path.rs:503-529`).
- **Screen coordinates.** Panels and hit tests use fixed screen-pixel coordinates (e.g. `ui/panels.rs:16`; the click regions at `ui/pointer.rs:19-23`).

Design:
1. **Two outputs — shipped in 07, as a source plane, not a scene.**
   - The 320×200 indexed frame stays the parity output (04 §Invariant).
   - Each frame buffer carries a per-pixel source plane (`Src`, `u6-render/src/lib.rs:89`): which record, portrait or panel bitmap the pixel came from, with its version and position. The HD compositor reads the shown frame and its plane and writes a 320·S × 200·S frame (`u6-render/src/hd.rs:499`, S = 2 or 4). Why not a scene: the drawn pixel is not a pure function of the records (the shore merge rewrites records in place; overlay, outline, clock, sparkles and missiles write pixels directly), and a scene list cannot follow copies, offsets, quakes, dissolves and pixels that persist in the front buffer (07 Measured facts, 07 rev 3).
2. **Logical input — shipped.** The pointer maps through the canvas box onto the game's 320×200 pixels whatever the canvas size (`site/app.js:914`), so the hit tests do not change.
3. **Assets by id — the format shipped (`u6-hdpack/1`, 07 H3); the page's pack load and the compositor's lookup are 07 H2, in progress.** HD images are keyed by the content hash of the classic record, portrait or bitmap they were made from (07 Design 2). A record whose bytes differ, such as a merged shore, falls back to MMPX of its own current pixels. Rights as in 07: art upscaled from the game's files is never hosted, committed or shared; the manifest says `derived` and the export path refuses it.
4. **View size — still open, not built in P3.**
   - P3 changes neither `VIEW`, `WINDOW` nor `WINDOW_TILES`, and no P3 code reads a view size.
   - A wider view changes game behaviour: sight, light, the window cache and which NPCs walk by steps. Whether simulation follows the view is the first research question of the wider-view phase.

Research for the wider view, after P4 (queued in `browser-port/loop.md`):
- prior art in other Ultima VI engines and remakes [추정 — to be checked];
- wide-view game semantics;
- a 16:9 UI layout and fonts;
- GPU rendering cost at 4K and 60 Hz;
- (the scaler and the HD packs' rights are settled in 07.)
