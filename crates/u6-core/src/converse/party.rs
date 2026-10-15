//! Actor and party helpers of the converse hunk (0x112b0a .. 0x11304a).

use super::script::{ScriptDst, load_script};
use super::seams;
use super::{Cx, Msg};
use crate::files::Files;
use crate::objects::ObjectId;
use crate::state::State;

/// Object operand meaning "the NPC being talked to".
pub(crate) const SELF_NPC: i16 = 0xeb;
/// Party roster limit 0x112daa checks.
const PARTY_MAX: u8 = 8;
/// Bytes of a party name slot (A4+0x470c + 14 × slot).
const NAME_SLOT: usize = 14;

pub(crate) fn ix(a: i16) -> Option<usize> {
    usize::try_from(a).ok()
}

pub(crate) fn status(s: &State, a: i16) -> u8 {
    ix(a)
        .and_then(|i| s.actors.attrs.status.get(i))
        .copied()
        .unwrap_or(0)
}

pub(crate) fn set_status(s: &mut State, a: i16, v: u8) {
    if let Some(b) = ix(a).and_then(|i| s.actors.attrs.status.get_mut(i)) {
        *b = v;
    }
}

pub(crate) fn obj_type(s: &State, a: i16) -> u16 {
    u16::try_from(a).map_or(0, |a| s.objects.obj_type(ObjectId(a)))
}

/// 0x188 → 0x182 with the frame kept (0x112daa, op 0xcd).
pub(crate) fn wake_type(s: &mut State, a: i16) {
    if let Some(w) = ix(a).and_then(|i| s.objects.type_frame.get_mut(i))
        && *w & 0x3ff == 0x188
    {
        *w = (*w & 0xfc00) + 0x182;
    }
}

fn member(s: &State, i: usize) -> u8 {
    s.actors.party.members.get(i).copied().unwrap_or(0)
}

/// The party sits in a vehicle (the leader index equals the party size).
fn in_vehicle(s: &State) -> bool {
    s.actors.party.leader_index == s.actors.party.size
}

/// Original: 0x112b0a (operand 0xeb → the NPC being talked to)
pub(crate) fn resolve_npc(s: &State, v: i16) -> i16 {
    if v == SELF_NPC { s.converse.npc } else { v }
}

/// Original: 0x112b20 (NPC name from its script; the line 0x112984 when there is none)
///
/// Loads `npc`'s script into a scratch buffer and copies its bytes 2..52 to the name buffer
/// A4+0x13ba (no script: only its first byte is cleared), cut at the first byte with bit 7 set
/// or after the first NUL. A cut at 0 (the script's name is empty) prints the line 0x112984;
/// NPC 0x6e gets a '.' as its third character.
pub(crate) async fn npc_name_from_script(cx: &Cx<'_>, npc: i16) -> Vec<u8> {
    let (name, undefined) = cx.st(|s| name_from_script(s, cx.io.files(), npc));
    if undefined {
        let m = cx.msg(Msg::Undefined);
        seams::print(cx, &m).await;
    }
    name
}

/// The synchronous body of 0x112b20: the name, and whether the line 0x112984 is due.
pub(crate) fn name_from_script(s: &mut State, files: &Files, npc: i16) -> (Vec<u8>, bool) {
    let mut tmp = Vec::new();
    let mut buf = if load_script(s, files, npc, ScriptDst::Other(&mut tmp)) != 0 {
        tmp.get(2..52).map(<[u8]>::to_vec).unwrap_or_default()
    } else {
        vec![0]
    };
    let mut i = 0;
    while i < buf.len() && buf[i] & 0x80 == 0 {
        i += 1;
        if buf[i - 1] == 0 {
            break;
        }
    }
    let undefined = i == 0;
    if !undefined && npc == 0x6e && buf.len() > 2 {
        buf[2] = b'.';
    }
    buf.truncate(i);
    if let Some(nul) = buf.iter().position(|&b| b == 0) {
        buf.truncate(nul);
    }
    s.converse.name_buf = buf.clone();
    (buf, undefined)
}

/// Where 0x112ba4's result points: a party name slot (the first 8 slots are searched; not
/// found = slot 8) for a party member, else 0x112b20's buffer.
pub(crate) fn name_ref(s: &State, npc: i16) -> super::StrRef {
    if status(s, npc) & 0x80 != 0 {
        let slot = (0..usize::from(PARTY_MAX))
            .find(|&i| i16::from(member(s, i)) == npc)
            .unwrap_or(usize::from(PARTY_MAX));
        super::StrRef::PartyName(slot as u8)
    } else {
        super::StrRef::NameBuf
    }
}

/// Original: 0x112ba4 (name of an actor: party name, else from its script)
pub async fn actor_name(cx: &Cx<'_>, npc: i16) -> super::StrRef {
    let r = cx.st(|s| name_ref(s, npc));
    if r == super::StrRef::NameBuf {
        npc_name_from_script(cx, npc).await;
    }
    r
}

/// Original: 0x112bf2 (actor in the 11×11 window around the view centre and on a visible cell)
///
/// In a vehicle every party member counts as present. Otherwise the actor must be strictly
/// within 5 tiles of the centre on both axes; its window cell then counts unless the cell's
/// flag byte is 0, its word is below 0x200 and equals 0xff or 0x1bc.
pub fn on_screen(s: &State, a: i16) -> bool {
    if in_vehicle(s) && status(s, a) & 0x80 != 0 {
        return true;
    }
    let Some(pos) = ix(a).and_then(|i| s.objects.location.get(i)).copied() else {
        return false;
    };
    let (x, y) = (pos.x as i16, pos.y as i16);
    let (cx, cy) = (s.map.centre.x as i16, s.map.centre.y as i16);
    if !(cx - 5 < x && x < cx + 5 && cy - 5 < y && y < cy + 5) {
        return false;
    }
    let (flag, word) = seams::view_cell(s, x - cx + 5, y - cy + 5);
    if flag != 0 || word >= 0x200 {
        return true;
    }
    word != 0xff && word != 0x1bc
}

/// Original: 0x112d72 (word variable 0x17 = party members on screen − 1)
pub(crate) fn count_present(s: &mut State) {
    let n = (0..usize::from(s.actors.party.size))
        .filter(|&i| on_screen(s, i16::from(member(s, i))))
        .count() as i16;
    super::vars::set_int(s, 0x17, (n - 1) as u16);
}

/// Original: 0x112daa (NPC joins the party: 0 joined, 1 in a vehicle, 2 party full)
///
/// Appends the actor and its script name (13 characters at most, as `strncpy(.., 13)` into the
/// 14-byte slot), marks it in the party (status bit 7; alignment bits 5-6 become 0x40 when 0),
/// worktype 1, combat mode 3, and a type 0x188 actor becomes 0x182.
pub async fn join_party(cx: &Cx<'_>, npc: i16) -> i16 {
    let slot = cx.st(|s| {
        if in_vehicle(s) {
            return Err(1);
        }
        if s.actors.party.size == PARTY_MAX {
            return Err(2);
        }
        s.actors.party.size += 1;
        let slot = usize::from(s.actors.party.size) - 1;
        if let Some(m) = s.actors.party.members.get_mut(slot) {
            *m = npc as u8;
        }
        Ok(slot)
    });
    let slot = match slot {
        Ok(slot) => slot,
        Err(code) => return code,
    };
    let name = npc_name_from_script(cx, npc).await;
    cx.st(|s| join_rest(s, npc, slot, name));
    0
}

/// 0x112daa after the name lookup.
fn join_rest(s: &mut State, npc: i16, slot: usize, mut name: Vec<u8>) {
    name.truncate(NAME_SLOT - 1);
    super::vars::write(s, super::StrRef::PartyName(slot as u8), &name);
    let mut st = status(s, npc) | 0x80;
    if st & 0x60 == 0 {
        st = (st & 0x9f) | 0x40;
    }
    set_status(s, npc, st);
    if let Some(i) = ix(npc) {
        if let Some(w) = s.actors.worktype.get_mut(i) {
            *w = 1;
        }
        if let Some(m) = s.actors.attrs.combat_mode.get_mut(i) {
            *m = 3;
        }
    }
    wake_type(s, npc);
}

/// Original: 0x112eb8 (NPC leaves the party: 0 left, 1 in a vehicle, 2 not a member)
///
/// Closes the gap in the roster and the names, clears status bit 7 (alignment 0x40 → 0),
/// worktype 0x8f, then drops what the actor carries (0x10ae52 / 0x10c9d2 / 0x128b0e) and puts
/// the actor itself at its x/y on the view's level.
pub fn leave_party(s: &mut State, npc: i16) -> i16 {
    if in_vehicle(s) {
        return 1;
    }
    let size = usize::from(s.actors.party.size);
    let Some(i) = (0..size).find(|&i| i16::from(member(s, i)) == npc) else {
        return 2;
    };
    for j in i..size {
        let next = member(s, j + 1);
        if let Some(m) = s.actors.party.members.get_mut(j) {
            *m = next;
        }
        let name = super::vars::resolve(s, super::StrRef::PartyName(j as u8 + 1));
        super::vars::write(s, super::StrRef::PartyName(j as u8), &name);
    }
    s.actors.party.size -= 1;
    let mut st = status(s, npc) & 0x7f;
    if let Some(w) = ix(npc).and_then(|i| s.actors.worktype.get_mut(i)) {
        *w = 0x8f;
    }
    if st & 0x60 == 0x40 {
        st &= 0x9f;
    }
    set_status(s, npc, st);
    let pos = ix(npc)
        .and_then(|i| s.objects.location.get(i))
        .copied()
        .unwrap_or_default();
    let z = s.map.centre.z;
    loop {
        let obj = seams::carried_object(s, npc);
        if obj < 0 {
            break;
        }
        seams::move_object(s, obj, pos.x, pos.y, z);
        seams::after_drop(s, obj);
    }
    seams::move_object(s, npc, pos.x, pos.y, z);
    0
}

/// Original: 0x11304a (first party member on screen who has an object of type/quality, or -1)
pub(crate) fn member_with_object(s: &mut State, obj_type: i16, quality: i16) -> i16 {
    for i in 0..usize::from(s.actors.party.size) {
        let m = i16::from(member(s, i));
        if on_screen(s, m) && seams::has_object(s, m, obj_type, quality) != -1 {
            return m;
        }
    }
    -1
}
