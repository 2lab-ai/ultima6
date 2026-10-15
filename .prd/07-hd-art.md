---
title: ultima6 — 07 HD art (imagegen upscale + scaler baseline)
type: spec
tags: [ultima6, hd, rendering, imagegen]
---

# 07 — HD art: imagegen upscale with a scaler baseline

Status: planned (rev 2, 2026-10-15: gpt-6-astra review folded in: portrait and panel entries, a classic overlay mask for effects and text, scaling of the classic cell for outline and dirty cells, terrain-seam and animation-group gates, a defined performance reference). This is user item 1 (`hd-i18n-voice/ssot.md`, verbatim): "hd 이미지 업스케일을 이미지젠 스킬로 해주고 hd 업스케일해줘". It builds the seam designed in `05-hd-output.md` (scene output, assets by id) for the map, portraits and panels. The research notes are kept outside the repo: `~/Games/ultima6/research/2026-10-15/r1-hd-findings.md`.

## Measured facts

- **Every drawn map image is one of 2048 tile records** (`gfx/mod.rs:32`, `gfx/load.rs:16`).
  - Actor and object frames are `basetile[type] + frame`, so there is no separate sprite table.
  - The pack key is the record, not (tile, frame).
- **A drawn image is not a pure function of its record:**
  - shore merges rewrite records on even ticks (`gfx/anim.rs:90-116`);
  - `substitute` repoints `record_of` (`anim.rs:20-33`);
  - overlay masks OR a colour bit (`blit.rs:83-100`);
  - outline words repaint colour-0 pixels (`blit.rs:104-117`);
  - the clock hands, sparkles and dissolve write pixels directly.
- **`blit_back` (`gfx/blit.rs:49`) is the single choke point** for every tile drawn into the map window (`gfx/view.rs:114-168`). Portraits are 56×64 (`converse/script.rs:183`); panels and the world map come from `bitmaps`.
- **imagegen (Codex built-in `image_gen` via `codex exec -m gpt-6-astra`) repaints; it does not upscale.** It was measured on synthetic tiles drawn for the test, never on game art.
  - The 3 sample calls each returned 1536×1024 RGB, at 76–103 s per call. That size is an observation, not a guarantee: the tool checks the dimensions before slicing and refuses a sheet that does not match.
  - Kept well: the grid (content shift ≤ 0.25 source px at a 128 px pitch), structure (correlation 0.97–0.99), silhouettes (IoU 0.90) and a flat chroma key (worst 7 levels).
  - Kept poorly: colour (mean error ~14/255), 1-px dither (correlation 0.39), seams between tiles (a wrap border lowers them 35–65% but does not remove them), and 1-px animation motion (19–47% of it kept).
- **A pixel-art scaler is the deterministic baseline.** MMPX is MIT-licensed, has a Rust crate, keeps square corners and adds no colours. xBRZ is GPL-3 and not used.

## Rights (fixed)

- The imagegen tool is a native, local tool that runs on the owner's machine with the owner's own files. Running it sends the game's art to OpenAI; the owner asked for exactly that. The page never calls an image API (`connect-src 'self'`).
- Every output of the tool (sheets, slices, packs, reports that contain art) lives outside the repo under `~/Games/ultima6/hd/`. Packs are never committed, hosted or shared. The manifest carries `derived: true`, and the export path refuses derived packs.
- The scaler baseline runs in the visitor's browser on the visitor's own files, so the public site gains HD with nothing derived on the server.
- Tests use synthetic art only.

## Design

1. **HD0: scene tap and HD layer (engine and page, no AI).**
   - **Scene entries.** When HD is on, the engine appends scene entries per present. Each records what it drew, where, and the identity of the asset. The list costs nothing when HD is off, and the parity frame is untouched. Three kinds of entry:
     - **map cells:** `blit_back` appends `(word, x, y, record, dirty)` in the original's draw order, including the cursor and border. A record is "pristine" unless a shore merge touched it.
     - **portraits:** the portrait blit (`gfx/blit.rs:335`, called from `converse/script.rs`) appends `(portrait file, item, x, y, w, h)`.
     - **panel bitmaps:** the bitmap draws (paper, spellbook, world map) append `(role, x, y, w, h)`.
   - `u6-web` exports `scene_ptr`, `scene_len` and `scene_version`.
   - **Composition (`site/hd.js`, canvas S×, S = 2, 3 or 4 by device pixels).** Pointer mapping stays on the 320×200 logical size.
     1. Draw the 320×200 classic frame scaled ×S (nearest).
     2. Replay the entries, each with its HD image and clipped to its rect.
     3. Draw the classic overlay on top: the pixels of the final classic frame that differ from what the replaced image alone would have drawn there. This keeps everything drawn after or over an image visible exactly as the classic frame has it: text over a portrait or panel, the clock hands, sparkles, the dissolve, and outline flashes. The engine computes the overlay mask per entry from the classic pixels it already has, and exports it with the scene.
   - **Image source per entry:**
     - the pack image, if the pack has it and the content hash matches;
     - otherwise MMPX×4 built in wasm. For a dirty, outline or missing map cell, MMPX scales the 16×16 cell as the classic frame drew it (word and mask already applied), not the raw record. That keeps outline semantics and shore merges correct.
   - **Overlay shimmer** is drawn by the renderer: the HD image is lightened under the overlay mask by the record's palette delta, so one HD image per record is enough.
   - **Lifecycle.** An entry lives for one present. The HD layer redraws only when `scene_version` changes (06 O1).
2. **HD1: pack format and loading.** One zip containing:
   - `manifest.json`: `format: "u6-hdpack/1"`, `tile`, `scale`, `derived`, generator, and a base digest over the record hashes;
   - `index.bin`: per record id, content hash, atlas, column, row, flags;
   - `atlas-N.webp` (lossless RGBA);
   - optional `portrait/NNN.webp`, `bitmap/<role>.webp`.

   The page accepts it through the existing drop or file picker. The engine's intake ignores it. The page stores it in IndexedDB under `hd:*` (`site/store.js`) and reloads it on later visits. "Forget the HD pack" sits next to the other forget buttons. The page never fetches a pack. Limits follow 04 W6a: ≤ 150 MB decoded, bounded entries, and a malformed pack is refused with no change.
3. **HD2: the local imagegen tool (`tools/hd`, native Rust plus a driver).** Commands:
   - `inventory`: counts by kind, printed locally only.
   - `atlas`: builds sheets of 12×8 cells at 128 px (×8) per call, plus a cell map and `jobs.jsonl`. All frames of one actor go in one sheet; multi-record objects are composed into one mosaic; terrain uses wrap-border sheets.
   - `run`: one `codex exec -m gpt-6-astra` per sheet through the imagegen skill, 2–4 in parallel.
   - `ingest`: slices at the fixed pitch, keys out the background, then gates each cell.
   - `pack`: writes the pack.

   Gate per cell: structure correlation ≥ 0.9, shift ≤ 0.5 source px, silhouette IoU ≥ 0.85, key purity ≤ 12 levels.

   Two group gates on top of the per-cell gate:
   - **Terrain seams.** Each terrain record is tiled 3×3 against itself and against its common neighbours. Neighbours are counted from the map's adjacency on the owner's machine; the counts are printed locally only. The seam step must stay ≤ 1.5× the interior step.
   - **Animation sequences.** For each actor frame group (all its directions and frames), the frame-pair motion kept must be ≥ 50% of the source's.

   Thresholds are tuned on synthetic sheets first. A failing cell is re-rolled once. A failing group falls back to MMPX as a whole, so a group never mixes generated and scaled frames. The run order, best fit first:
   1. portraits;
   2. panel bitmaps;
   3. terrain;
   4. actors and objects.

   Colour transfer to the record's own palette histogram reduces drift. Output is ×8 on disk; the page builds a ×4 atlas at load.
4. **HD3, later:**
   - an HD shore merge (lerp of the two HD records under a soft mask);
   - outline flashes by alpha edge;
   - clock hands as strokes;
   - WebGL2 only if canvas 2D is too slow.
5. **Out of v1:**
   - the animated `u` opening (palette cycling, copper gradients);
   - a wider view (05 §4).

## Acceptance

1. **HD off.** Every parity receipt is bit-identical, and the scene list is empty.
2. **HD on with no pack.** The map window, portraits and panels show MMPX ×4. The pointer still hits the right cell. Synthetic tests check:
   - each entry's placement and clipping;
   - that the overlay mask keeps text over a portrait, the clock hands, a sparkle and an outline-flash cell exactly as the classic frame has them;
   - that an outline-word cell's fallback equals the scaled classic cell.

   A headless capture is read by the dispatcher.
3. **The owner's pack.** It is built by `tools/hd` from the owner's files with imagegen and loaded by drop.
   - Portraits and panels show the imagegen art.
   - Map cells and groups that failed a gate show MMPX, and no actor group mixes the two.
   - The tool prints a local coverage report: records by kind, as generated, fallback or failed.
   - The pack survives a reload.
   - The pack is not in git, and the site serves no pack file (checked by a test over `site/` and `git ls-files`).
4. **Performance.** Reference: headless Chromium on the owner's Mac (M5 Max), 60 Hz rAF, 4× speed, a castle walk with MMPX updates for dirty cells and a full pack loaded. The HD composite, including scaler updates and the overlay, adds ≤ 4 ms per rAF at the median and ≤ 8 ms at p95. The page draws a frame only when it changed (06 O1).
5. **Gates and live check.** All gates pass. On the live site, HD switches on and off with no foreign requests.
