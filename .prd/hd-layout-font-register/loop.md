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
