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

Spec review: every spec had a gpt-6-astra review and an opus review read against the code, all folded in. 06 is at rev 3, 07 at rev 4, 08 at rev 3, 09 at rev 3. Closed units are in `loop.done.md`.

Shipped: 06's speed control is live in production (Cloudflare Pages 269ed8c1 at main 810df38). 06 is complete in production (9f831837 at main bf93585). Next: 07 HD, unit H0.

## Order, and why it differs from the user's numbering

The work runs one item at a time, as the user asked. The order follows dependencies:

1. **06 speed.** Its frame contract (one RGBA conversion per rAF, only when the frame changed) must exist before an HD frame does. It also fixes the vblank bug that every later receipt would otherwise inherit. It is the item the user feels most ("지금 너무 느린데").
2. **07 HD.** Its scene tap and S× canvas carry 08's 2× Hangul font and give a larger, clearer text window.
3. **08 Korean.** Its text path and extractor feed 09's speech tap (Korean lines) and 09's NPC catalog.
4. **09 voice.** Its Korean speech needs 08, and its bundles need the conversation disassembler that 08 builds.

## Work units (one agent each; branches `feat/<unit>`, worktrees `.worktrees/feat-<unit>`)

| Unit | Spec | Scope | Needs |
|---|---|---|---|
| H0 | 07 | `Framebuffer` source plane (`Src` with image version, `set_src`, `copy_px`), set by `gfx::blit`/`blit_bitmap`, copied by `copy_pixel`/`copy_rect`/dissolve; record versions bumped by the shore merge; enabled by u6-web; source receipt | T4 |
| H1 | 07 | `u6-render` compositor + MMPX×S (cached by content hash) in wasm; `u6-web` HD exports; page S× canvas, HD toggle, logical pointer mapping | H0 |
| H2 | 07 | HD pack format, drop classification before `importFiles`, IndexedDB/forget, limits | H1 |
| H3 | 07 | `tools/hd` inventory/atlas/run/ingest/pack with the gate; synthetic tests | H2 |
| H4 | 07 | ops: run `tools/hd` on the owner's files with imagegen (portraits → bitmaps → terrain → actors), load the pack in the owner's browser, read the frames | H3 |
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
