---
title: ultima6 browser-port — loop
type: loop
tags: [ultima6]
---

# Loop — browser-port

Status: in-progress

## Build facts (measured 2026-10-14, gate updated 2026-10-15)

- rea 6.4.0 built from 767fe0f0c (`npm ci`, `npm run build:cached`), installed via
  `npm pack` + `npm install -g`; Ghidra 12.1.4 + JDK 21 provider healthy
  (`rea doctor --provider ghidra`); arm64 decompiler built from source.
- `rea inspect game` → `unsupported_target / file_format_rejected` (Amiga hunk);
  raw-binary admits x86/x86_64/arm/arm64/6502 only.
- Ghidra ships `Processors/68000` (68000.ldefs).
- ScummVM web build: `./dists/emscripten/build.sh setup configure make dist
  --disable-all-engines --enable-engine=ultima6` (emsdk 4.0.10), log
  `~/Games/ultima6/scummvm-wasm-build.log`.
- Local data (never committed): `~/Games/ultima6/{original,img,files,unpacked}`.
- REA on the real binary: `tools/hunk2raw.py game` → `~/Games/ultima6/re/game.bin` (35 hunks,
  base 0x100000) → `rea inspect … --architecture m68k` = 1,383 procedures, 515 strings
  (41 s). Bulk decompile: `node tools/rea/dump-all.mjs` (one MCP session; first batch 30 s
  for import, then fast) → `~/Games/ultima6/re/dumps/game/` 1,383/1,383 status ok, 6.5 MB.
- Rust gates: `scripts/cargo.sh fmt --all --check` · `clippy --workspace --all-targets -D warnings`
  · `test --workspace --profile gate` (release + overflow checks + debug assertions) · `bash scripts/build-web.sh`
  (wasm32 via rustup stable 1.96.1) · `cd tools/web && npm ci && node test-web.mjs` ·
  `python3 -B tools/oracle/test_amifs.py` · `python3 -I -B tools/oracle/test_fight.py` · `python3 -I -B tools/rea/port_map.py`.
- Deploy channel: CF Pages project `ultima6` (account b6eba338…, created via API — `wrangler
  pages project create` autoconfigures a Worker instead; that Worker was deleted) →
  `npx wrangler@4 pages deploy site --project-name ultima6 --branch <b>`; preview alias
  https://preview.ultima6.pages.dev. Custom domain + Vercel CNAME `ultima6` at milestone 1.

## Open gaps from the M1 parity receipt (snapshot in loop.done.md)

| Gap | Where | Cause | State |
|---|---|---|---|
| G1 | gargoyles in the throne room (oracle) absent in port | egg hatching 0x1319b2 + monster spawn 0x13121a were stubs | ported by M2-objcmds (start frame 99.20%); re-measure on the M2 deploy |
| G2 | last command icon differs | icon reflects the combat flag set in the shipped DATA1 state | re-measure on the M2 deploy |

re-verified: G1 · gargoyles stand in the throne room in the M2 production frames (dispatcher-read, live-check2 on https://ultima6.2lab.ai, deploy 332fdd9e) · main 624477d — parity % not re-measured

## M2 follow-up units (open, from the round-3/4 reviews; base integrate/m2 093f583 = main 624477d code)

| Unit | Branch | Scope | State |
|---|---|---|---|
| web-flake | `fix/web-step-flake` | `tools/web` `stepSideways` threw "16 steps blocked in a row" in 2 of 17 runs on H0 code, at load 16–22: once at the worker's round 1, once in the dispatcher's merged gate (2026-10-16). It did not recur in 10 paired runs, 5 on main 0e7ebfa and 5 on the merged tree, up to load 29. Find out why the hero stays boxed in for 16 tries (seed from the clock? stale page state under load?), from a captured failure with counts only, and fix the test or the page | open, to dispatch after H1 starts |

Open items carried from WU-P7b (assign in M2-int):

## Next

- After the follow-ups: merge → gates → preview → live check → production → live check.
- Then P3 per `.prd/04-refactor-p3.md` (rev 5). Trinity: 2 of 3 approved rev 4. The grok-4.7 slot was rate-limited at 13:05 and again at 15:04 on 2026-10-15: one account is paused, and the other is at 100% of its weekly limit until 2026-10-16 15:34 KST. Retry 2026-10-16 15:41; no 2-of-3. Rev 5 (the HD/4K seam: W10, acceptance 7, `05-hd-output.md`) still needs all three engines. User step 4 = redeploy.
- After P4: research the HD/4K phase (user 2026-10-15, `05-hd-output.md` §Research). The research list is there.
