//! Port-only (spec 08 Design 1): what the text layer adds for Korean. Nothing here runs unless
//! the locale layer turns it on ([`super::TextState::wide`], [`super::TextState::language`]);
//! off and English, every byte reaches the escape layer as it did before.
//!
//! A cell code is what one cell of a text window shows: below 0x100 a glyph of `u6.ch`, and
//! from 0xAC00 a Hangul syllable, drawn from the Hangul table ([`u6_formats::font::HangulFont`]).
//! Text arrives as bytes; with `wide` on, the UTF-8 assembler ([`Utf8`]) turns the three bytes
//! of a Hangul syllable into its code in front of the escape layer, and lets every other byte
//! through as itself. With [`Language::Korean`], the josa resolver ([`Josa`]) then replaces
//! each particle marker by the form the last cell put asks for ([`Tail`]).

use u6_formats::font::{HANGUL_FIRST, HANGUL_LAST};

/// The grammar rules of the text layer (spec 08: the pack's declared language, set apart from
/// `wide`). English keeps every rule of the original.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Language {
    /// Articles before object names, plural letters after `/` and `\`, no particle markers.
    #[default]
    English,
    /// No article, no plural letters, the josa markers resolved ([`Josa`]).
    Korean,
}

/// Whether `c` is the code of a Hangul syllable (U+AC00..=U+D7A3).
pub fn is_hangul(c: i32) -> bool {
    u32::try_from(c).is_ok_and(|c| (HANGUL_FIRST..=HANGUL_LAST).contains(&c))
}

/// The character a cell code stands for in the transcript: a code below 0x100 as Latin-1 (one
/// char per byte, as before), any other code as its Unicode scalar (U+FFFD for none).
pub fn cell_char(c: u16) -> char {
    match u8::try_from(c) {
        Ok(b) => char::from(b),
        Err(_) => char::from_u32(u32::from(c)).unwrap_or(char::REPLACEMENT_CHARACTER),
    }
}

/// Test-only: the character of a cell code in a line that holds only bytes (an English-only
/// test reading the line or the transcript). A code from 0x100 there panics instead of being
/// read as some other character.
#[cfg(test)]
pub(crate) fn latin_char(c: u16) -> char {
    match u8::try_from(c) {
        Ok(b) => char::from(b),
        Err(_) => panic!("a wide cell {c:#06x} in a line of bytes"),
    }
}

/// The UTF-8 assembler: holds the bytes that can still become a Hangul syllable, and nothing
/// else.
///
/// A syllable is three bytes: a lead 0xEA..=0xED, then two continuation bytes 0x80..=0xBF, the
/// first of which is limited by the lead (0xB0.. after 0xEA, ..=0x9E after 0xED) and the code
/// at most U+D7A3. Every other byte passes at once as itself: ASCII, a byte that cannot start
/// a syllable (a lone 0x8f, Latin-1, the lead of any other code point). A byte that breaks a
/// held sequence first lets the held bytes out unchanged, then is taken as a new first byte.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Utf8 {
    held: [u8; 2],
    len: u8,
}

impl Utf8 {
    /// No byte is held.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Takes byte `b`; pushes to `out` the codes that go on: held bytes let out, then `b`
    /// itself or the syllable it completes (or nothing, when `b` is held).
    pub fn feed(&mut self, b: u8, out: &mut Vec<i32>) {
        match self.len {
            0 if (0xea..=0xed).contains(&b) => self.hold(b),
            0 => out.push(i32::from(b)),
            1 if second(self.held[0], b) => self.hold(b),
            1 => {
                self.drain(out);
                self.feed(b, out);
            }
            _ => {
                let [lead, mid] = self.held;
                let code =
                    u32::from(lead & 0x0f) << 12 | u32::from(mid & 0x3f) << 6 | u32::from(b & 0x3f);
                if (0x80..=0xbf).contains(&b) && code <= HANGUL_LAST {
                    self.len = 0;
                    out.push(code as i32);
                } else {
                    self.drain(out);
                    self.feed(b, out);
                }
            }
        }
    }

    /// Lets the held bytes out unchanged.
    pub fn drain(&mut self, out: &mut Vec<i32>) {
        out.extend(
            self.held[..usize::from(self.len)]
                .iter()
                .map(|&b| i32::from(b)),
        );
        self.len = 0;
    }

    fn hold(&mut self, b: u8) {
        self.held[usize::from(self.len)] = b;
        self.len += 1;
    }
}

/// Whether `b` may follow `lead` in a Hangul syllable's bytes.
fn second(lead: u8, b: u8) -> bool {
    match lead {
        0xea => (0xb0..=0xbf).contains(&b),
        0xed => (0x80..=0x9e).contains(&b),
        _ => (0x80..=0xbf).contains(&b),
    }
}

/// The codes of `text` through a fresh assembler, its end letting the held bytes out.
pub fn decode(text: &[u8]) -> Vec<i32> {
    let mut a = Utf8::default();
    let mut out = Vec::with_capacity(text.len());
    for &b in text {
        a.feed(b, &mut out);
    }
    a.drain(&mut out);
    out
}

/// A copy of the text layer's front (the assembler, the josa resolver and the last cell put)
/// that counts, for `printf`'s padding, the cells text takes from where it stands: every code
/// it lets through is one cell and is put to its tail, as the escape layer would put it with
/// no escape in effect (spec 08 §1, `printf` counts cells). As the text layer does with `wide`
/// on, it lets the `{` right after an `@` through as itself (it opens `@{…}`, no marker).
#[derive(Clone, Debug)]
pub struct Front {
    utf8: Utf8,
    josa: Josa,
    tail: Tail,
    korean: bool,
    /// The last code let through was `@`.
    after_at: bool,
    /// The cells counted so far.
    pub cells: usize,
}

impl Front {
    /// The front as `utf8`, `josa` and `tail` hold it now, under `language`'s rules, `after_at`
    /// when the escape layer's last code was `@` ([`super::TextState::after_at`]).
    pub fn new(utf8: Utf8, josa: &Josa, tail: Tail, language: Language, after_at: bool) -> Self {
        Front {
            utf8,
            josa: josa.clone(),
            tail,
            korean: language == Language::Korean,
            after_at,
            cells: 0,
        }
    }

    /// Takes the text bytes `text` (the assembler is on).
    pub fn feed(&mut self, text: &[u8]) {
        let mut codes = Vec::new();
        for &b in text {
            self.utf8.feed(b, &mut codes);
            for c in std::mem::take(&mut codes) {
                self.code(c);
            }
        }
    }

    /// Lets out what it holds, as `release` does: the assembler's bytes through the josa
    /// resolver, then the resolver's codes.
    pub fn release(&mut self) {
        let mut codes = Vec::new();
        self.utf8.drain(&mut codes);
        for c in codes {
            self.code(c);
        }
        for c in self.josa.drain() {
            self.put(c);
        }
    }

    fn code(&mut self, c: i32) {
        let mut out = Vec::new();
        let brace = self.after_at && c == 0x7b && self.josa.is_empty();
        if self.korean && !brace {
            self.josa.feed(c, &self.tail, &mut out);
        } else {
            out = self.josa.drain();
            out.push(c);
        }
        for c in out {
            self.put(c);
        }
    }

    /// One cell; a newline (or NUL, which prints as one) is counted but is no cell of the tail.
    fn put(&mut self, c: i32) {
        if c > 0 && c != 0x0a {
            self.tail.put(c);
        }
        self.after_at = c == 0x40;
        self.cells += 1;
    }
}

/// The last cell put into the line and the digit run it ends, for the josa resolver.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tail {
    /// The cell code (0 before any).
    pub last: i32,
    /// The zeros that end the run of digits `last` ends.
    pub zeros: u8,
    /// The run has a digit other than 0.
    pub nonzero: bool,
}

impl Tail {
    /// Cell `c` was put.
    pub fn put(&mut self, c: i32) {
        let digit = |c: i32| (0x30..=0x39).contains(&c);
        if digit(c) {
            if !digit(self.last) {
                self.zeros = 0;
                self.nonzero = false;
            }
            if c == 0x30 {
                self.zeros = self.zeros.saturating_add(1);
            } else {
                self.zeros = 0;
                self.nonzero = true;
            }
        }
        self.last = c;
    }

    /// How the text before a particle ends.
    fn coda(&self) -> Coda {
        let c = self.last;
        if is_hangul(c) {
            return match (c - HANGUL_FIRST as i32) % 28 {
                0 => Coda::Open,
                8 => Coda::Rieul,
                _ => Coda::Closed,
            };
        }
        if !(0x30..=0x39).contains(&c) {
            return Coda::Unknown;
        }
        // The number read in Korean: its last digit (영 일 이 삼 사 오 육 칠 팔 구), or for a
        // number ending in zeros the last unit spoken (십 백 천, then 만 억 조 경 for each four).
        if !self.nonzero {
            return Coda::Closed;
        }
        const DIGITS: [Coda; 10] = [
            Coda::Closed,
            Coda::Rieul,
            Coda::Open,
            Coda::Closed,
            Coda::Open,
            Coda::Open,
            Coda::Closed,
            Coda::Rieul,
            Coda::Rieul,
            Coda::Open,
        ];
        const MYRIADS: [Coda; 4] = [Coda::Closed, Coda::Closed, Coda::Open, Coda::Closed];
        match self.zeros {
            0 => DIGITS[(c - 0x30) as usize],
            1..=3 => Coda::Closed,
            z => MYRIADS
                .get(usize::from(z / 4) - 1)
                .copied()
                .unwrap_or(Coda::Unknown),
        }
    }
}

/// The end of the text before a particle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Coda {
    /// A final consonant other than ㄹ.
    Closed,
    /// The final consonant ㄹ.
    Rieul,
    /// No final consonant.
    Open,
    /// Not a syllable or a number (a Latin name, punctuation, nothing yet).
    Unknown,
}

/// The particle markers and their forms: after a final consonant, after none, and after
/// anything else.
const MARKERS: [(&str, [&str; 3]); 5] = [
    ("{이/가}", ["이", "가", "이(가)"]),
    ("{은/는}", ["은", "는", "은(는)"]),
    ("{을/를}", ["을", "를", "을(를)"]),
    ("{과/와}", ["과", "와", "과(와)"]),
    ("{(으)로}", ["으로", "로", "(으)로"]),
];

/// The josa resolver: holds the codes from a `{` while they can still become one of the
/// [`MARKERS`]; a whole marker becomes its form for the last cell put ([`Tail`]), anything
/// else lets the held codes out unchanged.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Josa {
    held: Vec<i32>,
}

impl Josa {
    /// No code is held.
    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }

    /// Takes code `c`; pushes to `out` the codes that go on, `tail` being the last cell put
    /// before them.
    pub fn feed(&mut self, c: i32, tail: &Tail, out: &mut Vec<i32>) {
        if self.held.is_empty() {
            if c == 0x7b {
                self.held.push(c);
            } else {
                out.push(c);
            }
            return;
        }
        self.held.push(c);
        let n = self.held.len();
        let mut prefix = false;
        for (marker, forms) in MARKERS {
            let codes = marker.chars().map(|m| m as i32);
            if !codes.clone().take(n).eq(self.held.iter().copied()) {
                continue;
            }
            if codes.count() == n {
                let form = match (tail.coda(), marker == "{(으)로}") {
                    (Coda::Closed, _) | (Coda::Rieul, false) => forms[0],
                    (Coda::Open, _) | (Coda::Rieul, true) => forms[1],
                    (Coda::Unknown, _) => forms[2],
                };
                out.extend(form.chars().map(|m| m as i32));
                self.held.clear();
                return;
            }
            prefix = true;
        }
        if !prefix {
            // Only the first held code is a `{`: the rest go out, `c` starts again.
            self.held.pop();
            out.append(&mut self.held);
            self.feed(c, tail, out);
        }
    }

    /// Lets the held codes out unchanged.
    pub fn drain(&mut self) -> Vec<i32> {
        std::mem::take(&mut self.held)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units(s: &str) -> Vec<i32> {
        s.chars().map(|c| c as i32).collect()
    }

    #[test]
    fn the_assembler_makes_only_hangul_syllables() {
        assert_eq!(decode("가".as_bytes()), [0xac00]);
        assert_eq!(decode("힣".as_bytes()), [0xd7a3]);
        assert_eq!(decode("a가b".as_bytes()), [0x61, 0xac00, 0x62]);
        // Lone, broken, interrupted and other sequences pass byte for byte.
        let pass = |b: &[u8]| {
            assert_eq!(
                decode(b),
                b.iter().map(|&x| i32::from(x)).collect::<Vec<_>>()
            )
        };
        pass(&[0x8f]);
        pass(&[0xea]);
        pass(&[0xea, 0xb0]);
        pass(&[0xea, 0xb0, 0x41]);
        pass(&[0xea, 0x41, 0x80]);
        pass("é".as_bytes());
        pass("ㄱ".as_bytes());
        pass("ꀀ".as_bytes());
        pass(&[0xed, 0x9e, 0xa4]);
        pass(&[0xed, 0xa0, 0x80]);
        pass(&[0xe9, 0x63, 0xea]);
        // A second lead after a lead: the first goes out, the second starts the syllable.
        assert_eq!(decode(&[0xea, 0xea, 0xb0, 0x80]), [0xea, 0xac00]);
        assert_eq!(decode("가나다 ab".as_bytes()).len(), 6);
        assert_eq!(decode(&[0xea, 0xb0]).len(), 2);
    }

    #[test]
    fn a_wide_cell_in_a_line_of_bytes_panics() {
        assert_eq!(latin_char(0x41), 'A');
        assert_eq!(latin_char(0xe9), 'é');
        assert!(std::panic::catch_unwind(|| latin_char(0xac00)).is_err());
        assert!(std::panic::catch_unwind(|| latin_char(0x100)).is_err());
        assert_eq!(cell_char(0xac00), '가');
    }

    #[test]
    fn every_syllable_round_trips_and_nothing_else_assembles() {
        for c in 0xac00..=0xd7a3u32 {
            let ch = char::from_u32(c).unwrap();
            let mut b = [0; 4];
            assert_eq!(
                decode(ch.encode_utf8(&mut b).as_bytes()),
                [c as i32],
                "U+{c:04X}"
            );
        }
        // The code points around the range, three bytes each, stay bytes.
        for c in [0xabff, 0xd7a4, 0xd7ff, 0xe000, 0x3131, 0xa000] {
            let ch = char::from_u32(c).unwrap();
            let mut b = [0; 4];
            let bytes = ch.encode_utf8(&mut b).as_bytes();
            let want: Vec<i32> = bytes.iter().map(|&x| i32::from(x)).collect();
            assert_eq!(decode(bytes), want, "U+{c:04X}");
        }
    }

    #[test]
    fn a_lead_in_the_third_place_starts_again() {
        // After two held bytes a fresh lead lets them out and begins its own syllable (its
        // last byte 0x80 adds nothing to the code).
        for (lead, mid, code) in [
            (0xea, 0xb0, 0xac00),
            (0xeb, 0x80, 0xb000),
            (0xec, 0xbf, 0xcfc0),
            (0xed, 0x9e, 0xd780),
        ] {
            assert_eq!(
                decode(&[0xea, 0xb0, lead, mid, 0x80]),
                [0xea, 0xb0, code],
                "lead {lead:#x}"
            );
        }
        assert_eq!(
            decode(&[0xeb, 0x80, 0xed, 0x9e, 0xa3]),
            [0xeb, 0x80, 0xd7a3]
        );
    }

    #[test]
    fn a_malformed_prefix_goes_out_and_the_next_syllable_assembles() {
        let cases: [(&[u8], &[i32]); 6] = [
            (&[0xed, 0xa0, 0xea, 0xb0, 0x80], &[0xed, 0xa0, 0xac00]),
            (
                &[0xed, 0x9e, 0xa4, 0xea, 0xb0, 0x80],
                &[0xed, 0x9e, 0xa4, 0xac00],
            ),
            (&[0xea, 0xaf, 0xea, 0xb0, 0x80], &[0xea, 0xaf, 0xac00]),
            (&[0xea, 0x41, 0xea, 0xb0, 0x80], &[0xea, 0x41, 0xac00]),
            (
                &[0xec, 0x80, 0xc0, 0xea, 0xb0, 0x80],
                &[0xec, 0x80, 0xc0, 0xac00],
            ),
            (
                &[0xeb, 0x80, 0x00, 0xeb, 0x80, 0x80],
                &[0xeb, 0x80, 0x00, 0xb000],
            ),
        ];
        for (bytes, want) in cases {
            assert_eq!(decode(bytes), want, "{bytes:02x?}");
        }
    }

    #[test]
    fn the_front_counts_the_cells_from_what_it_holds() {
        let count = |utf8: &[u8], language, tail: &str, text: &[u8]| {
            let mut a = Utf8::default();
            let mut out = Vec::new();
            for &b in utf8 {
                a.feed(b, &mut out);
            }
            assert!(out.is_empty(), "the held bytes are a prefix");
            let mut t = Tail::default();
            for c in tail.chars() {
                t.put(c as i32);
            }
            let mut f = Front::new(a, &Josa::default(), t, language, false);
            f.feed(text);
            f.release();
            f.cells
        };
        assert_eq!(count(&[], Language::English, "", "가a".as_bytes()), 2);
        assert_eq!(count(&[0xea], Language::English, "", &[0xb0, 0x80]), 1);
        assert_eq!(count(&[0xea], Language::English, "", b"x"), 2);
        assert_eq!(count(&[], Language::English, "", &[b'a', 0xea]), 2);
        assert_eq!(count(&[], Language::Korean, "검", "{이/가}".as_bytes()), 1);
        assert_eq!(count(&[], Language::Korean, "Bob", "{이/가}".as_bytes()), 4);
        assert_eq!(count(&[], Language::Korean, "검", "{(으)로}".as_bytes()), 2);
        assert_eq!(count(&[], Language::English, "검", "{이/가}".as_bytes()), 5);
        // The field's own cells are its tail: a marker after them follows them.
        assert_eq!(
            count(&[], Language::Korean, "Bob", "나무{을/를}".as_bytes()),
            3
        );
    }

    fn resolve(before: &str, marker: &str) -> String {
        let mut tail = Tail::default();
        for c in before.chars() {
            tail.put(c as i32);
        }
        let mut j = Josa::default();
        let mut out = Vec::new();
        for c in units(marker) {
            j.feed(c, &tail, &mut out);
        }
        out.extend(j.drain());
        out.iter()
            .map(|&c| char::from_u32(c as u32).unwrap())
            .collect()
    }

    #[test]
    fn josa_forms_follow_the_last_cell() {
        let all = ["{이/가}", "{은/는}", "{을/를}", "{과/와}", "{(으)로}"];
        let forms = |before: &str| all.map(|m| resolve(before, m));
        assert_eq!(forms("칼"), ["이", "은", "을", "과", "로"]);
        assert_eq!(forms("검"), ["이", "은", "을", "과", "으로"]);
        assert_eq!(forms("나무"), ["가", "는", "를", "와", "로"]);
        assert_eq!(
            forms("Bob"),
            ["이(가)", "은(는)", "을(를)", "과(와)", "(으)로"]
        );
        assert_eq!(
            forms("!"),
            ["이(가)", "은(는)", "을(를)", "과(와)", "(으)로"]
        );
        assert_eq!(
            forms(""),
            ["이(가)", "은(는)", "을(를)", "과(와)", "(으)로"]
        );
        let digit = |n: &str| [resolve(n, "{이/가}"), resolve(n, "{(으)로}")];
        assert_eq!(digit("0"), ["이", "으로"], "영");
        assert_eq!(digit("1"), ["이", "로"], "일");
        assert_eq!(digit("2"), ["가", "로"], "이");
        assert_eq!(digit("3"), ["이", "으로"], "삼");
        assert_eq!(digit("6"), ["이", "으로"], "육");
        assert_eq!(digit("10"), ["이", "으로"], "십");
        assert_eq!(digit("100"), ["이", "으로"], "백");
        assert_eq!(digit("1000"), ["이", "으로"], "천");
        assert_eq!(digit("10000"), ["이", "으로"], "만");
        assert_eq!(digit("120000"), ["이", "으로"], "십이만");
        assert_eq!(digit("1000000000000"), ["가", "로"], "일조");
        assert_eq!(digit("25"), ["가", "로"], "이십오");
        assert_eq!(digit("a7"), ["이", "로"], "칠");
        // A run of digits begins after a non-digit: 10 then 5 is "5".
        assert_eq!(digit("10 5"), ["가", "로"], "오");
    }

    #[test]
    fn other_braces_pass_unchanged() {
        assert_eq!(resolve("칼", "{이}"), "{이}");
        assert_eq!(resolve("칼", "{이/나}"), "{이/나}");
        assert_eq!(resolve("칼", "{x}"), "{x}");
        assert_eq!(resolve("칼", "{{을/를}"), "{을");
        assert_eq!(resolve("칼", "{이/가"), "{이/가");
        assert_eq!(resolve("칼", "{(으)로}{"), "로{");
    }
}
