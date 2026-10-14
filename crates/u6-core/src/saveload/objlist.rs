//! `savegame/objlist` as the Amiga game reads and writes it: 25 tables in one file, read by
//! 0x105350 (0x134ac4 per table) and written by 0x105716 (0x134afe per table), the same tables
//! in the same order. The file the game writes is 0x1C73 bytes (0x134a54 opens it with
//! MODE_NEWFILE); the shipped `DATA1:objlist` is 0x1D73 and the game reads only its first 0x1C73.

use crate::actors::{Calendar, PARTY_SLOTS};
use crate::objects::ACTOR_SLOTS;
use crate::state::State;

/// Bytes 0x105716 writes and 0x105350 reads.
pub const OBJLIST_LEN: usize = 0x1C73;
/// Bytes per party name (A4+0x470c, 16 × 14).
const NAME_LEN: usize = 14;
/// File offset of the party names (0xe0 bytes).
const NAMES: usize = 0x0F00;
/// File offset of the 0x82-byte block at A4-0x7b84 .. A4-0x7b02.
pub const TAIL: usize = 0x1BF1;
pub const TAIL_LEN: usize = 0x82;

/// The tables in file order: (file offset, length, destination in the original).
pub const TABLES: [(usize, usize, &str); 25] = [
    (0x0000, 0x100, "A4+0x190c object status"),
    (0x0100, 0x300, "0x135588 location"),
    (0x0400, 0x200, "0x137988 type + frame"),
    (0x0600, 0x200, "A4+0x250c quality / quantity"),
    (0x0800, 0x100, "A4+0x3d0c status"),
    (0x0900, 0x100, "A4+0x3f0c strength"),
    (0x0A00, 0x100, "A4+0x400c dexterity"),
    (0x0B00, 0x100, "A4+0x410c intelligence"),
    (0x0C00, 0x200, "A4+0x440c experience"),
    (0x0E00, 0x100, "A4+0x460c hit points"),
    (0x0F00, 0x0e0, "A4+0x470c party names"),
    (0x0FE0, 0x010, "A4+0x47ec party members"),
    (0x0FF0, 0x001, "A4+0x4b00 party size"),
    (0x0FF1, 0x100, "0x139888 level"),
    (0x10F1, 0x100, "0x139388 schedule entry"),
    (0x11F1, 0x100, "0x139188 worktype"),
    (0x12F1, 0x100, "0x139288 combat mode"),
    (0x13F1, 0x100, "A4+0x420c magic"),
    (0x14F1, 0x100, "A4+0x430c movement points"),
    (0x15F1, 0x200, "0x139688 base type + frame"),
    (0x17F1, 0x100, "A4+0x3e0c talk flags"),
    (0x18F1, 0x100, "0x139488 worktype operand"),
    (0x19F1, 0x100, "0x139588 movement flags"),
    (0x1AF1, 0x100, "A4+0x47fd path slots"),
    (TAIL, TAIL_LEN, "A4-0x7b84 block"),
];

/// Indices into the A4-0x7b84 block of the bytes the port keeps as decoded fields (every other
/// byte of the block lives in `Actors::objlist_tail`).
mod tail {
    pub const QUEST_FLAG: usize = 0x00;
    pub const REST_COUNTER: usize = 0x01;
    pub const CLOCK: usize = 0x02;
    pub const YEAR: usize = 0x06;
    pub const KARMA: usize = 0x08;
    pub const WIND: usize = 0x09;
    pub const LEADER_INDEX: usize = 0x0a;
    /// A4-0x7b78 / -0x7b76 / -0x7b74: the view centre, three big-endian words.
    pub const CENTRE: usize = 0x0c;
    pub const TIMERS: usize = 0x12;
}

fn put_words(out: &mut [u8], words: &[u16]) {
    for (w, d) in words.iter().zip(out.chunks_exact_mut(2)) {
        d.copy_from_slice(&w.to_be_bytes());
    }
}

fn get_words(src: &[u8], words: &mut [u16]) {
    for (w, d) in words.iter_mut().zip(src.chunks_exact(2)) {
        *w = u16::from_be_bytes([d[0], d[1]]);
    }
}

fn put_bytes(out: &mut [u8], src: &[u8]) {
    let n = out.len().min(src.len());
    out[..n].copy_from_slice(&src[..n]);
}

/// A name slot decoded the way the game prints it: Latin-1 up to the first NUL.
fn slot_name(slot: &[u8]) -> String {
    slot.iter()
        .take_while(|&&b| b != 0)
        .map(|&b| char::from(b))
        .collect()
}

/// The 14 bytes of name slot `i`: the bytes last read when the name is unchanged (so whatever
/// follows its NUL survives), else the name in Latin-1 and NUL padding.
fn name_slot(s: &State, i: usize) -> [u8; NAME_LEN] {
    let raw = s.saveload.names_raw.get(i * NAME_LEN..(i + 1) * NAME_LEN);
    let name = s.actors.party.names.get(i).map_or("", String::as_str);
    if let Some(raw) = raw.filter(|r| slot_name(r) == name) {
        return raw.try_into().unwrap_or([0; NAME_LEN]);
    }
    let mut out = [0u8; NAME_LEN];
    for (d, c) in out.iter_mut().zip(name.chars()) {
        *d = u32::from(c).min(0xff) as u8;
    }
    out
}

/// The A4-0x7b84 block: `objlist_tail` with the decoded fields written over their bytes.
fn tail_bytes(s: &State) -> [u8; TAIL_LEN] {
    let a = &s.actors;
    let mut t = [0u8; TAIL_LEN];
    put_bytes(&mut t, &a.objlist_tail);
    t[tail::QUEST_FLAG] = a.quest_flag;
    t[tail::REST_COUNTER] = a.rest_counter;
    let c = a.calendar;
    t[tail::CLOCK..tail::CLOCK + 4].copy_from_slice(&[c.minute, c.hour, c.day, c.month]);
    put_words(&mut t[tail::YEAR..tail::YEAR + 2], &[c.year]);
    t[tail::KARMA] = a.karma;
    t[tail::WIND] = a.wind;
    t[tail::LEADER_INDEX] = a.party.leader_index;
    let p = s.map.centre;
    put_words(
        &mut t[tail::CENTRE..tail::CENTRE + 6],
        &[p.x, p.y, u16::from(p.z)],
    );
    t[tail::TIMERS..tail::TIMERS + 16].copy_from_slice(&a.timers);
    t
}

/// The bytes 0x105716 writes: every table from the state, in file order.
pub fn encode(s: &State) -> Vec<u8> {
    let mut f = vec![0u8; OBJLIST_LEN];
    let (o, a) = (&s.objects, &s.actors);
    let at = &a.attrs;
    let n = ACTOR_SLOTS;
    put_bytes(&mut f[0x0000..0x0100], &o.obj_status[..n.min(o.obj_status.len())]);
    for (i, p) in o.location.iter().take(n).enumerate() {
        f[0x0100 + 3 * i..0x0103 + 3 * i].copy_from_slice(&p.to_packed());
    }
    put_words(&mut f[0x0400..0x0600], &o.type_frame[..n.min(o.type_frame.len())]);
    put_words(&mut f[0x0600..0x0800], &o.qq[..n.min(o.qq.len())]);
    put_bytes(&mut f[0x0800..0x0900], &at.status);
    put_bytes(&mut f[0x0900..0x0A00], &at.strength);
    put_bytes(&mut f[0x0A00..0x0B00], &at.dexterity);
    put_bytes(&mut f[0x0B00..0x0C00], &at.intelligence);
    put_words(&mut f[0x0C00..0x0E00], &at.exp);
    put_bytes(&mut f[0x0E00..0x0F00], &at.hp);
    for i in 0..PARTY_SLOTS {
        let at = NAMES + i * NAME_LEN;
        f[at..at + NAME_LEN].copy_from_slice(&name_slot(s, i));
    }
    f[0x0FE0..0x0FF0].copy_from_slice(&a.party.members);
    f[0x0FF0] = a.party.size;
    put_bytes(&mut f[0x0FF1..0x10F1], &at.level);
    put_bytes(&mut f[0x10F1..0x11F1], &at.unknown_10f1);
    put_bytes(&mut f[0x11F1..0x12F1], &a.worktype);
    put_bytes(&mut f[0x12F1..0x13F1], &at.combat_mode);
    put_bytes(&mut f[0x13F1..0x14F1], &at.magic);
    put_bytes(&mut f[0x14F1..0x15F1], &a.movement_points);
    put_words(&mut f[0x15F1..0x17F1], &at.base_type_frame);
    put_bytes(&mut f[0x17F1..0x18F1], &at.talk_flags);
    put_bytes(&mut f[0x18F1..0x19F1], &at.unknown_18f1);
    put_bytes(&mut f[0x19F1..0x1AF1], &at.movement_flags);
    put_bytes(&mut f[0x1AF1..0x1BF1], &at.unknown_1af1);
    f[TAIL..].copy_from_slice(&tail_bytes(s));
    f
}

fn get_bytes(dst: &mut [u8], src: &[u8]) {
    let n = dst.len().min(src.len());
    dst[..n].copy_from_slice(&src[..n]);
}

/// The 25 reads of 0x105350: each table gets the file's bytes. A file shorter than 0x1C73
/// leaves the tables (or the part of one) past its end as they were, as AmigaDOS `Read`
/// returns what the file holds; bytes past 0x1C73 are not read.
pub fn decode(s: &mut State, file: &[u8]) {
    let mut f = encode(s);
    let n = file.len().min(OBJLIST_LEN);
    f[..n].copy_from_slice(&file[..n]);
    let (o, a) = (&mut s.objects, &mut s.actors);
    for i in 0..ACTOR_SLOTS.min(o.obj_status.len()) {
        o.obj_status[i] = f[i];
        let p = 0x0100 + 3 * i;
        o.location[i] = u6_formats::position::Position::from_packed([f[p], f[p + 1], f[p + 2]]);
    }
    let n = ACTOR_SLOTS.min(o.type_frame.len());
    get_words(&f[0x0400..0x0600], &mut o.type_frame[..n]);
    get_words(&f[0x0600..0x0800], &mut o.qq[..n]);
    let at = &mut a.attrs;
    get_bytes(&mut at.status, &f[0x0800..0x0900]);
    get_bytes(&mut at.strength, &f[0x0900..0x0A00]);
    get_bytes(&mut at.dexterity, &f[0x0A00..0x0B00]);
    get_bytes(&mut at.intelligence, &f[0x0B00..0x0C00]);
    get_words(&f[0x0C00..0x0E00], &mut at.exp);
    get_bytes(&mut at.hp, &f[0x0E00..0x0F00]);
    a.party.names = f[NAMES..NAMES + PARTY_SLOTS * NAME_LEN]
        .chunks_exact(NAME_LEN)
        .map(slot_name)
        .collect();
    s.saveload.names_raw = f[NAMES..NAMES + PARTY_SLOTS * NAME_LEN].to_vec();
    a.party.members.copy_from_slice(&f[0x0FE0..0x0FF0]);
    a.party.size = f[0x0FF0];
    get_bytes(&mut at.level, &f[0x0FF1..0x10F1]);
    get_bytes(&mut at.unknown_10f1, &f[0x10F1..0x11F1]);
    get_bytes(&mut a.worktype, &f[0x11F1..0x12F1]);
    get_bytes(&mut at.combat_mode, &f[0x12F1..0x13F1]);
    get_bytes(&mut at.magic, &f[0x13F1..0x14F1]);
    get_bytes(&mut a.movement_points, &f[0x14F1..0x15F1]);
    get_words(&f[0x15F1..0x17F1], &mut at.base_type_frame);
    get_bytes(&mut at.talk_flags, &f[0x17F1..0x18F1]);
    get_bytes(&mut at.unknown_18f1, &f[0x18F1..0x19F1]);
    get_bytes(&mut at.movement_flags, &f[0x19F1..0x1AF1]);
    get_bytes(&mut at.unknown_1af1, &f[0x1AF1..0x1BF1]);
    let t = &f[TAIL..];
    a.objlist_tail = t.to_vec();
    a.quest_flag = t[tail::QUEST_FLAG];
    a.rest_counter = t[tail::REST_COUNTER];
    let c = &t[tail::CLOCK..];
    a.calendar = Calendar {
        minute: c[0],
        hour: c[1],
        day: c[2],
        month: c[3],
        year: u16::from_be_bytes([t[tail::YEAR], t[tail::YEAR + 1]]),
    };
    a.karma = t[tail::KARMA];
    a.wind = t[tail::WIND];
    a.party.leader_index = t[tail::LEADER_INDEX];
    a.timers.copy_from_slice(&t[tail::TIMERS..tail::TIMERS + 16]);
}
