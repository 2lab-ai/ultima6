//! Combat mode on and off (the B command, the boot with the combat byte set) and the random
//! jump of an actor (a wisp teleporting, a creature fleeing out of view).

use crate::actors::tail;
use crate::engine::with_current_io;
use crate::state::State;

/// Command-bar icons of combat on / off, drawn over the bar at (0x98, 0xb0).
const ICON_ON: u16 = 0x19f;
const ICON_OFF: u16 = 0x19e;
const ICON_AT: (u16, u16) = (0x98, 0xb0);
/// A4-0x7b0a (objlist block byte 0x7a): the player character's worktype saved meanwhile.
const SOLO_WORK: usize = 0x7a;
/// Song 7 = the combat music.
const COMBAT_SONG: u16 = 7;

fn member(s: &State, i: usize) -> usize {
    usize::from(s.actors.party.members.get(i).copied().unwrap_or(0))
}

fn icon(s: &State, tile: u16) {
    with_current_io(|io| {
        io.draw(|fb| crate::gfx::blit::blit_front(&s.gfx, fb, tile, ICON_AT.0, ICON_AT.1));
        io.present();
    });
}

/// Original: 0x121c64 (combat: begin combat mode)
///
/// The combat byte (A4-0x7b0c) is set; every member not charmed takes its combat mode
/// (0x139288) as worktype, and every member's foe (0x139488) is itself (none). The player's
/// character (the solo member, in party mode the first member able to act) keeps its old
/// worktype in A4-0x7b0a and gets worktype 2. The command bar shows combat on, the combat song
/// starts.
pub fn begin_combat(s: &mut State) {
    s.actors.set_tail(tail::COMBAT_MODE, 1);
    for i in 0..usize::from(s.actors.party.size) {
        let m = member(s, i);
        if s.objects.obj_status[m] & 0x04 == 0 {
            s.actors.worktype[m] = s.actors.attrs.combat_mode[m];
        }
        s.actors.attrs.unknown_18f1[m] = m as u8;
    }
    let solo = s.actors.tail(tail::SOLO_MEMBER) as i8;
    let k = if solo < 0 {
        let size = usize::from(s.actors.party.size);
        let mut k = 0;
        loop {
            let m = member(s, k);
            let able = s.actors.attrs.status[m] & 0x16 == 0
                && s.actors.attrs.movement_flags[m] & 0x10 == 0;
            if able || k >= size {
                break k;
            }
            k += 1;
        }
    } else {
        solo as usize
    };
    let m = member(s, k);
    let w = s.actors.worktype[m];
    s.actors.set_tail(SOLO_WORK, w);
    s.actors.worktype[m] = 2;
    icon(s, ICON_ON);
    super::seams::play_song(s, COMBAT_SONG);
}

/// Original: 0x121d78 (combat: break off combat)
///
/// A4-0x6a88 and the combat byte are cleared. Dead members get worktype 0; the others not
/// charmed follow (1, their rank as operand) in party mode or wait (0) in solo mode. The
/// player's character (the solo member, else the first) saves its worktype in A4-0x7b0a and
/// gets 2. The command bar shows combat off; the area song comes back if the combat song plays.
pub fn break_off(s: &mut State) {
    s.actors.work.flag_6a88 = 0;
    s.actors.set_tail(tail::COMBAT_MODE, 0);
    let solo = s.actors.tail(tail::SOLO_MEMBER) as i8;
    let follow: u8 = if solo < 0 { 1 } else { 0 };
    for i in 0..usize::from(s.actors.party.size) {
        let m = member(s, i);
        if s.actors.attrs.status[m] & 0x10 != 0 {
            s.actors.worktype[m] = 0;
        } else if s.objects.obj_status[m] & 0x04 == 0 {
            s.actors.worktype[m] = follow;
            if follow == 1 {
                s.actors.attrs.unknown_18f1[m] = i as u8;
            }
        }
    }
    let k = if solo < 0 { 0 } else { solo as usize };
    let m = member(s, k);
    let w = s.actors.worktype[m];
    s.actors.set_tail(SOLO_WORK, w);
    s.actors.worktype[m] = 2;
    icon(s, ICON_OFF);
    if super::seams::current_song(s) == COMBAT_SONG {
        super::seams::area_song(s);
    }
}

/// Original: 0x121e88 (combat: move `actor` to a random tile it may stand on within 3 tiles
/// (two `random(1, 4)` per axis), 8 tries; with `away` not inside the 11 × 11 view; true when
/// it moved)
pub fn scatter(s: &mut State, actor: i16, away: bool) -> bool {
    if actor < 0 {
        return false;
    }
    let p = s.objects.location[actor as usize];
    let (x, y, z) = (p.x as i16, p.y as i16, u16::from(p.z));
    for _ in 0..8 {
        let a = s.rng.range(1, 4) as i16;
        let b = s.rng.range(1, 4) as i16;
        let nx = a + b + x - 5;
        let a = s.rng.range(1, 4) as i16;
        let b = s.rng.range(1, 4) as i16;
        let ny = a + b + y - 5;
        if away {
            let c = s.map.centre;
            let (cx, cy) = (c.x as i16, c.y as i16);
            let inside = (cx - 5..=cx + 5).contains(&nx) && (cy - 5..=cy + 5).contains(&ny);
            if inside {
                continue;
            }
        }
        if crate::actors::walk::can_move_to(s, actor as u16, nx as u16, ny as u16) {
            crate::objects::move_to(s, actor, nx as u16, ny as u16, z);
            return true;
        }
    }
    false
}
