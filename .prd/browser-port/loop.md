---
title: ultima6 browser-port — loop
type: loop
tags: [ultima6]
---

# Loop — browser-port

Status: in-progress

## Build facts (measured 2026-10-14)

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
  · `test --workspace` · `bash scripts/build-web.sh` (wasm32 via rustup stable 1.96.1).
- Deploy channel: CF Pages project `ultima6` (account b6eba338…, created via API — `wrangler
  pages project create` autoconfigures a Worker instead; that Worker was deleted) →
  `npx wrangler@4 pages deploy site --project-name ultima6 --branch <b>`; preview alias
  https://preview.ultima6.pages.dev. Custom domain + Vercel CNAME `ultima6` at milestone 1.

## Open gaps from the M1 parity receipt (snapshot in loop.done.md)

| Gap | Where | Cause | State |
|---|---|---|---|
| G1 | gargoyles in the throne room (oracle) absent in port | egg hatching 0x1319b2 + monster spawn 0x13121a were stubs | ported by M2-objcmds (start frame 99.20%); re-measure on the M2 deploy |
| G2 | last command icon differs | icon reflects the combat flag set in the shipped DATA1 state | re-measure on the M2 deploy |

## Round 2 — milestone 2 (rest of the game), dispatched 2026-10-14

Base origin/port/main `6dce550`. One worktree per unit (`.worktrees/m2-<unit>`, branch `m2/<unit>`).
`u` (intro executable) REA: 479 procedures, 317 strings, 479/479 decompiled (`~/Games/ultima6/re/dumps/u/`).

| WU | Scope | State |
|---|---|---|
| M2-combat | hunk 13 + Attack + combat mode + NPC combat worktypes + missile effects | m2/combat `114621d`; 53 + 7 hunk-18 fns; 334 tests; gargoyle fight messages match oracle wording | done → M2-int-a |
| M2-magic | hunk 11 + Cast + spellbook + reagents | m2/magic `4c4ba6f` + m2/magic2 `4814fc5`; all 22 remaining spell routines ported; 384 tests; 17 spell receipts | done → M2-int-b |
| M2-objcmds | hunk 15 Look/Get/Drop/Move/doors/traps + eggs/monster spawn | m2/objcmds `5eea93a`; 339 tests; start frame 99.20% vs oracle (seed 0x1d3), 98.76% seed 0; Move-to-member partial | done → M2-int-a |
| M2-usecmd | hunk 16 Use + per-object handlers | m2/usecmd `73300c7`; 53/59; 329 tests; 8 object kinds; duplicated 10 hunk-15 fns (dedupe in int) | done → M2-int-a |
| M2-inventory | hunk 8 inventory/Ready/Unready + 0x128ce8 panel + hunk 14 Rest | m2/inventory `3227cc3`; 332 tests; inventory panel 100.00% vs oracle (Avatar + Dupre, controls 85.6%/86.0%); Rest 8 h receipt; Ready/Unready reachable only by click 0x10619c → M2-mouse | done → M2-int-b |
| M2-saveload | Save/Restore, byte-compatible savegame, SaveStore trait | m2/saveload `ad43c95`; 333 tests; original's save loads in port, port re-save = original re-save except frame bits + 1 stack-garbage byte | done → M2-int-a |
| M2-sound | MED player + Paula emulation (u6-audio) + music state | m2/sound `0f7958c`; 362 tests; start song = song 7 ULTIMA_Da; envelope vs oracle audio r 0.952, tempo drift +0.01 s / 7.5 s; host wiring (AudioDriver + AudioWorklet) + 4 commands.rs edits pending | done → M2-int-b |
| M2-intro | `u`: intro, menu, character creation, hand-off | m2/intro `355253d` + m2/intro2 `59d1008`; 351 tests; opening (storm/stones/moongate), Introduction story, Transfer (U4 rost.bin / U5 saved.gam) ported; `u` port map 110 ported / 3 stubbed (music, local time); oracle intro unblocked by patching OwnBlitter/DisownBlitter (recipe ~/Games/ultima6/oracle/patch-intro2/) → static frames 100% vs original | done → M2-int-c |
| M2-mouse+ui | pointer input, ui mouse subtree incl. 0x10619c, pointer sprite, remaining planned ui/gfx | m2/mouse `6cfc225`; 359 tests, web 11/11; whole click-only playthrough (talk, walk, inventory, Unready/Ready (14,2)↔(0,16)); turned flight sprites; sprite alignment vs oracle unverified | done → M2-int-c |
| M2-int-a | merge combat+objcmds+saveload+usecmd, dedupe hunk 15, wire seams + commands.rs, ctrl keys | integrate/m2 `b324737`; 396 tests, web 11/11; Attack/B/Look/Get/Drop/Move/Use/Ctrl-S/Ctrl-R/overview live in the command loop; port map ported 357/599 | done |
| M2-int-b | merge magic2+inventory+sound, wire Cast/Rest/sound, web IndexedDB saves + AudioWorklet | integrate/m2 `b312930`; 543 tests, web 14/14, port map ported 471/599 (gates re-run by dispatcher 2026-10-15); Light/Magic Arrow/Rest/inventory/save→reload frames read by dispatcher | done |
| M2-int-intro | merge m2/intro2, run `u` first as the launcher does, persist a created character | `.worktrees/int-intro` branch `int/intro` from `b312930`, dispatched 2026-10-15 | in flight |
| M2-int-mouse | merge m2/mouse, reconcile pointer/sprite/fill/spellbook seams, first click = audio start + game click | `.worktrees/int-mouse` branch `int/mouse` from `b312930`, dispatched 2026-10-15 | in flight |
| M2-int-c | dispatcher merges int/intro + int/mouse into integrate/m2, external review, merge to main, preview → live check → production → live check | after both | next |

Open items carried from WU-P7b (assign in M2-int):
- PP20 output is bounded per file (16 MiB) but not in aggregate (u6-intake).
- wasm memory stays ~192 MiB per tab because the whole zip is copied in; read only the disk
  entries with `Blob.slice` (~30× less).
- Panel keys inside `ui::wait::read_key` (inventory scroll, Ready/Unready, spellbook page,
  container) do nothing silently — say "not implemented" until M2-inventory/magic land.
