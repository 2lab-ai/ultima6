//! Map-window effects of hunk 18: the quake, the view reset and the full view redraw.

use super::seams;
use crate::engine::Io;
use crate::state::State;

/// Original: 0x133a1a (ui: shake the map window `times` times by up to `amp` pixels: each time
/// a random offset in 16 ± amp on both axes, and the 0x90-pixel window copied from there back
/// to (16, 16); then refresh the view)
///
/// Two RNG draws per shake, x first, as the original.
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
    }
    seams::refresh_view(s, io);
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
}
