//! Spec 07 HD0, the HD frame on the user's game (owner-local): the source receipt's run (walks,
//! the spellbook, a portrait; `plane_receipt`) with the platform's compositor kept across it. On
//! every frame the engine shows, each block of the 4× frame must be what the definition gives:
//! for a current unflagged tile, portrait or panel bitmap pixel, its block of MMPX ×4 of its
//! image decoded here independently (the record now, the portrait and the panel bitmaps through
//! the bitmap colour table), its transparent sub-pixels the classic colour; for every other
//! pixel the classic colour. Every 50th frame a new compositor must make the same frame. Skips
//! where the zip is missing; prints pass/fail and counts only.
#![allow(clippy::disallowed_methods)] // tests may read files

use std::collections::HashMap;
use std::rc::Rc;

use u6_formats::bitmap::Bitmap;
use u6_render::hd::{Compositor, Scale, mmpx, word};
use u6_render::{BITMAP_PAPER, BITMAP_SPELLBOOK, HEIGHT, Kind, WIDTH, portrait_of};

use super::receipts::{Setup, boot_then, chars, files, give_book};
use crate::engine::{Engine, InputEvent, Key};
use crate::hd::ImageCache;
use crate::state::State;
use u6_testkit::Quiet;

const SCALE: Scale = Scale::X4;

/// What the receipt counted.
#[derive(Default)]
struct Receipt {
    /// The (frame, source) versions last checked.
    seen: (u64, u64),
    frames: u64,
    /// HD blocks checked by kind: tile, portrait, paper, spellbook; and classic blocks.
    hd: [u64; 4],
    classic: u64,
    /// Blocks that differ from the definition's.
    wrong: u64,
    /// Frames a new compositor made too, and those that differed.
    fresh: (u64, u64),
    compositor: Compositor,
    images: ImageCache,
    /// MMPX ×4 by input content.
    made: HashMap<(usize, usize, Vec<u32>), Rc<Vec<u32>>>,
    paper: Option<Bitmap>,
    book: Option<Bitmap>,
}

impl Receipt {
    /// The image of a current unflagged pixel's source as the definition takes it: width, height
    /// and words (transparent 0); `None` for a pixel shown classic: a record at another version
    /// than its own now, a portrait or a panel bitmap at any version but 0 (they stay at 0 while
    /// loaded), an image missing or shorter than its size, and a bitmap past the screen (the
    /// compositor shows it classic: `u6_render::hd::Image`).
    fn image(
        (paper, book): (Option<&Bitmap>, Option<&Bitmap>),
        e: &Engine,
        s: &State,
        src: u6_render::Src,
        pal: &[u32; 256],
    ) -> Option<(usize, usize, Vec<u32>)> {
        let colours = &s.gfx.tables.bitmap_colours;
        let through = |b: u8| pal[usize::from(colours[usize::from(b)] & 0x1f)];
        match src.kind {
            Kind::Tile => {
                let id = usize::from(src.id);
                let rec = s.gfx.records.get(id)?;
                (src.ver == s.gfx.cache.version(id)).then(|| {
                    let w = (0..256).map(|i| {
                        if rec.transparent[i] {
                            0
                        } else {
                            pal[usize::from(rec.pixels[i])]
                        }
                    });
                    (16, 16, w.collect())
                })
            }
            Kind::Portrait | Kind::Bitmap if src.ver != 0 => None,
            Kind::Portrait => {
                let (letter, item) = portrait_of(src.id);
                let px =
                    crate::converse::portrait_pixels(s, e.io().files(), letter, u16::from(item))?;
                Some((56, 64, px.into_iter().map(through).collect()))
            }
            Kind::Bitmap => {
                let b = match src.id {
                    BITMAP_PAPER => paper,
                    BITMAP_SPELLBOOK => book,
                    _ => None,
                }?;
                let (w, h) = (usize::from(b.width), usize::from(b.height));
                if w > WIDTH || h > HEIGHT {
                    return None;
                }
                let pixels = b.pixels.get(..w * h)?;
                Some((w, h, pixels.iter().map(|&p| through(p)).collect()))
            }
            Kind::Classic | Kind::Glyph => None,
        }
    }

    /// Composes and checks the frame `e` shows when it changed since the last check.
    fn check(&mut self, e: &Engine) {
        let now = (e.frame_version(), e.src_version());
        if now == self.seen {
            return;
        }
        self.seen = now;
        self.frames += 1;
        let s = e.state();
        let fb = e.frame();
        self.compositor
            .compose(fb, &mut self.images.images(&s, e.io()), SCALE);
        let pal: [u32; 256] = std::array::from_fn(|i| word(fb.palette[i]));
        let sf = SCALE.factor();
        let src = fb.src().quiet();
        // This frame's images by (kind, id, version): their size and MMPX ×4.
        type Hd = Option<(usize, usize, Rc<Vec<u32>>)>;
        let mut frame: HashMap<(Kind, u16, u32), Hd> = HashMap::new();
        for (i, &sp) in src.iter().enumerate() {
            let (x, y) = (i % WIDTH, i / WIDTH);
            let classic = pal[usize::from(fb.pixels()[i])];
            let image = if sp.kind == Kind::Classic || sp.flags != 0 {
                None
            } else {
                let made = &mut self.made;
                let bitmaps = (self.paper.as_ref(), self.book.as_ref());
                let image = |sp| Self::image(bitmaps, e, &s, sp, &pal);
                frame
                    .entry((sp.kind, sp.id, sp.ver))
                    .or_insert_with(|| {
                        image(sp).map(|(w, h, words)| {
                            let hd =
                                made.entry((w, h, words))
                                    .or_insert_with_key(|(w, h, words)| {
                                        let mut out = Vec::new();
                                        mmpx(words, *w, *h, SCALE, &mut out);
                                        Rc::new(out)
                                    });
                            (w, h, Rc::clone(hd))
                        })
                    })
                    .clone()
            };
            let (u, v) = (usize::from(sp.u), usize::from(sp.v));
            let want: Vec<u32> = match image {
                Some((w, h, hd)) if u < w && v < h => {
                    self.hd[match sp.kind {
                        Kind::Tile => 0,
                        Kind::Portrait => 1,
                        _ if sp.id == BITMAP_PAPER => 2,
                        _ => 3,
                    }] += 1;
                    (0..sf * sf)
                        .map(|k| hd[(v * sf + k / sf) * w * sf + u * sf + k % sf])
                        .map(|c| if c == 0 { classic } else { c })
                        .collect()
                }
                _ => {
                    self.classic += 1;
                    vec![classic; sf * sf]
                }
            };
            let out = self.compositor.rgba();
            let got = (0..sf * sf).map(|k| out[(y * sf + k / sf) * WIDTH * sf + x * sf + k % sf]);
            if !got.eq(want) {
                self.wrong += 1;
            }
        }
        if self.frames.is_multiple_of(50) {
            let mut fresh = Compositor::new();
            fresh.compose(fb, &mut ImageCache::new().images(&s, e.io()), SCALE);
            self.fresh.0 += 1;
            self.fresh.1 += u64::from(fresh.rgba() != self.compositor.rgba());
        }
        assert_eq!(self.compositor.rgba().len(), WIDTH * HEIGHT * sf * sf);
    }
}

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
fn the_hd_frame_of_the_users_game_is_the_definitions() {
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
    press(&mut e, &mut r, &chars("c"), 60);
    press(&mut e, &mut r, &[Key::Escape], 60);
    press(&mut e, &mut r, &chars("l"), 30);
    press(&mut e, &mut r, &[Key::Enter], 120);
    press(&mut e, &mut r, &[Key::Space], 60);
    idle(&mut e, &mut r, 600);
    let covered = r.hd.iter().all(|&n| n > 0);
    let pass = r.wrong == 0 && r.fresh.1 == 0 && covered;
    println!(
        "hd receipt: {}; {} frames at 4x; HD blocks {} tile, {} portrait, {} paper, {} spellbook; \
         {} classic; {} blocks off the definition; a new compositor on {} frames, {} differ; \
         retained cache payload peak {} KiB of {} KiB",
        if pass { "pass" } else { "FAIL" },
        r.frames,
        r.hd[0],
        r.hd[1],
        r.hd[2],
        r.hd[3],
        r.classic,
        r.wrong,
        r.fresh.0,
        r.fresh.1,
        r.compositor.cache().peak() >> 10,
        r.compositor.cache().cap() >> 10,
    );
    assert_eq!(r.wrong, 0, "blocks off the definition");
    assert_eq!(r.fresh.1, 0, "a new compositor's frame");
    assert!(covered, "every kind was shown HD");
}

/// The receipt's own reference on synthetic images: a portrait (a one-item archive) and a panel
/// bitmap at version 0 are their images; at any other version, a bitmap shorter than its size
/// and one past the screen, they are classic (`None`).
#[test]
fn the_reference_shows_a_portrait_or_a_bitmap_at_another_version_classic() {
    use crate::converse::text::{ConverseText, MESSAGES, Msg};
    use std::collections::BTreeMap;
    let face: Vec<u8> = (0..56 * 64).map(|i| (i % 7) as u8 + 1).collect();
    let mut lib = 4u32.to_le_bytes().to_vec();
    lib.extend(u6_formats::lzw::encode(&face).quiet());
    let files = crate::Files::new(BTreeMap::from([("facefile.a".to_string(), lib)]));
    let mut s = State::empty();
    s.converse.text = Some(ConverseText::from_pairs(MESSAGES.iter().map(|&(m, _)| {
        let t: &[u8] = if m == Msg::PortraitFile {
            b"facefile.?"
        } else {
            b""
        };
        (m, t.to_vec())
    })));
    s.gfx.tables.bitmap_colours = std::array::from_fn(|i| i as u8);
    let e = Engine::with_game(s, files, |_, _| async {});
    let st = e.state();
    let pal: [u32; 256] = std::array::from_fn(|i| 0xff00_0000 | i as u32);
    let paper = Bitmap {
        width: 8,
        height: 4,
        pixels: (0..32).map(|i| (i % 5) as u8).collect(),
    };
    let short = Bitmap {
        pixels: vec![1; 31],
        ..paper.clone()
    };
    let src = |kind, id, ver| u6_render::Src {
        ver,
        id,
        kind,
        ..u6_render::Src::CLASSIC
    };
    let face_id = u6_render::portrait_id(b'a', 0).quiet();
    let image = |bitmaps, sp| Receipt::image(bitmaps, &e, &st, sp, &pal).map(|(w, h, _)| (w, h));
    assert_eq!(
        image((None, None), src(Kind::Portrait, face_id, 0)),
        Some((56, 64))
    );
    assert_eq!(image((None, None), src(Kind::Portrait, face_id, 1)), None);
    assert_eq!(
        image((Some(&paper), None), src(Kind::Bitmap, BITMAP_PAPER, 0)),
        Some((8, 4))
    );
    assert_eq!(
        image((Some(&paper), None), src(Kind::Bitmap, BITMAP_PAPER, 3)),
        None
    );
    assert_eq!(
        image((None, Some(&paper)), src(Kind::Bitmap, BITMAP_SPELLBOOK, 1)),
        None
    );
    assert_eq!(
        image((Some(&short), None), src(Kind::Bitmap, BITMAP_PAPER, 0)),
        None
    );
    // Past the screen one way or the other (full pixel data): classic, as the compositor shows
    // it; the screen's own size is an image.
    let sizes = [
        (WIDTH + 1, 1, false),
        (1, HEIGHT + 1, false),
        (WIDTH, HEIGHT, true),
    ];
    let bigs: Vec<Bitmap> = sizes
        .iter()
        .map(|&(w, h, _)| Bitmap {
            width: w as u16,
            height: h as u16,
            pixels: vec![2; w * h],
        })
        .collect();
    for (big, &(w, h, want)) in bigs.iter().zip(&sizes) {
        let got = image((Some(big), None), src(Kind::Bitmap, BITMAP_PAPER, 0));
        assert_eq!(got, want.then_some((w, h)), "{w} x {h}");
    }
}
