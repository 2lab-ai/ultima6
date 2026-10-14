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
use crate::state::State;

const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";

fn files() -> Option<Files> {
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
struct Run {
    e: Engine,
    outcome: Rc<RefCell<Option<IntroOutcome>>>,
    state: Rc<RefCell<Option<Rc<RefCell<State>>>>>,
}

impl Run {
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
        }
    }

    fn tick(&mut self, n: u32) {
        for _ in 0..n {
            self.e.tick(&[], 20);
        }
    }

    /// Sends `keys`, ticks until the engine took them, then `settle` ticks more.
    fn keys(&mut self, keys: &[Key], settle: u32) {
        let ev: Vec<InputEvent> = keys.iter().map(|&k| InputEvent::Key(k)).collect();
        self.e.tick(&ev, 20);
        let mut guard = 0;
        while self.e.io().pending_events() > 0 && guard < 20_000 {
            self.e.tick(&[], 20);
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

    /// The published frame: indices and the 32 colours as 12-bit words.
    fn frame(&self) -> (Vec<u8>, Vec<(u8, u8, u8)>) {
        let f = self.e.frame();
        let pal = f.palette[..32].iter().map(|c| (c.r, c.g, c.b)).collect();
        (f.pixels().to_vec(), pal)
    }
}

/// The scripted creation: answers A, B alternately; name "Zhuge", male, second portrait.
fn script(r: &mut Run, mut shot: impl FnMut(&str, &Run)) {
    // Opening: ORIGIN (palette 0 faded in by tick ~18), Lord British, the TV room.
    r.tick(30);
    shot("origin", r);
    r.tick(85);
    shot("lord-british", r);
    r.tick(150);
    shot("tv-room", r);
    r.keys(&[Key::Space], 60);
    shot("tv-room-2", r);
    r.keys(&[Key::Space], 400);
    shot("menu", r);
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
    assert_eq!(s.actors.party.names[0], "Zhuge");
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
fn png(path: &Path, idx: &[u8], pal: &[(u8, u8, u8)]) {
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
        "state: name {:?} str {} dex {} int {} magic {} hp {} level {} exp {} karma {} sex {} portrait {}",
        s.actors.party.names[0],
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
