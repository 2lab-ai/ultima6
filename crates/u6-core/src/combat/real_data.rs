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

/// The magic hunk's refusal line (0x1159f8): the port's former fallback for a spellbook's or
/// spell object's cast, which the original never prints there.
const CAST_REFUSED: u32 = 0x0011_59f8;
/// 0x124232's report of a paralysis after a spellbook's or spell object's cast (spell 0x45, result
/// 0: PC-relative 0x121bb0, asm 0x12473c..0x124768).
const PARALYSIS_REPORT: u32 = 0x0012_1bb0;
/// The division line of creature index 0x30 (A4-0x7760), which 0x1231be prints with the text
/// forced on.
const DIVIDES: u32 = A4 - 0x7760;
/// Attack's next-weapon prompt (A4-0x76ac, 0x124a98's tail).
const NEXT_WEAPON: u32 = A4 - 0x76ac;
/// The spells cast here: 0x45 (paralysis: status bit 0x02, no resistance roll, 0x116f2c's
/// case), 0x44 (1..30 damage through 0x1231be), 0x07 (0x1095bc: every member's movement points
/// refilled) and 0x64 (0x11916e: the party through a moongate, 0x10921a).
const PARALYZE: u16 = 0x45;
const HURT_SPELL: u16 = 0x44;
const RAISE_PARTY: u16 = 0x07;
const GATE_TRAVEL: u16 = 0x64;

/// A free tile next to `actor` a monster may stand on (the eight directions in the order of
/// the direction tables, north first).
fn free_next_to(s: &mut State, actor: i16) -> (u16, u16) {
    let p = s.objects.location[actor as usize];
    let leader = s.actors.party.leader().quiet().0;
    let d = s.objects.defs.clone();
    (0..8)
        .map(|k| {
            let x = (p.x as i16 + i16::from(d.dir_dx[k])) as u16;
            (x, (p.y as i16 + i16::from(d.dir_dy[k])) as u16)
        })
        .find(|&(x, y)| {
            crate::objects::actor_at(s, 0, x, y) < 0
                && crate::actors::walk::can_move_to(s, leader, x, y)
        })
        .quiet()
}

/// A monster of `kind` next to `actor`, evil (alignment bits 0x20), with `hp` hit points.
fn foe_next_to(s: &mut State, kind: u16, actor: i16, hp: u8) -> i16 {
    let (x, y) = free_next_to(s, actor);
    let foe = spawn(s, kind, x, y);
    assert!(foe >= 0, "a monster slot");
    let a = &mut s.actors.attrs;
    a.status[foe as usize] = (a.status[foe as usize] & !0x60) | 0x20;
    a.hp[foe as usize] = hp;
    foe
}

/// A readied object of `kind` whose quality is `spell`, held by `holder`.
fn readied(s: &mut State, kind: u16, spell: u16, holder: i16) -> i16 {
    let o = crate::objects::create_in(s, kind, crate::objects::READIED, spell << 8 | 1, holder);
    assert!(o >= 0, "an object slot");
    o
}

/// Party member 1 fighting in the assault mode (combat mode and worktype 8) at level 8 with
/// `magic` magic points and a readied spellbook (`magic::SPELLBOOK`, 0x39) whose quality is
/// `spell`: the member and the book.
fn member_with_book(s: &mut State, spell: u16, magic: u8) -> (i16, i16) {
    let m = i16::from(s.actors.party.members[1]);
    let u = m as usize;
    s.actors.attrs.magic[u] = magic;
    s.actors.attrs.level[u] = 8;
    s.actors.attrs.combat_mode[u] = 8;
    s.actors.worktype[u] = 8;
    (m, readied(s, crate::magic::SPELLBOOK, spell, m))
}

/// A cast receipt's set-up: the actor whose NPC turn runs, its item, the target its hit is to
/// take, whether the hit records a cast, and the RNG state, pinned, under which its worktype
/// takes them.
struct Fixture {
    actor: i16,
    item: i16,
    target: i16,
    casts: bool,
    rng: u32,
}

/// Checks on a copy of the state the turn started from (outside the engine: nothing drawn) that
/// the pinned turn's synchronous part hits the fixture's target with its item: the cast it
/// records, or (no cast: a book short of its circle) the hit's selection and the first draws of
/// the assault (0x12500a, 0x124e74) taking the item.
fn check_pin(before: &State, f: &Fixture) {
    let mut probe = before.clone();
    let mut first = before.clone();
    crate::actors::npc::npc_turn_sync(&mut probe, f.actor as u16);
    let cast = probe
        .combat
        .item_cast
        .as_ref()
        .map(|c| (c.item, c.selection));
    let ok = if f.casts {
        cast == Some((f.item, f.target))
    } else {
        let t = super::npc::find_target(&mut first, f.actor);
        cast.is_none()
            && probe.combat.hit == f.target
            && t == f.target
            && super::leaves::pick_weapon(&mut first, f.actor, t) == f.item
    };
    assert!(ok, "the pinned turn takes the item at the target");
}

/// The message scroll as a string: the finished lines, then the line being written.
fn scroll(s: &State) -> String {
    let t = &s.ui.text;
    let mut lines = s.ui.messages.clone();
    let pending = &t.line[usize::from(t.line_start)..usize::from(t.line_len)];
    let cells = t.transcript_line.iter().chain(pending);
    lines.push(cells.map(|&c| crate::ui::wide::cell_char(c)).collect());
    lines.join("\n")
}

/// `text` without its whitespace (the scroll wraps and breaks lines where the format does not).
fn squeeze(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

/// What a cast receipt reads: the state before and after the turn, the fixture, the addresses
/// said during the turn, the formats asked for (read from the user's `game`; compared, never
/// printed) and the scroll before and after.
struct Cast {
    before: State,
    after: State,
    f: Fixture,
    said: Vec<u32>,
    formats: Vec<String>,
    scroll_before: String,
    scroll_after: String,
}

/// The NPC turn (0x1200b0) of the fixture's actor, in an engine over the start state after the
/// boot screen, with the text switch `text` before it and `keys` typed one by one while it
/// runs; then the lines it left printed. `formats` reads the strings at `at`.
fn cast_turn(
    at: &'static [u32],
    keys: &'static [Key],
    text: bool,
    setup: impl FnOnce(&mut State) -> Fixture + 'static,
) -> Option<Cast> {
    let (state, files) = start()?;
    type Before = (State, Fixture, Vec<String>, String);
    let out: Rc<RefCell<Option<Before>>> = Rc::new(RefCell::new(None));
    let o = Rc::clone(&out);
    let mut e = Engine::with_game(state, files, move |st, io| async move {
        let formats = at
            .iter()
            .map(|&a| String::from_utf8_lossy(&crate::main_loop::game_string(&io, a)).into_owned())
            .collect();
        let actor = {
            let mut s = st.borrow_mut();
            boot_screen(&mut s, &io);
            assert!(s.actors.combat_mode(), "the start fight is on");
            let f = setup(&mut s);
            crate::map::view::update_view(&mut s);
            s.rng.set_state(f.rng);
            s.combat.said.clear();
            s.ui.messages.clear();
            s.ui.text.on = text;
            let actor = f.actor;
            *o.borrow_mut() = Some((s.clone(), f, formats, scroll(&s)));
            actor
        };
        crate::actors::npc::npc_turn(&st, &io, actor as u16).await;
        crate::main_loop::flush_prints(&st, &io).await;
    });
    for t in 0..40_000 {
        let k = (t >= 100 && t % 50 == 0)
            .then(|| keys.get((t - 100) / 50))
            .flatten();
        let input: Vec<InputEvent> = k.map(|&k| InputEvent::Key(k)).into_iter().collect();
        e.tick(&input, 20);
        if e.finished() {
            break;
        }
    }
    assert!(e.finished(), "the turn ends");
    let (before, f, formats, scroll_before) = out.borrow_mut().take().quiet();
    check_pin(&before, &f);
    let after = e.state().clone();
    let said = after.combat.said.iter().map(|m| m.at).collect();
    let scroll_after = scroll(&after);
    Some(Cast {
        before,
        after,
        f,
        said,
        formats,
        scroll_before,
        scroll_after,
    })
}

impl Cast {
    /// Prints the receipt's counts and checks what every cast keeps: no refusal line, the text
    /// switch on after (set at 0x124702), the leader index put back (0x12476e).
    fn common(&self, label: &str) {
        let (b, a) = (&self.before, &self.after);
        let (t, m) = (self.f.target as usize, self.f.actor as usize);
        println!(
            "{label}: target status {:#04x} -> {:#04x}, magic {} -> {}, said {}, refusal {}",
            b.actors.attrs.status[t],
            a.actors.attrs.status[t],
            b.actors.attrs.magic[m],
            a.actors.attrs.magic[m],
            self.said.len(),
            self.said.contains(&CAST_REFUSED),
        );
        assert!(!self.said.contains(&CAST_REFUSED), "no refusal line");
        assert!(a.ui.text.on, "the text switch on after");
        assert_eq!(a.actors.party.leader_index, b.actors.party.leader_index);
    }

    /// The item still readied by the actor, its quality the same.
    fn item_kept(&self) -> bool {
        let (b, a, i) = (&self.before, &self.after, self.f.item as usize);
        a.objects.qq[i] == b.objects.qq[i]
            && a.objects.obj_status[i] & 0x18 == 0x18
            && a.objects.type_frame[i] == b.objects.type_frame[i]
    }

    /// The format `n` of the receipt with its `%s` (if any) filled with the target's name.
    fn line(&self, n: usize) -> String {
        let name = crate::objects::shown_name(&self.after, self.f.target).into_bytes();
        self.formats[n].replacen("%s", &String::from_utf8_lossy(&name), 1)
    }

    /// The scroll gained exactly `lines` in order during the turn (whitespace aside): the cast
    /// printed nothing of its own with the text off.
    fn printed_only(&self, lines: &[String]) -> bool {
        squeeze(&self.scroll_after) == squeeze(&self.scroll_before) + &squeeze(&lines.concat())
    }
}

/// The monster fixture: a gargoyle next to the leader with a readied spell object (0x150) of
/// Paralyze; its assault takes it against the leader.
fn monster_with_spell(s: &mut State) -> Fixture {
    let leader = s.actors.party.leader().quiet().0 as i16;
    let foe = foe_next_to(s, GARGOYLE_TYPES[0], leader, 40);
    let item = readied(s, 0x150, PARALYZE, foe);
    Fixture {
        actor: foe,
        item,
        target: leader,
        casts: true,
        rng: PIN_MONSTER,
    }
}

/// The RNG states the fixtures pin (each found once by a search over `Rng::seed` words).
const PIN_MONSTER: u32 = 0x00de_5589;
const PIN_MEMBER: u32 = 0x00de_5589;
const PIN_SHORT: u32 = 0x00de_5589;
const PIN_DIVIDES: u32 = 0x00de_5561;
const PIN_RAISE: u32 = 0x00de_5559;
const PIN_GATE: u32 = 0x00de_5559;

/// Spec 16 item 1a (O1 row 5): a monster's readied spell object (0x150, quality = the spell, as
/// 0x13121a gives one) that its assault takes (0x124e74) is cast in its hit 0x124232 (asm
/// 0x12467a..0x12476e): 0x11a586(quality, 1) by the monster as party slot 16 at its target
/// with the text off, then the paralysis report, the only line the turn prints. A 0x150 pays no
/// charge (only 0x39 pays, asm 0x12461c..0x124676): the object stays, the magic too.
#[test]
fn a_monster_casts_the_spell_object_it_readied_at_its_target() {
    let Some(c) = cast_turn(&[PARALYSIS_REPORT], &[], true, monster_with_spell) else {
        return;
    };
    c.common("monster");
    let t = c.f.target as usize;
    assert_eq!(
        c.before.actors.attrs.status[t] & 0x02,
        0,
        "not paralysed before"
    );
    assert_ne!(c.after.actors.attrs.status[t] & 0x02, 0, "paralysed after");
    assert!(c.said.contains(&PARALYSIS_REPORT), "the hit reports it");
    assert!(c.printed_only(&[c.line(0)]), "only the report is printed");
    assert!(c.item_kept(), "the spell object stays");
    let a = c.f.actor as usize;
    assert_eq!(
        c.after.actors.attrs.magic[a],
        c.before.actors.attrs.magic[a]
    );
}

/// The same with the text switch off before the turn: the cast runs as before, and 0x124702 sets
/// the switch to 1 (not back to what it was), so the report is printed.
#[test]
fn a_cast_begun_with_the_text_off_ends_with_it_on_and_reports() {
    let Some(c) = cast_turn(&[PARALYSIS_REPORT], &[], false, monster_with_spell) else {
        return;
    };
    c.common("text off");
    assert!(!c.before.ui.text.on, "off before");
    assert_ne!(c.after.actors.attrs.status[c.f.target as usize] & 0x02, 0);
    assert!(c.printed_only(&[c.line(0)]), "only the report is printed");
}

/// The same from a party member's readied spellbook (0x39, quality = its spell): the circle
/// (quality / 16 + 1) is checked against the member's magic and level and taken from the magic
/// once (asm 0x12461c..0x124676); the target, the spawned monster, is paralysed.
#[test]
fn a_member_casts_its_readied_spellbook_and_pays_the_circle_once() {
    let Some(c) = cast_turn(&[PARALYSIS_REPORT], &[], true, |s| {
        let (m, book) = member_with_book(s, PARALYZE, 9);
        let foe = foe_next_to(s, GARGOYLE_TYPES[1], m, 40);
        Fixture {
            actor: m,
            item: book,
            target: foe,
            casts: true,
            rng: PIN_MEMBER,
        }
    }) else {
        return;
    };
    c.common("member");
    let t = c.f.target as usize;
    assert_eq!(
        c.before.actors.attrs.status[t] & 0x02,
        0,
        "not paralysed before"
    );
    assert_ne!(c.after.actors.attrs.status[t] & 0x02, 0, "paralysed after");
    assert!(c.printed_only(&[c.line(0)]), "only the report is printed");
    assert_eq!(
        c.after.actors.attrs.magic[c.f.actor as usize],
        9 - 5,
        "the circle, once"
    );
    assert!(c.item_kept(), "the book stays");
}

/// With less magic than the circle the spellbook branch returns before the charge and the cast (asm
/// 0x12464a..0x124660): nothing cast, nothing taken, nothing printed.
#[test]
fn a_member_short_of_the_circle_casts_nothing_and_pays_nothing() {
    let Some(c) = cast_turn(&[], &[], true, |s| {
        let (m, book) = member_with_book(s, PARALYZE, 4);
        let foe = foe_next_to(s, GARGOYLE_TYPES[1], m, 40);
        Fixture {
            actor: m,
            item: book,
            target: foe,
            casts: false,
            rng: PIN_SHORT,
        }
    }) else {
        return;
    };
    c.common("short");
    let t = c.f.target as usize;
    assert_eq!(c.after.actors.attrs.status[t] & 0x02, 0, "not paralysed");
    assert!(c.printed_only(&[]), "nothing printed");
    assert_eq!(
        c.after.actors.attrs.magic[c.f.actor as usize], 4,
        "nothing taken"
    );
}

/// N3: 0x1231be forces the text on while it reports, also inside the text-off cast. A damage
/// spell (0x44) cast from a book on a creature of index 0x30 with 200 hit points: it divides,
/// and the division line (A4-0x7760) is the one line the turn prints.
#[test]
fn a_hit_reported_inside_the_cast_prints_with_the_text_forced_on() {
    let Some(c) = cast_turn(&[DIVIDES], &[], true, |s| {
        let (m, book) = member_with_book(s, HURT_SPELL, 9);
        let kind = (0..0x400u16)
            .find(|&k| crate::objects::monster_index(&s.objects, k) == 0x30)
            .quiet();
        let foe = foe_next_to(s, kind, m, 200);
        Fixture {
            actor: m,
            item: book,
            target: foe,
            casts: true,
            rng: PIN_DIVIDES,
        }
    }) else {
        return;
    };
    c.common("divides");
    let t = c.f.target as usize;
    assert_eq!(c.before.actors.attrs.hp[t], 200, "200 before");
    assert!(c.after.actors.attrs.hp[t] < 200, "the spell hurt it");
    assert!(c.said.contains(&DIVIDES), "the division is said");
    assert!(c.printed_only(&[c.line(0)]), "and is the one line shown");
}

/// 0x124232 casts inside the hit, so the worktype's movement points (0x127162 after the hit,
/// asm 0x12539a) are paid after the cast: a member casting 0x07 from its book, whose movement
/// points the spell refills to its dexterity (0x1095bc), ends at its dexterity less 10.
#[test]
fn the_worktypes_points_are_paid_after_the_cast() {
    let Some(c) = cast_turn(&[], &[], true, |s| {
        let (m, book) = member_with_book(s, RAISE_PARTY, 9);
        let foe = foe_next_to(s, GARGOYLE_TYPES[1], m, 40);
        let u = m as usize;
        s.actors.attrs.dexterity[u] = 25;
        s.actors.movement_points[u] = 22;
        Fixture {
            actor: m,
            item: book,
            target: foe,
            casts: true,
            rng: PIN_RAISE,
        }
    }) else {
        return;
    };
    c.common("raise");
    let m = c.f.actor as usize;
    println!(
        "raise: points {} -> {}",
        c.before.actors.movement_points[m], c.after.actors.movement_points[m]
    );
    assert_eq!(c.after.actors.attrs.magic[m], 9 - 1, "the circle, once");
    assert_eq!(
        c.after.actors.movement_points[m],
        25 - 10,
        "dexterity less 10"
    );
}

/// A member's book of Gate Travel (0x64), the gate's number typed: 0x10952e takes the party to
/// the first moonstone. 0x10921a moves the members there, then 0x104094 makes member 0 the
/// leader (0x103fd4(0) writes A4-0x7b7a at 0x103fe8, called at 0x104126; 0x10921a calls
/// 0x104094 at 0x109268), so its five followers' passes (0x10928a: A4+0x47ec[A4-0x7b7a]) follow
/// member 0, not party slot 16. After the cast the hit puts the leader index back (0x12476e).
/// Placement: 0x10c9d2 moves each member onto the stone (0x109238..0x109260); 0x11c910 never
/// moves the leader and moves each follower at most once in each of its two passes, so after
/// five calls member 0 stands on the stone and every other member on its level within 10 tiles.
/// The members' movement points refilled at 0x1092b4..0x1092f2, the caster then pays its
/// worktype's 10 (asm 0x12539a).
#[test]
fn gate_travel_from_a_book_takes_the_party_round_member_0() {
    const KEYS: &[Key] = &[Key::Char('1'), Key::Enter];
    let Some(c) = cast_turn(&[], KEYS, true, |s| {
        let (m, book) = member_with_book(s, GATE_TRAVEL, 9);
        let foe = foe_next_to(s, GARGOYLE_TYPES[1], m, 40);
        assert_ne!(
            crate::map::seams::moonstone(s, 0),
            [0, 0, 0],
            "the stone is placed"
        );
        Fixture {
            actor: m,
            item: book,
            target: foe,
            casts: true,
            rng: PIN_GATE,
        }
    }) else {
        return;
    };
    c.common("gate");
    let (a, m) = (&c.after, c.f.actor as usize);
    let [x, y, z] = crate::map::seams::moonstone(&c.before, 0);
    let centre = a.map.centre;
    let first = a.actors.party.members[0];
    println!(
        "gate: centre on the stone {}, followers follow member 0 {}",
        (centre.x, centre.y, u16::from(centre.z)) == (x, y, z),
        a.actors.work.follow[0] == first,
    );
    assert_eq!(
        (centre.x, centre.y, u16::from(centre.z)),
        (x, y, z),
        "the stone"
    );
    assert_eq!(
        a.actors.work.follow[0], first,
        "member 0 leads the followers"
    );
    let at = |k: u8| a.objects.location[usize::from(k)];
    let p0 = at(first);
    assert_eq!(
        (p0.x, p0.y, u16::from(p0.z)),
        (x, y, z),
        "member 0 on the stone"
    );
    let near = |v: u16, w: u16| (v as i16).wrapping_sub(w as i16).wrapping_abs() <= 10;
    let placed = a.actors.party.active().all(|id| {
        let p = at(id.0 as u8);
        u16::from(p.z) == z && near(p.x, x) && near(p.y, y)
    });
    assert!(placed, "every member on the stone's level within 10 tiles");
    assert_eq!(a.actors.attrs.magic[m], 9 - 7, "the circle, once");
    let dex = a.actors.attrs.dexterity[m];
    assert_eq!(
        a.actors.movement_points[m],
        dex - 10,
        "refilled, then the worktype's 10"
    );
}

/// A swimmer (creature flag 0x200 only: it cannot step on land, 0x11b1c0) next to the leader in
/// worktype `wt`, at half its hit points, with a readied spell object of the damage spell 0x44:
/// fleeing, it is cornered (0x11f452 fails) and fights (0x125254).
fn cornered_swimmer(s: &mut State, wt: u8) -> Fixture {
    let leader = s.actors.party.leader().quiet().0 as i16;
    let kind = (0..0x400u16)
        .find(|&k| {
            let i = crate::objects::monster_index(&s.objects, k);
            i > 1
                && s.actors.game.creature_flags(i) & 0x700 == 0x200
                && ![0x19b, 0x19d, 0x176, 0x177].contains(&k)
        })
        .quiet();
    let foe = foe_next_to(s, kind, leader, 1);
    let u = foe as usize;
    let max = crate::actors::leaves::max_hp(s, foe as u16) as u8;
    s.actors.attrs.hp[u] = max / 2;
    s.actors.worktype[u] = wt;
    let item = readied(s, 0x150, HURT_SPELL, foe);
    Fixture {
        actor: foe,
        item,
        target: leader,
        casts: true,
        rng: PIN_RETREAT,
    }
}

/// The RNG state [`cornered_swimmer`] pins (found as the others).
const PIN_RETREAT: u32 = 0x00de_0987;

/// 0x126738 (worktype 7) calls the assault 0x125254 at 0x1268ce, whose hit casts, and only then
/// (0x12691a..0x126986) tests the worktype, draws `random(0, 3)` and heals 1 hit point, putting
/// back the combat mode. The same cornered swimmer in worktype 0x13 (no such tail) takes the
/// same draws up to and through its cast: the spell's damage is the same in both turns, and
/// worktype 7's RNG ends one `random(0, 3)` past 0x13's, that roll deciding its hit points and
/// worktype.
#[test]
fn a_fleeing_monsters_roll_comes_after_its_cast() {
    let Some(flee) = cast_turn(&[], &[], true, |s| cornered_swimmer(s, 7)) else {
        return;
    };
    let Some(other) = cast_turn(&[], &[], true, |s| cornered_swimmer(s, 0x13)) else {
        return;
    };
    flee.common("flee 7");
    other.common("flee 0x13");
    let (t, m) = (flee.f.target as usize, flee.f.actor as usize);
    let hp = |s: &State| i16::from(s.actors.attrs.hp[t]);
    let hurt = |c: &Cast| hp(&c.before) - hp(&c.after);
    let mut r = crate::rng::Rng::new();
    r.set_state(other.after.rng.state());
    let roll = r.range(0, 3);
    let (b, a) = (&flee.before.actors, &flee.after.actors);
    let healed = roll == 0;
    println!(
        "flee: same damage {}, one roll later {}, healed {healed}",
        hurt(&flee) == hurt(&other),
        r.state() == flee.after.rng.state(),
    );
    assert!(
        hurt(&flee) > 0 && hurt(&flee) == hurt(&other),
        "the spell's draws first"
    );
    assert_eq!(r.state(), flee.after.rng.state(), "then the roll");
    assert_eq!(
        a.attrs.hp[m],
        b.attrs.hp[m] + u8::from(healed),
        "the hit point"
    );
    let wt = if healed { b.attrs.combat_mode[m] } else { 7 };
    assert_eq!(a.worktype[m], wt, "the worktype");
    assert_eq!(other.after.actors.worktype[m], 0x13);
}

/// The Attack command's cast (0x124caa → 0x124a98 → 0x124232). The shipped weapon table
/// (A4-0x6800) has no entry for 0x39 or 0x150, so 0x1249de never lists one and the command
/// never reaches the cast in play; with an entry added here (damage 1) the leader's readied book
/// of spell 0x45 is listed first and a second weapon after it, the cross-hair on a monster next
/// to the leader is confirmed for both. The cast and its report come before 0x124a98's tail
/// (asm 0x124c20..0x124ca4) prompts the next weapon; the monster is paralysed, the circle paid
/// once.
#[test]
fn the_attack_command_casts_then_prompts_the_next_weapon() {
    let Some((mut state, files)) = start() else {
        return;
    };
    assert!(
        !state
            .combat
            .tables
            .weapons
            .iter()
            .any(|&(k, _)| k == 0x39 || k == 0x150),
        "the shipped table lists neither"
    );
    state.combat.tables.weapons.push((0x39, 1));
    let blade = state.combat.tables.weapons[0].0;
    type After = (i16, i16, u8, State);
    let out: Rc<RefCell<Option<After>>> = Rc::new(RefCell::new(None));
    let o = Rc::clone(&out);
    let mut e = Engine::with_game(state, files, move |st, io| async move {
        let (foe, book, magic) = {
            let mut s = st.borrow_mut();
            boot_screen(&mut s, &io);
            let leader = s.actors.party.leader().quiet().0 as i16;
            let u = leader as usize;
            s.actors.attrs.magic[u] = 30;
            s.actors.attrs.level[u] = 8;
            let mut c = crate::objects::first_child(&mut s.objects, leader);
            while c >= 0 {
                s.objects.obj_status[c as usize] &= !0x08;
                c = crate::objects::next_child(&mut s.objects);
            }
            readied(&mut s, blade, 0, leader);
            let book = readied(&mut s, 0x39, PARALYZE, leader);
            let foe = foe_next_to(&mut s, GARGOYLE_TYPES[0], leader, 40);
            crate::map::view::update_view(&mut s);
            let p = s.objects.location[foe as usize];
            let c = s.map.centre;
            let (hw, hh) = (crate::geom::HALF_W as i16, crate::geom::HALF_H as i16);
            let input = &mut s.ui.input;
            input.targeting = 1;
            input.cursor_by_tab = false;
            input.cursor = (
                (p.x as i16 - c.x as i16 + hw) as i8,
                (p.y as i16 - c.y as i16 + hh) as i8,
            );
            s.main.attack_target = foe;
            s.combat.said.clear();
            (foe, book, s.actors.attrs.magic[u])
        };
        super::attack::attack(&st, &io).await;
        let s = st.borrow();
        *o.borrow_mut() = Some((foe, book, magic, s.clone()));
    });
    for t in 0..20_000 {
        let input = if t % 50 == 2 {
            vec![InputEvent::Key(Key::Enter)]
        } else {
            Vec::new()
        };
        e.tick(&input, 20);
        if e.finished() {
            break;
        }
    }
    assert!(e.finished(), "the command ends");
    let (foe, book, magic, s) = out.borrow_mut().take().quiet();
    let u = usize::from(s.actors.party.leader().quiet().0);
    let said: Vec<u32> = s.combat.said.iter().map(|m| m.at).collect();
    let at = |a: u32| said.iter().position(|&x| x == a);
    let (report, next) = (at(PARALYSIS_REPORT), at(NEXT_WEAPON));
    println!(
        "attack: weapons {}, book first {}, report {report:?} next {next:?}, magic {magic} -> {}",
        s.combat.weapon_count,
        s.combat.weapons[0] == book,
        s.actors.attrs.magic[u],
    );
    assert!(
        s.combat.weapon_count == 2 && s.combat.weapons[0] == book,
        "the book, then the blade"
    );
    assert_ne!(s.actors.attrs.status[foe as usize] & 0x02, 0, "paralysed");
    assert_eq!(s.actors.attrs.magic[u], magic - 5, "the circle, once");
    assert!(!said.contains(&CAST_REFUSED));
    assert!(
        matches!((report, next), (Some(r), Some(n)) if r < n),
        "the report before the next weapon's prompt"
    );
    assert!(s.ui.text.on);
}
