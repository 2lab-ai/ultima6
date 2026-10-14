//! Map-window effects of hunk 18: the quake, the view reset and the full view redraw.

use std::cell::RefCell;

use super::seams;
use crate::combat::present::capture_window;
use crate::engine::Io;
use crate::state::State;

/// Display frames a shake stays on the screen. The original shows each for as long as its
/// copy (0x1344a8) and the next shake's draws take; that time is not measured, so one PAL frame.
pub const SHAKE_HOLD: u8 = 1;

/// Original: 0x133a1a (ui: shake the map window `times` times by up to `amp` pixels: each time
/// a random offset in 16 ± amp on both axes, and the 0x90-pixel window copied from there back
/// to (16, 16); then refresh the view)
///
/// Two RNG draws per shake, x first, as the original. The callers are synchronous (the end of
/// turn 0x10348c, the view refresh 0x102ef0, Tremor), and the engine publishes only the last
/// present of a poll, so each shake and then the refreshed window are recorded and shown by
/// the next playback ([`crate::combat::play_effects`]), each shake for [`SHAKE_HOLD`] frames.
pub fn quake(s: &mut State, io: &Io, times: i16, amp: i16) {
    seams::present_map(s, io, 0);
    for _ in 0..times.max(0) {
        let (lo, hi) = ((0x10 - amp) as u16, (amp + 0x10) as u16);
        let dx = s.rng.range(lo, hi) as i16;
        let dy = s.rng.range(lo, hi) as i16;
        io.with_buffers(|back, front| {
            seams::copy_rect(back, front, dx, dy, dx + 0x90, dy + 0x90, 0x10, 0x10);
        });
        io.present();
        capture_window(s, SHAKE_HOLD);
    }
    seams::refresh_view(s, io);
    capture_window(s, 0);
}

/// Port: frames that synchronous code recorded inside a key wait (a shake request carried out
/// by the idle refresh 0x102ef0 of 0x105c8e) are shown before the wait goes on, not after the
/// next command. Left for a later playback while lines are queued (a print's more prompt waits
/// here too, and playing would print the rest of its queue inside it) or Attack's newline is
/// held ([`crate::combat::CombatState::swallow`], which the playback hands on).
pub async fn show_recorded(state: &RefCell<State>, io: &Io) {
    let ready = {
        let s = state.borrow();
        !s.combat.effects.is_empty() && s.main.pending_print.is_empty() && !s.combat.swallow
    };
    if ready {
        crate::combat::play_effects(state, io).await;
    }
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
        // publishes the last present of a poll, so each is recorded for playback, one display
        // frame each, and the refreshed window after them.
        let mut e =
            crate::Engine::with_game(shaking(), crate::Files::default(), |st, io| async move {
                quake(&mut st.borrow_mut(), &io, 3, 4);
            });
        e.tick(&[], 0);
        let s = e.state();
        let holds: Vec<u8> = s.combat.effects.iter().map(|f| f.hold).collect();
        assert_eq!(holds, [1, 1, 1, 0], "three shakes, then the window");
        assert!(
            s.combat.effects.iter().all(|f| (f.w, f.h) == (176, 176)),
            "the whole map window each time"
        );
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
        for _ in 0..6 {
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
}
