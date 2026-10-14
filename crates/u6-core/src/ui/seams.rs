//! Calls from the ui into other modules' original functions, reconciled at integration
//! (WU-P7): each forwards to the owner's port; the ones whose target is outside milestone 1
//! keep a `todo_stub!` with the explicit fallback.

use u6_render::Framebuffer;

use crate::engine::Io;
use crate::state::State;

/// 0x122144 (actors: maximum hit points of actor slot `actor`).
pub(crate) fn max_hp(s: &State, actor: u8) -> u8 {
    crate::actors::leaves::max_hp(s, u16::from(actor)) as u8
}

/// 0x122176 (actors: maximum magic points).
pub(crate) fn max_magic(s: &State, actor: u8) -> u16 {
    crate::actors::leaves::max_magic(s, u16::from(actor))
}

/// 0x1220f0 (actors: level).
pub(crate) fn level(s: &State, actor: u8) -> u8 {
    crate::actors::leaves::level(s, u16::from(actor))
}

/// 0x10a4d6 (objects: frame offset of an object type facing `dir`).
pub(crate) fn facing_frame(obj_type: u16, dir: i16) -> i16 {
    crate::objects::actor_frame(obj_type, dir)
}

/// 0x133d3e (converse: load and draw an actor's portrait at pixel (x, y) on the screen).
pub(crate) fn draw_portrait(s: &State, io: &Io, actor: u8, x: i16, y: i16) {
    crate::converse::draw_portrait(s, io, i16::from(actor), x, y);
}

/// 0x153002 (gfx: draw tile `tile` at pixel (x, y) on the screen).
pub(crate) fn blit_tile(s: &State, fb: &mut Framebuffer, tile: u16, x: i16, y: i16) {
    crate::gfx::blit::blit_front(&s.gfx, fb, tile, x as u16, y as u16);
}

/// 0x152ffa (gfx: the same tile blit into the other screen buffer, 0x161be2).
pub(crate) fn blit_tile_back(s: &State, back: &mut Framebuffer, tile: u16, x: i16, y: i16) {
    crate::gfx::blit::blit_back(&s.gfx, back, tile, x as u16, y as u16);
}

/// 0x151fe6 (gfx: copy the inclusive pixel rectangle (x0, y0)..(x1, y1) of the other screen
/// buffer to (dx, dy) on the screen, whole bytes of 8 pixels, all five planes).
#[allow(clippy::too_many_arguments)]
pub(crate) fn copy_rect(
    back: &Framebuffer,
    front: &mut Framebuffer,
    x0: i16,
    y0: i16,
    x1: i16,
    y1: i16,
    dx: i16,
    dy: i16,
) {
    let w = |v: i16| v as u16;
    crate::gfx::blit::copy_rect(back, front, w(x0), w(y0), w(x1), w(y1), w(dx), w(dy));
}

/// 0x102ef0 (gfx: rebuild the view if dirty and redraw it).
pub(crate) fn refresh_view(s: &mut State, io: &Io) {
    crate::gfx::refresh_view(s, io);
}

/// 0x102c90 (gfx: render the map window; with `flag` add the cursor and border and present).
pub(crate) fn present_map(s: &mut State, io: &Io, flag: i16) {
    crate::gfx::render_window(s, io, flag != 0);
}

/// 0x124e0e (combat: distance between two window cells from the A4-0x69a0 table; the
/// cross-hair passes (5, 5) and the cell).
pub(crate) fn distance(s: &State, x0: i16, y0: i16, x1: i16, y1: i16) -> i16 {
    crate::combat::leaves::cell_distance(s, x0, y0, x1, y1)
}

/// 0x1225c0 (combat: the object at a map location a hit lands on, as `actor` sees it).
pub(crate) fn actor_at(s: &mut State, actor: u8, x: i16, y: i16) -> i16 {
    crate::combat::leaves::actor_at(s, i16::from(actor), x, y)
}

/// Map-window caches A4-0x15f0 (byte per window cell, 11 × 11: first draw-list node, non-zero =
/// something stands there) and A4-0x1476 (word per cell: tile shown), both built by 0x109e74 →
/// 0x10e81e (`objects.draw`).
pub(crate) fn window_cell(s: &State, col: i16, row: i16) -> (bool, u16) {
    let d = &s.objects.draw;
    let i = usize::try_from(row * 11 + col).ok().filter(|&i| i < 121);
    i.map_or((false, 0xff), |i| (d.cell[i] != 0, d.base[i]))
}

/// 0x135120 (boot: step the overlay frame unless A4-0x7b64 is set).
pub(crate) fn anim_step(s: &mut State) {
    crate::main_loop::anim_step(s);
}

/// 0x103632 called with 1 at the end of the sky bar: with a non-zero argument it skips its
/// random wind change, so this call draws no random number.
pub(crate) fn sky_bar_tail(s: &mut State) {
    crate::main_loop::wind_change(s, 1);
}
