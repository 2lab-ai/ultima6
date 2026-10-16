//! The fixed prompt of each sheet kind (ids in `jobs.jsonl`: `tile/1`, `terrain/1`,
//! `portrait/1`, `bitmap/1`). Only the sheet's geometry and key colour are filled in.

use crate::atlas::{
    BORDER, CELL, GRID, Job, PORTRAIT, PORTRAIT_GRID, SHEET_H, SHEET_W, SheetKind, TERRAIN_CELL,
    TERRAIN_GRID,
};

const HEAD: &str = "Use the built-in image_gen tool to EDIT the attached image (Image 1 = edit \
target). Do not run shell commands and do not write files; only generate the image.\n\
Use case: precise-object-edit.";

fn key_rules(key: &str) -> String {
    format!(
        "Every flat {key} area is empty background and must stay perfectly flat {key}: no \
         shadow, glow, gradient, texture or halo on it, and {key} is not used anywhere else."
    )
}

/// The prompt for `job`.
pub fn text(job: &Job) -> String {
    let key = format!("#{:02x}{:02x}{:02x}", job.key[0], job.key[1], job.key[2]);
    let keep = key_rules(&key);
    let size = format!("Output size {SHEET_W}x{SHEET_H}.");
    let body = match job.kind {
        SheetKind::Tile => format!(
            "Asset type: HD tile sheet for a 2D top-down game.\n\
             Image 1 is a pixel-art sheet: a grid of {c} columns x {r} rows of square cells, \
             each cell {CELL}x{CELL} px, edge to edge with no gaps. Every cell is one 16x16-pixel \
             tile shown at 8x nearest-neighbour scale. An object drawn across two or four \
             adjacent cells is one object.\n\
             Task: repaint every cell as a clean high-resolution version of the SAME tile: same \
             composition, same shapes and silhouette, same hues, same position inside the cell. \
             Replace the blocky pixel edges with smooth detailed edges and add fine texture and \
             soft shading; do not add, remove or move anything. Cells that are frames of one \
             character keep the same frame layout and the same differences between frames \
             (pose, legs, arms).\n\
             Constraints: the grid stays exactly where it is (cell boundaries at multiples of \
             {CELL} px); no borders, gutters, labels, text or watermark; nothing crosses a cell \
             boundary unless it does in Image 1. {keep}",
            c = GRID.0,
            r = GRID.1,
        ),
        SheetKind::Terrain => {
            let used = TERRAIN_GRID.1 * TERRAIN_CELL;
            format!(
                "Asset type: seamless HD terrain tile sheet for a 2D top-down game.\n\
                 Image 1 is a pixel-art sheet: {c} columns x {r} rows of square cells, each \
                 {TERRAIN_CELL}x{TERRAIN_CELL} px, edge to edge with no gaps; below {used} px \
                 the sheet is empty. Each cell shows one tileable 16x16-pixel texture at 8x \
                 nearest-neighbour scale, surrounded by a {BORDER}-pixel ({b} px) wrap-around \
                 border that repeats the opposite side of the same texture: the central \
                 {CELL}x{CELL} area of every cell is the real tile.\n\
                 Task: repaint every cell as a clean high-resolution version of the SAME \
                 texture: same composition, same hues, same position. Replace blocky pixel \
                 edges with smooth detail, add fine texture and soft shading. The texture must \
                 continue seamlessly through the border, so that the central tile repeats \
                 without visible seams.\n\
                 Constraints: the grid stays exactly where it is (cell boundaries at multiples \
                 of {TERRAIN_CELL} px); no borders, gutters, labels, text or watermark. {keep}",
                c = TERRAIN_GRID.0,
                r = TERRAIN_GRID.1,
                b = BORDER * 8,
            )
        }
        SheetKind::Portrait => format!(
            "Asset type: HD character portraits for a game's conversation window.\n\
             Image 1 holds {n} pixel-art portraits, each {w}x{h} px (a 56x64-pixel portrait \
             at 8x nearest-neighbour scale), in a grid of {c} columns x {r} rows from the \
             top-left corner, edge to edge.\n\
             Task: repaint every portrait as a clean high-resolution painted portrait of the \
             SAME character: same face, hair, clothing, pose, framing, hues and position inside \
             its {w}x{h} rectangle. Replace the blocky pixels with smooth detailed painting; \
             do not add, remove or move anything.\n\
             Constraints: each portrait stays inside its rectangle; no borders, gutters, labels, \
             text or watermark. {keep}",
            n = job.places.len(),
            w = PORTRAIT.0,
            h = PORTRAIT.1,
            c = PORTRAIT_GRID.0,
            r = PORTRAIT_GRID.1,
        ),
        SheetKind::Bitmap => {
            let p = &job.places[0];
            format!(
                "Asset type: HD panel picture for a game's screen.\n\
                 Image 1 holds one pixel-art picture of {w}x{h} px ({cw}x{ch} pixels at {k}x \
                 nearest-neighbour scale) at the top-left corner; the rest is background.\n\
                 Task: repaint the picture as a clean high-resolution version of the SAME \
                 picture: same layout, shapes, hues, position and size. Replace the blocky \
                 pixels with smooth detail; do not add, remove or move anything.\n\
                 Constraints: the picture stays in its rectangle; no borders, labels, text or \
                 watermark beyond what it shows. {keep}",
                k = p.scale,
                w = p.w * p.scale,
                h = p.h * p.scale,
                cw = p.w,
                ch = p.h,
            )
        }
    };
    format!("{HEAD}\n{body}\n{size}\n")
}
