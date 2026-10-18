//! Spec 08 §1, `printf`'s cell count: its copy of the escape layer and the line
//! (`printf::Escaped`) against the engine. On random synthetic text, with the window, the line
//! and the escape layer in random states (wrapped rows included), a field's count must equal the
//! codes the escape layer takes for it, and in Korean the copy's last cell the engine's, except
//! where the field's own end is held and the format's text after it completes it (the count is
//! provisional there): then the line printf draws must equal the field placed by hand. Hand
//! cases pin the copy's arms and seeds one by one.

use std::rc::Rc;

use u6_formats::font::{HANGUL_GLYPHS, HangulFont};

use super::text::{CAPITALISE, RUNIC, SCROLL, WRAP};
use super::*;
use crate::ui::wide::{Language, Tail};

/// A synthetic Hangul table (each syllable its own rows).
fn table() -> Rc<HangulFont> {
    let rows = |i: usize| [0xa5, (i >> 8) as u8, i as u8, 0x5a, 0xff, 0, 0x81, 0x3c];
    Rc::new(HangulFont::from_glyphs((0..HANGUL_GLYPHS).map(rows).collect()).unwrap())
}

fn wide(language: Language) -> State {
    let mut s = state();
    s.ui.hangul = Some(table());
    s.ui.text.set_wide(true);
    s.ui.text.set_language(language);
    s
}

fn leak(b: Vec<u8>) -> &'static [u8] {
    Box::leak(b.into_boxed_slice())
}

/// A linear congruential generator: the cases are the same on every run.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<T: Copy>(&mut self, v: &[T]) -> T {
        v[self.below(v.len())]
    }
}

/// Text pieces: escape characters, markers whole and cut, syllables, digits, words that wrap,
/// spaces and hyphens a row breaks at, newlines, `#n` and `$n` (no `*`: it waits for a key).
const PIECES: &[&str] = &[
    "@",
    "{",
    "}",
    "&",
    "<",
    ">",
    "`",
    "#3",
    "$1",
    "#",
    "$",
    "%",
    "/",
    "\\",
    "^",
    "^J",
    "^@",
    "^3",
    "가",
    "검",
    "달",
    "3",
    "0",
    "10",
    "a",
    "B",
    " ",
    "  ",
    ".",
    "-",
    "{이/가}",
    "{(으)로}",
    "{을/를}",
    "{이",
    "/가}",
    "{(",
    "\n",
    "x",
    "@{",
    "이",
    "으로",
    "}{이/가}",
    "@@",
    "abcdefgh",
    "가나다라마",
];
const LEADS: &[&[u8]] = &[b"\xea", b"\xea\xb0", b"\xed\x9e"];
/// Lines before a field and how the field begins: a row a padding left dropping spaces, then a
/// space and a marker; a held marker prefix that a padding lets out into a full row which
/// breaks after a hyphen (with a space before it, and without), or at the start a flush left;
/// a newline to skip after a dropping row, then a newline, or a space and a newline.
const SETUPS: &[(&[Pre], &str)] = &[
    (
        &[Pre::Puts("가".as_bytes()), Pre::Pad(20, 0x20)],
        " {이/가}",
    ),
    (&[Pre::Puts("aaaaaaaaaaa-bbbb{이".as_bytes())], " {을/를}"),
    (&[Pre::Puts("aa aaaaaaaa-bbbb{이".as_bytes())], " {을/를}"),
    (
        &[
            Pre::Puts(b"aaa bbbb"),
            Pre::Flush,
            Pre::Puts("cccccccc{이".as_bytes()),
        ],
        " {을/를}",
    ),
    (
        &[
            Pre::Puts("가".as_bytes()),
            Pre::Pad(9, 0x20),
            Pre::SkipNewline,
        ],
        "\n {이/가}",
    ),
    (
        &[
            Pre::Puts("가".as_bytes()),
            Pre::Pad(20, 0x20),
            Pre::SkipNewline,
        ],
        " \n {이/가}",
    ),
];

/// A field's held end and the format text that finishes it: a UTF-8 lead and its continuation
/// (cut after one byte and after two), a josa marker cut after `{이` and after `{(`.
const FOLLOWS: &[(&[u8], &[u8])] = &[
    (b"\xea\xb0", b"\x80"),
    (b"\xea", b"\xb0\x80"),
    ("{이".as_bytes(), "/가}".as_bytes()),
    (b"{(", "으)로}".as_bytes()),
];

/// Something held, and what finishes it: a UTF-8 lead (one byte and two), a josa marker begun,
/// an `@` before the `{` of a brace.
const HELD: &[(&[u8], &[u8])] = &[
    (b"\xea", b"\xb0\x80"),
    (b"\xea\xb0", b"\x80"),
    ("{이".as_bytes(), "/가}".as_bytes()),
    (b"{(", "으)로}".as_bytes()),
    (b"@", b"{x}"),
];

fn pieces(r: &mut Lcg, max: usize) -> Vec<u8> {
    let mut v = Vec::new();
    for _ in 0..r.below(max) + 1 {
        if r.below(10) == 0 {
            v.extend_from_slice(r.pick(LEADS));
        } else {
            v.extend_from_slice(r.pick(PIECES).as_bytes());
        }
    }
    v
}

/// What comes before the field: text put a byte at a time, padding, an unescaped string, a
/// flush, the newline a scrolling window skips (as a combat line sets it).
#[derive(Clone, Copy, Debug)]
enum Pre {
    Puts(&'static [u8]),
    Pad(i16, i32),
    Raw(&'static [u8]),
    Flush,
    SkipNewline,
}

/// A case: the grammar, the window (its columns and flags), the plural, hidden text shown,
/// `#3` and `$1`, what comes before the field, then the field, its alignment, width and padding.
#[derive(Clone, Debug)]
struct Case {
    korean: bool,
    x0: u8,
    x1: u8,
    flags: u8,
    plural: bool,
    shown: bool,
    int3: u16,
    str1: &'static str,
    pre: Vec<Pre>,
    field: &'static [u8],
    left: bool,
    width: i16,
    pad: u8,
    /// The format's text after the directive (none: the field ends the format).
    follow: &'static [u8],
}

impl Case {
    /// A Korean case in the 17-cell scroll, as main leaves it (wrapping, capitals).
    fn ko(pre: Vec<Pre>, field: &'static str, left: bool, width: i16) -> Self {
        Case {
            korean: true,
            x0: 0x16,
            x1: 0x26,
            flags: WRAP | SCROLL | CAPITALISE,
            plural: false,
            shown: false,
            int3: 2,
            str1: "검",
            pre,
            field: field.as_bytes(),
            left,
            width,
            pad: b' ',
            follow: b"",
        }
    }
}

/// What one put of a case gives: the padding `field_count` chose, its count and last cell (none
/// for a put with a padding given or through `printf`), the codes the escape layer took for the
/// field and the engine's last cell.
#[derive(Clone, Debug)]
struct Put {
    padding: i16,
    cells: usize,
    tail: Tail,
    taken: u32,
    engine: Tail,
    /// The scroll's lines after the field (the finished ones, then the last).
    lines: Vec<String>,
    /// The padding the other pad character (`0` for a space, a space for `0`) would choose.
    other: i16,
    /// The field's own end was held where it ended (and its padding, if after it, was put).
    held: bool,
    /// The branches of the copy's line layout the simulation whose count was returned took
    /// (`printf::Laid`, by index), and those of every simulation the count ran.
    laid: [u32; 13],
    ran: [u32; 13],
    /// The assembler or the josa resolver held a code where the field began.
    held_before: bool,
}

/// How a case puts its field: as `printf` does, by hand, with the padding `field_count` chose
/// or the one given (the text after the field letting out its held end), or with the padding
/// chosen and the held end kept for that text; or through `printf` itself with the directive
/// the case stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum How {
    Chosen,
    Padding(i16),
    Kept,
    Printf,
}

/// The `%s` directive of a case: its alignment, pad character and width.
fn directive(left: bool, pad: u8, width: i16) -> Vec<u8> {
    let mut d = b"%".to_vec();
    if left {
        d.push(b'-');
    }
    if pad == b'0' {
        d.push(b'0');
    }
    if width > 0 {
        d.extend_from_slice(width.to_string().as_bytes());
    }
    d.push(b's');
    d
}

/// The scroll's lines so far: the finished ones, then what is drawn and pending of the last.
fn scroll_lines(s: &State) -> Vec<String> {
    let t = &s.ui.text;
    let mut v = s.ui.messages.clone();
    let mut last: String = t
        .transcript_line
        .iter()
        .map(|&c| wide::cell_char(c))
        .collect();
    let pending = &t.line[usize::from(t.line_start)..usize::from(t.line_len)];
    last.extend(pending.iter().map(|&c| wide::cell_char(c)));
    v.push(last);
    v
}

/// Puts `c.pre`, then the field as `how` says; `None` when the engine did not finish.
fn put(c: &Case, how: How) -> Option<Put> {
    let mut s = wide(if c.korean {
        Language::Korean
    } else {
        Language::English
    });
    let tables = s.ui.tables.clone();
    s.ui.text
        .open(&tables, WindowId::Messages, c.x0, 0x0e, c.x1, 0x17);
    s.ui.text.win_mut().flags = c.flags;
    s.ui.text.plural = c.plural;
    if c.shown {
        s.actors.objlist_tail[0x5e] = 1;
    }
    s.converse.ints[3] = c.int3;
    s.converse.strs[1] = crate::converse::StrRef::Buffer(1);
    s.converse.buffers[1] = c.str1.as_bytes().to_vec();
    let Case {
        field,
        left,
        width,
        pad,
        follow,
        ..
    } = *c;
    let pre = c.pre.clone();
    let got: Rc<RefCell<Option<Put>>> = Rc::new(RefCell::new(None));
    let g = got.clone();
    let e = run(s, &[], 4, move |st, io| async move {
        for p in pre {
            match p {
                Pre::Puts(b) => printf::puts(&st, &io, b).await,
                Pre::Pad(n, c) => print::put_raw_n(&st, &io, n, c).await,
                Pre::Raw(b) => print::put_raw_str(&st, &io, b).await,
                Pre::Flush => print::put_raw(&st, &io, -1).await,
                Pre::SkipNewline => st.borrow_mut().ui.text.skip_newline = true,
            }
        }
        // Only the case's own put and the one that keeps the held end count the field (a put
        // with a padding given, or through printf, needs no count). Of the branches of the
        // copy's line that count took, those of the simulation whose count and tail it returned
        // are recorded apart from those of all it ran; the other pad character's count is not.
        let (mut chosen, mut cells, mut tail, mut other) = (0, 0, Tail::default(), 0);
        let (mut laid, mut ran) = ([0; 13], [0; 13]);
        let held_before = {
            let t = &st.borrow().ui.text;
            !t.utf8.is_empty() || !t.josa.is_empty()
        };
        if matches!(how, How::Chosen | How::Kept) {
            let before = printf::LAID.with(|l| l.get());
            (chosen, cells, tail) = printf::field_count(&st.borrow(), field, left, width, pad);
            let after = printf::LAID.with(|l| l.get());
            ran = std::array::from_fn(|i| after[i] - before[i]);
            laid = printf::RETURNED.with(|r| r.get());
        }
        if how == How::Chosen {
            let other_pad = if pad == b'0' { b' ' } else { b'0' };
            other = printf::field_count(&st.borrow(), field, left, width, other_pad).0;
        }
        // As printf puts a field: a left-aligned one, then its padding (which lets out its held
        // end); a right-aligned one after its padding (what that lets out is the text before
        // the field). What comes after the field lets out the rest of its held end.
        printf::TAKEN.with(|t| t.set(0));
        let (mut taken, mut engine, mut held) = (0, Tail::default(), false);
        if how == How::Printf {
            let fmt = leak([directive(left, pad, width), follow.to_vec()].concat());
            let args: &'static [Arg<'static>] = Box::leak(vec![Arg::Str(field)].into_boxed_slice());
            printf(&st, &io, fmt, args).await;
        } else {
            let p = match how {
                How::Padding(p) => p,
                _ => chosen,
            };
            if !left {
                print::put_raw_n(&st, &io, p, i32::from(pad)).await;
            }
            printf::TAKEN.with(|t| t.set(0));
            printf::puts(&st, &io, field).await;
            if left {
                print::put_raw_n(&st, &io, p, i32::from(pad)).await;
            }
            {
                let t = &st.borrow().ui.text;
                held = !t.utf8.is_empty() || !t.josa.is_empty();
            }
            // What comes after the field lets out its held end, unless the case keeps it for
            // the format's text after the field to go on from.
            if follow.is_empty() || how != How::Kept {
                printf::release(&st, &io).await;
            }
            taken = printf::TAKEN.with(|t| t.get());
            engine = st.borrow().ui.text.tail;
            printf::puts(&st, &io, follow).await;
            printf::release(&st, &io).await;
        }
        *g.borrow_mut() = Some(Put {
            padding: chosen,
            cells,
            tail,
            taken,
            engine,
            lines: scroll_lines(&st.borrow()),
            other,
            held,
            laid,
            ran,
            held_before,
        });
    });
    e.finished().then(|| got.borrow().clone()).flatten()
}

thread_local! {
    /// The cases where the pad character changes the padding chosen.
    static PAD_MATTERS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// The cases whose held end the format's text after the field completes (the count
    /// provisional).
    static PROVISIONAL: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// The branches of the copy's line layout taken by the simulations whose result is checked
    /// exactly (`printf::Laid`, by index), and by all the counts of those cases ran.
    static COUNTED: std::cell::Cell<[u32; 13]> = const { std::cell::Cell::new([0; 13]) };
    static RAN: std::cell::Cell<[u32; 13]> = const { std::cell::Cell::new([0; 13]) };
    /// The cases that no padding fits, and those of them with something held before the field.
    static NO_FIT: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    static NO_FIT_HELD: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// The padding the rule picks (spec 08 §1) from `cells(p)`, the engine's count of the field
/// after a padding of `p` (the codes its escape layer takes, drawn or not), for every `p` in
/// `0..=w`: the first fixed point of
/// `p -> max(0, w - cells(p))` from `max(0, w - cells(0))`; else 0 if something is `held`
/// before the field, and the smallest `p` of the cycle if not.
fn rule(cells: &[u32], held: bool) -> i16 {
    let w = cells.len() as i64 - 1;
    let fit = |p: usize| (w - i64::from(cells[p])).max(0) as usize;
    let mut p = fit(0);
    let mut seen = Vec::new();
    loop {
        let next = fit(p);
        if next == p {
            return p as i16;
        }
        if let Some(i) = seen.iter().position(|&q| q == p) {
            return if held {
                0
            } else {
                *seen[i..].iter().min().unwrap() as i16
            };
        }
        seen.push(p);
        p = next;
    }
}

/// The paddings that cycle, smallest first, where none of `0..=w` fits (`cells` as for `rule`);
/// `None` where one fits.
fn cycle(cells: &[u32]) -> Option<Vec<usize>> {
    let w = cells.len() as i64 - 1;
    let fit = |p: usize| (w - i64::from(cells[p])).max(0) as usize;
    let mut p = fit(0);
    let mut seen = Vec::new();
    loop {
        let next = fit(p);
        if next == p {
            return None;
        }
        if let Some(i) = seen.iter().position(|&q| q == p) {
            let mut c = seen[i..].to_vec();
            c.sort_unstable();
            return Some(c);
        }
        seen.push(p);
        p = next;
    }
}

/// Puts the field of `c` with the padding `field_count` chose, and with every padding from 0 to
/// its width; a description of the case when the copy and the engine differ: the count for the
/// chosen padding must be the codes the escape layer took, in Korean the copy's last cell the
/// engine's (in English nothing reads it), and the padding the one the rule picks from the
/// engine's count after each padding.
fn check(c: Case) -> Option<String> {
    let desc = format!("{c:?}");
    let Some(got) = put(&c, How::Chosen) else {
        return Some(format!("{desc}: did not finish"));
    };
    let mut bad = Vec::new();
    // The narrowed claim (spec 08 §1): where the field's own end is held and the format's text
    // after it does more than let it out (it completes it), the count is provisional; there only
    // the line printf draws is checked, against the field placed by hand with the padding
    // chosen and its held end kept for that text.
    if got.held && !c.follow.is_empty() {
        let Some(kept) = put(&c, How::Kept) else {
            return Some(format!("{desc}: kept did not finish"));
        };
        if kept.lines != got.lines {
            PROVISIONAL.with(|n| n.set(n.get() + 1));
            return match put(&c, How::Printf) {
                Some(p) if p.lines == kept.lines => None,
                Some(p) => Some(format!(
                    "{desc}: printf draws {:?}, by hand {:?}",
                    p.lines, kept.lines
                )),
                None => Some(format!("{desc}: printf did not finish")),
            };
        }
    }
    COUNTED.with(|n| n.set(std::array::from_fn(|i| n.get()[i] + got.laid[i])));
    RAN.with(|n| n.set(std::array::from_fn(|i| n.get()[i] + got.ran[i])));
    if got.cells as u32 != got.taken {
        bad.push(format!(
            "padding {}: count {}, the escape layer took {}",
            got.padding, got.cells, got.taken
        ));
    }
    if c.korean && got.tail != got.engine {
        bad.push(format!(
            "last cell {:?}, the engine's {:?}",
            got.tail, got.engine
        ));
    }
    let w = c.width.max(0);
    let mut counts = Vec::new();
    for p in 0..=w {
        match put(&c, How::Padding(p)) {
            Some(g) => counts.push(g.taken),
            None => return Some(format!("{desc}: padding {p} did not finish")),
        }
    }
    let want = rule(&counts, got.held_before);
    // Where no padding fits (spec 08 §1): the padding chosen and the engine's count of the field
    // after it come to less than the width, and the padding is 0 where something was held.
    if cycle(&counts).is_some() {
        NO_FIT.with(|n| n.set(n.get() + 1));
        let p = got.padding.max(0) as usize;
        if p > w as usize || p as u32 + counts[p] >= w as u32 {
            bad.push(format!(
                "no padding fits, and padding {p} and its count come to {w} or more, on the \
                 engine's counts {counts:?}"
            ));
        }
        if got.held_before {
            NO_FIT_HELD.with(|n| n.set(n.get() + 1));
            if got.padding != 0 {
                bad.push(format!(
                    "no padding fits and something is held, but the padding is {}",
                    got.padding
                ));
            }
        }
    }
    if got.padding != want {
        bad.push(format!(
            "padding {}, the rule on the engine's counts {counts:?} picks {want}",
            got.padding
        ));
    }
    // Through printf itself, with the directive the case stands for, the scroll is the same.
    match put(&c, How::Printf) {
        Some(p) if p.lines == got.lines => {}
        Some(p) => bad.push(format!(
            "printf draws {:?}, by hand {:?}",
            p.lines, got.lines
        )),
        None => bad.push("printf did not finish".to_string()),
    }
    // Where the pad character changes the padding chosen, printf must pass its own.
    if got.other != got.padding {
        PAD_MATTERS.with(|n| n.set(n.get() + 1));
    }
    (!bad.is_empty()).then(|| format!("{desc}: {}", bad.join("; ")))
}

/// One random case.
fn one(seed: u64) -> Option<String> {
    let mut r = Lcg(seed.wrapping_mul(0x9e3779b97f4a7c15) | 1);
    let korean = r.below(4) != 0;
    // The scroll as main leaves it (17 cells, wrapping, capitals), a 40-cell wrapping window,
    // a 9-cell one, and a 12-cell one that does not wrap.
    let (x0, x1, mut flags) = r.pick(&[
        (0x16, 0x26, WRAP | SCROLL | CAPITALISE),
        (0, 0x27, WRAP | SCROLL),
        (0x16, 0x1e, WRAP),
        (0x16, 0x21, 0),
    ]);
    if r.below(3) == 0 {
        flags |= RUNIC;
    }
    let (plural, shown) = (r.below(2) == 0, r.below(4) == 0);
    let int3 = r.pick(&[2u16, 10, 0, 1, 100]);
    let str1 = r.pick(&["검", "나", "달", "x3", " a"]);
    let mut pre = Vec::new();
    if r.below(4) == 0 {
        pre.push(Pre::SkipNewline);
    }
    for _ in 0..r.below(5) {
        pre.push(match r.below(7) {
            0..=2 => Pre::Puts(leak(pieces(&mut r, 4))),
            3 => Pre::Pad(r.pick(&[1, 3, 9, 16, 20, 40]), r.pick(&[0x20, 0x20, 0x30])),
            4 => Pre::Raw(leak(pieces(&mut r, 2))),
            5 => Pre::Flush,
            _ => Pre::SkipNewline,
        });
    }
    let mut field = if r.below(8) == 0 {
        &[][..]
    } else {
        leak(pieces(&mut r, 4))
    };
    let mut width = r.below(35) as i16;
    // Often something held just before the field, and the field begins by finishing it: where
    // a padding lets that out first and none lets the field finish it, no padding may fit.
    if r.below(4) == 0 {
        let (held, finish) = r.pick(HELD);
        pre.push(Pre::Puts(held));
        field = leak([finish, field].concat());
        width = r.below(7) as i16;
    } else if r.below(3) == 0 {
        // Or a line the rules of the copy's layout decide: a row left dropping spaces, a held
        // prefix let out into a full row that breaks at a hyphen or at a flushed start, a
        // newline to skip (ended by the field's first code, or in a window that does not
        // scroll); the field begins with what those rules act on.
        let (setup, begin) = r.pick(SETUPS);
        pre.extend_from_slice(setup);
        field = leak([begin.as_bytes(), field].concat());
        width = r.below(21) as i16;
    }
    let left = r.below(2) == 0;
    let pad = if r.below(5) == 0 { b'0' } else { b' ' };
    // Sometimes format text after the field: often after a field whose end is held, which the
    // text then completes (not well formed: the count is provisional there), else just text.
    let mut follow: &'static [u8] = b"";
    if r.below(5) == 0 {
        if r.below(2) == 0 {
            let (end, finish) = r.pick(FOLLOWS);
            field = leak([field, end].concat());
            follow = leak([finish, b"|"].concat());
            width = r.below(7) as i16;
        } else {
            follow = b"|";
        }
    }
    check(Case {
        korean,
        x0,
        x1,
        flags,
        plural,
        shown,
        int3,
        str1,
        pre,
        field,
        left,
        width,
        pad,
        follow,
    })
    .map(|d| format!("seed {seed}: {d}"))
}

#[test]
fn the_line_rules_a_padding_decides_count_as_the_engine_takes() {
    let puts = |s: &'static str| Pre::Puts(s.as_bytes());
    let mut no_scroll = Case::ko(
        vec![puts("가"), Pre::Pad(9, 0x20), Pre::SkipNewline],
        "\n {이/가}",
        true,
        8,
    );
    (no_scroll.x1, no_scroll.flags) = (0x1e, WRAP);
    let cases = [
        // A held marker prefix that the padding lets out into a full row: the row breaks after
        // its hyphen and keeps six cells, then a padding of 15 wraps the next row (the field's
        // space dropped); with a space before the hyphen it still breaks after the hyphen.
        Case::ko(vec![puts("aaaaaaaaaaa-bbbb{이")], " {을/를}", false, 17),
        Case::ko(vec![puts("aa aaaaaaaa-bbbb{이")], " {을/를}", false, 9),
        // After a flush the row starts at the flushed column: the wrap the let-out prefix makes
        // keeps all after it (ten cells), so a padding of 4 does not wrap.
        Case::ko(
            vec![puts("aaa bbbb"), Pre::Flush, puts("cccccccc{이")],
            " {을/를}",
            false,
            9,
        ),
        // A window that does not scroll skips no newline: the field's newline ends the row that
        // drops spaces, and its space is a cell.
        no_scroll,
        // The field's first code ends the skipped newline, so its newline ends the dropping row
        // and the next space is a cell.
        Case::ko(
            vec![puts("가"), Pre::Pad(20, 0x20), Pre::SkipNewline],
            " \n {이/가}",
            true,
            9,
        ),
        // A `0` padding after a row left dropping spaces ends the drop: the field's space is a
        // cell and the marker takes the paired form.
        Case {
            pad: b'0',
            ..Case::ko(vec![puts("가"), Pre::Pad(20, 0x20)], " {이/가}", false, 9)
        },
    ];
    let bad: Vec<String> = cases.into_iter().filter_map(check).collect();
    assert!(bad.is_empty(), "{bad:#?}");
}

/// The scroll lines printf draws after `pre`, a padding of `pad` and a flush when `flush`, then
/// `fmt` with the one string `arg` and a flush (Korean, wide on); the script must run to its end.
fn drawn(
    pre: &'static str,
    pad: i16,
    flush: bool,
    fmt: &'static str,
    arg: &'static str,
) -> Vec<String> {
    drawn_bytes(pre.as_bytes(), pad, flush, fmt.as_bytes(), arg.as_bytes())
}

/// [`drawn`], of bytes.
fn drawn_bytes(
    pre: &'static [u8],
    pad: i16,
    flush: bool,
    fmt: &'static [u8],
    arg: &'static [u8],
) -> Vec<String> {
    let args: &'static [Arg<'static>] = Box::leak(vec![Arg::Str(arg)].into_boxed_slice());
    let e = run(wide(Language::Korean), &[], 4, move |st, io| async move {
        printf::puts(&st, &io, pre).await;
        print::put_raw_n(&st, &io, pad, 0x20).await;
        if flush {
            print::put_raw(&st, &io, -1).await;
        }
        printf(&st, &io, fmt, args).await;
        print::put_raw(&st, &io, -1).await;
        io.present();
    });
    assert!(e.finished(), "the script ran to its end");
    scroll_lines(&e.state())
}

#[test]
fn printf_lays_out_the_padding_with_its_own_pad_character() {
    // After 가's padding left the 17-cell row dropping spaces, `%09s` pads with 0s: they end the
    // drop, the field's space is a cell, its marker the paired form, five cells padded by four.
    let pad16 = format!("가{}", " ".repeat(16));
    assert_eq!(
        drawn("가", 20, false, "%09s|\n", " {이/가}"),
        [pad16.as_str(), "0000 이(가)|", ""]
    );
    // At width 3 no padding fits: with none the field's space is dropped and its marker is one
    // cell after 가 (two cells), and any padding of 0s ends the drop (five cells, whatever its
    // size), so the paddings go 1, 0, 1. 0 is used: the field is drawn short, not too wide.
    assert_eq!(
        drawn("가", 20, false, "%03s|\n", " {이/가}"),
        [pad16.as_str(), "가|", ""]
    );
    // The hyphen and the flushed start, through printf: the field's rows as drawn. The row breaks
    // after the hyphen (not at the space before it) and the field stays on the next row; after a
    // flush the row breaks at the flushed start, and the padding of four ends that row.
    assert_eq!(
        drawn("", 0, false, "aa aaaaaaaa-bbbb{이%9s|\n", " {을/를}"),
        ["aa aaaaaaaa-", "bbbb{이     을(를)|", ""]
    );
    assert_eq!(
        drawn("aaa bbbb", 0, true, "cccccccc{이%9s|\n", " {을/를}"),
        ["aaa bbbb", "cccccccc{이    ", "을(를)|", ""]
    );
}

#[test]
fn zero_width_and_empty_fields_alone_and_after_held_codes() {
    let puts = |s: &'static str| Pre::Puts(s.as_bytes());
    // What is held before the field: nothing, a UTF-8 lead, a josa marker begun, a pending `@`;
    // with each a field that finishes it, and an empty field.
    let held: [(Vec<Pre>, &'static [u8]); 4] = [
        (vec![puts("검")], "가".as_bytes()),
        (vec![Pre::Puts(b"\xea")], &[0xb0, 0x80]),
        (vec![puts("검{이")], "/가}".as_bytes()),
        (vec![puts("검@")], b"{x}"),
    ];
    let mut cases = Vec::new();
    for (pre, field) in held {
        for left in [true, false] {
            // A zero-width `%s`, then an empty field of width 0 and of width 3.
            for (field, width) in [(field, 0), (&[][..], 0), (&[][..], 3)] {
                cases.push(Case {
                    field,
                    ..Case::ko(pre.clone(), "", left, width)
                });
            }
        }
    }
    let bad: Vec<String> = cases.into_iter().filter_map(check).collect();
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn the_copy_of_the_line_holds_where_rows_wrap_capitals_and_skipped_newlines_change_the_tail() {
    let puts = |s: &'static str| Pre::Puts(s.as_bytes());
    let cases = [
        // A capital after a back-quote: the last cell is the capital.
        Case::ko(vec![puts("가")], "`a", true, 5),
        // The capital's letter ends it: the letter after is lowercase, and so is the last cell.
        Case::ko(vec![puts("가")], "`ab", false, 5),
        // The row is full when the field's padding lets out a held marker prefix, so the
        // prefix wraps in the middle of a word and the next row keeps it; the padding then
        // runs past the end of that row and wraps it at a space, and the field's space after it
        // is dropped.
        Case::ko(vec![puts("가bcdefghijklmnopq{이")], " {을/를}", false, 21),
        // gpt-6-astra r4 (a): a pending `@` just before a right-aligned field that fits with no
        // padding: no padding puts nothing, so the `{` opens the brace, five codes (not the
        // marker after 검).
        Case::ko(vec![puts("검@")], "{이/가}", false, 1),
        // A newline the scrolling window skips (a combat line set it) is no code of the line:
        // after 가's padding wrapped the row the field's space is still dropped.
        Case::ko(
            vec![puts("가"), Pre::Pad(19, 0x20), Pre::SkipNewline],
            "\n {이/가}",
            true,
            9,
        ),
    ];
    let bad: Vec<String> = cases.into_iter().filter_map(check).collect();
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn printf_draws_the_padding_chosen_where_none_fits() {
    let lk = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
    // The digit join: a padding of 3 or more would wrap the row after the fifteen cells, drop
    // the field's space and join its zeros to the 1 (the paired form); none fits, nothing is
    // held, and the smallest of the cycle {1, 4} is used.
    let fifteen = lk(format!("{}1", "가".repeat(14)));
    let field = lk(format!(" {}{{이/가}}", "0".repeat(20)));
    let zeros17 = "0".repeat(17);
    assert_eq!(
        drawn(fifteen, 0, false, "%26s|\n", field),
        [&format!("{fifteen} ")[..], &zeros17, "000이|", ""]
    );
    // A pending `@` before `{가}`: no padding keeps the brace, so 가 is drawn highlighted and the
    // particle follows it, 가가가 (a padding would end the `@` and show `{가}`).
    assert_eq!(
        drawn("", 0, false, "가@%5s|\n", "{가}{이/가}"),
        ["가가가|", ""]
    );
    // Hidden text completes a held lead: something is held, so no padding; the syllable is
    // hidden, and the field's space is laid, so the particle takes the paired form.
    let field = leak([&b"\xb0\x80& "[..], "{이/가}".as_bytes()].concat());
    assert_eq!(
        drawn_bytes(b"aaaaaaaaa\xea\xb0\x80&\xea", 0, false, b"%8s|\n", field),
        ["aaaaaaaaa가 이(가)|", ""]
    );
    // The count bounds the cells counted, not the line drawn: a field whose own end is held and
    // completed by the text after it (its count provisional). After a held lead, `%5s/가}|`
    // with `\xb0\x80a{이` counts four with no padding and five after any; something is held,
    // so 0, and the format's `/가}` completes the marker after `a`: the field draws six cells
    // against a width of 5, the `|` after it.
    let field = leak([&b"\xb0\x80a"[..], "{이".as_bytes()].concat());
    assert_eq!(
        drawn_bytes(b"\xea", 0, false, "%5s/가}|\n".as_bytes(), field),
        ["가a이(가)|", ""]
    );
}

/// The engine's count of the field of `c` (the codes its escape layer takes, drawn or not) after
/// each padding from 0 to its width.
fn sweep(c: &Case) -> Vec<u32> {
    (0..=c.width.max(0))
        .map(|p| put(c, How::Padding(p)).expect("finished").taken)
        .collect()
}

#[test]
fn where_no_padding_fits_the_padding_is_0_after_something_held_else_the_smallest_of_the_cycle() {
    let puts = |s: &'static str| Pre::Puts(s.as_bytes());
    let lk = |s: String| -> &'static [u8] { leak(s.into_bytes()) };
    let zeros = "0".repeat(20);
    let digits = lk(format!(" {zeros}{{이/가}}"));
    let fifteen = lk(format!("{}1", "가".repeat(14)));
    let n = |a: u32, k: usize, b: u32, m: usize| [vec![a; k], vec![b; m]].concat();
    // (case, the engine's counts after paddings 0..=w, the cycle, the padding chosen)
    let cases: Vec<(Case, Vec<u32>, Vec<usize>, i16)> = vec![
        // gpt-6-astra r4 (b): a held UTF-8 lead before a right-aligned field of width 2: with no
        // padding the field finishes the syllable, one cell, which leaves a padding of 1; that
        // padding lets the lead out first, and the field is two bytes, which leaves none: no
        // padding fits, something is held, and 0 is used (the syllable whole, one cell short).
        (
            Case {
                field: &[0xb0, 0x80],
                ..Case::ko(vec![Pre::Puts(b"\xea")], "", false, 2)
            },
            vec![1, 2, 2],
            vec![0, 1],
            0,
        ),
        // After a row left dropping spaces, `0`s end the drop: two cells with none (the space
        // dropped, the particle after 가), five after any; nothing held, 0 the smallest.
        (
            Case {
                pad: b'0',
                ..Case::ko(vec![puts("가"), Pre::Pad(20, 0x20)], " {이/가}", false, 3)
            },
            vec![2, 5, 5, 5],
            vec![0, 1],
            0,
        ),
        // A pending `@`: with no padding `{가}` is a brace (its 가 and the particle, four
        // codes), after any the `{` opens none (seven); nothing held, 0 the smallest.
        (
            Case::ko(vec![puts("가@")], "{가}{이/가}", false, 5),
            vec![4, 7, 7, 7, 7, 7],
            vec![0, 1],
            0,
        ),
        // Digits joined across a dropped space: fifteen cells before, so a padding of 3 or more
        // wraps the row and drops the field's space, its twenty zeros continue the 1, and the
        // marker takes the paired form; the cycle has no 0, and nothing held: its smallest, 1.
        (
            Case {
                field: digits,
                ..Case::ko(vec![Pre::Puts(fifteen)], "", false, 26)
            },
            n(22, 3, 25, 24),
            vec![1, 4],
            1,
        ),
        // Hidden text completes a held lead, so the field's first laid code is a space a
        // wrapping padding drops: 7, then 8 after 1 to 7, 5 after 8; held, so 0.
        (
            Case {
                field: leak([&b"\xb0\x80& "[..], "{이/가}".as_bytes()].concat()),
                ..Case::ko(vec![puts("aaaaaaaaa가&"), Pre::Puts(b"\xea")], "", false, 8)
            },
            vec![7, 8, 8, 8, 8, 8, 8, 8, 5],
            vec![0, 1],
            0,
        ),
        // The digit join after a held lead (hidden, so the 1 stays the cell before the
        // padding): 24, 25 after 1 and 2, 28 after 3 or more; the cycle {1, 4} has no 0, but
        // something is held, so 0.
        (
            Case {
                field: leak([&b"\xb0\x80& "[..], zeros.as_bytes(), "{이/가}".as_bytes()].concat()),
                ..Case::ko(
                    vec![
                        Pre::Puts(lk(format!("{}1&", "가".repeat(14)))),
                        Pre::Puts(b"\xea"),
                    ],
                    "",
                    false,
                    29,
                )
            },
            n(24, 1, 25, 2).into_iter().chain(vec![28; 27]).collect(),
            vec![1, 4],
            0,
        ),
        // A held lead before a field that completes it and ends with a josa marker begun (the
        // last line of printf_draws_the_padding_chosen_where_none_fits, with nothing after the
        // field): four with no padding, five after any (the padding lets the lead out first);
        // something is held, so 0.
        (
            Case {
                field: leak([&b"\xb0\x80a"[..], "{이".as_bytes()].concat()),
                ..Case::ko(vec![Pre::Puts(b"\xea")], "", false, 5)
            },
            vec![4, 5, 5, 5, 5, 5],
            vec![0, 1],
            0,
        ),
        // The same after a josa marker begun (held by the resolver, not the assembler).
        (
            Case {
                field: lk(format!("/가}}& {zeros}{{이/가}}")),
                ..Case::ko(
                    vec![Pre::Puts(lk(format!("{}1&{{이", "가".repeat(14))))],
                    "",
                    false,
                    30,
                )
            },
            n(24, 1, 26, 2).into_iter().chain(vec![29; 28]).collect(),
            vec![1, 4],
            0,
        ),
    ];
    for (c, cells, cyc, chosen) in cases {
        let desc = format!("{c:?}");
        assert_eq!(sweep(&c), cells, "{desc}");
        assert_eq!(cycle(&cells), Some(cyc), "{desc}");
        assert_eq!(put(&c, How::Chosen).unwrap().padding, chosen, "{desc}");
        assert_eq!(check(c), None);
    }
}

#[test]
fn the_count_of_a_field_is_the_codes_the_escape_layer_takes_and_its_last_cell_the_engines() {
    let n = 6000;
    COUNTED.with(|l| l.set([0; 13]));
    RAN.with(|l| l.set([0; 13]));
    PAD_MATTERS.with(|m| m.set(0));
    PROVISIONAL.with(|m| m.set(0));
    NO_FIT.with(|m| m.set(0));
    NO_FIT_HELD.with(|m| m.set(0));
    let bad: Vec<String> = (0..n).filter_map(one).collect();
    // ... and has cases where printf's pad character decides the padding, and fields whose held
    // end the format's text after them completes.
    let pad_matters = PAD_MATTERS.with(|m| m.get());
    assert!(
        pad_matters > 0,
        "no case where the pad character changes the padding"
    );
    let provisional = PROVISIONAL.with(|m| m.get());
    assert!(
        provisional > 0,
        "no field whose held end the format completes"
    );
    // ... and fields that no padding fits, with something held before them and without.
    let no_fit = NO_FIT.with(|m| m.get());
    let no_fit_held = NO_FIT_HELD.with(|m| m.get());
    assert!(
        no_fit_held > 0 && no_fit_held < no_fit,
        "no-fit fields {no_fit}, {no_fit_held} of them after something held"
    );
    // The corpus reaches every branch of the copy's line layout, in the simulations whose
    // result it checks: only the one each count returned, not the others it ran (some cases
    // run several, so the receipt credits fewer branches than ran).
    let laid = COUNTED.with(|l| l.get());
    let ran = RAN.with(|l| l.get());
    assert!(
        laid.iter().sum::<u32>() < ran.iter().sum::<u32>(),
        "the receipt credits every simulation a count ran: {laid:?} of {ran:?}"
    );
    let missed: Vec<_> = printf::Laid::ALL
        .iter()
        .filter(|&&b| laid[b as usize] == 0)
        .collect();
    assert!(
        missed.is_empty(),
        "branches never reached: {missed:?} ({laid:?})"
    );
    assert!(
        bad.is_empty(),
        "{} of {n} cases differ, the first: {:#?}",
        bad.len(),
        &bad[..bad.len().min(5)]
    );
}

/// The first scroll line `fmt` with the one string argument `field` prints (Korean, wide on).
fn ko(fmt: &str, field: &'static str) -> String {
    let fmt = leak([fmt.as_bytes(), b"\n"].concat());
    let args: &'static [Arg<'static>] =
        Box::leak(vec![Arg::Str(field.as_bytes())].into_boxed_slice());
    print(wide(Language::Korean), fmt, args).state().ui.messages[0].clone()
}

#[test]
fn each_part_of_the_copy_of_the_escape_layer_counts_as_the_engine_takes() {
    // A brace open before the field (the format's), closed by the field's `}`: the marker
    // follows 검, one cell.
    assert_eq!(ko("@{검%-6s|", "}{이/가}"), "검이    |");
    // A `/` the format left pending drops the field's letters; the marker follows 검.
    assert_eq!(ko("검/%-6s|", "ab{이/가}"), "검이   |");
    // A window the format made runic: the field's digit draws its runic glyph, no digit, so
    // the marker takes the paired form (its parentheses runic too).
    assert_eq!(ko("<%-8s>|", "3{이/가}"), "\u{b3}이\u{a8}가\u{a9}   |");
    // A newline in the field is no cell: on the next row the marker follows 검.
    let fmt = leak(b"%-5s|\n".to_vec());
    let args: &'static [Arg<'static>] =
        Box::leak(vec![Arg::Str("검\n{이/가}".as_bytes())].into_boxed_slice());
    assert_eq!(
        print(wide(Language::Korean), fmt, args).state().ui.messages,
        ["검", "이  |"]
    );
    // In the field, `/` and the letters after it draw nothing; the marker follows 검.
    assert_eq!(ko("%-7s|", "검/ab{이/가}"), "검이  |");
    // `^` then a digit draws a control glyph, no digit: the paired form.
    assert_eq!(ko("%-7s|", "검^3{이/가}"), "검\u{13}이(가)|");
    // Hidden text is dropped before the brace's `}` is read: the hidden `}` leaves the
    // format's brace open, the shown one closes it, and the marker follows 가.
    assert_eq!(ko("@{가%-9s|", "&}&}{이/가}"), "가가    |");
}

#[test]
fn the_key_waits_flush_ends_the_start_of_a_wrapped_row() {
    // 가's padding wraps the 17-cell scroll at a space, so the row after drops spaces; the key
    // wait (`*`) flushes, and a flush is a code other than a space, so the field's space after
    // it is put and the marker follows it: six cells, padded by three.
    let fmt = leak(b"%-20s%-9s|\n".to_vec());
    let args: &'static [Arg<'static>] = Box::leak(
        vec![Arg::Str("가".as_bytes()), Arg::Str("* {이/가}".as_bytes())].into_boxed_slice(),
    );
    let e = run(
        wide(Language::Korean),
        &[Key::Char('a')],
        8,
        move |st, io| async move {
            printf(&st, &io, fmt, args).await;
            print::put_raw(&st, &io, -1).await;
            io.present();
        },
    );
    assert!(e.finished(), "the key ended the wait");
    assert_eq!(e.state().ui.messages[1], " 이(가)   |");
}
