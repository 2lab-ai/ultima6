//! The spells with their own routine in 0x11a586's cases, the window flash 0x115b7c and the
//! resurrection 0x11785e / 0x117b94.

use std::cell::RefCell;

use super::book::quality;
use super::effects::{FMT_NL_S_NL, FMT_S, FMT_S_NL, NEWLINE, print_at, print_name, say};
use super::seams;
use super::tables::Msg;
use crate::engine::Io;
use crate::geom::SHOWN;
use crate::objects::Slot;
use crate::state::State;
use crate::ui::printf::Arg;

pub(super) const MSG_FOOD: u32 = 0x0011_59ca;
const MSG_TOO_OLD: u32 = 0x0011_58ea;
/// Port-only (spec 08, slice K2; 2026-10-24): the strings these spells pass to `printf` as its
/// format, for the locale layer's catalogue ([`super::PRINTED`]): the food and too-old lines and
/// Wind Change's prompt, no-direction word and name format (the direction's name goes through
/// `%s`, a label read for display, [`super::tables::LABELS`]).
pub(super) const PRINTED: &[u32] = &[MSG_FOOD, MSG_TOO_OLD, MSG_DIRECTION, MSG_NOWHERE, FMT_S_DOT];
/// Food object type.
const FOOD: u16 = 0x81;
/// [replaced] Engine milliseconds of one pass of [`flash_window`] (the XOR 0x1521f4, the border,
/// the copy), which is how long the XORed window stays on the screen: 108 ms, 5.41 PAL frames,
/// half of what is left of the flash's 17.21 frames (timed on the CPU clock,
/// tools/oracle/timeit.mjs window; 16.96 from the other idle render it can stop at) after its
/// render ([`crate::combat::present::RENDER_MS`]).
pub const WINDOW_FLASH_MS: u16 = 108;

/// The caster flash 0x132338 (`mode` 0 on the caster, 1 on a target) through its seam, then
/// its frames played: the original draws the flash inside the call, so it is on the screen
/// before the window flash 0x115b7c or the waits that follow it in the spells.
pub async fn flash_actor(state: &RefCell<State>, io: &Io, actor: u16, mode: u16) {
    seams::flash_actor(&mut state.borrow_mut(), io, actor, mode);
    crate::combat::play_effects(state, io).await;
}

/// Original: 0x115b7c (magic: flash the map window for a spell of circle `circle` < 9)
///
/// Plot colour 15, XOR into the back buffer: the window is rendered (0x102c90 without the
/// frame, which sets its own plot colour A4-0x6570 first, so that colour is the one XORed), the
/// rectangle (8, 8)..(0xa8, 0xa8) XORed (the shown map and one pixel past its far edges: from
/// [`SHOWN`]'s top left to one past its bottom right, spec 13), the border redrawn over it
/// (0x1324ae) and the window copied to the screen (0x151f5a); then the same XOR again, which
/// restores it.
///
/// The XORed window is on the screen while the second pass draws; the engine publishes only
/// the last present of a poll, so the port waits on the engine clock before each pass is shown,
/// the render and a pass before the first, [`WINDOW_FLASH_MS`] before the second (no borrow held).
pub async fn flash_window(state: &RefCell<State>, io: &Io, circle: u16) {
    if circle >= 9 {
        return;
    }
    {
        let mut s = state.borrow_mut();
        crate::gfx::blit::set_plot_colour(&mut s.gfx, 0xf);
        s.gfx.plot_back = true;
        s.gfx.plot_xor = true;
        crate::gfx::view::render_window(&mut s, io, false);
    }
    let first = crate::combat::present::RENDER_MS + WINDOW_FLASH_MS;
    crate::combat::present::pace(state, io, first).await;
    for pass in 0..2 {
        {
            let mut s = state.borrow_mut();
            let (x0, y0, x1, y1) = SHOWN;
            seams::fill_rect(&mut s, io, x0, y0, x1 + 1, y1 + 1);
            seams::redraw_border(&mut s, x0, y0, x1, y1);
            io.with_buffers(|back, front| crate::gfx::blit::copy_window(back, front));
            io.present();
        }
        if pass == 0 {
            crate::combat::present::pace(state, io, WINDOW_FLASH_MS).await;
        }
    }
    let mut s = state.borrow_mut();
    s.gfx.plot_xor = false;
    s.gfx.plot_back = false;
}

/// Original: 0x118c80 (magic: Create Food: the flashes, random 1..10 food (type 0x81) into the
/// caster's inventory (0x10cf7c), the count in the food line (0x1159ca))
pub async fn create_food(state: &RefCell<State>, io: &Io, spell: u16) {
    flash_window(state, io, spell / 16 + 1).await;
    let caster = super::cast::caster(&state.borrow());
    flash_actor(state, io, caster, 0).await;
    let n = {
        let mut s = state.borrow_mut();
        let n = s.rng.range(1, 10);
        crate::objects::add_to_owner(&mut s, Slot::from(caster as i16), FOOD, n);
        n
    };
    print_at(
        state,
        io,
        crate::string_registry::text!(MSG_FOOD),
        &[Arg::Word(n)],
    )
    .await;
}

/// The synchronous part of 0x11785e ([`resurrect_and_rejoin`]): everything before the rejoin.
/// Returns the actor.
pub fn resurrect(s: &mut State, body: i16) -> i16 {
    let a = crate::objects::actor_from_body(s, Slot::from(body));
    let Ok(i) = usize::try_from(a) else {
        return a;
    };
    let attrs = &mut s.actors.attrs;
    attrs.status[i] &= !0x10;
    attrs.hp[i] = 1;
    s.objects.type_frame[i] = attrs.base_type_frame[i];
    s.actors.attrs.magic[i] = crate::actors::leaves::max_magic(s, a as u16) as u8;
    s.actors.attrs.status[i] &= 0xe0;
    if s.actors.karma < 0x62 {
        let e = u32::from(s.actors.attrs.exp[i]) * u32::from(s.actors.karma) / 100;
        s.actors.attrs.exp[i] = e as u16;
    }
    let (x, y) = (s.objects.x(a) as i16, s.objects.y(a) as i16);
    let z = u16::from(s.map.centre.z);
    let mut placed = false;
    'rings: for r in 1..5i16 {
        for k in 0..2 * r + 1 {
            for (px, py) in [
                (x - r + k, y - r),
                (x - r, y - r + k),
                (x + r - k, y + r),
                (x + r, y + r - k),
            ] {
                if crate::actors::walk::can_move_to(s, a as u16, px as u16, py as u16) {
                    crate::objects::move_to(s, a, px as u16, py as u16, z);
                    placed = true;
                    break 'rings;
                }
            }
        }
    }
    if !placed {
        crate::objects::move_to(s, a, x as u16, y as u16, z);
    }
    a
}

/// Original: 0x11785e (magic: raise the actor that body object `body` stands for)
///
/// The actor comes back where the body lay (0x10c810), alive (status bit 0x10 cleared, then
/// bits 0..4), 1 hit point, its base type, magic points from 0x122176; below karma 0x62 its
/// experience is scaled by karma / 100. It is then put on the first tile it may stand on
/// (0x11b1c0) on square rings of radius 1..4 around that place (per ring: the top edge from
/// the left, the left edge from the top, the bottom edge from the right, the right edge from
/// the bottom), else where it is ([`resurrect`]). A party member (status bit 0x80) rejoins
/// through 0x112daa, which reads its name from its script and may print (hence async); its
/// result is dropped.
pub async fn resurrect_and_rejoin(state: &RefCell<State>, io: &Io, body: i16) {
    let a = resurrect(&mut state.borrow_mut(), body);
    let member = usize::try_from(a)
        .ok()
        .is_some_and(|i| state.borrow().actors.attrs.status[i] & 0x80 != 0);
    if member {
        crate::converse::join_party(&crate::converse::Cx::new(state, io), a).await;
    }
}

/// Original: 0x117b94 (magic: Resurrect: `Msg::OnWhom`, the target from the inventory panel (mode
/// 0x92 for the read) unless chosen, its name; nothing, or the map locked (A4-0x7afb):
/// `Msg::NoEffect`; a body (0x153, 0x162, 0x155, 0x154) of an actor below 0xe0 is raised
/// (`Msg::Success`), one of quality 0 gets the too-old line (0x1158ea); anything else
/// `Msg::NoEffect`)
pub async fn resurrect_spell(state: &RefCell<State>, io: &Io, pre: i16) {
    say(state, io, crate::string_registry::text!(FMT_S), Msg::OnWhom).await;
    if pre == 0 {
        let saved = {
            let mut s = state.borrow_mut();
            let saved = s.ui.panel.mode;
            s.ui.panel.mode = 0x92;
            saved
        };
        crate::ui::panels::inventory(state, io).await;
        state.borrow_mut().ui.input.target_pending = 1;
        crate::ui::wait::read_key(state, io).await;
        state.borrow_mut().ui.panel.mode = saved;
    }
    let sel = state.borrow().ui.input.selection;
    if sel == -1 {
        say(
            state,
            io,
            crate::string_registry::text!(FMT_S_NL),
            Msg::Nothing,
        )
        .await;
    } else {
        print_name(state, io, sel).await;
        print_at(state, io, crate::string_registry::text!(NEWLINE), &[]).await;
    }
    let caster = super::cast::caster(&state.borrow());
    flash_actor(state, io, caster, 0).await;
    if sel == -1 || state.borrow().map.flag_7afb != 0 {
        say(
            state,
            io,
            crate::string_registry::text!(FMT_NL_S_NL),
            Msg::NoEffect,
        )
        .await;
        return;
    }
    flash_actor(state, io, sel as u16, 1).await;
    let (kind, q) = {
        let s = state.borrow();
        (s.objects.kind(sel), quality(&s.objects, sel))
    };
    if !matches!(kind, 0x153 | 0x162 | 0x155 | 0x154) || q >= 0xe0 {
        say(
            state,
            io,
            crate::string_registry::text!(FMT_NL_S_NL),
            Msg::NoEffect,
        )
        .await;
    } else if q == 0 {
        print_at(state, io, crate::string_registry::text!(MSG_TOO_OLD), &[]).await;
    } else {
        resurrect_and_rejoin(state, io, sel).await;
        say(
            state,
            io,
            crate::string_registry::text!(FMT_NL_S_NL),
            Msg::Success,
        )
        .await;
        state.borrow_mut().magic.result = 0;
    }
}

/// Peer (0x27) in 0x11a586: the flashes, the overview of 0x12c54a with no gem, `Msg::Done`.
pub async fn peer(state: &RefCell<State>, io: &Io, spell: u16) {
    let caster = super::cast::caster(&state.borrow());
    flash_actor(state, io, caster, 0).await;
    flash_window(state, io, spell / 16 + 1).await;
    crate::objects::commands::use_up(state, io, 0).await;
    say(
        state,
        io,
        crate::string_registry::text!(FMT_NL_S_NL),
        Msg::Done,
    )
    .await;
}

/// Original: 0x118ac8 (magic: a spell on the first object of every tile within 7 of the
/// view centre: Repel Undead (0x29) makes creatures with flag 0x80 flee (worktype 0x13) unless
/// they resist; Reveal (0x47) makes them visible and unreadies a readied 0x102 they carry;
/// Confuse (0x52) turns evil actors (alignment 0x20) to neutral worktype 8 unless they resist;
/// Fear (0x63) is 0x118a4e. Then `Msg::Success`.)
///
/// Columns x outside, rows y inside, as the original. The port does not write worktypes of
/// object slots ≥ 0x100 (the original's byte table would be overrun).
pub async fn all_foes(state: &RefCell<State>, io: &Io, spell: u16) {
    let caster = super::cast::caster(&state.borrow());
    flash_actor(state, io, caster, 0).await;
    {
        let s = &mut *state.borrow_mut();
        let c = s.map.centre;
        let (cx, cy, z) = (c.x as i16, c.y as i16, u16::from(c.z));
        for x in cx - 7..=cx + 7 {
            for y in cy - 7..=cy + 7 {
                let o = crate::objects::begin_at(&mut s.objects, &s.tables, x as u16, y as u16, z);
                if o >= 0 {
                    foe(s, spell, caster, o);
                }
            }
        }
    }
    say(
        state,
        io,
        crate::string_registry::text!(FMT_NL_S_NL),
        Msg::Success,
    )
    .await;
}

/// One tile's object under 0x118ac8.
fn foe(s: &mut State, spell: u16, caster: u16, o: Slot) {
    let i = o as usize;
    match spell {
        0x29 => {
            let idx = crate::objects::monster_index(&s.objects, s.objects.kind(o));
            if s.actors.game.creature_flags(idx) & 0x80 != 0
                && !seams::resists(s, caster, o as u16)
                && i < 0x100
            {
                s.actors.worktype[i] = 0x13;
            }
        }
        0x47 => {
            s.objects.obj_status[i] &= !2;
            let ring = crate::objects::find_child(&mut s.objects, o, 0x102, 0);
            if ring != -1 && s.objects.status(ring) & 0x18 == 0x18 {
                let st = &mut s.objects.obj_status[ring as usize];
                *st = (*st & 0xe7) | 0x10;
            }
        }
        0x52 => {
            if i < 0x100
                && s.actors.attrs.status[i] & 0x60 == 0x20
                && !seams::resists(s, caster, o as u16)
            {
                s.actors.worktype[i] = 8;
                s.actors.attrs.status[i] &= 0x9f;
            }
        }
        0x63 => super::effects::frighten(s, o as u16),
        _ => {}
    }
}

/// Locate (0x35) in 0x11a586: the flashes, then 0x11a470.
pub async fn locate(state: &RefCell<State>, io: &Io, spell: u16) {
    let caster = super::cast::caster(&state.borrow());
    flash_actor(state, io, caster, 0).await;
    flash_window(state, io, spell / 16 + 1).await;
    super::travel::locate(state, io).await;
}

const MSG_DIRECTION: u32 = 0x0011_5a08;
const MSG_NOWHERE: u32 = 0x0011_5a8a;
const FMT_S_DOT: u32 = 0x0011_5a94;
/// A4-0x61be: the direction names, a pointer per direction 0..7.
const DIRECTION_NAMES: u32 = 0x61be;

/// Original: 0x11a508 (magic: Wind Change: the direction prompt (0x115a08), one key without a
/// target pending; a direction prints its name, anything else the no-direction word (0x115a8a); the
/// wind takes the last direction read (A4-0x2efc) either way)
pub async fn wind_change(state: &RefCell<State>, io: &Io) {
    print_at(state, io, crate::string_registry::text!(MSG_DIRECTION), &[]).await;
    state.borrow_mut().ui.input.target_pending = 0;
    let key = crate::ui::wait::read_key(state, io).await;
    let dir = state.borrow().ui.input.direction;
    if key == 0x80 {
        // Port (spec 08, slice K2; 2026-10-24): read for display, so the locale pack's label of
        // it when it has one ([`super::tables::LABELS`], [`crate::locale::shown`]).
        let name = crate::locale::shown(
            &state.borrow(),
            crate::string_registry::text_table!(DIRECTION_NAMES)
                .get(dir as usize)
                .map_or_else(Vec::new, |text| text.io(io)),
        );
        print_at(
            state,
            io,
            crate::string_registry::text!(FMT_S_DOT),
            &[Arg::Str(&name)],
        )
        .await;
    } else {
        print_at(state, io, crate::string_registry::text!(MSG_NOWHERE), &[]).await;
    }
    state.borrow_mut().actors.wind = dir as u8;
}

/// Original: 0x119200 (magic: X-ray: the flashes, every view tile shown (A4-0x794c = 1, view
/// flags) for 100 view refreshes (0x102ef0), then the normal view and `Msg::Done`)
///
/// The port waits each refresh's time on the engine clock before drawing it (the original's
/// redraws take that long: [`crate::combat::present::REFRESH_MS`], 0x102ef0 as measured in the
/// oracle; without a wait the port would publish none of them).
pub async fn xray(state: &RefCell<State>, io: &Io, spell: u16) {
    flash_window(state, io, spell / 16 + 1).await;
    let caster = super::cast::caster(&state.borrow());
    flash_actor(state, io, caster, 0).await;
    {
        let mut s = state.borrow_mut();
        s.map.reveal_all = 1;
        crate::map::view::update_view(&mut s);
    }
    for _ in 0..100 {
        crate::combat::present::pace(state, io, crate::combat::present::REFRESH_MS).await;
        crate::gfx::view::refresh_view(&mut state.borrow_mut(), io);
    }
    // The last X-ray view stays up while the normal one is drawn.
    crate::combat::present::pace(state, io, crate::combat::present::REFRESH_MS).await;
    {
        let mut s = state.borrow_mut();
        s.map.reveal_all = 0;
        crate::map::view::update_view(&mut s);
        crate::gfx::view::refresh_view(&mut s, io);
    }
    say(
        state,
        io,
        crate::string_registry::text!(FMT_NL_S_NL),
        Msg::Done,
    )
    .await;
}
