---
title: ultima6 hd-layout-font-register — loop
type: loop
tags: [ultima6]
---

# Loop: hd-layout-font-register

Status: in progress (2026-10-22). One deploy per item, then one per follow-up.

## Round 1 (2026-10-22)

| Unit | Spec | Branch / worktree | Scope | Needs |
|---|---|---|---|---|
| L1 | 10 | `feat/hd-layout` → `.worktrees/feat-hd-layout` | page: 1280×720 layout of the ×4 frame, letterbox for `u`, inverse pointer, fullscreen stretch, layout setting, harness, web tests | — |
| R1 | 12 | `feat/ko-register` → `.worktrees/feat-ko-register` | `tools/i18n`: speaker file (age, class, note), `--speakers`, style guide by age; then the owner's re-translation run and a new pack name | — |
| F1 | 11 | `feat/hd-font` → `.worktrees/feat-hd-font` | engine + compositor: glyph source kind, ink flag, glyph cell export; then the page's HD font and the chooser | L1 merged (both touch `site/app.js`) |

Gate (per unit): `~/Games/ultima6/work/dispatch/merge-gate.sh <worktree> <logdir>`. Live check: a fresh visitor on the deployed site, the zip only.

## Deploys

| # | Date | Item | What went live | Main | Receipt |
|---|---|---|---|---|---|
| 1 | 2026-10-23 01:45 KST | 3 (R1) | Korean pack `ko-1023a.u6lp`: the 41 child, youth and elder scripts re-translated by age (child 5/5, youth 15/15, elder 21/21; 3 single lines kept from the earlier pack after upstream 502s); narration and other voices in a script keep their own level | `41ff1c8` | fresh visitor, zip only, 한글: `/packs/ko-1023a.u6lp` 200, `data-locale-pack` 8235,200; Talk to Iolo shows the new look line |

Findings during R1, fixed before deploy 1: the first run gave a child script's narration and a companion's interjection the child's 해요체 (guide fixed: `3663b6a`, `41ff1c8`; spec 12 Design 3 amended `70251a1`). Not fixed by this item (08's shared keyword words): a few `@{…}` words are another script's keyword (e.g. a pie shown as the word for magic), unchanged from the earlier pack.
