---
title: ultima6 — 07 HD art (imagegen upscale + scaler baseline)
type: spec
tags: [ultima6, hd, rendering, imagegen]
---

# 07 — HD art: imagegen upscale with a scaler baseline

Status: planned (rev 1, 2026-10-15). This is user item 1 (`hd-i18n-voice/ssot.md`, verbatim): "hd 이미지 업스케일을 이미지젠 스킬로 해주고 hd 업스케일해줘". It builds the seam designed in `05-hd-output.md` (scene output, assets by id) for the map, portraits and panels. The research notes are kept outside the repo: `~/Games/ultima6/research/2026-10-15/r1-hd-findings.md`.

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
  - The output is always 1536×1024 RGB, 76–103 s per call.
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
   - When HD is on, `blit_back` also appends `(word, x, y, record, dirty)` to a per-present draw list owned by `Io`. This is the 05 scene for the map, in the original's draw order, including the cursor and border. The parity frame is untouched, and the list costs nothing when HD is off.
   - A record is "pristine" unless a shore merge touched it.
   - `u6-web` exports `scene_ptr`, `scene_len` and `scene_version`.
   - `site/hd.js` draws on a canvas S× larger (S = 2, 3 or 4, chosen by device pixels):
     - the 320×200 frame scaled ×S (nearest);
     - then the list replayed with `drawImage(atlas, rect)`, clipped to the map window.
   - Pointer mapping stays on the 320×200 logical size.
   - Record source:
     - the pack image if the pack has the record and the record's content hash matches;
     - otherwise the MMPX×4 image built in wasm from the live record pixels.
   - Dirty, outline and missing records always take the MMPX path.
   - Overlay shimmer is drawn by the renderer: the HD image is lightened under the overlay mask by the record's palette delta, so one HD image per record is enough.
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

   Gate per cell: structure correlation ≥ 0.9, shift ≤ 0.5 source px, silhouette IoU ≥ 0.85, key purity ≤ 12 levels. Thresholds are tuned on synthetic sheets first. A failing cell is re-rolled once, then falls back to MMPX. The run order, best fit first:
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

1. **HD off.** Every parity receipt is bit-identical, and the draw list is empty.
2. **HD on with no pack.** The map window and portraits show MMPX ×4. The page is checked by screenshot comparison on synthetic state, plus a headless capture that the dispatcher reads. The pointer still hits the right cell.
3. **The owner's pack.** It is built by `tools/hd` from the owner's files with imagegen and loaded by drop. Portraits and panels show the imagegen art; map cells that failed the gate show MMPX. The pack survives a reload. The pack is not in git, and the site serves no pack file (checked by a test over `site/` and `git ls-files`).
4. **Performance.** At 4× speed the HD layer adds ≤ 4 ms per rAF at 60 Hz. The page draws a frame only when it changed (06 O1).
5. **Gates and live check.** All gates pass. On the live site, HD switches on and off with no foreign requests.
