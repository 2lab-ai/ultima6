---
title: ultima6 hd-i18n-voice — loop done
type: loop
tags: [ultima6]
---

# Loop done: hd-i18n-voice

Append-only. Closed units, with their receipts.

## Round 1 — 06 speed (2026-10-15)

| Unit | Spec | Scope | Needs |
|---|---|---|---|
| T1 | 06 | `u6-core` `Pacer` (`due(dt, held)`, `ran`, clamp/budget losses) + `STEP_MS` + tests; `Io` port-only key-owner flag set by the `u` read loop and menu; `u6-web` `step/present/pace/ran/set_speed/speed/dropped_ms/clamped_ms/keys_owned` (keep `tick`) | — |
| T6 | 06 | O4: release profile flags; wasm size receipt | — |

- **T6** merged at ba4c8ac (branch 998cfac + 178bb2d).
  - The release profile is `codegen-units = 1` with no LTO. Cargo skips LTO for u6-web's cdylib + rlib, and a cdylib-only fat-LTO wasm measured 9.8% bigger.
  - wasm 1,759,472 → 1,415,380 B (−19.6%).
  - Native frames at tick 1500 (opening and game) are bit-identical to a non-LTO build. A 6000-frame native run is never slower than main.
  - Gates: 793 passed, web 29/29.
  - Reviews: gpt-6-astra APPROVE; opus CHANGES (LTO never reached the wasm), then APPROVE after round 2.
- **T1** merged at 2096eda (branch a11659a + b013aaa).
  - `Pacer` with `due(dt, held)`, `ran`, Finite/Max, clamped and dropped counted apart, and the press edge forgetting the debt.
  - Web API: step/present/pace/ran/set_speed/speed/clamped_ms/dropped_ms/keys_owned, with `tick` = step + present.
  - `keys_owned` from `game`'s line editor, panels and roster, and from `u`'s line reader and menu, claimed per key on `Io`.
  - Gates on the merge: 812 passed, web 29/29, wasm 1,419,522 B.
  - Reviews:
    - gpt-6-astra, in three parts: pacer CHANGES (accelerated debt spent under a held press), then APPROVE; owner APPROVE; web CHANGES (the same defect).
    - opus: CHANGES (the same defect, measured at 60 steps against 1×'s 7), then APPROVE.

| T2 | 06 | `site/app.js` fixed-step loop, key intercept by DOM `key`, badge and Speed buttons, `localStorage`, gain ramp above 4×; harness `GLUE_HOOKS` wraps `step` + rAF/now shim; delete `Game::tick`; web tests | T1 |
| T3 | 06 | native `run_window` on `Pacer`, `HEADLESS_TICK_MS` 20, keys, speed in the title; re-run the 16 ms receipts at 20 ms | T1 |

- **T3** merged at 558eb5e (branch 433c071 + 395cfe9 + 14972e1).
  - Native `run_window` is paced by the Pacer within a frame budget, and a frame's input goes to its first step.
  - Keys: `-`/`=` and numpad ±, with the speed shown in the title. Music fades above 4×. Headless runs at 20 ms.
  - The real-data receipts now run at `STEP_MS`, all within ±15%: start fight median 325 (+8.3%) and 302 (+9.0%), castle 35 (+6.1%), held-walk cells unchanged.
  - The tick-independence check runs at 20 vs 10 ms. The 4 ms miss at 20 vs 8 ms was measured as key-tick delay plus end rounding, not drift.
  - Gates on the merge: 823 passed, web 29/29, oracle and trace tests pass.
  - Reviews: gpt-6-astra in three parts, no must-fix; opus APPROVE, then APPROVE again for round 3.
- **T2** merged at 810df38 (branch 0b6fb34 + 3fbc365 + 3800d1d + ea582a8 + 402f78e).
  - The page loop runs pace → bounded 20 ms steps → ran → present only when a step ran.
  - Keys: `-`/`=`/`+` by DOM key unless the game owns them; numpad always changes the speed.
  - Presets 1×–8× and Max, kept in localStorage. Badge, Speed buttons and "behind".
  - The music fade has τ ≈ 50/3 ms. `Game::tick` is deleted. The harness counts steps on a virtual 60/144 Hz display.
  - Gates on the merge: 824 passed, web 41/41 (twice on the branch).
  - Reviews: gpt-6-astra crates and site APPROVE; tests CHANGES (a direction printed from game state, a stall test mixing pre-existing loss, a zero-hold false pass), then APPROVE. opus APPROVE through round 4.
- **Deploys.**
  - Preview 3ed52338, then production 269ed8c1, both at main 810df38.
  - Live check on production with the owner's files:
    - speed 1 → 4 (`=`), 3 (`-`), 4 (numpad +), Max shown as 76.5×, back down to 1, and 1.5 kept across a reload;
    - a pass in the opening fight took 5.3–12.3 s at 1× and 0.6–1.9 s at 4×;
    - 0 foreign requests, 0 console errors.
  - The dispatcher read frames 07 (4× badge and controls) and 08 (Max 76.8× on preview).

| T5 | 06 | O3: planar row/word blits in the opening | T1 |

- **T5** merged at 4c2c07f (branch 34a1f13 + 3b1ef7d, plus merges of main).
  - The opening's masked blit and rectangle copy now work on whole bytes; the per-pixel forms are kept as test references.
  - Speed: 4.37–4.51× per opening step (native release).
  - Frames are unchanged:
    - 7,168 seeded whole-Chip equivalence cases, plus a matrix covering every shift pair, both copy modes and aliasing;
    - opus review fuzz: 12.0M random cases and 470k exhaustive edge cases, 0 differences;
    - owner-local comparison by the dispatcher at 20 ms steps: 400 PNG pairs, 0 differing.
  - Gates on the merge: 862 passed, web 41/41.
  - Reviews: gpt-6-astra APPROVE twice; opus APPROVE.
  - Deploys: preview, then production aeb272a5. live-check2 shows 0 foreign requests and 0 errors. The dispatcher read the opening title card frame.

| T4 | 06 | O1 + O2: one RGBA per changed frame; skip idle map re-render; `merge_shores` on its counter | T2 |

- **T4** merged at 4283973 (branch a01cc39 + f7080e3 + e054c76 + 824d0bf + 3eef552 + 9962db0).
  - **O1:** a frame is converted to RGBA and drawn only when `frame_version` moved.
  - **O2:** the window redraws only the cells whose inputs changed. A whole-render skip had fired once in about 2,200 idle refreshes on real data. The shore-merge skip never fired and was deleted.
  - **Speed:** idle tick 82 → 25 µs mean (native).
  - **Equivalence** (screen, back buffer, RNG and State equal to a full render):
    - synthetic: 320 steps;
    - adversarial: `adv.rs`, run by opus at 60 seeds × 400 steps;
    - real data: 3000 steps;
    - owner-local PNGs: 301/301 idle and 301/301 walking.
  - **Gates on the merge with nostep:** 905 passed, web 47/47.
  - **Reviews:** gpt-6-astra in three parts, APPROVE in both rounds; opus APPROVE in both rounds.
- **Deploys:** preview, then production 9f831837, both at bf93585.
  - live-speed on production: 1×→4×→3×→4×→Max (76.8×)→1×; 1.5× kept across a reload; 0 foreign requests, 0 errors.
  - The dispatcher read the walked-into-the-fight frame on preview.

| H0 | 07 | `Framebuffer` source plane (`Src` with image version, `set_src`, `copy_px`), set by `gfx::blit`/`blit_bitmap`, copied by `copy_pixel`/`copy_rect`/dissolve; record versions bumped by the shore merge; enabled by u6-web; source receipt | T4 |

- **H0** merged at 8d66e1b (branch 12be07a + 6c86bcb + 0198349 + 62d1421 + 7531c93, five rounds).
  - **The plane:**
    - each pixel names the record, portrait (`letter << 8 | item`) or panel bitmap it was drawn from, with the image's version and its (u, v); overlay and outline pixels are flagged;
    - copies, the dissolve, the quake, the text scroll, a flight's put-back and combat playback carry their sources; text, plots, sparkles, a missile's own pixels and the pointer are classic;
    - "current" means `ver` equal to the record's version; a relaunch resets every plane to classic.
  - **Cost:** published and shown copy only the changed 16-pixel chunks of the plane, each plane with one sync consumer. Idle tick +4.3 µs with the plane on, against +32 µs for whole-plane copies; plane off equals d767366 within noise.
  - **Source-only publish:** a playback that ends on equal colours puts back the last picture's sources without a new colour frame. `Engine::src_version()` moves only when shown's sources change, and the H1 compositor keys on it.
  - **Receipts:**
    - owner-local source receipt: pass, 2753 frames, 0 stale, map window 100.0% tile;
    - paired engines plane off vs on, 1500 ticks: State, RNG, cache decisions and the colours of all four buffers equal every tick, published plane = front plane, shown = published except the pointer;
    - mutations in release builds (debug assertions off): opus 8/8 on the last round, 20/23 and 10/11 on earlier rounds, survivors unreachable or equivalent.
  - **Gates on the merge:** 939 passed, web 47/47 three times.
  - **Reviews:**
    - gpt-6-astra in parts, every round: round 1 three parts (APPROVE, APPROVE, CHANGES on the tests); round 2 four parts (two CHANGES: the playback sources, downstream propagation); round 3 CHANGES (a stationary pointer); round 4 APPROVE;
    - opus: round 1 APPROVE; rounds 2–5 combined APPROVE.
  - **Deploys:** preview 9f7caf87, then production 3eba8ffb, both at 8d66e1b.
    - live-speed on production: Max 76.8×; 0 foreign requests, 0 errors.
    - The dispatcher read the menu, castle hall, walked-into-the-fight and after-reload frames on preview; they are unchanged.

| H0b | 07 | plane hardening from the H0 reviews: a hard (release) check of one sync consumer per plane; a `clone_from` → sync test; the paired run also takes the source-only path; quake captures share equal sources | H0 |

- **H0b** merged at 5d9c930 (branch 12604da … 575da1f, three rounds); spec 07 wording at 392d5b1.
  - **One consumer per plane:** a plane has one sync consumer and a consumer one source. Every build checks the link before a sync copies anything, release builds included. The HD compositor reads the shown frame and never syncs from front or published.
  - **Tests added:** `clone_from` with syncs around it; the paired engines take the source-only publish and compose the shown frame under a held pointer; the flight block's copy on write.
  - **Combat captures share sources:** a recorded frame whose sources equal an earlier frame's shares them, looked up by a hash kept beside the sources and then compared by contents (the scan over recorded frames is still O(n), with cheap hash compares). A 3-pixel quake: 21 → 10 allocations, ~7.6 → ~4.0 MB. 128 recorded frames that differ only in their last pixel: 22 ms against 507 ms.
  - **Gates on the merge** (358bcc9, same tree as 5d9c930): 944 passed, web 47/47 twice, oracles OK, port_map ok.
  - **Reviews:** gpt-6-astra and opus each round; round 3 APPROVE from both.
  - **Deploys:** preview 7a547e16, then production 2cc201d2, both at main 392d5b1.
    - live-check2 on preview: 0 foreign requests, 0 errors. The dispatcher read the menu, castle hall, walked-into-the-fight and Journey Onward frames; they are unchanged.
    - live-speed on production: 1×→4×→3×→4×→Max (76.3×)→1×; 1.5× kept across a reload; 0 foreign requests, 0 errors.

| H1 | 07 | `u6-render` compositor + MMPX×S (cached by content hash) in wasm; `u6-web` HD exports; page S× canvas, HD toggle, logical pointer mapping | H0 |

- **H1** merged at 2257e21 (branch e9d2fd9 … c7ac41b, three rounds, then f1ab76f merging main with H0b).
  - **MMPX** in `u6-render`, ported from the authors' MIT reference and tested against vectors from it on synthetic images; 4× is MMPX twice. The notice ships as `site/NOTICE.txt`, linked from the HD paragraph.
  - **Compositor:** each tile, portrait and panel bitmap pixel with a current `ver` and no flags takes its block of the image's MMPX×S, cached by content (a palette change runs MMPX again); classic, flagged and stale pixels stay classic. It reads the shown frame only and runs when `(frame_version, src_version, hd_version)` changes.
  - **Page:** an HD button next to Mute, on by default, kept in `u6.hd`; the canvas is 320·S × 200·S in the same box (S 4 at ≥ 3 device pixels per game pixel, else 2) and the pointer maps onto the logical 320 × 200.
  - **Receipts:**
    - owner-local HD receipt: pass, 1353 frames at 4×, 0 blocks off the definition, a fresh compositor on 27 frames 0 differ, retained cache peak 7607 KiB of 24576 KiB;
    - no-pack performance on this Mac at 4× speed: per display frame p95 0.9–1.4 ms at S 4 and S 2 over 6 runs at load 51–73 (limit 8 ms);
    - wasm 1,513,810 bytes (+28 KB).
  - **Gates on the merge** (e177b48, same tree as 2257e21): 974 passed, 7 ignored; web 52/52 in 6 of 7 full runs (the one failure was not recorded by test name; the web-flake unit in browser-port tracks the known one) and the HD tests 30/30 in 6 filtered runs; oracles OK, port_map ok.
  - **Reviews:** gpt-6-astra in parts: round 1 seven parts (CHANGES on render tests, core images and receipt, core tests), round 2 four parts APPROVE, round 3 APPROVE; opus: round 1 on the whole unit, round 2 APPROVE.
  - **Deploys:** preview 62548a40, then production 2ae0a36c, both at 2257e21.
    - live check on preview at device pixel ratio 2: HD on gives a 1280 × 800 canvas in a 960-pixel box, off gives 320 × 200, and on again restores it, in the menu, the game, after a walk and after a reload; 0 foreign requests, 0 errors. The dispatcher read the frames side by side: the map, figures and party icons show MMPX's smoothed diagonals, text stays classic, the opening menu is pixel-identical to classic, and HD differs from classic in about 0.5% of pixels in every game frame, after the reload too.
    - on production: live-speed Max 76.8×, 1.5× kept across a reload; HD on, off and on again in the game and after a reload; 0 foreign requests, 0 errors.

| H3 | 07 | `crates/u6-hdpack` (the pack format: writer, strict reader, limits, content hash) + the `tools/hd` binary: inventory/atlas/run(`--send`)/ingest/pack with the gates; synthetic tests; gate tuning on synthetic sheets | — (in parallel with H0) |

- **H3** merged at 8826434 (branch fe0a628 … e29fb09, 49 commits over 15 rounds).
  - **`u6-hdpack`** (builds for wasm): a stored zip of `manifest.json`, `index.bin`, lossless-WebP atlases, portraits and bitmaps, keyed by the FNV-1a 64 content hash of the classic image's bytes (transparent tile pixels 0xFF), stored at ×4. The reader is all-or-nothing: the entry set must equal the index before anything is inflated; every entry's CRC-32 and declared sizes are checked; ≤ 4096 images and ≤ 150 MB decoded, counted from the index first; exact sizes; base digest. A test checks that no pack is tracked in git or under `site/`.
  - **`tools/hd`** (`u6-hd`): `inventory` (counts only), `atlas` (sheets of ×8 cells plus the cell map and `jobs.jsonl`; all frames of an actor in one sheet; mosaics; terrain with wrap borders), `run` (one `codex exec -m gpt-6-astra` imagegen call per sheet, only with `--send`; spend bounded by reserved calls, 2 per attempt; a lock on the work dir; a cooperative stop on INT/TERM/HUP/QUIT with a 2 s grace; core dumps off before anything), `ingest` (slice, key out, the per-cell gates, portraits at 1/4 layout scale with negative controls, the terrain-seam and animation group gates, one re-roll, a failing group falls back to MMPX whole), `pack`. Every output stays in `~/Games/ultima6/hd/`.
  - **Intake (site):** the page's zip reader now refuses an entry whose CRC-32 or stored size is wrong, and encrypted entries.
  - **Owner-local inventory** (counts only): 63 sheets (35 portrait, 2 bitmap, 7 terrain, 19 tile) holding 2191 images; `atlas` on the owner's zip wrote them to the work dir.
  - **Tests:** 1042 in the tool and pack crates (synthetic art only), among them mutants pinned for the stop machinery, the core limit under five inherited limits and three shells, the inherited signal dispositions, call leaks under load and drain's turn bound.
  - **Gates on the merge** (036999e, same tree as 8826434): 1077 passed, 7 ignored; web 52/52 three times and oracles OK on the merge with the round-13 head (98c3981; the later rounds changed `tools/hd` tests only).
  - **Reviews:** gpt-6-astra and opus every round (findings in the session scratchpad, `review/h3/`); the last rounds: r12 both APPROVE, r13 both APPROVE, r14 opus APPROVE and gpt6 CHANGES on one assertion (removed in r15), r15 gpt6 APPROVE.
  - **Deploys:** preview 34e9ef60, then production 80cdffb2, both at 8826434.
    - live check on preview with the owner's zip through the new intake: the game starts, HD on, Journey Onward after a reload; 0 foreign requests, 0 errors. The dispatcher read the Journey Onward HD frame.
    - live-speed on production: Max 76.8×; 1.5× kept across a reload; 0 foreign requests, 0 errors.

| H2 | 07 | the page loads a pack through `u6-hdpack` (from H3): drop classification before `importFiles`, IndexedDB/forget, limits | H1, H3 |

- **H2** merged at a769a5a (branch 17f2f93 … f4b88af, 34 commits over six rounds).
  - **Classification:** a dropped zip is an HD pack iff its root `manifest.json` reads (within the reader's limit, 64 KiB unpacked, and the probe's for its range, its CRC-32 right) and parses as a JSON object whose root `format` is `u6-hdpack/1`; the probe reads the zip's tail and that one entry only. Every other zip goes to the import as before H2, which takes only disk images: a broken pack fails there with nothing started or stored. A pack mixed with game files, or two packs in one drop, is refused with nothing loaded. (Round 4 deleted a byte heuristic for broken manifests after rounds 2–3 each found a new edge case.)
  - **Load and use:** `u6-hdpack` reads the pack in wasm; a pack image is used where its content hash matches the record's current bytes (×4 at S 4, the alpha-weighted 2×2 box mean at S 2), MMPX otherwise; load and forget move `hd_version`. Kept in IndexedDB under `hd:pack`, reloaded before the first HD frame, "Forget the HD pack" beside the other forget buttons; storage that throws keeps the pack for the visit only. The status line prints counts only.
  - **Portraits:** one helper lists each portrait once with its pixels, skipping a portrait file name too short for its letter; the tool's list on the owner's game is unchanged (206, equal digests).
  - **Tests:** synthetic packs only; the probe's rule both ways (mutant tables in the reviews: the last round killed every probe mutant listed, at-limit and one-byte-over cases included); web tests for a dropped pack (no game, counts, the HD pixel at S 4 and S 2, reload, forget), broken packs, mixed drops, a full disk, the owner's disk images beside a mention or a broken declaration (the game starts), and a full-pack performance test on the owner's files.
  - **Performance (owner-local, full pack: MMPX×4 of every owner image):** load 282–490 ms, second load 232–320 ms; matched tiles 98%, portraits 100%, panels 100%; drawn frames p95 0.8–1.4 ms at S 4 and S 2 (limit 8 ms); wasm memory 192.1 MiB after a second load (budget 229.5 MiB). wasm 1,872,907 bytes (+340 KB with the WebP decoder).
  - **Gates on the merge** (9c472cd, the same tree as a769a5a): fmt and clippy clean; 1104 passed, 7 ignored (a first run under load ~100 failed one `tools/hd` timing test that H2 did not touch; it passed alone 5 of 5 and in the full rerun; unit hd-stderr-flake); web 63/63 three times and oracles OK on the round-5 merge (34ae552; round 6 changed `u6-hdpack` tests only, so the same wasm).
  - **Reviews:** gpt-6-astra and opus every round (findings in the session scratchpad, `review/h2/`); r4 both APPROVE, r5 both APPROVE, r6 both APPROVE.
  - **Deploys:** preview f936a49f, then production 19effa16, both at a769a5a.
    - live check on preview and on production at device pixel ratio 2, with the owner's zip and the owner's pack dropped on a running game: pack 309,202,2 matched 309,202,2; HD on, off and on again before and after the pack; 0 foreign requests, 0 errors. The dispatcher read the frames: the portrait (look at the hero), the paper doll and the border show the imagegen art; text stays classic; the throne-room carpet shows a seam where generated and scaled terrain meet (H5).
    - live-speed on production: Max 76.8×; 1.5× kept across a reload; 0 foreign requests, 0 errors.

| H4 | 07 | ops: run `tools/hd` on the owner's files with imagegen (portraits → bitmaps → terrain → actors), load the pack in the owner's browser, read the frames | H3 |

- **H4** run 2026-10-16 on the owner's machine, through the Codex daemon's binary (the Homebrew copy's code-mode host carries a quarantine flag and timed out; the flag is the owner's to clear). 87 sheets in the work dir, every call ok.
  - **Coverage (records):** generated / fallback — portraits 202 / 4, bitmaps 2 / 0, terrain 49 / 222, multi-record objects 26 / 252, objects 234 / 525, actors 0 / 675 (the motion gate, as spec 07 expected); 32 shore records by rule, 33 empty.
  - **Pack:** 309 tiles, 202 portraits, 2 bitmaps; `read_pack` reads it, and on the owner's game every image matches (309, 202, 2). Kept in `~/Games/ultima6/hd/pack/`, never committed or hosted.
  - **Frames (read by the dispatcher on the owner's machine):** portraits and panels show the imagegen art; where a generated terrain record meets a scaled one the frames show a seam (a castle carpet), which the 3×3 self-tiling gate cannot see → spec 07 rev 6 adds the neighbour-seam gate, unit H5.

| hd-stderr-flake | 07 | `tools/hd` test `stderr_written_after_the_events_end_is_still_kept` failed once in a full run under load (stderr empty: the helper's write came after the 1 s grace); reproduce, make the test load-independent without weakening it, sweep `tools/hd` for thin timing margins | H3 |

- **hd-stderr-flake** merged at a578045 (branch to 2b75fc4).
  - **Cause:** under machine load, a helper that had not yet called setsid when the call's deadline killed its group died with the group, so its output or pid file never came. The tests then failed on a missing file rather than on their own claim.
  - **Fix:** the helpers call setsid before they fork their child. Every deadline a stand-in or helper must beat is now at least 4 s, the keeps-both one included. Each such test asserts a floor of deadline + grace, so a helper that died with the group fails with a message naming it. The comments call these bounds slack for a loaded machine, not bounds on its load. The sweep of `tools/hd` listed every call and harness deadline; none that a helper must beat is under 4 s.
  - **Tests:** u6-hdtool lib 57, call_leaks 1, cli 16, events_memory 1, lock 2, terminal 1. RED checks: a helper 4.5 s slow before its setsid fails each new floor with its own message.
  - **Gates on the merge** (a578045): fmt and clippy clean; 1106 passed, 7 ignored; web 63/63 three times; oracles OK; loop-lint 0 FAIL.
  - **Reviews:** gpt-6-astra and opus; r2 both APPROVE, r3 (the delta) both APPROVE, and their wording items were applied in 2b75fc4.
  - **Deploys:** none; the change is to tool tests, and the site is unchanged.

| H5 | 07 | `tools/hd` neighbour-seam gate (spec 07 rev 6): pairs from the owner's map chunks and starting object lists, the cross-edge lattice step against the classic pair's, fallback to a fixed point, the count in the coverage report; then re-ingest locally (no new upload), rebuild the pack, read the frames | H3, H4 |

- **H5** merged at 953e744 (branch abfd815 … d081952).
  - **Pairs:** map tiles of edge-adjacent cells, and starting objects whose shared edges are both at least half opaque (`neighbour::pairs`). Each pair is labelled by where it comes from (`map`, `objects`).
  - **Measure:** the step across the join, over the mean interior lattice step, against the classic pair's own ratio. The limit is max(1.5, classic ratio) × 1.001.
  - **Fallback:** a failing generated record falls back to MMPX as its whole item, repeated to a fixed point.
  - **Coverage report:** counts the pairs checked, failing and unmeasured before and after the fixed point. An unmeasured pair is one `settle` gets no measure for, and it cannot fail.
  - **Owner run:** re-ingested from H4's sheets with no new upload, then rebuilt the pack locally. Most first-pass pairs failed and few generated terrain records survived (counts in the owner-local coverage report). That outcome went to trinity (record `~/Games/ultima6/work/dispatch/trinity-h5-decision.md`), and then to H6's read and H7.
  - **Tests:** synthetic records only. Pair-model controls; the ratio and limit; the fixed point; the after counts, with the mutant "after count set in pass 1 only" killed by a test where an unmeasured pair's generated side falls.
  - **Gates on the merge:** see the merge-gate line below.
  - **Reviews:** gpt-6-astra and opus, rounds 1–4. r3: gpt-6-astra APPROVE, opus CHANGES on one test (MF2, closed in r4 with opus's own probe). r4: both APPROVE; opus's wording item was applied in d081952. Findings: `~/Games/ultima6/work/review/h5/`.
  - **Deploys:** none; the change is to the tool, and the site is unchanged.
  - **Carried to H7:** the run-time draw variants (type 0x7b's other frame, the doorways' side tiles) were not paired.
  - **Gates on the merge** (9bbbb22, the merge 953e744 plus this loop text): fmt and clippy clean; 1125 passed, 0 failed, 7 ignored; wasm 1,872,907 bytes (unchanged); web 63/63 three times; oracles OK; port map unchanged; loop-lint 0 FAIL.

| L1 | 08 | text path: `i32` chars, `u16` line and narrowing sites, UTF-8 assembler (invalid bytes pass), Hangul table, runic guard, cell padding, josa, `%1$s`, empty articles; the `u` program's text path by code point (1b) | — |

- **L1** merged at 16be889, squashed as 6665cb8 from `feat/l1-text-path` at 8751380, so main's history holds only the reviewed end state.
  - **What:** spec 08 §1 and §1b.
    - The character path is `i32` and the line `u16`.
    - A UTF-8 assembler sits in front of the escape layer while a locale layer is active; invalid bytes pass unchanged.
    - The Hangul table is built from Galmuri7 (SIL OFL 1.1, notices in `site/fonts/`) by `tools/font/galmuri.py`.
    - The runic guard; josa markers; `%1$s`; `@{…}`; empty articles.
    - `printf` counts cells, with the fixed-point padding.
    - The `u` program's text runs by code point.
  - **English:** with `wide` off, output is unchanged; an owner-run comparison with the pre-change engine found no difference (counts stay local).
  - **Tests:** synthetic text only: `escaped_tests`, `wide_tests`, the intro wide tests, the font tests, and `test_galmuri.py` in both modes; the Python 3.9 refusal holds.
  - **Gates on the merge** (run on 6665cb8, whose tree the merge 16be889 has): fmt and clippy clean; 1176 passed, 0 failed, 7 ignored; wasm 1,882,766 bytes; web 63/63 three times; oracles OK; port map unchanged; loop-lint 0 FAIL.
  - **Reviews:** gpt-6-astra and opus over 15 rounds; r15, both APPROVE. Rounds 10–14 each found a counterexample in doc prose that listed, case by case, what a field's first code does after a padding that exactly fills the row. Round 14 deleted the list, keeping only the measured threshold. Lesson: prose that restates a state machine case by case is unbounded review surface; state the measured invariant and defer to the code. Findings: `~/Games/ultima6/work/review/l1-text-path/`.
  - **Deploys:** preview 409b5dea at 16be889. Live check with the owner's zip and the owner's local pack: HD on, off and on again in the menu, the game and after a look; the pack matched; 0 foreign requests, 0 errors. The dispatcher read the frames (a long wrapped question, the party list and the prompt, a look message); English text lays out as before. Production waits on the owner's deploy run.

## 2026-10-20 — the item-1 decisions of 2026-10-18, superseded

Superseded by the decision of 2026-10-20 (B: the read decides, H6 and H7 parked). Kept as they stood:

Decision 2026-10-18 (trinity, converged in round 3; record `~/Games/ultima6/work/dispatch/trinity-h5-decision.md`):
- H5 merged as gate code (953e744); item 1 stays open.
- Before any limit change, H6 reads the measure against the pixels on H4's pack, at S 4, outdoor pairs. Buckets: ≤1.1, 1.1–1.5, >1.5 × the limit, gen-gen and gen-MMPX separately. Controls: the carpet seam as positive, passing pairs as negative.
- Reading:
  - if ≤1.1 looks clean and >1.5 looks seamed, the ruler stands; a calibrated limit or C (edge-harmonise in ingest, no upload) follows;
  - if ~1.5× pairs look clean, the denominator's sampling (HD interior read only on pitch-k lattice lines) is fixed first, with a null-case fixture.
- H7 closes the pair model's two gaps before item 1 closes: (b) flags3 0x20 objects that replace the base tile, with the ghost base-tile pairs removed; (c) generated objects at least half opaque toward terrain, counted first.
- Then re-gate, read the final pack at S 4 and S 2, and write spec 07 rev 7. No arbitrary loosening (option B). D, terrain re-prompted with neighbour context, comes last if at all.

Decision 2026-10-18, after H6's read (trinity, unanimous in round 5; record `~/Games/ultima6/work/dispatch/trinity-h6-decision.md`). H6 read the sheets of H4's pre-gate pack:
- Terrain: almost all water. The fail side is verified, and the generated tiles are a different style from MMPX.
- Objects: some passing pairs are visibly defective at or beside the join, and every failing pair read is visibly defective in the window. The defects are restyle, content changes, and multi-cell mis-registration.

Outcome:
- The v1 pack generates no terrain. This is a policy constant, not a family rule.
- H7(b)(c) are unconditional. Count first. The first-pass object-terrain pairs are recorded, not gated.
- Re-gate.
- The acceptance is a window read of the final pack at S 4 and S 2:
  - passed pairs included;
  - each window compared against its classic and MMPX panels;
  - the fixed-point record matched to the final slices;
  - explicit inclusion of outdoor, town and castle windows, gen-gen and gen-MMPX, object-terrain, multi-record walls and carpets, and the defective families the object read named;
  - a type with no survivor recorded as 0.
- A new defect sends the whole object to MMPX, then re-gate and re-read. Then rev 7.
- Not done: C, any change to the limit or the denominator, a class rule, a pre-fallback from the pre-gate pack.
- D (an MMPX-compatible crisp style) comes after items 2 and 3.

## 2026-10-21 — the drive to production, closed

The loop.md body as it stood before this move (headings one level down):

Status: in-progress. Research is done (2026-10-15, notes kept locally in `~/Games/ultima6/research/2026-10-15/`). The specs are written:

| Spec | User item |
|---|---|
| `06-fixed-tick-speed.md` | 4 |
| `07-hd-art.md` | 1 |
| `08-i18n-ko.md` | 2 |
| `09-npc-voice.md` | 3 |

Spec review: every spec had a gpt-6-astra review and an opus review read against the code, all folded in. 06 is at rev 3, 07 at rev 7 (rev 5: what H0 and H3 measured; rev 6: the neighbour-seam gate; rev 7: the v1 pack, the read decides), 08 at rev 3, 09 at rev 3. Closed units are in `loop.done.md`.

Shipped: 06 is complete in production (9f831837 at main bf93585). 07 H0 (source plane), H0b (plane hardening), H1 (the HD compositor, MMPX at 2× or 4×, on by default), H3 (the pack format and the imagegen tool) and H2 (the page loads a pack) are in production (19effa16 at main a769a5a); H4 ran the tool on the owner's files (its pack stays local); H5 (the neighbour-seam gate, tool only, no deploy) is merged at 953e744. 08 L1 (the wide text path) is merged at 16be889 and on preview. In flight:
- Parked 2026-10-19 (see Re-plan below): H6 at 4443e4b, H7 at c74bccd (local only). L3 landed on main (03a2071) for V5. The L1 production deploy line is withdrawn.
- On preview, not in production: the first-run chooser (`first-run`), voice V1–V3 with the CPU path (`voice`, main 882cff2) and the NPC bundles button V5 (`npc`, main aa4018a). Production waits on the owner keeping a preview.
- On preview `npc-voices` (main 2aaa396): V4, a voice per NPC from the committed type-class table and the actor number.
- On preview `voice-pack` (main 70e2d23): V6, the personality voice pack tool (`tools/speech/profile.mjs`) and the page's pack loader. The owner's real run (his bundles to the LLM) waits on his typed `yes`; my own dry run on his bundles was refused by the auto-mode classifier (PII Data Handling), so the first payload is shown by his run.
- On preview `korean` (main 605c32f): K1, one NPC in archaic Korean. The engine's locale pack for conversation runs (hash-checked, English for a missing run), the page's drop/keep/forget of `ko.u6lp`, and `tools/i18n/translate.mjs` with the style guide. Live check with the owner's files and a synthetic pack: Hangul cells in the message window, hits 2 misses 0, forget gives English. The owner's one-NPC run waits on his typed `yes`.
- On preview `korean-strings` (main 473d280): K2, the executable's catalogued messages (with `$G`/`$T`) and the conversation table strings through the same pack, the page's Game strings export, and the tool's `--strings`, `--name` and merge. Live check with the owner's files and a synthetic pack: Look and Talk show the pack's syllable for the command names and "You see", the table string in Korean; game strings 4 shown 0 missed, table strings 1 shown 0 missed, forget gives English. The owner's string run waits on his typed `yes`.
- On preview `korean-look` (main e579e56): K3, object names (look.lzd) and books and signs (book.dat) through the same pack, per entry with length and hash (spec 08 rev 7), the Game strings export's names and books, and the tool's `--only`. Live check with the owner's files and a synthetic pack: Look ("You see" and the name, no article) and the combat lines show the pack's syllable as the name, names missed 0, forget gives English. No sign was in view at the start (120 tiles looked at), so the book read rests on the synthetic test and the sign receipt. The owner's run is `--strings` with `--only look,books` (without it the K2 strings are sent and replaced again), after his typed `yes`.
- R0 merged (main 5fac99d, tests only, no deploy): the conversation receipts print only counts their own conversations make and pass/fail, no size of the owner's data.
- On preview `korean-intro` (main 25a6c89): K4, the `u` program's text (the opening, the story passages, the walk and the gypsy's questions, character creation, the transfer; not the credits) through the same pack (spec 08 rev 8): each string checked against its English's length, printf codes and the answer keys its English shows standing alone; a box the translation does not fit is laid out first and drawn in English. The Game strings export's `u` part (with each prompt's keys) and the tool's `--only u`. Live check with the owner's files and two synthetic packs: a short one shows the pack's syllable in the opening caption, the name screen and the gypsy's questions with A) and B) kept, misses 0; a long one shows the opening caption and the gypsy's question in English without overprint and the name screen in Korean where it fits; forget gives English. The owner's run is `--strings --only u` after his typed `yes`.
- R1 merged (main 86b9ddc, tests only, no deploy): the page tests that run on the owner's files print only counts and timings they make and pass/fail (no bundle, name or book totals, no share of the HD pack, no memory figure after his files load); tools/web/measure-memory.mjs prints only measures taken before the drop.
- On preview `korean-keywords` (main d902a70): K5a, typing Korean in a conversation (spec 08 rev 9). The engine's text event reaches only the line editor, which in the conversation's three reads and with a pack takes Hangul syllables (`max` stays a byte budget); the pack's optional `aliases` part (per script and English keyword, 1–4 words, a repeated field refused) matches a typed word alone or with one particle, on a copy of the line; the page's hidden input takes the browser's IME in 한글 with the pack, only while a line is read (out of the tab order otherwise). Live check with the owner's files and a synthetic pack (his scripts' plain runs as they are, three keywords aliased): the committed syllables echo in the message window, an alias with a particle gets the English keyword's answer, an English keyword and an unknown word answer as before, the alias that ends the talk ends it and the focus returns to the canvas; forget gives English with no hidden input.
- K5b merged (main 4f9a464, tool only, no deploy): `tools/i18n/translate.mjs` asks one Korean word for each keyword of a script (the keys at least 3 chosen scripts share are asked once first), keeps 1–8 Hangul syllables the player can tell apart as the engine matches them (with a particle; two keys share a word only when their first keyword block is the same, also against the words the pack keeps), highlights the word as `@{word}` in the script's runs and table strings, and writes the pack's `aliases`, merged with the words the pack already has (spec 08 rev 10). Live check on preview `korean-keywords` with the owner's files: a pack the tool's own writer made from his bundles' keys, three keywords given words, read back by the tool and installed by the engine; the word typed with a particle got the English keyword's answer, the English keyword still answered, the word that ends the talk ended it.
- V7 merged (main 8032ac8, tests and one speech fix, no deploy alone): a line the Korean pack puts is said as one `ko` utterance, and at a more prompt each split part takes the language of its own codes; the page sends each line's language and text to the worker on the GPU and CPU paths. Live check on preview `korean-keywords` with the owner's files and a synthetic pack: Korean ASR heard every syllable of the greeting in order, on both paths.
- The translation run, 2026-10-20, with the owner's consent (verbatim: "처음 인트로부터 캐릭터 생성 스크린부터 모든 대화를 번역하고 배포해줘"): the `u` program's text and every conversation script, with their keywords; the pack is local. Live check on `korean-keywords` with his files and that pack: the opening and the gypsy's questions in Korean, a party member's greeting and two answers in Korean, the Korean words for two of his keywords typed through the IME got their answers. Not sent (not in his words): the executable's messages, object names and books, so the Look and conversation prompt words stay English. Found: a keyword two topics list is refused even when both give the same word (the K5c tool fix, then the conversations run again).
- K5c merged (main de086b3, tool only): a keyword the model answers twice with the same word is kept, and a script's request lists each key once, in its first block. The conversations ran again with it (same consent): far fewer keywords stay English.
- On preview `korean-keywords` (main ff95606): K6, the Korean pack hosted under `/packs/` behind the owner key, listed as `ko` in `index.json`, and loaded by default in 한글 (spec 08 rev 11): fetched with the key, kept with its source in one IndexedDB transaction, fetched again only under a new name, never over a drop; the first run picks 한글 for a key or a Korean browser; a setup stored before K6 counts as none once for a key. Live check on the preview: a fresh visit with the key preselects 한글, downloads the pack once and runs the intro and the conversations in Korean with no drop, the Korean keywords answer; a stored older English setup with the key kept opens the chooser on 한글 with the pack loaded, and OK keeps the new setup.
- On preview `korean-keywords` (main 8747579): the v1 HD pack, listed first in the chooser. H8 (main ab9a2e1) gives `u6-hd ingest --exclude FILE`: every item holding a listed record goes back to MMPX whole, before the neighbour gate; the list is the owner's local file. v1 is H5's sheets re-ingested with the list from the read of every generated tile object at S 4 and S 2 (no new generation, no upload); terrain stays MMPX; portraits stay generated. The u6-hdtool doc gate is green again (8747579). Live check with the owner's key: a fresh first run lists v1, it loads and shows at S 4 and S 2 in the castle rooms and courtyard; the same moment drawn with v1, H5 and no pack, every map-window pixel of v1 equals H5's or MMPX's (a listed bottle and bread equal MMPX, a kept mug and wall pictures equal H5); no listed record is in the pack. The town is out of reach of a new game's party (the gatehouse is locked). Production waits on the owner keeping it.
- On preview `korean-keywords` (main c6517c6): K7, a key letter the English shows followed by whitespace (the article A) is a word, not a key shown; a translation is scanned as before, so no pack valid before is refused. A few gypsy question strings had stayed English for it; the intro was translated again (same consent) with every string of `u` in Korean, hosted as a new pack name. Live checks 2026-10-20 with the owner's key and disks, as a new visitor. On this build (e765b4fc): the chooser offers English and 한글 and lists v1; 한글 loads the hosted pack with no drop; every gypsy question frame reads in Korean with A) and B) kept, program misses 0. On this build too, as on the one before it (02b11ff0): the keys - and = and + change the speed one preset at a time, at 1x and 4x the fixed step never fell behind over 6 s, Max reached 76.8×; on the CPU path (no WebGPU) two party members' greetings were sent and played in Korean in two different voices (Korean ASR heard one line whole, the other with one archaic word misheard by both ASR models); the NPC bundles download one bundle per script, each with its name and lines.
- In production 2026-10-21 (main 9bd8e93, the owner's answer: "지금 올려"): everything on preview `korean-keywords` up to K7, with the v1 HD pack and the Korean pack of the intro and all conversations hosted behind the key. Live check on https://ultima6.2lab.ai with the key and his disks: the chooser lists v1, 한글 loads the hosted pack, every gypsy question frame reads in Korean, program misses 0; packs without the key answer 404.
- On preview `korean-keywords` 2026-10-21, after the owner's yes to both: the executable's messages, object names and books translated into the same pack (books that are runes only, and one the tool's marker rule skips, stay English), hosted under a new name; and the personality voice pack made by `tools/speech/profile.mjs` from the NPC bundles (every script profiled, none failed), kept locally and taken by a drop. Live check: the talk's command name, the look line and the reply prompt read in Korean, game-string misses 0; Look shows the object's name from the pack, misses 0; a dropped voice pack is taken and gives the same NPC a voice other than its default.
- On preview `korean-keywords` (main 60bd80d): without WebGPU the voice steps stay at 4 or more (spec 09 rev 6). Before, a slow first line halved them to 3 for every later line, and Korean at 3 lost its words (one NPC's personality voice was heard at 0.40). Live check 2026-10-21 with his files and his voice pack on the CPU: that NPC's line heard 1.00 in the default voice and in the pack's, steps 4 throughout; a second NPC's line 0.63, the same line in the default voice at 16 steps also 0.63, and made-up lines in that NPC's pack voice at 4 steps 0.89–1.00, so the miss is an archaic word the ASR spells by its sound, not the voice. Production waits on the owner.
- Next: the owner's hand check in 한글 with a real Korean IME.

Decision 2026-10-20 (trinity, unanimous in round 2; record `~/Games/ultima6/work/dispatch/trinity-item1-decision.md`): item 1 closes with B, the read of every window of H5's final-read sheet at S 4 and S 2, each restyled object back to MMPX whole, the rebuilt pack re-read live and hosted behind the key, spec 07 rev 7. H6 and H7 stay parked (not merged); no new generation or upload; the crisp prompt (D) is a named follow-up.

Decision 2026-10-17: items still close one at a time and in order (07 closes before 08 is reported done), but 08's first units (L1, L3) start while 07's last work is tool-only (H5, hd-stderr-flake in `tools/hd`), since their files are disjoint and waiting idles the machine.

### Re-plan (2026-10-19, owner correction + trinity round 4, unanimous)

Nothing the owner could see changed after 2026-10-17 21:15 while H6, H7 and L3 ran review rounds. The plan from here:

1. **First-run chooser (done, on preview `first-run`).** Language English | 한글 for the page chrome, and HD: MMPX or a pack the site lists. Packs are hosted under `/packs/`, gitignored, and served only with the owner's key (a root Pages Function gates every spelling of the path). Owner instruction (verbatim): "hd pack은 드랍하게 하지말고 올려서 내가 고를수 있게 해줘 처음 실행활시에".
2. **Voice (done, on preview `voice`).** Spec 09 steps 1–3: the speech tap (V1), the Supertonic-3 weights and ort 1.30.0 self-hosted as ≤ 24 MiB parts with LICENSE and NOTICE, behind a terms click-through before the first download (V2), and one English default voice, preset M1 (V3). A real NPC line plays on device; the tap holds back a row not yet drawn at a more prompt. Started before the owner's answer on 1, since nothing else was in flight. Live receipt on the preview with the owner's files: pass/fail and timings only, kept locally.
3. **NPC bundles (done, on preview `npc`).** V5: one `u6.npc-bundle/1` per conversation script from the kept files, kept under `npc-bundle:` and downloaded as one JSON file, with a per-script item budget against hostile scripts. Without WebGPU the voices run on the CPU (owner, 2026-10-19).
4. **A voice per NPC (done).** V4 on preview `npc-voices`; V6, the personality pack tool and loader, on preview `voice-pack`. The pack itself is made by the owner's run of the tool (a typed yes before any upload).

Parked, no new commits or reviews: H6 at 4443e4b, H7 at c74bccd (local only), L3 at a253067 (unparked when L5 starts). The L1 production deploy line issued 2026-10-19 07:50 is withdrawn: it changes no English pixel.

Process: one code unit in flight; review at most 2 rounds (round 2 checks round 1's MUST fixes only); NICE findings neither block nor get fixed; reviews look only at user-visible behaviour, rights, security and data damage; a unit's exit names a URL, what the owner does, and the pixel or sound that changes. Commit subjects: English, imperative, ≤72 characters, no round or reviewer ids; one squash per unit on main.

### Order, and why it differs from the user's numbering

The work runs one item at a time, as the user asked. The order follows dependencies:

1. **06 speed.** Its frame contract (one RGBA conversion per rAF, only when the frame changed) must exist before an HD frame does. It also fixes the vblank bug that every later receipt would otherwise inherit. It is the item the user feels most ("지금 너무 느린데").
2. **07 HD.** Its scene tap and S× canvas carry 08's 2× Hangul font and give a larger, clearer text window.
3. **08 Korean.** Its text path and extractor feed 09's speech tap (Korean lines) and 09's NPC catalog.
4. **09 voice.** Its Korean speech needs 08, and its bundles need the conversation disassembler that 08 builds.

### Work units (one agent each; branches `feat/<unit>`, worktrees `.worktrees/feat-<unit>`)

| Unit | Spec | Scope | Needs |
|---|---|---|---|
| H6 | 07 | `u6-hd calibrate`: sample sheets of neighbour pairs by ratio bucket and side kind (classic / MMPX / H4 HD panels via `Compositor::compose`, ×4 and S 2), `samples.tsv`, a carpet control (read 2026-10-18: map and objects); then the final-read mode: the fixed-point pass against its final slices, one window per surviving generated item, explicit type and family inclusion, every pair label | H5 |
| H7 | 07 | pair model gaps (b) flags3 0x20 objects over terrain, ghost base-tile pairs removed, and (c) generated objects ≥½ opaque toward terrain, counted first; the terrain constant; re-gate; final window read at S 4 and S 2 (H6's final-read mode), whole-object fallbacks to a fixed point; the run-time draw variants (one object type's other frame, the doorways' side tiles) paired; spec 07 rev 7 | H5, H6 |
| L2 | 08 | locale layer + run, table-string and outermost-print hooks + pack parser; identity-pack receipt with `wide` on | L1, L3 |
| L3 | 08 | conversation disassembler (04 W3a's table) + run and table-string extractor; the catalog shared with 09 | — |
| L4 | 08 | page: language gate, en/ko UI catalog, pack drop/forget, hidden-input IME focused by the line bit; engine `InputEvent::Text` into `read_line`; aliases on a matching copy | L2 |
| L5 | 08 | `tools/i18n` glossary, speakers, translate, review, pack, coverage receipt, upload boundary; style guide | L3 |
| L6 | 08 | ops: run L5 on the owner's files, owner sample, full run, load, read frames | L4, L5 |
| V1 | 09 | `u6-core::speech` tap: kind from the call sites, flag around the three speech emitters in the Messages window, speaker after `0xbf`, flushes, capped queue, enable flag, `take_speech` | L1 |
| V2 | 09 | `tools/speech/fetch-models.sh`, parts + manifest + LICENSE/NOTICE; preview deploy | — |
| V3 | 09 | `site/voice/` worker and player, opt-in, chip, terms notice, forget buttons, mute both gains, no speech above 4× | V1, V2 |
| V4 | 09 | profiles: mixer, type-class table (no LLM) | V3 |
| V6 | 09 | personality profiles from bundles, local voice pack tool, coverage receipt, upload boundary | V4, V5 |
| V5 | 09 | NPC bundle exporter (wasm) keyed by (archive, item) on 08's run catalog, `npc-bundle:` keys, download | L3 |

Review and landing follow the re-plan above (at most 2 rounds, one squash per unit). Each unit ends with a preview deploy, a live check and a frame read; production follows once the owner keeps the preview.

### Round 2026-10-21 — the zip-only deploy and the owner's follow-ups

Owner, 2026-10-21 (verbatim in `ssot.md`): the site applies everything from the game zip alone; the repository carries the scripts that make the derived files, a private repository carries the files the deploy uses; a cleaner page with a fullscreen button; the drawbridge of the castle; the gatehouse plant gives the key.
- F1 (main a7ca1c5): the deploy takes the packs from the private `2lab-ai/ultima6-deploy`; `site/packs/` stays gitignored.
- K8 (main e5d219b), K8b (main 74df98c), K8c (main 376edca): the actors' and objects' own lines and the date line catalogued, short labels from the loader tables (the date line's wind name in Korean), the export checks taking a label of 1 or 2 bytes.
- E1 (main dbf0658): `scripts/make-packs.sh` and `u6-export` make the HD, Korean and voice packs from one's own copy of the game.
- DB (main 97349ad): a crank raises and lowers its drawbridge (0x12e3f6, 0x12de3a). Real-data receipt: every surface crank turns its bridge and back, followers parked: pass.
- P1 (main 1cf8054): the packs public under `/packs/`, the key gate deleted; the first run defaults to 한글, the first listed HD pack and NPC voices, OK as the terms' acceptance. Production check 2026-10-21 as a new visitor with only the game zip: chooser defaults, the voices ready, the game in Korean with HD, no `/packs/` request refused: pass.
- GK (main 94be33d): Look at a plant by the gatehouse door gives the key that opens it (port-only, owner 2026-10-21). Real-data receipt: every gatehouse plant gives the key, the key is taken and the door unlocks: pass.
- D1 (main fa3840a): the page cut to the game screen, one toolbar and Settings; a Fullscreen button with an Exit control, the canvas taking the focus back on every way out. Gates: web suite with models, fmt, u6-web and u6-export tests, clippy; review gpt-6-astra APPROVE (the focus fix after a NICE promoted to MUST).
- Production after D1: deploy f35d49af at main fa3840a. Live check as a new visitor with only the game zip: titled Ultima VI, the toolbar's six buttons only, the removed parts gone; fullscreen fills the screen with the canvas focused and a key reaching the game; left by the Exit button, the toolbar's click and the browser's own way out, each time the canvas has the focus and a key reaches the game: pass. Frames read: the intro caption in Korean on the page and in fullscreen, the Exit control at the top right.
