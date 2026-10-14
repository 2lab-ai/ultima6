//! Working globals of the actor code and the path-finder scratch, plus the named bytes of the
//! objlist block (A4-0x7b84) the actor code reads and writes.

use super::Actors;

/// Byte indices into [`Actors::objlist_tail`] (= A4 offset + 0x7b84; objlist offset = 0x1BF1 +
/// index). Bytes that `Actors` decodes into fields (calendar, timers, karma, wind, leader index,
/// rest counter, quest flag) are not listed: use the fields.
pub mod tail {
    /// A4-0x7b79 (objlist 0x1BFC): light level 0..7 set by the clock [likely].
    pub const LIGHT: usize = 0x0b;
    /// A4-0x7b60 / -0x7b5f (objlist 0x1C15 / 0x1C16): cleared on every hour change.
    pub const HOURLY_A: usize = 0x24;
    pub const HOURLY_B: usize = 0x25;
    /// A4-0x7b5e (objlist 0x1C17 alcohol): drunkenness, minus one per hour.
    pub const ALCOHOL: usize = 0x26;
    /// A4-0x7b5d (objlist 0x1C18): set on an hour change, consumed by the bell ringer (0x98).
    pub const BELL_PENDING: usize = 0x27;
    /// A4-0x7b2a (objlist 0x1C4B, big-endian word): keg timer.
    pub const KEG_TIMER: usize = 0x5a;
    /// A4-0x7b28 (objlist 0x1C4D, big-endian word): object the keg timer acts on.
    pub const KEG_OBJECT: usize = 0x5c;
    /// A4-0x7b24 (objlist 0x1C51, 8 bytes): actor walking in each of the 8 path slots (0 = free).
    pub const SLOT_ACTOR: usize = 0x60;
    /// A4-0x7b1c (objlist 0x1C59, 8 bytes): index of the path byte each slot is on.
    pub const SLOT_POS: usize = 0x68;
    /// A4-0x7b14 (objlist 0x1C61, 8 bytes): steps taken within that path byte.
    pub const SLOT_STEP: usize = 0x70;
    /// A4-0x7b0c (objlist 0x1C69): party combat mode on.
    pub const COMBAT_MODE: usize = 0x78;
    /// A4-0x7b0b (objlist 0x1C6A, signed): solo member index, negative = party mode.
    pub const SOLO_MEMBER: usize = 0x79;
    /// A4-0x7b08..-0x7b05 (objlist 0x1C6D..0x1C70): the two moons' phases and positions.
    pub const MOONS: usize = 0x7c;
    /// Number of path slots.
    pub const SLOTS: usize = 8;
}

/// Timer indices into [`Actors::timers`] (A4-0x7b72 + n) the actor code tests.
pub mod timer {
    /// A4-0x7b6b: doubles the move cost of actors with status bit 0x20.
    pub const SLOW_EVIL: usize = 7;
    /// A4-0x7b68: halves the move cost of party members.
    pub const HASTE_PARTY: usize = 10;
    /// A4-0x7b65: set by the clock when a status object fades [likely].
    pub const FADE: usize = 13;
    /// A4-0x7b64: time stop; NPCs lose their turns while it runs.
    pub const TIME_STOP: usize = 14;
    /// A4-0x7b63: forces the light level computation into its "dark" branch [likely].
    pub const ECLIPSE: usize = 15;
}

/// Working globals (small-data BSS of hunks 1, 12 and 13) the actor code shares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActorWork {
    /// Original: A4-0x7afd (byte: the command loop reprints the prompt when set)
    pub prompt: u8,
    /// Original: A4-0x7afe (byte: party-status counter the clock and command loop bump; data
    /// hunk 1)
    pub status_counter: u8,
    /// Original: A4-0x7af6 (word: set by the clock on a 1-in-8 draw)
    pub random_event: u16,
    /// Original: A4-0x7af8 (word: set by 0x121020; data hunk 1)
    pub darkness: u16,
    /// Original: A4-0x617a (byte: time stop has run out; the data hunk holds 1)
    pub time_stop_over: u8,
    /// Original: A4-0x7ac4 (byte: hour of the last hour-change bookkeeping; data hunk 0xff)
    pub last_hour: u8,
    /// Original: A4-0x779d (byte: bell strikes left for the bell ringer)
    pub bell_strikes: u8,
    /// Original: A4-0x779e (byte: bell strikes computed at the hour change)
    pub hour_strikes: u8,
    /// Original: A4-0x6a96 (word: 0x11b1c0 saw a damaging tile)
    pub saw_damage: u16,
    /// Original: A4-0x6a8a (word: 0x11b1c0 refuses damaging tiles while set)
    pub avoid_damage: u16,
    /// Original: A4-0x6a9c (word: an actor step happened; 0x11ba5e reads it)
    pub stepped: u16,
    /// Original: A4-0x6a9a (word: teleports to schedule locations this turn, max 3)
    pub teleports: u16,
    /// Original: A4-0x6a98 (word: path searches this turn, max 3)
    pub searches: u16,
    /// Original: A4-0x6a9e (word: while set, 0x11e60c teleports even in view)
    pub force_teleport: u16,
    /// Original: A4-0x6a8e (word: set when a swallowed actor is freed)
    pub freed: u16,
    /// Original: A4-0x6a8c (word: party members may swap places in 0x11b1c0; data hunk 1)
    pub swap_ok: u16,
    /// Original: A4-0x6a92 (byte: a drunk "Hic!" is pending; set by 0x11d4ee)
    pub hic: u8,
    /// Original: A4+0x184a (byte: path slot being filled)
    pub slot: u8,
    /// Original: A4+0x1846 (word: leader index kept by 0x1217d4)
    pub leader_kept: u16,
    /// Original: A4-0x6578 (word: set when an invisibility ring wears out; redraw request)
    pub ring_redraw: u16,
    /// Original: A4-0x7a30 (word: the command code being executed; 0x90 / 0x91 = rest)
    pub command: u16,
    /// Original: A4-0x7a3e (word: party index the command applies to)
    pub command_member: u16,
    /// Original: A4-0x6a94 (word: while 0, approach steps refuse damaging tiles)
    pub walk_through_damage: u16,
    /// Original: A4+0x1830 / +0x1832 (words: party centre, set by 0x120b64)
    pub party_centre: (i16, i16),
    /// Original: A4+0x1834 / +0x1836 (words: centre of the hostiles near the party)
    pub enemy_centre: (i16, i16),
    /// Original: A4+0x1838, +0x183a, +0x183c, +0x183e, +0x1840, +0x1842 (words: the party
    /// member furthest toward the hostiles, the first hostile, the party's two flank members,
    /// the hostiles' two flank actors; -1 = none)
    pub front: [i16; 6],
    /// Original: A4+0x1844 (word: hostiles counted near the party)
    pub enemies: i16,
    /// Original: A4-0x6a88 (word: cleared when there are no hostiles)
    pub flag_6a88: u16,
    /// Original: A4+0x184b (17 bytes: the leader then the followers in rank order, built by
    /// 0x11c910)
    pub follow: [u8; 17],
    /// Original: A4-0x6a90 (word: the last player step was blocked)
    pub step_blocked: u16,
    /// Original: A4-0x7a36 (word: walking toward a clicked target; the data hunk holds 0)
    pub mouse_walk: u16,
    /// Original: A4-0x7a2a / -0x7a28 (words: the clicked target; data hunk -1, -1)
    pub mouse_target: (i16, i16),
    /// Original: A4-0x7afc (byte: random 2..4 drawn when a 0x1a8 creature in view turns)
    pub random_byte: u8,
    /// Original: A4-0x7a4c (word: cleared around the portcullis call in 0x11db7c; data hunk 1)
    pub portcullis_guard: u16,
}

impl Default for ActorWork {
    fn default() -> Self {
        Self {
            prompt: 0,
            status_counter: 1,
            random_event: 0,
            darkness: 1,
            time_stop_over: 1,
            last_hour: 0xff,
            bell_strikes: 0,
            hour_strikes: 0,
            saw_damage: 0,
            avoid_damage: 0,
            stepped: 0,
            teleports: 0,
            searches: 0,
            force_teleport: 0,
            freed: 0,
            swap_ok: 1,
            hic: 0,
            slot: 0,
            leader_kept: 0,
            ring_redraw: 0,
            command: 0,
            command_member: 0,
            walk_through_damage: 0,
            party_centre: (0, 0),
            enemy_centre: (0, 0),
            front: [0; 6],
            enemies: 0,
            flag_6a88: 0,
            follow: [0; 17],
            step_blocked: 0,
            mouse_walk: 0,
            mouse_target: (-1, -1),
            random_byte: 0,
            portcullis_guard: 1,
        }
    }
}

/// Side of a 40 × 40 path grid (the map-window cache size).
pub const GRID: usize = 40;
/// Nodes in the path-finder's bucket queue.
pub const NODES: usize = 256;

/// The path-finder's scratch (0x11ea54 and helpers). Rebuilt by every search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathScratch {
    /// Original: 0x13c940 (40 × 40 bytes: cost so far, bit 7 = reached from the goal side;
    /// 0xff = not visited)
    pub grid: Vec<u8>,
    /// Original: 0x13d580 (40 × 40 bytes: cost of entering each cell, 0xff = blocked; built by
    /// 0x1206e2)
    pub cost: Vec<u8>,
    /// Original: 0x13cf80 (word per cost bucket: first queued node, -1 = empty)
    pub heads: Vec<i16>,
    /// Original: 0x13d180 (word per node: next node in its bucket or the free list)
    pub next: Vec<i16>,
    /// Original: 0x13d380 (byte per node: column)
    pub col: Vec<u8>,
    /// Original: 0x13d480 (byte per node: row)
    pub row: Vec<u8>,
    /// Original: A4+0x1824 (word: head of the free node list)
    pub free: i16,
    /// Original: A4+0x1826 (word: the search reached its goal)
    pub found: u16,
    /// Original: A4+0x1828 (word: object type sought, -1 = a location)
    pub seek_type: i16,
    /// Original: A4+0x182a (word: window edge sought 0..3, -1 = none)
    pub seek_edge: i16,
    /// Original: A4+0x182c / +0x182e (words: cell where an edge search ended)
    pub edge_cell: (i16, i16),
    /// Original: A4+0x1848 (byte: bucket being expanded)
    pub bucket: u8,
    /// Original: A4+0x1849 (byte: highest bucket queued)
    pub top: u8,
}

impl Default for PathScratch {
    fn default() -> Self {
        Self {
            grid: vec![0xff; GRID * GRID],
            cost: vec![0; GRID * GRID],
            heads: vec![-1; NODES],
            next: vec![-1; NODES],
            col: vec![0; NODES],
            row: vec![0; NODES],
            free: 0,
            found: 0,
            seek_type: -1,
            seek_edge: -1,
            edge_cell: (0, 0),
            bucket: 0,
            top: 0,
        }
    }
}

impl Actors {
    /// A byte of the objlist block ([`tail`] index).
    pub fn tail(&self, index: usize) -> u8 {
        self.objlist_tail.get(index).copied().unwrap_or(0)
    }

    /// Writes a byte of the objlist block.
    pub fn set_tail(&mut self, index: usize, value: u8) {
        if let Some(b) = self.objlist_tail.get_mut(index) {
            *b = value;
        }
    }

    /// A big-endian word of the objlist block.
    pub fn tail_word(&self, index: usize) -> u16 {
        u16::from_be_bytes([self.tail(index), self.tail(index + 1)])
    }

    /// Writes a big-endian word of the objlist block.
    pub fn set_tail_word(&mut self, index: usize, value: u16) {
        let [hi, lo] = value.to_be_bytes();
        self.set_tail(index, hi);
        self.set_tail(index + 1, lo);
    }

    /// Party combat mode (A4-0x7b0c).
    pub fn combat_mode(&self) -> bool {
        self.tail(tail::COMBAT_MODE) != 0
    }

    /// Solo member index, negative in party mode (A4-0x7b0b).
    pub fn solo_member(&self) -> i8 {
        self.tail(tail::SOLO_MEMBER) as i8
    }

    /// Movement points of `actor` as the signed byte the original reads.
    pub fn mp(&self, actor: usize) -> i8 {
        self.movement_points.get(actor).map_or(0, |&b| b as i8)
    }

    /// Writes the movement points of `actor` (low byte of `v`, as `move.b`).
    pub fn set_mp(&mut self, actor: usize, v: i16) {
        if let Some(b) = self.movement_points.get_mut(actor) {
            *b = v as u8;
        }
    }
}
