--
---
title: ultima6 — 17 context menu on a right press
type: spec
tags: [ultima6, ui, input]
---

# 17 — Context menu

Status: planned (2026-10-27). Item 2 of `objects-menu-fog-motion/ssot.md`; findings from round 1 unit M1.

## Measured facts (main 88eb5b0)

- The page sends the right button to the engine (`site/app.js:3870`, `buttons & 3`) and suppresses the browser menu on the canvas (`:3896-3898`).
- In the original a right press on the map with a command underlined repeats that command on the cell (0xa9); with none it walks like the left button (`crates/u6-core/src/ui/click.rs:116-148`). On the command bar a right press only underlines (`:167-179`); on the panels it repeats the underlined command on the object under the pointer (`:181-197`).
- 0xa9 (underline, then a target from the pointer) is a path that already exists for running one command on one cell.
