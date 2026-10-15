---
title: ultima6 — 06 fixed tick, speed control, performance
type: spec
tags: [ultima6, timing, web, native]
---

# 06 — Fixed game tick, - / + speed control, performance

Status: planned (rev 2, 2026-10-15: gpt-6-astra review folded in: `ran` in the API, Max as its own mode, admission loss counted, image-equivalence for optimizations). User item 4 (`hd-i18n-voice/ssot.md`, verbatim): "게임 루프(틱)을 고정해서 속도 유지해줘. 이거 키보드 -+로 조절할 수 있도록해줘. (지금 너무 느린데 최적화도 해야할 것 같음)". Research notes are kept outside the repo, in `~/Games/ultima6/research/2026-10-15/r4-speed-findings.md`.

## Measured facts (main 72865e5)

- **The pace is the original's, not the port falling behind.**
  - The engine clock tracks wall time to within 0–5 ms per 8 s.
  - The long waits are the oracle-timed `pace()` and `sleep()` waits, in engine ms: a start-fight turn takes about 300 PAL frames ≈ 6 s, and a castle step 33 frames.
  - The oracle ran the original with its code in fast RAM, so 1× is already roughly an accelerated Amiga.
- **Compute is about 1% of a core.** The world costs 0.09–0.2 ms of wasm per 20 ms step, and the opening scene 1.0–1.7 ms. Wasm runs at parity with native. Optimization therefore raises the ceiling of a speed multiplier; it does not make 1× faster.
- **A fixed-tick bug: `Io::vblank()` resolves at the next host tick**, and the page makes one tick per `requestAnimationFrame` (`site/app.js:255-286`, `engine.rs:242,505`). Everything counted in vblanks runs at the display rate rather than 50 Hz:
  - the idle cursor and map animation;
  - the whole `u` opening, which runs 1.2× PAL at 60 Hz and 2.88× at 144 Hz;
  - the Use machine's 101-vblank wait, which takes 1683 ms at 60 Hz instead of 2020 ms.
- The page caps a frame at 100 ms of wall time and drops the rest (`app.js:43,261`). Native does the same (`u6-native/src/main.rs:450`). Native headless steps 16 ms (`main.rs:54`).
- **The keys `-` and `+` collide with the game in three places:**
  - the stats/inventory panel's next/previous member (`ui/target.rs:361-362`);
  - the roster scroll (`:373-374`);
  - a hyphen in a typed name (`intro/create.rs:145`).

  `key_from_dom` never delivers function keys (`u6-web/src/lib.rs:695`), so in the panels these two keys are the only keyboard way to cycle members.
- **Hot spots (world, idle):** `write_rgba` 26%, `gfx::blit` 26%, `Io::with_buffers` 22%, `merge_shores` 8%. The map is re-rendered every idle poll (`render_window` 49% inclusive). In the opening, `intro::mem::blit_masked` takes 36% and `Chip::set_pixel` 35%.

## Design

1. **Fixed step.** The engine is stepped only in `STEP_MS = 20` (one PAL frame; reuse `combat::present::FRAME_MS`).
   - A step is one `Engine::tick(20)`, so `generation` counts PAL frames and every `vblank()` wait is correct on any display.
   - Engine-ms waits (`pace`, `sleep`) are unchanged.
2. **`Pacer` in `u6-core`.** A pure struct with no clock, shared by web and native. It commits on report:
   - `due(wall_dt_ms) -> n` at a finite speed s: `acc += clamp(dt, 0, CAP) × s`; returns `min(floor(acc / 20), MAX_STEPS)` without consuming. The wall time cut by the clamp, `max(dt − CAP, 0)`, is added to `dropped_ms` (wall ms).
   - The host runs `k ≤ n` steps, stopping early when its frame budget is spent. k may be 0.
   - `ran(k)`: `acc -= 20 k`. Anything owed beyond `CAP × s` is converted to wall ms (÷ s) and added to `dropped_ms`. `dropped_ms` is the one source for the "behind" indicator.
   - Max is a separate mode, not a large s. `due` returns `MAX_STEPS`, and the host runs steps until its budget is spent. `ran(k)` then sets `acc` to 0, so Max never owes and never reports "behind". Any change of speed or mode resets `acc` to 0.
   - Constants:
     - `CAP` = 250 ms of wall time per frame;
     - `MAX_STEPS` = 64 per frame;
     - frame budget = 0.5 × the measured rAF interval, never below 3 ms.
   - Unit tests: over T ms the host gets exactly `floor(T × speed / 20)` steps, ±1.
3. **Web API.**
   - `tick(ms)` is kept (= `step` + `present`; about 130 Rust test call sites and the harness use it).
   - New:
     - `step(ms)`: engine tick plus audio sync, no RGBA;
     - `present()`: one RGBA write;
     - `pace(dt) -> u32` (= `due`);
     - `ran(k)`;
     - `set_speed(x)` (x = 0 means Max);
     - `speed()`;
     - `dropped_ms()`;
     - `plus_minus_owner() -> u8`.
   - The page loop becomes:
     1. `n = pace(now - last)`.
     2. Step: `k = 0`; while `k < n` and (k = 0 or within the budget), call `step(20)` and `k += 1`.
     3. Report: `ran(k)`.
     4. Present: if k > 0, `present()` (06 O1 later skips unchanged frames), then draw.
     5. Once per frame: audio and `checkSave`.
   - Audio sync stays per step, because `CALL_LOG` is sized for one step's calls.
4. **Speed presets:** 1×, 1.5×, 2×, 3×, 4×, 6×, 8×, Max.
   - Max is as many steps as fit the frame budget; the badge shows the speed actually reached.
   - The default stays 1× (the oracle baseline). The viewer's choice persists in `localStorage`, guarded with try/catch.
   - Oracle waits stay in engine ms: a fight turn is the same number of steps at every speed, and only the wall time shrinks.
5. **Audio.** Music keeps playing in wall time at 1×. Above 4×, its gain ramps to 0 and is restored on return. Scaling the tempo without changing pitch through the CIA timer is a later option, not v1.
6. **Keys.** The page intercepts `keydown` before `game.key_down`:
   - `NumpadAdd` / `NumpadSubtract` always change the speed.
   - `Minus`, `Equal` and `Shift+Equal` change the speed unless `plus_minus_owner()` says the engine is using them right now. That is the case:
     - while a line is being typed;
     - in panel mode 0x90 or 0x92;
     - in roster mode 0x91 while there is a roster to scroll.
   - A one-step-stale answer is accepted.
   - Native maps `Minus`, `Equal` and the numpad keys the same way and shows the speed in the window title.
7. **UI.**
   - A badge over the canvas: shown for 1.5 s after a change, and kept visible while the speed is not 1×.
   - A Speed group beside Mute: `−  2×  +`, plus "behind" when steps are being dropped. It also works on touch screens.
   - The badge's title says that `-`/`+` belong to the panel while it is open.
8. **Native.** `run_window` uses the same `Pacer`, with audio sync after every step and one top-up per frame. Headless `HEADLESS_TICK_MS` changes from 16 to 20.

## Optimization (after the fixed step, in this order)

| # | Change | Cost it addresses | Expected gain |
|---|---|---|---|
| O1 | One RGBA conversion per rAF, only when the frame changed | `write_rgba` 0.05 ms per step | (steps in the frame − 1) × 0.05 ms per frame |
| O2 | Skip the idle map re-render when nothing it draws changed: view dirty, animation counter, actor frames, cursor; run `merge_shores` only when its counter changes | `render_window` 49% | idle step 0.2 → ~0.1 ms |
| O3 | Opening: row- and word-wide planar blits instead of per-pixel `set_pixel`/`blit_masked` | 71% of the opening step | 3–5× on the opening |
| O4 | `[profile.release] lto = "fat"`, `codegen-units = 1` | — | wasm −19.5% in size; no speed gain |

O1 is also the contract the HD renderer (07) depends on: an HD frame must not be produced per step.

Every optimization must leave the image unchanged at the same simulation step:
- An optimized function is tested against its unoptimized reference on synthetic state built in the test; their buffers must be equal.
- An owner-local real-data run compares the frame at the same step numbers with the optimization on and off, printing only pass/fail.

The cadence change from the fixed step is intentional and separate: vblank sequences now take true PAL time.

## Acceptance

1. **Steps match the clock.**
   - At a finite speed s, with no dropped time and enough budget, 10 s of wall time give `floor(10000 × s / 20)` ± 2 steps, measured by the harness on the engine clock.
   - Separate tests cover:
     - a stall longer than `CAP` (the steps stop, `dropped_ms` grows, then the pace recovers);
     - an overload with too small a budget (the indicator shows "behind");
     - Max (k is bounded by `MAX_STEPS` and by the budget; switching speed resets the debt).
2. **Vblank waits are true PAL frames on every display.**
   - The opening's frame at step N is the same at 60 Hz and 144 Hz rAF rates.
   - With the clock receipt, the opening's wall duration at 1× matches its step count × 20 ms on both rates.
   - The Use machine's 101-vblank wait is 2020 ms of engine time.
3. **Keys.** `-`/`+` change the speed at the prompt. They still reach the game in the inventory panel and while typing a name. Numpad ± always changes the speed. The badge and buttons show the current speed.
4. **Receipts.** The 1× fidelity receipts stay within their bounds, re-run at 20 ms steps: start fight, castle step, held walk. At 4×, a fight turn takes ¼ of its 1× wall time. A held click does not double-step: an in-progress press is measured at 1× if needed.
5. **Optimizations.** Each one passes its image-equivalence tests (above).
6. **Gates and live check.** All gates pass, plus new web tests for the pacer, the keys and the badge. The live check shows the speed control working on production.
