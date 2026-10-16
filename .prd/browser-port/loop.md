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
| chooser-flake | `fix/chooser-flake` | `tools/web` `keyboard only` timed out at `test-web.mjs:359` (`page.waitForEvent('filechooser')` after Space on the picker) in 1 of 3 full runs at load 114–131 on 2026-10-16. It passed 4 of 4 alone at load 157–251. Find why the chooser event is late or missed under load (focus? the default 30 s timeout? the event registered after the press?) and make the test wait for the right signal, without only raising timeouts | open |
| wake-in-fight | `fix/wake-in-fight` | After the hero dies in the opening fight, combat breaks off and the party wakes on the start cell between the same gargoyles, which kill him again. In the port, 0x1095bc (`resurrect_party`, `crates/u6-core/src/map/travel.rs:469-470`) breaks off combat and teleports the party to (0x133, 0x160, 0), the start cell. The gargoyles in assault (worktype 8) stay beside that cell, and the members, out of combat, no longer strike back. Found by web-flake: seen headless on seeds 1161, 2957, 5113, 6074 and 7608, and in the browser. Check against the original (oracle) whether the original does the same, and fix the port if it differs | open, to dispatch |

Open items carried from WU-P7b (assign in M2-int):

## Next

- After the follow-ups: merge → gates → preview → live check → production → live check.
- Then P3 per `.prd/04-refactor-p3.md` (rev 10). Trinity approved it 3 of 3 (grok-4.7, gpt-6-astra, fable) on 2026-10-16, at round 6. The rounds after rev 4 were: grok's round 1 on rev 5 (spec 07 had already shipped the HD seam, so W10 was removed), the acceptance 7 rounds, and the W7c gateway-auth rounds. The §3.a AI transport was decided 3 of 3 at round 2: web = loopback only, owner now = u6-native. Its CSP change ships only with the user's approval, asked when W7b is ready. Notes for the W7c dispatch brief (fable rounds 5–6): implement at llmux `client_auth`'s one `resolve` call (`src/proxy/server.rs:1795`), joining `origin.is_none()` to the loopback argument so both API paths close; pin the `keys.rs:162-167` citation to llmux `a162822`. The answers are in the session scratchpad, `trinity-p3-r*-{grok,gpt6,fable}.md` and `trinity-csp-r*-*.md`. Not dispatched yet: P3 starts after the current 06–09 work. User step 4 = redeploy.
- After P4: research the wider view (user 2026-10-15, `05-hd-output.md` §4 and its research list); x2/x4 resources shipped in spec 07.
