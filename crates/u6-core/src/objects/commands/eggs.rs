//! Eggs and the monsters they hatch: 0x1319b2, 0x131646, 0x13121a (hunk 17).
//!
//! An egg (type 0x14f) holds one template object per kind it hatches: the template's type is
//! the monster or object, its quantity the most it makes, its quality the worktype the monsters
//! get; template status bit 0x40 marks a monster with its extra parts drawn the other way. The
//! egg's quality chooses day-only (tens 1) or night-only (tens 2) hatching and an alignment
//! (units, 1..3 → alignment bits 5-6 of the monster), its quantity the hatch chance in percent
//! (100 also makes every template hatch its full count).
//!
//! Tables (data hunk, read from the user's `game` at the addresses below): the monster stat
//! bytes by monster index (0x131196 order), the equipment lists, the part offsets of a silver
//! serpent, the item types of the treasure, the direction names.

use super::seams;
use super::text;
use crate::main_loop::PrintArg;
use crate::objects::iter::{find_child, first_child, next_child};
use crate::objects::moves::{create_actor, create_in, create_on_map, remove};
use crate::objects::{ACTOR_SLOTS, Slot, monster_index, stack_kind, vary};
use crate::state::State;

/// Object types.
const SERPENT: u16 = 0x19d;
const HYDRA: u16 = 0x16d;
const HYDRA_HEAD: u16 = 0x16e;
const GARGOYLE: u16 = 0x16a;
const WINGED_GARGOYLE: u16 = 0x16b;
const DRAGON: u16 = 0x19b;
const CHEST: u16 = 0x62;
const AMULET: i16 = 0x4c;
const SPELL: u16 = 0x150;
const GOLD: u16 = 0x58;
const BAG: u8 = 0x3a;

/// Status bytes the new objects get (0x10be02 / 0x10bcee add 0x20, temporary).
const ST_SPELL: u8 = 0x38;
const ST_READIED: u8 = 0x39;
const ST_CARRIED: u8 = 0x31;
const ST_IN_CHEST: u8 = 0x29;

/// Original: A4-0x7f7e / -0x7f3e / -0x7efe / -0x7e3e / -0x7dfe (bytes by monster index:
/// strength, dexterity, intelligence, hit points, actor status)
const STR: i32 = -0x7f7e;
const DEX: i32 = -0x7f3e;
const INT: i32 = -0x7efe;
const HP: i32 = -0x7e3e;
const STATUS: i32 = -0x7dfe;
/// Original: A4-0x7d3e (word offsets by monster index into the equipment lists at A4-0x7cbe)
const EQUIP_OFFSETS: i32 = -0x7d3e;
const EQUIP_LISTS: i32 = -0x7cbe;
/// Original: A4-0x6590 (3 words: item types a treasure may hold)
const TREASURE_TYPES: i32 = -0x6590;
/// Original: A4-0x6582 / -0x657e (4 signed bytes each: dx / dy of serpent segment k & 3),
/// A4-0x658a (4 bytes: the frame a segment gets), A4-0x6586 (4 bytes: the tail's frame)
const SEG_DX: i32 = -0x6582;
const SEG_DY: i32 = -0x657e;
const SEG_FRAME: i32 = -0x658a;
const TAIL_FRAME: i32 = -0x6586;
/// Original: A4-0x763a (the warning format, a direction name for its `%s`) and A4-0x61be
/// (pointers to the 8 direction names)
const MSG_WARNING: i32 = -0x763a;
const DIR_NAMES: i32 = -0x61be;
/// The companion who warns (actor 3) and the status bits he needs.
const SCOUT: u16 = 3;

fn byte(s: &State, off: i32, i: i16) -> u8 {
    s.objcmds.data.a4_byte(off + i32::from(i))
}

fn sbyte(s: &State, off: i32, i: i16) -> i16 {
    i16::from(byte(s, off, i) as i8)
}

/// The quality (high byte) and quantity (low byte) of `slot`.
fn quality(s: &State, slot: Slot) -> u8 {
    (s.objects.qq_of(slot) >> 8) as u8
}

fn amount(s: &State, slot: Slot) -> u8 {
    s.objects.qq_of(slot) as u8
}

fn set_frame(s: &mut State, slot: Slot, frame: u16) {
    if let Some(t) = s.objects.type_frame.get_mut(slot as usize) {
        *t = (*t & 0x3ff).wrapping_add(frame << 10);
    }
}

fn status_or(s: &mut State, slot: Slot, bits: u8) {
    if let Some(b) = usize::try_from(slot)
        .ok()
        .and_then(|i| s.objects.obj_status.get_mut(i))
    {
        *b |= bits;
    }
}

fn status_clear(s: &mut State, slot: Slot, bits: u8) {
    if let Some(b) = usize::try_from(slot)
        .ok()
        .and_then(|i| s.objects.obj_status.get_mut(i))
    {
        *b &= !bits;
    }
}

fn leader_z(s: &State) -> u16 {
    u16::from(s.map.centre.z)
}

/// Original: 0x131646 (objects: create a monster of `kind` at (x, y, z) with stats, parts and
/// equipment; -1 when no temporary-actor slot is free)
///
/// `kind` bit 15 picks the other frames for a monster's extra parts (an egg template with
/// status bit 0x40). Stats come from the monster tables, each varied (0x1311d2); a type not in
/// the monster list gets 10 / 10 / 10, 30 hit points and status 0x20. Then the magic points
/// (0x122176), movement points = dexterity, experience 100, worktype and combat mode 8, the
/// frame of the shape and its extra parts (0x1aa.. two tiles, 0x19b four, 0x176 eight), the
/// equipment (0x13121a).
pub fn spawn_monster(s: &mut State, kind: u16, x: u16, y: u16, z: u16) -> Slot {
    let other_way = kind & 0x8000 != 0;
    let kind = kind & 0x7fff;
    let m = create_actor(s, kind, x, y, z);
    if m < 0 {
        return m;
    }
    let i = m as usize;
    let index = monster_index(&s.objects, kind);
    if index >= 0 {
        let v = |s: &mut State, off| {
            let n = byte(s, off, index);
            vary(s, i16::from(n)) as u8
        };
        s.actors.attrs.strength[i] = v(s, STR);
        s.actors.attrs.dexterity[i] = v(s, DEX);
        s.actors.attrs.intelligence[i] = v(s, INT);
        s.actors.attrs.hp[i] = v(s, HP);
        s.actors.attrs.level[i] = u16::from(s.actors.attrs.hp[i]).div_ceil(0x1e) as u8;
        s.actors.attrs.status[i] = byte(s, STATUS, index);
    } else {
        let a = &mut s.actors.attrs;
        a.intelligence[i] = 10;
        a.dexterity[i] = 10;
        a.strength[i] = 10;
        a.hp[i] = 0x1e;
        a.level[i] = 1;
        a.status[i] = 0x20;
    }
    s.actors.attrs.magic[i] = crate::actors::leaves::max_magic(s, m as u16) as u8;
    s.actors.movement_points[i] = s.actors.attrs.dexterity[i];
    s.actors.attrs.exp[i] = 100;
    s.actors.worktype[i] = 8;
    s.actors.attrs.combat_mode[i] = 8;
    if kind == GARGOYLE || kind == 0x1a8 {
        set_frame(s, m, 0x13);
    } else if kind >= 0x1aa {
        set_frame(s, m, 6);
        let tail = if other_way { 2 } else { 14 };
        let p = create_on_map(
            &mut s.objects,
            kind | tail << 10,
            1,
            0,
            x.wrapping_add(1),
            y,
            z,
        );
        status_clear(s, p, 0x20);
    } else if kind == DRAGON {
        set_frame(s, m, 0);
        let (xm, xp) = (x.wrapping_sub(1), x.wrapping_add(1));
        let (ym, yp) = (y.wrapping_sub(1), y.wrapping_add(1));
        let parts = [(8, x, ym), (16, x, yp), (24, xm, y), (32, xp, y)];
        for (frame, px, py) in parts {
            let p = create_on_map(&mut s.objects, DRAGON | frame << 10, 1, 0, px, py, z);
            status_clear(s, p, 0x20);
        }
    } else if kind == 0x176 {
        for d in 0..8u16 {
            let px = x.wrapping_add_signed(s.objects.defs.dx(usize::from(d)));
            let py = y.wrapping_add_signed(s.objects.defs.dy(usize::from(d)));
            let p = create_on_map(&mut s.objects, 0x1a9 | (d * 4) << 10, 1, 0, px, py, z);
            status_clear(s, p, 0x20);
        }
    }
    equip_monster(s, m, index);
    m
}

/// Original: 0x13121a (objects: give a new monster `m` its equipment from the A4-0x7cbe lists)
///
/// The monster's list (offset A4-0x7d3e + 2 × index) holds four 0-terminated byte lists: the
/// spells it casts (each a spell object 0x150, readied and hidden), readied equipment, more
/// readied items and the treasure. In each of the last three the n-th entry is given with
/// chance 1 / 2ⁿ (one draw of 1..=2ⁿ). Arrows / bolts come with a launcher, a sling or
/// throwing weapon makes 6..12, a chest (0x62) gets gold, gems and a trap, a bag (0x3a) gold
/// and other loot, gold (0x58) 2..60 pieces.
pub fn equip_monster(s: &mut State, m: Slot, index: i16) {
    let at = super::GameData::a4(EQUIP_LISTS).wrapping_add_signed(i32::from(
        s.objcmds.data.a4_word(EQUIP_OFFSETS + 2 * i32::from(index)) as i16,
    ));
    let mut p = at;
    let mut next = |s: &State| {
        let b = s.objcmds.data.byte(p);
        p = p.wrapping_add(1);
        b
    };
    loop {
        let b = next(s);
        if b == 0 {
            break;
        }
        let o = create_in(s, SPELL, ST_SPELL, u16::from(b) << 8 | 1, m);
        status_or(s, o, 0x02);
    }
    let mut chance = 2u16;
    loop {
        let b = next(s);
        if b == 0 {
            break;
        }
        if s.rng.range(1, chance) == 1 {
            let n = if matches!(b, 0x24..=0x26) {
                s.rng.range(6, 12)
            } else {
                1
            };
            for _ in 0..n {
                let o = create_in(s, u16::from(b), ST_READIED, 0, m);
                if o < 0 {
                    continue;
                }
                if b == 0x29 || b == 0x36 {
                    let n = s.rng.range(12, 24);
                    create_in(s, 0x37, ST_CARRIED, n, m);
                }
                if b == 0x2a || b == 0x32 {
                    let n = s.rng.range(12, 24);
                    create_in(s, 0x38, ST_CARRIED, n, m);
                }
            }
        }
        chance = chance.wrapping_mul(2);
    }
    chance = 2;
    loop {
        let b = next(s);
        if b == 0 {
            break;
        }
        if s.rng.range(1, chance) == 1 {
            create_in(s, u16::from(b), ST_READIED, 1, m);
        }
        chance = chance.wrapping_mul(2);
    }
    chance = 2;
    loop {
        let b = next(s);
        if b == 0 {
            break;
        }
        if s.rng.range(1, chance) == 1 {
            treasure(s, m, index, b);
        }
        chance = chance.wrapping_mul(2);
    }
}

/// One treasure entry `b` of 0x13121a for monster `m`.
fn treasure(s: &mut State, m: Slot, index: i16, b: u8) {
    let kinds = |s: &State, k: u16| s.objcmds.data.a4_word(TREASURE_TYPES + 2 * i32::from(k));
    match u16::from(b) {
        CHEST => {
            let n = s.rng.range(1, 100);
            let c = create_in(s, CHEST, ST_IN_CHEST, n, m);
            if c < 0 {
                return;
            }
            set_frame(s, c, 1);
            if s.rng.range(0, 1) != 0 && index != 0 {
                create_in(s, 0x151, ST_IN_CHEST, 0x1600, c);
            }
            if s.rng.range(0, 3) != 0 {
                let n = s.rng.range(1, 100);
                create_in(s, GOLD, ST_IN_CHEST, n, c);
            }
            if s.rng.range(0, 3) != 0 {
                let k = s.rng.range(0, 2);
                let n = s.rng.range(1, 10);
                let t = kinds(s, k);
                create_in(s, t, ST_IN_CHEST, n, c);
            }
            if s.rng.range(0, 3) == 0 {
                let n = s.rng.range(1, 6);
                create_in(s, 0x5a, ST_IN_CHEST, n, c);
            }
            if s.rng.range(0, 10) == 0 {
                let n = s.rng.range(1, 4);
                create_in(s, 0x4d, ST_IN_CHEST, n, c);
            }
        }
        k if k == u16::from(BAG) => {
            if s.rng.range(0, 3) != 0 && index != 0x13 {
                let n = s.rng.range(1, 100);
                create_in(s, GOLD, ST_CARRIED, n, m);
            }
            if s.rng.range(0, 3) == 0 {
                let k = s.rng.range(0, 2);
                let n = s.rng.range(1, 10);
                let t = kinds(s, k);
                create_in(s, t, ST_CARRIED, n, m);
            }
            if s.rng.range(0, 3) == 0 {
                let f = s.rng.range(0, 7);
                create_in(s, 0x113 | f << 10, ST_CARRIED, 1, m);
            }
        }
        GOLD => {
            let a = s.rng.range(1, 30);
            let b = s.rng.range(1, 30);
            create_in(s, GOLD, ST_CARRIED, a.wrapping_add(b), m);
        }
        other => {
            create_in(s, other, ST_CARRIED, 1, m);
        }
    }
}

/// Original: 0x1319b2 (objects: hatch egg `egg`: its monsters and objects, and the warning of
/// companion actor 3 about the direction they came from)
///
/// Nothing while A4-0x7afb is set, or outside the egg's hours (quality tens 1: 6..=18 only,
/// 2: never then). First it notes whether the gargoyles are friends: a gargoyle in the party, or
/// the hero (actor 1) carrying the amulet 0x4c. An egg already hatched (status 0x40) is only
/// marked again; else one draw 1..=100 against its quantity. Each template then makes 1..=its
/// quantity (one draw; the whole quantity when the egg's is 100, one silver serpent): objects
/// below type 0x156 (except a chest) are created on the egg's tile, monsters by 0x131646 with
/// the egg's alignment. Everything after the very first is scattered (0x121e88, out of view
/// unless the egg has status 0x20 or the party is arriving) or removed when no tile is free,
/// which ends that one (port: the original goes on with the freed slot, see the loop). A silver
/// serpent gets its body segments (no slot for the first removes it, which ends it too; the
/// original writes the actor bytes into its freed slot), a hydra its heads. Actors get the
/// egg's worktype, the quality of the first actor hatchling's template, a removed one's too
/// (taken again while it is 0; 12 from a template of gargoyles that are friends on), with the
/// movement flags of worktypes 15 / 16, and an attacking gargoyle doubled stats. A hostile
/// hatched out of an egg without status 0x20 may make actor 3 warn of its direction once per
/// fight (A4-0x6a88). The egg ends with status 0x40 (hatched) and 0x02.
pub fn hatch_egg(s: &mut State, egg: Slot) {
    let mut friends = false;
    let mut work: u8 = 0;
    let mut last: Slot = 0;
    if s.map.flag_7afb != 0 {
        return;
    }
    let q = i16::from(quality(s, egg));
    let hour = s.actors.calendar.hour;
    if q / 10 == 1 && !(6..=0x12).contains(&hour) {
        return;
    }
    if q / 10 == 2 && (6..=0x12).contains(&hour) {
        return;
    }
    let scatter_out = s.objects.status(egg) & 0x20 == 0 && s.map.arriving == 0;
    for m in 0..usize::from(s.actors.party.size) {
        let member = s.actors.party.members.get(m).copied().unwrap_or(0);
        let t = s.objects.kind(Slot::from(member));
        last = t as Slot;
        if t == WINGED_GARGOYLE || t == GARGOYLE {
            friends = true;
            break;
        }
        if find_child(&mut s.objects, 1, AMULET, -1) >= 0 {
            friends = true;
        }
    }
    if s.objects.status(egg) & 0x40 == 0 {
        let align = ((q % 10) - 1) << 5;
        let roll = s.rng.range(1, 100);
        if roll <= u16::from(amount(s, egg)) {
            hatch_templates(
                s,
                egg,
                q,
                align as u8,
                friends,
                scatter_out,
                &mut work,
                &mut last,
            );
        }
    }
    status_or(s, egg, 0x40 | 0x02);
}

/// The template loop of 0x1319b2 and the warning after it.
#[allow(clippy::too_many_arguments)]
fn hatch_templates(
    s: &mut State,
    egg: Slot,
    q: i16,
    align: u8,
    friends: bool,
    scatter_out: bool,
    work: &mut u8,
    last: &mut Slot,
) {
    let (ex, ey) = (s.objects.x(egg), s.objects.y(egg));
    let egg_full = amount(s, egg) == 100;
    let egg_marked = s.objects.status(egg) & 0x20 != 0;
    let mut hostile = false;
    let mut first = true;
    let mut c = first_child(&mut s.objects, egg);
    while c >= 0 {
        // The template walk keeps its own cursor: removing a hatchling walks its contents with
        // 0x10ae52, which in the original (0x10c1d6) overwrites the cursor this loop reads with
        // 0x10aed6 next and ends or derails the walk.
        let cursor = s.objects.children;
        let t = s.objects.kind(c);
        let flagged = if s.objects.status(c) & 0x40 != 0 {
            t | 0x8000
        } else {
            t
        };
        let mut n = u16::from(amount(s, c));
        if !egg_full {
            n = s.rng.range(1, n);
        }
        if t == SERPENT {
            n = 1;
        }
        if (t == WINGED_GARGOYLE || t == GARGOYLE) && friends {
            *work = 0xc;
        }
        for _ in 0..n as i16 {
            let z = leader_z(s);
            let m = if t < 0x156 && t != CHEST {
                let count = u16::from(stack_kind(&s.objects, t) != 0);
                create_on_map(&mut s.objects, t, 1, count, ex, ey, z)
            } else {
                let m = spawn_monster(s, flagged, ex, ey, z);
                if m >= 0 {
                    let a = &mut s.actors.attrs.status[m as usize];
                    if q % 10 != 0 {
                        *a = (*a & 0x9f) | align;
                    }
                    if *a & 0x20 != 0 && !egg_marked {
                        hostile = true;
                    }
                }
                m
            };
            if m < 0 {
                continue;
            }
            if first {
                first = false;
            } else if !seams::scatter(s, m, scatter_out) {
                // After removing it (0x131cfe-0x131d02) the original jumps forward (0x131d06)
                // to 0x131d0e, the path a placed hatchling takes, and gives the freed slot a
                // serpent body, hydra heads and its actor bytes; the port only takes the
                // worktype on the way (0x131ffe) and stops here.
                remove(s, m);
                take_worktype(s, c, m, work);
                continue;
            }
            if t == SERPENT {
                if !serpent_body(s, c, m, last) {
                    // The original goes on to 0x131ff2 with the freed slot; as above.
                    take_worktype(s, c, m, work);
                    continue;
                }
            } else if t == HYDRA {
                hydra_heads(s, m);
            }
            if m < ACTOR_SLOTS as Slot {
                let i = m as usize;
                take_worktype(s, c, m, work);
                s.actors.worktype[i] = *work;
                s.actors.attrs.combat_mode[i] = *work;
                let mf = &mut s.actors.attrs.movement_flags[i];
                if *work == 0xf {
                    *mf = (*mf & 0xf8) | 2;
                } else if *work == 0x10 {
                    *mf &= 0xf8;
                }
                if (t == WINGED_GARGOYLE || t == GARGOYLE) && s.actors.worktype[i] == 6 {
                    let a = &mut s.actors.attrs;
                    a.strength[i] = a.strength[i].wrapping_mul(2);
                    a.dexterity[i] = a.dexterity[i].wrapping_mul(2);
                    a.intelligence[i] = a.intelligence[i].wrapping_mul(2);
                }
            }
        }
        s.objects.children = cursor;
        c = next_child(&mut s.objects);
    }
    if s.actors.work.flag_6a88 == 0 && hostile {
        let c = s.map.centre;
        let dx = ex.wrapping_sub(c.x) as i16;
        let dy = ey.wrapping_sub(c.y) as i16;
        let (rx, ry) = s.inventory.pointer_origin;
        let dir = seams::direction_of(
            s,
            (dx << 4).wrapping_add(rx as i16),
            (dy << 4).wrapping_add(ry as i16),
        );
        let st = s.actors.attrs.status[usize::from(SCOUT)];
        if s.rng.range(0, 3) != 0
            && st & 0x80 != 0
            && st & 0x10 == 0
            && seams::distance(s, SCOUT, c.x, c.y) < 6
        {
            let at = super::GameData::a4(DIR_NAMES).wrapping_add_signed(i32::from(dir) * 4);
            // Port (spec 08, slice K2; 2026-10-24): read for display, so the locale pack's label
            // of it when it has one ([`crate::magic::LABELS`], [`crate::locale::shown`]).
            let name = crate::locale::shown(s, s.objcmds.data.pointed(at));
            let fmt = text::a4_string(s, MSG_WARNING);
            text::queue(s, fmt, vec![PrintArg::Str(name)]);
            s.actors.work.prompt = 1;
        }
        s.actors.work.flag_6a88 = 1;
    }
}

/// 0x131ff2-0x132016: for an actor slot `m` (a freed one too), the egg's worktype becomes the
/// quality of template `c` while it is still 0.
fn take_worktype(s: &State, c: Slot, m: Slot, work: &mut u8) {
    if m < ACTOR_SLOTS as Slot && *work == 0 {
        *work = quality(s, c);
    }
}

/// 0x1319b2 for a silver serpent `m` hatched from template `c`: frame 0, quality 0, then
/// quantity(c) + 1 body segments (A4-0x6592 numbers them) laid out from the head, the last
/// with the tail frame. A segment that finds no slot ends the body: the head goes when it was
/// the first, else the segment before becomes the tail. Returns whether the head is still
/// there: after removing it (0x131e54-0x131e58) the original jumps (0x131e5c) to 0x131ff2 and
/// writes the actor bytes into the freed slot, which the port's caller skips.
fn serpent_body(s: &mut State, c: Slot, m: Slot, last: &mut Slot) -> bool {
    let i = m as usize;
    s.objects.type_frame[i] &= 0x3ff;
    s.map.flag_6592 = 0;
    s.objects.qq[i] &= 0x00ff;
    s.map.flag_6592 = 1;
    let (hx, hy) = (s.objects.x(m), s.objects.y(m));
    let len = i16::from(amount(s, c));
    let mut k: i16 = 0;
    while k <= len {
        let j = k & 3;
        let tf = (sbyte(s, SEG_FRAME, j) << 10).wrapping_add(SERPENT as i16) as u16;
        let seq = s.map.flag_6592;
        s.map.flag_6592 = seq.wrapping_add(1);
        let qq = u16::from(seq) << 8 | m as u16;
        let px = hx.wrapping_add_signed(sbyte(s, SEG_DX, j));
        let py = hy.wrapping_add_signed(sbyte(s, SEG_DY, j));
        let z = leader_z(s);
        let p = create_on_map(&mut s.objects, tf, 1, qq, px, py, z);
        if p < 0 {
            if k == 0 {
                remove(s, m);
                return false;
            }
            let f = sbyte(s, TAIL_FRAME, (k - 1) & 3) as u16;
            set_frame(s, *last, f);
            break;
        }
        status_clear(s, p, 0x20);
        *last = p;
        if len == k {
            let f = sbyte(s, TAIL_FRAME, j) as u16;
            set_frame(s, p, f);
        }
        k += 1;
    }
    true
}

/// 0x1319b2 for a hydra `m`: frame 0 and a head (0x16e, 0x131646) on every second
/// neighbouring tile, facing out, with the body's alignment, worktype and combat mode 13.
fn hydra_heads(s: &mut State, m: Slot) {
    s.objects.type_frame[m as usize] &= 0x3ff;
    let (hx, hy) = (s.objects.x(m), s.objects.y(m));
    for d in (0..8u16).step_by(2) {
        let px = hx.wrapping_add_signed(s.objects.defs.dx(usize::from(d)));
        let py = hy.wrapping_add_signed(s.objects.defs.dy(usize::from(d)));
        let z = leader_z(s);
        let h = spawn_monster(s, HYDRA_HEAD, px, py, z);
        if h < 0 {
            continue;
        }
        let i = h as usize;
        let mf = &mut s.actors.attrs.movement_flags[i];
        *mf = (*mf & 0xf8) | d as u8;
        set_frame(s, h, u16::from(d & 2 == 0));
        let body = s.actors.attrs.status[m as usize] & 0x60;
        let st = &mut s.actors.attrs.status[i];
        *st = (*st & 0x9f) | body;
        s.actors.worktype[i] = 0xd;
        s.actors.attrs.combat_mode[i] = 0xd;
    }
}
