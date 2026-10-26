//! Spec 13: the geometry of the window, the view and the map's place in the frame, for both
//! builds. Original (the default) keeps the game's sizes; Wide (the `wide` feature, which also
//! turns on `u6-render`'s, the frame then 912 × 408, [`u6_render::WIDTH`] × [`u6_render::HEIGHT`])
//! has a 37 × 21 view in a 60 × 60 window, the map beside the UI's 320 × 200, and below the UI
//! the slots of the side panel's three views (spec 14 Design 1, [`SLOTS`]).
//!
//! The values that differ between the builds are the six of the two `mode` blocks below; every
//! other constant is derived from them. Which half a site reads (spec 13 Decisions 3–5): what
//! the player can see follows the view ([`HALF_W`], [`HALF_H`]); spell areas and targeting keep
//! the original box round the leader ([`RULE_HALF`]).
//!
//! Types: sizes and window cells `usize` (as [`crate::map::WINDOW`]), world coordinates `u16`
//! ([`LEAD`], [`window_origin`]), frame pixels `u16` ([`MAP_X`], [`SHOWN`]), and [`RULE_HALF`]
//! `i16`, the leader-relative arithmetic its sites do.

use std::ops::Range;

use u6_formats::chunks::CHUNK_SIZE;

/// The values the two builds choose; the public constants below read them.
#[cfg(not(feature = "wide"))]
mod mode {
    /// View columns × rows.
    pub const VIEW: (usize, usize) = (11, 11);
    /// Window side.
    pub const WINDOW: usize = 40;
    /// Mask of a view offset.
    pub const VIEW_OFFSET_MASK: u16 = 0x1f;
    /// Frame pixel of map cell (0, 0).
    pub const MAP: (u16, u16) = (0, 0);
    /// Sight queue entries.
    pub const QUEUE_LEN: usize = 256;
    /// No slots: the side panel shows one view.
    pub const SLOTS: &[(u16, u16)] = &[];
}

/// The values the two builds choose; the public constants below read them.
#[cfg(feature = "wide")]
mod mode {
    /// View columns × rows: 21 rows at 16:9 (spec 13 Decision 1).
    pub const VIEW: (usize, usize) = (37, 21);
    /// Window side.
    pub const WINDOW: usize = 60;
    /// Mask of a view offset.
    pub const VIEW_OFFSET_MASK: u16 = 0x3f;
    /// Frame pixel of map cell (0, 0): beside the UI's 320 columns.
    pub const MAP: (u16, u16) = (320, 0);
    /// Sight queue entries.
    pub const QUEUE_LEN: usize = 1024;
    /// The slots' frame origins: roster, stats, inventory (spec 14 Design 1).
    pub const SLOTS: &[(u16, u16)] = &[(8, 206), (160, 206), (8, 310)];
}

/// Pixels per map cell side.
const CELL: u16 = 16;
/// The UI's screen, columns × rows, in the frame's top-left corner in both builds (spec 13
/// Geometry).
const UI: (usize, usize) = (320, 200);

/// View columns. Original 11: the `moveq #0xb` loops of 0x102c90, the 11 × 11 words of the tile
/// cache A4-0x1476 that 0x109e74 writes (`gfx/view.rs:35-36`), and the draw-list pool's 121
/// cells, the 256 bytes of A4-0x1577 (`objects/draw.rs:23-24`).
pub const VIEW_W: usize = mode::VIEW.0;
/// View rows. Original 11, as [`VIEW_W`] (the view is square there, not in Wide).
pub const VIEW_H: usize = mode::VIEW.1;
/// View columns either side of the leader's. Original 5: the `- 5` of the view offset
/// (`map/region.rs:95`) and the leader's view cell (5, 5).
pub const HALF_W: usize = VIEW_W / 2;
/// View rows above and below the leader's. Original 5, as [`HALF_W`] (`map/region.rs:96`).
pub const HALF_H: usize = VIEW_H / 2;
/// Spell effect areas and targeting stay the original box of ±5 cells round the leader in both
/// builds (spec 13 Decisions 3–4): the `cx - 5..=cx + 5` of their sites.
pub const RULE_HALF: i16 = 5;
/// Side of the window and its grids. Original 40: `map::WINDOW` (`map/mod.rs:57`),
/// `objects::GRID_SIDE` (`objects/mod.rs:165`), `GRID` (`actors/work.rs:148`), `SIDE = 0x28`
/// (`combat/peer.rs:12`).
pub const WINDOW: usize = mode::WINDOW;
/// The window's lead: [`window_origin`] is `(c - LEAD) & 0x3f8`, which puts the leader in window
/// cells `LEAD..=LEAD + 7`, the chunk centred in the window. Original 16: the
/// `wrapping_sub(16)` of `map/region.rs:91-92`.
pub const LEAD: u16 = ((WINDOW - CHUNK_SIZE) / 2) as u16;
/// Mask of [`view_offset`]. Original 0x1f: `map/region.rs:95-96`.
pub const VIEW_OFFSET_MASK: u16 = mode::VIEW_OFFSET_MASK;
/// The inner sight block's window columns: the view and its one-cell ring for every leader cell,
/// chunk-aligned. Original `8..32`: the block `map/view.rs` `update_view` clears, `reveal_view`
/// lights, `mark_objects` walks and `flood_lights` floods from.
pub const BLOCK_X: Range<usize> = block(HALF_W);
/// The inner sight block's window rows, as [`BLOCK_X`]. Original `8..32`, the same sites.
pub const BLOCK_Y: Range<usize> = block(HALF_H);
/// Frame column of map cell (0, 0)'s left edge. Original 0: view cell (col, row) is drawn at
/// pixel `(col * 16, row * 16)` (`cell_pixel`, `gfx/view.rs:68-70`).
pub const MAP_X: u16 = mode::MAP.0;
/// Frame row of map cell (0, 0)'s top edge. Original 0, as [`MAP_X`].
pub const MAP_Y: u16 = mode::MAP.1;
/// The frame rect the map is shown in, `(x0, y0, x1, y1)` inclusive: the view less half a cell
/// on every side. Original (8, 8)–(0xa7, 0xa7): `gfx/seams.rs:67-72`, the `8..168` of
/// `gfx/blit.rs:305-322`.
pub const SHOWN: (u16, u16, u16, u16) = {
    let (x0, y0) = (MAP_X + CELL / 2, MAP_Y + CELL / 2);
    let (w, h) = ((VIEW_W as u16 - 1) * CELL, (VIEW_H as u16 - 1) * CELL);
    (x0, y0, x0 + w - 1, y0 + h - 1)
};
/// Entries of the sight flood's ring queue: u8 indices in Original, u16 in Wide. Original 256:
/// the x / y byte arrays A4-0x17f2 / -0x16f2 of `map/view.rs` `Queue`.
pub const QUEUE_LEN: usize = mode::QUEUE_LEN;
// A flood queues each cell at most once; what must fit is the cells queued at once. The sight
// flood's are at most the view and its ring, a light flood's the 15 × 15 cells within 7 of its
// start. The overview's flood (mode 2) reaches the whole window (1600 cells in Original), so only
// its frontier must fit: in an open window it peaks at 156 of 256 entries in Original and 236 of
// 1024 in Wide (`map/view.rs` test `the_overview_frontier_in_an_open_window_fits_the_queue`). A
// frontier past 255, which walls could shape, drops cells in Original (`Flood::enqueue`); Wide's
// queue holds one up to 1023 (a spec 13 widening, not the original's drop). The ring's index
// wraps at a power of two.
const _: () = assert!(
    QUEUE_LEN.is_power_of_two() && (VIEW_W + 2) * (VIEW_H + 2) <= QUEUE_LEN && 15 * 15 <= QUEUE_LEN,
    "the sight and light floods' cells fit the queue"
);

/// Spec 14 Design 1: the frame pixel of the top left of each slot the side panel's views are
/// copied into, by view: roster (0x91), stats (0x90), inventory (0x92). Each is [`SLOT_W`] ×
/// [`SLOT_H`], the side panel area's size. None in Original; in Wide (8, 206), (160, 206) and
/// (8, 310), below the UI and left of the map, where nothing else draws, each at a multiple of 8
/// pixels from the panel area on both axes (rev 3: the HD font draws a text cell only on the
/// 8-pixel grid, `u6_render::hd` `glyph_cell`; asserted in `ui/panels.rs`).
pub const SLOTS: &[(u16, u16)] = mode::SLOTS;
/// Width of a slot: the side panel area's 144 columns (`ui/panels.rs` `PANEL` from 0xa8).
pub const SLOT_W: u16 = 144;
/// Height of a slot: the side panel area's 98 rows (`PANEL`, rows 6..=0x67).
pub const SLOT_H: u16 = 98;
// Every slot lies in the frame, outside the UI's 320 × 200 and left of the map's columns, and
// apart from every other slot.
const _: () = assert!(
    slots_fit(),
    "the slots lie in the frame, off the UI and the map, apart"
);

/// The check of the assertion above.
const fn slots_fit() -> bool {
    let (w, h) = (SLOT_W as usize, SLOT_H as usize);
    let mut i = 0;
    while i < SLOTS.len() {
        let (x, y) = (SLOTS[i].0 as usize, SLOTS[i].1 as usize);
        let in_frame = x + w <= u6_render::WIDTH && y + h <= u6_render::HEIGHT;
        let off_ui = x >= UI.0 || y >= UI.1;
        let off_map = x + w <= MAP_X as usize;
        if !(in_frame && off_ui && off_map) {
            return false;
        }
        let mut j = i + 1;
        while j < SLOTS.len() {
            let (a, b) = (SLOTS[j].0 as usize, SLOTS[j].1 as usize);
            if x < a + w && a < x + w && y < b + h && b < y + h {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// Spec 13's inner sight block on one axis: `lo = (LEAD - half - 1) & !7`,
/// `hi = (LEAD + 7 + half + 1 + 8) & !7`.
const fn block(half: usize) -> Range<usize> {
    let lead = LEAD as usize;
    ((lead - half - 1) & !7)..((lead + 7 + half + 1 + 8) & !7)
}

/// The window origin on one axis for the view centre `c`: chunk-aligned, the centre in window
/// cells `LEAD..=LEAD + 7`. Original: `map/region.rs:91-92`.
pub const fn window_origin(c: u16) -> u16 {
    c.wrapping_sub(LEAD) & 0x3f8
}

/// The window cell of view cell 0 on one axis, for the view centre `c`, the window `origin` and
/// the view's `half` on that axis ([`HALF_W`], [`HALF_H`]). Original: `map/region.rs:95-96`.
pub const fn view_offset(c: u16, origin: u16, half: u16) -> i16 {
    (c.wrapping_sub(origin).wrapping_sub(half) & VIEW_OFFSET_MASK) as i16
}

#[cfg(test)]
mod tests {
    use super::*;
    use u6_render::{HEIGHT, WIDTH};

    /// The frame rect of the whole view, `(xs, ys)`.
    fn map_rect() -> (Range<usize>, Range<usize>) {
        let (x, y) = (usize::from(MAP_X), usize::from(MAP_Y));
        let cell = usize::from(CELL);
        (x..x + VIEW_W * cell, y..y + VIEW_H * cell)
    }

    #[cfg(not(feature = "wide"))]
    #[test]
    fn original_constants_are_todays_literals() {
        assert_eq!((VIEW_W, VIEW_H), (11, 11));
        assert_eq!(VIEW_W * VIEW_H, crate::objects::DRAW_CELLS);
        assert_eq!((HALF_W, HALF_H, RULE_HALF), (5, 5, 5));
        assert_eq!(WINDOW, crate::map::WINDOW);
        assert_eq!(WINDOW, crate::objects::GRID_SIDE);
        // `actors/work.rs:148` `GRID` and `combat/peer.rs:12` `SIDE` are private.
        assert_eq!(WINDOW, 0x28);
        assert_eq!(LEAD, 16);
        assert_eq!(VIEW_OFFSET_MASK, 0x1f);
        assert_eq!((BLOCK_X, BLOCK_Y), (8..32, 8..32));
        assert_eq!((MAP_X, MAP_Y), (0, 0));
        assert_eq!(SHOWN, (8, 8, 0xa7, 0xa7));
        assert_eq!(QUEUE_LEN, 256);
        assert_eq!((WIDTH, HEIGHT), UI);
        assert!(SLOTS.is_empty(), "no slots: spec 14 is Wide only");
    }

    /// `map/region.rs:91-96`, as written there, for every surface coordinate.
    #[cfg(not(feature = "wide"))]
    #[test]
    fn original_origin_and_offset_are_the_rebuild_formulas() {
        for x in 0..1024u16 {
            let ox = x.wrapping_sub(16) & 0x3f8;
            assert_eq!(window_origin(x), ox, "origin at {x}");
            let vx = (x.wrapping_sub(ox).wrapping_sub(5) & 0x1f) as i16;
            assert_eq!(view_offset(x, ox, HALF_W as u16), vx, "offset at {x}");
            assert_eq!(view_offset(x, ox, HALF_H as u16), vx, "offset at {x}");
        }
    }

    /// For every coordinate the leader stands in window cells `LEAD..=LEAD + 7`, the view starts
    /// [`HALF_W`] / [`HALF_H`] before it, and the view with a one-cell ring lies in the inner
    /// sight block, which lies in the window.
    #[test]
    fn the_view_and_its_ring_lie_in_the_sight_block_for_every_leader_cell() {
        let mut cells = [false; 8];
        for (half, view, block) in [(HALF_W, VIEW_W, BLOCK_X), (HALF_H, VIEW_H, BLOCK_Y)] {
            assert_eq!(2 * half + 1, view);
            assert!(
                block.start < block.end && block.end <= WINDOW,
                "{block:?} in the window"
            );
            for x in 0..1024u16 {
                let origin = window_origin(x);
                assert_eq!(usize::from(origin) % CHUNK_SIZE, 0, "chunk-aligned at {x}");
                let cell = usize::from(x.wrapping_sub(origin) & 0x3ff);
                let lead = usize::from(LEAD);
                assert!((lead..=lead + 7).contains(&cell), "leader cell at {x}");
                cells[cell - lead] = true;
                let first = view_offset(x, origin, half as u16);
                assert_eq!(
                    usize::try_from(first),
                    Ok(cell - half),
                    "view offset at {x}"
                );
                let ring = cell - half - 1..=cell + half + 1;
                assert!(
                    block.contains(ring.start()) && block.contains(ring.end()),
                    "ring {ring:?} in {block:?} at {x}"
                );
            }
        }
        assert_eq!(cells, [true; 8], "every leader cell reached");
    }

    /// The shown rect is the view less half a cell each side and lies in the frame.
    #[test]
    fn the_shown_map_lies_in_the_frame() {
        let (x0, y0, x1, y1) = SHOWN;
        let (xs, ys) = map_rect();
        assert!(xs.end <= WIDTH && ys.end <= HEIGHT, "the view in the frame");
        let [x0, y0, x1, y1] = [x0, y0, x1, y1].map(usize::from);
        assert!(
            xs.contains(&x0) && xs.contains(&x1),
            "{x0}..={x1} in {xs:?}"
        );
        assert!(
            ys.contains(&y0) && ys.contains(&y1),
            "{y0}..={y1} in {ys:?}"
        );
        let cell = usize::from(CELL);
        assert_eq!(
            (x1 + 1 - x0, y1 + 1 - y0),
            ((VIEW_W - 1) * cell, (VIEW_H - 1) * cell)
        );
        assert_eq!((x0 - xs.start, y0 - ys.start), (cell / 2, cell / 2));
    }

    /// The UI's 320 × 200 lies in the frame. In Wide the map lies beside it; in Original the map
    /// is part of it (the frame is the UI's screen, spec 13 Geometry).
    #[test]
    fn the_ui_lies_in_the_frame_and_the_map_beside_it_in_wide() {
        assert!(UI.0 <= WIDTH && UI.1 <= HEIGHT);
        let (xs, ys) = map_rect();
        if cfg!(feature = "wide") {
            assert!(
                xs.start >= UI.0 || ys.start >= UI.1,
                "{xs:?} × {ys:?} off the UI"
            );
        } else {
            assert!(
                xs.end <= UI.0 && ys.end <= UI.1,
                "{xs:?} × {ys:?} in the UI"
            );
        }
    }

    /// Spec 13's Geometry table, Wide column.
    #[cfg(feature = "wide")]
    #[test]
    fn wide_constants_are_the_spec_table() {
        assert_eq!(
            (VIEW_W, VIEW_H, HALF_W, HALF_H, RULE_HALF),
            (37, 21, 18, 10, 5)
        );
        assert_eq!((WINDOW, LEAD, VIEW_OFFSET_MASK), (60, 26, 0x3f));
        assert_eq!((BLOCK_X, BLOCK_Y), (0..56, 8..48));
        assert_eq!((MAP_X, MAP_Y), (320, 0));
        assert_eq!(SHOWN, (328, 8, 903, 327));
        assert_eq!(QUEUE_LEN, 1024);
        assert_eq!((WIDTH, HEIGHT), (912, 408), "spec 14 Design 1");
    }

    /// The view fills the frame's width beside the UI and its top 336 rows: 37 columns across,
    /// 21 rows down (below them, left of the map, the slots of spec 14). Fails with [`VIEW_W`]
    /// and [`VIEW_H`] swapped.
    #[cfg(feature = "wide")]
    #[test]
    fn wide_view_fills_the_frame_beside_the_ui_columns_across() {
        let (xs, ys) = map_rect();
        assert_eq!((xs.start, xs.end), (UI.0, WIDTH), "columns");
        assert_eq!((ys.start, ys.end), (0, 336), "rows");
    }

    /// Spec 14 Design 1's slot table: roster, stats, inventory, in that order (fails with any two
    /// origins swapped), each the side panel's 144 × 98; the inventory slot ends at the frame's
    /// bottom row, the stats slot left of the map.
    #[cfg(feature = "wide")]
    #[test]
    fn wide_slots_are_the_spec_table() {
        assert_eq!(SLOTS, [(8, 206), (160, 206), (8, 310)]);
        assert_eq!((SLOT_W, SLOT_H), (144, 98));
        let (x, y) = SLOTS[2];
        assert_eq!(
            usize::from(y + SLOT_H),
            HEIGHT,
            "the inventory slot at the bottom"
        );
        assert!(
            SLOTS[1].0 + SLOT_W <= MAP_X && x + SLOT_W <= MAP_X,
            "left of the map"
        );
        assert!(slots_fit());
    }
}
