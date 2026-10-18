//! Spec 08 Acceptance 2, the text path: Hangul cells, the UTF-8 assembler, `printf`'s cells and
//! positional arguments, the Korean grammar rules and `@{…}`, on the synthetic font and a
//! synthetic Hangul table (sample strings written for these tests).

use std::rc::Rc;

use u6_formats::font::{HANGUL_GLYPHS, HangulFont};
use u6_render::hd::{Compositor, Image, Images, Scale, word};
use u6_render::{Kind, Src};

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
    // two bytes then. Without room to pad, the field finishes the syllable.
    assert_eq!(
        b(b"\xea%4s|", args(vec![bytes(&[0xb0, 0x80])])),
        "\u{ea}  \u{b0}\u{80}|"
    );
    assert_eq!(b(b"\xea%2s|", args(vec![bytes(&[0xb0, 0x80])])), "가|");
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
    let cases: [Case; 17] = [
        // A syllable split across adjacent fields: no width, then left-aligned with a padding
        // of 0 and of -1, after one byte and after two.
        ("%s%s|", vec![ea, b080], en, "가|", b"\xea\xb0\x80|"),
        ("%-1s%s|", vec![ea, b080], en, "가|", b"\xea\xb0\x80|"),
        ("%-1s%s|", vec![aea, b080], en, "a가|", b"a\xea\xb0\x80|"),
        ("%-2s%s|", vec![eab0, x80], en, "가|", b"\xea\xb0\x80|"),
        // Right-aligned, a padding of 0 and of -1 (both fields), and a right-aligned field
        // after a held byte with a padding of 0 (nothing is let out, the field finishes it).
        ("%1s%s|", vec![ea, b080], en, "가|", b"\xea\xb0\x80|"),
        ("%1s%1s|", vec![aea, b080], en, "a가|", b"a\xea\xb0\x80|"),
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
    for (col, what) in cells {
        for y in 16..24 {
            for x in col * 8..col * 8 + 8 {
                assert_eq!(fb.src_at(x, y), Some(Src::CLASSIC), "{what} ({x}, {y})");
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
                            let got = c.rgba()[(y * n + sy) * 320 * n + x * n + sx];
                            assert_eq!(got, want, "{what} ({x}, {y}) at {n}x");
                        }
                    }
                }
            }
        }
        // The tile beside them is still the tile's.
        let tile = c.rgba()[(24 * n) * 320 * n + 16 * n];
        assert_eq!(tile, word(fb.palette[5]));
    }
}
