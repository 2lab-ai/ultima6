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

Shipped: 06 is complete in production (9f831837 at main bf93585). 07 H0 (source plane), H0b (plane hardening), H1 (the HD compositor, MMPX at 2× or 4×, on by default), H3 (the pack format and the imagegen tool) and H2 (the page loads a pack) are in production (19effa16 at main a769a5a); H4 ran the tool on the owner's files (its pack stays local); H5 (the neighbour-seam gate, tool only, no deploy) is merged at 953e744. 08 L1 (the wide text path) is merged at 16be889 and on preview. In flight:
- H6, the calibration tool with its final-read mode: review round 8 fixes in progress. Lands squashed (one early commit fails its own test).
- H7, the pair model's gaps (b) and (c) and the terrain constant: the pair model is now a search over configurations; review round 8 fixes in progress. After H6 lands, it merges main, rebuilds the pack locally and re-gates.
- L1's production deploy: the build at 16be889 passed its preview check; production waits on the owner's deploy run (this session's classifier refuses production deploys).
- L3 (08 disassembler): review round 6 in progress. Next it merges main and widens its uses of the text path to `i32`. Lands squashed.

Decision 2026-10-18 (trinity, converged in round 3; record `~/Games/ultima6/work/dispatch/trinity-h5-decision.md`):
- H5 merged as gate code (953e744); item 1 stays open.
- Before any limit change, H6 reads the measure against the pixels on H4's pack, at S 4, outdoor pairs. Buckets: ≤1.1, 1.1–1.5, >1.5 × the limit, gen-gen and gen-MMPX separately. Controls: the carpet seam as positive, passing pairs as negative.
- Reading:
  - if ≤1.1 looks clean and >1.5 looks seamed, the ruler stands; a calibrated limit or C (edge-harmonise in ingest, no upload) follows;
  - if ~1.5× pairs look clean, the denominator's sampling (HD interior read only on pitch-k lattice lines) is fixed first, with a null-case fixture.
- H7 closes the pair model's two gaps before item 1 closes: (b) flags3 0x20 objects that replace the base tile, with the ghost base-tile pairs removed; (c) generated objects at least half opaque toward terrain, counted first.
- Then re-gate, read the final pack at S 4 and S 2, and write spec 07 rev 7. No arbitrary loosening (option B). D, terrain re-prompted with neighbour context, comes last if at all.

Decision 2026-10-18, after H6's read (trinity, unanimous in round 5; record `~/Games/ultima6/work/dispatch/trinity-h6-decision.md`). H6 read the sheets of H4's pre-gate pack:
- Terrain: almost all water. The fail side is verified, and the generated tiles are a different style from MMPX.
- Objects: some passing pairs are visibly defective at or beside the join, and every failing pair read is visibly defective in the window. The defects are restyle, content changes, and multi-cell mis-registration.

Outcome:
- The v1 pack generates no terrain. This is a policy constant, not a family rule.
- H7(b)(c) are unconditional. Count first. The first-pass object-terrain pairs are recorded, not gated.
- Re-gate.
- The acceptance is a window read of the final pack at S 4 and S 2:
  - passed pairs included;
  - each window compared against its classic and MMPX panels;
  - the fixed-point record matched to the final slices;
  - explicit inclusion of outdoor, town and castle windows, gen-gen and gen-MMPX, object-terrain, multi-record walls and carpets, and the defective families the object read named;
  - a type with no survivor recorded as 0.
- A new defect sends the whole object to MMPX, then re-gate and re-read. Then rev 7.
- Not done: C, any change to the limit or the denominator, a class rule, a pre-fallback from the pre-gate pack.
- D (an MMPX-compatible crisp style) comes after items 2 and 3.

Decision 2026-10-17: items still close one at a time and in order (07 closes before 08 is reported done), but 08's first units (L1, L3) start while 07's last work is tool-only (H5, hd-stderr-flake in `tools/hd`), since their files are disjoint and waiting idles the machine.

## Re-plan (2026-10-19, owner correction + trinity round 4, unanimous)

Nothing the owner could see changed after 2026-10-17 21:15 while H6, H7 and L3 ran review rounds. The plan from here:

1. **First-run chooser (done, on preview `first-run`).** Language English | 한글 for the page chrome, and HD: MMPX or a pack the site lists. Packs are hosted under `/packs/`, gitignored, and served only with the owner's key (a root Pages Function gates every spelling of the path). Owner instruction (verbatim): "hd pack은 드랍하게 하지말고 올려서 내가 고를수 있게 해줘 처음 실행활시에".
2. **Voice, after the owner has used that preview.** Spec 09 steps 1–3 only: the speech tap (V1), the Supertonic-3 weights with the licence click-through before the first download (V2), one English default voice (V3), so a real NPC line plays. If the tap is not standing within 2 review rounds, ship one fixed Speak sentence instead.
3. The next unit is picked from the owner's answers to 1 and 2.

Parked, no new commits or reviews: H6 at 4443e4b, H7 at c74bccd (local only), L3 at a253067 (unparked when L5 starts). The L1 production deploy line issued 2026-10-19 07:50 is withdrawn: it changes no English pixel.

Process: one code unit in flight; review at most 2 rounds (round 2 checks round 1's MUST fixes only); NICE findings neither block nor get fixed; reviews look only at user-visible behaviour, rights, security and data damage; a unit's exit names a URL, what the owner does, and the pixel or sound that changes. Commit subjects: English, imperative, ≤72 characters, no round or reviewer ids; one squash per unit on main.

## Order, and why it differs from the user's numbering

The work runs one item at a time, as the user asked. The order follows dependencies:

1. **06 speed.** Its frame contract (one RGBA conversion per rAF, only when the frame changed) must exist before an HD frame does. It also fixes the vblank bug that every later receipt would otherwise inherit. It is the item the user feels most ("지금 너무 느린데").
2. **07 HD.** Its scene tap and S× canvas carry 08's 2× Hangul font and give a larger, clearer text window.
3. **08 Korean.** Its text path and extractor feed 09's speech tap (Korean lines) and 09's NPC catalog.
4. **09 voice.** Its Korean speech needs 08, and its bundles need the conversation disassembler that 08 builds.

## Work units (one agent each; branches `feat/<unit>`, worktrees `.worktrees/feat-<unit>`)

| Unit | Spec | Scope | Needs |
|---|---|---|---|
| H6 | 07 | `u6-hd calibrate`: sample sheets of neighbour pairs by ratio bucket and side kind (classic / MMPX / H4 HD panels via `Compositor::compose`, ×4 and S 2), `samples.tsv`, a carpet control (read 2026-10-18: map and objects); then the final-read mode: the fixed-point pass against its final slices, one window per surviving generated item, explicit type and family inclusion, every pair label | H5 |
| H7 | 07 | pair model gaps (b) flags3 0x20 objects over terrain, ghost base-tile pairs removed, and (c) generated objects ≥½ opaque toward terrain, counted first; the terrain constant; re-gate; final window read at S 4 and S 2 (H6's final-read mode), whole-object fallbacks to a fixed point; the run-time draw variants (one object type's other frame, the doorways' side tiles) paired; spec 07 rev 7 | H5, H6 |
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

Review and landing follow the re-plan above (at most 2 rounds, one squash per unit). Each unit ends with a preview deploy, a live check and a frame read; production follows once the owner keeps the preview.

## Carried from browser-port (still open there)

These continue in `browser-port/loop.md` and do not block this loop:
- the round-6 units intake-budget, zip-slice and nostep;
- the P3 trinity, waiting on the grok slot.
