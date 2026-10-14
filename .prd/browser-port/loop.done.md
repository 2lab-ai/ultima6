---
title: ultima6 browser-port — loop done
type: loop
tags: [ultima6]
---

# Loop done — browser-port (append-only)

## R1 — closed

Rust workspace, six M1 subsystems, boot and command loop, oracle, M1 deploy.

## Round 1 — work units (rev 2: Rust main source)

| WU | Scope | Result | State |
|---|---|---|---|
| WU-1 | rea `m68k` raw-binary | rea `9466e0712` (feat/m68k-raw-binary, local); 199 tests; fixture decompiles | done |
| WU-2 | hunk flattener + layout | `2725b02`; 30 tests; game = 35 hunks, no overlay, entry 0x100000 | done |
| WU-3 | Amiga format spec | `c142d64`, `0eae222` formats.md | done |
| WU-6 | TS intake (oracle for R1) | `8b02eda`; 33 tests; ADF sha = greaseweazle | done (delete after R1 parity — R1 done) |
| WU-R0 | Rust workspace skeleton | `cc620ab`; 14 tests; wasm 35 KB | done |
| WU-R1 | `u6-intake` Rust | `6b20c4e`; 30 tests incl. real zip 0.68 s (re-run by dispatcher) | done |
| WU-R2 | `u6-formats` parsers | `1c442b3`; 44+12 tests (re-run by dispatcher) | done |
| WU-4a | tile graphics + palette | `da9a115`, `1afd5f0`; 2048 tiles; sheet read by dispatcher = real U6 tiles | done |
| WU-4b | function inventory | `5630231`, `9d93cb4`; 1383/1383; M1 = 320 game fns + 41 crt, 45 stubs | done |
| WU-C0 | u6-core engine/State/RNG/seams (03 rev 2) | `65b421c`, `063b0f1`; 127 tests (re-run by dispatcher); zip→intake→State→Engine frame | done |
| WU-O | Amiga emulator oracle (AROS) | `6486e6b`, `7bf6d1a`, `9fc76d2`; floppies built from installu6's copy table; original reaches Britannia via `--game-first` (launcher 1-byte patch + objlist 0x1C71/0x1C72 = 1); intro program `u` still stalls under AROS (not root-caused) | done (game path) |
| WU-P1 | gfx world view | port/gfx `57d6c8a`; 18 fns; throne-room frame read by dispatcher | merged → integrate/m1 |
| WU-P2 | map | port/map `43bb7a4`; 16 ported + 3 replaced; view/light grids from real data | merged → integrate/m1 |
| WU-P3 | objects | port/objects `8ec66c3`; 60/70 + 4 helpers; 16838 records load + write back | merged → integrate/m1 |
| WU-P4 | ui | port/ui `7faa720`; 25 ported, 3 partial, 10 stubs; roster + stats panels render | merged → integrate/m1 |
| WU-P5 | actors | port/actors `7004468`; 52/52; NPCs walk to schedules at 12:00 | merged → integrate/m1 |
| WU-P6 | converse | port/converse `086b9bf`; 20/20; 200 scripts; real NPC dialogue coherent | merged → integrate/m1 |
| WU-P7 | boot + command loop + seam reconciliation + web IndexedDB | port/main `6dce550`; 315 tests (re-run by dispatcher); start/walk/talk/web/reload receipts read by dispatcher; M1 406: ported 219, replaced 76, stubbed 65, planned 34 | done |
| WU-P7b | external review CHANGES (lifecycle race, unsupported-command honesty, canvas focus, memory, Attack targeting, error boundary) + `_headers` CSP | `a0f7443`, `ec598e0` (MUST-1/2/3, NICE-3, input limits, CSP); 10/10 browser tests (re-run by dispatcher); agent stopped by API 429 mid NICE-2 — WIP `0864d8b` on port/main | done — `0864d8b` (page notice: party/solo need leaving combat) merged `aa145b8`, production `a549b08a` |
| WU-D | Deploy ultima6.2lab.ai | main `a28f4c2` (merge of ec598e0) → Pages production `108af585`; custom domain ultima6.2lab.ai active (Vercel CNAME rec_b5896faf… → ultima6.pages.dev); live playthrough in headless Chromium: drop → walk → reload from IndexedDB, 10 requests all same-origin, 0 console errors | done (M1 interim) |

Open review items: gpt6 spec review CHANGES ×8 applied in `d749513`; WU-C0 notified.
TS intake deleted in `1a414a6`. Port worktrees gfx/map/objects/ui/actors/converse removed after the clean+on-remote+merged gate.

## M1 parity receipt — start frame (2026-10-14, dispatcher-read)

Oracle `~/Games/ultima6/oracle/shots/play-20261014-190647/ingame.png` (crop at (93,44), 320×200) vs
port `port/main` headless start frame. Receipts kept outside the repo:
`~/Games/ultima6/receipts/parity-start.png` (sha256 dd74b474…), `parity-diff2.png` (f769019e…).

- Raw RGB agreement is 4.2% because the emulator expands 4-bit channels non-linearly (15→240,
  mid values one nibble low) while the port uses n×17. Under a palette-index mapping
  (each port colour → majority oracle colour) agreement is **98.80% whole screen, 99.23% outside
  the map window**.

| Gap | Where | Cause | State |
|---|---|---|---|
| G1 | gargoyles in the throne room (oracle) absent in port | egg hatching 0x1319b2 + monster spawn 0x13121a are stubs | open (M2 with combat) |
| G2 | last command icon differs | icon reflects the combat flag set in the shipped DATA1 state | open |
| G3 | one NPC sprite + brazier flames | animation phase / idle-pass RNG timing | not a defect (timing) |
| G4 | pointer at (0,0) | emulator mouse sprite | not a defect |

## Interim deploy receipt — 2026-10-14 (user: "중간 보고하고 중간 산출물 정리해서 한번 먼저 올려줘")

- main `a28f4c2` = merge of port/main `ec598e0` (+ deploy script). Gates on the merge: fmt, clippy
  -D warnings, 323 tests, wasm 706,192 bytes, `node tools/web/test-web.mjs` 10/10.
- Cloudflare Pages: preview `7bf16d98` (checked first), production `108af585`; headers on every
  path: CSP `default-src 'self'; connect-src 'self'; script-src 'self' 'wasm-unsafe-eval' …`,
  nosniff.
- https://ultima6.2lab.ai: 200; headless Chromium with the user's zip → game started → 3 steps
  south → reload → restarted from IndexedDB; requests 10, foreign 0, console errors 0.

## Round 2 status — API rate limit 2026-10-14

All M2 agents and WU-P7b stopped on a server-side HTTP 429 ("retry in ~12,600 s"). Partial work
committed as WIP and pushed (not built): m2/combat `283a748`, m2/magic `c224a2a`, m2/usecmd
`d967d6a`, m2/inventory `ac4924a`, m2/saveload `269cec6`, m2/sound `260c2a6`, m2/intro
`3d4f493`, port/main `0864d8b`; m2/objcmds had no uncommitted files. Resume each unit from its
worktree when the limit clears.

## Redeploy — 2026-10-14

main `aa145b8` (merge of port/main `0864d8b`): 323 tests, web tests 10/10. Production deployment
`a549b08a`; https://ultima6.2lab.ai serves the corrected notice (party/solo listed under "not yet":
the shipped state starts in combat mode and leaving combat is not ported). Live playthrough:
requests 10, foreign 0, console errors 0.
