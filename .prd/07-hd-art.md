---
title: ultima6 — 07 HD art (imagegen upscale + scaler baseline)
type: spec
tags: [ultima6, hd, rendering, imagegen]
---

# 07 — HD art: imagegen upscale with a scaler baseline

Status: planned (rev 3, 2026-10-15. rev 2 folded in the gpt-6-astra review. rev 3 folds in the opus review, checked against the code: the scene list is replaced by a per-pixel source plane in the framebuffer, which follows every copy, offset, effect and persistence by construction. rev 4 folds in the gpt-6-astra re-review: each source carries the image version it was drawn from, and the plane is kept whenever the web build runs, so switching HD on loses nothing. rev 5, 2026-10-16, folds in what H0 and H3 measured: a pixel is current only when its `ver` equals the record's version, S is 2 or 4, the pack stores ×4, and portraits are gated on their layout). This is user item 1 (`hd-i18n-voice/ssot.md`, verbatim): "hd 이미지 업스케일을 이미지젠 스킬로 해주고 hd 업스케일해줘". It builds the seam designed in `05-hd-output.md` (scene output, assets by id) for the map, portraits and panels. The research notes are kept outside the repo: `~/Games/ultima6/research/2026-10-15/r1-hd-findings.md`.

## Measured facts

- **Every drawn map image is one of 2048 tile records** (`u6-formats/src/gfx/mod.rs:31` `TILE_COUNT`, loaded by `u6-core/src/gfx/load.rs:16`).
  - Actor and object frames are `basetile[type] + frame`, so there is no separate sprite table.
  - The pack key is the record, not (tile, frame).
- **A drawn image is not a pure function of its record:**
  - shore merges rewrite `records[dst]` in place on every even tick (`u6-core/src/gfx/anim.rs:90-117,150-152`), so the 32 shore records are never pristine after the first even tick;
  - `substitute` repoints `record_of` (`anim.rs:20-30`);
  - overlay masks OR a colour bit, by the tile's masktype (`blit.rs:83-100`, `load.rs:173-181`);
  - outline words repaint colour-0 pixels (`blit.rs:104-117`);
  - the clock hands, sparkles, the dissolve and missile sprites write pixels directly (`gfx/sprite.rs:182-212`); combat flash and quake frames are replayed as pixel copies (`combat/present.rs:347-395`).
- **Every pixel write goes through `Framebuffer`** (`u6-render/src/lib.rs:27-105`: `set`, `clear`, `blit`, `clone_from`). In `u6-core`, record pixels are written by `gfx::blit` (`blit.rs:68-117`) for both `blit_back` and `blit_front`, bitmaps by `blit_bitmap` (`blit.rs:335-356`), and the buffers are copied pixel by pixel by `copy_pixel` (used by `copy_window` and the dissolve, `blit.rs:181,261-267`) and `copy_rect` (`blit.rs:275-305`).
  - `blit_back` is the only record draw into the back buffer, and it is also a scratch compositor: inventory icons, `tile_on_blank` and the sky bar are drawn there and then copied elsewhere with `copy_rect` (`inventory/panel.rs:137-178`, `ui/panels.rs:65-71,440-457`). The quake shifts the window copy (`ui/effects.rs:27`).
  - UI tiles are drawn with `blit_front` (the status bar, panels, inventory, spellbook tiles, the cursor).
  - The front buffer persists across presents (`engine.rs:251-271`), and publishing happens once per tick (`engine.rs:505-513`). The pointer sprite is drawn into `shown` after publishing (`engine.rs:518-521`).
- **Portraits and panels.** Portraits are 56×64, loaded by `load_portrait` (`converse/script.rs:189-215`) and drawn by `draw_portrait` (`ui/seams.rs:31`, `converse/script.rs:226`) through `blit_bitmap`. The panel bitmaps drawn are the paper (`main_loop.rs:369`) and the spellbook (`magic/book.rs:171`). The world map is never drawn.
- **The page sends every non-save drop to `importFiles`**, which starts a new game from it (`site/app.js:403-450`).
- **imagegen (Codex built-in `image_gen` via `codex exec -m gpt-6-astra`) repaints; it does not upscale.** It was measured on synthetic tiles drawn for the test, never on game art.
  - The 3 sample calls each returned 1536×1024 RGB, at 76–103 s per call. That size is an observation, not a guarantee: the tool checks the dimensions before slicing and refuses a sheet that does not match.
  - Kept well: the grid (content shift ≤ 0.25 source px at a 128 px pitch), structure (correlation 0.97–0.99), silhouettes (IoU 0.90) and a flat chroma key (worst 7 levels).
  - Kept poorly: colour (mean error ~14/255), 1-px dither (correlation 0.39), seams between tiles (a wrap border lowers them 35–65% but does not remove them), and 1-px animation motion (19–47% of it kept).
- **A pixel-art scaler is the deterministic baseline.** MMPX is MIT-licensed, has a Rust crate, keeps square corners and adds no colours. xBRZ is GPL-3 and not used.

## Rights (fixed)

- The imagegen tool is a native, local tool that runs on the owner's machine with the owner's own files. Running it sends the game's art to OpenAI. The authority is the user's own request, "hd 이미지 업스케일을 이미지젠 스킬로 해주고" (`hd-i18n-voice/ssot.md`); see Tensions. The page never calls an image API (`connect-src 'self'`).
- Every output of the tool (sheets, slices, packs, reports that contain art) lives outside the repo under `~/Games/ultima6/hd/`. Packs are never committed, hosted or shared. The manifest carries `derived: true`, and the export path refuses derived packs.
- The scaler baseline runs in the visitor's browser on the visitor's own files, so the public site gains HD with nothing derived on the server.
- Tests use synthetic art only.

## Design

1. **HD0: source plane and HD compositor (engine and page, no AI).**
   - **Source plane.** `Framebuffer` gains an optional plane of one `Src` per pixel. `u6-web` enables it when a game starts, so it exists whether or not HD is shown; native and the tests leave it off unless a test asks. With it absent, nothing changes and no work is done; the parity frame is untouched in both cases.
     - `Src` is `{kind, id, ver, u, v, flags}`:
       - `kind` is classic, tile, portrait or bitmap;
       - `id` is the record number, the portrait (letter, item), or the bitmap role (paper, spellbook);
       - `ver` is the image's version when the pixel was drawn. A record's version starts at 0 and increases each time its bytes are rewritten in place: the shore merge, `anim.rs:116`, is the one writer. Portraits and bitmaps never change while loaded, so they stay at 0;
       - `(u, v)` is the pixel's position inside that image;
       - `flags` marks a pixel whose colour the blit changed: the overlay OR and the outline.
     - `set`, `clear` and `blit` mark the pixel classic.
     - New `set_src(x, y, index, src)` is used by `gfx::blit` for record pixels and by `blit_bitmap` for portraits and panels. Those two take the source identity from their callers: `draw_portrait` passes (letter, item) from `load_portrait`, and `draw_paper` and `draw_book` pass their role.
     - New `copy_px(from, i, j)` copies the colour and the source together. It is used by `copy_pixel`, `copy_rect` and the dissolve.
     - The published frame and `shown` carry the plane. Each plane marks the 16-pixel chunks it writes; publishing (front → published → shown) and the window present (back → front, the window rect) copy only the marked chunks of the plane (`sync_from`, `sync_rect_from`), with the colours copied exactly as before. A sync consumes its source's marks, so each plane has exactly one sync consumer and one source; every build checks this at each sync (a second consumer is a panic, in release too, H0b), and the gate profile also checks that the planes are equal after every sync. The HD compositor therefore reads the shown frame (`Engine::frame()`) and never syncs from front or published. This keeps the plane's cost to about +4 µs per idle tick (H0 measured +32 µs with whole-plane copies). Copies of earlier pixels keep their sources too: the text scroll, a flight's put-back block, and combat playback with its last picture. Their saved sources compare equal whatever they hold, so `State` equality is unchanged.
   - **What this buys.**
     - The source follows every copy, offset, quake shift, dissolve and persisted front-buffer pixel by construction. That covers the scratch compositing at back (0,0) and (0,4), and a frame left on screen by a skipped render (06 O2).
     - Everything drawn directly (text, clock hands, sparkles, a missile's own pixels, the pointer sprite) is classic. It is shown as the classic pixel scaled, so it can never be painted over. Pixels put back from a saved copy (the text scroll, a missile's put-back block, combat playback frames and the last picture) carry the sources saved with them, so an attributed pixel stays attributed and a classic one stays classic.
     - No list and no mask are needed. The one lifetime rule left is the image version: a pixel whose `ver` differs from its record's current version was drawn from bytes that no longer exist, so the compositor shows it classic until the game redraws it. The test is equality, not "older": a relaunch replaces the State, and its versions restart, so the relaunch also resets every plane to classic (its colours untouched), and no old pixel can match a new version.
   - **Compositor (`u6-render`, in wasm).** It runs when its input key changes: `frame_version` (bumped on every change of `shown`'s colours), `src_version` (bumped when only `shown`'s sources change, e.g. combat playback ending on a picture whose colours equal the last one's) and `hd_version` (bumped by the HD toggle, a scale change, and a pack load or forget).
     - It writes an S×-sized RGBA frame, S = 2 or 4: MMPX is a 2× scaler and 4× is MMPX applied twice. The page picks 4 when the canvas's device-pixel size is at least 3× the logical 320 × 200, else 2.
     - For each screen pixel, it writes an S×S block:
       - a tile, portrait or bitmap pixel without flags and with a current `ver` takes the block `(u·S.., v·S..)` of that image's HD version;
       - an HD sub-pixel that is transparent takes the classic colour;
       - a classic, flagged or stale pixel takes its classic colour.
     - `u6-web` exports `hd_frame_ptr`, `hd_frame_len`, `hd_scale` and `set_hd(on, scale)`. The page draws that frame to a canvas sized S× and keeps pointer mapping on the 320×200 logical size.
   - **HD image per id:**
     - the pack image, if the pack has one whose content hash matches the record's current bytes;
     - otherwise MMPX×S of the record's own current content, with its transparency. Merged shores and substituted frames are covered this way. The result is cached by content hash, because the water and shore records cycle.
   - **Toggle.** "HD" next to Mute, on by default, kept in `localStorage`. With it off, the page draws the classic frame as today and the compositor does not run; the plane keeps being written, so switching on again is exact at once.
2. **HD1: pack format and loading.** One zip containing:
   - `manifest.json`: `format: "u6-hdpack/1"`, `tile`, `scale`, `derived`, generator, and a base digest over the record hashes;
   - `index.bin`: per record id, content hash, atlas, column, row, flags;
   - `atlas-N.webp` (lossless RGBA);
   - optional `portrait/<letter>-<item>.webp` (the `load_portrait` pair) and `bitmap/<role>.webp` (`paper`, `spellbook`);
   - every image's entry in `index.bin` names its kind, id, the content hash of the classic image it was made from, and its size. Tiles are 16·4 px square; portraits and bitmaps are 4× their classic size (the page's largest S; the tool keeps its ×8 slices as local masters and writes the pack at ×4, so a full pack of about 80 MB decoded fits the limit, where ×8 would need over 300 MB). An image whose hash does not match the owner's classic image is ignored, and the MMPX fallback is used.

   The page accepts it through the existing drop or file picker. The page classifies a drop before `importFiles`: a zip with a `manifest.json` whose `format` is `u6-hdpack/1` goes to the HD loader and never starts a game. The page stores it in IndexedDB under `hd:*` (`site/store.js`) and reloads it on later visits. "Forget the HD pack" sits next to the other forget buttons. The page never fetches a pack. The limits are written here, because 04 W6a is not built (07 HD0–HD1 replace 04's former W10 and the HD-pack part of W6a, its format and limits; W6a's mod resolver, save sidecar and package limits stay in 04): ≤ 150 MB decoded, ≤ 4096 entries, each image exactly 16·4 px square for tiles or 4× its classic size for portraits and bitmaps; a malformed pack is refused with no change.
3. **HD2: the local imagegen tool (`tools/hd`, native Rust plus a driver).** Commands:
   - `inventory`: counts by kind, printed locally only.
   - `atlas`: builds sheets of 12×8 cells at 128 px (×8) per call, plus a cell map and `jobs.jsonl`. All frames of one actor go in one sheet; multi-record objects are composed into one mosaic; terrain uses wrap-border sheets.
   - `run`: one `codex exec -m gpt-6-astra` per sheet through the imagegen skill, 2–4 in parallel.
   - `ingest`: slices at the fixed pitch, keys out the background, then gates each cell.
   - `pack`: writes the pack.

   Gate per cell: structure correlation ≥ 0.9, shift ≤ 0.5 source px, silhouette IoU ≥ 0.85, key purity ≤ 12 levels. Portraits measure structure at the layout scale: both images box-averaged to 1/4 of the source size, then correlated, still ≥ 0.9. At the source pixel a faithful repaint scores 0.55–0.66, measured by H3 on synthetic portraits: 1-px features become drawn ones. Negative controls on synthetic portraits (mirrored, shifted by 1/8, two portraits swapped) must each fail a gate.

   Two group gates on top of the per-cell gate:
   - **Terrain seams.** Each terrain record is tiled 3×3 against itself. The seam step must stay ≤ max(1.5, the classic tile's own seam ratio) × the interior step: a tile whose own wrap already steps cannot be asked to step less, and the generated one may not step more than both.
   - **Animation sequences.** For each actor frame group (all its directions and frames), every frame pair that moves in the source must keep ≥ 50% of its motion (the minimum over pairs, not the mean, so one exaggerated pair cannot hide frozen ones).

   Thresholds are tuned on synthetic sheets first. A failing cell is re-rolled once. A failing group falls back to MMPX as a whole, so a group never mixes generated and scaled frames. The run order, best fit first:
   1. portraits;
   2. panel bitmaps;
   3. terrain;
   4. actors and objects.

   Colour transfer to the record's own palette histogram reduces drift. Sheets and slices are ×8 on disk in the work dir; the pack is written at ×4, and the page downsamples it to ×2 for S = 2.
4. **HD3, later:**
   - an HD shore merge: the merge's single mask selecting between the HD versions of its two sources (all five planes share one mask, `anim.rs:42-60`); until then, coastlines show MMPX;
   - HD tints for the overlay OR and outline flashes, which v1 shows as classic pixels;
   - clock hands as strokes;
   - WebGL2 only if the wasm compositor is too slow.
5. **Out of v1:**
   - the animated `u` opening (palette cycling, copper gradients);
   - a wider view (05 §4).

## Tensions

- `01-spec.md` §Rights says game bytes are never uploaded. The imagegen run in HD2 uploads the owner's art from the owner's machine to OpenAI. Resolution: the exception is the user's own explicit request for this item (quoted in Rights). It applies only to the local tool run by the owner, and its outputs stay local. The page and the hosted site keep the rule unchanged.

## Acceptance

1. **HD off.** Every parity receipt is bit-identical. Native and the tests allocate no plane; the web build keeps writing it, and its classic frame is unchanged by it.
2. **HD on, no pack.** The map, UI tiles, portraits and panels show MMPX ×S, and the pointer still hits the right cell. Synthetic tests check:
   - that the plane follows `copy_window`, `copy_rect` at an offset, the dissolve, the quake shift and a front pixel that persists across presents;
   - that text over a portrait, the clock hands, a sparkle, a missile and the pointer sprite stay classic;
   - that an outline-flash and an overlay pixel are flagged;
   - that a merged shore record falls back to MMPX of its current bytes;
   - that a pixel drawn from a record, then left in place while the record is rewritten, then copied and presented, is shown classic (stale `ver`);
   - that switching HD off and on over a persistent panel and a skipped render, and loading a pack or changing the scale with unchanged game pixels, recompose the frame exactly;
   - the final S× RGBA, not only the plane, for overlapping HD art, an outline flash, an overlay pixel, the pointer and a combat replay;
   - coverage: on a synthetic scene of tiles, a portrait and a panel, the expected pixels are attributed and none of them is classic.
3. **Source receipt (consistency).** On every published frame of the real-data receipts, run locally:
   - every unflagged tile, portrait or bitmap pixel with a current `ver` equals the classic pixel of its image at `(u, v)`;
   - the share of map-window pixels attributed to a tile is printed, with the counts only.

   The receipt prints pass/fail and those counts. It checks that the attributed pixels are right; coverage is checked by the synthetic tests above.
4. **The owner's pack.** `tools/hd` builds it from the owner's files with imagegen, and the owner's browser loads it by drop.
   - Portraits and panels show the imagegen art.
   - Coastlines show MMPX (HD3).
   - Actor groups are expected to be mostly MMPX, because r1 measured 19–47% of 1-px motion kept against the ≥ 50% gate.
   - No actor group mixes generated and scaled frames.
   - The tool prints a local coverage report: records by kind, as generated, fallback or failed.
   - The pack survives a reload.
   - The dispatcher reads the frames on the owner's machine.
   - The pack is not in git, and the site serves no pack file. A test over `site/` and `git ls-files` checks this. A dropped pack never starts a game.
5. **Performance.** Reference: headless Chromium on the owner's Mac (M5 Max), 60 Hz rAF, 4× speed, a castle walk with a full pack loaded. The compositor plus the canvas draw adds ≤ 4 ms per rAF at the median and ≤ 8 ms at p95. The page composes only when `(frame_version, hd_version)` changed (06 O1).
6. **Gates and live check.** All gates pass. On the live site, HD switches on and off with no foreign requests.
