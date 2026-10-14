//! The game's random number generator (hunk 30, `util`).
//!
//! State is one longword (image address 0x151ee0, initial value 0x00000001 in the image). Each
//! draw multiplies it by 0x10003 modulo 2^32 and returns the high word. `main()` seeds the low
//! word once from the low word of the C `time()` result (0x1351d8 → 0x1346f4 → `time(NULL)` →
//! 0x151f2e), so a run is reproducible exactly when the platform passes the same seed word.

/// The 32-bit generator state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rng {
    /// Original: 0x151ee0 (longword; high word at 0x151ee0, low word at 0x151ee2)
    state: u32,
}

impl Default for Rng {
    fn default() -> Self {
        Self::new()
    }
}

impl Rng {
    /// The value the image holds before `main()` seeds it.
    pub const INITIAL_STATE: u32 = 0x0000_0001;

    /// Unseeded generator (the image's initial state).
    pub const fn new() -> Self {
        Self {
            state: Self::INITIAL_STATE,
        }
    }

    /// A generator seeded the way `main()` does it.
    pub fn seeded(word: u16) -> Self {
        let mut rng = Self::new();
        rng.seed(word);
        rng
    }

    /// Original: 0x151f2e (seed: low word = seed | 1, high word kept)
    pub fn seed(&mut self, word: u16) {
        self.state = (self.state & 0xffff_0000) | u32::from(word | 1);
    }

    /// Current state longword (for save files and tests).
    pub const fn state(&self) -> u32 {
        self.state
    }

    /// Restores a state previously read with [`Rng::state`].
    pub fn set_state(&mut self, state: u32) {
        self.state = state;
    }

    /// Original: 0x151ee4 (random: state *= 0x10003, return the high word)
    ///
    /// The 68000 code computes `3 * s + (s << 16)` with 32-bit adds, which is the same product.
    pub fn next_word(&mut self) -> u16 {
        self.state = self.state.wrapping_mul(0x0001_0003);
        (self.state >> 16) as u16
    }

    /// Original: 0x151f06 (random number in [lo, hi])
    ///
    /// Bounds are unsigned words in either order (an unsigned compare decides which is the low
    /// one). `span = high - low + 1` is a 16-bit sum: when it wraps to 0 (the full 0..=0xffff
    /// range) the 68000 skips the `divu` and the result is the low bound, not a random value.
    /// Callers that pass signed values pass their bit patterns. Returns the low word of D0; the
    /// high word (the quotient) is not used by any caller in milestone 1.
    pub fn range(&mut self, a: u16, b: u16) -> u16 {
        let r = self.next_word();
        let (high, low) = if a < b { (b, a) } else { (a, b) };
        let span = high.wrapping_sub(low).wrapping_add(1);
        let offset = if span == 0 { 0 } else { r % span };
        low.wrapping_add(offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Hand-derived from s' = s * 0x10003 mod 2^32 = 3s + (s << 16):
    // 0x00000001 -> 0x00010003 -> 0x00060009 -> 0x001b001b -> 0x006c0051.
    #[test]
    fn unseeded_sequence_matches_hand_derivation() {
        let mut rng = Rng::new();
        let words: Vec<u16> = (0..4).map(|_| rng.next_word()).collect();
        assert_eq!(words, [0x0001, 0x0006, 0x001b, 0x006c]);
        assert_eq!(rng.state(), 0x006c_0051);
    }

    #[test]
    fn high_bits_wrap_like_the_68000() {
        let mut rng = Rng::new();
        rng.set_state(0xffff_ffff);
        // 3 * 0xffffffff + 0xffff0000 = 0xfffffffd + 0xffff0000 (mod 2^32) = 0xfffefffd.
        assert_eq!(rng.next_word(), 0xfffe);
        assert_eq!(rng.state(), 0xfffe_fffd);
    }

    #[test]
    fn seed_sets_the_low_word_odd_and_keeps_the_high_word() {
        assert_eq!(Rng::seeded(0).state(), 0x0000_0001);
        assert_eq!(Rng::seeded(0x1234).state(), 0x0000_1235);
        let mut rng = Rng::new();
        rng.set_state(0xabcd_0000);
        rng.seed(0xfffe);
        assert_eq!(rng.state(), 0xabcd_ffff);
    }

    #[test]
    fn range_follows_the_divu_path() {
        // Draws 1, 6, 27, 108 (see above) into a span of 6 starting at 1.
        let mut rng = Rng::new();
        let rolls: Vec<u16> = (0..4).map(|_| rng.range(1, 6)).collect();
        assert_eq!(rolls, [2, 1, 4, 1]);
        // Bound order does not matter.
        let mut swapped = Rng::new();
        let rolls2: Vec<u16> = (0..4).map(|_| swapped.range(6, 1)).collect();
        assert_eq!(rolls, rolls2);
    }

    #[test]
    fn range_edge_cases() {
        let mut rng = Rng::new();
        // Equal bounds: span 1.
        assert_eq!(rng.range(5, 5), 5);
        // Full range: span wraps to 0, divu skipped, result = low bound; one draw consumed.
        let before = rng.state();
        assert_eq!(rng.range(0, 0xffff), 0);
        assert_eq!(rng.state(), before.wrapping_mul(0x10003));
        // Signed -1..1 is unsigned 0xffff vs 1: span 0xffff, offset = r % 0xffff.
        let mut a = Rng::new();
        let r = {
            let mut b = Rng::new();
            b.next_word()
        };
        assert_eq!(a.range(0xffff, 1), 1u16.wrapping_add(r % 0xffff));
    }
}
