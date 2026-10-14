//! The hunk-13 leaf helpers the clock, converse, panels and party code call (ported with the
//! actor model, subsystems.md "Dependency order").
//!
//! Bits used here: object flags (A4+0x190c) 0x02 invisible, 0x04 charmed, 0x40 cursed [likely];
//! status (A4+0x3d0c) 0x04 asleep, 0x60 alignment, 0x80 in party; movement flags (0x139588)
//! 0x10 swallowed, 0x60 original alignment [likely].

use crate::objects::ObjectId;
use crate::state::State;

use super::{seams, timer};

/// Object flag: cursed (attributes read 3 lower).
const CURSED: u8 = 0x40;

fn stat_less_curse(s: &State, actor: usize, value: u8) -> u16 {
    let v = u16::from(value);
    if s.objects.obj_status[actor] & CURSED != 0 {
        let w = (v as i16) - 3;
        if w < 1 { 1 } else { w as u16 }
    } else {
        v
    }
}

/// Original: 0x121bce (combat: end a charm: clear the flag, restore alignment and worktype)
///
/// Clears the charmed flag, copies the original alignment bits from the movement flags (party
/// members are forced to "good"), and resets the worktype: the combat worktype (0x139288) in
/// combat mode, else 1 (follow) in party mode or 0 in solo mode.
pub fn uncharm(s: &mut State, actor: u16) {
    let a = usize::from(actor);
    s.objects.obj_status[a] &= !0x04;
    let attrs = &mut s.actors.attrs;
    attrs.status[a] = (attrs.status[a] & 0x9f) | (attrs.movement_flags[a] & 0x60);
    if attrs.status[a] & 0x80 != 0 {
        attrs.status[a] = (attrs.status[a] & 0x9f) | 0x40;
    }
    s.actors.worktype[a] = if s.actors.combat_mode() {
        s.actors.attrs.combat_mode[a]
    } else if s.actors.solo_member() < 0 {
        1
    } else {
        0
    };
}

/// Original: 0x121fca (combat: effective strength, 3 lower when cursed, at least 1)
pub fn strength(s: &State, actor: u16) -> u16 {
    let a = usize::from(actor);
    stat_less_curse(s, a, s.actors.attrs.strength[a])
}

/// Original: 0x121ff8 (combat: effective dexterity: curse, time stop, sleep)
///
/// Cursed: 3 lower (at least 1). During a time stop every non-party actor has 1, and so has
/// every sleeping actor.
pub fn dexterity(s: &State, actor: u16) -> u16 {
    let a = usize::from(actor);
    let mut d = stat_less_curse(s, a, s.actors.attrs.dexterity[a]);
    let status = s.actors.attrs.status[a];
    if s.actors.timers[timer::TIME_STOP] != 0 && status & 0x80 == 0 {
        d = 1;
    }
    if status & 0x04 != 0 {
        d = 1;
    }
    d
}

/// Original: 0x122048 (combat: effective intelligence, 3 lower when cursed, at least 1)
pub fn intelligence(s: &State, actor: u16) -> u16 {
    let a = usize::from(actor);
    stat_less_curse(s, a, s.actors.attrs.intelligence[a])
}

/// Original: 0x1220f0 (combat: experience level byte)
pub fn level(s: &State, actor: u16) -> u8 {
    s.actors.attrs.level[usize::from(actor)]
}

/// Original: 0x122144 (combat: maximum hit points = 30 × level, clamped to 1..=255)
pub fn max_hp(s: &State, actor: u16) -> u16 {
    let hp = u16::from(level(s, actor)) * 30;
    hp.clamp(1, 0xff)
}

/// Original: 0x122176 (combat: maximum magic points by caster type)
///
/// By the actor's object type: 0x19a twice the intelligence, 0x17a the intelligence, 0x179 and
/// 0x182 half of it, any other type 0. (0x19a falls through the 0x17a test, as in the
/// original, and keeps its value.)
pub fn max_magic(s: &State, actor: u16) -> u16 {
    let a = usize::from(actor);
    let int = u16::from(s.actors.attrs.intelligence[a]);
    let t = s.objects.type_frame[a] & 0x3ff;
    let mut m = 0;
    if t == 0x19a {
        m = int * 2;
    }
    if t == 0x17a {
        m = int;
    } else if t == 0x179 || t == 0x182 {
        m = int >> 1;
    }
    m
}

/// Original: 0x1224ea (combat: can `viewer` see `target` (visible, same level, not hidden))
///
/// `target` is any object. Visible = not invisible; an invisible party member is seen by a
/// party member. Object flags exist for actor slots only: past them the original reads the next
/// table, the port reads 0 (visible). Never: a swallowed actor, the object word 0x165 (type 0x165 frame 0), an object
/// whose tile has flags3 bit 0x10, or one on another level.
pub fn can_see(s: &State, viewer: u16, target: u16) -> bool {
    let (v, t) = (usize::from(viewer), usize::from(target));
    let flags = |i: usize| s.objects.obj_status.get(i).copied().unwrap_or(0);
    let mut seen = flags(t) & 0x02 == 0;
    let status = &s.actors.attrs.status;
    if status[v] & 0x80 != 0 && !seen && t < 0x100 && status[t] & 0x80 != 0 {
        seen = true;
    }
    if t < 0x100 && s.actors.attrs.movement_flags[t] & 0x10 != 0 {
        seen = false;
    }
    let word = seams::obj_word(s, ObjectId(target));
    if word == 0x165 {
        seen = false;
    }
    let base = s
        .tables
        .basetile
        .base
        .get(usize::from(word & 0x3ff))
        .copied();
    let tile = base.unwrap_or(0).wrapping_add(word >> 10);
    if s.tables
        .tileflags
        .flags3
        .get(usize::from(tile))
        .copied()
        .unwrap_or(0)
        & 0x10
        != 0
    {
        seen = false;
    }
    if s.objects.location[v].z != seams::obj_pos(s, ObjectId(target)).z {
        seen = false;
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::tail;

    #[test]
    fn curse_lowers_stats_to_at_least_one() {
        let mut s = State::empty();
        s.actors.attrs.strength[5] = 20;
        s.actors.attrs.intelligence[5] = 2;
        assert_eq!(strength(&s, 5), 20);
        s.objects.obj_status[5] = CURSED;
        assert_eq!(strength(&s, 5), 17);
        assert_eq!(intelligence(&s, 5), 1);
    }

    #[test]
    fn dexterity_is_one_in_time_stop_and_sleep() {
        let mut s = State::empty();
        s.actors.attrs.dexterity[7] = 18;
        assert_eq!(dexterity(&s, 7), 18);
        s.actors.timers[timer::TIME_STOP] = 3;
        assert_eq!(dexterity(&s, 7), 1);
        s.actors.attrs.status[7] = 0x80;
        assert_eq!(dexterity(&s, 7), 18);
        s.actors.attrs.status[7] = 0x84;
        assert_eq!(dexterity(&s, 7), 1);
    }

    #[test]
    fn max_hp_and_magic() {
        let mut s = State::empty();
        assert_eq!(max_hp(&s, 1), 1);
        s.actors.attrs.level[1] = 9;
        assert_eq!(max_hp(&s, 1), 255);
        s.actors.attrs.level[1] = 3;
        assert_eq!(max_hp(&s, 1), 90);
        s.actors.attrs.intelligence[1] = 21;
        for (t, m) in [
            (0x19a, 42),
            (0x17a, 21),
            (0x179, 10),
            (0x182, 10),
            (0x100, 0),
        ] {
            s.objects.type_frame[1] = t;
            assert_eq!(max_magic(&s, 1), m, "type {t:#x}");
        }
    }

    #[test]
    fn uncharm_restores_alignment_and_worktype() {
        let mut s = State::empty();
        s.objects.obj_status[9] = 0x04 | 0x01;
        s.actors.attrs.status[9] = 0x20;
        s.actors.attrs.movement_flags[9] = 0x40;
        s.actors.set_tail(tail::SOLO_MEMBER, 0xff);
        uncharm(&mut s, 9);
        assert_eq!(s.objects.obj_status[9], 0x01);
        assert_eq!(s.actors.attrs.status[9], 0x40);
        assert_eq!(s.actors.worktype[9], 1);
        s.actors.set_tail(tail::SOLO_MEMBER, 2);
        s.actors.attrs.status[9] = 0x80;
        uncharm(&mut s, 9);
        assert_eq!(s.actors.attrs.status[9], 0xc0);
        assert_eq!(s.actors.worktype[9], 0);
        s.actors.set_tail(tail::COMBAT_MODE, 1);
        s.actors.attrs.combat_mode[9] = 0x8b;
        uncharm(&mut s, 9);
        assert_eq!(s.actors.worktype[9], 0x8b);
    }

    #[test]
    fn visibility_rules() {
        let mut s = State::empty();
        assert!(can_see(&s, 1, 2));
        s.objects.obj_status[2] = 0x02;
        assert!(!can_see(&s, 1, 2));
        s.actors.attrs.status[1] = 0x80;
        s.actors.attrs.status[2] = 0x80;
        assert!(can_see(&s, 1, 2));
        s.actors.attrs.movement_flags[2] = 0x10;
        assert!(!can_see(&s, 1, 2));
        s.actors.attrs.movement_flags[2] = 0;
        s.objects.location[2].z = 1;
        assert!(!can_see(&s, 1, 2));
    }
}
