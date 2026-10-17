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

Spec review: every spec had a gpt-6-astra review and an opus review read against the code, all folded in. 06 is at rev 3, 07 at rev 6 (rev 5: what H0 and H3 measured; rev 6: the neighbour-seam gate), 08 at rev 3, 09 at rev 3. Closed units are in `loop.done.md`.

Shipped: 06 is complete in production (9f831837 at main bf93585). 07 H0 (source plane), H0b (plane hardening), H1 (the HD compositor, MMPX at 2× or 4×, on by default) and H3 (the pack format and the imagegen tool) are in production (80cdffb2 at main 8826434). In flight:
- H2, the page loads the pack (branch `feat/h2-pack-load`);
- H4, the dispatcher runs the tool on the owner's files (portraits first). Its pack is built (generated: portraits 202/206, bitmaps 2/2, terrain 49/271, multi-record objects 26/278, objects 234/759, actors 0/675 by the motion gate); its frames showed seams where generated and scaled terrain meet, so H5 adds the neighbour gate before 07 closes.

## Order, and why it differs from the user's numbering

The work runs one item at a time, as the user asked. The order follows dependencies:

1. **06 speed.** Its frame contract (one RGBA conversion per rAF, only when the frame changed) must exist before an HD frame does. It also fixes the vblank bug that every later receipt would otherwise inherit. It is the item the user feels most ("지금 너무 느린데").
2. **07 HD.** Its scene tap and S× canvas carry 08's 2× Hangul font and give a larger, clearer text window.
3. **08 Korean.** Its text path and extractor feed 09's speech tap (Korean lines) and 09's NPC catalog.
4. **09 voice.** Its Korean speech needs 08, and its bundles need the conversation disassembler that 08 builds.

## Work units (one agent each; branches `feat/<unit>`, worktrees `.worktrees/feat-<unit>`)

| Unit | Spec | Scope | Needs |
|---|---|---|---|
| H2 | 07 | the page loads a pack through `u6-hdpack` (from H3): drop classification before `importFiles`, IndexedDB/forget, limits | H1, H3 |
| H4 | 07 | ops: run `tools/hd` on the owner's files with imagegen (portraits → bitmaps → terrain → actors), load the pack in the owner's browser, read the frames | H3 |
| H5 | 07 | `tools/hd` neighbour-seam gate (spec 07 rev 6): pairs from the owner's map chunks and starting object lists, the cross-edge lattice step against the classic pair's, fallback to a fixed point, the count in the coverage report; then re-ingest locally (no new upload), rebuild the pack, read the frames | H3, H4 |
| L1 | 08 | text path: `i32` chars, `u16` line and narrowing sites, UTF-8 assembler (invalid bytes pass), Hangul table, runic guard, cell padding, josa, `%1$s`, empty articles; the `u` program's text path by code point (1b) | — |
| L2 | 08 | locale layer + run, table-string and outermost-print hooks + pack parser; identity-pack receipt with `wide` on | L1, L3 |
| L3 | 08 | conversation disassembler (04 W3a's table) + run and table-string extractor; the catalog shared with 09 | — |
| L4 | 08 | page: language gate, en/ko UI catalog, pack drop/forget, hidden-input IME focused by the line bit; engine `InputEvent::Text` into `read_line`; aliases on a matching copy | L2 |
| L5 | 08 | `tools/i18n` glossary, speakers, translate, review, pack, coverage receipt, upload boundary; style guide | L3 |
| L6 | 08 | ops: run L5 on the owner's files, owner sample, full run, load, read frames | L4, L5 |
| V1 | 09 | `u6-core::speech` tap: kind from the call sites, flag around the three speech emitters in the Messages window, speaker after `0xbf`, flushes, capped queue, enable flag, `take_speech` | L1 |
| V2 | 09 | `tools/speech/fetch-models.sh`, parts + manifest + LICENSE/NOTICE; preview deploy | — |
| V3 | 09 | `site/voice/` worker and player, opt-in, chip, terms notice, forget buttons, mute both gains, no speech above 4× | V1, V2 |
| V4 | 09 | profiles: mixer, type-class table (no LLM) | V3 |
| V6 | 09 | personality profiles from bundles, local voice pack tool, coverage receipt, upload boundary | V4, V5 |
| V5 | 09 | NPC bundle exporter (wasm) keyed by (archive, item) on 08's run catalog, `npc-bundle:` keys, download | L3 |

Each unit is reviewed by gpt-6-astra and opus before it merges. Each item ends with a merge, a preview deploy with a live check and a frame read, then production.

## Carried from browser-port (still open there)

These continue in `browser-port/loop.md` and do not block this loop:
- the round-6 units intake-budget, zip-slice and nostep;
- the P3 trinity, waiting on the grok slot.
