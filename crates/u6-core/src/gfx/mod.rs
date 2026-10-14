//! World-view drawing: tile blit (chunky port of the blitter 0x153002 / 0x152ffa with the same
//! row ranges, masking, outline mode and overlay), map window draw 0x102c90, view refresh 0x102ef0
//! and the per-frame tile animation 0x10202e (WU-P1).
//!
//! Boundary: `u6-render` holds the platform-free framebuffer; this module draws the world view
//! into it. Tile pixels come from `u6_formats::gfx` (decoder for maptiles / objtiles / tileindx /
//! masktype / animmask); the palette is `State::tables.palette`. The copper/bitplane display
//! (hunk 34) is replaced by the engine's two chunky buffers ([`crate::Io::draw`],
//! [`crate::Io::present`]). A chunky pixel index is the 5-bit colour the 5 planes held, and the
//! byte address `y·40 + x/8` of a plane becomes the linear pixel index `(y·40 + x/8)·8 + bit`, so
//! the planar arithmetic (8-pixel steps, row wrap) carries over exactly ([`blit`]).
//!
//! Domain state = [`GfxState`]: the tile records, the live masktype / record tables that the
//! animation rewrites, animmask, the tables read out of `game`, and the display globals the draw
//! path reads. The window contents (tile cache A4-0x1476 and the per-cell object lists) are built
//! by map (0x109e74) and objects (0x10e81e); gfx reads them through [`seams::view_window`].
//!
//! Roles corrected from the inventory by reading the code: 0x10202e is the animation tick, not
//! the map render (counter A4-0x7b00, animdata substitution, shore merge, actor idle frames);
//! 0x102c90 is the map window render + present; 0x152dac merges the animated water into the 32
//! shore tiles using the masks in animmask block 2.
//!
//! Original functions this module will own (milestone 1):
//!
//! - 0x10202e [map] animation tick: animdata frames, shore merge, actor idle frames
//! - 0x102bf2 [main] clock hands (a step of 0x102c90; main_loop's listing names it too)
//! - 0x102c90 [map] render the 11×11 map window, border, present
//! - 0x102ef0 [main] post-step refresh: rebuild view if dirty, redraw
//! - 0x14f16c [graphics] object-tile cache prefill (subsumed: all records are decoded at load)
//! - 0x151f5a [graphics] copy the map window rect back → front
//! - 0x151fe6 [graphics] unnamed; 1 callees
//! - 0x15206c [graphics] copy one pixel back → front (step of 0x1520e2)
//! - 0x1520e2 [graphics] dissolve a rect back → front in LFSR order
//! - 0x152176 [graphics] set the plot colour
//! - 0x152188 [graphics] plot one pixel (set or XOR)
//! - 0x1521f4 [graphics] leaf; callers 12, from 4 subsystems
//! - 0x15265a [graphics] rotated object tile (thrown objects)
//! - 0x152800 [graphics] leaf; callers 2, from 1 subsystems
//! - 0x152876 [graphics] leaf; callers 2, from 1 subsystems
//! - 0x152cd2 [graphics] tile substitution (animation frame → tile)
//! - 0x152d30 [graphics] masked row merge (step of 0x152d52)
//! - 0x152d52 [graphics] shore merge of one plane (step of 0x152dac)
//! - 0x152dac [graphics] shore merge: animated water into tiles 16..47
//! - 0x152dc8 [graphics] empty
//! - 0x152dca [graphics] set the overlay frame (& 7)
//! - 0x152dd8 [graphics] empty
//! - 0x152e50 [graphics] load tile graphics (maptiles/tileindx/masktype/animmask)
//! - 0x152ffa [graphics] tile blitter, entry A (back buffer)
//! - 0x153002 [graphics] tile blitter, entry B (front buffer)
//! - 0x153384 [graphics] leaf; callers 2, from 2 subsystems
//! - 0x1533e0 [graphics] step of 0x133d3e (load an NPC portrait (portrait.a))
//! - 0x153550 [graphics] leaf; callers 1, from 1 subsystems
//! - 0x15355a [graphics] unnamed; 1 callees
//! - 0x161ec8 [graphics] unnamed; calls 0x134ac4 (read N bytes from the current~)
//! - 0x1621c0 [graphics] step of 0x1346a0 (seek in the current file)
//! - 0x162228 [graphics] step of 0x1351d8 (main(): setup, load data, charact~)
//! - 0x162238 [graphics] leaf; callers 1, from 1 subsystems
//! - 0x16227c [graphics] leaf; callers 1, from 1 subsystems
//! - 0x1622be [graphics] leaf; callers 2, from 2 subsystems
//! - 0x162358 [graphics] step of 0x14ef06 (system setup: memory, libraries, ~)
//! - 0x16236a [graphics] install the game copper list (COP1LC/COPJMP1)
//! - 0x1623d6 [graphics] object-tile cache miss: load one record (subsumed, see 0x14f16c)

pub mod anim;
pub mod blit;
pub(crate) mod idle;
mod load;
pub mod seams;
pub mod tables;
pub mod view;

#[cfg(test)]
mod tests;

pub use load::GfxState;
pub use view::{ViewWindow, WINDOW_TILES, refresh_view, render_window};
