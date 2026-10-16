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
