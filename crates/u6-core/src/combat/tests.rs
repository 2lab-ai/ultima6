//! Unit tests of the combat code on synthetic states (no `game`: tables are written by hand,
//! the values are the shape of the originals, not their contents).

use u6_formats::position::Position;

use super::present::{Said, flight};
use super::tables::CombatTables;
use super::*;
use crate::actors::tail;
use crate::image::A4;
use crate::main_loop::test_support::link_placed_objects;
use crate::rng::Rng;
use crate::state::State;

/// Creature types of the synthetic list: index 0..3.
const ORC: u16 = 0x16b;
const PLAYER: u16 = 0x17a;
const SLIME: u16 = 0x177;

fn distance_table() -> Vec<u8> {
    let mut d = vec![0u8; 64];
    for x in 0..8 {
        for y in 0..8 {
            d[x * 8 + y] = x.max(y) as u8;
        }
    }
    d
}

/// Party: member 0 (actor 1) at (20, 20) and member 1 (actor 2) at (21, 20); creature tables
/// for the orc (index 0), the player's type (1) and the slime (2).
fn world() -> State {
    let mut s = State::empty();
    s.map.centre = Position { x: 20, y: 20, z: 0 };
    s.objects.defs.monster_types = vec![ORC, PLAYER, SLIME, 0];
    let mut t = CombatTables {
        distance: distance_table(),
        ranges: vec![(0x29, 5), (0x21, 4)],
        weapons: vec![(0x22, 8), (0x29, 10)],
        armour: vec![(0x11, 2)],
        lying: vec![(ORC, ORC | 0x3000)],
        ..CombatTables::default()
    };
    for n in 0..6 {
        t.creature_stats[n] = vec![10, 4, 8, 0];
    }
    t.creature_stats[4] = vec![6, 3, 2, 0];
    t.creature_flags = vec![0, 0, 0, 0];
    t.creature_status = vec![0x20, 0x40, 0x20, 0];
    s.combat.tables = t;
    for (a, x, ty) in [(1usize, 20u16, PLAYER), (2, 21, PLAYER)] {
        s.objects.type_frame[a] = ty;
        s.objects.location[a] = Position { x, y: 20, z: 0 };
        s.actors.attrs.status[a] = 0xc0;
        s.actors.attrs.hp[a] = 60;
        s.actors.attrs.level[a] = 3;
        s.actors.attrs.dexterity[a] = 15;
        s.actors.attrs.strength[a] = 15;
        s.actors.attrs.combat_mode[a] = 3;
        s.actors.worktype[a] = 1;
    }
    s.actors.worktype[1] = 2;
    s.actors.party.members[..2].copy_from_slice(&[1, 2]);
    s.actors.party.size = 2;
    s.actors.party.names = vec!["m0".into(), "m1".into()];
    s.actors.set_tail(tail::SOLO_MEMBER, 0xff);
    s
}

/// An evil orc (actor `a`) at (x, y).
fn orc(s: &mut State, a: usize, x: u16, y: u16) {
    s.objects.type_frame[a] = ORC;
    s.objects.location[a] = Position { x, y, z: 0 };
    s.actors.attrs.status[a] = 0x20;
    s.actors.attrs.hp[a] = 20;
    s.actors.attrs.level[a] = 1;
    s.actors.attrs.dexterity[a] = 10;
    s.actors.attrs.strength[a] = 10;
    s.actors.worktype[a] = 8;
}

/// The image addresses of the messages queued so far.
fn said(s: &State) -> Vec<u32> {
    s.combat.said.iter().map(|m: &Said| m.at).collect()
}

#[test]
fn distances() {
    let mut s = world();
    assert_eq!(leaves::distance(&s, 1, 23, 18), 3);
    assert_eq!(leaves::distance(&s, 1, 20, 20), 0);
    assert_eq!(leaves::cell_distance(&s, 5, 5, 7, 2), 3);
    assert_eq!(leaves::cell_distance(&s, 0, 0, 8, 0), 9);
    s.combat.tables.distance[2 * 8 + 3] = 5;
    assert_eq!(leaves::cell_distance(&s, 1, 4, 3, 1), 5);
}

#[test]
fn ranges_damage_and_armour() {
    let mut s = world();
    s.objects.type_frame[0x200] = 0x29;
    s.objects.type_frame[0x201] = 0x22;
    s.objects.type_frame[0x202] = 0x11;
    assert_eq!(leaves::range(&s, 0x200), 5);
    assert_eq!(leaves::range(&s, 0x201), 1);
    assert_eq!(leaves::damage_of(&s, 0x201), 8);
    assert_eq!(leaves::damage_of(&s, 0x202), -1);
    // An actor of a creature type hits with the creature's damage byte.
    orc(&mut s, 0x10, 22, 20);
    assert_eq!(leaves::damage_of(&s, 0x10), 6);
    assert_eq!(leaves::armour_of(&s, 0x202), 2);
    // Cursed −3, protected +3.
    s.objects.obj_status[1] |= 0x40;
    assert_eq!(leaves::armour(&mut s, 1), -3);
    s.actors.attrs.status[1] |= 0x01;
    assert_eq!(leaves::armour(&mut s, 1), 0);
}

#[test]
fn experience_caps_and_levels() {
    let mut s = world();
    s.actors.attrs.exp[1] = 9990;
    leaves::add_experience(&mut s, 1, 5);
    assert_eq!(s.actors.attrs.exp[1], 9995);
    leaves::add_experience(&mut s, 1, 50);
    assert_eq!(s.actors.attrs.exp[1], 9999);
    for (e, l) in [(0u16, 1), (99, 1), (100, 2), (199, 2), (200, 3), (800, 5)] {
        s.actors.attrs.exp[1] = e;
        assert_eq!(leaves::level_for_experience(&s, 1), l, "exp {e}");
    }
}

#[test]
fn karma_never_goes_negative() {
    let mut s = world();
    s.actors.karma = 7;
    leaves::lose_karma(&mut s, 5);
    assert_eq!(s.actors.karma, 2);
    leaves::lose_karma(&mut s, 5);
    assert_eq!(s.actors.karma, 0);
}

#[test]
fn to_hit_rolls_once_against_half_the_difference() {
    let mut s = world();
    orc(&mut s, 0x10, 21, 21);
    // (dex 10 − dex 15 + 30) / 2 = 12: hit when random(1, 30) ≥ 12.
    let mut probe = Rng::new();
    probe.set_state(s.rng.state());
    let r = probe.range(1, 30) as i16;
    let hit = attack::to_hit(&mut s, 1, 0x10, 1);
    assert_eq!(hit, r >= 12);
    assert_eq!(s.rng.state(), probe.state(), "exactly one draw");
    // Objects and the glass sword always hit, without a draw.
    let before = s.rng.state();
    assert!(attack::to_hit(&mut s, 1, 0x300, 1));
    s.objects.type_frame[0x201] = 0x30;
    assert!(attack::to_hit(&mut s, 1, 0x10, 0x201));
    assert_eq!(s.rng.state(), before);
}

#[test]
fn damage_graze_hurt_and_kill() {
    let mut s = world();
    orc(&mut s, 0x10, 21, 21);
    link_placed_objects(&mut s);
    // Damage 1 is exact: one hit point, no draw for the roll (no armour).
    damage::apply_damage(&mut s, 1, 0x10, 1);
    assert_eq!(s.actors.attrs.hp[0x10], 19);
    // A killing blow: dead, lying, experience = a quarter of the orc's six stats
    // (10 + 10 + 10 + 10 + 6 + 10) / 4 = 14.
    s.actors.attrs.hp[0x10] = 1;
    let before = s.actors.attrs.exp[1];
    damage::apply_damage(&mut s, 1, 0x10, 1);
    assert_eq!(s.actors.attrs.hp[0x10], 0);
    assert_eq!(s.actors.attrs.status[0x10] & 0x10, 0x10);
    assert_eq!(s.actors.worktype[0x10], 0);
    assert_eq!(s.actors.attrs.exp[1] - before, 14);
    assert!(said(&s).contains(&(A4 - 0x7716)), "the death line");
    // The remains: a map object of the lying frame; the orc left for level 15.
    assert_eq!(s.objects.location[0x10].z, 0xf);
    let remains = (0x101..crate::objects::OBJECT_SLOTS)
        .find(|&o| s.objects.type_frame[o] == ORC | 0x3000)
        .expect("remains");
    assert_eq!(s.objects.qq[remains] >> 8, 0x10);
    // Damage 0 is a graze.
    orc(&mut s, 0x11, 19, 21);
    s.combat.said.clear();
    damage::apply_damage(&mut s, 1, 0x11, 0);
    assert_eq!(s.actors.attrs.hp[0x11], 20);
    assert_eq!(said(&s), [A4 - 0x7723]);
}

#[test]
fn wound_report_by_quarters() {
    let mut s = world();
    // Max hp 90 (level 3): 22 hp = 88 / 90 → quarter 0, the critical line; flees with int ≥ 5.
    orc(&mut s, 0x10, 21, 21);
    s.actors.attrs.level[0x10] = 3;
    s.actors.attrs.intelligence[0x10] = 9;
    s.actors.attrs.hp[0x10] = 22;
    damage::report_wounds(&mut s, 0x10);
    assert_eq!(said(&s), [0x0012_1b9a, A4 - 0x7750]);
    assert_eq!(s.actors.worktype[0x10], 7);
    for (hp, word) in [(23u8, 0x7745u32), (45, 0x773d), (68, 0x7735)] {
        s.combat.said.clear();
        s.actors.attrs.hp[0x10] = hp;
        damage::report_wounds(&mut s, 0x10);
        assert_eq!(said(&s), [0x0012_1b9a, A4 - word, A4 - 0x772e], "hp {hp}");
    }
    s.combat.said.clear();
    s.actors.attrs.hp[0x10] = 90;
    damage::report_wounds(&mut s, 0x10);
    assert!(said(&s).is_empty());
}

#[test]
fn survivors_lose_hit_points_and_slimes_try_to_divide() {
    let mut s = world();
    // The slime is creature index 0x30.
    s.objects.defs.monster_types = vec![ORC; 0x30];
    s.objects.defs.monster_types.extend([SLIME, 0]);
    orc(&mut s, 0x10, 25, 25);
    s.objects.type_frame[0x10] = SLIME;
    link_placed_objects(&mut s);
    let before = s.rng.state();
    damage::hurt(&mut s, 0x10, 3);
    assert_eq!(s.actors.attrs.hp[0x10], 17);
    assert_ne!(s.rng.state(), before, "the slime drew for a tile");
    // A worktype-9 creature hurt turns to assault.
    orc(&mut s, 0x11, 25, 26);
    s.actors.worktype[0x11] = 9;
    damage::hurt(&mut s, 0x11, 1);
    assert_eq!(s.actors.worktype[0x11], 8);
}

#[test]
fn combat_mode_on_and_off() {
    let mut s = world();
    s.actors.attrs.combat_mode[2] = 5;
    mode::begin_combat(&mut s);
    assert_eq!(s.actors.tail(tail::COMBAT_MODE), 1);
    assert_eq!(
        s.actors.worktype[1], 2,
        "the first able member is the player"
    );
    assert_eq!(s.actors.worktype[2], 5);
    assert_eq!(s.actors.tail(0x7a), 3, "its combat mode saved");
    assert_eq!(s.actors.attrs.unknown_18f1[2], 2);
    mode::break_off(&mut s);
    assert_eq!(s.actors.tail(tail::COMBAT_MODE), 0);
    assert_eq!(s.actors.worktype[1], 2);
    assert_eq!(s.actors.worktype[2], 1, "followers in party mode");
    assert_eq!(s.actors.attrs.unknown_18f1[2], 1, "rank");
    // A dead member gets worktype 0.
    mode::begin_combat(&mut s);
    s.actors.attrs.status[2] |= 0x10;
    mode::break_off(&mut s);
    assert_eq!(s.actors.worktype[2], 0);
}

#[test]
fn dirty_rectangle_clips_to_the_window() {
    let mut s = world();
    s.combat.dirty = [0xa8, 0xa8, 7, 7];
    present::extend_dirty(&mut s, 40, 50);
    assert_eq!(s.combat.dirty, [32, 42, 48, 58]);
    present::extend_dirty(&mut s, 10, 0xa0);
    assert_eq!(s.combat.dirty, [8, 42, 48, 0xa7]);
}

#[test]
fn flight_stops_at_range_and_lists_actors_passed() {
    let mut s = world();
    orc(&mut s, 0x10, 22, 20);
    link_placed_objects(&mut s);
    // Shooter actor 1 at (20, 20) east to (26, 20) with range 3: the missile reaches x 23.
    let (mut xs, mut ys) = ([26i16], [20i16]);
    s.combat.shooter = 1;
    let r = present::fly(&mut s, 20, 20, &mut xs, &mut ys, 1, 3, 0, 0);
    assert_eq!(r, 0);
    assert_eq!((xs[0], ys[0]), (23, 20));
    assert_eq!(s.combat.landing, (23, 20));
    // Member 1 (actor 2) stands on the way at (21, 20).
    assert_eq!(&s.combat.passed[..], [2, 0x10, -1]);
    // Within range it gets there.
    let (mut xs, mut ys) = ([22i16], [20i16]);
    let r = present::fly(&mut s, 20, 20, &mut xs, &mut ys, 1, 3, 0, 0);
    assert_eq!((r, xs[0]), (1, 22));
    // A return flight keeps the target arrays.
    let (mut xs, mut ys) = ([22i16], [21i16]);
    present::fly(&mut s, 20, 20, &mut xs, &mut ys, 1, 9, 0, flight::RETURN);
    assert_eq!((xs[0], ys[0]), (20, 20));
}

#[test]
fn target_search_takes_the_nearest_foe_by_alignment() {
    let mut s = world();
    orc(&mut s, 0x10, 25, 20);
    orc(&mut s, 0x11, 22, 21);
    // A neutral bystander is no target for the party.
    orc(&mut s, 0x12, 21, 21);
    s.actors.attrs.status[0x12] = 0;
    assert_eq!(npc::find_target(&mut s, 2), 0x11);
    // The orcs go for the good party members.
    assert_eq!(npc::find_target(&mut s, 0x11), 2);
    // Beyond 8 tiles nobody.
    s.objects.location[0x11].x = 40;
    s.objects.location[0x10].x = 40;
    assert_eq!(npc::find_target(&mut s, 2), -1);
}

#[test]
fn assault_hits_an_adjacent_foe_and_pays_ten() {
    let mut s = world();
    orc(&mut s, 0x10, 22, 20);
    link_placed_objects(&mut s);
    s.actors.movement_points[0x10] = 20;
    let hp = (s.actors.attrs.hp[1], s.actors.attrs.hp[2]);
    npc::assault(&mut s, 0x10);
    assert_eq!(s.actors.movement_points[0x10], 10);
    assert_eq!(s.ui.input.selection, 2, "nearest member");
    // The hit either missed (no change) or actor 2 lost hit points; actor 1 is untouched.
    assert_eq!(s.actors.attrs.hp[1], hp.0);
    assert!(s.actors.attrs.hp[2] <= hp.1);
}

#[test]
fn rouse_neutrals_sets_guards_on_the_party() {
    let mut s = world();
    s.actors.karma = 20;
    orc(&mut s, 0x10, 30, 20);
    s.objects.type_frame[0x10] = 0x17e;
    s.actors.attrs.status[0x10] = 0;
    orc(&mut s, 0x11, 22, 20);
    s.actors.attrs.status[0x11] = 0;
    s.actors.worktype[0x11] = 0x10;
    damage::rouse_neutrals(&mut s);
    assert_eq!(s.actors.karma, 15);
    assert_eq!(s.actors.worktype[0x10], 0x12, "the guard comes to arrest");
    assert_eq!(s.actors.worktype[0x11], 0x13, "the bystander flees");
}

#[test]
fn intelligence_contest_draws_once() {
    let mut s = world();
    s.actors.attrs.intelligence[1] = 10;
    s.actors.attrs.intelligence[2] = 20;
    let mut probe = Rng::new();
    probe.set_state(s.rng.state());
    let r = probe.range(1, 30) as i16;
    // int 20 / 2 − int 10 + 15 = 15.
    assert_eq!(leaves::contest(&mut s, 1, 2), r < 15);
}

#[test]
fn copy_actor_copies_without_movement_points() {
    let mut s = world();
    orc(&mut s, 0x10, 22, 20);
    s.actors.movement_points[0x10] = 9;
    leaves::copy_actor(&mut s, 0x10, 0x11);
    assert_eq!(s.objects.type_frame[0x11], ORC);
    assert_eq!(s.actors.attrs.hp[0x11], 20);
    assert_eq!(s.actors.movement_points[0x11], 0);
    assert_eq!(s.actors.worktype[0x11], 8);
}

#[test]
fn retreat_runs_away_from_a_near_foe() {
    let mut s = world();
    orc(&mut s, 0x10, 22, 20);
    s.actors.worktype[0x10] = 0x13;
    s.actors.movement_points[0x10] = 20;
    link_placed_objects(&mut s);
    npc::retreat(&mut s, 0x10);
    // It runs east, away from actor 2 at (21, 20), or pays if it could not move.
    let p = s.objects.location[0x10];
    assert!(p.x >= 22, "{p:?}");
}

#[test]
fn wounds_stop_at_the_ship() {
    let mut s = world();
    orc(&mut s, 0x10, 22, 20);
    s.objects.type_frame[0x10] = 0x1a7;
    s.actors.attrs.hp[0x10] = 1;
    damage::report_wounds(&mut s, 0x10);
    assert!(said(&s).is_empty());
    assert_eq!(damage::hurt(&mut s, 0x10, 5), 0);
    assert_eq!(s.actors.attrs.hp[0x10], 1, "the balloon takes no damage");
}

/// The magic hunk's refusal line (PC-relative 0x1159f8) a staff or wand attack prints while
/// its cast (0x11a586) is not wired into the synchronous hit.
const CAST_REFUSED: u32 = 0x0011_59f8;

/// A readied object of `kind` in slot 0x200 whose quality names spell 0x10 (staff charge 2).
fn charged_item(s: &mut State, kind: u16) -> i16 {
    s.objects.type_frame[0x200] = kind;
    s.objects.qq[0x200] = 0x10 << 8;
    0x200
}

#[test]
fn a_staff_attack_refuses_before_charging_magic() {
    let mut s = world();
    orc(&mut s, 0x10, 21, 21);
    link_placed_objects(&mut s);
    s.actors.attrs.magic[1] = 10;
    s.actors.attrs.level[1] = 5;
    let staff = charged_item(&mut s, 0x39);
    s.ui.input.selection = 0x10;
    let leader = s.actors.party.leader_index;
    assert_eq!(attack::hit(&mut s, 1, 21, 21, staff), 0);
    assert_eq!(
        s.actors.attrs.magic[1], 10,
        "no magic paid for a cast not run"
    );
    assert!(said(&s).contains(&CAST_REFUSED), "the refusal is shown");
    assert_eq!(s.actors.party.leader_index, leader);
    assert_eq!(s.actors.attrs.hp[0x10], 20, "the target is untouched");
}

#[test]
fn a_wand_attack_refuses_visibly() {
    let mut s = world();
    orc(&mut s, 0x10, 21, 21);
    link_placed_objects(&mut s);
    let wand = charged_item(&mut s, 0x150);
    s.ui.input.selection = 0x10;
    let leader = s.actors.party.leader_index;
    assert_eq!(attack::hit(&mut s, 1, 21, 21, wand), 0);
    assert!(said(&s).contains(&CAST_REFUSED), "the refusal is shown");
    assert_eq!(s.actors.party.leader_index, leader);
    assert_eq!(s.actors.attrs.hp[0x10], 20, "the target is untouched");
}

/// Hit points, status and worktype of actor `a`.
fn vitals(s: &State, a: usize) -> (u8, u8, u8) {
    let at = &s.actors.attrs;
    (at.hp[a], at.status[a], s.actors.worktype[a])
}

#[test]
fn a_grown_vine_segment_names_its_vine() {
    let mut s = world();
    // The vine: actor 0x10 of type 0x16e at (25, 25), facing north; actor 0 a bystander.
    orc(&mut s, 0x10, 25, 25);
    s.objects.type_frame[0x10] = 0x16e;
    s.actors.attrs.hp[0] = 30;
    s.actors.attrs.status[0] = 0x20;
    link_placed_objects(&mut s);
    let zero = vitals(&s, 0);
    assert_eq!(npc::grow(&mut s, 0x10, 2), 1, "the vine grew east");
    let seg = (0x101..crate::objects::OBJECT_SLOTS)
        .find(|&o| s.objects.type_frame[o] & 0x3ff == 0x16e)
        .expect("a vine segment");
    damage::apply_damage(&mut s, 1, seg as i16, 1);
    assert_eq!(vitals(&s, 0), zero, "actor 0 is untouched");
    assert_eq!(s.actors.attrs.hp[0x10], 0, "the vine dies");
    assert_eq!(s.actors.attrs.status[0x10] & 0x10, 0x10);
    assert_eq!(
        s.objects.qq[seg] >> 8,
        0x10,
        "the segment's quality is the vine"
    );
}

#[test]
fn a_vine_that_cannot_leave_a_segment_reports_no_growth() {
    let mut s = world();
    orc(&mut s, 0x10, 25, 25);
    s.objects.type_frame[0x10] = 0x16e;
    s.actors.attrs.movement_flags[0x10] = 0xf0;
    link_placed_objects(&mut s);
    // No free object slot: the vine steps back and 0x126de8 returns 0 (asm 0x126f4c).
    s.objects.free.1 = 0;
    assert_eq!(npc::grow(&mut s, 0x10, 2), 0);
    assert_eq!(s.objects.location[0x10].x, 25, "back on its tile");
    assert_eq!(s.actors.attrs.movement_flags[0x10], 0xf0, "its facing kept");
}

#[test]
fn party_loops_stop_at_the_sixteen_member_roster() {
    let mut s = world();
    for (i, m) in s.actors.party.members.iter_mut().enumerate() {
        *m = (i + 1) as u8;
    }
    // A malformed size byte past the roster.
    s.actors.party.size = 200;
    s.actors.worktype[0] = 0x55;
    s.actors.attrs.combat_mode[0] = 3;
    s.actors.attrs.unknown_18f1[0] = 0x77;
    mode::begin_combat(&mut s);
    assert_eq!(s.actors.worktype[0], 0x55, "actor 0 is no member");
    assert_eq!(s.actors.attrs.unknown_18f1[0], 0x77);
    mode::break_off(&mut s);
    assert_eq!(s.actors.worktype[0], 0x55);
}

#[test]
fn tame_looks_for_the_party_within_the_roster_only() {
    let mut s = world();
    // Nobody near, so every member is looked at; a size byte past the 16 slots.
    orc(&mut s, 0x10, 60, 60);
    link_placed_objects(&mut s);
    s.actors.party.size = 17;
    npc::tame(&mut s, 0x10);
    assert!(s.objects.location[0x10].x >= 59, "it ambled or waited");
}

/// The colour of window pixel (88, 88) in the frame the engine shows.
fn centre(e: &crate::Engine) -> u8 {
    e.frame().get(88, 88).unwrap_or(0)
}

/// Fills the map window's inside (8..168) on the screen with colour `c`.
fn fill_window(io: &crate::engine::Io, c: u8) {
    io.draw(|fb| {
        for y in 8..168 {
            for x in 8..168 {
                fb.set(x, y, c);
            }
        }
    });
}

/// Ticks `e` (each tick of `tick_ms` followed by `zeros` polls with no time) until `done`, at
/// most 20 000 ticks; per tick (engine ms since the first tick, the centre colour shown).
fn run_until(
    e: &mut crate::Engine,
    done: &std::rc::Rc<std::cell::Cell<Option<u64>>>,
    tick_ms: u32,
    zeros: usize,
) -> Vec<(u64, u8)> {
    let mut seen = Vec::new();
    let mut t0 = None;
    for _ in 0..20_000 {
        if done.get().is_some() {
            break;
        }
        e.tick(&[], tick_ms);
        for _ in 0..zeros {
            e.tick(&[], 0);
        }
        let now = e.io().now_ms();
        let t0 = *t0.get_or_insert(now);
        seen.push((now - t0, centre(e)));
    }
    seen
}

/// A step as production code leaves the screen: the window published in colour 1, then in one
/// poll its frames drawn and presented on the screen and recorded (colours 2, 3, 4, each with
/// `cost` ms), a last draw in colour `last` that is not recorded (if any), and the playback.
/// The engine time (from the first tick) the playback took, and per tick the time and the
/// colour shown.
fn recorded_step(cost: u16, last: Option<u8>, tick_ms: u32, zeros: usize) -> (u64, Vec<(u64, u8)>) {
    let done = std::rc::Rc::new(std::cell::Cell::new(None));
    let out = std::rc::Rc::clone(&done);
    let mut e = crate::Engine::with_game(
        State::empty(),
        crate::Files::default(),
        move |st, io| async move {
            fill_window(&io, 1);
            io.present();
            io.vblank().await;
            let t0 = io.now_ms();
            for c in [2, 3, 4] {
                fill_window(&io, c);
                io.present();
                present::capture_window(&mut st.borrow_mut(), cost);
            }
            if let Some(c) = last {
                fill_window(&io, c);
                io.present();
            }
            play_effects(&st, &io).await;
            out.set(Some(io.now_ms() - t0));
        },
    );
    let seen = run_until(&mut e, &done, tick_ms, zeros);
    (done.get().expect("the playback ends"), seen)
}

#[test]
fn the_old_picture_stays_until_the_first_recorded_frame() {
    // The original draws a step's pictures one after the other while the last one it showed
    // stays up: the published window is the old one (1) until the first recorded frame's time,
    // then each recorded frame in turn, and the step's last draw (9) only at the end.
    let cost = present::FLASH_MS;
    let (_, seen) = recorded_step(cost, Some(9), 16, 0);
    let before: Vec<u8> = seen
        .iter()
        .filter(|&&(t, _)| t > 0 && t < u64::from(cost))
        .map(|&(_, c)| c)
        .collect();
    println!("shown while the first frame is drawn: {before:?}");
    assert!(
        !before.is_empty() && before.iter().all(|&c| c == 1),
        "{before:?}"
    );
    let order: Vec<u8> = seen.iter().map(|&(_, c)| c).fold(Vec::new(), |mut v, c| {
        if v.last() != Some(&c) {
            v.push(c);
        }
        v
    });
    assert_eq!(
        order,
        [1, 2, 3, 4, 9],
        "each frame in turn, the last draw at the end"
    );
}

#[test]
fn recorded_frames_take_their_time_at_any_host_tick() {
    // Spec 03 §1: display waits run on the engine clock. Three frames of a cost that is no
    // multiple of the tick take their total (to within a tick) at 16 ms and 8 ms ticks and with
    // polls of no time between, and every frame is published.
    let cost = 333;
    let want = 3 * u64::from(cost);
    for (tick, zeros) in [(16, 0), (8, 0), (16, 3), (5, 2)] {
        let (ms, seen) = recorded_step(cost, None, tick, zeros);
        println!("three frames of {cost} ms: {ms} ms at {tick} ms ticks, {zeros} zero polls");
        assert!(
            ms >= want && ms <= want + u64::from(tick),
            "{tick} ms ticks: {ms} ms, want {want} + up to a tick"
        );
        for c in [2, 3, 4] {
            assert!(
                seen.iter().any(|&(_, s)| s == c),
                "frame {c} shown at {tick} ms"
            );
        }
    }
    // Polls with no time do not pass a deadline: after the first tick, a thousand of them
    // leave the old picture up and the playback waiting.
    let done = std::rc::Rc::new(std::cell::Cell::new(None));
    let out = std::rc::Rc::clone(&done);
    let mut e = crate::Engine::with_game(
        State::empty(),
        crate::Files::default(),
        move |st, io| async move {
            fill_window(&io, 1);
            io.present();
            io.vblank().await;
            fill_window(&io, 2);
            io.present();
            present::capture_window(&mut st.borrow_mut(), cost);
            play_effects(&st, &io).await;
            out.set(Some(io.now_ms()));
        },
    );
    e.tick(&[], 16);
    e.tick(&[], 16);
    for _ in 0..1000 {
        e.tick(&[], 0);
    }
    assert!(done.get().is_none(), "zero-time polls passed the deadline");
    assert_eq!(centre(&e), 1, "the old picture while it waits");
}

/// The engine ms each wait of `costs` took (one run of waits, from the game's first poll), with
/// the host's ticks given by `tick(n)` for tick n (0 = the game's first poll).
fn wait_lengths(costs: &'static [u16], tick: impl Fn(usize) -> u32) -> Vec<u64> {
    let ends = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let out = std::rc::Rc::clone(&ends);
    let mut e = crate::Engine::with_game(
        State::empty(),
        crate::Files::default(),
        move |st, io| async move {
            out.borrow_mut().push(io.now_ms());
            for &cost in costs {
                present::pace(&st, &io, cost).await;
                out.borrow_mut().push(io.now_ms());
            }
        },
    );
    for t in 0..100_000 {
        if e.finished() {
            break;
        }
        e.tick(&[], tick(t));
    }
    assert!(e.finished(), "every wait ended");
    let ends = ends.borrow();
    assert_eq!(ends.len(), costs.len() + 1, "every wait ended");
    ends.windows(2).map(|w| w[1] - w[0]).collect()
}

/// The host's longest step (site/app.js caps a tick at 100 ms; u6-native steps 20 ms, spec 06).
const HOST_MAX_TICK: u32 = 100;

#[test]
fn a_stalled_tick_does_not_squeeze_the_waits_after_it() {
    // A host that stalls (a tick of the hosts' cap, once or three times in a row) in the
    // middle of a run of ten 46 ms waits at 16 ms ticks: the waits keep their time, less a tick
    // at most, the one the stall falls in aside.
    const COSTS: [u16; 10] = [46; 10];
    for stalls in [8..9, 8..11] {
        let st = stalls.clone();
        let d = wait_lengths(
            &COSTS,
            move |t| if st.contains(&t) { HOST_MAX_TICK } else { 16 },
        );
        println!("waits of 46 ms around stalled ticks {stalls:?}: {d:?}");
        let crossing = d.iter().position(|&ms| ms >= u64::from(HOST_MAX_TICK));
        let crossing = crossing.expect("a wait spans the stall");
        let after = &d[crossing + 1..];
        assert!(!after.is_empty(), "waits follow the stall: {d:?}");
        for (i, &ms) in d.iter().enumerate().filter(|&(i, _)| i != crossing) {
            assert!(ms + 16 >= 46, "wait {i} lasted {ms} ms: {d:?}");
        }
    }
}

#[test]
fn a_stall_on_the_first_tick_of_a_run_squeezes_no_wait() {
    // The run's first observed tick is a stall (the host's longest step), then normal ticks of
    // 16 or 7 ms: every wait after the one the stall falls in keeps its time, less one normal
    // tick at most (a wait gives up at most the tick it ends on: a normal one, not the stall).
    const COSTS: [u16; 6] = [46, 311, 46, 311, 23, 129];
    for normal in [16u32, 7] {
        let d = wait_lengths(&COSTS, move |t| if t == 1 { HOST_MAX_TICK } else { normal });
        println!("first tick {HOST_MAX_TICK} ms then {normal} ms: waits {d:?} for {COSTS:?}");
        for (i, (&ms, &cost)) in d.iter().zip(&COSTS).enumerate().skip(1) {
            assert!(
                ms + u64::from(normal) >= u64::from(cost),
                "wait {i} of {cost} ms lasted {ms} ms at {normal} ms ticks: {d:?}"
            );
        }
    }
}

/// Each wait of `costs` (one run of waits from the game's first poll, ticks given by `tick(n)`):
/// the engine ms it took and the length of the host tick it ended on.
fn wait_ends(costs: &'static [u16], tick: impl Fn(usize) -> u32) -> Vec<(u64, u64)> {
    let ends = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let out = std::rc::Rc::clone(&ends);
    let mut e = crate::Engine::with_game(
        State::empty(),
        crate::Files::default(),
        move |st, io| async move {
            out.borrow_mut().push(io.now_ms());
            for &cost in costs {
                present::pace(&st, &io, cost).await;
                out.borrow_mut().push(io.now_ms());
            }
        },
    );
    // The engine time after each host tick, and that tick's length.
    let mut after = std::collections::HashMap::new();
    let mut now = 0u64;
    for t in 0..100_000 {
        if e.finished() {
            break;
        }
        let ms = tick(t);
        now += u64::from(ms);
        after.insert(now, u64::from(ms));
        e.tick(&[], ms);
    }
    assert!(e.finished(), "every wait ended");
    let ends = ends.borrow();
    assert_eq!(ends.len(), costs.len() + 1, "every wait ended");
    ends.windows(2)
        .map(|w| {
            (
                w[1] - w[0],
                after.get(&w[1]).copied().expect("a wait ends on a tick"),
            )
        })
        .collect()
}

#[test]
fn a_stall_after_lateness_built_up_shortens_no_wait_by_more_than_its_tick() {
    // Two stalls of the hosts' cap in a row early in a run of 46 ms waits at 16 ms ticks build
    // up lateness the waits after them pay back; a third stall then falls inside a long wait,
    // with that debt not yet paid. Every wait lasts its cost less the tick it ended on at most,
    // and the run ends within the longest tick of the waits' sum.
    const COSTS: [u16; 12] = [46, 46, 46, 46, 46, 427, 46, 46, 46, 311, 46, 46];
    let stalls = [4usize, 5, 20];
    let tick = move |t: usize| {
        if stalls.contains(&t) {
            HOST_MAX_TICK
        } else {
            16
        }
    };
    let d = wait_ends(&COSTS, tick);
    println!("waits (ms, tick ended on) around stalls at ticks {stalls:?}: {d:?} for {COSTS:?}");
    // The scenario: the third stall lands inside a long wait while the run is still behind its
    // schedule (the waits ended before it ended after they were due). The engine clock moves a
    // tick before each poll, so the run starts at the end of tick 0 and the stall spans the
    // engine time from the end of the tick before it to its own end.
    let clock = |t: usize| -> u64 { (0..=t).map(|u| u64::from(tick(u))).sum() };
    let (from, to) = (clock(stalls[2] - 1), clock(stalls[2]));
    let (mut ended, mut due) = (clock(0), clock(0));
    let mut spanning = None;
    for (i, (&(ms, _), &cost)) in d.iter().zip(&COSTS).enumerate() {
        if ended + ms > from {
            spanning = Some((i, ended + ms));
            break;
        }
        ended += ms;
        due += u64::from(cost);
    }
    let (i, end) = spanning.expect("a wait is in progress when the third stall lands");
    assert!(
        end >= to && COSTS[i] > HOST_MAX_TICK as u16,
        "the third stall falls inside wait {i} ({} ms, ends at {end}): {d:?}",
        COSTS[i]
    );
    assert!(
        ended > due,
        "lateness outstanding when the third stall lands: the waits before it ended at {ended}, \
         due at {due}: {d:?}"
    );
    let paid = d
        .iter()
        .zip(&COSTS)
        .any(|(&(ms, _), &cost)| ms < u64::from(cost));
    assert!(paid, "some wait paid lateness back: {d:?}");
    for (i, (&(ms, tick), &cost)) in d.iter().zip(&COSTS).enumerate() {
        assert!(
            ms + tick >= u64::from(cost),
            "wait {i} of {cost} ms lasted {ms} ms, ending on a {tick} ms tick: {d:?}"
        );
    }
    let took: u64 = d.iter().map(|&(ms, _)| ms).sum();
    let want: u64 = COSTS.iter().map(|&c| u64::from(c)).sum();
    assert!(
        took >= want && took <= want + u64::from(HOST_MAX_TICK),
        "the run took {took} ms for {want}"
    );
}

/// A host's ticks for [`wait_lengths`]: a label, the length (ms) of tick n, its longest tick.
type Host = (&'static str, fn(usize) -> u32, u64);

#[test]
fn an_uneven_host_tick_loses_no_time_over_a_run() {
    // Forty waits of mixed lengths at a host jittering between one frame and two (16, 17, 16,
    // 33, 16 ms over and over) and at a native-like one (17, 17, 16 ms, one tick of 2 ms early
    // in the run): every wait ends within the run's longest tick of the sum of the waits up to
    // it, so what one wait ends late the waits after it make up, whatever tick it ended on.
    const BASE: [u16; 10] = [46, 20, 311, 24, 26, 128, 47, 20, 427, 24];
    const COSTS: [u16; 40] = {
        let mut a = [0; 40];
        let mut i = 0;
        while i < 40 {
            a[i] = BASE[i % 10];
            i += 1;
        }
        a
    };
    let hosts: [Host; 2] = [
        ("16/17/16/33/16 ms", |t| [16, 17, 16, 33, 16][t % 5], 33),
        (
            "17/17/16 ms, one of 2 ms",
            |t| if t == 3 { 2 } else { [17, 17, 16][t % 3] },
            17,
        ),
    ];
    for (label, tick, longest) in hosts {
        let d = wait_lengths(&COSTS, tick);
        let (mut took, mut want) = (0, 0);
        for (i, (&ms, &cost)) in d.iter().zip(&COSTS).enumerate() {
            took += ms;
            want += u64::from(cost);
            assert!(
                took >= want && took <= want + longest,
                "{label}: waits 0..={i} took {took} ms, want {want} + up to {longest}"
            );
        }
        println!("{label}: {} waits, {took} ms for {want}", d.len());
    }
}

/// A window recorded dissolving (colour 5 over colour 0) with `cost` ms and played at ticks of
/// `tick_ms`: per tick, how many of its 160 × 160 dissolving pixels the published frame shows,
/// and the engine time the playback took.
fn dissolve_run(cost: u16, tick_ms: u32) -> (Vec<usize>, u64) {
    let done = std::rc::Rc::new(std::cell::Cell::new(None));
    let out = std::rc::Rc::clone(&done);
    let mut e = crate::Engine::with_game(
        State::empty(),
        crate::Files::default(),
        move |st, io| async move {
            let fill = |c: u8| {
                io.draw(|fb| {
                    for y in 8..168 {
                        for x in 8..168 {
                            fb.set(x, y, c);
                        }
                    }
                })
            };
            fill(0);
            io.present();
            io.vblank().await;
            let t0 = io.now_ms();
            fill(5);
            io.present();
            present::capture_dissolve(&mut st.borrow_mut(), cost);
            play_effects(&st, &io).await;
            out.set(Some(io.now_ms() - t0));
        },
    );
    let mut shown = Vec::new();
    for _ in 0..10_000 {
        if done.get().is_some() {
            break;
        }
        e.tick(&[], tick_ms);
        let f = e.frame();
        let n = (8..168)
            .flat_map(|y| (8..168).map(move |x| (x, y)))
            .filter(|&(x, y)| f.get(x, y) == Some(5))
            .count();
        shown.push(n);
    }
    (shown, done.get().expect("the playback ends"))
}

#[test]
fn a_dissolving_window_is_revealed_over_its_time() {
    // The original copies a dissolving window pixel by pixel for 171 frames (0x1520e2): the
    // playback shows it growing over its time, not all at once at the end, at any tick size.
    let cost = present::DISSOLVE_MS;
    let (shown, at16) = dissolve_run(cost, 16);
    let (_, at8) = dissolve_run(cost, 8);
    let partial = shown.iter().filter(|&&n| n > 0 && n < 160 * 160).count();
    println!(
        "dissolve of {cost} ms: done at {at16} ms (16 ms ticks), {at8} ms (8 ms); \
         {partial} ticks partly shown"
    );
    assert!(shown.windows(2).all(|w| w[0] <= w[1]), "it only grows");
    assert_eq!(*shown.last().unwrap(), 160 * 160, "all of it at the end");
    assert!(
        partial > shown.len() / 2,
        "most ticks show part of it: {partial} of {}",
        shown.len()
    );
    let want = u64::from(cost);
    assert!(
        at16 >= want && at16 <= want + 16,
        "{at16} ms at 16 ms ticks"
    );
    assert!(at8 >= want && at8 <= want + 8, "{at8} ms at 8 ms ticks");
}

#[test]
fn frames_left_after_a_flush_come_before_the_lines_queued_after_it() {
    // Synchronous code records a frame after three lines; a flush prints the lines and leaves
    // the frame (main_loop/commands.rs end_of_turn, objects/commands/get.rs). Its lines are
    // printed, so at the next playback it comes before the lines queued since, not after that
    // many of them.
    use crate::main_loop::{PrintArg, flush_prints, queue_print};
    let mut s = State::empty();
    s.ui = crate::main_loop::test_support::test_ui();
    let line = |s: &mut State, f: &[u8], i: u16| {
        queue_print(s, f.to_vec(), vec![PrintArg::Word(i)]);
    };
    for i in 0..3 {
        line(&mut s, b"q%d\n", i);
    }
    let mut e = crate::Engine::with_game(s, crate::Files::default(), move |st, io| async move {
        present::capture(&mut st.borrow_mut(), 0, 0, 0, 0, present::FLASH_MS);
        flush_prints(&st, &io).await;
        for i in 0..2 {
            line(&mut st.borrow_mut(), b"n%d\n", i);
        }
        play_effects(&st, &io).await;
    });
    let mut at_frame = None;
    for _ in 0..200 {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
        let s = e.state();
        if at_frame.is_none() && s.combat.shown > 0 {
            at_frame = Some(s.ui.messages.clone());
        }
    }
    assert!(e.finished(), "the playback ended");
    let at_frame = at_frame.expect("the frame is shown");
    let new = |l: &String| l == "n0" || l == "n1";
    assert!(
        !at_frame.iter().any(new),
        "no line queued after the flush comes before the frame: {at_frame:?}"
    );
    let s = e.state();
    assert_eq!(
        s.ui.messages.iter().filter(|l| new(l)).count(),
        2,
        "the later lines are printed"
    );
    assert_eq!(s.combat.shown, 1, "the frame is shown once");
}

/// A message scroll two rows high that pages on its next newline: each line after a more
/// prompt pages again (the prompt leaves one row, 0x107f2e..0x107fb4).
fn paging_scroll() -> State {
    let mut s = State::empty();
    s.ui = crate::main_loop::test_support::test_ui();
    let t = s.ui.tables.clone();
    s.ui.text
        .open(&t, crate::ui::WindowId::Messages, 0, 0, 0x27, 1);
    s.ui.text.win_mut().flags = crate::ui::text::SCROLL | crate::ui::text::PAGED;
    s.ui.text.more_rows = 1;
    s
}

/// Runs a flush of `before` (queued first), a frame recorded after them, then `after` and the
/// playback, in [`paging_scroll`]: per more prompt the frames shown when it waits (and the frame's
/// mark then), then the frames shown at the end.
fn frame_between(
    before: Vec<crate::main_loop::Pending>,
    after: &[&[u8]],
) -> (Vec<(u64, Vec<usize>)>, u64) {
    use crate::main_loop::{Pending, flush_prints};
    let mut s = paging_scroll();
    s.main.pending_print = before;
    let after: Vec<Pending> = after
        .iter()
        .map(|f| Pending::Print(f.to_vec(), Vec::new()))
        .collect();
    let mut e = crate::Engine::with_game(s, crate::Files::default(), move |st, io| async move {
        {
            let mut s = st.borrow_mut();
            present::capture(&mut s, 0, 0, 0, 0, present::FLASH_MS);
            s.main.pending_print.extend(after);
        }
        flush_prints(&st, &io).await;
        play_effects(&st, &io).await;
    });
    let mut prompts = Vec::new();
    for _ in 0..8 {
        for _ in 0..60 {
            e.tick(&[], 16);
        }
        if e.finished() {
            break;
        }
        let s = e.state();
        let marks = s.combat.effects.iter().map(|f| f.mark).collect();
        prompts.push((s.combat.shown, marks));
        drop(s);
        e.tick(&[crate::InputEvent::Key(crate::Key::Space)], 16);
    }
    assert!(e.finished(), "the flush and the playback ended");
    (prompts, e.state().combat.shown)
}

#[test]
fn a_frame_after_the_last_line_waits_for_its_more_key() {
    // A flush whose more prompt comes on its last line, a frame recorded after that line, no
    // newline held: the line is still being printed in the prompt, so the frame is not due
    // there (the original draws it after the line, after the key); it is shown once, after.
    use crate::main_loop::Pending;
    let line = Pending::Print(b"a\n".to_vec(), Vec::new());
    let (prompts, end) = frame_between(vec![line], &[]);
    assert_eq!(
        (prompts, end),
        (vec![(0, vec![1])], 1),
        "(frames shown, marks) at the prompt; frames shown at the end"
    );
}

#[test]
fn a_frame_after_a_line_is_due_once_that_line_is_printed() {
    // The frame recorded right after the first line: not due at that line's own prompt (mark
    // 1, the line in flight), due once it is printed, so the next line's prompt shows it; once.
    use crate::main_loop::Pending;
    let line = Pending::Print(b"a\n".to_vec(), Vec::new());
    let (prompts, end) = frame_between(vec![line], &[b"b\n"]);
    assert_eq!(
        (prompts, end),
        (vec![(0, vec![1]), (1, vec![])], 1),
        "(frames shown, marks) at each prompt; frames shown at the end"
    );
}

#[test]
fn entries_that_print_no_text_count_before_a_frame() {
    // A window select, a cursor move and an empty message (0x107dd8, 0x107e00, 0x108658 of
    // nothing) are entries of the queue like the lines: a frame recorded after them is due
    // once they are done, so the prompt of the line after it shows it.
    use crate::main_loop::Pending;
    use crate::ui::WindowId;
    let before = vec![
        Pending::Window(WindowId::Messages),
        Pending::Cursor(0, 0),
        Pending::Print(Vec::new(), Vec::new()),
    ];
    let (prompts, end) = frame_between(before, &[b"a\n"]);
    assert_eq!(
        (prompts, end),
        (vec![(1, vec![])], 1),
        "(frames shown, marks) at the prompt; frames shown at the end"
    );
}

#[test]
fn interrupted_prints_come_back_in_reverse_order() {
    // Two actions, the second inside the first: each sets aside the print it interrupts (its
    // queue, the frames not yet due, the held newline) after showing the frames that are due,
    // and before giving it back plays whatever it left itself. The second gives the first its
    // print back, then the first gives back the outermost.
    use crate::main_loop::{Pending, PrintArg, queue_print};
    let mut s = State::empty();
    s.ui = crate::main_loop::test_support::test_ui();
    let words = |s: &State| -> Vec<u16> {
        s.main
            .pending_print
            .iter()
            .map(|p| match p {
                Pending::Print(_, a) => match a.as_slice() {
                    [PrintArg::Word(w)] => *w,
                    _ => 0,
                },
                _ => 0,
            })
            .collect()
    };
    let marks = |s: &State| -> Vec<(u16, usize)> {
        s.combat.effects.iter().map(|e| (e.cost, e.mark)).collect()
    };
    let line = |s: &mut State, i: u16| queue_print(s, b"%d\n".to_vec(), vec![PrintArg::Word(i)]);
    let checks = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let out = std::rc::Rc::clone(&checks);
    let mut e = crate::Engine::with_game(s, crate::Files::default(), move |st, io| async move {
        {
            let mut s = st.borrow_mut();
            present::capture(&mut s, 0, 0, 0, 0, 1);
            line(&mut s, 10);
            line(&mut s, 11);
            present::capture(&mut s, 0, 0, 0, 0, 2);
            s.combat.swallow = true;
        }
        let first = present::interrupt(&st, &io).await;
        {
            let s = st.borrow();
            let state = (words(&s), marks(&s), s.combat.swallow, s.combat.shown);
            out.borrow_mut().push(("first set aside", state));
        }
        {
            let mut s = st.borrow_mut();
            line(&mut s, 20);
            present::capture(&mut s, 0, 0, 0, 0, 3);
        }
        let second = present::interrupt(&st, &io).await;
        line(&mut st.borrow_mut(), 30);
        present::resume(&st, &io, second).await;
        {
            let s = st.borrow();
            let state = (words(&s), marks(&s), s.combat.swallow, s.combat.shown);
            out.borrow_mut().push(("second gave back", state));
        }
        present::resume(&st, &io, first).await;
        let s = st.borrow();
        let state = (words(&s), marks(&s), s.combat.swallow, s.combat.shown);
        out.borrow_mut().push(("first gave back", state));
    });
    for _ in 0..200 {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
    }
    assert!(e.finished(), "the actions ended");
    assert_eq!(
        *checks.borrow(),
        [
            ("first set aside", (vec![], vec![], false, 1)),
            ("second gave back", (vec![20], vec![(3, 1)], false, 1)),
            ("first gave back", (vec![10, 11], vec![(2, 2)], true, 2)),
        ],
        "(queued lines, frames (cost, mark), newline held, frames shown) at each step: the due \
         frame shown at the first interrupt, the first action's own line and frame played \
         before the outermost print comes back as it was"
    );
    let lines = &e.state().ui.messages;
    assert_eq!(
        (line_index(lines, "30"), line_index(lines, "20")),
        (Some(0), Some(1)),
        "the second action's line, then the first's: {lines:?}"
    );
}

fn line_index(lines: &[String], want: &str) -> Option<usize> {
    lines.iter().position(|l| l == want)
}
