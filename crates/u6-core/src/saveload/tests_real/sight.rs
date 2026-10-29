//! Spec 18 (saved with the game, unit C3) on the user's files: the remembered sight written by a
//! Save, handed to a store as the platform's flush hands it, laid over the game files and read
//! back by a fresh engine's boot, and by a Restore; a save without it, or with one of another
//! save, as before. Each test prints `<name>: ran` or `<name>: skipped` and counts only; with
//! `U6_REQUIRE_ARCHIVE=1` a missing zip fails it instead of skipping it.

use super::*;
use crate::geom::{HALF_H, HALF_W, VIEW_H, VIEW_W};
use crate::gfx::view::cell_pixel;
use crate::map::view::{DARK, NOT_VISIBLE};
use u6_render::GREY;

/// The zip for real-data test `name`, which prints `name: ran` or `name: skipped`; with
/// `U6_REQUIRE_ARCHIVE=1` a missing zip fails it.
fn archive(name: &str) -> Option<Files> {
    let files = zip_files();
    if files.is_none() {
        assert_ne!(
            std::env::var("U6_REQUIRE_ARCHIVE").as_deref(),
            Ok("1"),
            "{name}: U6_REQUIRE_ARCHIVE=1 and the archive is missing"
        );
        println!("{name}: skipped");
        return None;
    }
    println!("{name}: ran");
    files
}

impl Run {
    /// [`Run::boot`] with the remembered sight switched `on` before the first tick.
    fn boot_seeing(files: Files, on: bool) -> Run {
        let mut state = State::new_game(&files).quiet();
        state.rng.seed(0);
        let cmds = Rc::new(RefCell::new(VecDeque::new()));
        let c = Rc::clone(&cmds);
        let e = Engine::with_game(state, files, move |st, io| Inject {
            game: Box::pin(crate::main_loop::game_first(Rc::clone(&st), Rc::clone(&io))),
            cmds: c,
            state: st,
            io,
        });
        e.io().set_sight_memory(on);
        let mut run = Run { e, cmds };
        run.settle();
        assert!(run.at_prompt(), "boot reaches the command prompt");
        run
    }

    /// `n` ticks with nothing pressed (the idle refresh renders the window).
    fn idle(&mut self, n: usize) {
        for _ in 0..n {
            self.e.tick(&[], 16);
        }
    }

    /// The remembered sight as the file a Save of it would write with `objlist`.
    fn sight(&self, objlist: &[u8]) -> Option<Vec<u8>> {
        self.e.state().gfx.cache.sight_file(objlist)
    }

    /// The cells the remembered sight holds.
    fn cells(&self) -> usize {
        self.e.state().gfx.cache.sight_cells()
    }
}

/// The back buffer's 16 × 16 pixels of every view cell out of sight now (its base tile
/// [`NOT_VISIBLE`]), by view cell, off the view's edge (where the border covers it).
fn out_of_sight(run: &Run) -> Vec<((usize, usize), Vec<u8>)> {
    let s = run.e.state();
    let io = run.e.io();
    inner()
        .filter(|&(col, row)| s.objects.draw.base[row * VIEW_W + col] == NOT_VISIBLE)
        .map(|at| {
            let (x, y) = cell_pixel(at.0, at.1);
            let (x, y) = (usize::from(x), usize::from(y));
            let px = io.peek_back(|b| {
                (0..256)
                    .map(|i| b.get(x + i % 16, y + i / 16).unwrap_or(0))
                    .collect()
            });
            (at, px)
        })
        .collect()
}

/// The grey pixels (the remembered sight's colours, [`GREY`] on) of boxes `b`.
fn grey(b: &[((usize, usize), Vec<u8>)]) -> usize {
    b.iter()
        .map(|(_, px)| px.iter().filter(|&&p| p >= GREY).count())
        .sum()
}

/// Spec 18: the calendar one whole day on (days 1..=28, months 1..=12).
fn next_day(s: &mut State) {
    let c = &mut s.actors.calendar;
    c.day += 1;
    if c.day > 28 {
        c.day = 1;
        c.month += 1;
        if c.month > 12 {
            c.month = 1;
            c.year += 1;
        }
    }
}

/// Whether view cell `at` of `s` is in sight and lit (its base neither out of sight nor dark).
fn in_sight(s: &State, (col, row): (usize, usize)) -> bool {
    !matches!(s.objects.draw.base[row * VIEW_W + col], NOT_VISIBLE | DARK)
}

/// The world cell view cell (col, row) shows round the view centre.
fn world(s: &State, (col, row): (usize, usize)) -> (u16, u16) {
    let (c, m) = (s.map.centre, s.map.world_mask);
    let x = c.x.wrapping_add(col as u16).wrapping_sub(HALF_W as u16);
    let y = c.y.wrapping_add(row as u16).wrapping_sub(HALF_H as u16);
    (x & m, y & m)
}

/// The view cells off the view's edge.
fn inner() -> impl Iterator<Item = (usize, usize)> {
    (1..VIEW_H - 1).flat_map(|row| (1..VIEW_W - 1).map(move |col| (col, row)))
}

/// Places and arrow keys (north, east, south, west) whose step takes a cell in sight out of sight
/// in the view, from `s`: the start, then the party teleported (0x1092fc) 8, 12, .. 60 cells off
/// the start cell in each of eight ways, on its level (as `gfx::view`'s tests search), the step
/// tried on a copy.
fn hiding_steps(s: &State) -> impl Iterator<Item = ((u16, u16, u16), Key)> + '_ {
    const WAYS: [(i16, i16); 8] = [
        (1, 0),
        (-1, 0),
        (0, 1),
        (0, -1),
        (1, 1),
        (-1, -1),
        (1, -1),
        (-1, 1),
    ];
    let c = s.map.centre;
    let offs = std::iter::once((0, 0)).chain(
        (8..64)
            .step_by(4)
            .flat_map(|r| WAYS.map(|(x, y)| (x * r, y * r))),
    );
    offs.flat_map(move |(dx, dy)| {
        let at = (
            c.x.wrapping_add_signed(dx),
            c.y.wrapping_add_signed(dy),
            u16::from(c.z),
        );
        let mut place = s.clone();
        crate::map::travel::teleport(&mut place, at.0, at.1, at.2, 0);
        crate::map::view::update_view(&mut place);
        let seen: Vec<(u16, u16)> = inner()
            .filter(|&v| in_sight(&place, v))
            .map(|v| world(&place, v))
            .collect();
        let keys = [
            (0, Key::Up),
            (2, Key::Right),
            (4, Key::Down),
            (6, Key::Left),
        ];
        keys.into_iter().filter_map(move |(d, key)| {
            let mut t = place.clone();
            let l = usize::from(t.actors.party.leader()?.0);
            t.actors.movement_points[l] = 30;
            if crate::actors::party::player_step(&mut t, d)
                != crate::actors::party::StepResult::Moved
            {
                return None;
            }
            crate::map::view::update_view(&mut t);
            let hides = inner().any(|v| {
                t.objects.draw.base[v.1 * VIEW_W + v.0] == NOT_VISIBLE
                    && seen.contains(&world(&t, v))
            });
            hides.then_some((at, key))
        })
    })
}

/// The game booted with the remembered sight on, the party taken to a place and stepped so that a
/// cell it saw is out of sight in the view and shows its remembered look ([`hiding_steps`], the
/// first that does so in the game), then saved: the run, and the store the platform's flush
/// filled.
fn seen_and_saved(files: &Files) -> (Run, MemStore) {
    let mut run = Run::boot_seeing(files.clone(), true);
    let tries: Vec<((u16, u16, u16), Key)> = hiding_steps(&run.state()).take(24).collect();
    let shown = tries.into_iter().any(|((x, y, z), key)| {
        run.command(Cmd::Teleport(x, y, z));
        run.press(key);
        run.at_prompt() && grey(&out_of_sight(&run)) > 0
    });
    assert!(shown, "a step shows a remembered cell out of sight");
    run.command(Cmd::Save);
    let mut store = MemStore::default();
    SaveSync::default().flush(&run.e, &mut store).quiet();
    (run, store)
}

/// Spec 18 (saved with the game: the round trip): a Save with something remembered writes
/// [`SIGHT_FILE`] beside the objlist, bound to it, and the platform's flush stores it. A fresh
/// engine booted from the stored files holds the same memory, byte for byte as the file says,
/// and draws every view cell out of sight as before the reload, the remembered ones grey. A day
/// later both draw the same fade, which shows less than the day before. A Restore in the session
/// reads the same memory back from the last Save.
#[test]
fn real_the_remembered_sight_comes_back_with_its_save() {
    let Some(files) = archive("real_the_remembered_sight_comes_back_with_its_save") else {
        return;
    };
    let (mut run, store) = seen_and_saved(&files);
    let saved = run.e.state().saveload.sight.clone().quiet();
    let objlist = store.files[OBJLIST_FILE].clone();
    assert!(
        store.files.get(SIGHT_FILE).map(Vec::as_slice) == Some(saved.bytes()),
        "the flush stores the file the Save wrote"
    );
    assert!(
        run.sight(&objlist).as_deref() == Some(saved.bytes()),
        "the memory as the Save left it"
    );
    run.idle(8);
    let before = out_of_sight(&run);
    let cal = run.e.state().actors.calendar;

    let mut f = files.clone();
    let laid = attach(&mut f, &store).quiet();
    assert!(
        laid == store.files.len(),
        "the sight file laid over with the save"
    );
    let mut fresh = Run::boot_seeing(f, false);
    assert!(
        fresh.sight(&objlist).as_deref() == Some(saved.bytes()),
        "the fresh engine holds the saved memory"
    );
    fresh.e.io().set_sight_memory(true);
    fresh.idle(8);
    assert!(fresh.e.state().actors.calendar == cal, "the saved clock");
    let after = out_of_sight(&fresh);
    assert!(
        after == before,
        "every cell out of sight as before the reload"
    );
    let day0 = grey(&after);
    assert!(day0 > 0, "remembered cells shown in grey");

    for r in [&mut run, &mut fresh] {
        r.command(Cmd::Edit(next_day));
        r.idle(8);
    }
    let (a, b) = (out_of_sight(&run), out_of_sight(&fresh));
    assert!(a == b, "a day later: the same fade");
    let day1 = grey(&b);
    assert!(day1 < day0 && day1 > 0, "a day later: less of it shown");

    run.e.io().set_sight_memory(false);
    run.command(Cmd::Restore);
    assert!(
        run.sight(&objlist).as_deref() == Some(saved.bytes()),
        "a Restore reads back the last Save's memory"
    );
    println!(
        "remembered sight round trip: {} cells, {} view cells out of sight, grey pixels {day0} on \
         day 0 and {day1} a day later",
        fresh.cells(),
        before.len()
    );
}

/// What a boot holds and shows: the cells its remembered sight holds once booted (the switch off,
/// so no render added any), then with the switch on after a few frames its state, the frame's
/// pixels and palette, and the cells held.
type Booted = (usize, State, Vec<u8>, Vec<u6_render::Rgb>, usize);

/// Boots `files` laid over with `store` ([`Booted`]).
fn boot_from(files: &Files, store: &MemStore) -> Booted {
    let mut f = files.clone();
    attach(&mut f, store).quiet();
    let mut run = Run::boot_seeing(f, false);
    let read = run.cells();
    run.e.io().set_sight_memory(true);
    run.idle(8);
    let fb = run.e.frame();
    let shown = (fb.pixels().to_vec(), fb.palette.to_vec());
    (read, run.state(), shown.0, shown.1, run.cells())
}

/// `store` with file `name` replaced by `bytes`, or removed (`None`).
fn with_file(store: &MemStore, name: &str, bytes: Option<Vec<u8>>) -> MemStore {
    let mut s = store.clone();
    s.files.remove(name);
    if let Some(b) = bytes {
        s.files.insert(name.to_string(), b);
    }
    s
}

/// Spec 18 (saved with the game: another save's file, a malformed file): the port's save with its
/// objlist changed after the file was written (a later save of the original over it: here one
/// talk flag) boots with nothing remembered; so do the new game with a stored file of another
/// game, and the port's save with its file truncated or of another version. Each boots to the
/// same state and the same frames as the same files without the sight file (an original's save;
/// that it boots as before C3 is round 1's scratch receipt against 54b9eea). A Save with the
/// switch off from a fresh start (nothing ever remembered) writes the original's files alone.
#[test]
fn real_a_save_without_its_own_sight_file_boots_with_nothing_remembered() {
    let Some(files) =
        archive("real_a_save_without_its_own_sight_file_boots_with_nothing_remembered")
    else {
        return;
    };
    let (_, store) = seen_and_saved(&files);
    let file = store.files[SIGHT_FILE].clone();
    let plain = with_file(&store, SIGHT_FILE, None);
    let mut later = store.files[OBJLIST_FILE].clone();
    later[0x17F1 + 0xff] ^= 1;
    let original_later = with_file(&plain, OBJLIST_FILE, Some(later));
    let mut short = file.clone();
    short.truncate(file.len() - 1);
    let mut version = file.clone();
    version[8] = 2;
    let only_file = MemStore {
        files: [(SIGHT_FILE.to_string(), file.clone())].into(),
    };
    let cases = [
        (
            "the objlist changed after the file",
            &original_later,
            with_file(&original_later, SIGHT_FILE, Some(file.clone())),
        ),
        (
            "a truncated file",
            &plain,
            with_file(&plain, SIGHT_FILE, Some(short)),
        ),
        (
            "another version",
            &plain,
            with_file(&plain, SIGHT_FILE, Some(version)),
        ),
        (
            "a new game with another game's file",
            &MemStore::default(),
            only_file,
        ),
    ];
    let count = cases.len();
    for (what, without, with) in cases {
        let (a, b) = (boot_from(&files, without), boot_from(&files, &with));
        assert!(
            a.0 == 0 && b.0 == 0,
            "{what}: nothing remembered at the boot"
        );
        assert!(a.1 == b.1, "{what}: the same state");
        assert!(a.2 == b.2 && a.3 == b.3, "{what}: the same frame");
        assert!(a.4 == b.4, "{what}: the same cells remembered since");
    }
    let mut off = Run::boot_seeing(files.clone(), false);
    off.walk(2);
    off.command(Cmd::Save);
    let mut written = MemStore::default();
    let n = SaveSync::default().flush(&off.e, &mut written).quiet();
    assert!(
        off.e.state().saveload.sight.is_none(),
        "the switch off: no sight file"
    );
    assert!(
        n == written.files.len() && !written.files.contains_key(SIGHT_FILE),
        "the original's files alone"
    );
    println!("no sight file of its own: {count} cases booted as without one");
}

/// The run taken to a place `cells` east of where it stands and rendered there (the view's cells
/// in sight remembered too).
fn see_more(run: &mut Run, cells: u16) {
    let c = run.e.state().map.centre;
    run.command(Cmd::Teleport(c.x + cells, c.y, u16::from(c.z)));
    run.idle(8);
}

/// Spec 18 (saved with the game: the switch): with the switch off, a memory the save already holds
/// is written again by a Save. A game booted with the switch off from a store holding its save's
/// file saves it again (the Save command: control-S, then Y): the flush lists the file, the bytes
/// loaded but its binding (offsets 12..24, the objlist's length and digest), which names the
/// objlist this Save wrote (the boot's ticks change two of its tables, the actors' type and frame
/// and their worktype), and it reads back with that objlist.
#[test]
fn real_with_the_switch_off_a_save_writes_the_memory_it_loaded_again() {
    let Some(files) = archive("real_with_the_switch_off_a_save_writes_the_memory_it_loaded_again")
    else {
        return;
    };
    let (_, store) = seen_and_saved(&files);
    let mut f = files.clone();
    attach(&mut f, &store).quiet();
    let mut off = Run::boot_seeing(f, false);
    assert!(off.cells() > 0, "the memory loaded");
    off.press(Key::Char('\u{13}'));
    off.press(Key::Char('y'));
    assert!(off.e.state().saveload.generation == 1, "the Save");
    let mut written = MemStore::default();
    SaveSync::default().flush(&off.e, &mut written).quiet();
    assert!(
        written.files.contains_key(SIGHT_FILE),
        "the switch off: the file listed again"
    );
    let (loaded, again) = (&store.files[SIGHT_FILE], &written.files[SIGHT_FILE]);
    let objlist = &written.files[OBJLIST_FILE];
    let tables = table_diffs(&store.files[OBJLIST_FILE], objlist).len();
    assert!(
        again.len() == loaded.len() && again[..12] == loaded[..12] && again[24..] == loaded[24..],
        "the file listed again: the bytes loaded but the binding"
    );
    assert!(
        crate::gfx::cache::read_sight(again, objlist).is_ok(),
        "bound to the objlist this Save wrote"
    );
    println!(
        "the switch off: a Save of a loaded memory of {} cells wrote its {} bytes again ({tables} \
         objlist tables moved)",
        off.cells(),
        again.len()
    );
}

/// Spec 18 (saved with the game: the orders): Save A, then Save B with more seen: a Restore reads
/// B's memory, and so does a boot from the store both were flushed into. Then the character `u`
/// (a new game's objlist, a save generation of its own, no sight file): a Restore reads no
/// memory, and a boot from the store (`u`'s objlist, A's and B's file still beside it) none.
#[test]
fn real_the_last_saves_memory_comes_back_and_none_after_u() {
    let Some(files) = archive("real_the_last_saves_memory_comes_back_and_none_after_u") else {
        return;
    };
    let (mut run, mut store) = seen_and_saved(&files);
    let mut sync = SaveSync {
        flushed: run.e.state().saveload.generation,
    };
    let a = run.e.state().saveload.sight.clone().quiet();
    let held = run.cells();
    see_more(&mut run, 24);
    assert!(run.cells() > held, "more seen after Save A");
    run.command(Cmd::Save);
    sync.flush(&run.e, &mut store).quiet();
    let b = run.e.state().saveload.sight.clone().quiet();
    assert!(b.bytes() != a.bytes(), "Save B wrote another memory");
    let objlist_b = store.files[OBJLIST_FILE].clone();
    assert!(store.files.get(SIGHT_FILE).map(Vec::as_slice) == Some(b.bytes()));
    see_more(&mut run, 24);
    run.e.io().set_sight_memory(false);
    run.command(Cmd::Restore);
    assert!(
        run.sight(&objlist_b).as_deref() == Some(b.bytes()),
        "a Restore reads Save B's memory"
    );
    let mut f = files.clone();
    attach(&mut f, &store).quiet();
    let fresh = Run::boot_seeing(f, false);
    assert!(
        fresh.sight(&objlist_b).as_deref() == Some(b.bytes()),
        "a boot from the store holds Save B's memory"
    );

    let new_game = boot_objlist(&files).quiet().to_vec();
    assert!(new_game != objlist_b);
    run.command(Cmd::Handoff(new_game.clone()));
    sync.flush(&run.e, &mut store).quiet();
    assert!(store.files[OBJLIST_FILE] == new_game, "u's objlist stored");
    assert!(
        store.files.get(SIGHT_FILE).map(Vec::as_slice) == Some(b.bytes()),
        "B's file left beside it"
    );
    run.command(Cmd::Restore);
    assert!(run.cells() == 0, "after u: a Restore reads no memory");
    let (read, ..) = boot_from(&files, &store);
    assert!(read == 0, "after u: a boot from the store reads none");
    println!(
        "the orders: Save A {} bytes, Save B {} bytes read back by a Restore and a boot; after u \
         none",
        a.bytes().len(),
        b.bytes().len()
    );
}

/// Spec 18 (saved with the game: a Restore without a file of its own): before any Save of the
/// session, a Restore reads the platform's file; with none, or one it refuses (truncated), the
/// memory the session gathered is forgotten whole, as before C3, and the two Restores leave the
/// same state and frames.
#[test]
fn real_a_restore_without_its_own_sight_file_forgets_the_memory() {
    let Some(files) = archive("real_a_restore_without_its_own_sight_file_forgets_the_memory")
    else {
        return;
    };
    let (_, store) = seen_and_saved(&files);
    let plain = with_file(&store, SIGHT_FILE, None);
    let mut short = store.files[SIGHT_FILE].clone();
    short.pop();
    let refused = with_file(&store, SIGHT_FILE, Some(short));
    let mut after = Vec::new();
    for (what, st) in [("no file", &plain), ("a truncated file", &refused)] {
        let mut f = files.clone();
        attach(&mut f, st).quiet();
        let mut run = Run::boot_seeing(f, true);
        see_more(&mut run, 24);
        assert!(run.cells() > 0, "{what}: cells seen in the session");
        run.e.io().set_sight_memory(false);
        run.command(Cmd::Restore);
        assert!(run.cells() == 0, "{what}: the Restore forgets them");
        run.idle(8);
        let fb = run.e.frame();
        after.push((run.state(), fb.pixels().to_vec(), fb.palette.to_vec()));
    }
    assert!(
        after[0] == after[1],
        "the same state and frame after either Restore"
    );
    println!("a Restore without its own file: the memory forgotten, 2 cases alike");
}
