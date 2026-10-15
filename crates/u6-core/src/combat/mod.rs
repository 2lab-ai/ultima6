//! Combat: the Attack command, hits, damage, death and experience, combat mode, the NPC combat
//! worktypes, and the missile / hit animations of hunk 18 (M2 unit "combat").
//!
//! Shape of the port (spec 03 §1). The combat core is synchronous code on `&mut State`, like the
//! actor code it is called from (the NPC turn 0x1200b0 runs a worktype inside one borrow). Where
//! the original presents a frame in the middle of an action (a hit flash 0x1230ce, every step of
//! a missile flight 0x132706) the core draws it the way the original does and records what
//! reached the screen in [`CombatState::effects`], marked with the number of message lines
//! queued before it ([`present`]). The async part ([`play_effects`]) shows the recorded frames
//! in order, interleaved with the queued messages, each after the time the original spends
//! drawing it, on the engine clock ([`present::pace`]). The Attack command ([`attack::attack`]) and the NPC turn play them right after the
//! synchronous step that made them, so state changes and pictures keep the original order.
//!
//! Domain state = [`CombatState`]: the hunk-13 working globals and the tables read from `game`
//! ([`tables::CombatTables`]). Globals other modules own are used in place: the attack target
//! (`main.attack_target`), the cross-hair and selection (`ui.input`), the prompt flag and the
//! formation words (`actors.work`).
//!
//! Files: `leaves` (distance, ranges, armour, experience, the actor at a location), `damage`
//! (hit flash, damage, wounds, death), `attack` (the Attack command and one hit), `mode`
//! (begin / break off combat), `npc` (the combat worktypes), `present` (hunk 18 drawing and the
//! effect log), `peer` (the map overview 0x13410a), `seams` (calls into modules not ported yet).
//!
//! Original functions this module owns (hunk 13 except the hunk-13 leaves the actor model owns,
//! plus the hunk-18 animations):
//!
//! - 0x121b08 distance from an actor to a tile (Chebyshev)
//! - 0x121c64 begin combat mode / 0x121d78 break off combat
//! - 0x121e88 move an actor to a random free tile near it
//! - 0x122076 intelligence contest / 0x1220c6 level from experience / 0x122104 add experience
//! - 0x1221da an actor drops (sleep, paralysis, death): lying frame, remains
//! - 0x1225c0 actor at a location / 0x12267a, 0x1226c8 armour / 0x12271e range
//! - 0x12276a sweep hit on an adjacent foe / 0x1228ba missile flight of one shot
//! - 0x122938 a missile weapon's flight and ammunition / 0x122f3e weapon damage
//! - 0x122fbe copy an actor (slime divides) / 0x1230ce hit flash / 0x1231be hurt or kill
//! - 0x12387e to-hit roll / 0x12395a wound level / 0x123a7c pick an object to take
//! - 0x123aec the party turned evil: hostiles / 0x123c56 yells for help
//! - 0x123d86 apply damage and report / 0x1240fa rider of a mount / 0x124232 one hit
//! - 0x1249de, 0x124a98, 0x124caa the Attack command / 0x124e0e cell distance
//! - 0x124e74 pick a weapon / 0x12500a pick a target / 0x125254 attack worktype
//! - 0x1254d6, 0x125938, 0x1259b0, 0x125e26, 0x1263a2, 0x126738, 0x126990, 0x126b28,
//!   0x126cc0, 0x126de8, 0x126f58 the other combat worktypes
//! - 0x13242e, 0x1324ae, 0x1325e4, 0x132706, 0x1331b4 missile flights (hunk 18)
//! - 0x134044, 0x13410a the map overview (hunk 18)

pub mod attack;
pub mod damage;
pub mod leaves;
pub mod mode;
pub mod npc;
pub mod peer;
pub mod present;
pub mod seams;
pub mod tables;

#[cfg(test)]
mod pace_real;
#[cfg(test)]
mod real_data;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_pace;

pub use present::{Effect, play_effects};
pub use tables::CombatTables;

/// Working globals of the combat code (small-data BSS / data of hunks 13 and 18) and its tables.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CombatState {
    /// Tables extracted from the user's `game`.
    pub tables: CombatTables,
    /// Original: A4+0x18ee (8 words: the weapons 0x1249de found readied, in order)
    pub weapons: [i16; 8],
    /// Original: A4-0x68e6 (word: how many of them)
    pub weapon_count: i16,
    /// Original: A4-0x68e4 (word: the one the attack is on)
    pub weapon_index: i16,
    /// Original: A4+0x185c (word: the object a hit lands on / a flight ended at, -1 none)
    pub hit: i16,
    /// Original: A4+0x185e / +0x1860 (words: the last tile a flight passed freely)
    pub landing: (i16, i16),
    /// Original: A4+0x1862..0x1866 / A4+0x1868..0x186c (3 words each: the targets of the three
    /// bolts of a triple crossbow)
    pub spread: ([i16; 3], [i16; 3]),
    /// Original: A4+0x186e (words: actors a flight passed, -1-terminated, at most 63)
    pub passed: Vec<i16>,
    /// Original: A4-0x69a4 (word: hit flashes are drawn; the data hunk holds 1, a blast clears
    /// it, 0x1230ce sets it again)
    pub flash: u16,
    /// Original: A4-0x69a2 (word: the actor that shot the current flight, -1 = none)
    pub shooter: i16,
    /// Original: A4+0x1900 / +0x1902 / +0x1904 / +0x1906 (words: the window rectangle a flight
    /// step changed, x0, y0, x1, y1)
    pub dirty: [i16; 4],
    /// Port-only: frames the synchronous code presented, for [`play_effects`].
    pub effects: Vec<Effect>,
    /// Original: A4-0x7a48 as the Attack command sets it after its newline (newlines are swallowed
    /// until other text): kept here while the lines are queued, applied by [`say`](present::say)
    /// and handed to the text layer when [`play_effects`] has printed them.
    pub swallow: bool,
    /// Port-only: frames [`play_effects`] has put on the screen so far (receipts and tests).
    pub shown: u64,
    /// Port-only: the last combat messages queued ([`present::say`]), for receipts and tests.
    pub said: Vec<present::Said>,
    /// Port-only: the engine time (ms) at which the original would be done with what has been
    /// paced so far ([`present::pace`]).
    pub due_ms: u64,
    /// Port-only: the engine time (ms) at which the last [`present::pace`] returned.
    pub paced_at_ms: u64,
    /// Port-only: how many waits [`present::pace`] took of each length (ms), for receipts.
    pub paced: std::collections::BTreeMap<u16, u32>,
}

impl Default for CombatState {
    fn default() -> Self {
        Self {
            tables: CombatTables::default(),
            weapons: [0; 8],
            weapon_count: 0,
            weapon_index: 0,
            hit: -1,
            landing: (0, 0),
            spread: ([0; 3], [0; 3]),
            passed: vec![-1],
            flash: 1,
            shooter: -1,
            dirty: [0; 4],
            effects: Vec::new(),
            swallow: false,
            shown: 0,
            said: Vec::new(),
            due_ms: 0,
            paced_at_ms: u64::MAX,
            paced: std::collections::BTreeMap::new(),
        }
    }
}

impl CombatState {
    /// The tables read from the user's `game` (`State::new_game` calls it).
    pub fn load(&mut self, game: &[u8]) -> Result<(), u6_formats::FormatError> {
        self.tables = CombatTables::from_game(game)?;
        Ok(())
    }
}
