//! Spec 07 Acceptance 3, the source receipt (owner-local): the user's game through the real
//! command loop with the source plane on, a few thousand steps (walks, the spellbook, a
//! portrait). On every frame the engine shows, each attributed pixel without flags whose version
//! is current must equal its image's classic pixel at (u, v): the record as the game holds it
//! now, the portrait and the panel bitmaps decoded as the engine decodes them, through the
//! bitmap colour table. The walks scroll the message window while its last row is still the
//! paper's, so a pixel of the paper of another colour than the paper above it is moved up (the
//! receipt checks that it was, where the paper has one). The panels the run shows (the
//! spellbook, the party, a portrait, an inventory) are cleared over the paper, and their clears
//! name it where it is of their colour; no pixel of the message window shows the paper's own
//! colour flat (classic, or a glyph's not drawn over the paper) on any frame. Skips where the zip
//! is missing; prints pass/fail and counts only.
#![allow(clippy::disallowed_methods)] // tests may read files

use std::collections::BTreeMap;

use u6_formats::bitmap::Bitmap;
use u6_render::{BITMAP_PAPER, BITMAP_SPELLBOOK, FLAG_INK, FLAG_PAPER, Kind, WIDTH, portrait_of};

use super::receipts::{Setup, boot_then, chars, files, give_book, hatch_no_eggs};
use crate::engine::{Engine, InputEvent, Key};
use crate::state::State;
use crate::ui::WindowId;
use crate::ui::text::Paper;
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
    /// Pixels of the shown map ([`crate::geom::SHOWN`]) attributed to a tile, and all of them.
    window: (u64, u64),
    /// The portraits decoded so far, by id.
    portraits: BTreeMap<u16, Option<Vec<u8>>>,
    paper: Option<Bitmap>,
    book: Option<Bitmap>,
    /// Whether the paper under the message window's last pixel row has a pixel of another colour
    /// than the paper one text row above it, and the pixels of the window, a whole number of text
    /// rows above such a pixel, that hold its colour where the paper's own colour is another (the
    /// scroll moved it up), summed over the frames; glyph pixels are not counted.
    corner: (bool, u64),
    /// Pixels of the message window shown flat in the paper's own colour at their place, summed
    /// over the frames: classic ones, and glyph pixels without ink not drawn over the paper.
    window_flat: u64,
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
        let (x0, y0, x1, y1) = crate::geom::SHOWN;
        let (xs, ys) = (
            usize::from(x0)..=usize::from(x1),
            usize::from(y0)..=usize::from(y1),
        );
        for (i, src) in fb.src().quiet().iter().enumerate() {
            let (x, y) = (i % WIDTH, i / WIDTH);
            if xs.contains(&x) && ys.contains(&y) {
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
        let w = s.ui.text.windows[WindowId::Messages.index()];
        if let Some(p) = s.ui.paper.as_ref().filter(|_| w.width > 0) {
            let (x0, y0) = (usize::from(w.x) * 8, usize::from(w.y) * 8);
            let x1 = (x0 + usize::from(w.width) * 8).min(WIDTH);
            let y1 = (y0 + usize::from(w.height) * 8).min(u6_render::HEIGHT);
            let src = fb.src().quiet();
            for (x, y) in (y0..y1).flat_map(|y| (x0..x1).map(move |x| (x, y))) {
                let (i, sp) = (y * WIDTH + x, src[y * WIDTH + x]);
                let flat = match sp.kind {
                    Kind::Classic => true,
                    Kind::Glyph => sp.flags & (FLAG_INK | FLAG_PAPER) == 0,
                    _ => false,
                };
                if flat && p.at(x, y) == Some(fb.pixels()[i]) {
                    self.window_flat += 1;
                }
            }
        }
        if let Some(b) = self.paper.as_ref().filter(|_| w.width > 0 && w.height > 1) {
            let bw = usize::from(b.width);
            let paper = |x: usize, y: usize| {
                let at = (x < bw).then(|| b.pixels.get(y * bw + x)).flatten();
                at.map(|&p| through(p))
            };
            let (x0, y0) = (usize::from(w.x) * 8, usize::from(w.y) * 8);
            let x1 = (x0 + usize::from(w.width) * 8).min(WIDTH);
            let last = y0 + usize::from(w.height) * 8 - 1;
            let src = fb.src().quiet();
            for x in x0..x1 {
                let Some(c) = paper(x, last) else { continue };
                if paper(x, last - 8) == Some(c) {
                    continue;
                }
                self.corner.0 = true;
                for y in (y0..last - 7).rev().step_by(8) {
                    let i = y * WIDTH + x;
                    let moved = fb.pixels()[i] == c && paper(x, y) != Some(c);
                    if moved && src[i].kind != Kind::Glyph {
                        self.corner.1 += 1;
                    }
                }
            }
        }
    }
}

/// The areas the panels' clears fill, inclusive: the side panel's (`ui::panels` `PANEL`), the
/// portrait panel's (`inventory::portrait`), the inventory list's (`inventory::panel::draw_list`),
/// and of the spellbook's clear (`magic::book::draw_book`, the portrait panel's area) the rows
/// below the book's picture, where the clear stays in sight (the book's page clear over the
/// picture is the book's, not the paper's).
const PANEL: (usize, usize, usize, usize) = (0xa8, 6, 0x138, 0x67);
const PORTRAIT: (usize, usize, usize, usize) = (0xa8, 6, 0x137, 0x67);
const LIST: (usize, usize, usize, usize) = (0xf8, 0x10, 0x137, 0x4f);
const BOOK_FOOT: (usize, usize, usize, usize) = (0xa8, 0x58, 0x137, 0x67);

/// In the frame `e` shows, the pixels of the inclusive rect that name the paper unflagged (the
/// HD paper shows there), and the classic ones of the paper's own colour there (shown flat where
/// the HD paper could show).
fn paper_in(e: &Engine, (x0, y0, x1, y1): (usize, usize, usize, usize)) -> (usize, usize) {
    let s = e.state();
    let Some(paper) = s.ui.paper.as_ref() else {
        return (0, 0);
    };
    let fb = e.frame();
    let (mut named, mut flat) = (0, 0);
    for (x, y) in (y0..=y1).flat_map(|y| (x0..=x1).map(move |x| (x, y))) {
        let src = fb.src_at(x, y).quiet();
        if src.kind == Kind::Bitmap && src.id == BITMAP_PAPER && src.flags == 0 {
            named += 1;
        } else if src.kind == Kind::Classic && fb.get(x, y) == paper.at(x, y) {
            flat += 1;
        }
    }
    (named, flat)
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
        // The walks below meet no hatchlings in either build (in Wide they move the window).
        hatch_no_eggs(s);
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
    // Twelve more steps, then two passes: the steps print no line, each pass prints its own and
    // the prompt, so with the lines of the keys after them the message window scrolls while its
    // last row is still the paper's.
    for k in [Key::Left, Key::Right].repeat(6) {
        press(&mut e, &mut r, &[k], 80);
    }
    press(&mut e, &mut r, &[Key::Space, Key::Space], 80);
    // The spellbook panel (C), closed; Look at the hero (the cursor starts on the leader): the
    // portrait panel until a key; the party panel again; a click on the leader's figure in it:
    // the leader's inventory panel.
    press(&mut e, &mut r, &chars("c"), 60);
    let book = paper_in(&e, BOOK_FOOT);
    press(&mut e, &mut r, &[Key::Escape], 60);
    press(&mut e, &mut r, &chars("l"), 30);
    press(&mut e, &mut r, &[Key::Enter], 120);
    let portrait = paper_in(&e, PORTRAIT);
    press(&mut e, &mut r, &[Key::Space], 60);
    idle(&mut e, &mut r, 200);
    let party = paper_in(&e, PANEL);
    let (x, y) = (0xb8, 0x20);
    let click = [
        InputEvent::Pointer { x, y, buttons: 1 },
        InputEvent::Pointer { x, y, buttons: 0 },
    ];
    tick(&mut e, &mut r, &click);
    idle(&mut e, &mut r, 200);
    let inventory = paper_in(&e, LIST);
    idle(&mut e, &mut r, 2000);
    let (tiles, window) = r.window;
    // The boot kept the colours it drew of the paper, and the message window's last text row,
    // cleared by the scrolls since, names the paper where it is not text.
    let s = e.state();
    let colours = &s.gfx.tables.bitmap_colours;
    let drawn = r.paper.as_ref().map(|b| Paper::drawn(colours, b));
    assert!(s.ui.paper == drawn, "the boot kept the paper's colours");
    let w = s.ui.text.windows[WindowId::Messages.index()];
    let (x0, y0) = (
        usize::from(w.x) * 8,
        (usize::from(w.y) + usize::from(w.height) - 1) * 8,
    );
    let named = (y0..y0 + 8)
        .flat_map(|y| (x0..x0 + usize::from(w.width) * 8).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let src = e.frame().src_at(x, y);
            src.is_some_and(|p| p.kind == Kind::Bitmap && p.id == BITMAP_PAPER && p.flags == 0)
        })
        .count();
    drop(s);
    // Pass: no attributed pixel differs from its image, every kind was checked, the scroll moved
    // the paper's off-colour pixel up where the paper has one, and the row it cleared names the
    // paper.
    let covered = r.checked.iter().all(|&n| n > 0);
    let moved = !r.corner.0 || r.corner.1 > 0;
    let cleared = drawn.is_none() || named > 0;
    // The panels' clears name the paper, each over its own area, none of it flat paper colour:
    // the spellbook's, the portrait's, the party panel's and the inventory list's.
    let named_all = |(named, flat): (usize, usize)| named > 0 && flat == 0;
    let panels = drawn.is_none()
        || [book, portrait, party, inventory]
            .into_iter()
            .all(named_all);
    let flat = drawn.is_some() && r.window_flat > 0;
    let pass = r.wrong == 0 && covered && moved && cleared && panels && !flat;
    println!(
        "source receipt: {}; {} frames; checked {} tile, {} portrait, {} paper, {} spellbook \
         pixels; {} stale, {} flagged; {} wrong; the paper's off-colour pixel moved up {} \
         (pixel-frames: {}); {named} pixels of the scroll's last row name the paper; the message \
         window flat in the paper's colour {} (pixel-frames); the panels name the paper / show \
         it flat: spellbook {book:?}, portrait {portrait:?}, party {party:?}, inventory list \
         {inventory:?}; map window {:.1}% tile ({tiles} of {window})",
        if pass { "pass" } else { "FAIL" },
        r.frames,
        r.checked[0],
        r.checked[1],
        r.checked[2],
        r.checked[3],
        r.stale,
        r.flagged,
        r.wrong,
        match r.corner {
            (false, _) => "(none in this paper)",
            (true, 0) => "never",
            (true, _) => "yes",
        },
        r.corner.1,
        r.window_flat,
        tiles as f64 * 100.0 / window.max(1) as f64,
    );
    assert!(
        r.wrong == 0,
        "{} attributed pixels differ from their image",
        r.wrong
    );
    assert!(covered, "every kind was checked");
    assert!(moved, "the scroll moved the paper's off-colour pixel up");
    assert!(cleared, "the scroll's cleared row names the paper");
    assert!(panels, "the panels' clears name the paper");
    assert!(
        !flat,
        "no pixel of the message window shows the paper's colour flat"
    );
}
