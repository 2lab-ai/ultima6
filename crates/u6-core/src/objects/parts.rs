//! Monsters made of several objects: the silver serpent (head 0x19d + body segments), dragons
//! and other multi-tile monsters (0x176, 0x19b, 0x1a9, ≥ 0x1aa), tangle vines (0x177).

use super::defs::at;
use super::iter::{actor_at, begin_at, next_at, tile_of};
use super::moves::{SERPENT, VINE, move_to, remove};
use super::{ACTOR_SLOTS, NONE, Slot};
use crate::state::{State, Tables};

fn flags1(t: &Tables, tile: u16) -> u8 {
    t.tileflags
        .flags1
        .get(usize::from(tile))
        .copied()
        .unwrap_or(0)
}

/// Direction (0 N, 2 E, 4 S, 6 W) a segment's highest connection bit points to: 0x80 → 4,
/// 0x40 → 6, 0x20 → 0, 0x10 → 2; `None` without one.
fn link_dir(f: u8) -> Option<usize> {
    [(0x80, 4), (0x40, 6), (0x20, 0), (0x10, 2)]
        .into_iter()
        .find(|&(bit, _)| f & bit != 0)
        .map(|(_, d)| d)
}

/// Original: 0x10a5b0 (objects: the object a monster part hangs on, or the part itself)
///
/// A part of a multi-tile monster (type ≥ 0x1aa, 0x19b, 0x19c) looks one tile the way its
/// flags1 bits 4-7 point for an object of its type whose flags point back; a serpent segment
/// (0x19d at slot ≥ 0x100) names its head in its quantity byte.
pub fn owner_of_part(s: &mut State, slot: Slot) -> Slot {
    if slot < 0 {
        return NONE;
    }
    let kind = s.objects.kind(slot);
    if kind >= 0x1aa || kind == 0x19b || kind == 0x19c {
        let o = &s.objects;
        let (mut x, mut y) = (o.x(slot), o.y(slot));
        let mask = match flags1(&s.tables, tile_of(&s.tables, o.tf(slot))) & 0xf0 {
            0x80 => {
                y = y.wrapping_sub(1);
                0x20
            }
            0x40 => {
                x += 1;
                0x10
            }
            0x20 => {
                y += 1;
                0x80
            }
            0x10 => {
                x = x.wrapping_sub(1);
                0x40
            }
            _ => return slot,
        };
        let z = u16::from(s.map.centre.z);
        let mut c = begin_at(&mut s.objects, &s.tables, x, y, z);
        while c >= 0 {
            let o = &s.objects;
            if o.kind(c) == kind && flags1(&s.tables, tile_of(&s.tables, o.tf(c))) & mask != 0 {
                return c;
            }
            c = next_at(&mut s.objects, &s.tables);
        }
        return slot;
    }
    let o = &s.objects;
    if kind == SERPENT && slot >= ACTOR_SLOTS as Slot {
        let head = (o.qq_of(slot) & 0xff) as Slot;
        if o.kind(head) == SERPENT {
            return head;
        }
    }
    slot
}

/// Original: 0x10b2cc (objects: re-frame every tangle vine from its vine neighbours)
///
/// For each vine actor 0xe0..0x100 (actor status bit 0x10 clear) the frame becomes the set of
/// directions N, E, S, W (bits 0-3) in which another such vine stands.
pub fn update_vine_frames(s: &mut State) {
    let dead = |s: &State, a: Slot| {
        let st = s.actors.attrs.status.get(a as usize).copied().unwrap_or(0);
        st & 0x10 != 0
    };
    for a in super::FIRST_TEMP_ACTOR..ACTOR_SLOTS as Slot {
        if s.objects.kind(a) != VINE || dead(s, a) {
            continue;
        }
        let (x, y) = (s.objects.x(a), s.objects.y(a));
        let mut bits = 0u16;
        for (n, d) in [0usize, 2, 4, 6].into_iter().enumerate() {
            let defs = &s.objects.defs;
            let nx = x.wrapping_add(defs.dx(d) as u16);
            let ny = y.wrapping_add(defs.dy(d) as u16);
            let mut c = actor_at(s, super::FIRST_TEMP_ACTOR, nx, ny);
            while c >= 0 {
                if s.objects.kind(c) == VINE && !dead(s, c) {
                    bits |= 1 << n;
                    break;
                }
                c = actor_at(s, c + 1, nx, ny);
            }
        }
        let i = a as usize;
        s.objects.type_frame[i] = (s.objects.type_frame[i] & 0x3ff) + (bits << 10);
    }
}

/// Original: 0x10b922 (objects: the dragon part (0x19b) beside (x, y, z) in direction `d` whose
/// frame matches the head's facing `facing`, -1)
pub fn find_segment_at(s: &mut State, x: u16, y: u16, z: u16, d: usize, facing: u16) -> Slot {
    let defs = &s.objects.defs;
    let want = u16::from(at(&defs.part_frame, usize::from(facing & 6) * 2 + (d >> 1)));
    let nx = x.wrapping_add(defs.dx(d) as u16);
    let ny = y.wrapping_add(defs.dy(d) as u16);
    let mut c = begin_at(&mut s.objects, &s.tables, nx, ny, z);
    while c >= 0 {
        let tf = s.objects.tf(c);
        if tf & 0x3ff == 0x19b && (tf >> 10) & 0x3e == want {
            return c;
        }
        c = next_at(&mut s.objects, &s.tables);
    }
    NONE
}

/// Original: 0x10b9ca (objects: move or remove the parts of a multi-tile monster)
///
/// `slot` has just moved from (ox, oy) (or is being removed: `drop`). For each direction the
/// monster's flags1 connect (every direction for 0x1a9 and 0x176), the part found next to the
/// old tile — a dragon part matching the head's facing (0x10b922), the tile of a 0x1a9 part
/// whose frame >> 2 is that direction, or a part of the same type whose flags1 point back —
/// moves the same way next to the new tile, or is removed.
pub fn move_monster_parts(s: &mut State, slot: Slot, ox: u16, oy: u16, drop: bool) {
    let o = &s.objects;
    let (x, y, z) = (o.x(slot), o.y(slot), u16::from(o.z(slot)));
    let tf = o.tf(slot);
    let f1 = flags1(&s.tables, tile_of(&s.tables, tf));
    let mut kind = tf & 0x3ff;
    if kind == 0x176 {
        kind = 0x1a9;
    }
    let facing = if kind == 0x19b {
        ((tf >> 10) & 6) >> 1
    } else {
        0
    };
    for d in 0..8usize {
        let dir_bit = at(&s.objects.defs.dir_bit, d);
        if kind != 0x1a9 && (d & 1 != 0 || dir_bit & f1 == 0) {
            continue;
        }
        let part = if kind == 0x19b {
            find_segment_at(s, ox, oy, z, d, facing * 2)
        } else {
            let defs = &s.objects.defs;
            let (px, py) = (
                ox.wrapping_add(defs.dx(d) as u16),
                oy.wrapping_add(defs.dy(d) as u16),
            );
            let back = at(&defs.dir_bit, d ^ 4);
            let mut c = begin_at(&mut s.objects, &s.tables, px, py, z);
            while c >= 0 {
                let ctf = s.objects.tf(c);
                if c >= ACTOR_SLOTS as Slot && ctf & 0x3ff == kind {
                    let hit = if kind == 0x1a9 {
                        usize::from(ctf >> 12) == d
                    } else {
                        back & flags1(&s.tables, tile_of(&s.tables, ctf)) != 0
                    };
                    if hit {
                        break;
                    }
                }
                c = next_at(&mut s.objects, &s.tables);
            }
            c
        };
        if part < 0 {
            continue;
        }
        if drop {
            remove(s, part);
        } else {
            let defs = &s.objects.defs;
            let nx = x.wrapping_add(defs.dx(d) as u16);
            let ny = y.wrapping_add(defs.dy(d) as u16);
            move_to(s, part, nx, ny, z);
        }
    }
}

/// Original: 0x10b42e (objects: drag a silver serpent's body after its head)
///
/// The head (actor `slot`) has moved from (ox, oy). Its frame becomes its direction of travel.
/// The body is walked from the head through the segments (0x19d, quality = segment number,
/// quantity = head) by their connection bits, renumbering each one up by one, until a tail
/// segment (frame & 0xf < 8). Then the tail jumps to the head's old tile as the first segment
/// (number head + 1, frame from the head's connections) and the segment before it becomes the
/// tail. With `drop` the segments are removed instead.
pub fn move_serpent(s: &mut State, slot: Slot, ox: u16, oy: u16, drop: bool) {
    let o = &s.objects;
    let kind_tf = |s: &State, c: Slot| flags1(&s.tables, tile_of(&s.tables, s.objects.tf(c)));
    let mut f1 = kind_tf(s, slot);
    let mut dir = link_dir(f1).unwrap_or(0);
    let (x, y, z) = (o.x(slot), o.y(slot), u16::from(o.z(slot)));
    let facing = if x > ox {
        f1 |= 0x40;
        2
    } else if x < ox {
        f1 |= 0x10;
        6
    } else if y > oy {
        f1 |= 0x20;
        4
    } else if y < oy {
        f1 |= 0x80;
        0
    } else {
        dir
    };
    let i = slot as usize;
    s.objects.type_frame[i] = (s.objects.type_frame[i] & 0x3ff) + ((facing as u16) << 10);
    let mut seg = (s.objects.qq[i] >> 8) + 1;
    let (mut prev, mut cur) = (slot, slot);
    let (mut px, mut py) = (ox, oy);
    loop {
        let defs = &s.objects.defs;
        let tx = px.wrapping_sub(defs.dx(dir) as u16);
        let ty = py.wrapping_sub(defs.dy(dir) as u16);
        let mut c = begin_at(&mut s.objects, &s.tables, tx, ty, z);
        let mut frame = 0;
        while c >= 0 {
            let o = &s.objects;
            let w = o.qq_of(c);
            if c >= ACTOR_SLOTS as Slot
                && o.kind(c) == SERPENT
                && w >> 8 == seg
                && (w & 0xff) as Slot == slot
            {
                prev = cur;
                frame = o.tf(c) >> 10;
                seg += 1;
                let q = &mut s.objects.qq[c as usize];
                *q = (*q & 0xff) | (seg << 8);
                cur = c;
                px = s.objects.x(c);
                py = s.objects.y(c);
                let keep = at(&s.objects.defs.seg_mask, dir);
                if let Some(d) = link_dir(kind_tf(s, c) & keep) {
                    dir = d;
                }
                if drop {
                    remove(s, c);
                }
                break;
            }
            c = next_at(&mut s.objects, &s.tables);
        }
        if c < 0 || frame & 0xf < 8 {
            break;
        }
    }
    if drop {
        return;
    }
    if prev != slot {
        let back = usize::from((s.objects.tf(cur) >> 10) & 0xf) ^ 5;
        let f = kind_tf(s, prev) & at(&s.objects.defs.seg_mask, back);
        let tail = [(0x80, 0x400), (0x40, 0xc00), (0x20, 0x1400), (0x10, 0x1c00)]
            .into_iter()
            .find(|&(bit, _)| f & bit != 0);
        if let Some((_, add)) = tail {
            let t = &mut s.objects.type_frame[prev as usize];
            *t = (*t & 0x3ff) + add;
        }
    }
    if cur == slot {
        return;
    }
    let frame = if prev != slot {
        u16::from(at(&s.objects.defs.seg_frame, usize::from(f1 >> 4)))
    } else {
        (s.objects.tf(slot) >> 10) + 1
    };
    let t = &mut s.objects.type_frame[cur as usize];
    *t = (*t & 0x3ff).wrapping_add(frame << 10);
    move_to(s, cur, ox, oy, z);
    let head_q = s.objects.qq[i] >> 8;
    let q = &mut s.objects.qq[cur as usize];
    *q = (*q & 0xff) | (((head_q + 1) & 0xff) << 8);
}
