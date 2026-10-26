--
---
title: ultima6 — 18 remembered sight (grey memory of cells out of sight)
type: spec
tags: [ultima6, render, hd]
---

# 18 — Remembered sight

Status: planned (2026-10-27). Item 3 of `objects-menu-fog-motion/ssot.md`; findings from round 1 unit S1.

## Measured facts (main 88eb5b0)

- The sight flood (0x109c12, `map/view.rs`) runs in mode Sight from the leader (`map/view.rs:389`) and adds light; mode Sight also sets `CELL_VISIBLE` on each cell it reaches (`:309-311`). Light sources add light in a second flood when the ambient light is below full daylight (`:390-391`).
- A cell the sight flood never reached draws `NOT_VISIBLE`; a cell reached but with no light draws `DARK` (`map/view.rs:555-565`). Both read as black on screen.
- The clock raises the ambient light to at least 4 while a light burns (`actors/clock.rs:425-427`). So with a light source the view has no `DARK` cells; at night without one most cells are `DARK` (S1: 37 of 121 lit in Original).
- The HD compositor receives a frame with a per-pixel source plane, not a tile list (`crates/u6-core/src/hd.rs:160`).
