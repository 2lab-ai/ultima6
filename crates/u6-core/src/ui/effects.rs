//! Map-window effects of hunk 18: the quake, the view reset and the full view redraw.

use super::seams;
use crate::combat::present::capture_window;
use crate::engine::Io;
use crate::geom::{MAP_X, MAP_Y, SHOWN};
use crate::state::State;

/// Engine milliseconds of one shake (its copy, 0x1344a8, and the next shake's draws). Not
/// measured (no quake happens in the oracle's start fight), so one PAL frame.
pub const SHAKE_MS: u16 = crate::combat::present::FRAME_MS;

/// The shaken window's inset from the view's top left: one cell (Original (16, 16)).
const SHAKE_INSET: i16 = 0x10;

/// Original: 0x133a1a (ui: shake the map window `times` times by up to `amp` pixels: each time
/// a random offset in 16 ± amp on both axes, and the 0x90-pixel window copied from there back
/// to (16, 16); then refresh the view)
///
/// The window is the shown map less a cell ([`SHOWN`]: 0x90 × 0x90 in Original, 560 × 304 in
/// Wide), its offsets from the view's top left ([`MAP_X`], [`MAP_Y`]). Two RNG draws per shake,
/// x first, as the original. The callers are synchronous (the end of
/// turn 0x10348c, the view refresh 0x102ef0, Tremor), and the engine publishes only the last
/// present of a poll, so each shake and then the refreshed window are recorded and shown by
/// the next playback ([`crate::combat::play_effects`]), each shake after [`SHAKE_MS`].
pub fn quake(s: &mut State, io: &Io, times: i16, amp: i16) {
    seams::present_map(s, io, 0);
    for _ in 0..times.max(0) {
        let (lo, hi) = ((SHAKE_INSET - amp) as u16, (amp + SHAKE_INSET) as u16);
        let ox = s.rng.range(lo, hi) as i16;
        let oy = s.rng.range(lo, hi) as i16;
        let [x0, y0, x1, y1, to_x, to_y] = shake_copy(ox, oy);
        io.with_buffers(|back, front| {
            seams::copy_rect(back, front, x0, y0, x1, y1, to_x, to_y);
        });
        io.present();
        capture_window(s, SHAKE_MS);
    }
    seams::refresh_view(s, io);
    capture_window(s, crate::combat::present::REFRESH_MS);
}

/// The copy of one shake (0x151fe6's arguments: x0, y0, x1, y1 inclusive, then the destination)
/// for the random offsets (`ox`, `oy`) from the view's top left ([`MAP_X`], [`MAP_Y`]): the
/// shown map less a cell ([`SHOWN`]) from there, to one cell in from the view's top left.
fn shake_copy(ox: i16, oy: i16) -> [i16; 6] {
    let (x0, y0, x1, y1) = SHOWN;
    let (w, h) = (
        (x1 - x0) as i16 + 1 - SHAKE_INSET,
        (y1 - y0) as i16 + 1 - SHAKE_INSET,
    );
    let (mx, my) = (MAP_X as i16, MAP_Y as i16);
    let (x, y) = (mx + ox, my + oy);
    [x, y, x + w, y + h, mx + SHAKE_INSET, my + SHAKE_INSET]
}

/// Original: 0x133a96 (ui: forget the map window's caches: every cell's tile 0xff (unknown),
/// every cell's object list empty, the node list free again (node n links to n + 1))
pub fn reset_window(s: &mut State) {
    let d = &mut s.objects.draw;
    d.base.iter_mut().for_each(|t| *t = 0xff);
    for (n, next) in d.node_next.iter_mut().enumerate() {
        *next = (n as u8).wrapping_add(1);
    }
    d.cell.iter_mut().for_each(|c| *c = 0);
}

/// Original: 0x133b0e (ui: redraw the view from scratch: the caches forgotten (0x133a96), no
/// view rebuild pending (A4-0x6a8e), the next present dissolving (A4-0x6578 = 1), the map
/// window rendered and presented (0x102c90 with 1), the tile cache (0x108d10) and the view
/// (0x109e74) rebuilt)
pub fn redraw_view(s: &mut State, io: &Io) {
    reset_window(s);
    s.map.view_dirty = 0;
    s.gfx.dissolve = 1;
    seams::present_map(s, io, 1);
    crate::map::region::rebuild_window(s);
    crate::map::view::update_view(s);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rng;

    #[test]
    fn reset_window_forgets_tiles_and_lists() {
        let mut s = State::empty();
        s.objects.draw.base[7] = 0x123;
        s.objects.draw.cell[7] = 9;
        s.objects.draw.node_next[3] = 0;
        reset_window(&mut s);
        let d = &s.objects.draw;
        assert!(d.base.iter().all(|&t| t == 0xff));
        assert!(d.cell.iter().all(|&c| c == 0));
        assert_eq!(
            (d.node_next[0], d.node_next[3], d.node_next[255]),
            (1, 4, 0)
        );
    }

    #[test]
    fn quake_draws_two_numbers_per_shake() {
        let mut s = State::empty();
        s.rng = Rng::seeded(7);
        let mut expect = s.rng;
        let mut e = crate::Engine::with_game(s, crate::Files::default(), |st, io| async move {
            quake(&mut st.borrow_mut(), &io, 3, 4);
        });
        e.tick(&[], 0);
        for _ in 0..6 {
            let v = expect.range(12, 20);
            assert!((12..=20).contains(&v));
        }
        assert_eq!(e.state().rng, expect);
    }

    /// A state with the test text layer whose map window is shown (0x102ef0 redraws it).
    fn shaking() -> State {
        let mut s = State::empty();
        s.ui = crate::main_loop::test_support::test_ui();
        s.gfx.window_shown = 1;
        s
    }

    #[test]
    fn quake_records_each_shake_then_the_window() {
        // The original presents every shake (0x1344a8 copies to the screen); the engine only
        // publishes the last present of a poll, so each is recorded for playback, each after its
        // time, and the refreshed window after them. The whole map window, as combat records
        // it, is recorded once more after the quake for reference.
        let mut e =
            crate::Engine::with_game(shaking(), crate::Files::default(), |st, io| async move {
                quake(&mut st.borrow_mut(), &io, 3, 4);
                capture_window(&mut st.borrow_mut(), 0);
            });
        e.tick(&[], 0);
        let s = e.state();
        let (whole, quake) = s.combat.effects.split_last().expect("recorded");
        let costs: Vec<u16> = quake.iter().map(|f| f.cost).collect();
        assert_eq!(
            costs,
            [
                SHAKE_MS,
                SHAKE_MS,
                SHAKE_MS,
                crate::combat::present::REFRESH_MS
            ],
            "three shakes, then the window"
        );
        assert!(
            quake
                .iter()
                .all(|f| (f.x, f.y, f.w, f.h) == (whole.x, whole.y, whole.w, whole.h)),
            "the whole map window each time"
        );
        #[cfg(not(feature = "wide"))]
        assert_eq!((whole.w, whole.h), (176, 176), "the 11 × 11 window");
        // Each shake's copy, by the contract of 0x133a1a: in Original the 0x90-pixel window from
        // (16 ± amp, 16 ± amp) to (0x10, 0x10); in Wide the shown map's 36 × 20 cells less the
        // same cell of margin (16 pixels), from the same offsets past (MAP_X, MAP_Y) to one cell
        // in from there.
        let (w, h, to): (i16, i16, (i16, i16)) = if cfg!(feature = "wide") {
            let (mx, my) = (crate::geom::MAP_X as i16, crate::geom::MAP_Y as i16);
            let cells = (
                crate::geom::VIEW_W as i16 - 1,
                crate::geom::VIEW_H as i16 - 1,
            );
            (cells.0 * 16 - 16, cells.1 * 16 - 16, (mx + 16, my + 16))
        } else {
            (0x90, 0x90, (0x10, 0x10))
        };
        #[cfg(feature = "wide")]
        assert_eq!((w, h, to), (560, 304, (336, 16)), "spec 13 Geometry");
        for (ox, oy) in [(12, 20), (16, 16), (20, 12)] {
            let (x, y) = (to.0 - 16 + ox, to.1 - 16 + oy);
            assert_eq!(
                shake_copy(ox, oy),
                [x, y, x + w, y + h, to.0, to.1],
                "offsets ({ox}, {oy})"
            );
        }
    }

    #[test]
    fn a_quake_in_a_key_wait_is_shown_before_the_key() {
        // A shake request (A4-0x7afc) that the idle refresh of the key wait (0x105c8e calls
        // 0x102ef0) carries out: its frames are shown while the key is awaited, none are left
        // for after the next command.
        let mut s = shaking();
        s.ui.input.idle_refresh = true;
        s.gfx.panel_redraw = 2;
        let got = std::rc::Rc::new(std::cell::Cell::new(None));
        let out = std::rc::Rc::clone(&got);
        let mut e =
            crate::Engine::with_game(s, crate::Files::default(), move |st, io| async move {
                let k = crate::ui::wait::read_key(&st, &io).await;
                let s = st.borrow();
                out.set(Some((k, s.combat.shown, s.combat.effects.len())));
            });
        e.tick(&[], 16);
        for _ in 0..30 {
            e.tick(&[], 16);
        }
        e.tick(&[crate::InputEvent::Key(crate::Key::Char('a'))], 16);
        for _ in 0..4 {
            e.tick(&[], 16);
        }
        assert_eq!(
            got.get(),
            Some((0x41, 3, 0)),
            "(key, frames shown, frames left): two shakes and the window"
        );
    }

    #[test]
    fn a_quake_in_a_more_prompt_is_shown_in_the_prompt() {
        // The same request carried out by the idle refresh of a more prompt while three lines
        // are still queued (a flush that pages after two): the shakes and the window are shown
        // in the prompt, as the original draws them there, and the queued lines still wait for
        // the more key.
        use crate::main_loop::{PrintArg, flush_prints, queue_print};
        let mut s = shaking();
        s.ui.input.idle_refresh = true;
        s.gfx.panel_redraw = 2;
        s.ui.text.win_mut().flags |= crate::ui::text::PAGED;
        s.ui.text.more_rows = 3;
        for i in 0..6 {
            queue_print(&mut s, b"q%d\n".to_vec(), vec![PrintArg::Word(i)]);
        }
        let mut e =
            crate::Engine::with_game(s, crate::Files::default(), move |st, io| async move {
                flush_prints(&st, &io).await;
            });
        for _ in 0..30 {
            e.tick(&[], 16);
        }
        assert!(!e.finished(), "the more prompt waits");
        let later = |l: &String| ["q3", "q4", "q5"].contains(&l.as_str());
        {
            let s = e.state();
            assert_eq!(
                (
                    s.combat.shown,
                    s.combat.effects.len(),
                    s.main.pending_print.len()
                ),
                (3, 0, 3),
                "(frames shown, frames left, lines queued) in the prompt"
            );
            assert!(!s.ui.messages.iter().any(later), "the queue waits");
        }
        e.tick(&[crate::InputEvent::Key(crate::Key::Space)], 16);
        for _ in 0..10 {
            e.tick(&[], 16);
        }
        assert!(e.finished(), "the flush ended after the more key");
        let s = e.state();
        assert_eq!(
            (
                s.combat.shown,
                s.ui.messages.iter().filter(|l| later(l)).count()
            ),
            (3, 3),
            "nothing shown again, the queued lines printed"
        );
    }
}
