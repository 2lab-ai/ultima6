//! Unit tests on synthetic data for the spells of the second magic unit (no game files: the
//! strings print nothing, the A4 tables read as zeros, the empty map lets anything stand
//! anywhere, the seams take their fallbacks).

use std::cell::RefCell;
use std::rc::Rc;

use super::tests::{run, world, world_with};
use crate::engine::Key;
use crate::objects::Slot;
use crate::rng::Rng;
use crate::state::State;
use u6_formats::position::Position;

/// The objects of type `kind` at (x, y).
fn kinds_at(s: &mut State, x: u16, y: u16, kind: u16) -> Vec<Slot> {
    let mut out = Vec::new();
    let mut o = crate::objects::begin_at(&mut s.objects, &s.tables, x, y, 0);
    while o >= 0 {
        if s.objects.kind(o) == kind {
            out.push(o);
        }
        o = crate::objects::next_at(&mut s.objects, &s.tables);
    }
    out
}

/// The eight directions north first, clockwise.
fn compass(s: &mut State) {
    s.objects.defs.dir_dx = vec![0, 1, 1, 1, 0, -1, -1, -1];
    s.objects.defs.dir_dy = vec![-1, -1, 0, 1, 1, 1, 0, -1];
}

#[test]
fn degrees_follow_the_shifts() {
    let p = |x, y, z| Position { x, y, z };
    assert_eq!(super::travel::degrees(p(307, 352, 0)), (-1, 0));
    assert_eq!(super::travel::degrees(p(0x130 + 16, 0x168 + 8, 0)), (1, 2));
    assert_eq!(super::travel::degrees(p(100, 100, 1)), (5, 12), "× 4 below");
    assert_eq!(super::travel::degrees(p(0, 0, 0)), (-45, -38));
}

#[test]
fn parse_number_reads_digits_only() {
    let s = State::empty();
    assert_eq!(super::seams::parse_number(&s, b"12"), 12);
    assert_eq!(super::seams::parse_number(&s, b""), -1);
    assert_eq!(super::seams::parse_number(&s, b"1a"), -2);
}

#[test]
fn flights_list_the_actors_met_and_stop_past_the_range() {
    let s = world_with(&[(2, 0x178, 100, 98), (3, 0x178, 100, 91)]);
    let s = run(s, &[], |st, io| async move {
        let mut s = st.borrow_mut();
        let (mut xs, mut ys) = ([100i16], [90i16]);
        let (r, list) = super::seams::fly(&mut s, &io, 100, 100, &mut xs, &mut ys, 7, 0, 0);
        assert_eq!((r, list), (0, vec![2]), "actor 3 lies past range 7");
        assert_eq!((xs[0], ys[0]), (100, 93));
        let (mut xs, mut ys) = ([100i16], [97i16]);
        let (r, list) = super::seams::fly(&mut s, &io, 100, 100, &mut xs, &mut ys, 7, 0, 0);
        assert_eq!((r, list), (1, vec![2]));
        s.actors.attrs.hp[2] = 7;
    });
    assert_eq!(s.actors.attrs.hp[2], 7);
}

#[test]
fn the_wind_fans_out_around_the_target() {
    // No `game`: the A4-0x6924 grid reads 0 (north twice), so the five targets are the
    // target, one north of it twice and two north of it twice.
    let mut s = world_with(&[
        (2, 0x178, 100, 96),
        (3, 0x178, 100, 95),
        (4, 0x178, 101, 95),
    ]);
    compass(&mut s);
    run(s, &[], |st, io| async move {
        let list = super::area::wind_flight(&mut st.borrow_mut(), &io, 1, 100, 97, 0x18c);
        assert_eq!(list, vec![2, 3], "the column north of the caster");
    });
}

#[test]
fn mass_spells_act_on_the_actors_the_blast_reaches() {
    // No `game`: the blast offsets read 0, so the blast reaches the target tile only.
    let mut s = world_with(&[(2, 0x178, 102, 100), (3, 0x178, 103, 100)]);
    s.actors.attrs.status[2] = 4;
    s.objects.type_frame[2] = 0x179;
    s.actors.attrs.base_type_frame[2] = 0x178;
    s.ui.input.target = (102, 100);
    let s = run(s, &[], |st, io| async move {
        super::area::area(&st, &io, 0x25, 0x18b, 0x17f, 1).await;
        for spell in [0x55, 0x66, 0x67] {
            st.borrow_mut().ui.input.target = (102, 100);
            super::area::area(&st, &io, spell, 0x188, 0x18a, 1).await;
        }
    });
    assert_eq!(s.actors.attrs.status[2], 1, "awake, then protected");
    assert_eq!(s.objects.type_frame[2], 0x178, "the base type back");
    assert_eq!(s.objects.obj_status[2] & 0x42, 0x42, "cursed and invisible");
    assert_eq!(
        (s.actors.attrs.status[3], s.objects.obj_status[3] & 0x42),
        (0, 0)
    );
}

#[test]
fn fields_lay_the_type_of_their_spell() {
    for (spell, kind) in [(0x33, 0x13d), (0x37, 0x13e), (0x38, 0x140), (0x40, 0x13f)] {
        let mut s = world();
        s.ui.input.target = (103, 100);
        let mut s = run(s, &[], move |st, io| async move {
            super::area::field(&st, &io, spell, 0x17e, 1).await;
        });
        assert_eq!(
            kinds_at(&mut s, 103, 100, kind).len(),
            1,
            "spell {spell:#x}"
        );
    }
}

#[test]
fn no_location_does_nothing() {
    let mut s = world();
    s.ui.input.target = (-1, -1);
    let s0 = s.clone();
    let s = run(s, &[], |st, io| async move {
        super::area::field(&st, &io, 0x33, 0x17e, 1).await;
        super::area::web(&st, &io, 1).await;
        super::area::reappear(&st, &io, 1).await;
    });
    assert_eq!(s.objects.type_frame, s0.objects.type_frame);
}

#[test]
fn web_covers_the_square_around_the_target() {
    let mut s = world();
    compass(&mut s);
    s.ui.input.target = (104, 100);
    let mut s = run(s, &[], |st, io| async move {
        super::area::web(&st, &io, 1).await;
    });
    let n: usize = (103..=105)
        .flat_map(|x| (99..=101).map(move |y| (x, y)))
        .map(|(x, y)| kinds_at(&mut s, x, y, super::area::WEB).len())
        .sum();
    assert_eq!(n, 9);
}

#[test]
fn reappear_needs_a_vanished_type() {
    let mut s = world();
    s.ui.input.target = (104, 100);
    s.actors.set_tail_word(0x22, 0x0055 | 2 << 10);
    let mut s = run(s, &[], |st, io| async move {
        super::area::reappear(&st, &io, 1).await;
    });
    let back = kinds_at(&mut s, 104, 100, 0x55);
    assert_eq!(back.len(), 1);
    assert_eq!(s.objects.type_frame[back[0] as usize], 0x0055 | 2 << 10);
    assert_eq!(s.objects.qq[back[0] as usize], 1);
    assert_eq!(s.actors.tail_word(0x22), 0, "forgotten");
}

#[test]
fn conjure_picks_its_creature_with_the_first_draw() {
    for seed in [1, 2, 3, 4, 5, 6] {
        let mut s = world();
        s.rng = Rng::seeded(seed);
        let mut r = s.rng;
        let kind = match r.range(0, 0xf) {
            0..=3 => 0x173,
            4..=6 => 0x156,
            7..=9 => 0x169,
            10..=12 => 0x158,
            _ => 0x166,
        };
        s.actors.attrs.status[1] = 0x40;
        let s = run(s, &[], |st, io| async move {
            super::creatures::summon(&st, &io, 0x31, 1).await;
        });
        let made: Vec<usize> = (0xe0..0x100)
            .filter(|&a| s.objects.type_frame[a] != 0)
            .collect();
        assert_eq!(made.len(), 1, "seed {seed}");
        let a = made[0];
        assert_eq!(s.objects.type_frame[a], kind, "seed {seed}");
        assert_eq!(
            (s.actors.attrs.status[a] & 0x60, s.actors.worktype[a]),
            (0x40, 8)
        );
        let p = s.objects.location[a];
        assert!(p.x.abs_diff(100) <= 5 && p.y.abs_diff(100) <= 5);
    }
}

#[test]
fn a_daemon_that_resists_turns_hostile() {
    // Daemon intelligence 0 against the caster's 1: random 1..30 < 0 − 1 + 15 resists mostly.
    let mut s = world();
    s.rng = Rng::seeded(11);
    s.actors.attrs.status[1] = 0x40;
    s.actors.attrs.intelligence[1] = 1;
    let s = run(s, &[], |st, io| async move {
        super::creatures::summon(&st, &io, 0x77, 1).await;
    });
    let a = (0xe0..0x100)
        .find(|&a| s.objects.type_frame[a] == 0x16f)
        .unwrap();
    assert!(matches!(s.actors.attrs.status[a] & 0x60, 0x40 | 0x60));
}

#[test]
fn insect_swarm_places_eight() {
    let s = run(world(), &[], |st, io| async move {
        super::creatures::insect_swarm(&st, &io, 0x42).await;
    });
    let n = (0xe0..0x100)
        .filter(|&a| s.objects.type_frame[a] == 0x157)
        .count();
    assert_eq!(n, 8);
}

#[test]
fn slime_and_tremor_strike_evil_actors_only() {
    let mut s = world_with(&[(5, 0x178, 101, 100), (6, 0x178, 99, 100)]);
    s.actors.attrs.status[5] = 0x20;
    s.actors.attrs.status[6] = 0x40;
    s.actors.attrs.intelligence[1] = 30;
    s.actors.attrs.hp[5] = 50;
    s.actors.attrs.hp[6] = 50;
    s.rng = Rng::seeded(3);
    let s = run(s, &[], |st, io| async move {
        super::creatures::tremor(&st, &io).await;
    });
    assert!((20..50).contains(&s.actors.attrs.hp[5]));
    assert_eq!(s.actors.attrs.hp[6], 50);
    let s = run(s, &[], |st, io| async move {
        super::creatures::slime(&st, &io).await;
    });
    assert_eq!(
        (s.objects.type_frame[5], s.objects.type_frame[6]),
        (0x177, 0x178)
    );
}

#[test]
fn armageddon_spares_the_avatar_and_lord_british() {
    let mut s = world_with(&[
        (2, 0x178, 101, 100),
        (5, 0x178, 99, 100),
        (7, 0x178, 99, 99),
    ]);
    s.actors.party.size = 2;
    let s = run(s, &[], |st, io| async move {
        super::creatures::armageddon(&st, &io).await;
    });
    let left: Vec<usize> = (0..0x100)
        .filter(|&a| s.objects.type_frame[a] != 0)
        .collect();
    assert_eq!(left, [1, 5]);
    assert_eq!((s.actors.party.size, s.map.flag_7afb), (1, 1));
}

#[test]
fn spell_0x80_turns_the_caster_invisible() {
    let s = run(world(), &[], |st, io| async move {
        super::creatures::spell_0x80(&st, &io).await;
        assert_eq!(st.borrow().objects.obj_status[1] & 2, 2);
        super::creatures::spell_0x80(&st, &io).await;
    });
    assert_eq!(s.objects.obj_status[1] & 2, 2);
}

#[test]
fn wing_strike_hits_on_the_foe_s_column() {
    // No `game`: the step sign reads 0, so the dragon stays on the centre column all ten steps
    // and a foe on that column is struck every step.
    let mut s = world_with(&[(5, 0x178, 100, 97), (6, 0x178, 102, 97)]);
    s.actors.attrs.status[5] = 0x20;
    s.actors.attrs.status[6] = 0x40;
    s.actors.attrs.hp[5] = 0xff;
    s.actors.attrs.hp[6] = 0xff;
    s.rng = Rng::seeded(5);
    let mut r = s.rng;
    let total: i16 = (0..10).map(|_| r.range(1, 0x14) as i16).sum();
    let s = run(s, &[], |st, io| async move {
        super::creatures::wing_strike(&st, &io).await;
    });
    assert_eq!(i16::from(s.actors.attrs.hp[5]), 0xff - total);
    assert_eq!(s.actors.attrs.hp[6], 0xff, "good actors are left alone");
    assert!(
        (0x100..crate::objects::OBJECT_SLOTS).all(|o| s.objects.type_frame[o] & 0x3ff != 0x19b)
    );
}

/// 0x133562's sprite calls: a stone (sprite 7) over its saved background reaches the screen
/// through the changed rectangle (whole bytes) as one frame, and the back buffer gets the
/// background back; the hit mark (tile 0x101) copies its 16 x 16 cell to the screen and leaves
/// the back buffer as it was; the restores grow the rectangle for the next copy.
#[test]
fn hail_stones_and_the_hit_mark_reach_the_screen_only() {
    type Seen = ((u8, u8, u8, u8), bool, bool, Vec<u8>, [i16; 4], usize);
    let mut s = world_with(&[]);
    s.gfx.records[0x101].pixels = vec![5; 256];
    s.gfx.records[0x101].transparent = vec![false; 256];
    let seen: Rc<RefCell<Option<Seen>>> = Rc::new(RefCell::new(None));
    let out = Rc::clone(&seen);
    run(s, &[], move |st, io| async move {
        io.draw_back(|b| {
            for y in 0..200 {
                for x in 0..320 {
                    b.set(x, y, ((x + y) % 7) as u8);
                }
            }
        });
        let dump = |b: &mut u6_render::Framebuffer| b.pixels().to_vec();
        let back0 = io.draw_back(dump);
        let mut s = st.borrow_mut();
        super::seams::hail_start(&mut s);
        let stone = &mut s.gfx.sprites.slots[7];
        stone.fill(0xff);
        stone[0] = 9;
        stone[17] = 0x2a;
        super::seams::draw_hail(&mut s, &io, &[(40, 50)]);
        let stone_px = io.draw(|f| {
            let g = |x, y| f.get(x, y).unwrap_or(0xee);
            (g(32, 42), g(33, 43), g(34, 42), g(56, 42))
        });
        let restored = io.draw_back(dump) == back0;
        let rect = s.combat.dirty;
        super::seams::hail_hit(&mut s, &io, 88, 88);
        let cell = io.draw(|f| {
            (80..96)
                .flat_map(|y| (80..96).map(move |x| (x, y)))
                .map(|(x, y)| f.get(x, y).unwrap_or(0xee))
                .collect()
        });
        let still = io.draw_back(dump) == back0;
        *out.borrow_mut() = Some((
            stone_px,
            restored,
            still,
            cell,
            rect,
            s.combat.effects.len(),
        ));
    });
    let (stone_px, restored, still, cell, rect, frames) = seen.borrow_mut().take().unwrap();
    assert_eq!(
        stone_px,
        (9, 0x0a, (34 + 42) % 7, 0),
        "the stone's pixels (5 bits), its background, nothing past the copied bytes"
    );
    assert!(
        restored,
        "the stone's background is back in the back buffer"
    );
    assert_eq!(
        rect,
        [32, 42, 48, 58],
        "the restore grew the rectangle again"
    );
    assert!(cell.iter().all(|&p| p == 5), "the hit mark on the screen");
    assert!(still, "the hit mark's background is back");
    assert_eq!(frames, 2, "one frame per copy");
}

#[test]
fn the_hail_storm_ends() {
    let mut s = world_with(&[(2, 0x178, 100, 96)]);
    s.actors.attrs.hp[2] = 100;
    s.rng = Rng::seeded(8);
    let mut r = s.rng;
    let stones = r.range(10, 0x1e);
    let s = run(s, &[], |st, io| async move {
        super::area::hail(&mut st.borrow_mut(), &io, 100, 96);
    });
    let hits = 100 - u16::from(s.actors.attrs.hp[2]);
    assert!(hits <= stones, "{hits} hits of {stones} stones");
}

/// An object of `kind` at (x, y), its weight byte `w`, selected with its place as 0x1225c0
/// would leave them.
fn pick(s: &mut State, kind: u16, w: u8, x: u16, y: u16) -> Slot {
    if s.tables.tileflags.weight.len() < 0x400 {
        s.tables.tileflags.weight = vec![0; 0x400];
    }
    s.tables.tileflags.weight[usize::from(kind)] = w;
    let o = crate::objects::create_on_map(&mut s.objects, kind, 0, 1, x, y, 0);
    s.ui.input.selection = o;
    s.ui.input.target = (x as i16, y as i16);
    o
}

#[test]
fn telekinesis_needs_an_object_with_weight_and_a_direction() {
    // An actor: nothing happens.
    let mut s = world_with(&[(2, 0x178, 102, 100)]);
    s.ui.input.selection = 2;
    s.ui.input.target = (102, 100);
    let s = run(s, &[], |st, io| async move {
        super::items::telekinesis(&st, &io, 1).await;
    });
    assert_eq!(
        s.objects.location[2],
        Position {
            x: 102,
            y: 100,
            z: 0
        }
    );
    // Weight 0: the not-possible line (0x1159f8), no key read.
    let mut s = world();
    let o = pick(&mut s, 0x77, 0, 102, 100);
    let s = run(s, &[], |st, io| async move {
        super::items::telekinesis(&st, &io, 1).await;
    });
    assert_eq!(
        s.objects.location[o as usize],
        Position {
            x: 102,
            y: 100,
            z: 0
        }
    );
    // A key that is not a direction: the no-direction word (0x115a14), the object stays.
    let mut s = world();
    let o = pick(&mut s, 0x77, 5, 102, 100);
    let s = run(s, &[Key::Char('x')], |st, io| async move {
        super::items::telekinesis(&st, &io, 1).await;
    });
    assert_eq!(
        s.objects.location[o as usize],
        Position {
            x: 102,
            y: 100,
            z: 0
        }
    );
}

#[test]
fn enchant_needs_a_wand_with_room() {
    let mut s = world();
    let o = pick(&mut s, 0x77, 5, 102, 100);
    let s = run(s, &[], |st, io| async move {
        super::items::enchant(&st, &io, 1).await;
    });
    assert_eq!(crate::objects::first_child(&mut s.clone().objects, o), -1);
    assert_eq!(s.ui.panel.mode, 0x92, "the leader's inventory is up");
    let mut s = world();
    let wand = crate::objects::create_in(&mut s, 0x4e, crate::objects::IN_INVENTORY, 0, 1);
    for _ in 0..10 {
        crate::objects::create_in(&mut s, 0x150, crate::objects::IN_CONTAINER, 1 << 8, wand);
    }
    s.ui.input.selection = wand;
    let mut s = run(s, &[], |st, io| async move {
        super::items::enchant(&st, &io, 1).await;
    });
    let mut n = 0;
    let mut c = crate::objects::first_child(&mut s.objects, wand);
    while c >= 0 {
        n += 1;
        c = crate::objects::next_child(&mut s.objects);
    }
    assert_eq!(n, 10, "it's full");
    assert_ne!(s.ui.panel.mode, super::BOOK_PANEL, "no book opened");
}

#[test]
fn seance_needs_a_body() {
    let mut s = world();
    pick(&mut s, 0x77, 5, 102, 100);
    let s = run(s, &[], |st, io| async move {
        super::items::seance(&st, &io, 1).await;
    });
    assert!(!s.converse.by_spell && !s.converse.active);
}

#[test]
fn pickpocket_needs_an_actor() {
    let mut s = world();
    pick(&mut s, 0x77, 5, 102, 100);
    let s = run(s, &[], |st, io| async move {
        super::items::pickpocket(&st, &io, 1).await;
    });
    assert_eq!(s.ui.panel.open_container, 0, "no victim's inventory opened");
    assert!(!s.ui.panel.no_rows);
}

#[test]
fn peer_spends_a_gem() {
    let mut s = world();
    let gem = crate::objects::create_in(&mut s, 0x4d, crate::objects::IN_INVENTORY, 2, 1);
    // 0x12c54a is objects::commands' port; each overview (0x13410a) waits for a key.
    let keys = [Key::Char(' '), Key::Char(' ')];
    let s = run(s, &keys, move |st, io| async move {
        crate::objects::commands::use_up(&st, &io, gem).await;
        assert_eq!(st.borrow().objects.qq[gem as usize] & 0xff, 1);
        crate::objects::commands::use_up(&st, &io, gem).await;
    });
    assert_eq!(s.objects.type_frame[gem as usize], 0, "the last one goes");
}

#[test]
fn gate_travel_needs_a_placed_stone() {
    // Stone 0 at (0, 0, 0): nothing; 9: not a phase.
    for typed in ["1", "9", "x"] {
        let keys: Vec<Key> = typed.chars().map(Key::Char).chain([Key::Enter]).collect();
        let s = run(world(), &keys, |st, io| async move {
            super::travel::gate_travel(&st, &io).await;
        });
        assert_eq!(
            s.map.centre,
            Position {
                x: 100,
                y: 100,
                z: 0
            },
            "{typed:?}"
        );
    }
}

#[test]
fn resurrect_and_rejoin_brings_a_member_back() {
    let mut s = world_with(&[(3, 0x153, 7, 7)]);
    s.actors.attrs.base_type_frame[3] = 0x178;
    s.actors.attrs.status[3] = 0x80 | 0x10;
    let c = s.map.centre;
    let body = crate::objects::create_on_map(&mut s.objects, 0x153, 0, 3 << 8, c.x, c.y, 0);
    let s = run(s, &[], move |st, io| async move {
        super::resurrect_and_rejoin(&st, &io, body).await;
    });
    assert_eq!(s.actors.attrs.hp[3], 1);
    assert_eq!(s.actors.party.size, 2, "0x112daa took it back");
    assert_eq!(s.actors.party.members[1], 3);
    assert_eq!(s.actors.worktype[3], 1);
}
