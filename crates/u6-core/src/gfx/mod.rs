//! World-view drawing: tile blit (chunky port of the blitter 0x153002 / 0x152ffa with the same
//! clipping and masking), map window draw 0x10202e / 0x102c90, view refresh 0x102ef0 (WU-P1).
//!
//! Boundary: `u6-render` holds the platform-free framebuffer and blit primitives; this module
//! orchestrates the world view with them. Tile pixels come from `u6_formats::gfx` (decoder for
//! maptiles / objtiles / tileindx / masktype / animmask and the palette). The copper/bitplane
//! display (hunk 34) is replaced by the engine's two chunky buffers ([`crate::Io::draw`],
//! [`crate::Io::present`]).
//!
//! Original functions this module will own (milestone 1):
//!
//! - 0x10202e [map] render map window: tiles then objects [likely]
//! - 0x102c90 [map] present/refresh map window [likely]
//! - 0x102ef0 [main] post-step refresh: rebuild view if dirty, redraw
//! - 0x14f16c [graphics] load objtiles.ega
//! - 0x151f5a [graphics] leaf; callers 3, from 3 subsystems
//! - 0x151fe6 [graphics] unnamed; 1 callees
//! - 0x15206c [graphics] leaf; callers 1, from 1 subsystems
//! - 0x1520e2 [graphics] step of 0x102c90 (present/refresh map window [likel~)
//! - 0x152176 [graphics] leaf; callers 16, from 6 subsystems
//! - 0x152188 [graphics] leaf; callers 4, from 3 subsystems
//! - 0x1521f4 [graphics] leaf; callers 12, from 4 subsystems
//! - 0x15265a [graphics] unnamed; calls 0x1623d6 (load objtiles.ega [likely])
//! - 0x152800 [graphics] leaf; callers 2, from 1 subsystems
//! - 0x152876 [graphics] leaf; callers 2, from 1 subsystems
//! - 0x152cd2 [graphics] unnamed; calls 0x1623d6 (load objtiles.ega [likely])
//! - 0x152d30 [graphics] leaf; callers 1, from 1 subsystems
//! - 0x152d52 [graphics] unnamed; 1 callees
//! - 0x152dac [graphics] step of 0x10202e (render map window: tiles then obj~)
//! - 0x152dc8 [graphics] leaf; callers 1, from 1 subsystems
//! - 0x152dca [graphics] leaf; callers 1, from 1 subsystems
//! - 0x152dd8 [graphics] leaf; callers 1, from 1 subsystems
//! - 0x152e50 [graphics] load tile graphics (maptiles/tileindx/masktype/animmask)
//! - 0x152ffa [graphics] tile blitter, entry A (shares body with 0x153002) [likely]
//! - 0x153002 [graphics] tile blitter, entry B [likely]
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
//! - 0x1623d6 [graphics] load objtiles.ega [likely]
