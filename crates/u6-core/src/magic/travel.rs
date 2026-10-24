//! The spells that move the party or its view: Gate Travel 0x11916e, Wizard Eye 0x119afc and
//! Locate 0x11a470.

use std::cell::RefCell;

use super::effects::{FMT_NL_S_NL, NEWLINE, print_at, say};
use super::seams;
use super::tables::Msg;
use crate::engine::Io;
use crate::state::State;
use crate::ui::printf::Arg;

/// Hunk 11: the Gate Travel prompt, then the Locate formats and the four hemisphere letters
/// (latitude, south, north, longitude, east, west; `{` is the degree glyph of the font).
pub(super) const MSG_TO_PHASE: u32 = 0x0011_5a1e;
pub(super) const FMT_LAT: u32 = 0x0011_5a72;
pub(super) const MSG_SOUTH: u32 = 0x0011_5a78;
pub(super) const MSG_NORTH: u32 = 0x0011_5a7a;
pub(super) const FMT_LONG: u32 = 0x0011_5a7c;
pub(super) const MSG_EAST: u32 = 0x0011_5a82;
pub(super) const MSG_WEST: u32 = 0x0011_5a86;
/// Port-only (spec 08, slice K2; 2026-10-24): those strings, for the locale layer's catalogue
/// ([`super::PRINTED`]).
pub(super) const PRINTED: &[u32] = &[
    MSG_TO_PHASE,
    FMT_LAT,
    MSG_SOUTH,
    MSG_NORTH,
    FMT_LONG,
    MSG_EAST,
    MSG_WEST,
];
/// The wizard's eye object.
const EYE: u16 = 0x34;
/// Wizard Eye's moves.
const EYE_STEPS: i16 = 0x28;

/// Original: 0x11916e (magic: Gate Travel: the phase prompt (0x115a1e) and up to four characters
/// (0x107cba), a newline, the caster's flash; a number 1..8 (0x128b98) whose moonstone is placed
/// (the words at A4-0x7b5a plus 6 (n − 1) not 0, 0, 0) sends the party through the moongate to it
/// (0x10952e); anything else does nothing)
pub async fn gate_travel(state: &RefCell<State>, io: &Io) {
    print_at(state, io, MSG_TO_PHASE, &[]).await;
    let (_, text) = crate::ui::keyread::read_line(state, io, 4).await;
    print_at(state, io, NEWLINE, &[]).await;
    let caster = super::cast::caster(&state.borrow());
    // The flash is on the screen before the party goes (0x1191a2, then 0x10952e).
    super::spells::flash_actor(state, io, caster, 0).await;
    let n = seams::parse_number(&state.borrow(), &text);
    if !(1..=8).contains(&n) {
        return;
    }
    let mut s = state.borrow_mut();
    let at = 0x2a + 6 * usize::from(n as u16 - 1);
    if (0..3).any(|i| s.actors.tail_word(at + 2 * i) != 0) {
        crate::map::travel::moongate(&mut s, (n - 1) as u8);
    }
}

/// The view after the centre moved: the draw lists (0x10e81e), the window cache (0x108f90),
/// the refresh (0x102ef0).
fn redraw_centre(s: &mut State, io: &Io) {
    crate::objects::build_draw_lists(s);
    crate::map::region::refresh_region(s);
    crate::gfx::view::refresh_view(s, io);
}

/// The map tile under the view centre (0x10996c).
fn centre_tile(s: &State) -> u8 {
    let c = s.map.centre;
    crate::actors::seams::tile_at(s, c.x, c.y, u16::from(c.z))
}

/// Original: 0x119afc (magic: Wizard Eye: a free view that moves from (x, y), `mode` ≠ 0 with
/// the flashes)
///
/// The step-blocked word A4-0x6a90 is set; the view centre moves to (x, y) with an eye object
/// (0x34) there, every view tile shown (A4-0x794c), the view redrawn dissolving. Up to 40 steps
/// follow while the key is not escape (0x1b) and the centre's map tile is below 0xfc: a key is
/// read without a target pending (A4-0x6a90 cleared); a direction moves the centre and the eye
/// a tile and redraws. Then the eye goes, the centre comes back, the view is redrawn dissolving
/// and `Msg::Done`; with the inventory panel up its contents are redrawn (0x111be0).
pub async fn wizard_eye(state: &RefCell<State>, io: &Io, x: i16, y: i16, mode: i16) {
    if mode != 0 {
        super::spells::flash_window(state, io, 6).await;
        let caster = super::cast::caster(&state.borrow());
        // On the screen before the eye moves the view (0x119b36, then the eye's redraws).
        super::spells::flash_actor(state, io, caster, 0).await;
    }
    let (saved, eye) = {
        let mut s = state.borrow_mut();
        s.actors.work.step_blocked = 1;
        let saved = (s.map.centre.x, s.map.centre.y);
        s.map.centre.x = x as u16;
        s.map.centre.y = y as u16;
        let z = u16::from(s.map.centre.z);
        let eye = super::area::new_object(&mut s, EYE, x as u16, y as u16, z);
        s.map.reveal_all = 1;
        s.gfx.dissolve = 1;
        redraw_centre(&mut s, io);
        (saved, eye)
    };
    let mut tile = centre_tile(&state.borrow());
    let (mut steps, mut key) = (EYE_STEPS, 0u16);
    while steps >= 0 && key != 0x1b && tile < 0xfc {
        {
            let mut s = state.borrow_mut();
            s.ui.input.target_pending = 0;
            s.actors.work.step_blocked = 0;
        }
        key = crate::ui::wait::read_key(state, io).await;
        if key != 0x80 {
            continue;
        }
        let mut s = state.borrow_mut();
        let d = usize::from(s.ui.input.direction as u16 & 7);
        let c = s.map.centre;
        let (nx, ny) = (
            c.x.wrapping_add_signed(s.objects.defs.dx(d)),
            c.y.wrapping_add_signed(s.objects.defs.dy(d)),
        );
        s.map.centre.x = nx;
        s.map.centre.y = ny;
        crate::objects::move_to(&mut s, eye, nx, ny, u16::from(c.z));
        redraw_centre(&mut s, io);
        steps -= 1;
        tile = centre_tile(&s);
    }
    {
        let mut s = state.borrow_mut();
        crate::objects::remove(&mut s, eye);
        s.map.reveal_all = 0;
        (s.map.centre.x, s.map.centre.y) = saved;
        crate::objects::build_draw_lists(&mut s);
        crate::map::region::refresh_region(&mut s);
        s.gfx.dissolve = 1;
        crate::gfx::view::refresh_view(&mut s, io);
    }
    say(state, io, FMT_NL_S_NL, Msg::Done).await;
    let shown = {
        let s = state.borrow();
        let m = usize::try_from(s.ui.panel.member).unwrap_or(0).min(15);
        (s.ui.panel.mode == 0x92).then(|| u16::from(s.actors.party.members[m]))
    };
    if let Some(a) = shown {
        seams::inventory_of(state, io, a).await;
    }
}

/// Latitude and longitude of 0x11a470 for the place `c` (south and east positive).
pub(super) fn degrees(c: u6_formats::position::Position) -> (i16, i16) {
    let (mut x, mut y) = (c.x as i16, c.y as i16);
    if c.z != 0 {
        x = x.wrapping_shl(2);
        y = y.wrapping_shl(2);
    }
    (y.wrapping_sub(0x168) >> 3, x.wrapping_sub(0x130) >> 3)
}

/// Original: 0x11a470 (magic: Locate: the centre as degrees: latitude (y − 0x168) / 8 and
/// longitude (x − 0x130) / 8 (arithmetic shifts; the coordinates × 4 below the surface),
/// each with its hemisphere letter (0x115a72 .. 0x115a86))
pub async fn locate(state: &RefCell<State>, io: &Io) {
    let (lat, long) = degrees(state.borrow().map.centre);
    print_at(state, io, FMT_LAT, &[Arg::Word(lat.wrapping_abs() as u16)]).await;
    print_at(state, io, if lat > 0 { MSG_SOUTH } else { MSG_NORTH }, &[]).await;
    print_at(
        state,
        io,
        FMT_LONG,
        &[Arg::Word(long.wrapping_abs() as u16)],
    )
    .await;
    print_at(state, io, if long > 0 { MSG_EAST } else { MSG_WEST }, &[]).await;
}
