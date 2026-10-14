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
