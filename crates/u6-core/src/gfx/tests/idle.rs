//! The actor idle-frame pass of 0x10202e, branch by branch. RNG states are searched so the
//! draws a branch needs come out as wanted; expectations follow the assembly, not the port.

use u6_formats::position::Position;

use crate::geom::{HALF_H, HALF_W};
use crate::gfx::GfxState;
use crate::rng::Rng;
use crate::state::State;

/// The pass with `g` as the state's graphics (its tables and the due word).
fn actor_idle_frames(s: &mut State, g: &GfxState) -> bool {
    let saved = std::mem::replace(&mut s.gfx, g.clone());
    let changed = crate::gfx::idle::actor_idle_frames(s);
    s.gfx = saved;
    changed
}

const CENTRE: Position = Position {
    x: 100,
    y: 100,
    z: 0,
};

/// A state whose RNG makes `want` (run on a copy) true; the copy's state after `want` is
/// returned too so the test can check the exact number of draws.
fn seeded(mut want: impl FnMut(&mut Rng) -> bool) -> (Rng, Rng) {
    for s in 1u32..1 << 22 {
        let mut probe = Rng::new();
        probe.set_state(s);
        let start = probe;
        if want(&mut probe) {
            return (start, probe);
        }
    }
    panic!("no RNG state found");
}

fn world() -> (State, GfxState) {
    let mut s = State::empty();
    s.map.centre = CENTRE;
    let mut g = GfxState {
        actor_anim_due: 1,
        ..GfxState::default()
    };
    g.tables.dir_dx = [0, 1, 1, 1, 0, -1, -1, -1];
    g.tables.dir_dy = [-1, -1, 0, 1, 1, 1, 0, -1];
    (s, g)
}

fn put(s: &mut State, slot: u16, ty: u16, frame: u16, at: Position) {
    s.objects.location[usize::from(slot)] = at;
    s.objects.type_frame[usize::from(slot)] = ty | frame << 10;
}

fn word(s: &State, slot: u16) -> (u16, u16) {
    let w = s.objects.type_frame[usize::from(slot)];
    (w & 0x3ff, w >> 10)
}

/// Skipped: outside the view on either axis (6 tiles from the centre in Original; one tile past
/// the view's half, spec 13 Decision 5), on another level, dead, or with the due flag clear.
#[test]
fn pre_checks_skip_without_drawing() {
    let (mut s, mut g) = world();
    let (past_x, past_y) = (HALF_W as u16 + 1, HALF_H as u16 + 1);
    let east = Position {
        x: CENTRE.x + past_x,
        ..CENTRE
    };
    let north = Position {
        y: CENTRE.y - past_y,
        ..CENTRE
    };
    put(&mut s, 1, 0x19a, 0, east);
    put(&mut s, 2, 0x19a, 0, Position { z: 1, ..CENTRE });
    put(&mut s, 3, 0x19a, 0, north);
    put(&mut s, 4, 0x19a, 0, CENTRE);
    s.actors.attrs.status[4] = 0x10;
    let before = s.rng;
    assert!(!actor_idle_frames(&mut s, &g));
    assert_eq!(s.rng, before);
    // On the view's edge (in the view), but the due flag is clear.
    put(
        &mut s,
        5,
        0x19a,
        0,
        Position {
            x: CENTRE.x - HALF_W as u16,
            y: CENTRE.y + HALF_H as u16,
            z: 0,
        },
    );
    g.actor_anim_due = 0;
    assert!(!actor_idle_frames(&mut s, &g));
    assert_eq!(s.rng, before);
}

#[test]
fn person_idle_cycle_and_the_0x187_cases() {
    // Phase 1: roll(0, 0x3f) == 0, then the new frame is roll(0, 2).
    let (start, end) = seeded(|r| r.range(0, 0x3f) == 0 && r.range(0, 2) == 2);
    let (mut s, g) = world();
    s.rng = start;
    put(&mut s, 1, 0x19a, 4 + 1, CENTRE);
    assert!(actor_idle_frames(&mut s, &g));
    assert_eq!(word(&s, 1), (0x19a, 4 + 2));
    assert_eq!(s.rng, end);
    // 0x187: roll(0, 3) == 0 flips bit 0.
    let (start, _) = seeded(|r| r.range(0, 3) == 0);
    s.rng = start;
    put(&mut s, 1, 0x187, 4, CENTRE);
    actor_idle_frames(&mut s, &g);
    assert_eq!(word(&s, 1), (0x187, 5));
    // 0x187: else roll(0, 15) == 0 turns it into 0x181 frame 8.
    let (start, _) = seeded(|r| r.range(0, 3) != 0 && r.range(0, 0xf) == 0);
    s.rng = start;
    put(&mut s, 1, 0x187, 5, CENTRE);
    actor_idle_frames(&mut s, &g);
    assert_eq!(word(&s, 1), (0x181, 8));
    // 0x181 at phase 3 (no idle roll) with worktype 0x94: roll(0, 15) == 0 → 0x187 frame 0.
    let (start, end) = seeded(|r| r.range(0, 0xf) == 0);
    s.rng = start;
    put(&mut s, 1, 0x181, 7, CENTRE);
    s.actors.worktype[1] = 0x94;
    actor_idle_frames(&mut s, &g);
    assert_eq!(word(&s, 1), (0x187, 0));
    assert_eq!(s.rng, end);
}

#[test]
fn modulo_cycles_of_0x16b_and_0x16a() {
    // 0x16b frame 4: base 3, phase 1 → roll(0x3f) == 0, frame = 3 + roll(0, 2).
    let (start, _) = seeded(|r| r.range(0, 0x3f) == 0 && r.range(0, 2) == 1);
    let (mut s, g) = world();
    s.rng = start;
    put(&mut s, 1, 0x16b, 4, CENTRE);
    actor_idle_frames(&mut s, &g);
    assert_eq!(word(&s, 1).1, 4);
    // 0x16a frame 15: base 12, phase 3 → roll(0x1f) == 0 → phase 7 → frame 19.
    let (start, _) = seeded(|r| r.range(0, 0x1f) == 0);
    s.rng = start;
    put(&mut s, 1, 0x16a, 15, CENTRE);
    actor_idle_frames(&mut s, &g);
    assert_eq!(word(&s, 1).1, 19);
    // Phase 7: roll(0x3f) == 0, then roll(0, 1) == 0 → phase 11.
    let (start, _) = seeded(|r| r.range(0, 0x3f) == 0 && r.range(0, 1) == 0);
    s.rng = start;
    put(&mut s, 1, 0x16a, 12 + 7, CENTRE);
    actor_idle_frames(&mut s, &g);
    assert_eq!(word(&s, 1).1, 12 + 11);
}

#[test]
fn up_down_walkers_turn_at_their_ends() {
    // 0x158 going up from 2: 3 reached → bit 7 cleared, frame 3.
    let (start, _) = seeded(|r| r.range(0, 1) != 0);
    let (mut s, g) = world();
    s.rng = start;
    put(&mut s, 1, 0x158, 2, CENTRE);
    s.actors.attrs.movement_flags[1] = 0x81;
    actor_idle_frames(&mut s, &g);
    assert_eq!((word(&s, 1).1, s.actors.attrs.movement_flags[1]), (3, 0x01));
    // Going down from 1: 0 reached → bit 7 set, frame 0.
    s.rng = start;
    put(&mut s, 1, 0x16c, 1, CENTRE);
    actor_idle_frames(&mut s, &g);
    assert_eq!((word(&s, 1).1, s.actors.attrs.movement_flags[1]), (0, 0x81));
    // 0x165 at 0 never moves and draws nothing.
    let before = s.rng;
    put(&mut s, 1, 0x165, 0, CENTRE);
    assert!(!actor_idle_frames(&mut s, &g));
    assert_eq!(s.rng, before);
    // 0x165 up past 3 (bit 7 clear means up here): turns, frame 2.
    s.rng = start;
    put(&mut s, 1, 0x165, 3, CENTRE);
    s.actors.attrs.movement_flags[1] = 0;
    actor_idle_frames(&mut s, &g);
    assert_eq!((word(&s, 1).1, s.actors.attrs.movement_flags[1]), (2, 0x80));
}

#[test]
fn toggles_and_two_tile_creatures() {
    let (start, _) = seeded(|r| r.range(0, 7) == 0);
    let (mut s, g) = world();
    s.rng = start;
    put(&mut s, 1, 0x15c, 2, CENTRE);
    actor_idle_frames(&mut s, &g);
    assert_eq!(word(&s, 1).1, 3);
    // 0x1ac also needs roll(0, 3) == 0.
    let (start, end) = seeded(|r| r.range(0, 7) == 0 && r.range(0, 3) != 0);
    s.rng = start;
    put(&mut s, 1, 0x1ac, 2, CENTRE);
    assert!(!actor_idle_frames(&mut s, &g));
    assert_eq!((word(&s, 1).1, s.rng), (2, end));
    // An unlisted type keeps its frame and draws nothing.
    let before = s.rng;
    put(&mut s, 1, 0x150, 5, CENTRE);
    assert!(!actor_idle_frames(&mut s, &g));
    assert_eq!((word(&s, 1).1, s.rng), (5, before));
}

#[test]
fn hydra_steps_the_neighbour_head_and_itself() {
    // Draw 1: the head in direction 2 (east) moves; draw 2: the body moves; frame 0 → up.
    let (start, end) = seeded(|r| r.range(0, 1) != 0 && r.range(0, 1) != 0);
    let (mut s, g) = world();
    s.rng = start;
    put(&mut s, 1, 0x176, 0, CENTRE);
    // The neighbour head is a map object in the list (the walk 0x10ad1a finds it there).
    crate::objects::init_slots(&mut s.objects);
    crate::objects::link_actors(&mut s.objects);
    let head = crate::objects::create_on_map(&mut s.objects, 0x1a9 | 9 << 10, 0, 0, 101, 100, 0);
    let head = head as u16;
    assert!(actor_idle_frames(&mut s, &g));
    assert_eq!(word(&s, head), (0x1a9, 8 + 2));
    assert_eq!((word(&s, 1).1, s.actors.attrs.movement_flags[1]), (1, 0x80));
    assert_eq!(s.rng, end);
}
