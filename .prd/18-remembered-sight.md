--
---
title: ultima6 — 18 remembered sight (grey memory of cells out of sight)
type: spec
tags: [ultima6, render, hd]
---

# 18 — Remembered sight

Status: planned (2026-10-27). Item 3 of `objects-menu-fog-motion/ssot.md`; findings from round 1 unit S1.

## Measured facts (main 88eb5b0)

- `map/view.rs` ports the sight flood 0x109c12 for its modes 0 (light) and 1 (sight); bit 7 of `s.map.view_flags` marks a cell seen (`combat/seams.rs:135-137`). A cell without it is drawn black.
