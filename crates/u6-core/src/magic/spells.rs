//! The spells with their own routine in 0x11a586's cases, the window flash 0x115b7c and the
//! resurrection 0x11785e / 0x117b94.

use std::cell::RefCell;

use super::book::quality;
use super::effects::{FMT_NL_S_NL, FMT_S, FMT_S_NL, NEWLINE, print_at, print_name, say};
use super::seams;
use super::tables::Msg;
use crate::engine::Io;
use crate::objects::Slot;
use crate::state::State;
use crate::todo_stub;
use crate::ui::printf::Arg;

const MSG_FOOD: u32 = 0x0011_59ca;
const MSG_TOO_OLD: u32 = 0x0011_58ea;
/// Food object type.
const FOOD: u16 = 0x81;

/// The caster flash 0x132338 (`mode` 0 on the caster, 1 on a target) through its seam.
pub async fn flash_actor(state: &RefCell<State>, io: &Io, actor: u16, mode: u16) {
    seams::flash_actor(&mut state.borrow_mut(), io, actor, mode);
}

/// Original: 0x115b7c (magic: flash the map window for a spell of circle `circle` < 9)
///
/// Plot colour 15, XOR into the back buffer: the window is rendered (0x102c90 without the
/// frame), the rectangle (8, 8)..(0xa8, 0xa8) XORed, the border redrawn over it (0x1324ae) and
/// the window copied to the screen (0x151f5a); then the same XOR again, which restores it.
pub async fn flash_window(state: &RefCell<State>, io: &Io, circle: u16) {
    if circle >= 9 {
        return;
    }
    let mut s = state.borrow_mut();
    crate::gfx::blit::set_plot_colour(&mut s.gfx, 0xf);
    s.gfx.plot_back = true;
    s.gfx.plot_xor = true;
    crate::gfx::view::render_window(&mut s, io, false);
    for _ in 0..2 {
        seams::fill_rect(&mut s, io, 8, 8, 0xa8, 0xa8);
        seams::redraw_border(&mut s, 8, 8, 0xa7, 0xa7);
        io.with_buffers(|back, front| crate::gfx::blit::copy_window(back, front));
        io.present();
    }
    s.gfx.plot_xor = false;
    s.gfx.plot_back = false;
}

/// Original: 0x118c80 (magic: Create Food: the flashes, random 1..10 food (type 0x81) into the
/// caster's inventory (0x10cf7c), "Create %d food.")
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
    print_at(state, io, MSG_FOOD, &[Arg::Word(n)]).await;
}

/// Original: 0x11785e (magic: raise the actor that body object `body` stands for) [partial]
///
/// The actor comes back where the body lay (0x10c810), alive (status bit 0x10 cleared, then
/// bits 0..4), 1 hit point, its base type, magic points from 0x122176; below karma 0x62 its
/// experience is scaled by karma / 100. It is then put on the first tile it may stand on
/// (0x11b1c0) on square rings of radius 1..4 around that place (per ring: the top edge from
/// the left, the left edge from the top, the bottom edge from the right, the right edge from
/// the bottom), else where it is. Returns the actor. A party member (status bit 0x80) rejoins
/// through 0x112daa, which reads its name from its script (async): [`resurrect_join`] does
/// it; this synchronous part leaves it to the caller.
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

/// 0x11785e with its rejoin: [`resurrect`], then 0x112daa for a party member.
pub async fn resurrect_join(state: &RefCell<State>, io: &Io, body: i16) {
    let a = resurrect(&mut state.borrow_mut(), body);
    let member = usize::try_from(a)
        .ok()
        .is_some_and(|i| state.borrow().actors.attrs.status[i] & 0x80 != 0);
    if member {
        crate::converse::join_party(&crate::converse::Cx::new(state, io), a).await;
    }
}

/// Original: 0x117b94 (magic: Resurrect: "On whom: ", the target from the inventory panel
/// (mode 0x92 for the read) unless chosen, its name; nothing, or the map locked (A4-0x7afb):
/// "No effect"; a body (0x153, 0x162, 0x155, 0x154) of an actor below 0xe0 is raised
/// ("Success"), one of quality 0 is "too old"; anything else "No effect")
pub async fn resurrect_spell(state: &RefCell<State>, io: &Io, pre: i16) {
    say(state, io, FMT_S, Msg::OnWhom).await;
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
        say(state, io, FMT_S_NL, Msg::Nothing).await;
    } else {
        print_name(state, io, sel).await;
        print_at(state, io, NEWLINE, &[]).await;
    }
    let caster = super::cast::caster(&state.borrow());
    flash_actor(state, io, caster, 0).await;
    if sel == -1 || state.borrow().map.flag_7afb != 0 {
        say(state, io, FMT_NL_S_NL, Msg::NoEffect).await;
        return;
    }
    flash_actor(state, io, sel as u16, 1).await;
    let (kind, q) = {
        let s = state.borrow();
        (s.objects.kind(sel), quality(&s.objects, sel))
    };
    if !matches!(kind, 0x153 | 0x162 | 0x155 | 0x154) || q >= 0xe0 {
        say(state, io, FMT_NL_S_NL, Msg::NoEffect).await;
    } else if q == 0 {
        print_at(state, io, MSG_TOO_OLD, &[]).await;
    } else {
        resurrect_join(state, io, sel).await;
        say(state, io, FMT_NL_S_NL, Msg::Success).await;
        state.borrow_mut().magic.result = 0;
    }
}

// ---- spells not ported yet: each is a stub point with the contract of its routine ----------

/// Stub of 0x118494 (Reappear: the vanished object type A4-0x7b62 back at a chosen spot).
/// Fallback: nothing (the reagents and points are spent already).
pub async fn reappear(_state: &RefCell<State>, _io: &Io, _pre: i16) {
    todo_stub!("0x118494 Reappear spell", ())
}

/// Stub of 0x118e98 (Telekinesis: move or use an object at a distance). Fallback: nothing.
pub async fn telekinesis(_state: &RefCell<State>, _io: &Io, _pre: i16) {
    todo_stub!("0x118e98 Telekinesis spell", ())
}

/// Stub of 0x118602 (the mass spells on the actors around a chosen location: a missile of
/// `tile`, then the effect of `spell`; `field` = the object type of the visual). Fallback:
/// nothing.
pub async fn area(
    _state: &RefCell<State>,
    _io: &Io,
    _spell: u16,
    _tile: u16,
    _field: u16,
    _pre: i16,
) {
    todo_stub!("0x118602 mass spell at a location", ())
}

/// Stub of 0x12c54a (Peer: the map view of the area) for spell 0x27; the original then
/// prints "Done". Fallback: no map, "Done" printed.
pub async fn peer(state: &RefCell<State>, io: &Io, spell: u16) {
    let caster = super::cast::caster(&state.borrow());
    flash_actor(state, io, caster, 0).await;
    flash_window(state, io, spell / 16 + 1).await;
    todo_stub!("0x12c54a Peer: show the area map", ());
    say(state, io, FMT_NL_S_NL, Msg::Done).await;
}

/// Stub of 0x118ac8 (a spell on every foe in view: Repel Undead, Reveal, Confuse, Fear).
/// Fallback: nothing.
pub async fn all_foes(_state: &RefCell<State>, _io: &Io, _spell: u16) {
    todo_stub!("0x118ac8 spell on every foe in view", ())
}

/// Stub of 0x118cec (Conjure / Summon a creature next to `caster`). Fallback: nothing.
pub async fn summon(_state: &RefCell<State>, _io: &Io, _spell: u16, _caster: u16) {
    todo_stub!("0x118cec Conjure / Summon", ())
}

/// Stub of 0x11883c (a field of object type `tile` at a chosen location). Fallback: nothing.
pub async fn field(_state: &RefCell<State>, _io: &Io, _spell: u16, _tile: u16, _pre: i16) {
    todo_stub!("0x11883c field spell", ())
}

/// Stub of 0x11a470 (Locate: the party's position as degrees). Fallback: nothing.
pub async fn locate(state: &RefCell<State>, io: &Io, spell: u16) {
    let caster = super::cast::caster(&state.borrow());
    flash_actor(state, io, caster, 0).await;
    flash_window(state, io, spell / 16 + 1).await;
    todo_stub!("0x11a470 Locate: print the position", ())
}

/// Stub of 0x11a508 (Wind Change: a direction prompt). Fallback: nothing.
pub async fn wind_change(_state: &RefCell<State>, _io: &Io) {
    todo_stub!("0x11a508 Wind Change", ())
}

/// Stub of 0x1195f2 (Insect Swarm). Fallback: nothing.
pub async fn insect_swarm(_state: &RefCell<State>, _io: &Io) {
    todo_stub!("0x1195f2 Insect Swarm", ())
}

/// Stub of 0x119266 (Pickpocket: take an object from an actor). Fallback: nothing.
pub async fn pickpocket(_state: &RefCell<State>, _io: &Io) {
    todo_stub!("0x119266 Pickpocket", ())
}

/// Stub of 0x119dbc (Seance: talk to the dead, 0x1125b8 with A4+0x1821). Fallback: nothing.
pub async fn seance(_state: &RefCell<State>, _io: &Io, _pre: i16) {
    todo_stub!("0x119dbc Seance", ())
}

/// Stub of 0x119200 (X-ray: the view through walls). Fallback: nothing.
pub async fn xray(_state: &RefCell<State>, _io: &Io) {
    todo_stub!("0x119200 X-ray", ())
}

/// Stub of 0x119f26 (the wind spells: a line of missiles `tile` in a direction). Fallback:
/// nothing.
pub async fn wind(_state: &RefCell<State>, _io: &Io, _spell: u16, _tile: u16, _pre: i16) {
    todo_stub!("0x119f26 wind spell", ())
}

/// Stub of 0x119e8c (Hail Storm). Fallback: nothing.
pub async fn hail_storm(_state: &RefCell<State>, _io: &Io, _pre: i16) {
    todo_stub!("0x119e8c Hail Storm", ())
}

/// Stub of 0x11971a (Web). Fallback: nothing.
pub async fn web(_state: &RefCell<State>, _io: &Io) {
    todo_stub!("0x11971a Web", ())
}

/// Stub of 0x119876 (Enchant: a wand gets a charge of a spell). Fallback: nothing.
pub async fn enchant(_state: &RefCell<State>, _io: &Io) {
    todo_stub!("0x119876 Enchant", ())
}

/// Stub of 0x11916e (Gate Travel: "To phase" prompt, then the moongate travel). Fallback:
/// nothing.
pub async fn gate_travel(_state: &RefCell<State>, _io: &Io) {
    todo_stub!("0x11916e Gate Travel", ())
}

/// Stub of 0x11a0cc (Wing Strike: every foe in view, "No foes."). Fallback: nothing.
pub async fn wing_strike(_state: &RefCell<State>, _io: &Io) {
    todo_stub!("0x11a0cc Wing Strike", ())
}

/// Stub of 0x119afc (Wizard Eye: a free-moving view from (x, y)). Fallback: nothing.
pub async fn wizard_eye(_state: &RefCell<State>, _io: &Io, _x: i16, _y: i16, _mode: i16) {
    todo_stub!("0x119afc Wizard Eye", ())
}

/// Stub of 0x11a550 (Armageddon: every creature but the caster and Lord British dies).
/// Fallback: nothing.
pub async fn armageddon(_state: &RefCell<State>, _io: &Io) {
    todo_stub!("0x11a550 Armageddon", ())
}

/// Stub of 0x118382 (Slime). Fallback: nothing.
pub async fn slime(_state: &RefCell<State>, _io: &Io) {
    todo_stub!("0x118382 Slime", ())
}

/// Stub of 0x119cbe (Tremor: quake, damage to the foes in view). Fallback: nothing.
pub async fn tremor(_state: &RefCell<State>, _io: &Io) {
    todo_stub!("0x119cbe Tremor", ())
}

/// Stub of 0x11a41e (spell number 0x80, outside the eight circles; not reachable from a book).
/// Fallback: nothing.
pub async fn spell_0x80(_state: &RefCell<State>, _io: &Io) {
    todo_stub!("0x11a41e spell 0x80", ())
}
