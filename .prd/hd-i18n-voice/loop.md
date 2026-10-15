---
title: ultima6 hd-i18n-voice — loop
type: loop
tags: [ultima6]
---

# Loop: hd-i18n-voice

Status: in-progress. Research is done (2026-10-15, notes kept locally in `~/Games/ultima6/research/2026-10-15/`). The specs are written:

| Spec | User item |
|---|---|
| `06-fixed-tick-speed.md` | 4 |
| `07-hd-art.md` | 1 |
| `08-i18n-ko.md` | 2 |
| `09-npc-voice.md` | 3 |

Spec review: gpt-6-astra reviewed 06–09 (all CHANGES); every must-fix is folded into rev 2 of each spec. Next: the opus spec review of rev 2, then the units below in order, starting with T1.

## Order, and why it differs from the user's numbering

The work runs one item at a time, as the user asked. The order follows dependencies:

1. **06 speed.** Its frame contract (one RGBA conversion per rAF, only when the frame changed) must exist before an HD frame does. It also fixes the vblank bug that every later receipt would otherwise inherit. It is the item the user feels most ("지금 너무 느린데").
2. **07 HD.** Its scene tap and S× canvas carry 08's 2× Hangul font and give a larger, clearer text window.
3. **08 Korean.** Its text path and extractor feed 09's speech tap (Korean lines) and 09's NPC catalog.
4. **09 voice.** Its Korean speech needs 08, and its bundles need the conversation disassembler that 08 builds.

## Work units (one agent each; branches `feat/<unit>`, worktrees `.worktrees/feat-<unit>`)

| Unit | Spec | Scope | Needs |
|---|---|---|---|
| T1 | 06 | `u6-core` `Pacer` + `STEP_MS` + tests; `u6-web` `step/present/pace/set_speed/speed/plus_minus_owner` (keep `tick`) | — |
| T2 | 06 | `site/app.js` fixed-step loop, key intercept, badge and Speed buttons, `localStorage`, gain ramp above 4×; harness `GLUE_HOOKS` wraps `step`; web tests | T1 |
| T3 | 06 | native `run_window` on `Pacer`, `HEADLESS_TICK_MS` 20, keys, speed in the title; re-run the 16 ms receipts at 20 ms | T1 |
| T4 | 06 | O1 + O2: one RGBA per changed frame; skip idle map re-render; `merge_shores` on its counter | T2 |
| T5 | 06 | O3: planar row/word blits in the opening | T1 |
| T6 | 06 | O4: release profile flags; wasm size receipt | — |
| H0 | 07 | scene draw list in `blit_back` (zero cost when off) + record dirty bit + `u6-web` scene export | T4 |
| H1 | 07 | `site/hd.js` S× canvas + replay + logical pointer mapping; MMPX×4 baseline in wasm | H0 |
| H2 | 07 | HD pack format, drop/IndexedDB/forget, limits | H1 |
| H3 | 07 | `tools/hd` inventory/atlas/run/ingest/pack with the gate; synthetic tests | H2 |
| H4 | 07 | ops: run `tools/hd` on the owner's files with imagegen (portraits → bitmaps → terrain → actors), load the pack in the owner's browser, read the frames | H3 |
| L1 | 08 | text path: `u16` line, UTF-8 assembler, Hangul table, runic guard, cell padding, josa, `%1$s` | — |
| L2 | 08 | locale layer + print-boundary and converse hooks + pack parser; identity-pack receipt | L1 |
| L3 | 08 | conversation disassembler (04 W3a's table) + run/group extractor; the catalog shared with 09 | — |
| L4 | 08 | page: language gate, en/ko UI catalog, pack drop/forget, hidden-input IME, keyword aliases | L2 |
| L5 | 08 | `tools/i18n` glossary, speakers, translate, review, pack, coverage receipt, upload boundary; style guide | L3 |
| L6 | 08 | ops: run L5 on the owner's files, owner sample, full run, load, read frames | L4, L5 |
| V1 | 09 | `u6-core::speech` tap + `take_speech` | L1 |
| V2 | 09 | `tools/speech/fetch-models.sh`, parts + manifest + LICENSE/NOTICE; preview deploy | — |
| V3 | 09 | `site/voice/` worker and player, opt-in, chip, terms notice | V1, V2 |
| V4 | 09 | profiles: mixer, type-class table (no LLM) | V3 |
| V6 | 09 | personality profiles from bundles, local voice pack tool, coverage receipt, upload boundary | V4, V5 |
| V5 | 09 | NPC bundle exporter (wasm), IndexedDB, download | L3 |

Each unit is reviewed by gpt-6-astra and opus before it merges. Each item ends with a merge, a preview deploy with a live check and a frame read, then production.

## Carried from browser-port (still open there)

These continue in `browser-port/loop.md` and do not block this loop:
- the round-6 units intake-budget, zip-slice and nostep;
- the P3 trinity, waiting on the grok slot.
