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
    Rc::new(HangulFont {
        glyphs: (0..HANGUL_GLYPHS).map(rows).collect(),
    })
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
    let fmt = leak(format!("{fmt}\n").into_bytes());
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
        assert_eq!(
            st.borrow().ui.text.line_len,
            2,
            "nothing drawn before the flush"
        );
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
    let e = put_bytes(s, "a가힣b".as_bytes());
    let fb = e.frame();
    assert_eq!(cell(fb, 22, 14, INK), b'a');
    // Not glyph 0 or 0xa3 of u6.ch (the low bytes of the codes): paper only.
    for col in [23, 24] {
        for dy in 0..CELL {
            for dx in 0..CELL {
                assert_eq!(fb.get(col * CELL + dx, 14 * CELL + dy), Some(PAPER));
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
    // Positions do not move the others: the next plain directive takes the first argument.
    assert_eq!(line(Language::English, "%2$s %s %s", a), "cd ab cd");
    // Off, `%1$s` is width 1 and the directive `$` (as before).
    assert_eq!(
        print(state(), b"%1$s\n", &[Arg::Str(b"ab")])
            .state()
            .ui
            .messages[0],
        "$ s"
    );
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
    for (n, josa) in [
        (0, "이 으로"),
        (1, "이 로"),
        (2, "가 로"),
        (3, "이 으로"),
        (6, "이 으로"),
        (10, "이 으로"),
        (100, "이 으로"),
        (1000, "이 으로"),
        (10000, "이 으로"),
    ] {
        let words = args(vec![Arg::Word(n), Arg::Word(n)]);
        let (a, b) = josa.split_once(' ').unwrap();
        assert_eq!(
            line(Language::Korean, "%d{이/가} %d{(으)로}", words),
            format!("{n}{a} {n}{b}"),
            "{n}"
        );
    }
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
    // Off, `@{` is the plain keyword with its brace.
    let e = print(state(), b"@{x} y", &[]);
    assert_eq!(row_text(e.frame(), 22..25, 14, ALERT), "{x}");
}

#[test]
fn the_committed_table_parses_and_draws() {
    let blob = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../site/fonts/galmuri7-hangul.bin"
    ));
    let t = HangulFont::parse(blob).unwrap();
    assert_eq!(t.glyphs.len(), 11_172);
    // Every syllable has ink in rows 0..=6 only and in columns 0..=6 only.
    assert!(t.glyphs.iter().all(|g| g.iter().any(|&r| r != 0)));
    assert!(
        t.glyphs
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
