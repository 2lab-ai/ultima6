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
