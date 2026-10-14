//! Calls from the ui into other modules' original functions, reconciled at integration
//! (WU-P7): each forwards to the owner's port; the ones whose target is outside milestone 1
//! keep a `todo_stub!` with the explicit fallback.

use u6_render::Framebuffer;

use crate::engine::Io;
use crate::state::State;
use crate::todo_stub;

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

/// 0x124e0e (combat: distance between two window cells; the cross-hair passes (5, 5) and the
/// cell). Contract: → distance in cells. Fallback [추정]: the larger of |dx| and |dy|.
pub(crate) fn distance(x0: i16, y0: i16, x1: i16, y1: i16) -> i16 {
    let d = (x1 - x0).abs().max((y1 - y0).abs());
    todo_stub!("0x124e0e distance between cells", d)
}

/// 0x1225c0 (combat: the actor at a map location, starting from `actor`'s level): walks the
/// objects at (x, y) (0x10ad1a / 0x10aaca), skipping any that 0x1224ea says `actor` cannot see;
/// the first visible actor slot below 0x100 without status bit 0x10 wins, else the first visible
/// object, else -1. Not ported (a combat leaf outside milestone 1). Fallback: the first actor
/// standing there (objects 0x10b20e), visibility not checked.
pub(crate) fn actor_at(s: &State, actor: u8, x: i16, y: i16) -> i16 {
    let _ = actor;
    let found = crate::objects::actor_at(s, 1, x as u16, y as u16);
    todo_stub!("0x1225c0 actor at a location", found)
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

/// The NUL-terminated string at image address `addr` of the user's `game` (main_loop's reader).
pub(crate) fn game_string(io: &Io, addr: u32) -> Vec<u8> {
    crate::main_loop::game_string(io, addr)
}

/// 0x12ba36(chest) (objects: spring the trap in a chest: acid, poison, bomb or gas, then remove
/// the trap). The objects port's stub takes the trap object itself, so this passes the trap
/// 0x10b18c finds in the chest (type 0x151, quality 0x16), as 0x12ba36 does at its end
/// (0x116526).
pub(crate) fn spring_trap(s: &mut State, chest: i16) {
    let trap = crate::objects::find_child(&mut s.objects, chest, 0x151, 0x16);
    if trap >= 0 {
        crate::objects::spring_trap(s, trap);
    }
}

/// [magic] A4+0x181a + `tab` (bytes: the spellbook's page tabs: previous circle, previous page,
/// next page, next circle; 0xff = none). Contract: the magic module's page state. Fallback: no
/// tab (the spellbook panel 0xa1 is not reached without it).
pub(crate) fn spellbook_tab(s: &State, tab: usize) -> u8 {
    let _ = (s, tab);
    todo_stub!("0x116218 spellbook page state (tabs, lines)", 0xff)
}

/// [magic] A4+0x1809 (byte: lines on the shown spellbook page). Fallback: none.
pub(crate) fn spellbook_lines(s: &State) -> u8 {
    let _ = s;
    todo_stub!("0x116218 spellbook page state (tabs, lines)", 0)
}
