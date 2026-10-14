//! Map-window effects of hunk 18 that the milestone reaches.

use super::seams;
use crate::engine::Io;
use crate::state::State;

/// Original: 0x133a1a (ui: shake the map window `times` times by up to `amp` pixels: each time
/// a random offset in 16 ± amp on both axes, and the 0x90-pixel window copied from there back
/// to (16, 16); then refresh the view)
///
/// Two RNG draws per shake, x first, as the original.
pub fn quake(s: &mut State, io: &Io, times: i16, amp: i16) {
    seams::present_map(s, 0);
    for _ in 0..times.max(0) {
        let (lo, hi) = ((0x10 - amp) as u16, (amp + 0x10) as u16);
        let dx = s.rng.range(lo, hi) as i16;
        let dy = s.rng.range(lo, hi) as i16;
        io.draw(|fb| seams::copy_rect(fb, dx, dy, dx + 0x90, dy + 0x90, 0x10, 0x10));
    }
    seams::refresh_view(s);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rng;

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
