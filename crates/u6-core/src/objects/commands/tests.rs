//! Synthetic-data tests of the object commands (tables are made-up bytes at the original's
//! addresses; nothing from the game).

use super::*;
use crate::main_loop::{Pending, PrintArg};
use crate::objects::tests::{put, tile, world};
use crate::objects::{READIED, Slot, create_in, move_into};
use crate::state::State;

/// Image addresses of the hunk-15 strings the tests read back, as the commands use them.
const S_CLOSED: u32 = 0x0012_87f6;
const S_OPENED: u32 = 0x0012_883e;
const S_LOCKED: u32 = 0x0012_8820;
const S_UNLOCKED: u32 = 0x0012_8848;
const S_MAGIC: u32 = 0x0012_882a;
const S_LOCKED_PLAIN: u32 = 0x0012_8854;
const S_OPEN_LOCK: u32 = 0x0012_8800;

fn a4(off: i32) -> u32 {
    GameData::a4(off)
}

/// Each string slot holds its own address as text ("<12883e>"), so a queued line names the
/// string the code chose.
fn tagged(addrs: &[u32]) -> Vec<(u32, Vec<u8>)> {
    addrs
        .iter()
        .map(|&a| {
            let mut t = format!("<{a:x}>").into_bytes();
            t.push(0);
            (a, t)
        })
        .collect()
}

fn with_data(s: &mut State, parts: Vec<(u32, Vec<u8>)>) {
    let refs: Vec<(u32, &[u8])> = parts.iter().map(|(a, b)| (*a, b.as_slice())).collect();
    s.objcmds = ObjCommands::from_data(GameData::with_bytes(&refs));
}

/// The queued text, the format strings only.
fn queued(s: &State) -> Vec<String> {
    s.main
        .pending_print
        .iter()
        .filter_map(|p| match p {
            Pending::Print(f, _) => Some(String::from_utf8_lossy(f).to_string()),
            _ => None,
        })
        .collect()
}

fn tag(a: u32) -> String {
    format!("<{a:x}>")
}

fn door_world() -> State {
    let mut s = world();
    with_data(
        &mut s,
        tagged(&[
            S_CLOSED,
            S_OPENED,
            S_LOCKED,
            S_UNLOCKED,
            S_MAGIC,
            S_LOCKED_PLAIN,
            S_OPEN_LOCK,
        ]),
    );
    s
}

fn frame(s: &State, o: Slot) -> u16 {
    s.objects.tf(o) >> 10
}

fn monsters(s: &State) -> Vec<Slot> {
    (0xe0..0x100)
        .filter(|&m| s.objects.type_frame[m as usize] != 0)
        .collect()
}

#[test]
fn a_door_cycles_through_its_frames_by_key_and_spell() {
    let mut s = door_world();
    let d = put(&mut s, 0x12c, 10, 10);
    // Open (frame 1): plain use closes it, keeping its half.
    s.objects.type_frame[d as usize] = 0x12c | 1 << 10;
    door(&mut s, d, false, false, false);
    assert_eq!(frame(&s, d), 5);
    // Closed: a key locks (xor 0xc), the lock spell locks magically (xor 8).
    door(&mut s, d, true, false, false);
    assert_eq!(frame(&s, d), 9);
    door(&mut s, d, false, false, false);
    assert_eq!(frame(&s, d), 9, "locked: plain use does nothing");
    door(&mut s, d, true, false, false);
    assert_eq!(frame(&s, d), 5);
    door(&mut s, d, false, true, false);
    assert_eq!(frame(&s, d), 13);
    door(&mut s, d, true, false, false);
    assert_eq!(frame(&s, d), 13, "magically locked: a key does nothing");
    door(&mut s, d, false, false, true);
    assert_eq!(frame(&s, d), 5);
    door(&mut s, d, false, false, false);
    assert_eq!(frame(&s, d), 1);
    // An open door refuses a key.
    door(&mut s, d, true, false, false);
    assert_eq!(frame(&s, d), 1);
    let q = queued(&s);
    let lines: Vec<&str> = q
        .iter()
        .map(String::as_str)
        .filter(|l| *l != "\n")
        .collect();
    let want = [
        S_CLOSED,
        S_LOCKED,
        S_LOCKED_PLAIN,
        S_UNLOCKED,
        S_MAGIC,
        S_MAGIC,
        S_UNLOCKED,
        S_OPENED,
        S_OPEN_LOCK,
    ]
    .map(tag);
    assert_eq!(lines, want);
}

#[test]
fn a_chest_opens_and_spills_next_to_the_leader() {
    let mut s = door_world();
    s.actors.party.members[0] = 1;
    s.actors.party.size = 1;
    s.objects.type_frame[1] = 0x19a;
    s.objects.set_xyz(1, 10, 11, 0);
    crate::main_loop::test_support::link_placed_objects(&mut s);
    let c = put(&mut s, 0x62, 10, 10);
    s.objects.type_frame[c as usize] = 0x62 | 1 << 10;
    let gem = create_in(&mut s, 0x4d, 0x08, 3, c);
    chest(&mut s, c, false, false, false);
    assert_eq!(frame(&s, c), 0);
    assert_eq!(s.objects.place(gem), 0, "spilled onto the chest's tile");
    assert_eq!((s.objects.x(gem), s.objects.y(gem)), (10, 10));
    chest(&mut s, c, false, false, false);
    assert_eq!(frame(&s, c), 1);
    chest(&mut s, c, true, false, false);
    assert_eq!(frame(&s, c), 2);
    chest(&mut s, c, false, true, false);
    assert_eq!(frame(&s, c), 3);
    chest(&mut s, c, true, false, false);
    assert_eq!(frame(&s, c), 3, "a key does nothing to a magic lock");
    chest(&mut s, c, false, false, true);
    assert_eq!(frame(&s, c), 1);
}

/// Kept from usecmd's chest test at the integration: a trap inside (0x151, quality 0x16)
/// springs instead of the opened line and goes.
#[test]
fn a_chests_trap_springs_instead_of_opened_and_goes() {
    let mut s = door_world();
    s.actors.party.members[0] = 1;
    s.actors.party.size = 1;
    s.objects.type_frame[1] = 0x19a;
    s.actors.attrs.hp[1] = 60;
    s.objects.set_xyz(1, 10, 11, 0);
    crate::main_loop::test_support::link_placed_objects(&mut s);
    let c = put(&mut s, 0x62, 10, 10);
    s.objects.type_frame[c as usize] = 0x62 | 1 << 10;
    let trap = create_in(&mut s, 0x151, 0x08, 0, c);
    s.objects.qq[trap as usize] = 0x1600;
    chest(&mut s, c, false, false, false);
    assert_eq!(frame(&s, c), 0, "the chest is open");
    assert_eq!(s.objects.type_frame[trap as usize], 0, "the trap is gone");
    assert!(
        !queued(&s).contains(&tag(S_OPENED)),
        "the trap springs instead of the opened line"
    );
}

/// The inventory click 0x10619c springs a carried chest's trap by calling 0x12ba36 with the
/// chest itself (0x10643c: D0 = A4-0x2f04, the selection); 0x12ba36 finds the trap in it
/// (0x12bb8e..0x12bb94). The ui seam passes the chest, so the trap goes.
#[test]
fn the_inventory_clicks_trap_seam_passes_the_chest() {
    let mut s = door_world();
    s.actors.party.members[0] = 1;
    s.actors.party.size = 1;
    s.objects.type_frame[1] = 0x19a;
    s.actors.attrs.hp[1] = 60;
    s.objects.set_xyz(1, 10, 11, 0);
    crate::main_loop::test_support::link_placed_objects(&mut s);
    let c = create_in(&mut s, 0x62, 0, 0, 1);
    s.objects.type_frame[c as usize] = 0x62 | 1 << 10;
    let trap = create_in(&mut s, 0x151, 0x08, 0, c);
    s.objects.qq[trap as usize] = 0x1600;
    crate::ui::seams::spring_trap(&mut s, c);
    assert_eq!(s.objects.type_frame[trap as usize], 0, "the trap is gone");
    assert_eq!(
        s.objects.type_frame[c as usize],
        0x62 | 1 << 10,
        "the chest stays"
    );
}

#[test]
fn closed_containers_and_what_fits_in_them() {
    let mut s = world();
    s.objects.defs.type_set_670c = [0x62, 0x63, 0x39, 0xba, 0xbc, 0xbf, 0xc0, 0x153, 0x3e].to_vec();
    let chest = put(&mut s, 0x62, 1, 1);
    let bag = put(&mut s, 0x63, 2, 1);
    let book = put(&mut s, 0x39, 3, 1);
    let gem = put(&mut s, 0x4d, 4, 1);
    let other_chest = put(&mut s, 0x62, 5, 1);
    assert!(!is_closed_container(&s, chest));
    s.objects.type_frame[chest as usize] = 0x62 | 1 << 10;
    assert!(is_closed_container(&s, chest));
    assert!(is_closed_container(&s, book), "type 0x39 always counts");
    assert!(!fits_in(&s, chest, gem), "closed");
    assert!(fits_in(&s, bag, gem));
    assert!(!fits_in(&s, bag, other_chest), "no chest into a container");
    assert!(!fits_in(&s, bag, 5), "no actor");
    assert!(!fits_in(&s, book, gem));
    assert!(!fits_in(&s, -1, gem));
    assert!(!fits_in(&s, gem, bag), "not a container type");
}

#[test]
fn a_surface_counts_until_something_impassable() {
    let mut s = world();
    put(&mut s, 0x117, 7, 7);
    assert!(!surface_at(&mut s, 7, 7));
    s.tables.tileflags.flags3[usize::from(tile(0x117, 0))] = 0x02;
    assert!(surface_at(&mut s, 7, 7));
    // A wall in front of it (the newest object comes first at its tile).
    put(&mut s, 0x20, 7, 7);
    s.tables.tileflags.flags1[usize::from(tile(0x20, 0))] = 0x02;
    assert!(!surface_at(&mut s, 7, 7));
}

#[test]
fn typed_answers_parse_like_the_original() {
    let mut s = world();
    assert_eq!(parse_answer(&s, b"12", 0), 12);
    assert_eq!(parse_answer(&s, b"", 0), -1);
    assert_eq!(parse_answer(&s, b"1x", 0), -2);
    assert_eq!(parse_answer(&s, b"45n", 1), -45);
    assert_eq!(parse_answer(&s, b"45S", 1), 45);
    assert_eq!(parse_answer(&s, b"45q", 1), 100);
    assert_eq!(parse_answer(&s, b"45", 1), 100, "no hemisphere letter");
    assert_eq!(parse_answer(&s, b"7w", 2), -7);
    assert_eq!(parse_answer(&s, b"7e", 2), 7);
    assert_eq!(parse_answer(&s, b"9\0junk", 0), 9, "ends at the NUL");
    s.ui.input.line_cancelled = true;
    assert_eq!(parse_answer(&s, b"", 0), -2, "escape");
}

#[test]
fn the_view_direction_of_a_pixel() {
    let mut s = world();
    s.inventory.pointer_origin = (0x58, 0x58);
    let d = |s: &State, x: i16, y: i16| seams::direction_of(s, x, y);
    assert_eq!(d(&s, 0x58, 0x58), -1);
    assert_eq!(d(&s, 0x58, 0x48), 0);
    assert_eq!(d(&s, 0x68, 0x48), 1);
    assert_eq!(d(&s, 0x68, 0x58), 2);
    assert_eq!(d(&s, 0x48, 0x68), 5);
    assert_eq!(d(&s, 0x48, 0x48), 7);
    // Far: the dominant axis, the diagonal when neither dominates by 2.
    assert_eq!(d(&s, 0x58 + 100, 0x58 + 10), 2);
    assert_eq!(d(&s, 0x58 - 30, 0x58 + 100), 4);
    assert_eq!(d(&s, 0x58 + 40, 0x58 - 30), 1);
    assert_eq!(d(&s, 0x58 - 40, 0x58 + 30), 5);
}

/// A world with a 2-entry monster list (types 0x16b, 0x1ac), their stat bytes, one spell for
/// both, and the Avatar (actor 1) leading at (500, 500) at noon.
fn monster_world() -> State {
    let mut s = world();
    s.objects.defs.monster_types = vec![0x16b, 0x1ac, 0];
    s.objects.defs.dir_dx = vec![0, 1, 1, 1, 0, -1, -1, -1];
    s.objects.defs.dir_dy = vec![-1, -1, 0, 1, 1, 1, 0, -1];
    let parts = vec![
        (a4(-0x7f7e), vec![20, 4]),
        (a4(-0x7f3e), vec![10, 2]),
        (a4(-0x7efe), vec![8, 1]),
        (a4(-0x7e3e), vec![61, 1]),
        (a4(-0x7dfe), vec![0x20, 0x00]),
        (a4(-0x7d3e), vec![0, 0, 0, 0]),
        (a4(-0x7cbe), vec![7, 0, 0, 0, 0]),
    ];
    with_data(&mut s, parts);
    s.actors.party.size = 1;
    s.actors.party.members[0] = 1;
    s.objects.type_frame[1] = 0x19a;
    s.objects.set_xyz(1, 500, 500, 0);
    crate::main_loop::test_support::link_placed_objects(&mut s);
    s.actors.calendar.hour = 12;
    s
}

#[test]
fn a_monster_gets_varied_stats_its_spells_and_mp() {
    let mut s = monster_world();
    s.rng = crate::rng::Rng::seeded(0x1234);
    let mut r = s.rng;
    let m = spawn_monster(&mut s, 0x16b, 520, 500, 0);
    assert!(m >= 0xe0);
    let i = m as usize;
    // vary(n) = n - n/2 + two draws 0..=n/2, in the order strength, dexterity, int, hp.
    let mut vary = |n: u16| {
        let h = n / 2;
        let a = r.range(0, h);
        let b = r.range(0, h);
        (n - h + a + b) as u8
    };
    let want = [vary(20), vary(10), vary(8), vary(61)];
    let a = &s.actors.attrs;
    assert_eq!(
        [a.strength[i], a.dexterity[i], a.intelligence[i], a.hp[i]],
        want
    );
    assert_eq!(a.level[i], u16::from(want[3]).div_ceil(30) as u8);
    assert_eq!(a.status[i], 0x20);
    assert_eq!(a.exp[i], 100);
    assert_eq!(s.actors.movement_points[i], want[1]);
    assert_eq!((s.actors.worktype[i], a.combat_mode[i]), (8, 8));
    let spell = crate::objects::find_child(&mut s.objects, m, 0x150, -1);
    assert!(spell >= 0);
    assert_eq!(s.objects.qq_of(spell), 7 << 8 | 1);
    assert_eq!(s.objects.place(spell), READIED);
    assert_eq!(s.objects.status(spell) & 0x02, 0x02, "hidden");
    // An unlisted type gets the fixed stats; a two-tile type its tail east of it.
    let m2 = spawn_monster(&mut s, 0x1ae, 530, 500, 0);
    let i2 = m2 as usize;
    let a = &s.actors.attrs;
    assert_eq!(
        (
            a.strength[i2],
            a.dexterity[i2],
            a.intelligence[i2],
            a.hp[i2],
            a.level[i2],
            a.status[i2]
        ),
        (10, 10, 10, 30, 1, 0x20)
    );
    assert_eq!(s.objects.tf(m2) >> 10, 6);
    let tail = crate::objects::object_of_kind_at(&mut s, 531, 500, 0, 0x1ae);
    assert_eq!(s.objects.tf(tail) >> 10, 14);
}

#[test]
fn an_egg_hatches_once_with_the_templates_count_and_worktype() {
    let mut s = monster_world();
    let egg = put(&mut s, 0x14f, 520, 500);
    s.objects.qq[egg as usize] = 0x0264; // quality 2 (alignment 2), 100%: full counts
    s.objects.obj_status[egg as usize] = 0x20;
    let t = create_in(&mut s, 0x16b, 0x08, 0x0603, egg); // worktype 6, three
    assert!(t >= 0);
    hatch_egg(&mut s, egg);
    let ms = monsters(&s);
    assert_eq!(ms.len(), 3);
    for &m in &ms {
        let i = m as usize;
        assert_eq!(s.actors.worktype[i], 6);
        assert_eq!(
            s.actors.attrs.status[i] & 0x60,
            0x20,
            "alignment (2 - 1) << 5"
        );
    }
    assert_eq!(s.objects.status(egg) & 0x42, 0x42);
    // The first stays on the egg; worktype-6 gargoyles have doubled strength.
    assert_eq!((s.objects.x(ms[0]), s.objects.y(ms[0])), (520, 500));
    assert!(s.actors.attrs.strength[ms[0] as usize] >= 20);
    // Hatched: nothing more.
    hatch_egg(&mut s, egg);
    assert_eq!(monsters(&s).len(), 3);
}

#[test]
fn a_day_egg_waits_for_the_day_and_a_failed_roll_marks_it() {
    let mut s = monster_world();
    let egg = put(&mut s, 0x14f, 520, 500);
    s.objects.qq[egg as usize] = 0x0a00 | 100; // tens 1: day only
    create_in(&mut s, 0x16b, 0x08, 0x0801, egg);
    s.actors.calendar.hour = 3;
    let rng = s.rng;
    hatch_egg(&mut s, egg);
    assert_eq!(s.objects.status(egg) & 0x40, 0, "not hatched at night");
    assert_eq!(s.rng, rng, "no draw");
    s.actors.calendar.hour = 9;
    s.objects.qq[egg as usize] = 0x0a00; // 0 %: the roll fails
    hatch_egg(&mut s, egg);
    assert_eq!(
        s.objects.status(egg) & 0x42,
        0x42,
        "marked even when nothing hatched"
    );
    assert!(monsters(&s).is_empty());
}

#[test]
fn friends_of_gargoyles_get_peaceful_gargoyles() {
    let mut s = monster_world();
    let egg = put(&mut s, 0x14f, 520, 500);
    s.objects.qq[egg as usize] = 100;
    s.objects.obj_status[egg as usize] = 0x20;
    create_in(&mut s, 0x16b, 0x08, 0x0801, egg);
    // The Avatar carries the amulet 0x4c.
    create_in(&mut s, 0x4c, 0x10, 0, 1);
    hatch_egg(&mut s, egg);
    let m = monsters(&s)[0];
    assert_eq!(s.actors.worktype[m as usize], 0xc);
}

#[test]
fn takeable_needs_every_content_marked() {
    let mut s = world();
    let bag = put(&mut s, 0x63, 3, 3);
    let gem = create_in(&mut s, 0x4d, 0x08, 1, bag);
    assert!(!takeable(&s, bag));
    s.objects.obj_status[bag as usize] |= 1;
    assert!(!takeable(&s, bag), "its content is not");
    crate::objects::mark_takeable(&mut s.objects, bag);
    assert!(takeable(&s, bag));
    assert!(takeable(&s, gem));
}

#[test]
fn name_prefixes_count_stacks_and_name_articles() {
    let mut s = world();
    // The article strings are 4 bytes apart: one-letter stand-ins.
    let short = |a: u32, t: &[u8]| (a, t.to_vec());
    with_data(
        &mut s,
        vec![
            short(0x0012_83e2, b"a\0"),
            short(0x0012_83e6, b"n\0"),
            short(0x0012_83ea, b"t\0"),
            short(0x0012_83fe, b"#\0"),
        ],
    );
    let gold = put(&mut s, 0x58, 1, 1);
    s.objects.qq[gold as usize] = 300;
    s.objects.defs.stackable = vec![60];
    let arrows = put(&mut s, 60, 2, 1);
    s.objects.qq[arrows as usize] = 0x0307;
    let sword = put(&mut s, 70, 3, 1);
    s.tables.tileflags.flags3[usize::from(tile(70, 0))] = 0x80;
    assert_eq!(name_prefix(&mut s, gold), 300);
    assert_eq!(name_prefix(&mut s, arrows), 7);
    assert_eq!(name_prefix(&mut s, sword), 1);
    let args: Vec<_> = s
        .main
        .pending_print
        .iter()
        .filter_map(|p| match p {
            Pending::Print(_, a) => a.first().cloned(),
            _ => None,
        })
        .collect();
    assert_eq!(args, [PrintArg::Word(300), PrintArg::Word(7)]);
    assert_eq!(
        queued(&s),
        ["#", "#", "n"],
        "flags3 0x80: the second article"
    );
}

#[test]
fn a_blast_lights_kegs_and_breaks_doors_not_magic_ones() {
    let mut s = world();
    with_data(&mut s, tagged(&[0x0012_ca12]));
    let d1 = put(&mut s, 0x12c, 11, 10);
    s.objects.type_frame[d1 as usize] = 0x12c | 5 << 10;
    let d2 = put(&mut s, 0x129, 9, 9);
    s.objects.type_frame[d2 as usize] = 0x129 | 13 << 10; // magically locked
    let keg = put(&mut s, 0xdf, 12, 11);
    // Row y + 2 is not reached, nor by the keg's own blast at (12, 11).
    let far = put(&mut s, 0x12c, 10, 13);
    blast_doors(&mut s, 10, 10);
    assert_eq!(s.objects.type_frame[d1 as usize], 0, "removed");
    assert_ne!(s.objects.type_frame[d2 as usize], 0);
    assert_eq!(s.objects.type_frame[keg as usize], 0, "exploded");
    assert_ne!(s.objects.type_frame[far as usize], 0);
    assert!(queued(&s).contains(&tag(0x0012_ca12)));
}

#[test]
fn a_diagonal_push_needs_a_side_free() {
    let mut s = world();
    let crate_ = put(&mut s, 0x40, 10, 10);
    s.tables.tileflags.weight[0x40] = 10;
    // Walls north and east of the crate; north-east itself free.
    for (x, y) in [(10, 9), (11, 10)] {
        put(&mut s, 0x20, x, y);
    }
    s.tables.tileflags.flags1[usize::from(tile(0x20, 0))] = 0x02;
    assert!(push_blocked(&mut s, crate_, 10, 10, 1));
    assert!(!push_blocked(&mut s, crate_, 10, 10, 4), "south is free");
    assert!(push_blocked(&mut s, crate_, 10, 10, 0), "north is a wall");
}

#[test]
fn a_strong_target_resists_a_weak_pusher() {
    let mut s = world();
    s.actors.attrs.strength[1] = 5;
    s.actors.attrs.strength[2] = 60;
    s.rng = crate::rng::Rng::seeded(7);
    let mut r = s.rng;
    let roll = r.range(1, 30) as i16;
    // 0x115c06 is magic's port (hunk 11); the push calls it.
    assert_eq!(
        crate::magic::strength_contest(&mut s, 1, 2),
        roll < (60 / 2 - 5 + 30) / 2
    );
    assert_eq!(s.rng, r, "one draw");
}

#[test]
fn weights_count_stacks_and_tenths() {
    let mut s = world();
    s.tables.tileflags.weight[0x58] = 1;
    s.tables.tileflags.weight[60] = 3;
    s.tables.tileflags.weight[0x80] = 25;
    s.objects.defs.stackable = vec![60];
    // The 9 types weighed in tenths (A4-0x71ca) are inventory's table since the M2 merge.
    s.inventory.tables.counted = vec![0x80];
    let gold = put(&mut s, 0x58, 1, 1);
    s.objects.qq[gold as usize] = 40;
    let arrows = put(&mut s, 60, 2, 1);
    s.objects.qq[arrows as usize] = 0x0205;
    let feather = put(&mut s, 0x80, 3, 1);
    assert_eq!(seams::weight(&s, gold), 40);
    assert_eq!(seams::weight(&s, arrows), 15);
    assert_eq!(seams::weight(&s, feather), 2, "25 in tenths");
    let bag = put(&mut s, 0x63, 4, 1);
    move_into(&mut s, arrows, bag, 0x08);
    assert_eq!(seams::contents_weight(&s, bag), 15);
}

#[test]
fn fixtures_light_and_douse_keeping_their_half() {
    let mut s = world();
    let brazier = put(&mut s, 0xce, 1, 1);
    s.objects.type_frame[brazier as usize] = 0xce | 2 << 10;
    douse_or_light(&mut s, brazier);
    assert_eq!(frame(&s, brazier), 3);
    douse_or_light(&mut s, brazier);
    assert_eq!(frame(&s, brazier), 2);
}

#[test]
fn a_carried_torch_stack_gives_one_lit_torch_in_a_free_hand() {
    let mut s = world();
    s.actors.party.size = 1;
    s.actors.party.members[0] = 1;
    s.objects.type_frame[1] = 0x19a;
    s.objects.set_xyz(1, 500, 500, 0);
    crate::main_loop::test_support::link_placed_objects(&mut s);
    s.ui.panel.member = 0;
    let stack = create_in(&mut s, 0x5a, 0x10, 3, 1);
    light_torch(&mut s, stack);
    assert_eq!(s.objects.qq_of(stack) & 0xff, 2, "the stack lost one");
    // The split-off torch is made readied (0x10be02 with 0x18), so it takes no hand slot.
    let lit = (0x101..0xc00)
        .find(|&o| s.objects.kind(o) == 0x5a && s.objects.status(o) & 0x80 != 0)
        .unwrap();
    assert_ne!(lit, stack);
    assert_eq!(s.objects.place(lit), READIED);
    assert_eq!((frame(&s, lit), s.objects.qq_of(lit) & 0xff), (1, 200));
    assert_eq!(s.inventory.cells[crate::inventory::EQUIP..], [0; 8]);
    // A single carried torch goes into the first free hand.
    s.objects.qq[stack as usize] = 1;
    light_torch(&mut s, stack);
    assert_eq!(s.inventory.equip(2), stack);
    assert_eq!(s.objects.place(stack), READIED);
    // Both hands full: refused.
    s.inventory.set_equip(5, lit);
    let other = create_in(&mut s, 0x5a, 0x10, 1, 1);
    light_torch(&mut s, other);
    assert_eq!(s.objects.status(other) & 0x80, 0, "no free hand");
    // A lit torch used again goes out.
    light_torch(&mut s, lit);
    assert_eq!(s.objects.type_frame[lit as usize], 0);
}

#[test]
fn using_up_the_last_of_a_stack_removes_it() {
    let mut s = world();
    let potions = put(&mut s, 60, 1, 1);
    s.objects.qq[potions as usize] = 0x0302;
    use_up_one(&mut s, potions);
    assert_eq!(s.objects.qq_of(potions), 0x0301);
    use_up_one(&mut s, potions);
    assert_eq!(s.objects.type_frame[potions as usize], 0);
}

#[test]
fn lock_picks_break_more_in_clumsy_hands_and_lights_toggle() {
    let mut s = world();
    s.rng = crate::rng::Rng::seeded(99);
    let mut r = s.rng;
    let roll = r.range(1, 30) as i16;
    assert_eq!(pick_breaks(&mut s, 10), roll < (45 - 10) / 2);
    let roll = r.range(1, 30) as i16;
    assert_eq!(fails_roll(&mut s, 44), roll < 0);
    let candle = put(&mut s, 0x7a, 1, 1);
    s.objects.type_frame[candle as usize] = 0x7a | 2 << 10;
    toggle_lit(&mut s, candle);
    assert_eq!(frame(&s, candle), 3);
    toggle_lit(&mut s, candle);
    assert_eq!(frame(&s, candle), 2);
    with_data(
        &mut s,
        vec![(a4(-0x66dc + 2 * 0x54), 0x77u16.to_be_bytes().to_vec())],
    );
    assert!(in_type_list(&s, 0x77), "the 85th entry counts");
    assert!(!in_type_list(&s, 0x78));
}

// Merge-gate regressions (M2 review): each fails on the integration base 4ab938d.

fn of_kind(s: &State, kind: u16) -> Vec<Slot> {
    monsters(s)
        .into_iter()
        .filter(|&m| s.objects.kind(m) == kind)
        .collect()
}

#[test]
fn a_hydra_that_finds_no_tile_grows_no_heads() {
    // The egg lies inside the view without status 0x20: every hatchling after the first must
    // scatter out of view, and from (502, 500) no candidate tile is outside it, so the second
    // hydra is removed. 0x1319b2 goes on to grow heads on the freed slot (after the removal at
    // 0x131cfe-0x131d02, 0x131d06 jumps forward to 0x131d0e as for a placed hatchling); the
    // port ends that hatchling there.
    let mut s = monster_world();
    let egg = put(&mut s, 0x14f, 502, 500);
    s.objects.qq[egg as usize] = 100;
    s.objects.obj_status[egg as usize] = 0;
    create_in(&mut s, 0x16d, 0x08, 0x0802, egg); // two hydras, worktype 8
    hatch_egg(&mut s, egg);
    let hydras = of_kind(&s, 0x16d);
    let heads = of_kind(&s, 0x16e);
    assert_eq!(
        (hydras.len(), heads.len()),
        (1, 4),
        "one hydra with its four heads"
    );
    assert!(heads.iter().all(|&h| s.actors.worktype[h as usize] == 0xd));
}

#[test]
fn removing_an_equipped_hatchling_keeps_the_eggs_next_template() {
    // Template 1: two of type 0x168, which stands only on damaging tiles, so the second fails
    // its scatter and is removed with its spell (0x10c1d6 walks the contents with 0x10ae52,
    // the cursor the egg's template walk shares). Template 2, one winged gargoyle, must still
    // hatch.
    let mut s = monster_world();
    let egg = put(&mut s, 0x14f, 520, 500);
    s.objects.qq[egg as usize] = 100;
    s.objects.obj_status[egg as usize] = 0x20;
    create_in(&mut s, 0x16b, 0x08, 0x0801, egg); // linked second
    create_in(&mut s, 0x168, 0x08, 0x0802, egg); // linked first
    hatch_egg(&mut s, egg);
    assert_eq!(
        of_kind(&s, 0x168).len(),
        1,
        "the first stays, the second went"
    );
    assert_eq!(of_kind(&s, 0x16b).len(), 1, "the next template hatched");
}

#[test]
fn search_stops_at_a_content_that_refuses_to_move() {
    // A body may hold type 0x4c, which 0x10c9d2 always refuses; 0x129544 ignores the refusal
    // (0x1297c4) and finds the same content again forever. The port passes over it.
    let (gem_place, fixed_place, out) = crate::objects::tests::returns("search", || {
        let mut s = world();
        let parts = tagged(&[
            0x0012_8402,
            0x0012_842c,
            0x0012_8430,
            0x0012_8436,
            0x0012_8440,
        ]);
        with_data(&mut s, parts);
        s.actors.party.size = 1;
        s.actors.party.members[0] = 1;
        s.objects.type_frame[1] = 0x19a;
        s.objects.set_xyz(1, 500, 500, 0);
        crate::main_loop::test_support::link_placed_objects(&mut s);
        let body = put(&mut s, 0x63, 501, 500);
        let fixed = create_in(&mut s, 0x4c, 0x08, 0, body);
        let gem = create_in(&mut s, 0x4d, 0x08, 1, body); // first content, then the 0x4c
        search(&mut s, body, 501, 500);
        (s.objects.place(gem), s.objects.place(fixed), queued(&s))
    });
    assert_eq!(gem_place, 0, "the gem spilled onto the tile");
    assert_ne!(fixed_place, 0, "the 0x4c stays inside");
    assert_eq!(out.last().map(String::as_str), Some("<128440>"), "{out:?}");
}

#[test]
fn a_dropped_stack_with_no_free_slot_marks_nothing() {
    // 0x12a226 marks the new stack takeable without testing the -1 0x10bc4c gives when no
    // slot is free (0x12a750-0x12a758); the port must not index with it.
    let mut s = monster_world();
    s.ui.panel.member = 0;
    let stack = create_in(&mut s, 60, 0x10, 5, 1);
    s.ui.input.target = (501, 500);
    s.objects.free.1 = 0;
    let mut e = crate::Engine::with_game(s, crate::Files::default(), move |st, io| async move {
        let mut s = st.borrow_mut();
        drop::land(&mut s, &io, stack, 1, -1, 60, 2, 2);
    });
    for _ in 0..8 {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
    }
    assert!(e.finished());
    let s = e.state();
    let made = (0x101..0xc00).filter(|&o| s.objects.kind(o) == 60 && s.objects.place(o) == 0);
    assert_eq!(made.count(), 0, "nothing was created on the map");
}

#[test]
fn takeable_marks_and_passthroughs_ignore_slot_none() {
    let mut s = world();
    let before = s.objects.clone();
    crate::objects::mark_takeable(&mut s.objects, -1);
    crate::objects::toggle_passthrough(&mut s, -1);
    assert_eq!(s.objects, before);
}

#[test]
fn dismounting_a_rear_half_without_its_rider_does_nothing() {
    // A mounted-horse part in an object slot whose owner 0x1240fa cannot find comes back as
    // itself (0x10a5b0 returns the part); 0x12f8fe would index its 256-entry actor tables
    // with it.
    let mut s = monster_world();
    let rear = put(&mut s, 0x1af, 510, 500);
    assert!(rear >= 0x100);
    let before = s.objects.clone();
    dismount(&mut s, rear);
    assert_eq!(s.objects, before);
}

#[test]
fn a_keg_on_a_cyclic_chain_does_not_go_off() {
    // The keg and a bag each "inside" the other: 0x12db7e walks the holders to the map without
    // a bound (0x12dbdc-0x12dc0a); the port leaves the keg alone.
    let (kind, keg_status) = crate::objects::tests::returns("explode_keg", || {
        let mut s = world();
        let keg = put(&mut s, 0xdf, 7, 7);
        let bag = put(&mut s, 0x63, 8, 8);
        s.objects.obj_status[keg as usize] = 0x08;
        s.objects.obj_status[bag as usize] = 0x08;
        s.objects.set_parent(keg, bag);
        s.objects.set_parent(bag, keg);
        explode_keg(&mut s, keg);
        (s.objects.kind(keg), s.objects.status(keg))
    });
    assert_eq!((kind, keg_status), (0xdf, 0x08), "the keg stays as it was");
}

// Round-2 review regressions: each fails on the integration base 04982cc.

#[test]
fn a_serpent_head_without_a_body_slot_leaves_its_freed_slot_alone() {
    // No object slot is free, so the first body segment fails and 0x1319b2 removes the head
    // (0x131e54-0x131e58), then branches on to 0x131ff2 and writes the template's worktype,
    // combat mode and movement flags into the freed temporary-actor slot.
    let mut s = monster_world();
    let egg = put(&mut s, 0x14f, 520, 500);
    s.objects.qq[egg as usize] = 100;
    s.objects.obj_status[egg as usize] = 0x20;
    create_in(&mut s, 0x19d, 0x08, 0x0f02, egg); // one serpent, worktype 15, body of 2
    s.objects.free.1 = 0;
    hatch_egg(&mut s, egg);
    assert!(of_kind(&s, 0x19d).is_empty(), "the head went");
    let written: Vec<usize> = (0xe0..0x100)
        .filter(|&i| s.actors.worktype[i] == 0xf || s.actors.attrs.combat_mode[i] == 0xf)
        .collect();
    assert!(
        written.is_empty(),
        "freed slots given worktype 15: {written:x?}"
    );
}

#[test]
fn a_removed_serpent_head_still_sets_the_eggs_worktype() {
    // 0x1319b2 takes the worktype once per egg from the first template that reaches
    // 0x131ffe, a removed serpent head's included: the gargoyle after it gets the serpent's.
    let mut s = monster_world();
    let egg = put(&mut s, 0x14f, 520, 500);
    s.objects.qq[egg as usize] = 100;
    s.objects.obj_status[egg as usize] = 0x20;
    create_in(&mut s, 0x16b, 0x08, 0x0601, egg); // second: one gargoyle, worktype 6
    create_in(&mut s, 0x19d, 0x08, 0x0f02, egg); // first: the serpent, worktype 15
    s.objects.free.1 = 0;
    hatch_egg(&mut s, egg);
    assert!(of_kind(&s, 0x19d).is_empty());
    let g = of_kind(&s, 0x16b);
    assert_eq!(g.len(), 1);
    assert_eq!(s.actors.worktype[g[0] as usize], 0xf);
}

#[test]
fn a_hatchling_removed_after_its_scatter_still_sets_the_eggs_worktype() {
    // Template 1 (type 0x168, worktype 0) places its one; template 2's (0x168, worktype 6)
    // fails its scatter and is removed, and 0x1319b2 goes on to 0x131ffe, which takes 6 as the
    // egg's worktype; template 3's winged gargoyle gets it, not its own 8.
    let mut s = monster_world();
    let egg = put(&mut s, 0x14f, 520, 500);
    s.objects.qq[egg as usize] = 100;
    s.objects.obj_status[egg as usize] = 0x20;
    create_in(&mut s, 0x16b, 0x08, 0x0801, egg); // third
    create_in(&mut s, 0x168, 0x08, 0x0601, egg); // second
    create_in(&mut s, 0x168, 0x08, 0x0001, egg); // first
    hatch_egg(&mut s, egg);
    assert_eq!(of_kind(&s, 0x168).len(), 1);
    let g = of_kind(&s, 0x16b);
    assert_eq!(g.len(), 1);
    assert_eq!(s.actors.worktype[g[0] as usize], 6);
}

#[test]
fn search_joins_the_last_spilled_name_with_and_past_a_fixed_content() {
    // The look-ahead that picks the separator must skip the 0x4c the spill never moves: with
    // [gem, ring, 0x4c] the ring is the last name (the and line), and a 0x4c first in the body
    // must not keep the rest from spilling.
    // The comma, and, nothing and full-stop strings lie 4, 6 and 10 bytes apart: one short
    // token each.
    const ENDS: [(u32, &str); 4] = [
        (0x0012_842c, "<c>"),
        (0x0012_8430, "<a>"),
        (0x0012_8436, "<n>"),
        (0x0012_8440, "<d>"),
    ];
    fn run(order: &'static [u16]) -> (Vec<u8>, Vec<String>) {
        crate::objects::tests::returns("search", move || {
            let mut s = world();
            let mut parts = tagged(&[0x0012_8402]);
            parts.extend(
                ENDS.iter()
                    .map(|&(a, t)| (a, [t.as_bytes(), &[0]].concat())),
            );
            with_data(&mut s, parts);
            s.actors.party.size = 1;
            s.actors.party.members[0] = 1;
            s.objects.type_frame[1] = 0x19a;
            s.objects.set_xyz(1, 500, 500, 0);
            crate::main_loop::test_support::link_placed_objects(&mut s);
            let body = put(&mut s, 0x63, 501, 500);
            // create_in links after the owner: the last created is the first content.
            let made: Vec<Slot> = order
                .iter()
                .rev()
                .map(|&t| create_in(&mut s, t, 0x08, 1, body))
                .collect();
            search(&mut s, body, 501, 500);
            let places = made.iter().rev().map(|&o| s.objects.place(o)).collect();
            let ends = queued(&s)
                .into_iter()
                .filter(|l| ENDS.iter().any(|&(_, t)| t == l))
                .collect();
            (places, ends)
        })
    }
    let (places, ends) = run(&[0x4d, 0x4e, 0x4c]);
    assert_eq!(places, [0, 0, 0x08], "gem and ring out, the 0x4c inside");
    assert_eq!(ends, ["<a>", "<d>"]);
    let (places, ends) = run(&[0x4c, 0x4d]);
    assert_eq!(places, [0x08, 0], "the gem spills past the 0x4c");
    assert_eq!(ends, ["<d>"]);
}
