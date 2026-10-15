---
title: ultima6 — 06 fixed tick, speed control, performance
type: spec
tags: [ultima6, timing, web, native]
---

# 06 — Fixed game tick, - / + speed control, performance

Status: planned (rev 3, 2026-10-15. rev 2 folded in the gpt-6-astra review: `ran` in the API, Max as its own mode, admission loss counted, image-equivalence for optimizations. rev 3 folds in the opus review, checked against the code: a held press runs at 1×, separate clamp and budget losses, a per-key ownership bitmask that also sees the `u` program, `=` as the free "faster" key, O2 keyed on what a step really changes and kept off while sparkle or dissolve is on). User item 4 (`hd-i18n-voice/ssot.md`, verbatim): "게임 루프(틱)을 고정해서 속도 유지해줘. 이거 키보드 -+로 조절할 수 있도록해줘. (지금 너무 느린데 최적화도 해야할 것 같음)". Research notes are kept outside the repo, in `~/Games/ultima6/research/2026-10-15/r4-speed-findings.md`.

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
- **The keys `-` and `+` collide with the game in four places:**
  - the stats/inventory panel's next/previous member, modes 0x90/0x92 (`ui/target.rs:361-362`);
  - the roster scroll, mode 0x91 (`:373-374`): `+` while `first < size − 5`, `-` while `first ≠ 0`. The roster is the default panel (`saveload/restore.rs:121`) and a party holds up to 8 (`converse/party.rs:13`), so with 6 or more members `+` belongs to the game at the ordinary prompt;
  - a typed line: in `game` while `ui.input.line_input` is set (`ui/keyread.rs:161,217`); in `u`, the name entry accepts `-` (`intro/create.rs:145`);
  - the `u` main menu, where `-` and `=` pick songs 0xb and 0xc, the original's music-test keys (`intro/mod.rs:311,314`).

  Unshifted `=` (0x3d) is free in `game`: both panel arms pass it through (`target.rs:367,379`) and no command uses it. The `u` program's state is local to its future (`intro/mod.rs:122`), so the engine cannot see it without a port-only flag. `key_from_dom` maps characters by DOM `key` (`u6-web/src/lib.rs:98,117-121`) and never delivers function keys, so in the panels `-`/`+` are the only keyboard way to cycle members. Native `map_key` never delivers `-`, `=` or shifted characters (`u6-native/src/main.rs:375-407`).
- **A held press is measured in engine ms.** `HELD_WALK_MS = 660` (`ui/target.rs:42`, used at `:160-178`): at 8×, a click held longer than about 82 ms of wall time would walk.
- **The idle map render is not pure.** While `gfx.sparkle ≠ 0` it draws 200 values from the game RNG (`gfx/view.rs:142-148`), and it clears `gfx.dissolve` (`:161-165`). `anim_counter` rises on every `anim_tick` (`gfx/anim.rs:137`), which every idle step runs (`view.rs:182-183`, `ui/keyread.rs:88`); `merge_shores` already runs only on even counters (`anim.rs:150-151`).
- **Hot spots (world, idle):** `write_rgba` 26%, `gfx::blit` 26%, `Io::with_buffers` 22%, `merge_shores` 8%. The map is re-rendered every idle poll (`render_window` 49% inclusive). In the opening, `intro::mem::blit_masked` takes 36% and `Chip::set_pixel` 35%.

## Design

1. **Fixed step.** The engine is stepped only in `STEP_MS = 20` (one PAL frame; reuse `combat::present::FRAME_MS`).
   - A step is one `Engine::tick(20)`, so `generation` counts PAL frames and every `vblank()` wait is correct on any display.
   - Engine-ms waits (`pace`, `sleep`) are unchanged.
2. **`Pacer` in `u6-core`.** A pure struct with no clock, shared by web and native. It commits on report:
   - `due(wall_dt_ms, held) -> n` at a finite speed s: `acc += clamp(dt, 0, CAP) × r`, where r = s, or 1 while `held`; returns `min(floor(acc / 20), MAX_STEPS)` without consuming. The wall time cut by the clamp, `max(dt − CAP, 0)`, is added to `clamped_ms`.
   - `held` is true while a pointer button is down: the pointer handler's state after this frame's queued input (the last queued pointer event, else the last one the engine took). A held press is therefore measured at 1× in every mode, Max included, from the frame it is queued in.
   - The press edge (`held` turning true) resets `acc` to 0 and counts nothing as lost, so steps owed at a faster speed are never run under a held button. At 1×, where the rate does not change, what is owed is kept. While held, Max keeps a 1× accumulator, fractional steps included.
   - The host runs `k ≤ n` steps, stopping early when its frame budget is spent. k may be 0.
   - `ran(k)`: `acc -= 20 k`. Anything owed beyond `CAP × r` is converted to wall ms (÷ r) and added to `dropped_ms`. `dropped_ms` (budget loss only) is the one source for the "behind" indicator; `clamped_ms` (a hidden tab, a debugger pause) is not shown as "behind".
   - Max is a separate mode, not a large s. `due` returns `MAX_STEPS` (the 1× count while `held`), and the host runs steps until its budget is spent. `ran(k)` then sets `acc` to 0, so Max never owes and never reports "behind". Any change of speed or mode resets `acc` to 0.
   - Constants:
     - `CAP` = 250 ms of wall time per frame;
     - `MAX_STEPS` = 64 per frame;
     - frame budget = 0.5 × the measured rAF interval, never below 3 ms.
   - Unit tests: over T ms the host gets exactly `floor(T × speed / 20)` steps, ±1; a 150 ms hold at 8× and at Max gets the 1× count, also when steps were owed before the press; the press adds nothing to `dropped_ms`.
3. **Web API.**
   - `tick(ms)` (= `step` + `present`) stays until the page and the harness move to `step` (T2), then it is deleted with its 7 call sites in `crates/u6-web`. The ~170 `.tick(&` sites elsewhere call `Engine::tick`, which this spec does not change.
   - New:
     - `step(ms)`: engine tick plus audio sync, no RGBA;
     - `present()`: one RGBA write;
     - `pace(dt) -> u32` (= `due`);
     - `ran(k)`;
     - `set_speed(x)` (x = 0 means Max);
     - `speed()`;
     - `dropped_ms()`, `clamped_ms()`;
     - `keys_owned() -> u8`, a bitmask of the characters the engine uses right now: bit 0 `-`, bit 1 `+`, bit 2 `=`.
   - The page loop becomes:
     1. `n = pace(now - last)`.
     2. Step: `k = 0`; while `k < n` and (k = 0 or within the budget), call `step(20)` and `k += 1`.
     3. Report: `ran(k)`.
     4. Present: if k > 0, `present()` (06 O1 later skips unchanged frames), then draw.
     5. Once per frame: audio and `checkSave`.
   - `driver.sync` runs inside every `step()`, because `CALL_LOG` is sized for one step's calls (`sound/mod.rs:69`). `feedAudio` (render and post) runs once per frame.
4. **Speed presets:** 1×, 1.5×, 2×, 3×, 4×, 6×, 8×, Max.
   - Max is as many steps as fit the frame budget; the badge shows the speed actually reached.
   - The default stays 1× (the oracle baseline). The viewer's choice persists in `localStorage`, guarded with try/catch.
   - Oracle waits stay in engine ms: a fight turn is the same number of steps at every speed, and only the wall time shrinks.
5. **Audio.** Music keeps playing in wall time at 1×. Above 4×, its gain ramps to 0 and is restored on return. Scaling the tempo without changing pitch through the CIA timer is a later option, not v1.
6. **Keys.** The page intercepts `keydown` before `game.key_down`, matching the DOM `key` character so that every keyboard layout works:
   - `NumpadAdd` / `NumpadSubtract` (by `code`) always change the speed.
   - `=` is faster, `-` slower, `+` faster, each unless its bit in `keys_owned()` is set. The bits come from:
     - a line being typed, in `game` (`line_input`) or in `u` (a port-only flag on `Io`, set and cleared by the intro's read loop): all three;
     - panel mode 0x90 or 0x92: `-` and `+`;
     - roster mode 0x91: `+` while it can scroll down, `-` while it can scroll up;
     - the `u` main menu (the same `Io` flag): `-` and `=`.
   - So in `game`, the unshifted `=`/`-` pair works at the ordinary prompt even with a large party, and `+`/`-` still cycle and scroll the panels.
   - A one-step-stale answer is accepted.
   - Native binds `-`, `=` and the numpad keys to speed directly (its `map_key` never sends them to the game) and shows the speed in the window title.
7. **UI.**
   - A badge over the canvas: shown for 1.5 s after a change, and kept visible while the speed is not 1×.
   - A Speed group beside Mute: `−  2×  +`, plus "behind" when steps are being dropped. It also works on touch screens.
   - The badge's title says that `-`/`+` belong to the panel while it is open.
8. **Native.** `run_window` uses the same `Pacer`, with audio sync after every step and one top-up per frame. Headless `HEADLESS_TICK_MS` changes from 16 to 20.

## Optimization (after the fixed step, in this order)

| # | Change | Cost it addresses | Expected gain |
|---|---|---|---|
| O1 | One RGBA conversion per rAF, only when the frame changed | `write_rgba` 0.05 ms per step | (steps in the frame − 1) × 0.05 ms per frame |
| O2 | Redraw only the window cells whose inputs changed (measured: a whole-render skip fired once in ~2,200 idle refreshes, because the overlay frame steps every poll and water every even count). A cell's key: each word drawn with its record, masktype and record version, the overlay frame, the clock's hour and minute, the cursor and border tile. Nothing cached while `sparkle`, `dissolve` or XOR plotting is on, or when the back buffer was touched since the last render. The shore merge is skipped only when no target record is also a source | `render_window` 49% | idle tick 82 → 25 µs (native, measured) |
| O3 | Opening: row- and word-wide planar blits instead of per-pixel `set_pixel`/`blit_masked` | 71% of the opening step | 3–5× on the opening |
| O4 | `[profile.release] codegen-units = 1`, no LTO (Cargo skips LTO for u6-web's cdylib + rlib anyway, and a cdylib-only fat-LTO wasm measured 9.8% bigger) | — | wasm −19.6% in size (1,759,472 → 1,415,380 B); no speed gain |

O1 is also the contract the HD renderer (07) depends on: an HD frame must not be produced per step.

Every optimization must leave the image unchanged at the same simulation step:
- An optimized function is tested against its unoptimized reference on synthetic state built in the test; their buffers and the whole `State` (the RNG included) must be equal.
- An owner-local real-data run compares the frame at the same step numbers with the optimization on and off, printing only pass/fail.

The cadence change from the fixed step is intentional and separate: vblank sequences now take true PAL time. At 1×, that makes them slower than today on fast displays: the opening runs 1.2× fast at 60 Hz and 2.88× at 144 Hz today, and the Use machine's wait grows from 1683 ms to 2020 ms at 60 Hz. The report to the user says this in one sentence, next to the speed keys that now cover it.

## Acceptance

1. **Steps match the clock.**
   - Exact step counts are proven in the Rust `Pacer` unit tests.
   - In the page, at a finite speed s, over 10 s of wall time with `dropped_ms` and `clamped_ms` unchanged, the harness measures `floor(10000 × s / 20)` ± 2 steps on the engine clock. A run where either counter moved is reported as such, not failed.
   - Separate tests cover:
     - a stall longer than `CAP` (the steps stop, `dropped_ms` grows, then the pace recovers);
     - an overload with too small a budget (the indicator shows "behind");
     - Max (k is bounded by `MAX_STEPS` and by the budget; switching speed resets the debt).
2. **Vblank waits are true PAL frames on every display.**
   - With a rAF and `performance.now` shim in the harness, the opening's frame at step N is the same at 60 Hz and 144 Hz. The opening is the owner's game data, so this runs owner-local and prints pass/fail only.
   - With the clock receipt, the opening's wall duration at 1× matches its step count × 20 ms on both rates.
   - The Use machine's 101-vblank wait is 2020 ms of engine time.
3. **Keys.**
   - `=`/`-` change the speed at the prompt, also with a 6-member party.
   - `-`/`+` still reach the game in the inventory panel, in a scrollable roster, while typing a line in `game`, while typing a name in `u`, and in the `u` main menu.
   - Numpad ± always changes the speed.
   - A non-US layout, tested with a synthetic `key` on another `code`, works the same.
   - The badge and buttons show the current speed.
4. **Receipts.** The 1× fidelity receipts stay within their bounds, re-run at 20 ms steps: start fight, castle step, held walk. At 4×, a fight turn takes ¼ of its 1× wall time. A 150 ms click at 8× and at Max walks exactly as at 1×.
5. **Optimizations.** Each one passes its image-equivalence tests (above).
6. **Gates and live check.** All gates pass, plus new web tests for the pacer, the keys and the badge. The live check shows the speed control working on production.
