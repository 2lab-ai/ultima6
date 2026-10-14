//! Object actions of hunk 15 that the commands and the Use handlers share: doors and chests
//! (0x12bbc0, 0x12bdfa), searching a spot (0x129544 with 0x129438 / 0x12948a), traps
//! (0x12ba36), the name prefixes (0x12904c, 0x129382).
//!
//! These are synchronous, as their callers in the Use and spell code are: their text is queued
//! for the command loop in the order the original prints it.

use super::seams;
use super::text::{queue, queue_at};
use crate::main_loop::PrintArg;
use crate::objects::iter::{begin_at, find_child, first_child, next_at, next_child, tile_of};
use crate::objects::moves::{move_to, remove};
use crate::objects::{ACTOR_SLOTS, Slot, party_member, stack_kind};
use crate::state::State;

/// Strings of hunk 15 (image addresses).
pub(super) const S_LINE: u32 = 0x0012_83de; // "%s\n"
const S_A: u32 = 0x0012_83e2;
const S_AN: u32 = 0x0012_83e6;
const S_THE: u32 = 0x0012_83ea;
const S_COUNT: u32 = 0x0012_83fe; // "%d "
const S_SEARCHING: u32 = 0x0012_8402;
const S_SECRET_DOOR: u32 = 0x0012_841e;
const S_COMMA: u32 = 0x0012_842c;
const S_AND: u32 = 0x0012_8430;
const S_NOTHING_DOT: u32 = 0x0012_8436;
const S_DOT: u32 = 0x0012_8440;
const S_ACID: u32 = 0x0012_87d6;
const S_POISON: u32 = 0x0012_87de;
const S_BOMB: u32 = 0x0012_87e8;
const S_GAS: u32 = 0x0012_87f0;
const S_CLOSED: u32 = 0x0012_87f6;
const S_OPEN_DOOR_LOCK: u32 = 0x0012_8800;
const S_LOCKED: u32 = 0x0012_8820;
const S_MAGIC_LOCKED: u32 = 0x0012_882a;
const S_OPENED: u32 = 0x0012_883e;
const S_UNLOCKED: u32 = 0x0012_8848;
const S_LOCKED_PLAIN: u32 = 0x0012_8854;
/// Original: A4-0x6c64 (pointer to "No effect" in the message table at A4-0x6c6c)
pub(super) const P_NO_EFFECT: i32 = -0x6c64;

/// The trap object (type 0x151, quality 0x16) a container may hold.
const TRAP: u16 = 0x151;
const TRAP_QUALITY: i16 = 0x16;
/// Secret door, spell, trap.
const SECRET_DOOR: u16 = 0x14e;
const SPELL: u16 = 0x150;

fn set_frame(s: &mut State, slot: Slot, frame: u16) {
    if let Some(t) = s.objects.type_frame.get_mut(slot as usize) {
        *t = (*t & 0x3ff).wrapping_add(frame << 10);
    }
}

fn frame(s: &State, slot: Slot) -> u16 {
    s.objects.tf(slot) >> 10
}

fn line(s: &mut State, addr: u32) {
    queue_at(s, addr, Vec::new());
}

/// `printf("%s\n", A4-0x6c64)`: "No effect".
fn no_effect(s: &mut State) {
    let msg = s.objcmds.data.pointed(super::GameData::a4(P_NO_EFFECT));
    queue_at(s, S_LINE, vec![PrintArg::Str(msg)]);
}

fn leader_z(s: &State) -> u16 {
    u16::from(s.map.centre.z)
}

/// Original: 0x12bbc0 (objects: open, close, lock or unlock door `door`)
///
/// A newline, then by the door's frame: 0..3 open (closes it, "closed!", unless a key or spell
/// is given: "Can't (Un)lock an opened door"), 4..7 closed (`key`: "locked!", `lock` (the lock
/// spell): "magically locked!", else it opens: "opened!", or its trap (0x151 quality 0x16)
/// springs), 8..11 locked (`key` unlocks, `lock` locks magically, else "locked"), 12.. magically
/// locked (only `unlock`, the spell, opens it). The frame changes by xor so the door keeps its
/// half (bits 0-1).
pub fn door(s: &mut State, door: Slot, key: bool, lock: bool, unlock: bool) {
    queue(s, b"\n".to_vec(), Vec::new());
    let f = frame(s, door);
    match f {
        0..=3 => {
            if !key && !lock && !unlock {
                line(s, S_CLOSED);
                set_frame(s, door, f ^ 4);
            } else {
                line(s, S_OPEN_DOOR_LOCK);
            }
        }
        4..=7 => {
            if key {
                line(s, S_LOCKED);
                set_frame(s, door, f ^ 0xc);
            } else if lock {
                line(s, S_MAGIC_LOCKED);
                set_frame(s, door, f ^ 8);
            } else {
                if find_child(&mut s.objects, door, TRAP as i16, TRAP_QUALITY) != -1 {
                    spring_trap(s, door);
                } else {
                    line(s, S_OPENED);
                }
                set_frame(s, door, f ^ 4);
            }
        }
        8..=11 => {
            if key {
                line(s, S_UNLOCKED);
                set_frame(s, door, f ^ 0xc);
            } else if lock {
                line(s, S_MAGIC_LOCKED);
                set_frame(s, door, f ^ 4);
            } else {
                line(s, S_LOCKED_PLAIN);
            }
        }
        _ => {
            if unlock {
                line(s, S_UNLOCKED);
                set_frame(s, door, f ^ 8);
            } else {
                line(s, S_MAGIC_LOCKED);
            }
        }
    }
}

/// Original: 0x12bdfa (objects: open, close, lock or unlock chest `chest`)
///
/// A newline, then by frame: 0 open (closes, "closed!"; a key or spell: "No effect"), 1 closed
/// (opens: "opened!" or its trap springs, then its contents spill (0x129544); `key` locks,
/// `lock` locks magically, `unlock`: "No effect"), 2 locked (`key` unlocks to 1, `lock` to 3,
/// else "No effect"), 3 magically locked (`unlock` opens it to 1, else "No effect"). A chest in
/// a container or inventory redraws its panel (0x1118c6).
pub fn chest(s: &mut State, chest: Slot, key: bool, lock: bool, unlock: bool) {
    queue(s, b"\n".to_vec(), Vec::new());
    match frame(s, chest) {
        0 => {
            if key || lock || unlock {
                no_effect(s);
            } else {
                line(s, S_CLOSED);
                set_frame(s, chest, 1);
            }
        }
        1 => {
            if !key && !lock && !unlock {
                if find_child(&mut s.objects, chest, TRAP as i16, TRAP_QUALITY) != -1 {
                    spring_trap(s, chest);
                } else {
                    line(s, S_OPENED);
                }
                set_frame(s, chest, 0);
                let (x, y) = (s.objects.x(chest), s.objects.y(chest));
                search(s, chest, x, y);
            } else if key {
                line(s, S_LOCKED);
                set_frame(s, chest, 2);
            } else if lock {
                line(s, S_MAGIC_LOCKED);
                set_frame(s, chest, 3);
            } else {
                no_effect(s);
            }
        }
        2 => {
            if key {
                line(s, S_UNLOCKED);
                set_frame(s, chest, 1);
            } else if lock {
                line(s, S_MAGIC_LOCKED);
                set_frame(s, chest, 3);
            } else {
                no_effect(s);
            }
        }
        _ => {
            if unlock {
                line(s, S_UNLOCKED);
                set_frame(s, chest, 1);
            } else {
                no_effect(s);
            }
        }
    }
    if s.objects.place(chest) != 0 {
        seams::redraw_inventory(s);
    }
}

/// Original: 0x129438 (objects: a closed container (chest 0x62, 0xc0 or 0xba with a frame
/// other than 0) or type 0x39)
pub fn is_closed_container(s: &State, slot: Slot) -> bool {
    let t = s.objects.kind(slot);
    let f = frame(s, slot);
    (matches!(t, 0x62 | 0xc0 | 0xba) && f != 0) || t == 0x39
}

/// Original: 0x12948a (objects: the object below cover `slot` at its tile: the first other
/// non-actor whose tile lacks flags3 bit 4, -1)
pub fn object_under(s: &mut State, slot: Slot) -> Slot {
    let (x, y, z) = (s.objects.x(slot), s.objects.y(slot), leader_z(s));
    let mut c = begin_at(&mut s.objects, &s.tables, x, y, z);
    while c >= 0 {
        let cover = flags3(s, tile_of(&s.tables, s.objects.tf(c))) & 0x10 != 0;
        if c != slot && !cover && c > 0xff {
            return c;
        }
        c = next_at(&mut s.objects, &s.tables);
    }
    c
}

fn flags3(s: &State, tile: u16) -> u8 {
    s.tables
        .tileflags
        .flags3
        .get(usize::from(tile))
        .copied()
        .unwrap_or(0)
}

fn flags1(s: &State, tile: u16) -> u8 {
    s.tables
        .tileflags
        .flags1
        .get(usize::from(tile))
        .copied()
        .unwrap_or(0)
}

/// Original: 0x129544 (objects: search `obj`, standing next to (x, y): find a secret door or
/// spill what the object holds onto its tile)
///
/// Only within one tile of the leader, for a non-actor whose tile has no flags1 bit 3 and that
/// is not type 0x39. Prints "Searching here, you find ", then: a closed secret door (0x14e,
/// frame bit 0 clear) opens ("a secret door"); else (past a cover, flags3 bit 4, to the object
/// below it, 0x12948a) unless it is a closed container (0x129438), every object inside but
/// spells and traps moves to the object's tile with the leader re-placed on top, the view
/// refreshed after each, and is named ("a, b and c"); "nothing." when none.
pub fn search(s: &mut State, obj: Slot, x: u16, y: u16) {
    let leader = Slot::from(s.actors.party.members[usize::from(s.actors.party.leader_index).min(15)]);
    let (lx, ly) = (s.objects.x(leader), s.objects.y(leader));
    let mut kind = s.objects.kind(obj);
    let mut f = frame(s, obj);
    let mut found = false;
    if (x as i16).wrapping_sub(lx as i16).wrapping_abs() > 1
        || (y as i16).wrapping_sub(ly as i16).wrapping_abs() > 1
        || obj < ACTOR_SLOTS as Slot
        || flags1(s, tile_of(&s.tables, s.objects.tf(obj))) & 0x08 != 0
        || kind == 0x39
    {
        return;
    }
    line(s, S_SEARCHING);
    let mut obj = obj;
    if kind == SECRET_DOOR {
        if f & 1 == 0 {
            line(s, S_SECRET_DOOR);
            set_frame(s, obj, f ^ 1);
            refresh(s);
            found = true;
        }
    } else {
        if flags3(s, tile_of(&s.tables, s.objects.tf(obj))) & 0x10 != 0 {
            obj = object_under(s, obj);
            kind = s.objects.kind(obj);
            f = frame(s, obj);
        }
        let _ = (kind, f);
        if !is_closed_container(s, obj) {
            let mut c = first_child(&mut s.objects, obj);
            while c >= 0 {
                let t = s.objects.kind(c);
                if t == SPELL || t == TRAP {
                    c = next_child(&mut s.objects);
                    continue;
                }
                let z = leader_z(s);
                let (ox, oy) = (s.objects.x(obj), s.objects.y(obj));
                move_to(s, c, ox, oy, z);
                move_to(s, leader, lx, ly, z);
                refresh(s);
                let mut d = first_child(&mut s.objects, obj);
                while d >= 0 && matches!(s.objects.kind(d), SPELL | TRAP) {
                    d = next_child(&mut s.objects);
                }
                if found && d >= 0 {
                    line(s, S_COMMA);
                } else if found && d == -1 {
                    line(s, S_AND);
                }
                name_prefix(s, c);
                let name = crate::objects::display_name(s, c).into_bytes();
                queue(s, name, Vec::new());
                found = true;
                c = d;
            }
        }
    }
    line(s, if found { S_DOT } else { S_NOTHING_DOT });
}

/// 0x109e74 then 0x102ef0, the redraw the search does after each change.
fn refresh(s: &mut State) {
    crate::map::view::update_view(s);
    crate::engine::with_current_io(|io| crate::gfx::refresh_view(s, io));
}

/// Original: 0x12904c (objects: print the article of `tile` from flags3 bits 6-7: "a ", "an ",
/// "the " or nothing)
pub fn article(s: &mut State, tile: u16) {
    let at = match flags3(s, tile) & 0xc0 {
        0x40 => S_A,
        0x80 => S_AN,
        0xc0 => S_THE,
        _ => return,
    };
    line(s, at);
}

/// Original: 0x129382 (objects: print the count or article in front of `slot`'s name; returns
/// the count)
///
/// Gold-like stacks (0x10cef2 kind 4) print their word, quantity stacks (kind 2) not lit
/// (status 0x80) their quantity byte, both through "%d "; anything else its tile's article and
/// counts 1.
pub fn name_prefix(s: &mut State, slot: Slot) -> u16 {
    let o = &s.objects;
    let n = match stack_kind(o, o.kind(slot)) {
        4 => o.qq_of(slot),
        2 if o.status(slot) & 0x80 == 0 => o.qq_of(slot) & 0xff,
        _ => {
            let tile = tile_of(&s.tables, o.tf(slot));
            article(s, tile);
            return 1;
        }
    };
    queue_at(s, S_COUNT, vec![PrintArg::Word(n)]);
    n
}

/// Original: 0x12ba36 (objects: spring the trap of `obj`: acid, poison, bomb or gas)
///
/// The victim is the leader for an object on the map (the blast at the object), the member the
/// panel shows for one carried (the blast at that member). One draw 0..=3: acid hurts the
/// victim 1..=20 (combat 0x1231be, then its wound text 0x12395a), poison poisons it (status
/// bit 3, 0x1230ce), a bomb (explosion tile 0x17e) hurts and a gas cloud (0x17c) poisons every
/// actor the blast (ui 0x1331b4) reaches. Then the trap object goes (0x116526 finds it,
/// 0x10c1d6) and an open stats or roster panel is redrawn.
pub fn spring_trap(s: &mut State, obj: Slot) {
    let (victim, bx, by);
    if s.objects.place(obj) == 0 {
        victim = party_member(s, usize::from(s.actors.party.leader_index));
        bx = crate::objects::member_x(s, obj);
        by = crate::objects::member_y(s, obj);
    } else {
        let m = usize::try_from(s.ui.panel.member).unwrap_or(0);
        victim = party_member(s, m);
        bx = crate::objects::member_x(s, Slot::from(victim));
        by = crate::objects::member_y(s, Slot::from(victim));
    }
    let victim = Slot::from(victim);
    match s.rng.range(0, 3) {
        0 => {
            line(s, S_ACID);
            let n = s.rng.range(1, 20);
            seams::hurt(s, victim, n as i16);
            queue(s, b"\n".to_vec(), Vec::new());
            seams::wound_text(s, victim);
        }
        1 => {
            line(s, S_POISON);
            s.actors.attrs.status[victim as usize] |= 0x08;
            seams::flash(s, victim);
        }
        2 => {
            line(s, S_BOMB);
            for a in seams::blast(s, bx, by, 0x17e) {
                if a < ACTOR_SLOTS as Slot {
                    let n = s.rng.range(1, 20);
                    seams::hurt(s, a, n as i16);
                    queue(s, b"\n".to_vec(), Vec::new());
                    seams::wound_text(s, a);
                }
            }
        }
        3 => {
            line(s, S_GAS);
            for a in seams::blast(s, bx, by, 0x17c) {
                if a < ACTOR_SLOTS as Slot {
                    s.actors.attrs.status[a as usize] |= 0x08;
                    seams::flash(s, a);
                }
            }
        }
        _ => {}
    }
    let trap = seams::carried(s, obj, TRAP, TRAP_QUALITY);
    remove(s, trap);
    if matches!(s.ui.panel.mode, 0x90 | 0x91) {
        s.ui.panel.dirty = true;
    }
}

/// "\nThe door is blown up!\n" (hunk 16).
const S_DOOR_BLOWN: u32 = 0x0012_ca12;
/// Powder keg and the four door types 0x12d9ee tries, in its order.
const KEG: u16 = 0xdf;
const DOORS: [u16; 4] = [0x12c, 0x129, 0x12a, 0x12b];
/// Explosion tile of a keg (0x12db7e).
const KEG_BLAST: u16 = 0x189;

/// Original: 0x12d9ee (objects: a blast at (x, y): light the kegs and blow up the doors near
/// it)
///
/// Columns x − 2..=x + 2, rows y − 2..y + 2 (the last row is not reached), on the leader's
/// level: a keg there takes frame 1, the view is refreshed and it explodes (0x12db7e). Then the
/// door of the first type of 0x12c, 0x129, 0x12a, 0x12b found that is not magically locked
/// (frame & 0xc ≠ 0xc) is removed, the view refreshed, "The door is blown up!".
pub fn blast_doors(s: &mut State, x: u16, y: u16) {
    let mut cx = (x as i16).wrapping_sub(2);
    while cx <= (x as i16).wrapping_add(2) {
        let mut cy = (y as i16).wrapping_sub(2);
        while cy < (y as i16).wrapping_add(2) {
            let z = leader_z(s);
            let (ux, uy) = (cx as u16, cy as u16);
            let keg = crate::objects::object_of_kind_at(s, ux, uy, z, KEG);
            if keg >= 0 {
                set_frame(s, keg, 1);
                refresh(s);
                explode_keg(s, keg);
            }
            let sealed = |s: &State, d: Slot| d == -1 || frame(s, d) & 0xc == 0xc;
            let mut d = crate::objects::object_of_kind_at(s, ux, uy, z, DOORS[0]);
            for &kind in &DOORS[1..] {
                if sealed(s, d) {
                    d = crate::objects::object_of_kind_at(s, ux, uy, z, kind);
                }
            }
            if d >= 0 && frame(s, d) & 0xc != 0xc {
                remove(s, d);
                refresh(s);
                line(s, S_DOOR_BLOWN);
            }
            cy = cy.wrapping_add(1);
        }
        cx = cx.wrapping_add(1);
    }
}

/// Original: 0x12db7e (objects: explode powder keg `keg`)
///
/// The blast is where the keg lies, or where the outermost object holding it lies. The keg is
/// removed, the explosion (tile 0x189, ui 0x1331b4) hurts everything it reaches that is not a
/// counted stack (0x10cef2 kind 0) by 1..=60 (combat 0x1231be), then the doors and kegs around
/// go (0x12d9ee) and the view is refreshed.
pub fn explode_keg(s: &mut State, keg: Slot) {
    let mut at = keg;
    while s.objects.place(at) != 0 {
        at = s.objects.parent(at);
    }
    let (x, y) = (s.objects.x(at), s.objects.y(at));
    remove(s, keg);
    for a in seams::blast(s, x, y, KEG_BLAST) {
        if stack_kind(&s.objects, s.objects.kind(a)) == 0 {
            let n = s.rng.range(1, 60);
            seams::hurt(s, a, n as i16);
        }
    }
    blast_doors(s, x, y);
    refresh(s);
}
