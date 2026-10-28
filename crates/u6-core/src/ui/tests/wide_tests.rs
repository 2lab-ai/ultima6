//! Spec 08 Acceptance 2, the text path: Hangul cells, the UTF-8 assembler, `printf`'s cells and
//! positional arguments, the Korean grammar rules and `@{…}`, on the synthetic font and a
//! synthetic Hangul table (sample strings written for these tests).

use std::rc::Rc;

use u6_formats::font::{HANGUL_GLYPHS, HangulFont};
use u6_render::hd::{Compositor, HdGlyph, Image, Images, NoImages, Scale, word};
use u6_render::{FLAG_INK, Kind, Src, WIDTH};

use super::text::{TextWindow, draw_char};
use super::*;
use crate::ui::wide::Language;

/// Syllable i's rows: a fixed frame around i, so U+AC00 is not blank and no glyph has eight
/// equal rows (every synthetic `u6.ch` glyph has).
fn rows(i: usize) -> [u8; 8] {
    [0xa5, (i >> 8) as u8, i as u8, 0x5a, 0xff, 0, 0x81, 0x3c]
}

/// The rows of syllable `c`.
fn syl(c: char) -> [u8; 8] {
    rows(c as usize - 0xac00)
}

fn table() -> Rc<HangulFont> {
    Rc::new(HangulFont::from_glyphs((0..HANGUL_GLYPHS).map(rows).collect()).unwrap())
}

/// The test state with the table, `wide` on and the grammar of `language`.
fn wide(language: Language) -> State {
    let mut s = state();
    s.ui.hangul = Some(table());
    s.ui.text.set_wide(true);
    s.ui.text.set_language(language);
    s
}

/// The 8 rows of screen cell (col, row) in ink `fg` (bit 7 = leftmost).
fn glyph(fb: &Framebuffer, col: usize, row: usize, fg: u8) -> [u8; 8] {
    std::array::from_fn(|dy| {
        (0..CELL).fold(0, |c, dx| {
            let on = fb.get(col * CELL + dx, row * CELL + dy) == Some(fg & 0x0f);
            (c << 1) | u8::from(on)
        })
    })
}

fn leak(b: Vec<u8>) -> &'static [u8] {
    Box::leak(b.into_boxed_slice())
}

fn args(v: Vec<Arg<'static>>) -> &'static [Arg<'static>] {
    Box::leak(v.into_boxed_slice())
}

/// A string argument.
fn text(s: &'static str) -> Arg<'static> {
    Arg::Str(s.as_bytes())
}

/// `bytes` through the escape layer byte by byte (as a conversation prints), then a flush.
fn put_bytes(s: State, bytes: &'static [u8]) -> Engine {
    run(s, &[], 4, move |st, io| async move {
        printf::puts(&st, &io, bytes).await;
        print::put_raw(&st, &io, -1).await;
        io.present();
    })
}

/// The scroll's lines so far: the finished ones, then what is drawn and pending of the last.
fn lines(e: &Engine) -> Vec<String> {
    let s = e.state();
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

/// The first scroll line printed by `fmt` with `args` and a newline, in the grammar of `l`.
fn line(l: Language, fmt: &str, args: &'static [Arg<'static>]) -> String {
    bline(l, fmt.as_bytes(), args)
}

/// [`line`] of a format given as bytes (a UTF-8 sequence cut where the test says).
fn bline(l: Language, fmt: &[u8], args: &'static [Arg<'static>]) -> String {
    let fmt = leak([fmt, b"\n"].concat());
    print(wide(l), fmt, args).state().ui.messages[0].clone()
}

#[test]
fn a_syllable_takes_one_cell_in_a_window_a_runic_window_and_the_transcript() {
    let ac00 = rows(0);
    let d7a3 = rows(HANGUL_GLYPHS - 1);
    // Ordinary window: two cells, the third paper; the flush drew them, no newline yet.
    let e = put_bytes(wide(Language::English), "가힣".as_bytes());
    let fb = e.frame();
    assert_eq!(glyph(fb, 22, 14, INK), ac00);
    assert_eq!(glyph(fb, 23, 14, INK), d7a3);
    let s = e.state();
    assert_eq!((s.ui.text.win().col, s.ui.text.line_len), (2, 2));
    assert!(s.ui.messages.is_empty());
    assert_eq!(s.ui.text.transcript_line, [0xac00, 0xd7a3]);
    drop(s);
    // Runic window: the syllables keep their glyphs, the letters around them take the runic
    // ones (code | 0x80).
    let e = put_bytes(wide(Language::English), "<a가힣b>".as_bytes());
    let fb = e.frame();
    assert_eq!(cell(fb, 22, 14, INK), 0xe1);
    assert_eq!(glyph(fb, 23, 14, INK), ac00);
    assert_eq!(glyph(fb, 24, 14, INK), d7a3);
    assert_eq!(cell(fb, 25, 14, INK), 0xe2);
    // The transcript: one char each.
    let e = put_bytes(wide(Language::English), "가힣 x\n".as_bytes());
    assert_eq!(e.state().ui.messages, ["가힣 x"]);
    // A code put straight in (above i16::MAX) is a cell too, and -1 still only flushes.
    let e = run(wide(Language::English), &[], 4, |st, io| async move {
        print::put_raw(&st, &io, 0xac00).await;
        print::put_raw(&st, &io, 0xd7a3).await;
        // Before the flush both wait in the line: the screen and the cursor are untouched.
        {
            let s = st.borrow();
            let t = &s.ui.text;
            assert_eq!((t.line_len, t.line[0], t.line[1]), (2, 0xac00, 0xd7a3));
            assert_eq!((t.win().col, t.win().row), (0, 0));
        }
        let untouched = io.draw(|fb| {
            (22 * CELL..24 * CELL).all(|x| (14 * CELL..15 * CELL).all(|y| fb.get(x, y) == Some(0)))
        });
        assert!(untouched, "nothing drawn before the flush");
        print::put_raw(&st, &io, -1).await;
        print::put_raw(&st, &io, -1).await;
        io.present();
    });
    let fb = e.frame();
    assert_eq!(
        [glyph(fb, 22, 14, INK), glyph(fb, 23, 14, INK)],
        [ac00, d7a3]
    );
    let s = e.state();
    assert_eq!((s.ui.text.win().col, s.ui.text.win().row), (2, 0));
}

#[test]
fn without_a_table_a_syllable_is_a_blank_cell() {
    let mut s = wide(Language::English);
    s.ui.hangul = None;
    // Old ink in every pixel of the two cells first.
    let e = run(s, &[], 4, |st, io| async move {
        io.draw(|fb| {
            for y in 14 * CELL..15 * CELL {
                for x in 23 * CELL..25 * CELL {
                    fb.set(x, y, INK);
                }
            }
        });
        printf::puts(&st, &io, "a힣나b".as_bytes()).await;
        print::put_raw(&st, &io, -1).await;
        io.present();
    });
    let fb = e.frame();
    assert_eq!(cell(fb, 22, 14, INK), b'a');
    // Not glyph 0xa3 or 0x98 of u6.ch (the low bytes of the codes, both with ink in the
    // synthetic font, where glyph 0 has none): paper only.
    for col in [23, 24] {
        for dy in 0..CELL {
            for dx in 0..CELL {
                let px = fb.get(col * CELL + dx, 14 * CELL + dy);
                assert_eq!(px, Some(PAPER), "column {col} ({dx}, {dy})");
            }
        }
    }
    assert_eq!(cell(fb, 25, 14, INK), b'b');
    assert_eq!(e.state().ui.text.win().col, 4);
}

#[test]
fn other_bytes_pass_through_byte_for_byte() {
    let cases: [&[u8]; 6] = [
        &[0x8f],
        &[0xea, 0xb0, b'x'],
        &[0xed, 0x9e, 0xa4],
        "é".as_bytes(),
        "ㄱ".as_bytes(),
        &[0xea, 0xb0],
    ];
    for bytes in cases {
        let e = put_bytes(wide(Language::English), leak(bytes.to_vec()));
        let drawn: Vec<u8> = (0..bytes.len())
            .map(|i| cell(e.frame(), 22 + i, 14, INK))
            .collect();
        assert_eq!(drawn, bytes, "{bytes:02x?}");
        let codes: Vec<u16> = bytes.iter().map(|&b| u16::from(b)).collect();
        assert_eq!(e.state().ui.text.transcript_line, codes, "{bytes:02x?}");
    }
    // A lead after a lead: the first goes out first, the second starts the syllable.
    let e = put_bytes(wide(Language::English), &[0xea, 0xea, 0xb0, 0x80]);
    assert_eq!(cell(e.frame(), 22, 14, INK), 0xea);
    assert_eq!(glyph(e.frame(), 23, 14, INK), rows(0));
}

#[test]
fn wide_on_english_and_latin_1_render_as_with_it_off() {
    // The test font gives every code its own glyph, so a byte drawn as another shows.
    let f = font();
    for (a, ga) in f.glyphs.iter().enumerate() {
        assert!(f.glyphs[a + 1..].iter().all(|gb| gb != ga), "glyph {a:#x}");
    }
    // Every byte but NUL and the key wait, then text that ends inside a sequence.
    let all: Vec<u8> = (1..=0xffu8).filter(|&b| b != b'*' && b != b'&').collect();
    let samples = [
        all.clone(),
        [b"caf".as_slice(), &[0xea]].concat(),
        [b"x".as_slice(), &[0xea, 0xb0]].concat(),
        [b"y ".as_slice(), &[0xed, 0x9e], b"-z".as_slice(), &[0xec]].concat(),
        b"`hello @name. 3 apple\\s a/b".to_vec(),
    ];
    for bytes in samples {
        let bytes = leak(bytes);
        let mut on = state();
        on.ui.text.set_wide(true);
        let (a, b) = (put_bytes(state(), bytes), put_bytes(on, bytes));
        assert!(a.frame().pixels() == b.frame().pixels(), "{bytes:02x?}");
        assert_eq!(lines(&a), lines(&b));
    }
    // A put from outside the escape layer comes after the bytes before it.
    let game = |st: Rc<RefCell<State>>, io: Rc<Io>| async move {
        printf::puts(&st, &io, &[b'a', 0xea]).await;
        print::put_raw(&st, &io, 0x2d).await;
        print::put_raw_n(&st, &io, 2, 0x2b).await;
        printf::puts(&st, &io, &[0xed]).await;
        print::put_raw_str(&st, &io, b"q").await;
        print::put_raw(&st, &io, -1).await;
        io.present();
    };
    let mut on = state();
    on.ui.text.set_wide(true);
    let (a, b) = (run(state(), &[], 4, game), run(on, &[], 4, game));
    assert!(a.frame().pixels() == b.frame().pixels());
    assert_eq!(lines(&b), ["a\u{ea}-++\u{ed}q"]);
}

#[test]
fn printf_pads_by_cells_with_wide_on() {
    assert_eq!(
        line(Language::English, "[%5s]", args(vec![text("가나")])),
        "[   가나]"
    );
    assert_eq!(
        line(Language::English, "[%-4s]", args(vec![text("가a")])),
        "[가a  ]"
    );
    // Off, the field still counts its bytes (three: four of padding) and draws them.
    let e = print(state(), b"[%7s]\n", args(vec![text("가")]));
    assert_eq!(e.state().ui.messages[0], "[    \u{ea}\u{b0}\u{80}]");
}

#[test]
fn positional_arguments_in_either_order_with_wide_on() {
    let a: &[Arg] = &[Arg::Str(b"ab"), Arg::Str(b"cd"), Arg::Word(7)];
    assert_eq!(line(Language::English, "%1$s %2$s", a), "ab cd");
    assert_eq!(line(Language::English, "%2$s %1$s", a), "cd ab");
    assert_eq!(line(Language::English, "%2$s:%3$-3d|%1$s", a), "cd:7  |ab");
    // Off, `%1$s` is width 1 and the directive `$` (as before).
    assert_eq!(
        print(state(), b"%1$s\n", &[Arg::Str(b"ab")])
            .state()
            .ui
            .messages[0],
        "$ s"
    );
    // Positions do not move the others: the next plain directive takes the first argument.
    assert_eq!(line(Language::English, "%2$s %s %s", a), "cd ab cd");
    // Off, `%1$s` is width 1 and the directive `$` (as before).
}

#[test]
fn printf_fields_join_across_a_padding_of_zero() {
    let en = |fmt: &str, a: &'static [Arg<'static>]| line(Language::English, fmt, a);
    let bytes = |b: &'static [u8]| Arg::Str(b);
    // A syllable split across two adjacent fields, across a left-aligned field with no room
    // to pad and the next, and across the format and a field.
    assert_eq!(
        en("%s%s|", args(vec![bytes(&[0xea]), bytes(&[0xb0, 0x80])])),
        "가|"
    );
    assert_eq!(
        en(
            "%-2s%s|",
            args(vec![bytes(&[b'a', 0xea]), bytes(&[0xb0, 0x80])])
        ),
        "a가|"
    );
    let b = |fmt: &[u8], a: &'static [Arg<'static>]| bline(Language::English, fmt, a);
    assert_eq!(b(b"\xea%s|", args(vec![bytes(&[0xb0, 0x80])])), "가|");
    assert_eq!(b(b"%s\x80|", args(vec![bytes(&[0xea, 0xb0])])), "가|");
    // A josa marker split across two fields, and across the format and a field.
    let ko = |fmt: &str, a: &'static [Arg<'static>]| line(Language::Korean, fmt, a);
    assert_eq!(
        ko("검%s%s|", args(vec![text("{이/"), text("가}")])),
        "검이|"
    );
    assert_eq!(ko("검{을%s|", args(vec![text("/를}")])), "검을|");
    // With room to pad the held bytes go out first, as with wide off.
    assert_eq!(
        en(
            "%-3s%s|",
            args(vec![bytes(&[b'a', 0xea]), bytes(&[0xb0, 0x80])])
        ),
        "a\u{ea} \u{b0}\u{80}|"
    );
}

#[test]
fn printf_pads_by_the_cells_a_field_takes_where_it_is_put() {
    let en = |fmt: &str, a: &'static [Arg<'static>]| line(Language::English, fmt, a);
    let ko = |fmt: &str, a: &'static [Arg<'static>]| line(Language::Korean, fmt, a);
    let bytes = |b: &'static [u8]| Arg::Str(b);
    let b = |fmt: &[u8], a: &'static [Arg<'static>]| bline(Language::English, fmt, a);
    // A syllable begun before a left-aligned field and finished in it is the field's.
    assert_eq!(
        b(b"\xea%-3s|", args(vec![bytes(&[0xb0, 0x80, b'x'])])),
        "가x |"
    );
    // Before a right-aligned field with padding the held byte goes out first; the field is
    // two bytes then. At width 2 no padding fits: with none the field finishes the syllable,
    // one cell, and a padding of 1 would let the byte out and leave two bytes, which need none.
    // The smaller is used, so the syllable is whole and the field one cell short of its width
    // (a pad put inside the syllable would show its bytes apart). At width 1 none fits exactly.
    assert_eq!(
        b(b"\xea%4s|", args(vec![bytes(&[0xb0, 0x80])])),
        "\u{ea}  \u{b0}\u{80}|"
    );
    assert_eq!(b(b"\xea%2s|", args(vec![bytes(&[0xb0, 0x80])])), "가|");
    assert_eq!(b(b"\xea%1s|", args(vec![bytes(&[0xb0, 0x80])])), "가|");
    // At width 3 a padding of 1 fits (after it the field is two bytes): it is used, and it
    // lets the lead out first, so the sequence is drawn apart though 0 would keep it whole.
    assert_eq!(
        b(b"\xea%3s|", args(vec![bytes(&[0xb0, 0x80])])),
        "\u{ea} \u{b0}\u{80}|"
    );
    // A field that ends holding bytes: the padding lets them out, and they count.
    assert_eq!(en("%-3s|", args(vec![bytes(&[b'a', 0xea])])), "a\u{ea} |");
    // A particle is one cell after Hangul, the paired form after a Latin name.
    assert_eq!(ko("검%-3s|", args(vec![text("{이/가}")])), "검이  |");
    assert_eq!(ko("Bob%-6s|", args(vec![text("{이/가}")])), "Bob이(가)  |");
    assert_eq!(ko("Bob%-3s|", args(vec![text("{(으)로}")])), "Bob(으)로|");
    // Padding is not text: at the start of a right-aligned field a particle follows the text
    // before the padding, and after a padded field it follows the field.
    assert_eq!(ko("검%3s|", args(vec![text("{이/가}")])), "검  이|");
    assert_eq!(ko("%-4s{을/를}", args(vec![text("검")])), "검   을");
    assert_eq!(ko("%-4s{을/를}", args(vec![text("나무")])), "나무  를");
    // A field that finishes a marker held from the format.
    assert_eq!(ko("검{이%-3s|", args(vec![text("/가}")])), "검이  |");
    // Escapes in a field count as themselves (as its bytes do with wide off): `@{가}` is four.
    assert_eq!(en("[%5s]", args(vec![text("@{가}")])), "[ 가]");
}

#[test]
fn josa_markers_follow_the_last_cell() {
    let ko = |fmt: &str| line(Language::Korean, fmt, &[]);
    let markers = ["{이/가}", "{은/는}", "{을/를}", "{과/와}", "{(으)로}"];
    let each = |word: &str| markers.map(|m| ko(&format!("{word}{m}")));
    assert_eq!(
        each("검"),
        ["검이", "검은", "검을", "검과", "검으로"],
        "final"
    );
    assert_eq!(each("칼"), ["칼이", "칼은", "칼을", "칼과", "칼로"], "ㄹ");
    assert_eq!(
        each("나무"),
        ["나무가", "나무는", "나무를", "나무와", "나무로"]
    );
    let name = |m: &str| line(Language::Korean, &format!("%s{m}"), &[Arg::Str(b"Bob")]);
    assert_eq!(
        markers.map(name),
        [
            "Bob이(가)",
            "Bob은(는)",
            "Bob을(를)",
            "Bob과(와)",
            "Bob(으)로"
        ],
        "a Latin name"
    );
    assert_eq!(ko("!{을/를}"), "!을(를)");
    // After a number, its Korean reading: 영 일 이 삼 육, 십 백 천 만.
    let number = |n: u16| {
        markers.map(|m| {
            line(
                Language::Korean,
                &format!("%d{m}"),
                args(vec![Arg::Word(n)]),
            )
        })
    };
    assert_eq!(number(0), ["0이", "0은", "0을", "0과", "0으로"], "영");
    assert_eq!(number(1), ["1이", "1은", "1을", "1과", "1로"], "일");
    assert_eq!(number(2), ["2가", "2는", "2를", "2와", "2로"], "이");
    assert_eq!(number(3), ["3이", "3은", "3을", "3과", "3으로"], "삼");
    assert_eq!(number(6), ["6이", "6은", "6을", "6과", "6으로"], "육");
    assert_eq!(number(10), ["10이", "10은", "10을", "10과", "10으로"], "십");
    assert_eq!(
        number(100),
        ["100이", "100은", "100을", "100과", "100으로"],
        "백"
    );
    assert_eq!(
        number(1000),
        ["1000이", "1000은", "1000을", "1000과", "1000으로"],
        "천"
    );
    assert_eq!(
        number(10000),
        ["10000이", "10000은", "10000을", "10000과", "10000으로"],
        "만"
    );
    // `#n` prints its number through the line too.
    let mut s = wide(Language::Korean);
    s.converse.ints[3] = 2;
    assert_eq!(
        print(s, b"#3{\xec\x9d\xb4/\xea\xb0\x80}\n", &[])
            .state()
            .ui
            .messages[0],
        "2가"
    );
    // Any other brace passes, and in English every brace does (a marker's `/` is still the
    // plural escape there; the one without passes whole).
    assert_eq!(ko("검{이}{x} {이:나}"), "검{이}{x} {이:나}");
    assert_eq!(line(Language::English, "검{(으)로}", &[]), "검{(으)로}");
}

#[test]
fn korean_drops_the_plural_letters_and_english_keeps_them() {
    let one: &[Arg] = &[Arg::Word(1)];
    let three: &[Arg] = &[Arg::Word(3)];
    // `/` and `\\` print nothing, nor the letters after them; the next other character prints.
    assert_eq!(line(Language::Korean, "%d apple\\s.", three), "3 apple.");
    assert_eq!(line(Language::Korean, "%d m/ouse\\ice!", one), "1 m!");
    assert_eq!(line(Language::Korean, "%d m/ouse\\ice!", three), "3 m!");
    assert_eq!(line(Language::Korean, "%d 사과\\s개", three), "3 사과개");
    assert_eq!(line(Language::English, "%d m/ouse\\ice!", one), "1 mouse!");
    assert_eq!(line(Language::English, "%d m/ouse\\ice!", three), "3 mice!");
}

#[test]
fn a_brace_highlight_with_wide_on() {
    let e = print(wide(Language::Korean), "a @{말 두} b @c. d".as_bytes(), &[]);
    let fb = e.frame();
    // The braces print nothing; the text between them, its space too, in the alert colour.
    assert_eq!(cell(fb, 22, 14, INK), b'a');
    assert_eq!(glyph(fb, 24, 14, ALERT), syl('말'));
    assert_eq!(cell(fb, 25, 14, ALERT), b' ', "the space too");
    assert_eq!(glyph(fb, 26, 14, ALERT), syl('두'));
    assert_eq!(row_text(fb, 27..30, 14, INK), " b ");
    // Plain `@` as before: to the end set.
    assert_eq!(cell(fb, 30, 14, ALERT), b'c');
    assert_eq!(row_text(fb, 31..34, 14, INK), ". d");
    // A braced phrase keeps the colour past a wrapped row, past the flush of a key wait (`*`)
    // and past a `#n` inside it.
    let e = print(wide(Language::English), b"@{aaaa bbbb cccc dddd} e", &[]);
    assert_eq!(row_text(e.frame(), 22..36, 14, ALERT), "aaaa bbbb cccc");
    assert_eq!(row_text(e.frame(), 22..26, 15, ALERT), "dddd");
    assert_eq!(row_text(e.frame(), 26..28, 15, INK), " e");
    let e = run(
        wide(Language::English),
        &[Key::Char('a')],
        6,
        |st, io| async move {
            printf(&st, &io, b"@{ab*#1d} e", &[]).await;
            print::put_raw(&st, &io, -1).await;
            io.present();
        },
    );
    assert!(e.finished(), "the key ended the wait");
    assert_eq!(row_text(e.frame(), 22..26, 14, ALERT), "ab0d");
    assert_eq!(row_text(e.frame(), 26..28, 14, INK), " e");
    // Off, `@{` is the plain keyword with its brace.
    let e = print(state(), b"@{x} y", &[]);
    assert_eq!(row_text(e.frame(), 22..25, 14, ALERT), "{x}");
}

#[test]
fn a_brace_left_open_closes_where_the_printf_format_ends() {
    // `@{ab` with no `}`: the brace closes at the end of its format, the text ink comes back,
    // and a `}` in the next format is a character like any other.
    let e = run(wide(Language::English), &[], 4, |st, io| async move {
        printf(&st, &io, b"@{ab", &[]).await;
        {
            let t = &st.borrow().ui.text;
            assert!(!t.brace, "the brace is closed");
            assert_eq!(t.win().fg, INK, "the text ink");
        }
        printf(&st, &io, b"c}d", &[]).await;
        print::put_raw(&st, &io, -1).await;
        io.present();
    });
    assert_eq!(row_text(e.frame(), 22..24, 14, ALERT), "ab");
    assert_eq!(row_text(e.frame(), 24..27, 14, INK), "c}d");
    assert_eq!(lines(&e), ["abc}d"]);
    // Opened in a field, it closes at the end of the format the field is in.
    let e = run(wide(Language::English), &[], 4, |st, io| async move {
        printf(&st, &io, b"%s-", &[Arg::Str(b"@{x")]).await;
        printf(&st, &io, b"y", &[]).await;
        print::put_raw(&st, &io, -1).await;
        io.present();
    });
    assert_eq!(row_text(e.frame(), 22..24, 14, ALERT), "x-");
    assert_eq!(row_text(e.frame(), 24..25, 14, INK), "y");
    // Off, `@{` is no brace (the plain keyword, as before), and nothing is closed.
    let e = run(state(), &[], 4, |st, io| async move {
        printf(&st, &io, b"@{ab", &[]).await;
        printf(&st, &io, b"c d", &[]).await;
        print::put_raw(&st, &io, -1).await;
        io.present();
    });
    assert_eq!(row_text(e.frame(), 22..26, 14, ALERT), "{abc");
    assert_eq!(row_text(e.frame(), 26..28, 14, INK), " d");
}

#[test]
fn a_brace_is_closed_by_no_brace_in_hidden_text() {
    // Spec 08 §1: hidden text is dropped before the escape layer reads it, so a `}` in it does
    // not close the brace: `@{a&}&b` put a byte at a time leaves it open, `a` and `b` alert.
    let e = run(wide(Language::English), &[], 4, |st, io| async move {
        printf::puts(&st, &io, b"@{a&}&b").await;
        assert!(st.borrow().ui.text.brace, "still open after the hidden }}");
        printf::puts(&st, &io, b"}c").await;
        print::put_raw(&st, &io, -1).await;
        io.present();
    });
    assert_eq!(row_text(e.frame(), 22..24, 14, ALERT), "ab");
    assert_eq!(row_text(e.frame(), 24..25, 14, INK), "c");
    assert_eq!(lines(&e), ["abc"]);
}

#[test]
fn a_printf_leaves_open_a_brace_it_did_not_open() {
    // `@{ab` put a byte at a time, a `printf`, then `ef}`: the brace is not the format's, so its
    // end leaves it open; `cd` and `ef` keep the alert colour and the `}` closes it, printing
    // nothing.
    let e = run(wide(Language::English), &[], 4, |st, io| async move {
        printf::puts(&st, &io, b"@{ab").await;
        printf(&st, &io, b"cd", &[]).await;
        {
            let t = &st.borrow().ui.text;
            assert!(t.brace, "the brace is still open");
            assert_eq!(t.win().fg, ALERT, "the alert colour");
        }
        printf::puts(&st, &io, b"ef}g").await;
        print::put_raw(&st, &io, -1).await;
        io.present();
    });
    assert_eq!(row_text(e.frame(), 22..28, 14, ALERT), "abcdef");
    assert_eq!(row_text(e.frame(), 28..29, 14, INK), "g");
    assert_eq!(lines(&e), ["abcdefg"]);
    // A brace the format opens is the format's, after a `}` of its own closed the one before
    // (`c}d@{e`) and when its `@{` comes with that one still open (`c@{de`): it is closed where
    // the format ends, and the `}` after it prints.
    let cases: [(&[u8], &str, &str); 2] = [
        (b"c}d@{e", "abc e  ", "   d f}"),
        (b"c@{de", "abcde  ", "     f}"),
    ];
    for (fmt, alert, ink) in cases {
        let e = run(wide(Language::English), &[], 4, move |st, io| async move {
            printf::puts(&st, &io, b"@{ab").await;
            printf(&st, &io, fmt, &[]).await;
            assert!(!st.borrow().ui.text.brace, "{fmt:?}: closed");
            printf::puts(&st, &io, b"f}").await;
            print::put_raw(&st, &io, -1).await;
            io.present();
        });
        let fb = e.frame();
        assert_eq!(row_text(fb, 22..29, 14, ALERT), alert, "{fmt:?}");
        assert_eq!(row_text(fb, 22..29, 14, INK), ink, "{fmt:?}");
        assert_eq!(lines(&e), ["abcdef}"], "{fmt:?}");
    }
}

#[test]
fn a_unit_of_text_lets_out_what_it_holds_where_it_ends() {
    // A format that ends inside a josa marker: where it ends the held codes are let out as they
    // are, in its ink (the brace's here, before the brace closes), so the next format does not
    // finish the marker; it starts with nothing held.
    let e = run(wide(Language::Korean), &[], 4, |st, io| async move {
        printf(&st, &io, "@{검{(으".as_bytes(), &[]).await;
        {
            let t = &st.borrow().ui.text;
            assert!(t.josa.is_empty() && t.utf8.is_empty(), "nothing held");
            assert!(!t.brace, "the brace is closed");
        }
        printf(&st, &io, ")로} x".as_bytes(), &[]).await;
        print::put_raw(&st, &io, -1).await;
        io.present();
    });
    assert_eq!(lines(&e), ["검{(으)로} x"]);
    let fb = e.frame();
    assert_eq!(glyph(fb, 22, 14, ALERT), syl('검'));
    assert_eq!(row_text(fb, 23..25, 14, ALERT), "{(");
    assert_eq!(glyph(fb, 25, 14, ALERT), syl('으'));
    assert_eq!(cell(fb, 26, 14, INK), b')');
    assert_eq!(glyph(fb, 27, 14, INK), syl('로'));
    assert_eq!(row_text(fb, 28..31, 14, INK), "} x");
    // A format that ends after a UTF-8 lead: the lead is let out as itself in its ink, and the
    // next format's continuation bytes are bytes, not the rest of a syllable.
    let e = run(wide(Language::English), &[], 4, |st, io| async move {
        printf(&st, &io, b"@{a\xea", &[]).await;
        {
            let t = &st.borrow().ui.text;
            assert!(t.josa.is_empty() && t.utf8.is_empty(), "nothing held");
        }
        printf(&st, &io, b"\xb0\x80b", &[]).await;
        print::put_raw(&st, &io, -1).await;
        io.present();
    });
    assert_eq!(
        e.state().ui.text.transcript_line,
        [0x61, 0xea, 0xb0, 0x80, 0x62]
    );
    let fb = e.frame();
    assert_eq!(
        [cell(fb, 22, 14, ALERT), cell(fb, 23, 14, ALERT)],
        [b'a', 0xea]
    );
    let ink: Vec<u8> = (24..27).map(|col| cell(fb, col, 14, INK)).collect();
    assert_eq!(ink, [0xb0, 0x80, b'b']);
}

#[test]
fn an_at_that_ends_a_unit_opens_no_brace_in_the_next() {
    // `x@` ends one format and `{y` starts the next: the end of the first ends the `@`'s claim,
    // so the `{` is the `@` keyword's, as with wide off, and every frame and line is the same.
    let game = |st: Rc<RefCell<State>>, io: Rc<Io>| async move {
        printf(&st, &io, b"x@", &[]).await;
        printf(&st, &io, b"{y", &[]).await;
        printf(&st, &io, b"z}w", &[]).await;
        print::put_raw(&st, &io, -1).await;
        io.present();
    };
    let (on, off) = (
        run(wide(Language::English), &[], 4, game),
        run(state(), &[], 4, game),
    );
    assert_eq!(on.state().ui.text.braces, 0, "no brace opened");
    assert!(on.frame().pixels() == off.frame().pixels());
    assert_eq!(lines(&on), ["x{yz}w"]);
    assert_eq!(row_text(on.frame(), 23..28, 14, ALERT), "{yz}w");
    // In Korean the `{` after the unit's end is no brace's, so it starts a marker, which resolves.
    let e = run(wide(Language::Korean), &[], 4, |st, io| async move {
        printf(&st, &io, "나@".as_bytes(), &[]).await;
        printf(&st, &io, "{(으)로}".as_bytes(), &[]).await;
        print::put_raw(&st, &io, -1).await;
        io.present();
    });
    assert_eq!(lines(&e), ["나로"], "the marker resolves: no brace");
}

#[test]
fn the_brace_of_at_brace_comes_before_a_josa_marker() {
    // With wide on, the `{` right after `@` opens the brace before the josa resolver can take
    // it, so the braced text is plain text: `@{(으)로}` puts `(으)로` itself in the alert colour,
    // where the same marker without the `@` resolves.
    let e = print(
        wide(Language::Korean),
        "나@{(으)로} 다{(으)로}".as_bytes(),
        &[],
    );
    let fb = e.frame();
    assert_eq!(glyph(fb, 22, 14, INK), syl('나'));
    assert_eq!(cell(fb, 23, 14, ALERT), b'(');
    assert_eq!(glyph(fb, 24, 14, ALERT), syl('으'));
    assert_eq!(cell(fb, 25, 14, ALERT), b')');
    assert_eq!(glyph(fb, 26, 14, ALERT), syl('로'));
    assert_eq!(cell(fb, 27, 14, INK), b' ');
    assert_eq!(glyph(fb, 28, 14, INK), syl('다'));
    assert_eq!(glyph(fb, 29, 14, INK), syl('로'));
    assert_eq!(lines(&e), ["나(으)로 다로"]);
    let ko = |fmt: &str, a: &'static [Arg<'static>]| line(Language::Korean, fmt, a);
    // The text is text as anywhere else: a marker later in it resolves, and a `/` is still the
    // plural escape (in Korean it prints nothing).
    assert_eq!(ko("@{다{(으)로}}", &[]), "다로");
    assert_eq!(ko("@{이/가}", &[]), "이가");
    // An `@` at the end of the format and the `{` at the start of a field: the same.
    assert_eq!(ko("나@%s|", args(vec![text("{(으)로}")])), "나(으)로|");
    // Only the `{` right after `@` opens a brace: later in an `@` keyword it is the keyword's,
    // and a marker there resolves.
    let e = print(wide(Language::English), b"@ab{x} y", &[]);
    assert_eq!(row_text(e.frame(), 22..27, 14, ALERT), "ab{x}");
    assert_eq!(row_text(e.frame(), 27..29, 14, INK), " y");
    assert_eq!(ko("@다{(으)로} 나", &[]), "다로 나");
}

#[test]
fn a_josa_marker_right_after_the_closing_brace_follows_the_braced_text() {
    // Spec 08 §4's use: `@{…}` marks a word, and a particle right after its `}` follows the
    // word's last syllable (the braces are no cell), in the text ink: after a final consonant,
    // after none, and after ㄹ with `{(으)로}`.
    let cases = [
        ("@{검}{을/를} x", '검', '을', "검을 x"),
        ("@{나}{을/를} x", '나', '를', "나를 x"),
        ("@{달}{(으)로} x", '달', '로', "달로 x"),
    ];
    for (fmt, word, particle, want) in cases {
        let e = print(wide(Language::Korean), leak(fmt.as_bytes().to_vec()), &[]);
        let fb = e.frame();
        assert_eq!(glyph(fb, 22, 14, ALERT), syl(word), "{fmt}");
        assert_eq!(glyph(fb, 23, 14, INK), syl(particle), "{fmt}");
        assert_eq!(row_text(fb, 24..26, 14, INK), " x", "{fmt}");
        assert_eq!(lines(&e), [want], "{fmt}");
    }
}

#[test]
fn a_field_with_at_brace_counts_the_brace_text_as_its_cells() {
    // `@{(으)로}` is seven cells, each code as itself (the `{` is the brace's, not a marker's),
    // so `%8s` pads it by one; were the `{` a marker's it would be five, padded by three.
    let ko = |fmt: &str, a: &'static [Arg<'static>]| line(Language::Korean, fmt, a);
    assert_eq!(ko("[%8s]", args(vec![text("@{(으)로}")])), "[ (으)로]");
    assert_eq!(ko("[%-8s]", args(vec![text("@{(으)로}")])), "[(으)로 ]");
    // An `@` the format put just before the field: the field's `{` is the brace's too, so
    // `{(으)로}` is six cells and `%-5s` pads nothing.
    assert_eq!(ko("@%-5s|", args(vec![text("{(으)로}")])), "(으)로|");
    assert_eq!(ko("@%-7s|", args(vec![text("{(으)로}")])), "(으)로 |");
    // The same field after any other code is a marker: one form, so padded.
    assert_eq!(ko("나%-3s|", args(vec![text("{(으)로}")])), "나로  |");
}

#[test]
fn a_put_between_at_and_its_brace_or_a_hidden_at_opens_none() {
    // A put from outside the escape layer between `@` and `{` (padding, a directive put as it
    // is, a flush, a string put unescaped) ends the `@`: the `{` is the `@` keyword's, as with
    // wide off, and draws the same.
    let game = |st: Rc<RefCell<State>>, io: Rc<Io>| async move {
        printf(&st, &io, b"a@%5s|\n", &[Arg::Str(b"{x}")]).await;
        printf(&st, &io, b"a@%%{x}|\n", &[]).await;
        printf::puts(&st, &io, b"a@").await;
        print::put_raw(&st, &io, -1).await;
        printf::puts(&st, &io, b"{x}|\n").await;
        printf::puts(&st, &io, b"a@").await;
        print::put_raw_str(&st, &io, b"-").await;
        printf::puts(&st, &io, b"{x}|\n").await;
        io.present();
    };
    let mut on = state();
    on.ui.text.set_wide(true);
    let (a, b) = (run(state(), &[], 4, game), run(on, &[], 4, game));
    assert!(a.frame().pixels() == b.frame().pixels());
    assert_eq!(lines(&b), ["a  {x}|", "a%{x}|", "a{x}|", "a-{x}|", ""]);
    // So a marker after the padding resolves, and the field is counted that way: one cell.
    let ko = |fmt: &str, a: &'static [Arg<'static>]| line(Language::Korean, fmt, a);
    assert_eq!(ko("나@%7s|", args(vec![text("{(으)로}")])), "나      로|");
    // An `@` in hidden text that is not shown is dropped and opens nothing, and the cells of
    // the field are counted that way (each code the escape layer takes is one, as before): the
    // marker after it resolves to its paired form after the Latin name, seven cells in all.
    assert_eq!(ko("Bob%-8s|", args(vec![text("&@{이/가}&")])), "Bob |");
    // The same when the format opened the hidden text.
    assert_eq!(ko("Bob&%-7s|", args(vec![text("@{이/가}&")])), "Bob |");
    // Shown, the `@` opens the brace, eight cells; `이가` drawn.
    let mut s = wide(Language::Korean);
    s.actors.objlist_tail[0x5e] = 1;
    let e = print(s, "Bob%-8s|\n".as_bytes(), args(vec![text("&@{이/가}&")]));
    assert_eq!(e.state().ui.messages[0], "Bob이가|");
    // Text put while text output is off reaches nothing and so ends nothing: the `{` after it
    // still opens the brace.
    let e = run(wide(Language::English), &[], 4, |st, io| async move {
        printf::puts(&st, &io, b"a@").await;
        st.borrow_mut().ui.text.on = false;
        printf::puts(&st, &io, b"x").await;
        print::put_raw(&st, &io, i32::from(b'y')).await;
        printf(&st, &io, b"%3s", &[Arg::Str(b"z")]).await;
        st.borrow_mut().ui.text.on = true;
        printf::puts(&st, &io, b"{w}|").await;
        print::put_raw(&st, &io, -1).await;
        io.present();
    });
    assert_eq!(lines(&e), ["aw|"]);
    assert_eq!(cell(e.frame(), 23, 14, ALERT), b'w');
}

#[test]
fn a_marker_in_a_field_follows_the_cell_drawn_before_it() {
    let ko = |fmt: &str, a: &'static [Arg<'static>]| line(Language::Korean, fmt, a);
    // A field's escapes count as cells for its width (spec 08 §1, as its bytes do with wide
    // off), but the josa resolver's last cell is the one the engine drew: escapes, hidden text
    // and braces draw none, so a marker after them follows the cell before them and takes as
    // many cells as without them. With the width larger by the escapes' own cells, the padding
    // is the same.
    let plain = ko("검%-5s|", args(vec![text("{이/가}")]));
    assert_eq!(plain, "검이    |");
    assert_eq!(ko("검%-7s|", args(vec![text("&&{이/가}")])), plain);
    assert_eq!(ko("검%-6s|", args(vec![text(">{이/가}")])), plain);
    assert_eq!(ko("검%-6s|", args(vec![text("`{이/가}")])), plain);
    // Hidden text draws nothing: the marker follows 나, not the hidden 검.
    assert_eq!(ko("나%-6s|", args(vec![text("&검&{(으)로}")])), "나로  |");
    // The braced word of spec 08 §4 in a field: after its `}`, 으로 follows 검 (two cells).
    assert_eq!(ko("%-8s|", args(vec![text("@{검}{(으)로}")])), "검으로  |");
    // `#n` and `$n` leave what they print: 2 (이, so 로) and 검 (so 으로), not the index.
    let mut s = wide(Language::Korean);
    s.converse.ints[3] = 2;
    s.converse.strs[1] = crate::converse::StrRef::Buffer(1);
    s.converse.buffers[1] = "검".as_bytes().to_vec();
    let fields = args(vec![text("#3{(으)로}"), text("$1{(으)로}")]);
    let e = print(s, b"%-5s|%-6s|\n", fields);
    assert_eq!(e.state().ui.messages[0], "2로  |검으로  |");
    // A digit in a runic window draws its runic glyph, no digit to the resolver: the paired
    // form, its parentheses runic too.
    assert_eq!(
        ko("%-8s|", args(vec![text("<3{이/가}>")])),
        "\u{b3}이\u{a8}가\u{a9} |"
    );
}

#[test]
fn a_space_the_start_of_a_wrapped_row_drops_is_no_cell_of_the_tail() {
    // The 17-cell scroll wraps: 가's padding runs past the end of the row and wraps it at a
    // space, the row after drops the rest of the padding and the next field's leading space,
    // and the padding leaves the tail on 가. So the marker follows 가, two cells in all (the
    // space and 가), and the field is padded by six.
    let fmt = b"%-20s%-8s|\n";
    let e = print(
        wide(Language::Korean),
        fmt,
        args(vec![text("가"), text(" {이/가}")]),
    );
    assert_eq!(
        e.state().ui.messages,
        [format!("가{}", " ".repeat(16)), "가      |".to_string()]
    );
    // After a cell of the row the space is put, and the marker follows it; at the start of the
    // scroll's first row (nothing put yet) it is dropped too.
    let ko = |fmt: &str, a: &'static [Arg<'static>]| line(Language::Korean, fmt, a);
    assert_eq!(ko("가%-8s|", args(vec![text(" {이/가}")])), "가 이(가)   |");
    assert_eq!(ko("%-8s|", args(vec![text(" {이/가}")])), "이(가)   |");
    // A right-aligned field after its own padding that wraps the row: counted after that
    // padding, its leading space dropped; what is drawn after the wrap is the field.
    let e = print(
        wide(Language::Korean),
        "가%25s|\n".as_bytes(),
        args(vec![text(" {이/가}")]),
    );
    assert_eq!(e.state().ui.messages[1], "가|");
}

#[test]
fn a_field_whose_held_end_the_format_completes_is_drawn_other_than_its_count_either_way() {
    // The field's end is held and the text after it in the format completes it: the field is
    // counted as if the held end were let out, one cell per code, and is drawn as the format
    // completes it, fewer cells, as many or more. Not well formed (the translation tool refuses
    // such a string); the engine draws this, deterministically. An `x` comes first, so the
    // scroll's row has begun and keeps the pad (the start of a row drops it).
    //
    // A two-byte lead: counted as two cells, so `%3s` pads by one and `%-2s` by none; the
    // format's byte completes the syllable, one cell.
    let en = |fmt: &[u8], a: &'static [Arg<'static>]| bline(Language::English, fmt, a);
    let lead = |b: &'static [u8]| args(vec![Arg::Str(b)]);
    assert_eq!(en(b"x%3s\x80|", lead(&[0xea, 0xb0])), "x 가|");
    assert_eq!(en(b"x%-2s\x80|", lead(&[0xea, 0xb0])), "x가|");
    // A one-byte lead: counted as one cell, so `%3s` pads by two; the format's two bytes
    // complete the syllable, one cell, as counted.
    assert_eq!(en(b"x%3s\xb0\x80|", lead(&[0xea])), "x  가|");
    // A josa marker the field begins and the format finishes: counted as its two codes, so `%3s`
    // pads by one; the format's `/가}` completes it, and after 검 it is one cell, 이.
    let ko = |fmt: &str, a: &'static [Arg<'static>]| line(Language::Korean, fmt, a);
    assert_eq!(ko("검%3s/가}|", args(vec![text("{이")])), "검 이|");
    assert_eq!(ko("검%-2s/가}|", args(vec![text("{이")])), "검이|");
    // After `x`, no syllable, it takes its paired form: four cells, more than counted.
    assert_eq!(ko("x%3s/가}|", args(vec![text("{이")])), "x 이(가)|");
    // With room to pad after a left-aligned field, the padding lets the held end out first, so
    // nothing completes it.
    assert_eq!(
        en(b"%-3s\x80|", lead(&[0xea, 0xb0])),
        "\u{ea}\u{b0} \u{80}|"
    );
}

/// The cell codes `fmt` with `args` puts on the scroll's first row (flushed, no newline),
/// checked against the frame: each drawn cell is the glyph of its code, the cell after them
/// paper.
fn drawn(s: State, fmt: &'static [u8], args: &'static [Arg<'static>]) -> Vec<u16> {
    let e = print(s, fmt, args);
    let codes = e.state().ui.text.transcript_line.clone();
    let fb = e.frame();
    for (i, &c) in codes.iter().enumerate() {
        let want = match u8::try_from(c) {
            Ok(b) => [b; 8],
            Err(_) => syl(char::from_u32(u32::from(c)).unwrap()),
        };
        assert_eq!(glyph(fb, 22 + i, 14, INK), want, "cell {i} of {fmt:02x?}");
    }
    assert_eq!(glyph(fb, 22 + codes.len(), 14, INK), [0; 8], "{fmt:02x?}");
    codes
}

/// The cell codes of `s`: a char below U+0100 as its byte, a Hangul syllable as its code.
fn codes(s: &str) -> Vec<u16> {
    s.chars()
        .map(|c| u16::try_from(u32::from(c)).unwrap())
        .collect()
}

/// A `printf` case: the format, its arguments, the grammar, the cells with wide on, the cells
/// with wide off.
type Case = (
    &'static str,
    Vec<Arg<'static>>,
    Language,
    &'static str,
    &'static [u8],
);

#[test]
fn printf_fields_join_at_zero_and_negative_padding_and_wide_off_draws_bytes() {
    let b = |v: &'static [u8]| Arg::Str(v);
    let t = |v: &'static str| Arg::Str(v.as_bytes());
    let (ea, b080, aea, eab0, x80) = (
        b(&[0xea]),
        b(&[0xb0, 0x80]),
        b(&[b'a', 0xea]),
        b(&[0xea, 0xb0]),
        b(&[0x80]),
    );
    let (open, close) = (t("{(으"), t(")로}"));
    let en = Language::English;
    let ko = Language::Korean;
    // (format, arguments, grammar, the cells with wide on, the cells with wide off). Off is
    // main's printf: each byte a cell, a field padded by its width less its bytes, never by
    // less than nothing.
    #[rustfmt::skip]
    let cases: [Case; 18] = [
        // A syllable split across adjacent fields: no width, then left-aligned with a padding
        // of 0 and of -1, after one byte and after two.
        ("%s%s|", vec![ea, b080], en, "가|", b"\xea\xb0\x80|"),
        ("%-1s%s|", vec![ea, b080], en, "가|", b"\xea\xb0\x80|"),
        ("%-1s%s|", vec![aea, b080], en, "a가|", b"a\xea\xb0\x80|"),
        ("%-2s%s|", vec![eab0, x80], en, "가|", b"\xea\xb0\x80|"),
        // Right-aligned, a padding of 0 and of -1 (both fields), and a right-aligned field
        // after a held byte with a padding of 0 (nothing is let out, the field finishes it);
        // at width 2 no padding fits (finished, the field is one cell; padded by one, its two
        // bytes need none), and the smaller, none, is used: the field finishes the syllable,
        // one cell short of its width.
        ("%1s%s|", vec![ea, b080], en, "가|", b"\xea\xb0\x80|"),
        ("%1s%1s|", vec![aea, b080], en, "a가|", b"a\xea\xb0\x80|"),
        ("%-1s%1s|", vec![ea, b080], en, "가|", b"\xea\xb0\x80|"),
        ("%-1s%2s|", vec![ea, b080], en, "가|", b"\xea\xb0\x80|"),
        // A padding above 0 lets the held byte out first, left- and right-aligned; the `0` flag.
        ("%-2s%s|", vec![ea, b080], en, "\u{ea} \u{b0}\u{80}|", b"\xea \xb0\x80|"),
        ("%-1s%3s|", vec![ea, b080], en, "\u{ea} \u{b0}\u{80}|", b"\xea \xb0\x80|"),
        ("%04s|", vec![b(&[0xea, 0xb0, 0x80])], en, "000가|", b"0\xea\xb0\x80|"),
        // A josa marker split across adjacent fields, after an open syllable, a closed one and
        // one in ㄹ: no width, left-aligned with a padding of 0 and of -2, right-aligned with
        // a padding of 0 and of -1 (both fields).
        ("나%s%s|", vec![open, close], ko, "나로|", "나{(으)로}|".as_bytes()),
        ("각%-3s%s|", vec![open, close], ko, "각으로|", "각{(으)로}|".as_bytes()),
        ("각%-1s%s|", vec![open, close], ko, "각으로|", "각{(으)로}|".as_bytes()),
        ("달%3s%s|", vec![open, close], ko, "달로|", "달{(으)로}|".as_bytes()),
        ("나%1s%1s|", vec![open, close], ko, "나로|", "나{(으)로}|".as_bytes()),
        // A padding above 0 lets the held codes out as they are, left- and right-aligned.
        ("나%-4s%s|", vec![open, close], ko, "나{(으 )로}|", "나{(으)로}|".as_bytes()),
        ("각%-1s%5s|", vec![open, close], ko, "각{(으  )로}|", "각{(으)로}|".as_bytes()),
    ];
    for (fmt, a, language, on, off) in cases {
        let (f, a) = (leak(fmt.as_bytes().to_vec()), args(a));
        assert_eq!(
            drawn(wide(language), f, a),
            codes(on),
            "{fmt} {a:02x?} wide on"
        );
        let main: Vec<u16> = off.iter().map(|&b| u16::from(b)).collect();
        assert_eq!(drawn(state(), f, a), main, "{fmt} {a:02x?} wide off");
    }
}

#[test]
fn the_committed_table_parses_and_draws() {
    let blob = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../site/fonts/galmuri7-hangul.bin"
    ));
    let t = HangulFont::parse(blob).unwrap();
    assert_eq!(t.glyphs().len(), 11_172);
    // Every syllable has ink in rows 0..=6 only and in columns 0..=6 only.
    assert!(t.glyphs().iter().all(|g| g.iter().any(|&r| r != 0)));
    assert!(
        t.glyphs()
            .iter()
            .all(|g| g[7] == 0 && g.iter().all(|&r| r & 1 == 0))
    );
    let mut s = state();
    s.ui.hangul = Some(Rc::new(t));
    s.ui.text.set_wide(true);
    let e = put_bytes(s, "가".as_bytes());
    let g = glyph(e.frame(), 22, 14, INK);
    assert_eq!(
        &g,
        e.state().ui.hangul.as_ref().unwrap().glyph(0xac00).unwrap()
    );
}

/// One 16 × 16 tile record (id 7) of colour 5.
struct Tile(Vec<u8>);

impl Images for Tile {
    fn image(&mut self, kind: Kind, id: u16) -> Option<Image<'_>> {
        (kind == Kind::Tile && id == 7).then(|| Image {
            ver: 0,
            width: 16,
            height: 16,
            pixels: &self.0,
            transparent: None,
        })
    }
}

#[test]
fn a_hangul_cell_composes_as_a_latin_cell_does_at_2x_and_4x() {
    let mut fb = Framebuffer::new();
    fb.enable_src();
    for (i, c) in fb.palette.iter_mut().enumerate().take(16) {
        let i = i as u8;
        *c = u6_render::Rgb::new(i * 16, 255 - i * 8, i);
    }
    // A tile under both cells first: its pixels name the record.
    for y in 16..32 {
        for x in 16..48 {
            let src = Src {
                kind: Kind::Tile,
                id: 7,
                u: (x % 16) as u16,
                v: (y % 16) as u16,
                ..Src::CLASSIC
            };
            fb.set_src(x, y, 5, src);
        }
    }
    let w = TextWindow::new(0, 0, 39, 24, INK, PAPER, 0);
    let (font, t) = (font(), table());
    draw_char(&mut fb, Some(&font), Some(&t), &w, 0xac00, 3, 2);
    draw_char(&mut fb, Some(&font), Some(&t), &w, u16::from(b'A'), 4, 2);
    let cells = [(3usize, "Hangul"), (4, "Latin")];
    // Spec 11: each pixel names its glyph (the cell's code, its place, the ink flag where set).
    for ((col, what), code) in cells.into_iter().zip([0xac00, u16::from(b'A')]) {
        for y in 16..24 {
            for x in col * 8..col * 8 + 8 {
                let ink = fb.get(x, y) == Some(INK);
                let want = Src {
                    id: code,
                    u: (x - col * 8) as u16,
                    v: (y - 16) as u16,
                    kind: Kind::Glyph,
                    flags: if ink { FLAG_INK } else { 0 },
                    ..Src::CLASSIC
                };
                assert_eq!(fb.src_at(x, y), Some(want), "{what} ({x}, {y})");
            }
        }
    }
    for scale in [Scale::X2, Scale::X4] {
        let mut c = Compositor::new();
        c.compose(&fb, &mut Tile(vec![5; 256]), scale);
        let n = scale.factor();
        for (col, what) in cells {
            for y in 16..24 {
                for x in col * 8..col * 8 + 8 {
                    let want = word(fb.palette[usize::from(fb.get(x, y).unwrap())]);
                    for sy in 0..n {
                        for sx in 0..n {
                            let got = c.rgba()[(y * n + sy) * WIDTH * n + x * n + sx];
                            assert_eq!(got, want, "{what} ({x}, {y}) at {n}x");
                        }
                    }
                }
            }
        }
        // The tile beside them is still the tile's.
        let tile = c.rgba()[(24 * n) * WIDTH * n + 16 * n];
        assert_eq!(tile, word(fb.palette[5]));
        // Spec 11, the HD font mode: both cells listed with their ink and paper, each block the
        // paper; the tile is as before.
        let mut c = Compositor::new();
        c.set_font_mode(true);
        c.compose(&fb, &mut Tile(vec![5; 256]), scale);
        let (ink, paper) = (fb.palette[usize::from(INK)], fb.palette[usize::from(PAPER)]);
        let listed = |x: u16, code: u16| HdGlyph {
            x,
            y: 16,
            code,
            ink,
            paper,
            halftone: false,
        };
        assert_eq!(
            c.glyphs(),
            [listed(24, 0xac00), listed(32, u16::from(b'A'))]
        );
        for y in 16 * n..24 * n {
            let row = &c.rgba()[y * WIDTH * n..][24 * n..40 * n];
            assert!(row.iter().all(|&g| g == word(paper)), "the paper at {n}x");
        }
        assert_eq!(c.rgba()[(24 * n) * WIDTH * n + 16 * n], tile);
    }
}

/// Spec 11 Acceptance 1 on the text path: twelve lines in the ten rows of the message scroll
/// (so its rows have scrolled up three times), the plane on and off. With it, every pixel has
/// the colour it has without it; every glyph pixel names its cell's code and its place, with the
/// ink flag exactly on its rows' set bits, also on the rows moved up; the HD font lists the
/// letter and the syllable of every line at their cells, in the ink and paper the frame shows,
/// not the space; and the key wait's cursor, below the last line, is classic.
#[test]
fn the_scroll_names_its_glyphs_and_the_hd_font_lists_its_letters() {
    let game = |st: Rc<RefCell<State>>, io: Rc<Io>| async move {
        io.draw(|fb| {
            for (i, c) in fb.palette.iter_mut().enumerate() {
                let v = i as u8;
                *c = u6_render::Rgb::new(v.wrapping_mul(9), 255 - v, 7);
            }
        });
        for _ in 0..12 {
            printf::puts(&st, &io, "가A B\n".as_bytes()).await;
        }
        let _ = wait::read_key(&st, &io).await;
    };
    let run = |plane: bool| {
        let mut e = Engine::with_game(wide(Language::English), Files::default(), game);
        if plane {
            e.enable_src_plane();
        }
        for _ in 0..6 {
            e.tick(&[], 16);
        }
        e
    };
    let (on, off) = (run(true), run(false));
    let fb = on.frame();
    assert!(off.frame().src().is_none(), "off: no plane");
    assert!(
        fb.pixels() == off.frame().pixels(),
        "the colours of the plane off"
    );
    let (font, table) = (font(), table());
    let mut glyphs = 0;
    for (i, src) in fb.src().unwrap().iter().enumerate() {
        if src.kind != Kind::Glyph {
            continue;
        }
        glyphs += 1;
        let (x, y) = (i % WIDTH, i / WIDTH);
        let rows = match u8::try_from(src.id) {
            Ok(b) => font.glyphs[usize::from(b)],
            Err(_) => *table.glyph(u32::from(src.id)).unwrap(),
        };
        let (u, v) = (usize::from(src.u), usize::from(src.v));
        assert_eq!((u, v), (x % 8, y % 8), "({x}, {y}): its place in the cell");
        let set = rows[v] & (0x80 >> u) != 0;
        assert_eq!(src.flags, if set { FLAG_INK } else { 0 }, "({x}, {y})");
    }
    assert!(glyphs >= 9 * 4 * 64, "every cell of the nine lines left");
    // The scroll's columns 22.. and rows 14..=22 hold the last nine lines; row 23 the cursor.
    let at = |col: usize, row: usize| {
        let i = row * 8 * WIDTH + col * 8;
        (i + 1, i)
    };
    let (ink, paper) = at(23, 14);
    let (ink, paper) = (fb.pixels()[ink], fb.pixels()[paper]);
    assert_ne!(ink, paper, "the letter's ink and paper differ");
    let want: Vec<HdGlyph> = (14..=22u16)
        .flat_map(|row| {
            [
                (22u16, 0xac00),
                (23, u16::from(b'A')),
                (25, u16::from(b'B')),
            ]
            .map(move |(col, code)| (col, row, code))
        })
        .map(|(col, row, code)| HdGlyph {
            x: col * 8,
            y: row * 8,
            code,
            ink: fb.palette[usize::from(ink)],
            paper: fb.palette[usize::from(paper)],
            halftone: false,
        })
        .collect();
    let mut c = Compositor::new();
    c.set_font_mode(true);
    c.compose(fb, &mut NoImages, Scale::X2);
    assert_eq!(
        c.glyphs(),
        want.as_slice(),
        "the letters and syllables of nine lines"
    );
    let w = *on.state().ui.text.win();
    let cursor = (usize::from(w.x + w.col), usize::from(w.y + w.row));
    assert_eq!(cursor, (22, 23), "the cursor below the last line");
    for k in 0..64 {
        let (x, y) = (cursor.0 * 8 + k % 8, cursor.1 * 8 + k / 8);
        assert_eq!(fb.src_at(x, y), Some(Src::CLASSIC), "the cursor ({x}, {y})");
    }
    assert!(
        (0..64).any(|k| fb.get(cursor.0 * 8 + k % 8, cursor.1 * 8 + k / 8) == Some(ink)),
        "the cursor is drawn"
    );
}
