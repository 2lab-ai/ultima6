//! `objlist` — formats.md §6 (7539 = 0x1D73 bytes; actor arrays indexed by actor 0..256;
//! `AM` = actors/actor_manager.cpp). Every field formats.md names is decoded; the whole file is
//! kept in [`ObjList::raw`] so the unread regions (0x18F1, 0x1AF1, 0x1C73.. residue, gaps) survive,
//! and [`ObjList::to_bytes`] writes the named fields back over it.

use crate::bytes;
use crate::error::Result;
use crate::position::Position;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// File size of `objlist`.
pub const OBJLIST_LEN: usize = 0x1D73;
/// Number of actors.
pub const ACTORS: usize = 256;
/// Party slots.
pub const PARTY_SLOTS: usize = 16;
/// Bytes per party name slot (NUL-padded).
pub const PARTY_NAME_LEN: usize = 14;

const OBJ_FLAGS: usize = 0x0000;
const POS: usize = 0x0100;
const OBJ_N: usize = 0x0400;
const UNKNOWN_0600: usize = 0x0600;
const STATUS: usize = 0x0800;
const STR: usize = 0x0900;
const DEX: usize = 0x0A00;
const INT: usize = 0x0B00;
const EXP: usize = 0x0C00;
const HP: usize = 0x0E00;
const PARTY_NAMES: usize = 0x0F00;
const PARTY_ROSTER: usize = 0x0FE0;
const PARTY_SIZE: usize = 0x0FF0;
const LEVEL: usize = 0x0FF1;
const UNKNOWN_10F1: usize = 0x10F1;
const WORKTYPE: usize = 0x11F1;
const COMBAT_MODE: usize = 0x12F1;
const MAGIC: usize = 0x13F1;
const MOVEMENT_POINTS: usize = 0x14F1;
const BASE_OBJ_N: usize = 0x15F1;
const TALK_FLAGS: usize = 0x17F1;
const MOVEMENT_FLAGS: usize = 0x19F1;
const QUEST_FLAG: usize = 0x1BF1;
const REST_COUNTER: usize = 0x1BF2;
const CLOCK: usize = 0x1BF3;
const YEAR: usize = 0x1BF7;
const KARMA: usize = 0x1BF9;
const WIND: usize = 0x1BFA;
const UNKNOWN_1BFB: usize = 0x1BFB;
const TIMERS: usize = 0x1C03;
const VANISH_OBJ: usize = 0x1C13;
const ALCOHOL: usize = 0x1C17;
const MOONSTONES: usize = 0x1C1B;
const KEG_TIMER: usize = 0x1C4B;
const GARGISH_FLAG: usize = 0x1C5F;
const PLAYER_COMBAT_MODE: usize = 0x1C69;
const SOLO_MEMBER: usize = 0x1C6A;
const COMMAND_BAR: usize = 0x1C6C;
const GENDER: usize = 0x1C71;
const AVATAR_PORTRAIT: usize = 0x1C72;

/// Per-actor fields (each from its own 256-entry array).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ActorRecord {
    /// 0x0000 u8 (AM:158-162).
    pub obj_flags: u8,
    /// 0x0100, 3 bytes packed as §5 bytes 1-3 (AM:104-136).
    pub pos: Position,
    /// 0x0400 u16 BE `v & 0x3FF` (AM:140-155).
    pub obj_n: u16,
    /// 0x0400 u16 BE `v >> 10`.
    pub frame: u8,
    /// 0x0600 u16 BE, **inferred** (byte parity only); not read by Nuvie.
    pub unknown_0600: u16,
    /// 0x0800 u8 status flags; alignment = bits 5-6 (AM:166-171).
    pub status: u8,
    /// 0x0900 (AM:198-219).
    pub strength: u8,
    /// 0x0A00.
    pub dexterity: u8,
    /// 0x0B00.
    pub intelligence: u8,
    /// 0x0C00 u16 BE (AM:222-227).
    pub exp: u16,
    /// 0x0E00 (AM:230-235).
    pub hp: u8,
    /// 0x0FF1 (AM:238-243).
    pub level: u8,
    /// 0x10F1, not read by Nuvie.
    pub unknown_10f1: u8,
    /// 0x11F1 (AM:247-317).
    pub worktype: u8,
    /// 0x12F1.
    pub combat_mode: u8,
    /// 0x13F1.
    pub magic: u8,
    /// 0x14F1.
    pub movement_points: u8,
    /// 0x15F1 u16 BE `v & 0x3FF` (AM:175-195).
    pub base_obj_n: u16,
    /// 0x15F1 u16 BE `v >> 10`.
    pub base_frame: u8,
    /// 0x17F1 (AM:280-293).
    pub talk_flags: u8,
    /// 0x19F1.
    pub movement_flags: u8,
}

impl ActorRecord {
    /// Alignment = status bits 5-6.
    pub fn alignment(&self) -> u8 {
        (self.status >> 5) & 0x3
    }
}

/// Party block (core/party.cpp:96-107).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Party {
    /// 0x0F00: 16 × 14-byte NUL-padded names, decoded up to the NUL (Latin-1).
    pub names: Vec<String>,
    /// 0x0FE0: actor number per party slot.
    pub roster: [u8; PARTY_SLOTS],
    /// 0x0FF0: number of members.
    pub size: u8,
}

impl Party {
    /// `(actor, name)` for each of the first `size` slots.
    pub fn members(&self) -> impl Iterator<Item = (u8, &str)> {
        let n = usize::from(self.size).min(PARTY_SLOTS);
        self.roster[..n]
            .iter()
            .zip(&self.names)
            .map(|(&a, s)| (a, s.as_str()))
    }
}

/// Game clock (core/game_clock.cpp:63-71).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct GameClock {
    pub minute: u8,
    pub hour: u8,
    pub day: u8,
    pub month: u8,
    /// u16 BE.
    pub year: u16,
}

/// Single-byte player and session state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PlayerState {
    /// 0x1BF1 (core/player.cpp:85).
    pub quest_flag: u8,
    /// 0x1BF2 (game_clock.cpp:96).
    pub rest_counter: u8,
    /// 0x1BF9 (player.cpp:88).
    pub karma: u8,
    /// 0x1BFA (weather.cpp:102).
    pub wind_direction: u8,
    /// 0x1C17 (player.cpp:91).
    pub alcohol: u8,
    /// 0x1C5F (player.cpp:94-98).
    pub gargish_flag: u8,
    /// 0x1C69.
    pub combat_mode: u8,
    /// 0x1C6A: party member in solo mode, 0xFF = whole party (party.cpp:112).
    pub solo_member: u8,
    /// 0x1C6C (command_bar.cpp:113).
    pub command_bar: u8,
    /// 0x1C71 (player.cpp:100).
    pub gender: u8,
    /// 0x1C72, 1-based (portrait_u6.cpp:80-82).
    pub avatar_portrait: u8,
}

/// The parsed `objlist`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ObjList {
    /// 256 actors, index = actor number.
    pub actors: Vec<ActorRecord>,
    pub party: Party,
    pub clock: GameClock,
    pub player: PlayerState,
    /// 0x1C03 u8[16]; `[15]` = eclipse (game_clock.cpp:90-93).
    pub timers: [u8; 16],
    /// 0x1BFB u16 BE[4], **inferred** (byte parity only); not read by Nuvie.
    pub unknown_1bfb: [u16; 4],
    /// 0x1C13 u16 BE; values > 1023 encode a frame (L: init.lua:57,366-377).
    pub vanish_obj: u16,
    /// 0x1C1B u16 BE[8][3]: moonstone x, y, z (L: init.lua:58,380-389).
    pub moonstones: [[u16; 3]; 8],
    /// 0x1C4B u16 BE (L: init.lua:59,391-392).
    pub keg_timer: u16,
    /// The whole file as read; authoritative for every byte not decoded above.
    pub raw: Vec<u8>,
}

fn be(d: &[u8], off: usize) -> u16 {
    u16::from_be_bytes([d[off], d[off + 1]])
}

fn put_be(d: &mut [u8], off: usize, v: u16) {
    d[off..off + 2].copy_from_slice(&v.to_be_bytes());
}

fn obj_word(obj_n: u16, frame: u8) -> u16 {
    (obj_n & 0x3FF) | u16::from(frame & 0x3F) << 10
}

impl ObjList {
    /// Parses `objlist`. Fails unless the file is exactly 7539 bytes.
    pub fn parse(data: &[u8]) -> Result<Self> {
        bytes::expect_len(data, OBJLIST_LEN, "objlist")?;
        let d = data;
        let actors = (0..ACTORS)
            .map(|a| {
                let v = be(d, OBJ_N + 2 * a);
                let bv = be(d, BASE_OBJ_N + 2 * a);
                let p = POS + 3 * a;
                ActorRecord {
                    obj_flags: d[OBJ_FLAGS + a],
                    pos: Position::from_packed([d[p], d[p + 1], d[p + 2]]),
                    obj_n: v & 0x3FF,
                    frame: (v >> 10) as u8,
                    unknown_0600: be(d, UNKNOWN_0600 + 2 * a),
                    status: d[STATUS + a],
                    strength: d[STR + a],
                    dexterity: d[DEX + a],
                    intelligence: d[INT + a],
                    exp: be(d, EXP + 2 * a),
                    hp: d[HP + a],
                    level: d[LEVEL + a],
                    unknown_10f1: d[UNKNOWN_10F1 + a],
                    worktype: d[WORKTYPE + a],
                    combat_mode: d[COMBAT_MODE + a],
                    magic: d[MAGIC + a],
                    movement_points: d[MOVEMENT_POINTS + a],
                    base_obj_n: bv & 0x3FF,
                    base_frame: (bv >> 10) as u8,
                    talk_flags: d[TALK_FLAGS + a],
                    movement_flags: d[MOVEMENT_FLAGS + a],
                }
            })
            .collect();
        let names = (0..PARTY_SLOTS)
            .map(|i| {
                let at = PARTY_NAMES + i * PARTY_NAME_LEN;
                bytes::latin1_until_nul(&d[at..at + PARTY_NAME_LEN])
            })
            .collect();
        let party = Party {
            names,
            roster: std::array::from_fn(|i| d[PARTY_ROSTER + i]),
            size: d[PARTY_SIZE],
        };
        let clock = GameClock {
            minute: d[CLOCK],
            hour: d[CLOCK + 1],
            day: d[CLOCK + 2],
            month: d[CLOCK + 3],
            year: be(d, YEAR),
        };
        let player = PlayerState {
            quest_flag: d[QUEST_FLAG],
            rest_counter: d[REST_COUNTER],
            karma: d[KARMA],
            wind_direction: d[WIND],
            alcohol: d[ALCOHOL],
            gargish_flag: d[GARGISH_FLAG],
            combat_mode: d[PLAYER_COMBAT_MODE],
            solo_member: d[SOLO_MEMBER],
            command_bar: d[COMMAND_BAR],
            gender: d[GENDER],
            avatar_portrait: d[AVATAR_PORTRAIT],
        };
        Ok(ObjList {
            actors,
            party,
            clock,
            player,
            timers: std::array::from_fn(|i| d[TIMERS + i]),
            unknown_1bfb: std::array::from_fn(|i| be(d, UNKNOWN_1BFB + 2 * i)),
            vanish_obj: be(d, VANISH_OBJ),
            moonstones: std::array::from_fn(|m| {
                std::array::from_fn(|k| be(d, MOONSTONES + 6 * m + 2 * k))
            }),
            keg_timer: be(d, KEG_TIMER),
            raw: data.to_vec(),
        })
    }

    /// Eclipse timer (`timers[15]`, 0x1C12).
    pub fn eclipse(&self) -> u8 {
        self.timers[15]
    }
}

impl ObjList {
    /// The file bytes: [`ObjList::raw`] with every decoded field written back. A party name is
    /// rewritten only when it differs from the raw slot (≤ 13 chars + NUL padding, chars above
    /// U+00FF become `?`), so unchanged slots keep their padding bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut d = self.raw.clone();
        d.resize(OBJLIST_LEN, 0);
        for (a, r) in self.actors.iter().enumerate().take(ACTORS) {
            d[OBJ_FLAGS + a] = r.obj_flags;
            d[POS + 3 * a..POS + 3 * a + 3].copy_from_slice(&r.pos.to_packed());
            put_be(&mut d, OBJ_N + 2 * a, obj_word(r.obj_n, r.frame));
            put_be(&mut d, UNKNOWN_0600 + 2 * a, r.unknown_0600);
            d[STATUS + a] = r.status;
            d[STR + a] = r.strength;
            d[DEX + a] = r.dexterity;
            d[INT + a] = r.intelligence;
            put_be(&mut d, EXP + 2 * a, r.exp);
            d[HP + a] = r.hp;
            d[LEVEL + a] = r.level;
            d[UNKNOWN_10F1 + a] = r.unknown_10f1;
            d[WORKTYPE + a] = r.worktype;
            d[COMBAT_MODE + a] = r.combat_mode;
            d[MAGIC + a] = r.magic;
            d[MOVEMENT_POINTS + a] = r.movement_points;
            put_be(
                &mut d,
                BASE_OBJ_N + 2 * a,
                obj_word(r.base_obj_n, r.base_frame),
            );
            d[TALK_FLAGS + a] = r.talk_flags;
            d[MOVEMENT_FLAGS + a] = r.movement_flags;
        }
        for (i, name) in self.party.names.iter().enumerate().take(PARTY_SLOTS) {
            let slot = &mut d[PARTY_NAMES + i * PARTY_NAME_LEN..][..PARTY_NAME_LEN];
            if bytes::latin1_until_nul(slot) != *name {
                slot.fill(0);
                for (dst, c) in slot.iter_mut().zip(name.chars().take(PARTY_NAME_LEN - 1)) {
                    *dst = u8::try_from(u32::from(c)).unwrap_or(b'?');
                }
            }
        }
        d[PARTY_ROSTER..PARTY_ROSTER + PARTY_SLOTS].copy_from_slice(&self.party.roster);
        d[PARTY_SIZE] = self.party.size;
        let c = &self.clock;
        d[CLOCK..CLOCK + 4].copy_from_slice(&[c.minute, c.hour, c.day, c.month]);
        put_be(&mut d, YEAR, c.year);
        let p = &self.player;
        for (off, v) in [
            (QUEST_FLAG, p.quest_flag),
            (REST_COUNTER, p.rest_counter),
            (KARMA, p.karma),
            (WIND, p.wind_direction),
            (ALCOHOL, p.alcohol),
            (GARGISH_FLAG, p.gargish_flag),
            (PLAYER_COMBAT_MODE, p.combat_mode),
            (SOLO_MEMBER, p.solo_member),
            (COMMAND_BAR, p.command_bar),
            (GENDER, p.gender),
            (AVATAR_PORTRAIT, p.avatar_portrait),
        ] {
            d[off] = v;
        }
        d[TIMERS..TIMERS + 16].copy_from_slice(&self.timers);
        for (i, &v) in self.unknown_1bfb.iter().enumerate() {
            put_be(&mut d, UNKNOWN_1BFB + 2 * i, v);
        }
        put_be(&mut d, VANISH_OBJ, self.vanish_obj);
        for (m, xyz) in self.moonstones.iter().enumerate() {
            for (k, &v) in xyz.iter().enumerate() {
                put_be(&mut d, MOONSTONES + 6 * m + 2 * k, v);
            }
        }
        put_be(&mut d, KEG_TIMER, self.keg_timer);
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A zeroed objlist carrying the formats.md §6 proof values.
    fn sample() -> Vec<u8> {
        let mut d = vec![0u8; OBJLIST_LEN];
        // Actor 1 = (307,352,0) obj 410 (OBJ_U6_AVATAR) frame 3.
        d[POS + 3..POS + 6].copy_from_slice(
            &Position {
                x: 307,
                y: 352,
                z: 0,
            }
            .to_packed(),
        );
        put_be(&mut d, OBJ_N + 2, obj_word(410, 3));
        // Actors 2-4 exp 374/373/372.
        for (a, e) in [(2, 374), (3, 373), (4, 372)] {
            put_be(&mut d, EXP + 2 * a, e);
        }
        d[STATUS + 1] = 0x40; // alignment 2
        d[STR + 1] = 25;
        d[LEVEL + 255] = 8;
        d[MOVEMENT_FLAGS + 255] = 0x5A;
        put_be(&mut d, BASE_OBJ_N + 2 * 7, obj_word(387, 2));
        for (i, n) in ["Avatar", "Dupre", "Shamino", "Iolo"].iter().enumerate() {
            d[PARTY_NAMES + i * 14..][..n.len()].copy_from_slice(n.as_bytes());
        }
        d[PARTY_ROSTER..PARTY_ROSTER + 4].copy_from_slice(&[1, 2, 3, 4]);
        d[PARTY_SIZE] = 4;
        d[CLOCK..CLOCK + 4].copy_from_slice(&[0, 8, 4, 7]);
        d[YEAR..YEAR + 2].copy_from_slice(&[0x00, 0xa1]);
        put_be(&mut d, MOONSTONES, 935);
        put_be(&mut d, MOONSTONES + 2, 262);
        d[UNKNOWN_1BFB..UNKNOWN_1BFB + 8].copy_from_slice(&[0, 7, 0, 0xc5, 0, 0xe2, 0, 0]);
        d[TIMERS + 15] = 9;
        d[SOLO_MEMBER] = 0xFF;
        d[AVATAR_PORTRAIT] = 3;
        d[OBJLIST_LEN - 1] = 0x77; // residue byte, never decoded
        d
    }

    #[test]
    fn decodes_named_fields() {
        let o = ObjList::parse(&sample()).unwrap();
        let a1 = o.actors[1];
        assert_eq!(
            (a1.pos.x, a1.pos.y, a1.pos.z, a1.obj_n, a1.frame),
            (307, 352, 0, 410, 3)
        );
        assert_eq!((a1.alignment(), a1.strength), (2, 25));
        let exp: Vec<_> = o.actors[2..5].iter().map(|a| a.exp).collect();
        assert_eq!(exp, [374, 373, 372]);
        assert_eq!(
            (o.actors[255].level, o.actors[255].movement_flags),
            (8, 0x5A)
        );
        assert_eq!((o.actors[7].base_obj_n, o.actors[7].base_frame), (387, 2));
        let members: Vec<_> = o.party.members().collect();
        assert_eq!(
            members,
            [(1, "Avatar"), (2, "Dupre"), (3, "Shamino"), (4, "Iolo")]
        );
        assert_eq!(o.party.names[4], "");
        assert_eq!(
            (o.clock.hour, o.clock.day, o.clock.month, o.clock.year),
            (8, 4, 7, 161)
        );
        assert_eq!(o.moonstones[0], [935, 262, 0]);
        assert_eq!(o.unknown_1bfb, [7, 0xc5, 0xe2, 0]);
        assert_eq!(
            (o.eclipse(), o.player.solo_member, o.player.avatar_portrait),
            (9, 0xFF, 3)
        );
    }

    #[test]
    fn to_bytes_round_trips_and_writes_edits() {
        let d = sample();
        let mut o = ObjList::parse(&d).unwrap();
        assert_eq!(o.to_bytes(), d);
        o.actors[2].exp = 0x1234;
        o.clock.year = 162;
        o.party.names[1] = "Sentri".into();
        let e = o.to_bytes();
        assert_eq!(&e[EXP + 4..EXP + 6], &[0x12, 0x34]);
        assert_eq!(&e[YEAR..YEAR + 2], &[0x00, 0xa2]);
        assert_eq!(&e[PARTY_NAMES + 14..PARTY_NAMES + 21], b"Sentri\0");
        assert_eq!(e[OBJLIST_LEN - 1], 0x77);
        assert_eq!(ObjList::parse(&e).unwrap().party.names[1], "Sentri");
    }

    #[test]
    fn rejects_wrong_length() {
        assert!(ObjList::parse(&[0u8; OBJLIST_LEN - 1]).is_err());
        assert!(ObjList::parse(&[0u8; OBJLIST_LEN + 1]).is_err());
    }
}
