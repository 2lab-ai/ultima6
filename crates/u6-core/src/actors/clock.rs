//! The clock (0x10368a) and the hourly schedule pick (0x121826).

use crate::objects::ObjectId;
use crate::state::State;

use super::seams::{self, Arg};
use super::{leaves, tail, timer, walk};

/// Hours in a day, days in a month, months in a year (the calendar wraps after these).
const HOURS: u8 = 24;
const DAYS: u8 = 28;
const MONTHS: u8 = 12;

/// Original: 0x121826 (schedules: pick each NPC's schedule entry for the hour and weekday)
///
/// For actors 2..0xdf: worktype 0x9b becomes 0x81 first. An existing actor in a schedule
/// worktype (≥ 0x80, or 7) and not in the party scans its entries from the last down to the
/// first and takes the first (= last listed) whose hour is the current hour and whose weekday
/// is today's ((day − 1) % 7 + 1) or 0 = any. It stores the entry, sets worktype 0x81 (go to
/// the scheduled place); a 0x188 actor whose new entry is not 0x95 gets its base type back and
/// faces south.
pub fn pick_schedules(s: &mut State) {
    let hour = s.actors.calendar.hour;
    let weekday = ((i16::from(s.actors.calendar.day) - 1) % 7 + 1) as u8;
    for a in 2..0xe0usize {
        if s.actors.worktype[a] == 0x9b {
            s.actors.worktype[a] = 0x81;
        }
        let wt = s.actors.worktype[a];
        if s.objects.type_frame[a] == 0
            || (wt < 0x80 && wt != 7)
            || s.actors.attrs.status[a] & 0x80 != 0
        {
            continue;
        }
        let first_of = |i: usize| s.actors.schedules.first.get(i).copied().unwrap_or(0) as i16;
        let first = first_of(a);
        let last = first_of(a + 1).wrapping_sub(1);
        let mut i = last;
        while i >= first {
            let Some(e) = s.actors.schedules.entries.get(i as usize).copied() else {
                break;
            };
            if e.hour == hour && (e.day_of_week == weekday || e.day_of_week == 0) {
                s.actors.attrs.unknown_10f1[a] = (i - first) as u8;
                s.actors.worktype[a] = 0x81;
                if s.objects.type_frame[a] & 0x3ff == 0x188 && e.worktype != 0x95 {
                    s.objects.type_frame[a] = s.actors.attrs.base_type_frame[a];
                    walk::face(s, a as u16, 4);
                }
                break;
            }
            i -= 1;
        }
    }
}

/// Flags 0x10368a keeps across its actor loop (frame locals -0xe, -0x12, -0x14, -0x16).
#[derive(Default)]
struct TickFlags {
    /// A carried light source still burns.
    burning: bool,
    /// The actor carries an invisibility ring (its invisibility does not wear off). Like the
    /// original, it is reset only for party members and carries over to later actors.
    ring: bool,
    /// The carried object being looked at vanishes.
    vanish: bool,
    /// A cloak (0x51) lasts (sets timer 13 after the loop).
    amulet: bool,
}

/// Original: 0x10368a (time: advance the clock by `minutes`: timers, carried lights, status
/// wear-off, the calendar, hourly schedules, sky, light level, date line)
///
/// While a time stop runs it only counts down. Otherwise, in the original order: the 16
/// timers, the keg timer, a 1-in-8 random event flag, then per existing actor: (party members)
/// the carried torches, invisibility rings, regeneration rings and cloaks wear; then, once per
/// minute, invisibility, protection, curse, charm, paralysis and sleep may wear off and poison
/// may hurt (each a fixed die against an attribute), then the per-actor tick 0x1032b6. Then the
/// calendar; for every hour crossed: rest counter and alcohol down, [`pick_schedules`], party
/// casters regain magic. On an hour change: bell, sky tile, moons, sun strip, moongates. Then
/// the wind, the light level (0 at night, in dungeons 1..4 and during eclipses; ramps at 5 and
/// 19 o'clock; at least 4 with a light source) and the date line.
pub fn tick(s: &mut State, minutes: i16) {
    let ts = s.actors.timers[timer::TIME_STOP];
    if ts != 0 {
        if minutes >= i16::from(ts) {
            s.actors.work.time_stop_over = 1;
            s.actors.timers[timer::TIME_STOP] = 0;
        } else {
            s.actors.work.time_stop_over = 0;
            s.actors.timers[timer::TIME_STOP] = (i16::from(ts) - minutes) as u8;
            return;
        }
    }
    for t in s.actors.timers.iter_mut() {
        if *t != 0 {
            *t = if minutes >= i16::from(*t) {
                0
            } else {
                (i16::from(*t) - minutes) as u8
            };
        }
    }
    let keg = s.actors.tail_word(tail::KEG_TIMER) as i16;
    if keg > minutes {
        s.actors
            .set_tail_word(tail::KEG_TIMER, (keg - minutes) as u16);
    } else if keg != 0 {
        s.actors.set_tail_word(tail::KEG_TIMER, 0);
        let obj = s.actors.tail_word(tail::KEG_OBJECT);
        keg_expires(s, obj);
    }
    if s.rng.range(0, 7) == 0 {
        s.objects.draw.volcano_probe = 1;
    }
    let mut f = TickFlags::default();
    for actor in 0..0x100u16 {
        let a = usize::from(actor);
        if s.objects.type_frame[a] == 0 {
            continue;
        }
        if s.actors.attrs.status[a] & 0x80 != 0 {
            f.ring = false;
            f.vanish = false;
            carried_items(s, actor, minutes, &mut f);
        }
        for _ in 0..minutes.max(0) {
            wear_off(s, actor, f.ring);
        }
        seams::actor_status_tick(s, actor);
        s.actors.attrs.movement_flags[a] &= !0x08;
    }
    if f.amulet {
        s.actors.timers[timer::FADE] = 1;
    }
    hours_pass(s, minutes, &f);
}

/// The torches, rings and cloaks a party member carries (0x10368a, 0x10ae52 iteration).
fn carried_items(s: &mut State, actor: u16, minutes: i16, f: &mut TickFlags) {
    // A4 string offsets the original prints (the text is game data in `game`).
    const TORCH_OUT: i32 = -0x756f;
    const VANISHED: i32 = -0x7354;
    // The two item names the "vanished" line uses (PC-relative strings in hunk 1).
    const RING_NAME: u32 = 0x101ef6;
    const CLOAK_NAME: u32 = 0x101efc;
    let a = usize::from(actor);
    for obj in seams::contents(s, ObjectId(actor)) {
        let flags = s.objects.obj_status.get(obj.index()).copied().unwrap_or(0);
        if flags & 0x18 == 0x10 {
            continue;
        }
        let mut name = RING_NAME;
        let word = seams::obj_word(s, obj);
        match word & 0x3ff {
            0x5a if word >> 10 == 1 => {
                if s.rng.range(0, 1) != 0 {
                    let left = seams::obj_quality(s, obj);
                    if minutes >= i16::from(left) {
                        seams::print(s, TORCH_OUT, &[]);
                        seams::remove_object(s, obj);
                        s.actors.work.prompt = 1;
                    } else {
                        seams::set_obj_quality(s, obj, (i16::from(left) - minutes) as u8);
                        f.burning = true;
                    }
                } else {
                    f.burning = true;
                }
            }
            0x102 => {
                if s.rng.range(0, 999) == 0x1ad {
                    s.objects.obj_status[a] &= !0x02;
                    s.gfx.dissolve = 1;
                    f.vanish = true;
                    seams::present_map(s);
                } else {
                    f.ring = true;
                }
            }
            0x101 => {
                let regen_all = s.ui.panel.mode == 0x91;
                let regen_one = s.ui.panel.mode == 0x90
                    && s.actors.party.members.get(s.ui.panel.member as usize)
                        == Some(&(actor as u8));
                if (regen_all || regen_one)
                    && u16::from(s.actors.attrs.hp[a]) < leaves::max_hp(s, actor)
                {
                    s.ui.panel.dirty = true;
                }
                let hp = i16::from(s.actors.attrs.hp[a]) + minutes;
                let max = leaves::max_hp(s, actor);
                s.actors.attrs.hp[a] = if hp < max as i16 { hp as u8 } else { max as u8 };
                if s.rng.range(0, 999) == 0x2de {
                    f.vanish = true;
                }
            }
            0x51 => {
                if s.rng.range(0, 999) == 0x24c {
                    name = CLOAK_NAME;
                    s.actors.timers[timer::FADE] = 0;
                    f.vanish = true;
                } else {
                    f.amulet = true;
                }
            }
            _ => {}
        }
        if f.vanish {
            seams::print(s, VANISHED, &[Arg::Image(name)]);
            seams::remove_object(s, obj);
            s.actors.work.prompt = 1;
            f.vanish = false;
        }
    }
}

/// One minute of status wear-off for `actor` (0x10368a's inner loop). Each test draws its die
/// only when the condition holds; the attribute roll is `random(1, 30)`.
fn wear_off(s: &mut State, actor: u16, ring: bool) {
    let a = usize::from(actor);
    if s.objects.obj_status[a] & 0x02 != 0 && !ring && s.rng.range(0, 63) == 0 {
        let v = leaves::intelligence(s, actor) as i16;
        if v <= s.rng.range(1, 30) as i16 {
            s.objects.obj_status[a] &= !0x02;
        }
    }
    if s.actors.attrs.status[a] & 0x01 != 0 && s.rng.range(0, 63) == 0 {
        let v = leaves::intelligence(s, actor) as i16;
        if v <= s.rng.range(1, 30) as i16 {
            s.actors.attrs.status[a] &= !0x01;
        }
    }
    if s.objects.obj_status[a] & 0x40 != 0 && s.rng.range(0, 15) == 0 {
        let v = leaves::intelligence(s, actor) as i16;
        if v >= s.rng.range(1, 30) as i16 {
            s.objects.obj_status[a] &= !0x40;
        }
    }
    if s.objects.obj_status[a] & 0x04 != 0 && s.rng.range(0, 7) == 0 {
        let v = leaves::intelligence(s, actor) as i16;
        if v >= s.rng.range(1, 30) as i16 {
            leaves::uncharm(s, actor);
        }
    }
    if s.actors.attrs.status[a] & 0x02 != 0 && s.rng.range(0, 3) == 0 {
        let v = leaves::strength(s, actor) as i16;
        if v >= s.rng.range(1, 30) as i16 {
            s.actors.attrs.status[a] &= !0x02;
        }
    }
    if s.actors.attrs.status[a] & 0x04 != 0
        && s.actors.worktype[a] != 0x91
        && s.rng.range(0, 15) == 0
    {
        s.actors.attrs.status[a] &= !0x04;
        s.actors.attrs.movement_flags[a] |= 0x08;
        s.objects.type_frame[a] = s.actors.attrs.base_type_frame[a];
    }
    let st = s.actors.attrs.status[a];
    if st & 0x08 != 0 && st & 0x01 == 0 && s.rng.range(0, 7) == 0 {
        hurt(s, actor, 1);
    }
}

/// [combat] 0x1231be (damage / death; "Slime divides!"): the actor loses `damage` hit points.
pub fn hurt(s: &mut State, actor: u16, damage: u8) {
    crate::combat::damage::hurt(s, actor as i16, i16::from(damage));
}

/// [objects] 0x12db7e (the keg / timed object effect). Stub contract: argument the object of
/// the keg timer; no result; nothing happens. The start state has no keg timer.
fn keg_expires(_s: &mut State, _obj: u16) {
    crate::todo_stub!("0x12db7e keg timer runs out (fallback: none)", ())
}

/// The calendar part of 0x10368a, after the actor loop.
fn hours_pass(s: &mut State, minutes: i16, f: &TickFlags) {
    let mut m = i16::from(s.actors.calendar.minute).wrapping_add(minutes);
    while m > 59 {
        m -= 60;
        let c = &mut s.actors.calendar;
        c.hour = c.hour.wrapping_add(1);
        if c.hour > HOURS - 1 {
            c.hour = 0;
            c.day = c.day.wrapping_add(1);
            if c.day > DAYS {
                c.day = 1;
                c.month = c.month.wrapping_add(1);
                if c.month > MONTHS {
                    c.month = 1;
                    c.year = c.year.wrapping_add(1);
                }
            }
        }
        if s.actors.rest_counter != 0 {
            s.actors.rest_counter -= 1;
        }
        let alcohol = s.actors.tail(tail::ALCOHOL);
        if alcohol != 0 {
            s.actors.set_tail(tail::ALCOHOL, alcohol - 1);
        }
        pick_schedules(s);
        regain_magic(s);
    }
    let hour = s.actors.calendar.hour;
    if hour != s.actors.work.last_hour {
        hour_changed(s, hour);
    }
    seams::wind_change(s, 0);
    s.actors.calendar.minute = m as u8;
    set_light(s, f);
    date_line(s);
}

/// Party casters (types 0x19a, 0x17a, 0x179, 0x182) regain their level in magic points, up to
/// [`leaves::max_magic`]; each bumps the party-status counter.
fn regain_magic(s: &mut State) {
    for i in 0..usize::from(s.actors.party.size) {
        let actor = u16::from(s.actors.party.members[i]);
        let a = usize::from(actor);
        let t = s.objects.type_frame[a] & 0x3ff;
        if matches!(t, 0x19a | 0x17a | 0x179 | 0x182) {
            let lvl = s.actors.attrs.level[a];
            let mp = &mut s.actors.attrs.magic[a];
            *mp = mp.wrapping_add(lvl);
            let max = leaves::max_magic(s, actor);
            if u16::from(s.actors.attrs.magic[a]) > max {
                s.actors.attrs.magic[a] = max as u8;
            }
            s.ui.panel.dirty = true;
        }
    }
}

/// The hour-change bookkeeping of 0x10368a: bell strikes, sky tile, moon phases, strip,
/// moongates.
fn hour_changed(s: &mut State, hour: u8) {
    if s.actors.work.last_hour != 0xff {
        s.actors.set_tail(tail::HOURLY_B, 0);
        s.actors.set_tail(tail::HOURLY_A, 0);
        s.actors.set_tail(tail::BELL_PENDING, 1);
        let h = hour % 12;
        s.actors.work.hour_strikes = if h == 0 { 12 } else { h };
    }
    s.actors.work.last_hour = hour;
    let frame = if !(5..=20).contains(&hour) {
        0x13c
    } else {
        ((u16::from(hour) - 5) >> 1) + 0x148
    };
    // The word at 0x13ab16 is the base tile of object type 0xeb (basetile 0x13a940 + 2 × 0xeb).
    let sky = s.tables.basetile.base.get(0xeb).copied().unwrap_or(0);
    seams::sky_tile(s, sky, frame);
    let day = s.actors.calendar.day;
    let h = i16::from(hour);
    for (k, add) in [(0, 0x12i16), (1, 0x14)] {
        let phase = s.actors.game.moon(day, k);
        s.actors.set_tail(tail::MOONS + 2 * k, phase);
        let pos = (i16::from(phase) * 3 - h + add) % 24;
        s.actors.set_tail(tail::MOONS + 2 * k + 1, pos as u8);
    }
    seams::draw_sky_strip(s);
    seams::moongates(s);
}

/// Light level (objlist 0x1BFC): 0 at night, in dungeon levels 1..4, on the first day of
/// every third month and during an eclipse timer; ramps up from 5 o'clock and down at 19;
/// at least 4 while a light burns. A change recomputes the view flags.
fn set_light(s: &mut State, f: &TickFlags) {
    let old = s.actors.tail(tail::LIGHT) as i8;
    let c = s.actors.calendar;
    let dark = (c.day == 1 && c.month.is_multiple_of(3)) || s.actors.timers[timer::ECLIPSE] != 0;
    let z = s.map.centre.z;
    let light = if dark || c.hour < 5 || c.hour > 19 || (1..5).contains(&z) {
        0
    } else if c.hour == 5 {
        c.minute / 10 + 1
    } else if c.hour == 19 {
        (59 - c.minute) / 10 + 1
    } else {
        7
    };
    let mut light = light as i8;
    if light < 4 && (f.burning || s.actors.timers[0] != 0) {
        light = 4;
    }
    s.actors.set_tail(tail::LIGHT, light as u8);
    if light != old {
        seams::refresh_view_flags(s);
    }
}

/// The date line in the status area: "%2d-%02d-%04d Wind:%2s" with month, day, year and the
/// wind name (the name table at A4-0x7ae4 is indexed by the signed wind byte).
fn date_line(s: &mut State) {
    const DATE: i32 = -0x7293;
    let c = s.actors.calendar;
    const WIND_NAMES: i32 = -0x7ae4;
    seams::date_line_window(s, true);
    let args = [
        Arg::Word(i16::from(c.month)),
        Arg::Word(i16::from(c.day)),
        Arg::Word(c.year as i16),
        Arg::Pointer(WIND_NAMES + 4 * i32::from(s.actors.wind as i8)),
    ];
    seams::print(s, DATE, &args);
    seams::date_line_window(s, false);
}

#[cfg(test)]
mod tests {
    use u6_formats::position::Position;
    use u6_formats::schedule::ScheduleEntry;

    use super::*;
    use crate::actors::Calendar;

    fn entry(hour: u8, day_of_week: u8, worktype: u8, x: u16) -> ScheduleEntry {
        ScheduleEntry {
            hour,
            day_of_week,
            worktype,
            pos: Position { x, y: 10, z: 0 },
        }
    }

    /// Actor 5 with entries 0..4: 8:00 any day, 8:00 on weekday 3, 9:00 any day, 20:00.
    fn scheduled() -> State {
        let mut s = State::empty();
        s.actors.schedules.entries = vec![
            entry(8, 0, 0x90, 100),
            entry(8, 3, 0x91, 101),
            entry(9, 0, 0x92, 102),
            entry(20, 0, 0x93, 103),
        ];
        s.actors.schedules.first = vec![0; 257];
        for a in 6..257 {
            s.actors.schedules.first[a] = 4;
        }
        s.objects.type_frame[5] = 0x17a;
        s.actors.worktype[5] = 0x90;
        s
    }

    #[test]
    fn schedule_pick_by_hour_and_weekday() {
        let mut s = scheduled();
        // Day 3 → weekday (3 − 1) % 7 + 1 = 3: the weekday-3 entry (listed later) wins.
        s.actors.calendar = Calendar {
            hour: 8,
            day: 3,
            ..Calendar::default()
        };
        pick_schedules(&mut s);
        assert_eq!(s.actors.attrs.unknown_10f1[5], 1);
        assert_eq!(s.actors.worktype[5], 0x81);
        // Day 4 → weekday 4: only the "any day" entry matches.
        s.actors.calendar.day = 4;
        pick_schedules(&mut s);
        assert_eq!(s.actors.attrs.unknown_10f1[5], 0);
        // Day 10 → weekday 3 again.
        s.actors.calendar.day = 10;
        pick_schedules(&mut s);
        assert_eq!(s.actors.attrs.unknown_10f1[5], 1);
        // 9:00 and 20:00.
        s.actors.calendar.hour = 9;
        pick_schedules(&mut s);
        assert_eq!(s.actors.attrs.unknown_10f1[5], 2);
        s.actors.calendar.hour = 20;
        pick_schedules(&mut s);
        assert_eq!(s.actors.attrs.unknown_10f1[5], 3);
        // No entry at 21:00: the old one stays.
        s.actors.calendar.hour = 21;
        s.actors.worktype[5] = 0x93;
        pick_schedules(&mut s);
        assert_eq!(
            (s.actors.attrs.unknown_10f1[5], s.actors.worktype[5]),
            (3, 0x93)
        );
    }

    #[test]
    fn schedule_pick_skips_party_and_non_schedule_worktypes() {
        let mut s = scheduled();
        s.actors.calendar = Calendar {
            hour: 9,
            day: 1,
            ..Calendar::default()
        };
        s.actors.worktype[5] = 0x10;
        pick_schedules(&mut s);
        assert_eq!(s.actors.worktype[5], 0x10);
        // Worktype 7 takes part; 0x9b becomes 0x81 first.
        s.actors.worktype[5] = 7;
        pick_schedules(&mut s);
        assert_eq!(s.actors.worktype[5], 0x81);
        s.actors.worktype[5] = 0x9b;
        s.actors.attrs.status[5] = 0x80;
        pick_schedules(&mut s);
        assert_eq!(
            s.actors.worktype[5], 0x81,
            "0x9b reset even for a party member"
        );
        s.actors.worktype[5] = 0x92;
        pick_schedules(&mut s);
        assert_eq!(
            s.actors.worktype[5], 0x92,
            "party members keep their worktype"
        );
    }

    #[test]
    fn clock_rolls_minutes_hours_days_months_years() {
        let mut s = scheduled();
        s.actors.calendar = Calendar {
            minute: 59,
            hour: 8,
            day: 4,
            month: 6,
            year: 161,
        };
        tick(&mut s, 1);
        let c = s.actors.calendar;
        assert_eq!((c.minute, c.hour, c.day), (0, 9, 4));
        assert_eq!(
            s.actors.worktype[5], 0x81,
            "the hour change picked a schedule"
        );
        assert_eq!(s.actors.attrs.unknown_10f1[5], 2);
        s.actors.calendar = Calendar {
            minute: 30,
            hour: 23,
            day: 28,
            month: 12,
            year: 161,
        };
        tick(&mut s, 45);
        let c = s.actors.calendar;
        assert_eq!(
            (c.minute, c.hour, c.day, c.month, c.year),
            (15, 0, 1, 1, 162)
        );
        // Two hours at once.
        tick(&mut s, 125);
        let c = s.actors.calendar;
        assert_eq!((c.minute, c.hour), (20, 2));
    }

    #[test]
    fn time_stop_only_counts_down() {
        let mut s = scheduled();
        s.actors.calendar.minute = 10;
        s.actors.timers[timer::TIME_STOP] = 5;
        s.actors.timers[0] = 9;
        let rng = s.rng;
        tick(&mut s, 2);
        assert_eq!(s.actors.timers[timer::TIME_STOP], 3);
        assert_eq!(s.actors.timers[0], 9, "other timers frozen");
        assert_eq!(s.actors.calendar.minute, 10);
        assert_eq!(s.rng, rng, "no draws");
        tick(&mut s, 3);
        assert_eq!(s.actors.timers[timer::TIME_STOP], 0);
        assert_eq!(s.actors.work.time_stop_over, 1);
        assert_eq!(s.actors.timers[0], 6);
        assert_eq!(s.actors.calendar.minute, 13);
    }

    #[test]
    fn light_level_by_hour_and_light_source() {
        let mut s = State::empty();
        let at = |s: &mut State, hour: u8, minute: u8| {
            s.actors.calendar.hour = hour;
            s.actors.calendar.minute = minute;
            set_light(s, &TickFlags::default());
            s.actors.tail(tail::LIGHT)
        };
        assert_eq!(at(&mut s, 3, 0), 0);
        assert_eq!(at(&mut s, 5, 25), 3);
        assert_eq!(at(&mut s, 12, 0), 7);
        assert_eq!(at(&mut s, 19, 30), 3);
        s.actors.timers[0] = 1;
        assert_eq!(at(&mut s, 3, 0), 4, "a light source");
        s.map.centre.z = 2;
        s.actors.timers[0] = 0;
        assert_eq!(at(&mut s, 12, 0), 0, "dungeon");
    }
}
