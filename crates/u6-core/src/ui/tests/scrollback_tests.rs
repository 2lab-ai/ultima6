//! Spec 23: the message window's history, on the synthetic font (glyph `c` = eight rows of `c`)
//! and tables. The window is the record main sets up, so the tests hold in both builds.

use std::collections::BTreeMap;

use u6_render::{BITMAP_PAPER, Src};

use super::*;
use crate::engine::{POINTER_LEFT, POINTER_RIGHT};
use crate::main_loop::Program;
use crate::ui::scrollback::{self, CellKind, HISTORY, MIN_THUMB};
use crate::ui::text::Paper;

/// The message window's first and last frame pixels: x 176..312, y 112..192.
const X: std::ops::Range<usize> = 176..312;
const Y: std::ops::Range<usize> = 112..192;
/// A pixel inside the window, and one on the map.
const IN: (i16, i16) = (200, 150);
const MAP: (i16, i16) = (40, 40);
/// The bar's place: the window's last cell column, x 304..312.
const BAR_X: i16 = 310;

/// The synthetic state with `game` running.
fn game_state() -> State {
    let mut s = state();
    s.main.program = Program::Game;
    s
}

/// A paper all [`PAPER`] but one pixel of colour 7 inside the window's last row (x 200, y 190),
/// and the bitmap it is drawn from.
fn speck_paper() -> (Paper, u6_formats::bitmap::Bitmap) {
    let mut pixels = vec![PAPER; 320 * 200];
    pixels[190 * 320 + 200] = 7;
    let bmp = u6_formats::bitmap::Bitmap {
        width: 320,
        height: 200,
        pixels,
    };
    let identity: [u8; 256] = std::array::from_fn(|i| i as u8);
    (Paper::drawn(&identity, &bmp), bmp)
}

/// The game future [`talker`] makes.
type Talk = std::pin::Pin<Box<dyn Future<Output = ()>>>;

/// A game that draws the paper (when the state has one), prints `l0`, `l1`, … one line per frame
/// (`n` of them, each presented), then at each key it takes prints `k0`, `k1`, … the same way;
/// with `tick_ms` > 0 it also prints `t0`, `t1`, … every `tick_ms` while it waits.
fn talker(n: u16, tick_ms: u32) -> impl FnOnce(Rc<RefCell<State>>, Rc<Io>) -> Talk {
    move |st, io| {
        Box::pin(async move {
            if st.borrow().ui.paper.is_some() {
                let (_, bmp) = speck_paper();
                let identity: [u8; 256] = std::array::from_fn(|i| i as u8);
                io.draw(|fb| crate::gfx::blit::blit_bmp(&identity, fb, &bmp, 0, 0, BITMAP_PAPER));
            }
            for i in 0..n {
                printf(&st, &io, b"l%d\n", &[Arg::Word(i)]).await;
                io.present();
                io.vblank().await;
            }
            let (mut k, mut t) = (0u16, 0u16);
            loop {
                if tick_ms > 0 {
                    io.sleep(tick_ms).await;
                    if !io.key_ready() {
                        printf(&st, &io, b"t%d\n", &[Arg::Word(t)]).await;
                        t += 1;
                        io.present();
                        continue;
                    }
                }
                let _ = io.next_key().await;
                printf(&st, &io, b"k%d\n", &[Arg::Word(k)]).await;
                k += 1;
                io.present();
            }
        })
    }
}

/// An engine running [`talker`] on `s`, ticked until its first `n` lines are published.
fn engine(s: State, n: u16, tick_ms: u32, src: bool) -> Engine {
    let mut e = Engine::with_game(s, Files::default(), talker(n, tick_ms));
    if src {
        e.enable_src_plane();
    }
    for _ in 0..usize::from(n) + 4 {
        e.tick(&[], 16);
    }
    e
}

/// The text of screen row `row` of the message window in `fb` (the ink's cells, trimmed), less
/// its last cell, where the bar is drawn.
fn text_at(fb: &Framebuffer, row: usize) -> String {
    row_text(fb, 22..38, row, INK).trim_end().to_string()
}

/// Whether every pixel row of screen cell (col, row) is `code`'s glyph row (the synthetic font:
/// each row the code) in ink `fg` over the paper.
fn whole_glyph(fb: &Framebuffer, (col, row): (usize, usize), code: u8) -> bool {
    (0..8).all(|dy| {
        (0..8).all(|dx| {
            let want = if code & (0x80 >> dx) != 0 { INK } else { PAPER };
            fb.get(col * 8 + dx, row * 8 + dy) == Some(want)
        })
    })
}

/// The window's rows of `fb` as shown, top first.
fn shown_rows(fb: &Framebuffer) -> Vec<String> {
    (14..24).map(|r| text_at(fb, r)).collect()
}

/// The pixels (colour and source) of screen row `row` of the message window in `fb`.
fn row_pixels(fb: &Framebuffer, row: usize) -> Vec<(u8, Src)> {
    let mut out = Vec::new();
    for y in row * 8..row * 8 + 8 {
        for x in X {
            let src = fb.src_at(x, y).unwrap_or(Src::CLASSIC);
            out.push((fb.get(x, y).unwrap_or(0), src));
        }
    }
    out
}

/// Pixel (colour and source) equality of the whole frames.
fn same_frame(a: &Framebuffer, b: &Framebuffer) -> bool {
    a.pixels() == b.pixels() && a.src() == b.src()
}

/// The text of a kept line's cells.
fn line_text(l: &scrollback::Line) -> String {
    let s: String = l
        .cells
        .iter()
        .map(|c| match c.kind {
            CellKind::Blank => ' ',
            _ => char::from(c.code as u8),
        })
        .collect();
    s.trim_end().to_string()
}

/// The rows (y) of the window where the bar's thumb is drawn: every pixel of x 309..312 the ink.
fn thumb_rows(fb: &Framebuffer) -> Vec<usize> {
    Y.filter(|&y| (309..312).all(|x| fb.get(x, y) == Some(INK)))
        .collect()
}

/// Whether any pixel of the bar's place (x 309..312) is the ink.
fn bar_drawn(fb: &Framebuffer) -> bool {
    Y.clone()
        .any(|y| (309..312).any(|x| fb.get(x, y) == Some(INK)))
}

/// The pointer at (x, y) with `buttons`.
fn at((x, y): (i16, i16), buttons: u8) -> InputEvent {
    InputEvent::Pointer { x, y, buttons }
}

#[test]
fn history_keeps_the_scrolled_off_lines_in_order_up_to_the_bound() {
    let n = (HISTORY + 30) as u16;
    let e = engine(game_state(), n, 0, false);
    let s = e.state();
    let sb = &s.ui.scrollback;
    // Ten rows: n lines and the cursor's row leave n - 9 lines scrolled off.
    let off = usize::from(n) - 9;
    assert_eq!(sb.total(), off as u64, "every line that scrolled off");
    assert_eq!(sb.lines().len(), HISTORY, "at most HISTORY kept");
    let want: Vec<String> = (off - HISTORY..off).map(|i| format!("l{i}")).collect();
    let got: Vec<String> = sb.lines().iter().map(line_text).collect();
    assert_eq!(got, want, "the newest HISTORY, oldest first");
    assert!(
        sb.lines()
            .iter()
            .all(|l| l.cells.len() == 17 && l.bg == PAPER && l.patch.is_empty()),
        "each a row of the window's cells, drawn as shown"
    );
    let bytes = sb.heap_bytes();
    assert!(bytes < 150_000, "about 0.1 MB at the bound: {bytes}");
    // The live rows are the shadow's.
    let rows: Vec<String> = sb.rows().iter().map(line_text).collect();
    assert_eq!(rows, shown_rows(e.frame()));
}

#[test]
fn a_kept_line_keeps_its_keyword_spans_and_colours() {
    let e = run(game_state(), &[], 4, |st, io| async move {
        printf(&st, &io, b"say @name. ok\n", &[]).await;
        for _ in 0..10 {
            printf(&st, &io, b"x\n", &[]).await;
        }
        io.present();
    });
    let s = e.state();
    let first = &s.ui.scrollback.lines()[0];
    assert_eq!(line_text(first), "say name. ok");
    assert_eq!(
        first.spans,
        [(4, 8)],
        "the keyword's cells, drawn in the alert ink"
    );
    assert!(first.cells[4..8].iter().all(|c| c.fg == ALERT));
    assert!(first.cells[..4].iter().all(|c| c.fg == INK));
}

#[test]
fn the_wheel_shows_a_kept_line_as_it_looked_before_it_scrolled_off() {
    let mut s = game_state();
    s.ui.paper = Some(speck_paper().0);
    let n = 40u16;
    let mut e = Engine::with_game(s, Files::default(), talker(n, 0));
    e.enable_src_plane();
    // Each published frame: every row of the window by its text, as it showed there.
    let mut seen: BTreeMap<(String, usize), Vec<(u8, Src)>> = BTreeMap::new();
    for _ in 0..usize::from(n) + 4 {
        e.tick(&[], 16);
        let fb = e.published_frame();
        for row in 14..24 {
            let t = text_at(&fb, row);
            if !t.is_empty() {
                seen.insert((t, row), row_pixels(&fb, row));
            }
        }
    }
    let kept = e.state().ui.scrollback.lines().len();
    assert_eq!(kept, usize::from(n) - 9);
    assert!(
        e.state()
            .ui
            .scrollback
            .lines()
            .iter()
            .any(|l| !l.patch.is_empty()),
        "the paper's speck went off with a line, as a patch"
    );
    let (mut compared, mut lines) = (0, std::collections::BTreeSet::new());
    for k in 1..=kept {
        assert!(e.wheel(IN.0, IN.1, -1), "over the window");
        assert_eq!(e.history_offset(), k);
        let fb = e.frame();
        for (r, row) in (14..24).enumerate() {
            let want = format!("l{}", kept - k + r);
            let t = text_at(fb, row);
            assert_eq!(t, want, "offset {k}, row {r}");
            let Some(was) = seen.get(&(t.clone(), row)) else {
                continue;
            };
            let mut now = row_pixels(fb, row);
            let mut was = was.clone();
            // The bar's columns (x 309..312 of each row) are compared apart.
            for p in [&mut now, &mut was] {
                for y in 0..8 {
                    for x in 133..136 {
                        p[y * 136 + x] = (0, Src::CLASSIC);
                    }
                }
            }
            assert!(
                now == was,
                "offset {k}: {want} at row {r} as it showed there"
            );
            compared += 1;
            lines.insert(want);
        }
        // The HD frame's view is the shown frame (no pointer sprite here).
        assert!(e.with_view_frame(|v| same_frame(v, e.frame())));
    }
    // Every line in view but the newest at the window's last row: the live frames show a line
    // there only before its newline, which scrolls it first.
    assert!(
        lines.len() == kept + 8,
        "every line compared: {}",
        lines.len()
    );
    assert!(
        compared == kept * 9,
        "rows 0..9 at every offset: {compared}"
    );
    // Past the oldest the wheel stops there.
    assert!(e.wheel(IN.0, IN.1, -5));
    assert_eq!(e.history_offset(), kept);
}

#[test]
fn with_no_history_view_the_frame_is_the_frame_without_the_feature() {
    let mut s = game_state();
    s.ui.paper = Some(speck_paper().0);
    let n = 30u16;
    let mut e = Engine::with_game(s.clone(), Files::default(), talker(n, 0));
    e.enable_src_plane();
    for _ in 0..usize::from(n) + 4 {
        e.tick(&[], 16);
        assert!(
            same_frame(e.frame(), &e.published_frame()),
            "live: as published"
        );
        assert!(e.with_view_frame(|v| same_frame(v, &e.published_frame())));
    }
    // Back and forward again: the live frame once more, the bar gone.
    assert!(e.wheel(IN.0, IN.1, -3));
    assert!(
        !same_frame(e.frame(), &e.published_frame()),
        "scrolled back"
    );
    assert!(bar_drawn(e.frame()));
    assert!(e.wheel(IN.0, IN.1, 3));
    assert_eq!(e.history_offset(), 0);
    assert!(same_frame(e.frame(), &e.published_frame()));
    assert!(!bar_drawn(e.frame()));
    e.tick(&[], 16);
    assert!(same_frame(e.frame(), &e.published_frame()));
    // The game's frames do not see the feature: the published frame and the state are those of a
    // run that never scrolled back.
    let mut plain = Engine::with_game(s, Files::default(), talker(n, 0));
    plain.enable_src_plane();
    for _ in 0..usize::from(n) + 5 {
        plain.tick(&[], 16);
    }
    assert!(same_frame(&plain.published_frame(), &e.published_frame()));
    assert!(*plain.state() == *e.state());
}

#[test]
fn the_thumb_size_and_place_are_the_history_fraction() {
    assert_eq!(scrollback::thumb(80, 10, 0, 0), (0, 80));
    // 30 kept + 10 shown: a quarter of the track.
    assert_eq!(
        scrollback::thumb(80, 10, 30, 30),
        (0, 20),
        "at the oldest, the top"
    );
    assert_eq!(scrollback::thumb(80, 10, 30, 1), (58, 20));
    assert_eq!(scrollback::thumb(80, 10, 30, 15), (30, 20));
    // Long histories keep a cell's height.
    assert_eq!(scrollback::thumb(80, 10, 490, 245).1, MIN_THUMB);
    let mut e = engine(game_state(), 39, 0, false);
    let kept = e.state().ui.scrollback.lines().len();
    assert_eq!(kept, 30);
    for k in [1, 7, 15, 30] {
        let dy = k as i32 - e.history_offset() as i32;
        assert!(e.wheel(IN.0, IN.1, -dy));
        let (top, len) = scrollback::thumb(80, 10, kept, k);
        let want: Vec<usize> = (112 + top..112 + top + len).collect();
        assert_eq!(thumb_rows(e.frame()), want, "offset {k}");
    }
}

#[test]
fn the_bar_never_covers_a_letter() {
    let mut e = Engine::with_game(game_state(), Files::default(), |st, io| async move {
        for i in 0..30u16 {
            // Every other line fills the window's 17 cells.
            let line: &[u8] = if i % 2 == 0 {
                b"aaaaaaaaaaaaaaaaa"
            } else {
                b"b\n"
            };
            printf(&st, &io, line, &[]).await;
            io.present();
            io.vblank().await;
        }
        loop {
            io.vblank().await;
        }
    });
    for _ in 0..40 {
        e.tick(&[], 16);
    }
    assert!(e.wheel(IN.0, IN.1, -4));
    let fb = e.frame();
    let (mut letters, mut bars) = (0, 0);
    for row in 14..24 {
        let full = cell(fb, 38, row, INK) == b'a';
        let bar = (row * 8..row * 8 + 8).any(|y| (309..312).any(|x| fb.get(x, y) == Some(INK)))
            && cell(fb, 38, row, INK) != b'a';
        if full {
            letters += 1;
            assert!(
                whole_glyph(fb, (38, row), b'a'),
                "row {row}: the letter whole"
            );
            assert_eq!(row_text(fb, 22..39, row, INK), "a".repeat(17));
        }
        bars += usize::from(bar);
    }
    assert!(
        letters >= 4 && bars >= 4,
        "{letters} letter rows, {bars} bar rows"
    );
}

#[test]
fn a_key_returns_the_view_to_the_live_bottom() {
    let mut e = engine(game_state(), 25, 0, true);
    let live = shown_rows(e.frame());
    assert!(e.wheel(IN.0, IN.1, -6));
    assert_ne!(shown_rows(e.frame()), live);
    e.tick(&[InputEvent::Key(Key::Char('q'))], 16);
    assert_eq!(e.history_offset(), 0, "the key leaves the history");
    e.tick(&[], 16);
    assert!(same_frame(e.frame(), &e.published_frame()));
    assert_eq!(text_at(e.frame(), 22), "k0", "and reached the game");
    // A release and a move without a button stay in the history.
    assert!(e.wheel(IN.0, IN.1, -2));
    e.tick(&[InputEvent::Release(Key::Char('q')), at(MAP, 0)], 16);
    assert_eq!(e.history_offset(), 2);
    // Typed text leaves it.
    e.tick(&[InputEvent::Text('a')], 16);
    assert_eq!(e.history_offset(), 0);
}

#[test]
fn a_press_off_the_bar_returns_and_reaches_the_game() {
    let mut e = engine(game_state(), 25, 0, false);
    assert!(e.wheel(IN.0, IN.1, -3));
    e.tick(&[at(MAP, POINTER_LEFT)], 16);
    assert_eq!(e.history_offset(), 0);
    assert_eq!(
        e.io().handler_pointer().buttons,
        POINTER_LEFT,
        "the game's press"
    );
    e.tick(&[at(MAP, 0)], 16);
    // A press inside the window off the bar too.
    assert!(e.wheel(IN.0, IN.1, -3));
    e.tick(&[at(IN, POINTER_RIGHT)], 16);
    assert_eq!(e.history_offset(), 0);
    assert_eq!(e.io().handler_pointer().buttons, POINTER_RIGHT);
}

#[test]
fn the_wheel_over_anything_but_the_window_does_nothing() {
    let mut e = engine(game_state(), 25, 0, false);
    let before = e.frame_version();
    assert!(!e.wheel(MAP.0, MAP.1, -3), "on the map");
    assert!(!e.wheel(175, 150, -3), "beside the window");
    assert!(!e.wheel(200, 192, -3), "below it");
    assert_eq!(e.history_offset(), 0);
    assert_eq!(e.frame_version(), before, "nothing composed");
    // Not in `game` (the launcher's `u`): the window is not the game's.
    let mut s = game_state();
    s.main.program = Program::U;
    let mut u = engine(s, 25, 0, false);
    assert!(!u.wheel(IN.0, IN.1, -3));
    assert_eq!(u.history_offset(), 0);
    // No history yet: taken, and nothing to show.
    let mut fresh = engine(game_state(), 3, 0, false);
    assert!(fresh.wheel(IN.0, IN.1, -3));
    assert_eq!(fresh.history_offset(), 0);
}

#[test]
fn dragging_the_thumb_and_pressing_the_track_scroll() {
    let mut e = engine(game_state(), 39, 0, false);
    let kept = 30;
    assert!(e.wheel(IN.0, IN.1, -1));
    let (top, len) = scrollback::thumb(80, 10, kept, 1);
    // Press on the track above the thumb: a window (10 rows) older.
    e.tick(&[at((BAR_X, 112 + 2), POINTER_LEFT)], 16);
    assert_eq!(e.history_offset(), 11);
    e.tick(&[at((BAR_X, 112 + 2), 0)], 16);
    // Below it: 10 newer, at most to the live bottom.
    let (top2, len2) = scrollback::thumb(80, 10, kept, 11);
    assert!(top2 + len2 < 79);
    e.tick(&[at((BAR_X, 191), POINTER_LEFT), at((BAR_X, 191), 0)], 16);
    assert_eq!(e.history_offset(), 1);
    let _ = (top, len);
    // Grab the thumb at its middle and drag it to the top: the oldest line.
    let (top, len) = scrollback::thumb(80, 10, kept, 1);
    let grab = 112 + (top + len / 2) as i16;
    e.tick(&[at((BAR_X, grab), POINTER_LEFT)], 16);
    assert_eq!(e.history_offset(), 1, "a grab alone moves nothing");
    e.tick(&[at((BAR_X, 100), POINTER_LEFT)], 16);
    assert_eq!(e.history_offset(), kept, "the thumb at the top");
    // Half way down the track.
    let span = 80 - len;
    let mid = 112 + (len / 2 + span / 2) as i16;
    e.tick(&[at((BAR_X, mid), POINTER_LEFT)], 16);
    let want = kept - (span / 2 * kept + span / 2) / span;
    assert_eq!(e.history_offset(), want, "half way");
    // The drag holds off the bar too, and ends with the release.
    e.tick(&[at((40, 300), POINTER_LEFT), at((40, 300), 0)], 16);
    assert_eq!(e.history_offset(), 0, "dragged past the bottom: live");
}

#[test]
fn a_bar_press_never_reaches_the_game() {
    let mut e = engine(game_state(), 39, 0, false);
    assert!(e.wheel(IN.0, IN.1, -5));
    e.tick(&[at((BAR_X, 120), POINTER_LEFT)], 16);
    assert_eq!(
        e.io().handler_pointer().buttons,
        0,
        "the press is the bar's"
    );
    e.tick(&[at((BAR_X, 140), POINTER_LEFT | POINTER_RIGHT)], 16);
    assert_eq!(e.io().handler_pointer().buttons, 0);
    assert_eq!(
        e.io().handler_pointer().y,
        140,
        "the sprite follows the pointer"
    );
    e.tick(&[at((BAR_X, 140), 0)], 16);
    assert!(
        e.history_offset() > 0,
        "the release leaves the view as it is"
    );
    // The next press is the game's again.
    e.tick(&[at(MAP, POINTER_LEFT)], 16);
    assert_eq!(e.io().handler_pointer().buttons, POINTER_LEFT);
}

#[test]
fn new_text_while_scrolled_back_keeps_the_view() {
    let mut e = engine(game_state(), 25, 200, false);
    assert!(e.wheel(IN.0, IN.1, -4));
    let rows = shown_rows(e.frame());
    let total = e.state().ui.scrollback.total();
    let bar = thumb_rows(e.frame());
    for _ in 0..60 {
        e.tick(&[], 16);
    }
    let grew = (e.state().ui.scrollback.total() - total) as usize;
    assert!(grew >= 3, "{grew} lines printed while scrolled back");
    assert_eq!(shown_rows(e.frame()), rows, "the same lines in view");
    assert_eq!(e.history_offset(), 4 + grew);
    assert_ne!(thumb_rows(e.frame()), bar, "the thumb moved");
    let s = e.state();
    let live: Vec<String> = s.ui.scrollback.rows().iter().map(line_text).collect();
    drop(s);
    assert!(e.wheel(IN.0, IN.1, 100));
    assert_eq!(shown_rows(e.frame()), live, "the wheel to the bottom: live");
}

#[test]
fn viewing_changes_no_state() {
    let s = game_state();
    let mut a = engine(s.clone(), 39, 0, true);
    let mut b = engine(s, 39, 0, true);
    assert!(a.wheel(IN.0, IN.1, -8));
    a.tick(&[at((BAR_X, 120), POINTER_LEFT)], 16);
    a.tick(&[at((BAR_X, 130), POINTER_LEFT)], 16);
    a.tick(&[at((BAR_X, 130), 0)], 16);
    b.tick(&[at((BAR_X, 120), 0)], 16);
    b.tick(&[at((BAR_X, 130), 0)], 16);
    b.tick(&[at((BAR_X, 130), 0)], 16);
    for e in [&mut a, &mut b] {
        e.tick(&[InputEvent::Key(Key::Char('z'))], 16);
        e.tick(&[], 16);
    }
    assert!(*a.state() == *b.state(), "the same game state");
    assert!(same_frame(&a.published_frame(), &b.published_frame()));
    assert!(
        same_frame(a.frame(), b.frame()),
        "and live again, the same frame"
    );
}

#[test]
fn a_window_over_the_message_window_clears_the_shadow() {
    let e = run(game_state(), &[], 6, |st, io| async move {
        for _ in 0..12 {
            printf(&st, &io, b"m\n", &[]).await;
        }
        let w = TextWindow::new(0, 0, 0x27, 0x18, INK, PAPER, text::SCROLL);
        let mut s = st.borrow_mut();
        io.draw(|fb| scrollback::scrolled(&mut s.ui, fb, &w));
        drop(s);
        io.present();
    });
    let s = e.state();
    assert!(s.ui.scrollback.rows().is_empty());
    assert_eq!(s.ui.scrollback.total(), 3, "the kept lines stay");
}

#[test]
fn the_hd_frame_shows_the_view_and_at_the_live_bottom_is_the_frame_without_it() {
    use crate::hd::HdView;
    use u6_render::Framebuffer;
    use u6_render::hd::Scale;
    let n = 30u16;
    let mut e = Engine::with_game(game_state(), Files::default(), talker(n, 0));
    e.enable_src_plane();
    // A palette of distinct colours (the synthetic state has none: every index black).
    e.io().draw(|fb| {
        for (i, c) in fb.palette.iter_mut().enumerate() {
            let v = i as u8;
            *c = u6_render::Rgb::new(v.wrapping_mul(9), 255 - v, 7);
        }
    });
    for _ in 0..usize::from(n) + 4 {
        e.tick(&[], 16);
    }
    let blank = Framebuffer::new();
    for scale in [Scale::X2, Scale::X4] {
        let mut hd = HdView::new();
        hd.set(true, scale);
        hd.present(Some(&e), &blank);
        let live = hd.rgba().to_vec();
        // A fresh view of the published frame alone composes the same live frame.
        let mut plain = HdView::new();
        plain.set(true, scale);
        plain.present(Some(&e), &blank);
        assert!(
            plain.rgba() == live.as_slice(),
            "live: the frame without the view"
        );
        assert!(e.wheel(IN.0, IN.1, -3));
        assert!(
            hd.present(Some(&e), &blank),
            "scrolled back: composed again"
        );
        let back = hd.rgba().to_vec();
        assert!(back != live, "the HD frame shows the history");
        // The same frame as an HD view of the view frame composed from nothing.
        let mut fresh = HdView::new();
        fresh.set(true, scale);
        fresh.present(Some(&e), &blank);
        assert!(
            fresh.rgba() == back.as_slice(),
            "the view frame's composition"
        );
        assert!(e.wheel(IN.0, IN.1, 3));
        assert_eq!(e.history_offset(), 0);
        hd.present(Some(&e), &blank);
        assert!(
            hd.rgba() == live.as_slice(),
            "back at the live bottom: the frame without it"
        );
    }
}
