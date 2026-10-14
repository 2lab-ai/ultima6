//! Calls into other modules' original functions that are not ported yet (spec: the narrowest
//! signature the ui needs, a `todo_stub!` body, an explicit fallback). The dispatcher replaces
//! each body with a call to the owner's port at merge.

use u6_render::Framebuffer;

use crate::objects::ObjectId;
use crate::state::State;
use crate::todo_stub;

/// 0x122144 (actors: maximum hit points). Contract: actor slot → 1..=255. Fallback: the leaf's
/// formula as read from its code (level byte at 0x139888 × 30, at least 1, at most 255), so the
/// panels show real numbers until actors/ ports it. Every stats-panel draw reaches it.
pub(crate) fn max_hp(s: &State, actor: u8) -> u8 {
    let level = s.actors.attrs.level[usize::from(actor)];
    todo_stub!(
        "0x122144 maximum hit points",
        (u16::from(level) * 30).clamp(1, 255) as u8
    )
}

/// 0x122176 (actors: maximum magic points). Contract: actor slot → points. Fallback: the
/// leaf's formula (by object type: 0x19a → 2 × INT, 0x17a → INT, 0x179 / 0x182 → INT / 2,
/// others 0).
pub(crate) fn max_magic(s: &State, actor: u8) -> u16 {
    let int = u16::from(s.actors.attrs.intelligence[usize::from(actor)]);
    let fallback = match s.objects.obj_type(ObjectId(u16::from(actor))) {
        0x19a => int * 2,
        0x17a => int,
        0x179 | 0x182 => int / 2,
        _ => 0,
    };
    todo_stub!("0x122176 maximum magic points", fallback)
}

/// 0x1220f0 (actors: level). Contract: actor slot → level. Fallback: the level byte at
/// 0x139888 the leaf returns.
pub(crate) fn level(s: &State, actor: u8) -> u8 {
    let level = s.actors.attrs.level[usize::from(actor)];
    todo_stub!("0x1220f0 actor level", level)
}

/// 0x10a4d6 (objects: frame offset of an object type facing `dir`). Contract: (type, dir) →
/// frame added to the type's base tile. Fallback 0 (the first frame); the roster shows each
/// member's base tile.
pub(crate) fn facing_frame(obj_type: u16, dir: i16) -> i16 {
    let _ = (obj_type, dir);
    todo_stub!("0x10a4d6 frame of a type facing a direction", 0)
}

/// 0x133d3e (converse: load and draw an actor's portrait at pixel (x, y)). Contract: draws a
/// 56 × 64 portrait; no result. Fallback: nothing drawn (the stats panel shows no portrait).
pub(crate) fn draw_portrait(fb: &mut Framebuffer, actor: u8, x: i16, y: i16) {
    let _ = (fb, actor, x, y);
    todo_stub!("0x133d3e draw an actor portrait", ())
}

/// 0x153002 (gfx: draw tile `tile` at pixel (x, y) on the screen the text layer draws on).
/// Contract: masked 16 × 16 tile blit, no result. Fallback: nothing drawn.
pub(crate) fn blit_tile(fb: &mut Framebuffer, tile: u16, x: i16, y: i16) {
    let _ = (fb, tile, x, y);
    todo_stub!("0x153002 tile blit", ())
}

/// 0x152ffa (gfx: the same tile blit into the other screen buffer, 0x161be2). Fallback: nothing
/// drawn.
pub(crate) fn blit_tile_back(fb: &mut Framebuffer, tile: u16, x: i16, y: i16) {
    let _ = (fb, tile, x, y);
    todo_stub!("0x152ffa tile blit to the back screen", ())
}

/// 0x151fe6 (gfx: copy the inclusive pixel rectangle (x0, y0)..(x1, y1) of the other screen
/// buffer to (dx, dy) on the screen, whole bytes of 8 pixels, all five planes; stack words in
/// that order, read from its code). No result. Fallback: nothing copied.
pub(crate) fn copy_rect(
    fb: &mut Framebuffer,
    x0: i16,
    y0: i16,
    x1: i16,
    y1: i16,
    dx: i16,
    dy: i16,
) {
    let _ = (fb, x0, y0, x1, y1, dx, dy);
    todo_stub!("0x151fe6 copy a screen rectangle", ())
}

/// 0x102ef0 (gfx/main: rebuild the view if dirty and redraw it). Contract: no result.
/// Fallback: nothing redrawn (the idle cursor still blinks).
pub(crate) fn refresh_view(s: &mut State) {
    let _ = s;
    todo_stub!("0x102ef0 post-step view refresh", ())
}

/// 0x102c90 (gfx: present the map window; `flag` as passed). Fallback: nothing presented.
pub(crate) fn present_map(s: &mut State, flag: i16) {
    let _ = (s, flag);
    todo_stub!("0x102c90 present the map window", ())
}

/// 0x124e0e (combat: distance between two window cells; the cross-hair passes (5, 5) and the
/// cell). Contract: → distance in cells. Fallback [추정]: the larger of |dx| and |dy|.
pub(crate) fn distance(x0: i16, y0: i16, x1: i16, y1: i16) -> i16 {
    let d = (x1 - x0).abs().max((y1 - y0).abs());
    todo_stub!("0x124e0e distance between cells", d)
}

/// 0x1225c0 (actors: the actor at a map location, starting from `actor`'s level). Contract:
/// (actor, x, y) → object slot, or -1. Fallback -1 (nobody there).
pub(crate) fn actor_at(s: &State, actor: u8, x: i16, y: i16) -> i16 {
    let _ = (s, actor, x, y);
    todo_stub!("0x1225c0 actor at a location", -1)
}

/// Map-window caches A4-0x15f0 (byte per window cell, 11 × 11: something stands there) and
/// A4-0x1476 (word per cell: tile shown), filled with the map-window tile cache 0x108f90
/// [likely] (map / gfx). Contract: cell → (occupied, tile). Fallback: empty cell showing tile
/// 0xff (nothing visible).
pub(crate) fn window_cell(s: &State, col: i16, row: i16) -> (bool, u16) {
    let _ = (s, col, row);
    todo_stub!(
        "0x15a27a map-window cell caches A4-0x15f0 / A4-0x1476",
        (false, 0xff)
    )
}

/// 0x135120 (boot: step the tile animation frame unless A4-0x7b64 is set). Fallback: no step.
pub(crate) fn anim_step(s: &mut State) {
    let _ = s;
    todo_stub!("0x135120 step the tile animation", ())
}

/// 0x103632 (main: called with 1 at the end of the sky bar; it draws a random number with
/// 0x151f06). Contract: no result. Fallback: nothing, so no RNG draw happens there.
pub(crate) fn sky_bar_tail(s: &mut State) {
    let _ = s;
    todo_stub!("0x103632 sky bar tail (random)", ())
}
