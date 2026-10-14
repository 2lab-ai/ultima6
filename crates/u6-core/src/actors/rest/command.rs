//! The Rest command 0x127b1a.

use std::cell::RefCell;

use super::*;
use crate::objects::{build_draw_lists, create_from_record, display_name};

/// Original: 0x127b1a (party: the Rest command: camp checks, hull repair aboard ship, the
/// hours, the guard, the camp, the meal, the sleep with ambush rolls, the healing)
///
/// Prints the command's name (0x1272cc), then refuses in combat, aboard anything but a ship,
/// outside the wilderness, with foes or others near, in solo mode or without room. Aboard ship
/// the hull is repaired. Otherwise (nobody on a horse) it asks the hours (`1`..`9`) and, for a
/// party of more than one, the guard (the cross-hair when the command came with one, else a
/// digit `1`.. below `0` + the party size picks a member, so never the last one). A campfire
/// (type 0xfd, frame 1) is lit at the centre and the members walk to their camp places (20
/// rounds at most). Mealtime: a bard (0x182) plays (0x188 while the meal lasts); the meal goes
/// on while A4-0x77a0 is set (four frames per member), and nothing in `game` sets it, so only
/// the first member eats; the first without food bounds the healing. Everybody but the guard
/// lies down (combat 0x1221da). Each ten minutes of the hours: the guard's turn (worktype
/// 0x0c), the clock, the view; every step whose number has bits 0 and 2 clear rolls 1..=64,
/// and a 1 is an ambush (0x12783c): with a guard the party hears it (0x127422) and wakes,
/// without one it stays asleep. The fire goes out; members take their types back (0x139688)
/// and their movement points (dexterity).
/// Four hours or more with the rest counter at 0 and no ambush heal every member before the
/// first hungry one (0x1279d4, announced), the counter becomes 12. The view centre moves to the
/// leader.
pub async fn rest(state: &RefCell<State>, io: &Io) {
    say(state, io, STR_REST, &[]).await;
    let targeting = state.borrow().ui.input.targeting;
    let refused = refusal(&mut state.borrow_mut());
    if let Some(msg) = refused {
        say(state, io, msg, &[]).await;
        return;
    }
    if aboard(&state.borrow()) {
        repair_hull(state, io).await;
        return;
    }
    let mounted = {
        let s = state.borrow();
        (0..size(&s)).any(|i| s.objects.kind(member(&s, i)) == HORSE_RIDER)
    };
    if mounted {
        say(state, io, STR_DISMOUNT, &[]).await;
        return;
    }
    say(state, io, STR_HOURS, &[]).await;
    let key = crate::ui::wait::read_key(state, io).await;
    if !(0x31..=0x39).contains(&key) {
        say(state, io, STR_ZERO, &[]).await;
        return;
    }
    say(state, io, STR_KEY, &[Arg::Word(key)]).await;
    let hours = key - 0x30;
    let guard = if size(&state.borrow()) != 1 {
        choose_guard(state, io, targeting).await
    } else {
        NO_GUARD
    };
    let fire = make_camp(state, io);
    say(state, io, STR_MEAL, &[]).await;
    let bard = start_tune(&mut state.borrow_mut());
    if let Some(bard) = bard {
        say_name(state, io, bard).await;
        say(state, io, STR_TUNE, &[]).await;
    }
    build_draw_lists(&mut state.borrow_mut());
    let hungry = meal(state, io).await;
    if state.borrow().ui.panel.mode == 0x92 {
        crate::inventory::draw_list(state, io).await;
    }
    end_tune(&mut state.borrow_mut());
    let n = size(&state.borrow());
    for i in 0..n {
        let m = member(&state.borrow(), i);
        if m == guard {
            say(state, io, STR_NEWLINE, &[]).await;
            say_name(state, io, i).await;
            say(state, io, STR_STANDS_GUARD, &[]).await;
        } else {
            lie_down(&mut state.borrow_mut(), m);
        }
    }
    build_draw_lists(&mut state.borrow_mut());
    let ambushed = sleep(state, io, guard, hours).await;
    wake(state, io, guard, hours, hungry, ambushed, fire).await;
}

/// The guard question (0x1273aa): the guard chosen with the cross-hair or a digit, `NO_GUARD`
/// for none.
async fn choose_guard(state: &RefCell<State>, io: &Io, targeting: u8) -> Slot {
    say(state, io, STR_GUARD, &[]).await;
    state.borrow_mut().ui.input.target_pending = 1;
    if targeting != 0 {
        state.borrow_mut().ui.input.range = 7;
        crate::ui::cursor::panel_cursor(state, io, 0);
        state.borrow_mut().ui.input.targeting = 1;
    }
    let mut key = crate::ui::wait::read_key(state, io).await;
    {
        let mut s = state.borrow_mut();
        let n = s.actors.party.size;
        // 0x127ce2..0x127cee compares with '0' + size (not '1' + size, as the cross-hair's
        // digits and the command loop's do): the last member cannot be picked by digit.
        if targeting == 0 && key >= 0x31 && key < u16::from(n) + 0x30 {
            // A4+0x47bb + key: the roster byte of member key - '1'.
            let sel = member(&s, usize::from(key - 0x31));
            let x = s.objects.location[sel as usize].x as i16;
            let input = &mut s.ui.input;
            input.selection = sel;
            input.target.0 = x;
            key = TARGET_DONE;
        }
        s.ui.input.targeting = 0;
        crate::gfx::refresh_view(&mut s, io);
    }
    let (sel, tx) = {
        let s = state.borrow();
        (s.ui.input.selection, s.ui.input.target.0)
    };
    let is_member = |s: &State| s.actors.attrs.status[sel as usize & 0xff] & 0x80 != 0;
    if key == TARGET_DONE && sel != -1 && tx != -1 && sel < 0x100 && is_member(&state.borrow()) {
        let name = display_name(&state.borrow(), sel);
        printf(state, io, name.as_bytes(), &[]).await;
        say(state, io, STR_NEWLINE, &[]).await;
        sel
    } else {
        say(state, io, STR_NONE, &[]).await;
        NO_GUARD
    }
}

/// The campfire at the centre (temporary), then up to 20 rounds of the members walking to their
/// camp places (0x11f452), each facing its way once there (0x11ba5e); the fire's slot.
fn make_camp(state: &RefCell<State>, io: &Io) -> Slot {
    let mut s = state.borrow_mut();
    let c = s.map.centre;
    let [b0, b1, b2] = pack_xyz(c.x, c.y, u16::from(c.z)).to_packed();
    let [t0, t1] = (1u16 << 10 | CAMPFIRE).to_be_bytes();
    let fire = create_from_record(&mut s, [0, b0, b1, b2, t0, t1, 0, 1]);
    // The original sets bit 5 at slot -1 when no slot was free; the port skips it.
    if let Some(st) = usize::try_from(fire)
        .ok()
        .and_then(|f| s.objects.obj_status.get_mut(f))
    {
        *st |= crate::objects::TEMPORARY;
    }
    build_draw_lists(&mut s);
    let (mut moving, mut tries) = (true, 20);
    while moving && tries != 0 {
        tries -= 1;
        moving = false;
        for i in 0..size(&s) {
            let m = member(&s, i) as u16;
            let tx = c.x.wrapping_add_signed(i16::from(camp(&s, i)));
            let ty = c.y.wrapping_add_signed(i16::from(camp(&s, 8 + i)));
            if crate::actors::npc::approach(&mut s, m, tx, ty) {
                moving = true;
            } else {
                let dir = camp(&s, 16 + i) as i16 as u16;
                face(&mut s, m, dir);
            }
        }
        build_draw_lists(&mut s);
        crate::gfx::refresh_view(&mut s, io);
    }
    fire
}

/// The first bard of the roster starts playing (type 0x182 → 0x188, facing its camp way);
/// its member index.
fn start_tune(s: &mut State) -> Option<usize> {
    retype(s, BARD, BARD_PLAYING)
}

/// The playing bard stops (0x188 → 0x182).
fn end_tune(s: &mut State) {
    retype(s, BARD_PLAYING, BARD);
}

fn retype(s: &mut State, from: u16, to: u16) -> Option<usize> {
    let i = (0..size(s)).find(|&i| s.objects.kind(member(s, i)) == from)?;
    let m = member(s, i) as usize;
    let tf = &mut s.objects.type_frame[m];
    *tf = (*tf & 0xfc00).wrapping_add(to);
    let dir = camp(s, 16 + i) as i16 as u16;
    face(s, m as u16, dir);
    Some(i)
}

/// Mealtime: one frame per round while A4-0x77a0 is set (and the first round), member `i / 4`
/// eating at every fourth; the index of the first member without food, else the party size.
///
/// Only the first round runs: the byte's only accesses in `game` are the clears at 0x10ed24
/// (0x10e81e, an object 0x188 coming into view) and 0x127f10 and the test at 0x127f1a; no
/// absolute, `lea` or `pea` reference to it exists, and the data hunk starts it at 0. So the
/// port, like the original, feeds member 0 alone.
async fn meal(state: &RefCell<State>, io: &Io) -> usize {
    state.borrow_mut().objects.draw.anim_188[0] = 0;
    let mut hungry = None;
    let mut i = 0usize;
    while state.borrow().objects.draw.anim_188[0] != 0 || i == 0 {
        let m = i >> 2;
        if i & 3 == 0 && m < size(&state.borrow()) {
            say_name(state, io, m).await;
            let fed = member_eats(&mut state.borrow_mut(), m);
            if fed {
                say(state, io, STR_FOOD, &[]).await;
            } else {
                say(state, io, STR_NO_FOOD, &[]).await;
                hungry.get_or_insert(m);
            }
        }
        camp_frame(&mut state.borrow_mut(), io);
        i += 1;
    }
    hungry.unwrap_or_else(|| size(&state.borrow()))
}

/// The hours: six steps of ten minutes each; the result is 0, or the ambush outcome (1 = the
/// guard heard it, 2 = no guard).
async fn sleep(state: &RefCell<State>, io: &Io, guard: Slot, hours: u16) -> u8 {
    let saved = (guard < 0x100).then(|| {
        let w = &mut state.borrow_mut().actors.worktype[guard as usize];
        std::mem::replace(w, GUARDING)
    });
    let mut ambushed = 0;
    for step in 0..hours * 6 {
        if guard < 0x100 {
            crate::actors::npc::npc_turn(state, io, guard as u16).await;
        }
        crate::actors::clock::tick(&mut state.borrow_mut(), 10);
        flush_prints(state, io).await;
        let attacked = {
            let mut s = state.borrow_mut();
            build_draw_lists(&mut s);
            camp_frame(&mut s, io);
            step & 5 == 0 && s.rng.range(1, 0x40) == 1 && ambush(&mut s)
        };
        if attacked {
            if guard < 0x100 {
                say(state, io, STR_HEAR, &[]).await;
                ambushed = 1;
            } else {
                ambushed = 2;
            }
            break;
        }
    }
    if let Some(w) = saved {
        state.borrow_mut().actors.worktype[guard as usize] = w;
    }
    ambushed
}

/// After the hours: the fire out, the members up (or left asleep by an unguarded ambush), the
/// healing, the view on the leader.
async fn wake(
    state: &RefCell<State>,
    io: &Io,
    guard: Slot,
    hours: u16,
    hungry: usize,
    ambushed: u8,
    fire: Slot,
) {
    {
        let mut s = state.borrow_mut();
        if let Some(tf) = usize::try_from(fire)
            .ok()
            .and_then(|f| s.objects.type_frame.get_mut(f))
        {
            *tf &= 0x3ff;
        }
        build_draw_lists(&mut s);
        for i in 0..size(&s) {
            let m = member(&s, i) as usize;
            if ambushed < 2 {
                if m as Slot != guard {
                    s.objects.type_frame[m] = s.actors.attrs.base_type_frame[m];
                }
            } else {
                s.actors.attrs.status[m] |= 0x04;
            }
            s.actors.movement_points[m] = s.actors.attrs.dexterity[m];
        }
        if hours >= 4 && s.actors.rest_counter == 0 && ambushed == 0 {
            for i in 0..hungry {
                if member(&s, i) != guard {
                    crate::actors::party::heal(&mut s, i, true);
                }
            }
            s.actors.rest_counter = 0x0c;
        }
    }
    flush_prints(state, io).await;
    let mut s = state.borrow_mut();
    let l = leader(&s) as usize;
    let p = s.objects.location[l];
    s.map.centre.x = p.x;
    s.map.centre.y = p.y;
    build_draw_lists(&mut s);
    crate::map::region::refresh_region(&mut s);
    crate::gfx::refresh_view(&mut s, io);
    if s.ui.panel.mode == 0x91 {
        s.ui.panel.dirty = true;
    }
}
