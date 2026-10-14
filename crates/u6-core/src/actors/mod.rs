//! Actors: turn scheduler, NPC turns, schedules, party step/follow, the clock (WU-P5).
//!
//! Domain state = [`Actors`]: per-actor attributes, movement points, schedules and worktypes,
//! the party and the calendar. Actors are object slots 0..256 ([`crate::objects::ObjectId`]);
//! their positions live only in [`crate::objects::Objects::location`].
//!
//! The arrays sit where 0x105350 reads them from `savegame/objlist` (destination of each read in
//! the original, then the objlist offset). The hunk-13 combat leaves are listed here because
//! time, converse, panels and party code call them (see subsystems.md).
//!
//! Files: `turn` (scheduler 0x1213de and its per-turn bookkeeping), `npc` (the NPC turn 0x1200b0
//! and its worktypes), `path` (path finder, slots, jump home, arrival), `walk` (one step: costs,
//! "may stand here", doors, facing frames), `party` (player step, followers, hazards, heal),
//! `clock` (0x10368a, hourly schedule pick 0x121826), `leaves` (hunk-13 leaves), `tables` (data
//! extracted from `game` at run time), `work` (working globals), `seams` (calls into modules
//! that are not ported yet, each a stub point with an explicit fallback).
//!
//! Integration: `State::new_game` must also set `actors.game = GameTables::from_game(game)`;
//! until then creature flags, work objects, the formation and the sailing tables are empty and
//! NPCs open no doors and find no chairs or beds.
//!
//! Original functions this module will own (milestone 1):
//!
//! - 0x102f3c [party] per-turn party status effects (poison, web)
//! - 0x10368a [time] advance clock: light/status timers, date+wind line
//! - 0x11b1c0 [actors] uses ""Thou art not upon a Sacred~"
//! - 0x11b92a [actors] leaf; callers 2, from 1 subsystems
//! - 0x11b9ae [actors] unnamed; calls 0x10996c (tile at (x,y): window cache, ~)
//! - 0x11ba5e [actors] unnamed; calls 0x10aaca (next object of the iteration)
//! - 0x11c650 [actors] leaf; callers 25, from 6 subsystems
//! - 0x11c6c2 [actors] uses "regurgitated!"
//! - 0x11c76e [actors] step of 0x11c910 (party member follows the leader [~)
//! - 0x11c910 [party] party member follows the leader [likely]
//! - 0x11d4ee [party] player move step (direction, drunk wobble, blocking)
//! - 0x11db7c [actors] unnamed; calls 0x109e74 (view-window flags (blocking/l~)
//! - 0x11dcc2 [actors] unnamed; calls 0x10aaca (next object of the iteration)
//! - 0x11dd56 [actors] unnamed; calls 0x10aaca (next object of the iteration)
//! - 0x11de8c [actors] unnamed; calls 0x10aaca (next object of the iteration)
//! - 0x11e2a6 [actors] leaf; callers 1, from 1 subsystems
//! - 0x11e60c [actors] unnamed; calls 0x10c9d2 (move an object/actor to a map~)
//! - 0x11e7a4 [actors] unnamed; calls 0x10aaca (next object of the iteration)
//! - 0x11e9d6 [actors] leaf; callers 1, from 1 subsystems
//! - 0x11ea54 [actors] unnamed; calls 0x10c9d2 (move an object/actor to a map~)
//! - 0x11ef54 [actors] unnamed; calls 0x151f06 (random number in [lo, hi])
//! - 0x11ef84 [actors] unnamed; calls 0x10996c (tile at (x,y): window cache, ~)
//! - 0x11f0a0 [actors] unnamed; calls 0x10c9d2 (move an object/actor to a map~)
//! - 0x11f1d2 [actors] step of 0x1200b0 (NPC turn: walk to current schedul~)
//! - 0x11f2b0 [actors] unnamed; calls 0x10c9d2 (move an object/actor to a map~)
//! - 0x11f452 [actors] unnamed; calls 0x151f06 (random number in [lo, hi])
//! - 0x11f72e [actors] unnamed; calls 0x151f06 (random number in [lo, hi])
//! - 0x11f804 [actors] step of 0x1200b0 (NPC turn: walk to current schedul~)
//! - 0x11f9c6 [actors] step of 0x1200b0 (NPC turn: walk to current schedul~)
//! - 0x11fd20 [actors] uses ""Thou art under arrest!"  "~"
//! - 0x11ffae [actors] uses "The %s stole some gold!"
//! - 0x120034 [actors] step of 0x1200b0 (NPC turn: walk to current schedul~)
//! - 0x12005a [actors] leaf; callers 2, from 2 subsystems
//! - 0x1200b0 [schedules] NPC turn: walk to current schedule entry / do worktype
//! - 0x12061e [actors] leaf; callers 1, from 1 subsystems
//! - 0x1206e2 [actors] unnamed; 3 callees
//! - 0x120a1c [actors] scan nearby hostiles, combat-state toggle [likely]
//! - 0x120b64 [party] party centre / formation anchor [likely]
//! - 0x121020 [actors] uses "An unending darkness engulf~"
//! - 0x121056 [party] leader / solo-mode bookkeeping per turn [likely]
//! - 0x1213de [actors] turn scheduler: next actor by movement points; NPC turns; tick clock
//! - 0x1217d4 [actors] unnamed; calls 0x120b64 (party centre / formation anch~)
//! - 0x121826 [schedules] hourly: pick each NPC's schedule entry for hour+weekday
//! - 0x121bce [combat] leaf; callers 4, from 3 subsystems
//! - 0x121fca [combat] leaf; callers 7, from 6 subsystems
//! - 0x121ff8 [combat] leaf; callers 5, from 5 subsystems
//! - 0x122048 [combat] leaf; callers 8, from 5 subsystems
//! - 0x1220f0 [combat] leaf; callers 2, from 2 subsystems
//! - 0x122144 [combat] leaf; callers 9, from 7 subsystems
//! - 0x122176 [combat] leaf; callers 5, from 5 subsystems
//! - 0x1224ea [combat] leaf; callers 7, from 3 subsystems
//! - 0x1279d4 [party] uses "has healed."

use u6_formats::schedule::ScheduleEntry;

use crate::objects::{ACTOR_SLOTS, ObjectId};

pub mod clock;
pub mod leaves;
pub mod npc;
pub mod party;
pub mod path;
pub mod seams;
pub mod tables;
pub mod turn;
pub mod walk;
mod work;

#[cfg(test)]
mod real_data;

pub use tables::GameTables;
pub use work::{ActorWork, PathScratch, tail, timer};

/// Party roster slots.
pub const PARTY_SLOTS: usize = 16;

/// Per-actor attribute arrays (index = actor = object slot 0..256).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActorAttrs {
    /// Original: A4+0x250c (word per actor; objlist 0x0600, meaning not identified)
    pub unknown_0600: Vec<u16>,
    /// Original: A4+0x3d0c (objlist 0x0800 status)
    pub status: Vec<u8>,
    /// Original: A4+0x3f0c (objlist 0x0900 strength)
    pub strength: Vec<u8>,
    /// Original: A4+0x400c (objlist 0x0A00 dexterity; subsystems.md: the movement-point refill
    /// table [likely])
    pub dexterity: Vec<u8>,
    /// Original: A4+0x410c (objlist 0x0B00 intelligence)
    pub intelligence: Vec<u8>,
    /// Original: A4+0x440c (word per actor; objlist 0x0C00 experience)
    pub exp: Vec<u16>,
    /// Original: A4+0x460c (objlist 0x0E00 hit points)
    pub hp: Vec<u8>,
    /// Original: 0x139888 (objlist 0x0FF1 level)
    pub level: Vec<u8>,
    /// Original: 0x139388 (objlist 0x10F1: index of the actor's current schedule entry, relative
    /// to its first entry; written by 0x121826)
    pub unknown_10f1: Vec<u8>,
    /// Original: 0x139288 (objlist 0x12F1 combat mode)
    pub combat_mode: Vec<u8>,
    /// Original: A4+0x420c (objlist 0x13F1 magic points)
    pub magic: Vec<u8>,
    /// Original: 0x139688 (word per actor; objlist 0x15F1 base type 10 bits + frame 6 bits)
    pub base_type_frame: Vec<u16>,
    /// Original: A4+0x3e0c (objlist 0x17F1 talk flags)
    pub talk_flags: Vec<u8>,
    /// Original: 0x139488 (objlist 0x18F1: per-worktype operand: the walking slot of a 0x83..0x85
    /// walker, the object-kind index of a 0x82 seeker, the actor itself for worktypes 2..0x0f)
    pub unknown_18f1: Vec<u8>,
    /// Original: 0x139588 (objlist 0x19F1 movement flags)
    pub movement_flags: Vec<u8>,
    /// Original: A4+0x47fd (objlist 0x1AF1: 8 walking slots × 32 path bytes; byte = 4-way
    /// direction in bits 0-1 plus 4 × run length, 0 ends the path; written by 0x11e2a6)
    pub unknown_1af1: Vec<u8>,
}

impl Default for ActorAttrs {
    fn default() -> Self {
        let bytes = || vec![0u8; ACTOR_SLOTS];
        Self {
            unknown_0600: vec![0; ACTOR_SLOTS],
            status: bytes(),
            strength: bytes(),
            dexterity: bytes(),
            intelligence: bytes(),
            exp: vec![0; ACTOR_SLOTS],
            hp: bytes(),
            level: bytes(),
            unknown_10f1: bytes(),
            combat_mode: bytes(),
            magic: bytes(),
            base_type_frame: vec![0; ACTOR_SLOTS],
            talk_flags: bytes(),
            unknown_18f1: bytes(),
            movement_flags: bytes(),
            unknown_1af1: bytes(),
        }
    }
}

/// The party.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Party {
    /// Original: A4+0x470c (16 × 14-byte names; objlist 0x0F00)
    pub names: Vec<String>,
    /// Original: A4+0x47ec (party member actor numbers; objlist 0x0FE0)
    pub members: [u8; PARTY_SLOTS],
    /// Original: A4+0x4b00 (party size; objlist 0x0FF0)
    pub size: u8,
    /// Original: A4-0x7b7a (index into `members` of the member the view follows; objlist
    /// 0x1BFB) [likely]
    pub leader_index: u8,
}

impl Party {
    /// The actor the view follows. The original indexes the roster without a bound; an index past
    /// the 16 slots (never seen in the data) yields `None` here.
    pub fn leader(&self) -> Option<ObjectId> {
        let actor = *self.members.get(usize::from(self.leader_index))?;
        Some(ObjectId(u16::from(actor)))
    }

    /// The first `size` members.
    pub fn active(&self) -> impl Iterator<Item = ObjectId> + '_ {
        let n = usize::from(self.size).min(PARTY_SLOTS);
        self.members[..n].iter().map(|&a| ObjectId(u16::from(a)))
    }
}

/// Schedules: per-actor first-entry table plus all entries.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Schedules {
    /// Original: A4+0x48fe (word per actor: index of its first entry; the next actor's value
    /// ends the range)
    pub first: Vec<u16>,
    /// Original: A4+0x4afe (entry count; bytes 512..514 of `schedule`)
    pub total: u16,
    /// Original: 0x139988 (5 bytes per entry: hour 5 bits, weekday 3 bits, worktype, location)
    pub entries: Vec<ScheduleEntry>,
}

/// The calendar (the original keeps it inside the 0x82-byte objlist block at A4-0x7b84).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Calendar {
    /// Original: A4-0x7b82 (objlist 0x1BF3)
    pub minute: u8,
    /// Original: A4-0x7b81 (objlist 0x1BF4)
    pub hour: u8,
    /// Original: A4-0x7b80 (objlist 0x1BF5)
    pub day: u8,
    /// Original: A4-0x7b7f (objlist 0x1BF6)
    pub month: u8,
    /// Original: A4-0x7b7e (word; objlist 0x1BF7)
    pub year: u16,
}

/// Everything the actor model owns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Actors {
    pub attrs: ActorAttrs,
    /// Original: A4+0x430c (current movement points; objlist 0x14F1)
    pub movement_points: Vec<u8>,
    /// Original: 0x139188 (byte per actor: 0x81 = follow schedule, 0x80 / 0x83..0x86 other
    /// modes; objlist 0x11F1)
    pub worktype: Vec<u8>,
    pub schedules: Schedules,
    pub party: Party,
    pub calendar: Calendar,
    /// Original: A4-0x7b84 (objlist 0x1BF1 quest flag)
    pub quest_flag: u8,
    /// Original: A4-0x2fd2 (word; 0x105350 copies the quest flag here, use not identified)
    ///
    /// Sync rule: written once at load from `quest_flag`; other writers not identified yet.
    pub quest_flag_copy: u16,
    /// Original: A4-0x7b83 (objlist 0x1BF2 rest counter)
    pub rest_counter: u8,
    /// Original: A4-0x7b7c (objlist 0x1BF9 karma)
    pub karma: u8,
    /// Original: A4-0x7b7b (objlist 0x1BFA wind direction)
    pub wind: u8,
    /// Original: A4-0x7b72 (16 bytes; objlist 0x1C03 timers)
    pub timers: [u8; 16],
    /// Original: A4-0x7b84 (the whole 0x82-byte objlist block 0x1BF1..0x1C73 as read). Bytes the
    /// fields above decode are superseded by those fields; the rest is kept for saving and read
    /// through the [`tail`] indices.
    pub objlist_tail: Vec<u8>,
    /// Working globals of the actor code outside the objlist block.
    pub work: ActorWork,
    /// Path-finder scratch (rebuilt on every search, never saved).
    pub path: PathScratch,
    /// Tables extracted from the user's `game` executable at run time.
    pub game: GameTables,
}

impl Default for Actors {
    fn default() -> Self {
        Self {
            attrs: ActorAttrs::default(),
            movement_points: vec![0; ACTOR_SLOTS],
            worktype: vec![0; ACTOR_SLOTS],
            schedules: Schedules::default(),
            party: Party::default(),
            calendar: Calendar::default(),
            quest_flag: 0,
            quest_flag_copy: 0,
            rest_counter: 0,
            karma: 0,
            wind: 0,
            timers: [0; 16],
            objlist_tail: vec![0; 0x82],
            work: ActorWork::default(),
            path: PathScratch::default(),
            game: GameTables::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leader_and_active_members_come_from_the_roster() {
        let mut party = Party {
            members: [1, 2, 3, 4, 4, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            size: 4,
            ..Party::default()
        };
        assert_eq!(party.leader(), Some(ObjectId(1)));
        let active: Vec<u16> = party.active().map(|id| id.0).collect();
        assert_eq!(active, [1, 2, 3, 4]);
        party.leader_index = 2;
        assert_eq!(party.leader(), Some(ObjectId(3)));
        party.leader_index = 16;
        assert_eq!(party.leader(), None);
        party.size = 200;
        assert_eq!(party.active().count(), PARTY_SLOTS);
    }
}
