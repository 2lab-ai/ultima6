//! The spells on objects and the dead: Telekinesis 0x118e98, Pickpocket 0x119266, Enchant
//! 0x119876 and Seance 0x119dbc.

use std::cell::RefCell;

use super::effects::{FMT_NL_S_NL, FMT_S, FMT_S_NL, NEWLINE, print_at, print_name, say};
use super::seams;
use super::tables::Msg;
use crate::engine::Io;
use crate::objects::Slot;
use crate::state::State;
use crate::ui::printf::Arg;

/// "\nNot Possible\n", "Direction-", "nowhere\n", "Which object: ", "\nCan't carry!\n",
/// "\nIt's full\n", "With: ", "none\n", " %s" (hunk 11).
const MSG_NOT_POSSIBLE: u32 = 0x0011_59f8;
const MSG_DIRECTION: u32 = 0x0011_5a08;
const MSG_NOWHERE: u32 = 0x0011_5a14;
const MSG_WHICH: u32 = 0x0011_5a28;
const MSG_CANT_CARRY: u32 = 0x0011_5a38;
const MSG_FULL: u32 = 0x0011_5a48;
const MSG_WITH: u32 = 0x0011_5a54;
const MSG_NONE: u32 = 0x0011_5a5c;
const FMT_SPACE_S: u32 = 0x0011_5a62;
/// A4-0x61be: a name pointer per direction.
const DIRECTION_NAMES: u32 = 0x61be;
/// The objects Telekinesis treats apart: the drawbridge crank, the lever, the cannon.
const CRANK: u16 = 0x120;
const LEVER: u16 = 0x10c;
const CANNON: u16 = 0xdd;
/// A wand, and the spell object a wand holds.
const WAND: u16 = 0x4e;
const SPELL_OBJECT: u16 = 0x150;

/// True when the selection or the location is missing.
fn missing(s: &State) -> bool {
    super::effects::no_target(s)
}

/// The caster and whether it is a party member (status bit 7).
fn caster_member(s: &State) -> (u16, bool) {
    let c = super::cast::caster(s);
    (c, s.actors.attrs.status[usize::from(c)] & 0x80 != 0)
}

/// The selection's name and a newline, or "nothing".
async fn name_or_nothing(state: &RefCell<State>, io: &Io, none: bool) {
    if none {
        say(state, io, FMT_S_NL, Msg::Nothing).await;
    } else {
        let sel = state.borrow().ui.input.selection;
        print_name(state, io, sel).await;
        print_at(state, io, NEWLINE, &[]).await;
    }
}

/// The missile from the caster to the selected location (range 8) when one is set.
async fn shoot(state: &RefCell<State>, io: &Io, caster: u16, tile: u16) -> bool {
    let mut target = state.borrow().ui.input.target;
    if target.0 == -1 || target.1 == -1 {
        return false;
    }
    let r = seams::missile(state, io, caster, 8, &mut target, tile).await;
    state.borrow_mut().ui.input.target = target;
    r != 0
}

/// Original: 0x118e98 (magic: Telekinesis: push an object at a distance, or work a crank or a
/// lever)
///
/// "On what: " for a member caster, the target read unless chosen; its name or "nothing"; the
/// caster flashes and the missile 0x17f flies to it. No object (an actor slot, or no
/// location): "No effect"; short: "Blocked". A crank (0x120) works (0x12e3f6), a lever (0x10c)
/// switches (0x12e538); an object of weight 0 (0x13a540): "Not Possible". Otherwise it
/// flashes, "Direction-" and one key without a target pending: no direction "nowhere"; a
/// direction prints its name and the object moves a tile that way (0x10c9d2) unless the push
/// is blocked (0x12acc0, "Blocked"); a cannon (0xdd) turns to face it; the view flags. The
/// original moves the origin of the pointer direction (A4-0x7a26 / -0x7a25, 0x105016) onto the
/// object and back to (0x58, 0x58); the port reads directions from keys only and keeps none.
pub async fn telekinesis(state: &RefCell<State>, io: &Io, pre: i16) {
    let (caster, member) = caster_member(&state.borrow());
    if member {
        say(state, io, FMT_S, Msg::OnWhat).await;
    }
    if pre == 0 {
        state.borrow_mut().ui.input.target_pending = 1;
        crate::ui::wait::read_key(state, io).await;
    }
    let none = missing(&state.borrow());
    name_or_nothing(state, io, none).await;
    seams::flash_actor(&mut state.borrow_mut(), io, caster, 0);
    let reached = shoot(state, io, caster, 0x17f).await;
    let sel = state.borrow().ui.input.selection;
    if missing(&state.borrow()) || sel < 0x100 {
        say(state, io, FMT_NL_S_NL, Msg::NoEffect).await;
        return;
    }
    if !reached {
        say(state, io, FMT_NL_S_NL, Msg::Blocked).await;
        return;
    }
    let kind = state.borrow().objects.kind(sel);
    match kind {
        CRANK => return seams::crank(state, io, sel).await,
        LEVER => return seams::lever(state, io, sel).await,
        _ => {}
    }
    let weight = {
        let s = state.borrow();
        s.tables
            .tileflags
            .weight
            .get(usize::from(kind))
            .copied()
            .unwrap_or(0)
    };
    if weight == 0 {
        print_at(state, io, MSG_NOT_POSSIBLE, &[]).await;
        return;
    }
    let (ox, oy) = {
        let mut s = state.borrow_mut();
        seams::flash_actor(&mut s, io, sel as u16, 1);
        s.ui.input.target_pending = 0;
        (s.objects.x(sel), s.objects.y(sel))
    };
    print_at(state, io, MSG_DIRECTION, &[]).await;
    if crate::ui::wait::read_key(state, io).await != 0x80 {
        print_at(state, io, MSG_NOWHERE, &[]).await;
        return;
    }
    let dir = state.borrow().ui.input.direction;
    let at = crate::image::A4.wrapping_sub(DIRECTION_NAMES) + 4 * (dir as u32);
    let name = crate::main_loop::pointed_string(io, at);
    print_at(state, io, FMT_S_NL, &[Arg::Str(&name)]).await;
    let blocked = {
        let mut s = state.borrow_mut();
        let d = usize::from(dir as u16 & 7);
        let nx = ox.wrapping_add_signed(s.objects.defs.dx(d));
        let ny = oy.wrapping_add_signed(s.objects.defs.dy(d));
        if seams::push_blocked(&mut s, sel, ox, oy, dir as u16) {
            true
        } else {
            let z = u16::from(s.map.centre.z);
            crate::objects::move_to(&mut s, sel, nx, ny, z);
            if kind == CANNON {
                s.objects.type_frame[sel as usize] = kind + ((((dir >> 1) & 3) as u16) << 10);
            }
            crate::map::view::update_view(&mut s);
            false
        }
    };
    if blocked {
        say(state, io, FMT_NL_S_NL, Msg::Blocked).await;
    }
}

/// The end of Pickpocket: the panel off the victim (A4+0x1380 / +0x1382 = 0).
fn close_victim(s: &mut State) {
    s.ui.panel.open_container = 0;
    s.ui.panel.container_flag = false;
}

/// Original: 0x119266 (magic: Pickpocket: take an object from an actor at a distance)
///
/// "On whom: " for a member caster, the target read unless chosen (the cursor mode A4-0x7a60
/// kept); its name or "nothing"; the caster flashes and the missile 0x18b flies. Not an actor
/// slot, or no location: "No effect"; short: "Blocked". The victim's inventory opens in the
/// panel (A4+0x1380, 0x111c30, mode 0x92, A4+0x1820 set while it is read), "Which object: " and
/// one key (the panel cursor on when a cursor mode was on). Not a confirmed pick: "nothing";
/// its name otherwise. Not a carried object (status 0x18 clear): "No effect". What taking it
/// meets first (0x1122dc) may stop it. Heavier than the caster may carry (0x1110f6 + the
/// weights 0x11121e leaves, against strength × 20): "Can't carry!". Else it flies to the
/// caster (0x1228ba with its tile), leaves its effect on the victim (0x11236c), a stack moves
/// by its count (0x10cf7c / 0x10d16e), anything else into the caster's inventory (0x10cda8);
/// the caster's inventory is shown (0x111be0), "Success" and karma − 5. Then the panel goes
/// back to the leader.
pub async fn pickpocket(state: &RefCell<State>, io: &Io, pre: i16) {
    let (caster, member) = caster_member(&state.borrow());
    if member {
        say(state, io, FMT_S, Msg::OnWhom).await;
    }
    if pre == 0 {
        let mode = {
            let mut s = state.borrow_mut();
            s.ui.input.target_pending = 1;
            s.ui.input.targeting
        };
        crate::ui::wait::read_key(state, io).await;
        state.borrow_mut().ui.input.targeting = mode;
    }
    let none = missing(&state.borrow());
    name_or_nothing(state, io, none).await;
    seams::flash_actor(&mut state.borrow_mut(), io, caster, 0);
    let reached = shoot(state, io, caster, 0x18b).await;
    let victim = state.borrow().ui.input.selection;
    if missing(&state.borrow()) || victim >= 0x100 {
        say(state, io, FMT_NL_S_NL, Msg::NoEffect).await;
        return;
    }
    if !reached {
        say(state, io, FMT_NL_S_NL, Msg::Blocked).await;
        return;
    }
    let home = {
        let mut s = state.borrow_mut();
        s.ui.panel.no_rows = true;
        s.ui.panel.open_container = victim as u16;
        (
            s.objects.x(caster as Slot) as i16,
            s.objects.y(caster as Slot) as i16,
        )
    };
    crate::ui::panels::inventory(state, io).await;
    state.borrow_mut().ui.panel.mode = 0x92;
    print_at(state, io, MSG_WHICH, &[]).await;
    if state.borrow().ui.input.targeting != 0 {
        state.borrow_mut().ui.input.targeting = 2;
        crate::ui::cursor::panel_cursor(state, io, 1);
    }
    let key = crate::ui::wait::read_key(state, io).await;
    state.borrow_mut().ui.panel.no_rows = false;
    let obj = state.borrow().ui.input.selection;
    name_or_nothing(state, io, key != 0x8e || obj == -1).await;
    let carried = obj >= 0x100 && state.borrow().objects.status(obj) & 0x18 != 0;
    if !carried {
        say(state, io, FMT_NL_S_NL, Msg::NoEffect).await;
        close_victim(&mut state.borrow_mut());
        return;
    }
    if seams::unready_check(state, io, obj, victim).await != 0 {
        close_victim(&mut state.borrow_mut());
        return;
    }
    let too_heavy = {
        let mut s = state.borrow_mut();
        let (readied, packed) = seams::carried_weights(&mut s, caster);
        let w = seams::object_weight(&s, obj)
            .wrapping_add(packed)
            .wrapping_add(readied);
        w > i16::from(s.actors.attrs.strength[usize::from(caster)]) * 0x14
    };
    if too_heavy {
        print_at(state, io, MSG_CANT_CARRY, &[]).await;
    } else {
        let (kind, tile) = {
            let s = state.borrow();
            (s.objects.kind(obj), super::effects::tile_of(&s, obj) as u16)
        };
        let mut to = home;
        seams::missile(state, io, victim as u16, 8, &mut to, tile).await;
        {
            let mut s = state.borrow_mut();
            seams::release_effect(&mut s, obj, victim);
            if crate::objects::stack_kind(&s.objects, kind) != 0 {
                let qq = s.objects.qq_of(obj);
                crate::objects::add_to_owner(&mut s, caster as Slot, kind, qq);
                crate::objects::take_from_owner(&mut s, victim, kind, qq);
            } else {
                crate::objects::move_into(&mut s, obj, caster as Slot, 0x10);
            }
            seams::inventory_of(&mut s, caster);
        }
        say(state, io, FMT_NL_S_NL, Msg::Success).await;
        let mut s = state.borrow_mut();
        s.actors.karma = s.actors.karma.wrapping_sub(5);
    }
    let mut s = state.borrow_mut();
    let p = &mut s.actors.party;
    let leader = i16::from(p.leader_index);
    s.ui.panel.member = leader;
    s.ui.panel.inventory_first = 0;
    s.ui.panel.dirty = true;
    close_victim(&mut s);
}

/// Original: 0x119876 (magic: Enchant: a wand takes a spell from the caster's book)
///
/// The leader's inventory in the panel (mode 0x92, 0x111c30), "On what: " and the target read
/// unless chosen; its name or "nothing". Not a wand (0x4e): "No effect" (the original's test of
/// a readied target never holds). A wand holding 10 objects: "It's full". Otherwise the caster's
/// readied books (the second found or any later one as the second book) open (0x115da4,
/// 0x116028, 0x116218, panel mode 0xa1), "With: " and one key; the caster flashes. Not a
/// confirmed line: "none"; else the line's spell is spoken (its syllables) and the wand gets a
/// spell object (0x150) of that quality (0x10cf7c). A newline ends it.
pub async fn enchant(state: &RefCell<State>, io: &Io, pre: i16) {
    state.borrow_mut().ui.panel.mode = 0x92;
    crate::ui::panels::inventory(state, io).await;
    say(state, io, FMT_S, Msg::OnWhat).await;
    if pre == 0 {
        state.borrow_mut().ui.input.target_pending = 1;
        crate::ui::wait::read_key(state, io).await;
    }
    let wand = state.borrow().ui.input.selection;
    name_or_nothing(state, io, wand == -1).await;
    if state.borrow().objects.kind(wand) != WAND {
        say(state, io, FMT_NL_S_NL, Msg::NoEffect).await;
        return;
    }
    let full = {
        let mut s = state.borrow_mut();
        let mut n = 0;
        let mut o = crate::objects::first_child(&mut s.objects, wand);
        while o >= 0 {
            n += 1;
            o = crate::objects::next_child(&mut s.objects);
        }
        n == 10
    };
    if full {
        print_at(state, io, MSG_FULL, &[]).await;
        return;
    }
    {
        let mut s = state.borrow_mut();
        let caster = super::cast::caster(&s) as Slot;
        let mut books = [0i16; 2];
        let mut o = crate::objects::first_child(&mut s.objects, caster);
        while o >= 0 {
            if s.objects.status(o) & 0x18 == 0x18 && s.objects.kind(o) == super::SPELLBOOK {
                if books[0] == 0 {
                    books[0] = o;
                } else {
                    books[1] = o;
                }
            }
            o = crate::objects::next_child(&mut s.objects);
        }
        s.magic.books = books;
        super::book::load_book(&mut s);
        super::book::draw_book(&mut s, io);
    }
    super::book::draw_book_page(state, io).await;
    state.borrow_mut().ui.panel.mode = super::BOOK_PANEL;
    print_at(state, io, MSG_WITH, &[]).await;
    let key = crate::ui::wait::read_key(state, io).await;
    let caster = super::cast::caster(&state.borrow());
    seams::flash_actor(&mut state.borrow_mut(), io, caster, 0);
    let line = state.borrow().ui.input.selection;
    if key & 0xff != crate::ui::wait::REPEAT || line == -1 {
        print_at(state, io, MSG_NONE, &[]).await;
    } else {
        let (spell, word) = {
            let s = state.borrow();
            let spell = usize::try_from(line)
                .ok()
                .and_then(|i| s.magic.page.get(i))
                .copied()
                .unwrap_or(0);
            (spell, s.magic.tables.word(usize::from(spell)).to_vec())
        };
        let first = word.first().copied().unwrap_or(0);
        for (i, c) in std::iter::once(first)
            .chain(word.iter().skip(1).copied())
            .enumerate()
        {
            let syl = state.borrow().magic.tables.syllable(c).to_vec();
            let fmt = if i == 0 { FMT_S } else { FMT_SPACE_S };
            print_at(state, io, fmt, &[Arg::Str(&syl)]).await;
        }
        crate::objects::add_to_owner(
            &mut state.borrow_mut(),
            wand,
            SPELL_OBJECT,
            u16::from(spell) << 8 | 1,
        );
    }
    print_at(state, io, NEWLINE, &[]).await;
}

/// Original: 0x119dbc (magic: Seance: talk with the dead)
///
/// "On whom: " and the target read unless chosen; a newline; the caster flashes. Not a body
/// (0x153, 0x162, 0x155, 0x154): "No effect". Else its name, and the leader talks with the
/// person the body's quality names (0x1125b8) with A4+0x1821 set around the conversation.
pub async fn seance(state: &RefCell<State>, io: &Io, pre: i16) {
    say(state, io, FMT_S, Msg::OnWhom).await;
    if pre == 0 {
        state.borrow_mut().ui.input.target_pending = 1;
        crate::ui::wait::read_key(state, io).await;
    }
    print_at(state, io, NEWLINE, &[]).await;
    let caster = super::cast::caster(&state.borrow());
    seams::flash_actor(&mut state.borrow_mut(), io, caster, 0);
    let (sel, kind) = {
        let s = state.borrow();
        let sel = s.ui.input.selection;
        (sel, s.objects.kind(sel))
    };
    if sel == -1 || !matches!(kind, 0x153 | 0x162 | 0x155 | 0x154) {
        say(state, io, FMT_NL_S_NL, Msg::NoEffect).await;
        return;
    }
    print_name(state, io, sel).await;
    print_at(state, io, NEWLINE, &[]).await;
    let (talker, whom) = {
        let mut s = state.borrow_mut();
        s.converse.by_spell = true;
        (
            s.actors.party.leader_index,
            super::book::quality(&s.objects, sel),
        )
    };
    let cx = crate::converse::Cx::new(state, io);
    Box::pin(crate::converse::begin(&cx, talker, i16::from(whom))).await;
    state.borrow_mut().converse.by_spell = false;
}
