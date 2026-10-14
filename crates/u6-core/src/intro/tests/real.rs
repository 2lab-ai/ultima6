//! Real data: the user's archive.org zip → u6-intake → `Files`, then `u` run through the engine
//! with scripted keys. Skips when the zip is absent; reads only. The receipt run (ignored by
//! default) writes PNGs of the key screens and their raw palette indices to the system temp
//! directory, outside the repository.
#![allow(clippy::disallowed_methods)] // the tests read the zip and write receipt images

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::engine::{Engine, InputEvent, Key};
use crate::files::Files;
use crate::intro::{IntroOutcome, handoff, run};
use crate::sound::AudioDriver;
use crate::state::State;
use u6_testkit::assert_same;

pub(super) const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";

pub(super) fn files() -> Option<Files> {
    if !Path::new(ZIP).exists() {
        eprintln!("skipped: {ZIP} not present");
        return None;
    }
    let data = std::fs::read(ZIP).unwrap();
    let r = u6_intake::intake(vec![u6_intake::InputFile {
        name: "ultima-vi.zip".into(),
        data,
    }]);
    Some(Files::new(r.files))
}

/// The intro in an engine, with the outcome and the game state it changes.
pub(super) struct Run {
    pub(super) e: Engine,
    outcome: Rc<RefCell<Option<IntroOutcome>>>,
    state: Rc<RefCell<Option<Rc<RefCell<State>>>>>,
    /// The host's audio, when the run has one ([`Run::with_speaker`]).
    pub(super) speaker: Option<Speaker>,
}

/// Sample rate of the test's audio.
const RATE: u32 = 22_050;

/// The audio driver the hosts run, fed as they feed it: synced to the engine's sound log after
/// every tick and rendered for the tick's 20 ms.
pub(super) struct Speaker {
    pub(super) driver: AudioDriver,
    /// The loudest sample since the last [`Run::peak`].
    peak: f32,
    buf: Vec<f32>,
}

impl Run {
    /// The engine as the platforms boot it (`Engine::with_seed`: the launcher's sequence, `u`
    /// then `game`); no outcome is recorded.
    pub(super) fn launcher(files: Files, seed: u16) -> Self {
        Self {
            e: Engine::with_seed(files, seed).unwrap_or_else(|e| panic!("{e}")),
            outcome: Rc::new(RefCell::new(None)),
            state: Rc::new(RefCell::new(None)),
            speaker: None,
        }
    }

    /// The run with the host's audio driver on its files.
    pub(super) fn with_speaker(mut self) -> Self {
        let driver = AudioDriver::new(self.e.io().files()).unwrap_or_else(|e| panic!("{e}"));
        self.speaker = Some(Speaker {
            driver,
            peak: 0.0,
            buf: vec![0.0; 2 * (RATE / 50) as usize],
        });
        self
    }

    /// The loudest sample the speaker rendered since the last call (0 without a speaker).
    pub(super) fn peak(&mut self) -> f32 {
        self.speaker
            .as_mut()
            .map_or(0.0, |s| std::mem::take(&mut s.peak))
    }

    /// One engine tick of 20 ms with `ev`, then the host's audio step.
    fn step(&mut self, ev: &[InputEvent]) {
        self.e.tick(ev, 20);
        if let Some(sp) = &mut self.speaker {
            sp.driver.sync(&self.e.state().sound, self.e.io().files());
            sp.driver.render(&mut sp.buf, RATE);
            sp.peak = sp.buf.iter().fold(sp.peak, |m, &v| m.max(v.abs()));
        }
    }

    fn new(files: Files) -> Self {
        let state = State::new_game(&files).unwrap_or_else(|e| panic!("{e}"));
        let outcome = Rc::new(RefCell::new(None));
        let shared = Rc::new(RefCell::new(None));
        let (out, keep) = (Rc::clone(&outcome), Rc::clone(&shared));
        let e = Engine::with_game(state, files, move |s, io| async move {
            *keep.borrow_mut() = Some(Rc::clone(&s));
            let o = run(&s, &io).await;
            *out.borrow_mut() = Some(o);
        });
        Self {
            e,
            outcome,
            state: shared,
            speaker: None,
        }
    }

    pub(super) fn tick(&mut self, n: u32) {
        for _ in 0..n {
            self.step(&[]);
        }
    }

    /// Sends `keys`, ticks until the engine took them, then `settle` ticks more.
    pub(super) fn keys(&mut self, keys: &[Key], settle: u32) {
        let ev: Vec<InputEvent> = keys.iter().map(|&k| InputEvent::Key(k)).collect();
        self.step(&ev);
        let mut guard = 0;
        while self.e.io().pending_events() > 0 && guard < 20_000 {
            self.step(&[]);
            guard += 1;
        }
        self.tick(settle);
    }

    fn text(&mut self, s: &str, settle: u32) {
        for c in s.chars() {
            self.keys(&[Key::Char(c)], 30);
        }
        self.tick(settle);
    }

    /// The published frame: indices and the palette (32 colours, then the copper gradient's).
    pub(super) fn frame(&self) -> (Vec<u8>, Vec<(u8, u8, u8)>) {
        let f = self.e.frame();
        let pal = f.palette.iter().map(|c| (c.r, c.g, c.b)).collect();
        (f.pixels().to_vec(), pal)
    }
}

/// The scripted creation: answers A, B alternately; name "Zhuge", male, second portrait.
fn script(r: &mut Run, mut shot: impl FnMut(&str, &Run)) {
    // Opening: ORIGIN (palette 0 faded in by tick ~18), the second title card, the TV room.
    r.tick(30);
    shot("origin", r);
    r.tick(85);
    shot("lord-british", r);
    r.tick(150);
    shot("tv-room", r);
    r.keys(&[Key::Space], 60);
    shot("tv-room-2", r);
    // Passage 2 while the room pans away, then the storm waits for a key.
    r.keys(&[Key::Space], 120);
    r.keys(&[Key::Space], 160);
    shot("storm", r);
    for _ in 0..3 {
        r.keys(&[Key::Space], 40);
    }
    shot("door-pan", r);
    r.keys(&[Key::Space], 90);
    shot("forest", r);
    r.keys(&[Key::Space], 80);
    shot("stones-night", r);
    r.keys(&[Key::Space], 60);
    shot("stone-in-hand", r);
    r.keys(&[Key::Space], 60);
    shot("moongate-risen", r);
    r.keys(&[Key::Space], 90);
    shot("stone-close-up", r);
    r.keys(&[Key::Space], 60);
    shot("gate-apprehension", r);
    r.keys(&[Key::Space], 60);
    r.keys(&[Key::Space], 30);
    r.keys(&[Key::Space], 30);
    shot("walk-into-gate", r);
    r.keys(&[Key::Space], 40);
    shot("gate-sunk", r);
    r.tick(80);
    shot("empty-circle", r);
    r.keys(&[Key::Space], 60);
    shot("menu", r);
    r.keys(&[Key::Char('a')], 120);
    shot("credits", r);
    for _ in 0..4 {
        r.keys(&[Key::Space], 60);
    }
    shot("menu-again", r);
    create_from_menu(r, shot);
}

/// The creation from the menu: answers A, B alternately; name "Zhuge", male, second portrait;
/// Escape ends the introduction story that follows.
pub(super) fn create_from_menu(r: &mut Run, mut shot: impl FnMut(&str, &Run)) {
    r.keys(&[Key::Char('c')], 120);
    r.text("Zhuge", 10);
    r.keys(&[Key::Enter], 60);
    r.keys(&[Key::Char('m')], 60);
    shot("name-sex", r);
    r.keys(&[Key::Char('p')], 30);
    shot("portrait", r);
    r.keys(&[Key::Char('c')], 60);
    shot("woods", r);
    for _ in 0..3 {
        r.keys(&[Key::Space], 200);
    }
    shot("wagon", r);
    r.keys(&[Key::Space], 120);
    r.keys(&[Key::Space], 120);
    shot("question", r);
    for q in 0..7 {
        let k = if q % 2 == 0 { 'a' } else { 'b' };
        r.keys(&[Key::Char(k)], 220);
    }
    shot("flask", r);
    r.keys(&[Key::Space], 200);
    shot("farewell", r);
    for _ in 0..4 {
        r.keys(&[Key::Space], 120);
    }
    r.tick(60);
    // The introduction story follows a creation it was not seen before; Escape ends it.
    shot("story-after-creation", r);
    r.keys(&[Key::Escape], 200);
}

#[test]
fn a_scripted_creation_writes_the_character_into_the_objlist() {
    let Some(files) = files() else { return };
    let mut r = Run::new(files);
    script(&mut r, |_, _| {});
    let outcome = r.outcome.borrow().clone();
    assert_eq!(outcome, Some(IntroOutcome::Play { created: true }));
    let st = r.state.borrow().clone().unwrap();
    let s = st.borrow();
    assert_same!(s.actors.party.names[0], "Zhuge", "the created name");
    assert_eq!(s.actors.attrs.level[1], 3);
    assert_eq!(s.actors.attrs.hp[1], 90);
    assert_eq!(s.actors.attrs.exp[1], 370);
    assert_ne!(s.actors.objlist_tail[0x81], 0, "portrait byte");
    let hits = r.e.stub_hits();
    assert!(
        hits.keys().all(|k| k.starts_with("u ")),
        "only u's declared stubs: {hits:?}"
    );
}

/// PNG (RGB, stored deflate) of a frame.
pub(super) fn png(path: &Path, idx: &[u8], pal: &[(u8, u8, u8)]) {
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xedb8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    }
    let (w, h) = (320usize, 200usize);
    let mut raw = Vec::with_capacity(h * (1 + 3 * w));
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            let (r, g, b) = pal
                .get(usize::from(idx[y * w + x]))
                .copied()
                .unwrap_or((0, 0, 0));
            raw.extend_from_slice(&[r, g, b]);
        }
    }
    let mut z = vec![0x78, 0x01];
    for (i, chunk) in raw.chunks(0xffff).enumerate() {
        let last = (i + 1) * 0xffff >= raw.len();
        z.push(u8::from(last));
        let n = chunk.len() as u16;
        z.extend_from_slice(&n.to_le_bytes());
        z.extend_from_slice(&(!n).to_le_bytes());
        z.extend_from_slice(chunk);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc(&body).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    std::fs::write(path, out).unwrap();
}

/// Receipt: the key screens as PNG + raw indices, and the objlist fields, under the system
/// temp directory (`u6-intro-receipt/`). Run with `--ignored --nocapture`.
#[test]
#[ignore = "writes receipt images outside the repository"]
fn receipt_screens() {
    let Some(files) = files() else { return };
    let dir: PathBuf = std::env::temp_dir().join("u6-intro-receipt");
    std::fs::create_dir_all(&dir).unwrap();
    let mut r = Run::new(files);
    script(&mut r, |name, run| {
        let (idx, pal) = run.frame();
        png(&dir.join(format!("{name}.png")), &idx, &pal);
        std::fs::write(dir.join(format!("{name}.idx")), &idx).unwrap();
        println!("shot {name} at tick {}", run.e.ticks());
    });
    println!("outcome {:?}", r.outcome.borrow());
    let st = r.state.borrow().clone().unwrap();
    let s = st.borrow();
    println!(
        "state: the typed name kept {} str {} dex {} int {} magic {} hp {} level {} exp {} karma {} sex {} portrait {}",
        s.actors.party.names[0] == "Zhuge",
        s.actors.attrs.strength[1],
        s.actors.attrs.dexterity[1],
        s.actors.attrs.intelligence[1],
        s.actors.attrs.magic[1],
        s.actors.attrs.hp[1],
        s.actors.attrs.level[1],
        s.actors.attrs.exp[1],
        s.actors.karma,
        s.actors.objlist_tail[0x80],
        s.actors.objlist_tail[0x81],
    );
    let _ = handoff::summary;
    println!("stubs {:?}", r.e.stub_hits());
    println!("dir {}", dir.display());
}

/// Receipt aid: the opening from the TV room on, one PNG every 8 ticks with Space every 120,
/// under the system temp directory (`u6-intro-frames/`). Run with `--ignored --nocapture`.
#[test]
#[ignore = "writes receipt images outside the repository"]
fn receipt_opening_frames() {
    let Some(files) = files() else { return };
    let dir: PathBuf = std::env::temp_dir().join("u6-intro-frames");
    std::fs::create_dir_all(&dir).unwrap();
    let mut r = Run::new(files);
    r.tick(265);
    r.keys(&[Key::Space], 60);
    for k in 0..18 {
        r.keys(&[Key::Space], 0);
        for _ in 0..15 {
            r.tick(8);
            let (idx, pal) = r.frame();
            let t = r.e.ticks();
            png(&dir.join(format!("f{t:05}-k{k:02}.png")), &idx, &pal);
            std::fs::write(dir.join(format!("f{t:05}-k{k:02}.idx")), &idx).unwrap();
        }
    }
    println!("dir {}", dir.display());
}

/// Receipt aid: the introduction story from the menu, one PNG every 8 ticks with Space every
/// 120 (`u6-story-frames/`). Run with `--ignored --nocapture`.
#[test]
#[ignore = "writes receipt images outside the repository"]
fn receipt_story_frames() {
    let Some(files) = files() else { return };
    let dir: PathBuf = std::env::temp_dir().join("u6-story-frames");
    std::fs::create_dir_all(&dir).unwrap();
    let mut r = Run::new(files);
    r.tick(30);
    r.keys(&[Key::Escape], 120);
    r.keys(&[Key::Char('i')], 0);
    for k in 0..26 {
        for _ in 0..15 {
            r.tick(8);
            let (idx, pal) = r.frame();
            let t = r.e.ticks();
            png(&dir.join(format!("f{t:05}-k{k:02}.png")), &idx, &pal);
            std::fs::write(dir.join(format!("f{t:05}-k{k:02}.idx")), &idx).unwrap();
        }
        r.keys(&[Key::Space], 0);
    }
    println!("stubs {:?}", r.e.stub_hits());
    println!("dir {}", dir.display());
}

/// The transfer from the menu: B with no Ultima V save in the files ends in "No character
/// found!"; a key returns to the menu.
fn transfer_script(r: &mut Run, mut shot: impl FnMut(&str, &Run)) {
    r.tick(30);
    r.keys(&[Key::Escape], 120);
    r.keys(&[Key::Char('t')], 80);
    shot("transfer-dialog", r);
    r.keys(&[Key::Char('b')], 40);
    shot("transfer-result", r);
    r.keys(&[Key::Space], 80);
    shot("transfer-back-at-menu", r);
}

#[test]
fn transfer_without_a_save_says_no_character_found_and_returns_to_the_menu() {
    let Some(files) = files() else { return };
    let mut r = Run::new(files);
    let mut seen = Vec::new();
    transfer_script(&mut r, |name, run| {
        seen.push((name.to_string(), run.frame().0))
    });
    assert_eq!(*r.outcome.borrow(), None, "still in u (the menu)");
    let result = &seen[1].1;
    let dialog = &seen[0].1;
    assert!(result != dialog, "the text box changed");
    let hits = r.e.stub_hits();
    assert!(hits.keys().all(|k| k.starts_with("u ")), "{hits:?}");
    assert!(
        !hits.keys().any(|k| k.contains("0x10ac08")),
        "every passage decodes: {hits:?}"
    );
}

/// A synthetic Ultima V `saved.gam` (record at byte 2): name, male, Str 20 Dex 25 Int 18, MP 10,
/// HP 200 / 200, Exp 3000, level 5.
fn synthetic_u5_save() -> Vec<u8> {
    let mut b = vec![0u8; 0x40];
    b[2..9].copy_from_slice(b"Quintus");
    b[2 + 9] = 0xb;
    b[2 + 0xc] = 20;
    b[2 + 0xd] = 25;
    b[2 + 0xe] = 18;
    b[2 + 0xf] = 10;
    b[2 + 0x10..2 + 0x12].copy_from_slice(&200u16.to_be_bytes());
    b[2 + 0x12..2 + 0x14].copy_from_slice(&200u16.to_be_bytes());
    b[2 + 0x14..2 + 0x16].copy_from_slice(&3000u16.to_be_bytes());
    b[2 + 0x16] = 5;
    b
}

fn transfer_u5_script(r: &mut Run, mut shot: impl FnMut(&str, &Run)) {
    r.tick(30);
    r.keys(&[Key::Escape], 120);
    r.keys(&[Key::Char('t')], 80);
    r.keys(&[Key::Char('b')], 60);
    shot("transfer-sheet", r);
    r.keys(&[Key::Char('c')], 60);
    shot("transfer-portrait", r);
    r.keys(&[Key::Char('t')], 120);
    shot("transfer-story", r);
    r.keys(&[Key::Escape], 200);
}

#[test]
fn a_synthetic_ultima_v_save_is_transferred_into_the_objlist() {
    let Some(mut files) = files() else { return };
    let mut extra = std::collections::BTreeMap::new();
    extra.insert("saved.gam".to_string(), synthetic_u5_save());
    files.overlay("ultima5", extra);
    let mut r = Run::new(files);
    transfer_u5_script(&mut r, |_, _| {});
    assert_eq!(
        *r.outcome.borrow(),
        Some(IntroOutcome::Play { created: true })
    );
    let st = r.state.borrow().clone().unwrap();
    let s = st.borrow();
    assert_same!(s.actors.party.names[0], "Quintus", "the transferred name");
    let a = &s.actors.attrs;
    assert_eq!(
        (a.strength[1], a.dexterity[1], a.intelligence[1]),
        (20, 25, 18)
    );
    assert_eq!(
        (a.exp[1], a.level[1], a.hp[1], a.magic[1]),
        (300, 3, 90, 36)
    );
}

/// Receipt: the transfer screens (no save, then a synthetic Ultima V save) as PNG + raw indices
/// under the system temp directory (`u6-intro-receipt/`). Run with `--ignored --nocapture`.
#[test]
#[ignore = "writes receipt images outside the repository"]
fn receipt_transfer_screens() {
    let Some(files) = files() else { return };
    let dir: PathBuf = std::env::temp_dir().join("u6-intro-receipt");
    std::fs::create_dir_all(&dir).unwrap();
    let save = |name: &str, run: &Run| {
        let (idx, pal) = run.frame();
        png(&dir.join(format!("{name}.png")), &idx, &pal);
        std::fs::write(dir.join(format!("{name}.idx")), &idx).unwrap();
        println!("shot {name} at tick {}", run.e.ticks());
    };
    let mut r = Run::new(files.clone());
    transfer_script(&mut r, save);
    let mut with_save = files;
    let mut extra = std::collections::BTreeMap::new();
    extra.insert("saved.gam".to_string(), synthetic_u5_save());
    with_save.overlay("ultima5", extra);
    let mut r = Run::new(with_save);
    transfer_u5_script(&mut r, save);
    println!("outcome {:?}", r.outcome.borrow());
    println!("stubs {:?}", r.e.stub_hits());
}

/// Every passage the opening, the story and the transfer lay out decodes from the user's `u`
/// (no evaluator stub fires) and puts text into the passage bitmap.
#[test]
fn every_passage_of_this_unit_decodes() {
    let Some(files) = files() else { return };
    let hits = Rc::new(RefCell::new(Vec::new()));
    let h2 = Rc::clone(&hits);
    let f2 = files.clone();
    let state = State::new_game(&files).unwrap_or_else(|e| panic!("{e}"));
    let mut e = Engine::with_game(state, files, move |_s, io| async move {
        let mut st = crate::intro::state_for(&f2).unwrap();
        st.font = crate::intro::load::load_lzw(&st.files, b"u6.set")
            .and_then(|d| u6_formats::font::WouFont::parse(&d).ok());
        let screen = crate::intro::mem::View::screen(crate::intro::mem::Ptr::at(
            crate::intro::mem::Region::Screen1,
        ));
        st.views.draw = screen;
        st.views.screen = screen;
        st.text.view = screen;
        let ids = (3..=0xf).chain(0xc8..=0xdb).chain([0x3e6]);
        for n in ids {
            st.views.passage = crate::intro::mem::View {
                planes: crate::intro::mem::Ptr::at(crate::intro::mem::Region::Work),
                ..screen
            };
            st.views.passage.y1 = 0x37;
            let work = st
                .video
                .chip
                .bytes_mut(crate::intro::mem::Ptr::at(crate::intro::mem::Region::Work));
            work.iter_mut().for_each(|b| *b = 0);
            crate::intro::passages::passage(&mut st, &io, n);
            let p = st.views.passage;
            let inked = (p.y0..=p.y1).any(|y| {
                (p.x0..=p.x1)
                    .any(|x| st.video.chip.pixel(p.planes, i32::from(x), i32::from(y)) == 0)
            });
            h2.borrow_mut().push((n, inked));
        }
    });
    for _ in 0..10 {
        e.tick(&[], 20);
    }
    let stubs = e.stub_hits();
    assert!(
        !stubs.keys().any(|k| k.contains("0x10ac08")),
        "evaluator stubs: {stubs:?}"
    );
    let got = hits.borrow();
    assert_eq!(got.len(), 13 + 20 + 1);
    let blank: Vec<u16> = got
        .iter()
        .filter(|(_, ink)| !ink)
        .map(|&(n, _)| n)
        .collect();
    assert!(blank.is_empty(), "passages without ink: {blank:x?}");
}
