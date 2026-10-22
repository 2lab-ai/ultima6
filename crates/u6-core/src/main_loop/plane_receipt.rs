//! Spec 07 Acceptance 3, the source receipt (owner-local): the user's game through the real
//! command loop with the source plane on, a few thousand steps (walks, the spellbook, a
//! portrait). On every frame the engine shows, each attributed pixel without flags whose version
//! is current must equal its image's classic pixel at (u, v): the record as the game holds it
//! now, the portrait and the panel bitmaps decoded as the engine decodes them, through the
//! bitmap colour table. Skips where the zip is missing; prints pass/fail and counts only.
#![allow(clippy::disallowed_methods)] // tests may read files

use std::collections::BTreeMap;

use u6_formats::bitmap::Bitmap;
use u6_render::{BITMAP_PAPER, BITMAP_SPELLBOOK, Kind, WIDTH, portrait_of};

use super::receipts::{Setup, boot_then, chars, files, give_book};
use crate::engine::{Engine, InputEvent, Key};
use crate::state::State;
use u6_testkit::Quiet;

/// What the receipt counted over the frames it checked.
#[derive(Default)]
struct Receipt {
    /// The frame version last checked.
    seen: u64,
    frames: u64,
    /// Pixels checked against their image: tile, portrait, paper, spellbook.
    checked: [u64; 4],
    /// Tile pixels whose record changed since they were drawn (not checked).
    stale: u64,
    /// Pixels the blit flagged (overlay, outline; not checked).
    flagged: u64,
    /// Checked pixels that differ from their image.
    wrong: u64,
    /// Map-window pixels (8..168 square) attributed to a tile, and all of them.
    window: (u64, u64),
    /// The portraits decoded so far, by id.
    portraits: BTreeMap<u16, Option<Vec<u8>>>,
    paper: Option<Bitmap>,
    book: Option<Bitmap>,
}

impl Receipt {
    /// Checks the frame `e` shows when it was composed since the last check.
    fn check(&mut self, e: &Engine) {
        if e.frame_version() == self.seen {
            return;
        }
        self.seen = e.frame_version();
        self.frames += 1;
        let s = e.state();
        let fb = e.frame();
        let colours = &s.gfx.tables.bitmap_colours;
        let through = |b: u8| colours[usize::from(b)] & 0x1f;
        for (i, src) in fb.src().quiet().iter().enumerate() {
            let (x, y) = (i % WIDTH, i / WIDTH);
            if (8..168).contains(&x) && (8..168).contains(&y) {
                self.window.1 += 1;
                self.window.0 += u64::from(src.kind == Kind::Tile);
            }
            // A glyph names no image (spec 11): its colours are the text's, as a classic pixel's.
            if matches!(src.kind, Kind::Classic | Kind::Glyph) {
                continue;
            }
            if src.flags != 0 {
                self.flagged += 1;
                continue;
            }
            let (u, v) = (usize::from(src.u), usize::from(src.v));
            let want = match src.kind {
                Kind::Tile => {
                    let id = usize::from(src.id);
                    if src.ver != s.gfx.cache.version(id) {
                        self.stale += 1;
                        continue;
                    }
                    self.checked[0] += 1;
                    let rec = s.gfx.records.get(id);
                    rec.and_then(|r| (u < 16).then(|| r.pixels.get(v * 16 + u)).flatten())
                        .copied()
                }
                Kind::Portrait => {
                    self.checked[1] += 1;
                    let image = self.portraits.entry(src.id).or_insert_with(|| {
                        let (letter, item) = portrait_of(src.id);
                        let files = e.io().files();
                        crate::converse::portrait_pixels(&s, files, letter, u16::from(item))
                    });
                    let at = (u < 56).then_some(v * 56 + u);
                    image.as_ref().and_then(|p| p.get(at?)).map(|&b| through(b))
                }
                Kind::Bitmap => {
                    let bmp = match src.id {
                        BITMAP_PAPER => self.paper.as_ref(),
                        BITMAP_SPELLBOOK => self.book.as_ref(),
                        _ => None,
                    };
                    self.checked[if src.id == BITMAP_PAPER { 2 } else { 3 }] += 1;
                    bmp.and_then(|b| {
                        let w = usize::from(b.width);
                        (u < w).then(|| b.pixels.get(v * w + u)).flatten()
                    })
                    .map(|&b| through(b))
                }
                Kind::Classic | Kind::Glyph => None,
            };
            if want != Some(fb.pixels()[i]) {
                self.wrong += 1;
            }
        }
    }
}

/// One tick with `input`, then the check.
fn tick(e: &mut Engine, r: &mut Receipt, input: &[InputEvent]) {
    e.tick(input, 16);
    r.check(e);
}

fn idle(e: &mut Engine, r: &mut Receipt, n: u32) {
    for _ in 0..n {
        tick(e, r, &[]);
    }
}

fn press(e: &mut Engine, r: &mut Receipt, keys: &[Key], settle: u32) {
    for &k in keys {
        tick(e, r, &[InputEvent::Key(k)]);
        idle(e, r, settle);
    }
}

#[test]
fn the_source_plane_names_the_classic_pixels_of_the_users_game() {
    let Some(files) = files() else { return };
    let setup: Setup = Box::new(|s: &mut State| {
        give_book(s, &[0, 1, 6, 9]);
        // The game-first patch of the hero's portrait byte (re/notes/oracle.md §D), so Look at
        // the hero shows a portrait.
        s.actors.objlist_tail[0x81] = 1;
    });
    let state = State::new_game(&files).quiet();
    let mut e = Engine::with_game(state, files.clone(), move |st, io| async move {
        boot_then(&st, &io, setup).await;
    });
    e.enable_src_plane();
    let mut r = Receipt {
        paper: super::paper_bitmap(e.io()),
        book: crate::magic::book_bitmap(e.io()),
        ..Receipt::default()
    };
    idle(&mut e, &mut r, 200);
    for k in [
        Key::Right,
        Key::Right,
        Key::Down,
        Key::Down,
        Key::Left,
        Key::Up,
    ] {
        press(&mut e, &mut r, &[k], 80);
    }
    // The spellbook panel (C), closed; Look at the hero (the cursor starts on the leader): the
    // portrait panel until a key.
    press(&mut e, &mut r, &chars("c"), 60);
    press(&mut e, &mut r, &[Key::Escape], 60);
    press(&mut e, &mut r, &chars("l"), 30);
    press(&mut e, &mut r, &[Key::Enter], 120);
    press(&mut e, &mut r, &[Key::Space], 60);
    idle(&mut e, &mut r, 2000);
    let (tiles, window) = r.window;
    // Pass: no attributed pixel differs from its image, and every kind was checked.
    let covered = r.checked.iter().all(|&n| n > 0);
    let pass = r.wrong == 0 && covered;
    println!(
        "source receipt: {}; {} frames; checked {} tile, {} portrait, {} paper, {} spellbook \
         pixels; {} stale, {} flagged; map window {:.1}% tile ({tiles} of {window})",
        if pass { "pass" } else { "FAIL" },
        r.frames,
        r.checked[0],
        r.checked[1],
        r.checked[2],
        r.checked[3],
        r.stale,
        r.flagged,
        tiles as f64 * 100.0 / window.max(1) as f64,
    );
    assert!(
        r.wrong == 0,
        "{} attributed pixels differ from their image",
        r.wrong
    );
    assert!(covered, "every kind was checked");
}
