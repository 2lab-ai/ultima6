//! The object list, its window grid and the free-slot chains.

use super::{
    ACTOR_SLOTS, FIRST_OBJECT_SLOT, FIRST_TEMP_ACTOR, GRID_SIDE, HEAD, IN_CONTAINER, IN_INVENTORY,
    NONE, OBJECT_SLOTS, Objects, Slot,
};

const GRID_CELLS: i16 = (GRID_SIDE * GRID_SIDE) as i16;
const LAST_CELL: i16 = GRID_CELLS - 1;

/// Original: 0x10ed44 (objects: reset the list, the free chains and the region keys)
///
/// Every slot links to the next one (0xbff and 0xff end their chain), the list is empty, free
/// objects start at 0x101 (0xaff of them), free temporary actors at 0xe0 (0x20), and no
/// region is loaded.
pub fn init_slots(o: &mut Objects) {
    for (i, n) in o.next.iter_mut().enumerate().take(OBJECT_SLOTS - 1) {
        *n = (i + 1) as Slot;
    }
    o.set_next((OBJECT_SLOTS - 1) as Slot, NONE);
    o.set_next(FIRST_TEMP_ACTOR + 0x1f, NONE);
    o.free_actors = (FIRST_TEMP_ACTOR, 0x20);
    o.free = (FIRST_OBJECT_SLOT, 0xaff);
    o.set_next(HEAD, NONE);
    o.loaded_surface = [-1; 4];
    o.surface_base = -1;
    o.loaded_level = -1;
}

impl Objects {
    /// Original: 0x10d86e (objects: list order of two slots, negative = `a` sorts first)
    ///
    /// Each side climbs out of containers to the object on the map or the carried object that
    /// holds it. A carried object sorts before every map object (load 0x10db82 relies on
    /// 0x10da94 to move it behind its actor afterwards); otherwise the order is y, then x, then
    /// z descending, carried objects counting at their actor's tile, and an object before its
    /// own contents. -1 sorts last.
    pub(crate) fn compare(&self, a: Slot, b: Slot) -> i16 {
        if a == NONE {
            return 1;
        }
        if b == NONE {
            return -1;
        }
        let root = |mut s: Slot| {
            while self.place(s) == IN_CONTAINER {
                s = self.parent(s);
            }
            s
        };
        let (mut ra, mut rb) = (root(a), root(b));
        if self.place(ra) != 0 && self.place(b) == 0 {
            return -1;
        }
        if self.place(rb) != 0 && self.place(a) == 0 {
            return 1;
        }
        if self.status(ra) & IN_INVENTORY != 0 {
            ra = self.parent(ra);
        }
        if self.status(rb) & IN_INVENTORY != 0 {
            rb = self.parent(rb);
        }
        let mut r = (self.y(ra) as i16).wrapping_sub(self.y(rb) as i16);
        if r == 0 {
            r = (self.x(ra) as i16).wrapping_sub(self.x(rb) as i16);
        }
        if r == 0 {
            r = i16::from(self.z(rb)) - i16::from(self.z(ra));
        }
        if ra == rb {
            if self.place(ra) == 0 {
                r = -1;
            } else if self.place(rb) == 0 {
                r = 1;
            }
        }
        r
    }

    /// Original: 0x10a7b2 (objects: list position of tile (x, y))
    ///
    /// With `before` = false: the first list entry at or after (y, x) in list order. With
    /// `before` = true: the entry after which an object at (x, y) is linked (slot 0x100 at the
    /// head). Inside the 40 × 40 window the grid gives the start; outside it the whole list is
    /// walked from the head (or from the window's last entry when (x, y) sorts after it).
    pub(crate) fn list_pos(&self, x: u16, y: u16, before: bool) -> Slot {
        let dx = x.wrapping_sub(self.window[0]) & 0x3ff;
        let dy = y.wrapping_sub(self.window[1]) & 0x3ff;
        let grid = |c: i16| self.grid.get(c as usize).copied().unwrap_or(NONE);
        if dx < GRID_SIDE as u16 && dy < GRID_SIDE as u16 {
            let mut c = (dy * GRID_SIDE as u16 + dx) as i16;
            if !before {
                if grid(c) >= 0 {
                    return grid(c);
                }
                while c < LAST_CELL && grid(c) == NONE {
                    c += 1;
                }
                return grid(c);
            }
            let mut p = c - 1;
            while p >= 0 && grid(p) == NONE {
                p -= 1;
            }
            while c < LAST_CELL && grid(c) == NONE {
                c += 1;
            }
            let mut prev = if p >= 0 { grid(p) } else { HEAD };
            let stop = grid(c);
            let mut cur = self.next_of(prev);
            while cur >= 0 && cur != stop {
                if self.place(cur) == 0 {
                    let cy = self.y(cur);
                    if cy > y || (cy == y && self.x(cur) >= x) {
                        break;
                    }
                }
                prev = cur;
                cur = self.next_of(cur);
            }
            return prev;
        }
        // Outside the window: compare packed bytes 2 (low nibble), 1, 0 = (y, x).
        let want = super::pack_xyz(x, y, 0).to_packed();
        let wy = (self.window[1] + 39) & 0x3ff;
        let wx = (self.window[0] + 39) & 0x3ff;
        let mut prev = HEAD;
        if y > wy || (y == wy && x > wx) {
            let mut p = LAST_CELL;
            while p >= 0 && grid(p) == NONE {
                p -= 1;
            }
            if p >= 0 {
                prev = grid(p);
            }
        }
        let mut cur = self.next_of(prev);
        while cur >= 0 {
            if self.place(cur) == 0 {
                let b = self.packed(cur);
                let key = (b[2] & 0xf, b[1], b[0]);
                if key >= (want[2] & 0xf, want[1], want[0]) {
                    break;
                }
            }
            prev = cur;
            cur = self.next_of(cur);
        }
        if before { prev } else { self.next_of(prev) }
    }

    /// Original: 0x10c00a (objects: the list entry linking to `slot`)
    pub(crate) fn predecessor(&self, slot: Slot) -> Slot {
        let mut p = if self.place(slot) == 0 {
            self.list_pos(self.x(slot), self.y(slot), true)
        } else {
            self.parent(slot)
        };
        while p >= 0 && self.next_of(p) != slot {
            p = self.next_of(p);
        }
        p
    }

    /// Original: 0x10c928 (objects: `slot` is inside `owner`, at any depth)
    pub(crate) fn contained_in(&self, mut slot: Slot, owner: Slot) -> bool {
        loop {
            if self.place(slot) == 0 {
                return false;
            }
            slot = self.parent(slot);
            if slot == owner {
                return true;
            }
        }
    }

    /// Original: 0x10c970 (objects: last list entry of `slot`'s contents, `slot` if empty)
    ///
    /// Walks the run of contained entries after `slot` and keeps the last one inside it.
    pub(crate) fn last_inside(&self, slot: Slot) -> Slot {
        if slot < 0 {
            return NONE;
        }
        let (mut cur, mut last) = (slot, slot);
        loop {
            let n = self.next_of(cur);
            if n < 0 || self.place(n) == 0 {
                return last;
            }
            cur = n;
            if self.contained_in(cur, slot) {
                last = cur;
            }
        }
    }

    /// Original: 0x10a79a (objects: first free object slot if `n` are free, else -1)
    pub(crate) fn free_slot(&self, n: i16) -> Slot {
        if self.free.1 < n { NONE } else { self.free.0 }
    }

    /// Takes the first free object slot off its chain (callers checked [`Self::free_slot`]).
    pub(crate) fn pop_free(&mut self) -> Slot {
        let s = self.free.0;
        self.free.0 = self.next_of(s);
        self.free.1 -= 1;
        s
    }

    /// Grid index of tile (x, y) masked by `mask`, if it lies in the window.
    pub(crate) fn cell(&self, x: u16, y: u16, mask: u16) -> Option<usize> {
        let dx = x.wrapping_sub(self.window[0]) & mask;
        let dy = y.wrapping_sub(self.window[1]) & mask;
        (dx < GRID_SIDE as u16 && dy < GRID_SIDE as u16)
            .then(|| usize::from(dy) * GRID_SIDE + usize::from(dx))
    }

    /// Original: 0x10c0c4 (objects: after unlinking `slot` from (x, y, z), repair its grid cell)
    ///
    /// If the cell starts with `slot`, it now starts with `next` when `next` is contained or
    /// stands on (x, y), else it is empty. `next` = -1 makes it empty (the original reads the
    /// status byte before the table there).
    pub(crate) fn unlink_cell(&mut self, x: u16, y: u16, z: u16, slot: Slot, next: Slot) {
        if self.place(slot) != 0 {
            return;
        }
        let mask = if z != 0 { 0xff } else { 0x3ff };
        let Some(c) = self.cell(x, y, mask) else {
            return;
        };
        if self.grid[c] != slot {
            return;
        }
        let keep = next >= 0 && (self.place(next) != 0 || (self.x(next) == x && self.y(next) == y));
        self.grid[c] = if keep { next } else { NONE };
    }

    /// Original: 0x10dea4 (objects: rebuild the window grid from the list)
    pub fn rebuild_grid(&mut self) {
        self.grid.fill(NONE);
        let mut s = self.next_of(HEAD);
        while s >= 0 {
            if self.place(s) == 0
                && let Some(c) = self.cell(self.x(s), self.y(s), 0x3ff)
                && self.grid[c] == NONE
            {
                self.grid[c] = s;
            }
            s = self.next_of(s);
        }
    }

    /// Links `slot` after `prev`.
    pub(crate) fn link_after(&mut self, prev: Slot, slot: Slot) {
        let n = self.next_of(prev);
        self.set_next(slot, n);
        self.set_next(prev, slot);
    }
}

/// Original: 0x10edb4 (objects: link the actors into the list, unchain used monster slots)
///
/// Called by load state 0x105350 after objlist. Each actor with a type is linked in list
/// order; a used slot 0xe0..0x100 is taken off the monster free chain.
pub fn link_actors(o: &mut Objects) {
    let mut last_free = HEAD;
    for a in 0..ACTOR_SLOTS as Slot {
        if o.tf(a) == 0 {
            if a >= FIRST_TEMP_ACTOR {
                last_free = a;
            }
            continue;
        }
        if a >= FIRST_TEMP_ACTOR {
            if last_free == HEAD {
                o.free_actors.0 = a + 1;
            } else {
                o.set_next(last_free, a + 1);
            }
            o.free_actors.1 -= 1;
        }
        let mut s = HEAD;
        while o.next_of(s) >= 0 && o.compare(o.next_of(s), a) < 0 {
            s = o.next_of(s);
        }
        o.link_after(s, a);
    }
}

/// The object steps of load state 0x105350 in its order: slots reset (0x10ed44, a boot step in
/// the original), the save mode from the data hunk, the actors linked (0x10edb4) and the region
/// at the leader loaded (0x10e1ce). The aggregate's loader already put objlist 0x0600 (the
/// actors' quality / quantity words) into [`super::Objects::qq`]. `main_loop` calls the steps
/// itself to keep 0x106506 between 0x10edb4 and 0x10e1ce; tools and tests call this.
pub fn load_state_objects(s: &mut crate::state::State) {
    let o = &mut s.objects;
    init_slots(o);
    o.save_mode = o.defs.save_mode;
    link_actors(o);
    let at = s.map.centre;
    super::blocks::load_region(s, at.x, at.y, u16::from(at.z));
}
