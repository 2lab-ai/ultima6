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
