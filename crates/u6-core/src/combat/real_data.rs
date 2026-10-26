//! Real-data receipt of the combat code: the shipped start state (its combat byte is set, so
//! the boot begins combat 0x121c64), two gargoyles made next to the party the way 0x131646
//! makes a monster (the start egg is kept from hatching), the hero's Attack on one
//! (0x124caa with a confirmed cross-hair), then world turns: the party fights in its combat
//! modes, the gargoyles assault (worktype 8). Skips where the zip is missing; prints slots,
//! counts and numbers only (no names, no message text). The messages it looks for are read from
//! the user's `game` at run time. With `U6_COMBAT_FRAMES=<dir>` it writes the frames the effect
//! playback showed as binary PPM files there (outside the repository).
#![allow(clippy::disallowed_methods)] // tests may read files and the environment

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use u6_intake::{InputFile, intake};

use crate::engine::{Engine, InputEvent, Io, Key};
use crate::files::Files;
use crate::image::A4;
use crate::state::State;
use u6_testkit::Quiet;

const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";
/// The two creature types named after gargoyles in the monster list.
const GARGOYLE_TYPES: [u16; 2] = [0x16a, 0x16b];
/// Messages the receipt looks for: the Attack command word and its weapon lines (with a
/// weapon, bare hands), the death line and the graze line (A4 − offset).
const ATTACK_WORD: u32 = A4 - 0x769b;
const WEAPON_LINES: [u32; 2] = [A4 - 0x76e5, A4 - 0x76f6];
const KILLED: u32 = A4 - 0x7716;
const GRAZED: u32 = A4 - 0x7723;

/// The words of a one-argument message format after its `%s`, as the message scroll shows
/// them (read from the user's `game`).
fn after_argument(io: &Io, at: u32) -> String {
    let fmt = crate::main_loop::game_string(io, at);
    let fmt = String::from_utf8_lossy(&fmt);
    let tail = fmt.split_once("%s").map_or(&*fmt, |(_, t)| t);
    tail.trim().to_string()
}

fn start() -> Option<(State, Files)> {
    if !Path::new(ZIP).exists() {
        eprintln!("skipped: {ZIP} not present");
        return None;
    }
    let data = std::fs::read(ZIP).quiet();
    let r = intake(vec![InputFile {
        name: "ultima-vi.zip".into(),
        data,
    }]);
    let files = Files::new(r.files);
    let mut state = State::new_game(&files).quiet();
    crate::objects::load_state_objects(&mut state);
    state.map.flag_784e = 1;
    // The start window's egg stays unhatched here (A4-0x7afb, which 0x1321dc / 0x1319b2 check):
    // since the objects port hatches it, its gargoyles would join the two this receipt makes.
    state.map.flag_7afb = 1;
    crate::map::region::refresh_region(&mut state);
    state.map.flag_7afb = 0;
    crate::map::view::update_view(&mut state);
    Some((state, files))
}

/// A monster of `kind` at (x, y) as 0x131646 makes one: a temporary actor (0x10bec8), stats
/// varied from the creature tables (0x1311d2), level from the hit points, the table's status,
/// magic, 100 experience, combat mode and worktype 8; the winged gargoyle gets frame 19. (Its
/// equipment, 0x13121a, is an objects stub.)
fn spawn(s: &mut State, kind: u16, x: u16, y: u16) -> i16 {
    let z = u16::from(s.map.centre.z);
    let a = crate::objects::create_actor(s, kind, x, y, z);
    if a < 0 {
        return a;
    }
    let i = crate::objects::monster_index(&s.objects, kind);
    let t = s.combat.tables.clone();
    let stat = |n: usize| i16::from(t.creature_stat(n, i));
    let u = a as usize;
    s.actors.attrs.strength[u] = crate::objects::vary(s, stat(0)) as u8;
    s.actors.attrs.dexterity[u] = crate::objects::vary(s, stat(1)) as u8;
    s.actors.attrs.intelligence[u] = crate::objects::vary(s, stat(2)) as u8;
    s.actors.attrs.hp[u] = crate::objects::vary(s, stat(5)) as u8;
    s.actors.attrs.level[u] = ((i16::from(s.actors.attrs.hp[u]) + 0x1d) / 0x1e) as u8;
    s.actors.attrs.status[u] = t.creature_status.get(i as usize).copied().unwrap_or(0);
    s.actors.attrs.magic[u] = crate::actors::leaves::max_magic(s, a as u16) as u8;
    s.actors.movement_points[u] = s.actors.attrs.dexterity[u];
    s.actors.attrs.exp[u] = 100;
    s.actors.attrs.combat_mode[u] = 8;
    s.actors.worktype[u] = 8;
    if kind == 0x16a || kind == 0x1a8 {
        s.objects.type_frame[u] = (s.objects.type_frame[u] & 0x3ff) + 0x4c00;
    }
    a
}

/// The screen as main() leaves it before the command loop: screen window, palette, paper, the
/// status and message windows, the command bar; then the steps after load (0x1217d4, the
/// clock) and combat begun for the disk's combat byte.
fn boot_screen(s: &mut State, io: &Io) {
    let tables = s.ui.tables.clone();
    s.ui.text
        .open(&tables, crate::ui::WindowId::Screen, 0, 0, 0x27, 0x18);
    io.draw(|fb| {
        for (dst, c) in fb.palette.iter_mut().zip(&s.tables.palette) {
            *dst = u6_render::Rgb::new(c.r, c.g, c.b);
        }
    });
    crate::main_loop::draw_paper(s, io);
    s.ui.text.main_windows(&tables);
    let st = &*s;
    io.draw(|fb| {
        for i in 0..9u16 {
            crate::gfx::blit::blit_front(&st.gfx, fb, 0x190 + i, i * 16 + 8, 0xb0);
        }
        crate::gfx::blit::blit_front(&st.gfx, fb, 0x19e, 0x98, 0xb0);
    });
    crate::actors::turn::after_load(s);
    crate::actors::clock::tick(s, 0);
    if s.actors.tail(crate::actors::tail::COMBAT_MODE) != 0 {
        super::mode::begin_combat(s);
    }
}

/// Free tiles around the leader for the spawned monsters.
fn free_tiles(s: &mut State, n: usize) -> Vec<(u16, u16)> {
    let c = s.map.centre;
    let leader = s.actors.party.leader().quiet().0;
    let mut out = Vec::new();
    for (dx, dy) in [
        (1i16, -1i16),
        (-1, -1),
        (0, -2),
        (2, 0),
        (-2, 0),
        (1, 1),
        (-1, 1),
    ] {
        let (x, y) = ((c.x as i16 + dx) as u16, (c.y as i16 + dy) as u16);
        if crate::objects::actor_at(s, 0, x, y) < 0
            && crate::actors::walk::can_move_to(s, leader, x, y)
        {
            out.push((x, y));
        }
        if out.len() == n {
            break;
        }
    }
    out
}

fn write_ppm(dir: &Path, name: &str, fb: &u6_render::Framebuffer) {
    let mut out = format!("P6\n{} {}\n255\n", fb.width(), fb.height()).into_bytes();
    for &i in fb.pixels() {
        let c = fb.palette[usize::from(i)];
        out.extend([c.r, c.g, c.b]);
    }
    std::fs::write(dir.join(name), out).quiet();
}

#[test]
fn the_party_fights_gargoyles_in_the_throne_room() {
    let Some((mut state, files)) = start() else {
        return;
    };
    state.rng.seed(0x1234);
    let which = GARGOYLE_TYPES
        .into_iter()
        .position(|k| crate::objects::monster_index(&state.objects, k) >= 0)
        .expect("a gargoyle creature type");
    let kind = GARGOYLE_TYPES[which];
    let report: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let rep = Rc::clone(&report);
    let needles: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let words = Rc::clone(&needles);
    let mut e = Engine::with_game(state, files, move |st, io| async move {
        words
            .borrow_mut()
            .extend([after_argument(&io, KILLED), after_argument(&io, GRAZED)]);
        let foes = {
            let mut s = st.borrow_mut();
            boot_screen(&mut s, &io);
            let tiles = free_tiles(&mut s, 2);
            let foes: Vec<i16> = tiles
                .iter()
                .map(|&(x, y)| spawn(&mut s, kind, x, y))
                .collect();
            crate::map::view::update_view(&mut s);
            foes
        };
        assert!(foes.len() == 2, "two foes spawned");
        crate::ui::panels::redraw_panel(&st, &io).await;
        crate::gfx::refresh_view(&mut st.borrow_mut(), &io);
        crate::gfx::render_window(&mut st.borrow_mut(), &io, true);
        io.vblank().await;
        // A: the cross-hair on the first gargoyle, confirmed by the queued Return.
        {
            let mut s = st.borrow_mut();
            let target = s.objects.location[foes[0] as usize];
            let c = s.map.centre;
            let input = &mut s.ui.input;
            input.targeting = 1;
            input.cursor_by_tab = false;
            // The target's view cell: the centre at (HALF_W, HALF_H) ((5, 5) in Original).
            let (hw, hh) = (crate::geom::HALF_W as i16, crate::geom::HALF_H as i16);
            input.cursor = (
                (target.x as i16 - c.x as i16 + hw) as i8,
                (target.y as i16 - c.y as i16 + hh) as i8,
            );
            s.main.attack_target = foes[0];
            rep.borrow_mut().push(format!(
                "two foes spawned, gargoyle type {which} of {}",
                GARGOYLE_TYPES.len()
            ));
        }
        super::attack::attack(&st, &io).await;
        {
            let s = st.borrow();
            let said: Vec<u32> = s.combat.said.iter().map(|m| m.at).collect();
            let word = said.contains(&ATTACK_WORD);
            let weapon = WEAPON_LINES.iter().any(|w| said.contains(w));
            let mut r = rep.borrow_mut();
            r.push(format!("attack: command word {word}, weapon line {weapon}"));
        }
        for _ in 0..120 {
            {
                let mut s = st.borrow_mut();
                let l = s.actors.party.leader().quiet().0;
                s.actors.movement_points[usize::from(l)] = 0;
            }
            crate::actors::turn::world_turn(&st, &io).await;
            crate::main_loop::flush_prints(&st, &io).await;
            let s = st.borrow();
            let dead = foes
                .iter()
                .all(|&g| s.actors.attrs.status[g as usize] & 0x10 != 0);
            if dead {
                rep.borrow_mut().push("both gargoyles dead".to_string());
                break;
            }
        }
        rep.borrow_mut().push("done".into());
    });
    let frames_dir = std::env::var("U6_COMBAT_FRAMES").ok();
    let mut last = 0;
    let mut frames = Vec::new();
    let (hp0, exp0) = {
        let s = e.state();
        (s.actors.attrs.hp.clone(), s.actors.attrs.exp.clone())
    };
    for t in 0..20_000 {
        let input = if t == 2 {
            vec![InputEvent::Key(Key::Enter)]
        } else {
            Vec::new()
        };
        e.tick(&input, 20);
        let shown = e.state().combat.shown;
        if shown != last {
            last = shown;
            frames.push((t, e.frame().clone()));
        }
        if report.borrow().last().map(String::as_str) == Some("done") {
            break;
        }
    }
    let s = e.state();
    // Output: labels, booleans and an index into the test's own type list (spec 01 §Rights).
    println!("--- report ---");
    for line in report.borrow().iter() {
        println!("{line}");
    }
    let party: Vec<usize> = s.actors.party.active().map(|a| a.index()).collect();
    println!("stub hits: {:?}", e.stub_hits().keys().collect::<Vec<_>>());
    assert!(report.borrow().last().map(String::as_str) == Some("done"));
    let text = s.ui.messages.join("\n");
    let attack_line = report
        .borrow()
        .iter()
        .any(|l| l.starts_with("attack: command word true, weapon line true"));
    assert!(attack_line, "the Attack line");
    let (killed, grazed) = {
        let n = needles.borrow();
        (n[0].clone(), n[1].clone())
    };
    assert!(!killed.is_empty() && !grazed.is_empty(), "both lines read");
    let foes_dead = report
        .borrow()
        .iter()
        .any(|l| l.starts_with("both gargoyles dead"));
    assert!(foes_dead, "the fight ends");
    assert!(text.contains(&killed), "a death is reported");
    let gained = party.iter().any(|&m| s.actors.attrs.exp[m] > exp0[m]);
    assert!(gained, "experience for the kills");
    let hurt = party.iter().any(|&m| s.actors.attrs.hp[m] < hp0[m]) || text.contains(&grazed);
    assert!(hurt, "the gargoyles struck back");
    assert!(s.combat.shown > 0, "hit flashes / missiles were shown");
    let name = crate::objects::display_name(&s, i16::from(s.actors.party.members[0]));
    assert!(!name.is_empty());
    if let Some(dir) = frames_dir {
        let dir = Path::new(&dir);
        std::fs::create_dir_all(dir).quiet();
        for (t, fb) in &frames {
            write_ppm(dir, &format!("effect-{t:05}.ppm"), fb);
        }
        write_ppm(dir, "final.ppm", e.frame());
    }
}

/// Stub hits of 0x10c6a6 on this thread outside an engine.
fn remains_stub_hits() -> u64 {
    let hits = crate::stub::detached_hits();
    hits.iter()
        .filter(|(id, _)| id.starts_with("0x10c6a6"))
        .map(|(_, n)| n)
        .sum()
}

/// The remains (0x10c6a6, asm 0x10c6a6..0x10c80e) of a monster carrying an object and a readied
/// type-0x4c one (which 0x10cda8 moves only while A4-0x78da is set), on its tile under a type
/// 0x152 object: one remains object of the monster's type and frame word (quality the
/// monster, temporary) on the tile, holding both (location 0x08, status bit 0); the 0x152 back
/// on top of the tile; the monster on level 15; and no stub point reached.
#[test]
fn a_dead_monster_leaves_one_remains_object_holding_what_it_carried() {
    let Some((mut s, _files)) = start() else {
        return;
    };
    let kind = GARGOYLE_TYPES
        .into_iter()
        .find(|&k| crate::objects::monster_index(&s.objects, k) >= 0)
        .expect("a gargoyle creature type");
    let (x, y) = free_tiles(&mut s, 1)[0];
    let z = u16::from(s.map.centre.z);
    let foe = spawn(&mut s, kind, x, y);
    assert!(foe >= 0, "a monster slot");
    let carried = [
        crate::objects::create_in(&mut s, 0x58, crate::objects::IN_INVENTORY, 1, foe),
        crate::objects::create_in(&mut s, 0x4c, crate::objects::READIED, 1, foe),
    ];
    let pool = crate::objects::create_on_map(&mut s.objects, 0x152, 0, 0, x, y, z);
    assert!(carried.iter().all(|&c| c >= 0) && pool >= 0, "object slots");
    let tf = s.objects.type_frame[foe as usize];
    let hits = remains_stub_hits();
    let body = super::seams::remains(&mut s, foe);
    assert_eq!(remains_stub_hits() - hits, 0, "no stub point");
    assert!(body >= 0, "the remains");
    let o = &mut s.objects;
    let b = body as usize;
    assert_eq!((o.type_frame[b], o.qq[b] >> 8), (tf, foe as u16));
    assert_eq!(o.obj_status[b] & 0x39, 0x21, "on the map, temporary, bit 0");
    assert_eq!((o.x(body), o.y(body), u16::from(o.z(body))), (x, y, z));
    let mut inside = Vec::new();
    let mut c = crate::objects::first_child(o, body);
    while c >= 0 {
        inside.push(c);
        c = crate::objects::next_child(o);
    }
    let mut want = carried;
    want.sort_unstable();
    inside.sort_unstable();
    assert_eq!(inside, want, "everything it carried");
    for c in carried {
        assert_eq!(
            o.obj_status[c as usize] & 0x19,
            0x09,
            "in a container, bit 0"
        );
    }
    assert_eq!(o.z(foe), 0xf, "the monster on level 15");
    assert_eq!(o.allow_type_4c, 0);
    let first = crate::objects::begin_at(o, &s.tables, x, y, z);
    let second = crate::objects::next_at(o, &s.tables);
    assert_eq!(
        (first, second),
        (pool, body),
        "the 0x152 on top, the remains next"
    );
    println!("remains holds {} objects", inside.len());
}
