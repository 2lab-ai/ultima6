//! Object blocks: which superchunks live in slots, reading a block into slots and writing one
//! back.
//!
//! Block index `i` (0..69) is the original's: surface superchunk `i = sy * 8 + sx` for
//! `OBJBLK{A+sx}{A+sy}`, dungeon level z at `63 + z` (`OBJBLK{A+z-1}I`), the same order as
//! [`ObjBlockId::all`](u6_formats::objblk::ObjBlockId::all). The port keeps the files in memory:
//! [`Objects::blocks`] are `savegame/objblk??`, [`Objects::tmp`] the `.tmp` copies 0x10d538
//! writes while playing.

use super::moves::remove;
use super::{HEAD, IN_CONTAINER, IN_INVENTORY, NONE, Objects, Slot, TEMPORARY, WHERE_MASK, seams};
use crate::state::State;

/// Number of object blocks (64 surface + 5 dungeon levels).
pub const BLOCK_COUNT: usize = 69;
/// Records a block holds at most (0x10ddd2 clamps the count; 0x10d376 warns at 0xc00).
pub const MAX_RECORDS: usize = 0xc00;
/// Save-mode values of A4-0x637a.
const MODE_SAVE: u16 = 0x53;
const MODE_FROZEN: u16 = 0x52;
const MODE_GAME: u16 = 0x47;
/// Image address of the region-full error line (0x10a490 + 4) printed by 0x10d376.
const STR_TOO_MANY: u32 = 0x0010_a494;
/// Distance (tiles) from the party beyond which 0x10c49c drops temporary objects.
const KEEP: i16 = 0x14;

/// Block index of dungeon level `z` (1..=5).
pub fn block_index(z: u16) -> usize {
    63 + usize::from(z)
}

impl Objects {
    /// The bytes of block `i`: its `.tmp` file when the flag says it is current, else the
    /// plain file (none when the port has no such block).
    fn block_file(&self, i: usize) -> Option<Vec<u8>> {
        if self.tmp_flag.get(i).copied().unwrap_or(0) != 0 {
            self.tmp.get(i).cloned().flatten()
        } else {
            self.blocks.get(i).map(|(_, b)| b.to_bytes())
        }
    }

    /// Original: 0x10d376 (objects: pack `slot` into record `n` of the buffer; frees its type)
    ///
    /// A contained object whose parent is an actor becomes inventory, one whose parent is an
    /// object becomes container content; a container content's parent becomes the record index
    /// of the parent (searched back from `n`). Warns when the buffer is full.
    pub(crate) fn pack_record(&mut self, s_print: &mut Vec<u32>, slot: Slot, n: usize) {
        let i = slot as usize;
        if self.place(slot) != 0 {
            let st = &mut self.obj_status[i];
            let parent = (u16::from(self.location[i].to_packed()[1]) << 8
                | u16::from(self.location[i].to_packed()[0])) as Slot;
            if parent >= 0x100 {
                *st = (*st & !WHERE_MASK) | IN_CONTAINER;
            } else if *st & WHERE_MASK == IN_CONTAINER {
                *st = (*st & !WHERE_MASK) | IN_INVENTORY;
            }
        }
        let r = n * 8;
        let [b0, b1, b2] = self.location[i].to_packed();
        let [t0, t1] = self.type_frame[i].to_be_bytes();
        let [q0, q1] = self.qq[i].to_be_bytes();
        self.rec[r..r + 8].copy_from_slice(&[self.obj_status[i], b0, b1, b2, t0, t1, q0, q1]);
        if self.place(slot) == IN_CONTAINER {
            let parent = self.parent(slot);
            let mut m = n;
            while m > 0 && self.rec_slots[m] != parent {
                m -= 1;
            }
            let [lo, hi] = (m as u16).to_le_bytes();
            self.rec[r + 1] = lo;
            self.rec[r + 2] = hi;
        }
        self.type_frame[i] = 0;
        if n + 1 >= MAX_RECORDS {
            s_print.push(STR_TOO_MANY);
        }
    }

    /// The walk shared by 0x10d6ac and 0x10dfb4: every map object `wanted` and everything
    /// inside it goes into the record buffer and back to the free chain; actors stay linked
    /// (only their contents go). Returns the record count.
    fn collect(&mut self, prints: &mut Vec<u32>, wanted: impl Fn(&Objects, Slot) -> bool) -> usize {
        let mut n = 0;
        let mut prev = HEAD;
        let mut cur = self.next_of(HEAD);
        while n < MAX_RECORDS && cur >= 0 {
            if self.place(cur) != 0 || !wanted(self, cur) {
                prev = cur;
                cur = self.next_of(cur);
                continue;
            }
            if cur < super::ACTOR_SLOTS as Slot {
                prev = cur;
            } else {
                self.rec_slots[n] = cur;
                self.pack_record(prints, cur, n);
                n += 1;
                self.free.1 += 1;
            }
            let mut last = cur;
            cur = self.next_of(cur);
            while cur >= 0 && self.place(cur) != 0 {
                if n < MAX_RECORDS {
                    self.rec_slots[n] = cur;
                    self.pack_record(prints, cur, n);
                    n += 1;
                }
                self.free.1 += 1;
                last = cur;
                cur = self.next_of(cur);
            }
            self.set_next(last, self.free.0);
            self.free.0 = self.next_of(prev);
            self.set_next(prev, cur);
        }
        n
    }

    /// Original: 0x10d538 (objects: write `n` buffer records as block `i`'s `.tmp` file)
    ///
    /// While saving (mode 'S') every current `.tmp` replaces its plain file instead, block `i`
    /// is written as its plain file, the flags clear and the mode returns to 'G'. In mode 'R'
    /// only the count is written.
    fn write_block_file(&mut self, n: usize, i: usize) {
        if let Some(f) = self.tmp_flag.get_mut(i) {
            *f = 1;
        }
        let mut bytes = (n as u16).to_be_bytes().to_vec();
        if self.save_mode != MODE_SAVE && self.save_mode != MODE_FROZEN {
            bytes.extend_from_slice(&self.rec[..n * 8]);
        }
        if let Some(t) = self.tmp.get_mut(i) {
            *t = Some(bytes);
        }
        if self.save_mode != MODE_SAVE {
            return;
        }
        for j in 0..BLOCK_COUNT {
            if self.tmp_flag[j] == 0 {
                continue;
            }
            let file = self.tmp[j].take();
            if j != i
                && let (Some(bytes), Some(block)) = (file, self.blocks.get_mut(j))
            {
                block.1 = parse_lenient(&bytes);
            }
            self.tmp_flag[j] = 0;
        }
        let mut plain = (n as u16).to_be_bytes().to_vec();
        plain.extend_from_slice(&self.rec[..n * 8]);
        if let Some(block) = self.blocks.get_mut(i) {
            block.1 = parse_lenient(&plain);
        }
        self.save_mode = MODE_GAME;
        self.saved_blocks = 1;
    }
}

/// A block file as the game would read it back: the count, then as many whole records as the
/// file holds.
fn parse_lenient(bytes: &[u8]) -> u6_formats::objblk::ObjBlock {
    let count = bytes
        .get(..2)
        .map_or(0, |w| usize::from(u16::from_be_bytes([w[0], w[1]])));
    let objects = bytes
        .get(2..)
        .unwrap_or(&[])
        .chunks_exact(8)
        .take(count)
        .map(u6_formats::objblk::ObjRecord::from_bytes)
        .collect();
    u6_formats::objblk::ObjBlock { objects }
}

/// Prints the messages a block routine queued (0x10d376's warning).
fn flush(s: &mut State, prints: Vec<u32>) {
    for p in prints {
        seams::print(s, p, &[]);
    }
}

/// Original: 0x10d6ac (objects: write surface block `b` back and free its slots)
///
/// Map objects on level 0 whose x >> 7, y >> 7 are the block's column and row go to the
/// `.tmp` file with their contents; actors there keep their slots and write their contents.
pub fn write_back_surface(s: &mut State, b: i16) {
    let o = &mut s.objects;
    if b < 0 || o.save_mode == MODE_FROZEN {
        return;
    }
    let (bx, by) = (((b & 7) << 7) as u16, ((b & 0x38) << 4) as u16);
    let mut prints = Vec::new();
    let n = o.collect(&mut prints, |o, c| {
        o.z(c) == 0 && o.x(c) & 0x380 == bx && o.y(c) & 0x380 == by
    });
    o.write_block_file(n, (b & 0x3f) as usize);
    flush(s, prints);
}

/// Original: 0x10dfb4 (objects: write dungeon level `z` (0..=5) back and free its slots)
pub fn write_back_level(s: &mut State, z: i16) {
    let o = &mut s.objects;
    if !(0..=5).contains(&z) || o.save_mode == MODE_FROZEN {
        return;
    }
    let mut prints = Vec::new();
    let n = o.collect(&mut prints, |o, c| i16::from(o.z(c)) == z);
    o.write_block_file(n, (z + 63) as usize);
    flush(s, prints);
}

/// Writes block `i` back (surface 0..64 through 0x10d6ac, levels 64..69 through 0x10dfb4).
pub fn write_back_block(s: &mut State, i: usize) {
    if i < 64 {
        write_back_surface(s, i as i16);
    } else {
        write_back_level(s, i as i16 - 63);
    }
}

/// Reads the count and records of block file `i` into the buffer (the count clamped to 0xc00;
/// a short file leaves the rest of the buffer as it was) and unpacks them.
fn read_block_file(o: &mut Objects, i: usize) {
    let Some(file) = o.block_file(i) else {
        return;
    };
    let count = file
        .get(..2)
        .map_or(0, |w| usize::from(u16::from_be_bytes([w[0], w[1]])))
        .min(MAX_RECORDS);
    if count == 0 {
        return;
    }
    let data = file.get(2..).unwrap_or(&[]);
    let len = (count * 8).min(data.len());
    o.rec[..len].copy_from_slice(&data[..len]);
    o.unpack(count);
}

/// Original: 0x10ddd2 (objects: read surface block `b` into slots)
pub fn load_surface(o: &mut Objects, b: i16) {
    if b < 0 || o.save_mode == MODE_FROZEN {
        return;
    }
    read_block_file(o, (b & 0x3f) as usize);
}

/// Original: 0x10e10c (objects: read dungeon level `z` (0..=5) into slots)
pub fn load_level(o: &mut Objects, z: i16) {
    if !(0..=5).contains(&z) || o.save_mode == MODE_FROZEN {
        return;
    }
    read_block_file(o, (z + 63) as usize);
}

/// Reads block `i` into slots (surface through 0x10ddd2, levels through 0x10e10c).
pub fn load_block(o: &mut Objects, i: usize) {
    if i < 64 {
        load_surface(o, i as i16);
    } else {
        load_level(o, i as i16 - 63);
    }
}

impl Objects {
    /// Original: 0x10db82 (objects: give `n` buffer records slots and merge them into the list)
    ///
    /// Records take free slots in file order (stopping when none is left) and chain in that
    /// order; a container content's record index becomes its parent's slot. The chain is then
    /// merged into the sorted list run by run (0x10d86e order); a run out of order restarts
    /// the merge from the head.
    pub(crate) fn unpack(&mut self, n: usize) {
        let mut i = 0;
        while i < n {
            if self.free.1 == 0 {
                self.rec_slots[i] = NONE;
                if i != 0 {
                    self.set_next(self.rec_slots[i - 1], NONE);
                }
                break;
            }
            let slot = self.pop_free();
            self.rec_slots[i] = slot;
            let r = &self.rec[i * 8..i * 8 + 8];
            let k = slot as usize;
            self.obj_status[k] = r[0];
            self.location[k] = u6_formats::position::Position::from_packed([r[1], r[2], r[3]]);
            self.type_frame[k] = u16::from_be_bytes([r[4], r[5]]);
            self.qq[k] = u16::from_be_bytes([r[6], r[7]]);
            if self.place(slot) == IN_CONTAINER {
                let idx = usize::from(u16::from_le_bytes([r[1], r[2]]));
                let parent = self.rec_slots.get(idx).copied().unwrap_or(NONE);
                self.set_parent(slot, parent);
            }
            i += 1;
        }
        if i == n {
            self.set_next(self.rec_slots[i - 1], NONE);
        }
        let mut at = HEAD;
        let mut chain = self.rec_slots[0];
        let mut restart = false;
        while at >= 0 && chain >= 0 {
            if self.compare(self.next_of(at), chain) < 0 {
                at = self.next_of(at);
            } else {
                let rest = self.next_of(at);
                self.set_next(at, chain);
                at = rest;
                while self.next_of(chain) >= 0 {
                    if self.compare(at, self.next_of(chain)) <= 0 {
                        break;
                    }
                    if self.compare(chain, self.next_of(chain)) > 0 {
                        restart = true;
                        break;
                    }
                    chain = self.next_of(chain);
                }
                let after = self.next_of(chain);
                self.set_next(chain, at);
                chain = after;
                if restart {
                    at = HEAD;
                    restart = false;
                    continue;
                }
            }
            if self.next_of(at) < 0 {
                self.set_next(at, chain);
                at = NONE;
            }
        }
    }

    /// Original: 0x10da94 (objects: move inventory that is not behind its actor to the actor)
    pub(crate) fn fix_inventory(&mut self) {
        let mut at = HEAD;
        loop {
            let c = self.next_of(at);
            if c < 0 {
                return;
            }
            if self.status(c) & IN_INVENTORY == 0 {
                at = c;
                continue;
            }
            let owner = self.parent(c);
            let mut p = self.next_of(owner);
            while p >= 0 && self.place(p) != 0 && p != c {
                p = self.next_of(p);
            }
            if p == c {
                at = c;
                continue;
            }
            let end = self.last_inside(c);
            let tail = self.last_inside(owner);
            self.set_next(at, self.next_of(end));
            self.set_next(end, self.next_of(tail));
            self.set_next(tail, c);
        }
    }
}

/// Original: 0x10c49c (objects: drop temporary objects more than 20 tiles from (x, y, z))
///
/// Temporary actors 0xe0..0x100 and temporary map objects out of range are removed (the walk
/// continues with the last content a removal moved out); a permanent egg (0x14f) out of range
/// is deactivated (status 0x40 cleared).
pub fn prune_far(s: &mut State, x: u16, y: u16, z: u16) {
    let mask = s.objects.coord_mask;
    let (x, y) = ((x & mask) as i16, (y & mask) as i16);
    let far = |o: &Objects, c: Slot| {
        let (cx, cy) = (o.x(c) as i16, o.y(c) as i16);
        cx < x - KEEP || cx >= x + KEEP || cy < y - KEEP || cy >= y + KEEP || u16::from(o.z(c)) != z
    };
    for a in super::FIRST_TEMP_ACTOR..super::ACTOR_SLOTS as Slot {
        let o = &s.objects;
        if o.tf(a) != 0 && far(o, a) && o.status(a) & TEMPORARY != 0 {
            remove(s, a);
        }
    }
    let mut c = s.objects.next_of(HEAD);
    while c >= 0 {
        let o = &s.objects;
        let mut next = o.next_of(c);
        if o.place(c) == 0 && far(o, c) {
            if o.status(c) & TEMPORARY == 0 && o.tf(c) != 0 {
                if o.kind(c) == 0x14f && o.status(c) & 0x40 != 0 {
                    s.objects.obj_status[c as usize] &= !0x40;
                }
            } else {
                remove(s, c);
                if s.objects.last_moved_child >= 0 {
                    next = s.objects.last_moved_child;
                }
            }
        }
        c = next;
    }
}

/// Original: 0x10e1ce (objects: load the object blocks for the party at (x, y, z))
///
/// On the surface the 2 × 2 superchunks nearest (x, y) are kept in slots: the base superchunk
/// follows the party with 5 tiles of hysteresis at each 128-tile edge. Leaving a dungeon level
/// prunes and writes it back; superchunks no longer wanted are written back (pruning first);
/// newly wanted ones and a newly entered level are read, and inventory is re-attached after
/// any read.
pub fn load_region(s: &mut State, x: u16, y: u16, z: u16) {
    let mut wrote = 0;
    let mut read = 0;
    let level = s.objects.loaded_level;
    if level > 0 && level != z as i16 {
        prune_far(s, x, y, z);
        write_back_level(s, level);
        wrote += 1;
    }
    let (xi, yi) = (x as i16, y as i16);
    let mut want = [-1i16; 4];
    if z == 0 {
        let base = s.objects.surface_base;
        let b = if base == -1 {
            ((yi.wrapping_sub(0x40) >> 4) & 0x38) + ((xi.wrapping_sub(0x40) >> 7) & 7)
        } else {
            let (mut dc, mut e) = (0i16, xi.wrapping_sub((base & 7) * 0x80));
            while e < 5 {
                e += 0x80;
                dc -= 1;
            }
            while e >= 0xfb {
                e -= 0x80;
                dc += 1;
            }
            let (mut dr, mut f) = (0i16, yi.wrapping_sub((base & 0x38) * 0x10));
            while f < 5 {
                f += 0x80;
                dr -= 8;
            }
            while f >= 0xfb {
                f -= 0x80;
                dr += 8;
            }
            (((base & 7) + dc) & 7) + (((base & 0x38) + dr) & 0x38)
        };
        let right = (b & 0x38) + ((b + 1) & 7);
        want = [b, right, (b + 8) & 0x3f, (right + 8) & 0x3f];
        s.objects.surface_base = b;
    } else {
        s.objects.surface_base = -1;
    }
    if s.objects.loaded_level == 0 {
        for i in 0..4 {
            let have = s.objects.loaded_surface[i];
            if !want.contains(&have) && have != -1 {
                if wrote == 0 {
                    prune_far(s, x, y, z);
                }
                write_back_surface(s, have);
                wrote += 1;
                s.objects.loaded_surface[i] = -1;
            }
        }
    }
    if z == 0 {
        for w in want {
            if s.objects.loaded_surface.contains(&w) {
                continue;
            }
            let free = s.objects.loaded_surface.iter().position(|&b| b == -1);
            load_surface(&mut s.objects, w);
            read += 1;
            match free {
                Some(i) => s.objects.loaded_surface[i] = w,
                // The original writes one word past the table (into A4-0x7844).
                None => s.objects.surface_base = w,
            }
        }
    } else if s.objects.loaded_level != z as i16 {
        load_level(&mut s.objects, z as i16);
        read += 1;
    }
    s.objects.loaded_level = z as i16;
    if read != 0 {
        s.objects.fix_inventory();
    }
}

/// Original: 0x10d4e8 (objects: delete every `.tmp` block file whose flag is set)
///
/// The flags stay set (the boot path calls this before any block was written).
pub fn delete_tmp_files(o: &mut Objects) {
    for i in 0..BLOCK_COUNT {
        if o.tmp_flag[i] != 0 {
            o.tmp[i] = None;
        }
    }
}

/// The object half of a region change (0x10e1ce at the leader's tile); map owns the region
/// cache and calls this when the view moves (wired at merge).
#[allow(dead_code)]
pub(crate) fn load_region_objects(s: &mut State, at: u6_formats::position::Position) {
    load_region(s, at.x, at.y, u16::from(at.z));
}

/// Writes every block in slots back to its `.tmp` file and empties the region keys (what
/// leaving the world or saving needs; the original does it per block in 0x10e1ce / 0x10e4bc).
#[allow(dead_code)]
pub(crate) fn unload_region_objects(s: &mut State) {
    for i in 0..4 {
        let b = s.objects.loaded_surface[i];
        if b != -1 {
            write_back_surface(s, b);
            s.objects.loaded_surface[i] = -1;
        }
    }
    let level = s.objects.loaded_level;
    if level > 0 {
        write_back_level(s, level);
    }
    s.objects.loaded_level = -1;
    s.objects.surface_base = -1;
}
