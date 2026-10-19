//! Spec 08 slice K4: the `u` program's strings from a locale pack, on a synthetic `u` (passage
//! cases of the test's own code and strings), the synthetic 4-pixel font and a synthetic Hangul
//! table: a passage format translated shows in Korean and wraps by glyph; the player's name and a
//! string of `u` printed through `%s` are never looked up (each also catalogued and translated
//! here); a format the pack lacks stays English and is counted; without a pack (the text context
//! left as main makes it), and with the identity pack (`en`, `wide` on), the passage is the one
//! English lays out, byte for byte. A translation taller than its box: the passage, and a print
//! of the creation or the transfer (one call, or the calls that fill one buffer), shows the
//! English, each catalogued format a miss; one that just fits shows Korean; the layout measured
//! first draws nothing and leaves the context as it was.

use std::rc::Rc;

use u6_formats::font::{HANGUL_GLYPHS, HangulFont};

use super::asm::*;
use super::synthetic::{drive, exe, font, keyboard, poke_words, rel};
use crate::converse::disasm::catalog::hash64;
use crate::files::Files;
use crate::intro::exe::UExe;
use crate::intro::locale::{self, IntroLocale};
use crate::intro::mem::{Chip, Ptr, Region, View};
use crate::intro::state::IntroState;
use crate::intro::text::Entry;
use crate::intro::{passages, text};
use crate::locale::program::{Catalogue, Role};
use crate::locale::{Counts, Pack, StringText, fingerprint};
use crate::ui::Language;

const CHAIN: u32 = 0x0010_ac7a;
const CASE0: u32 = 0x0010_ae00;
const CASE1: u32 = 0x0010_b400;
const TAIL: u32 = 0x0010_c7f4;
const FILL_THUNK: u32 = 0x0010_f51a;
const PRINT_THUNK: u32 = 0x0010_f532;
/// Where the test's strings go, 0x80 bytes apart.
const STRINGS: u32 = 0x0010_c900;
/// The passage box: 40 pixels wide (five syllables), 64 high.
const BOX_X1: u16 = 39;
const BOX_Y1: u16 = 63;

/// What a print of the case pushes for its `%s` first.
#[derive(Clone, Copy)]
enum Push {
    /// The player's name buffer (`pea $2bd8(a4)`).
    Name,
    /// Another of the test's strings (`pea d(pc)`).
    Str(usize),
}

fn string_at(i: usize) -> u32 {
    STRINGS + 0x80 * i as u32
}

/// The code of a case at `at`: the box, left alignment, the fill with D6, then each print through
/// 0x1142f0 (a string of the test's as the format, with its `%s` pushed first), then the tail.
fn case_code(at: u32, prints: &[(usize, Option<Push>)]) -> Vec<u16> {
    let mut code: Vec<u16> = vec![MOVE_W_IMM_TO_A4, BOX_X1, 0x2d0a];
    code.extend([MOVE_W_IMM_TO_A4, BOX_Y1, 0x2d10]);
    code.extend([MOVE_B_IMM_TO_A5, 0x0000, 0xfffe]);
    code.extend([MOVE_L_D6_D0, LEA_A4_A0, 0x2d02]);
    let here = |code: &Vec<u16>| at + 2 * code.len() as u32;
    let ext = here(&code) + 2;
    code.extend([JSR_PC, rel(ext, FILL_THUNK)]);
    for &(fmt, push) in prints {
        let mut pushed = 4;
        match push {
            Some(Push::Name) => code.extend([0x486c, 0x2bd8]),
            Some(Push::Str(i)) => {
                let ext = here(&code) + 2;
                code.extend([PEA_PC, rel(ext, string_at(i))]);
            }
            None => pushed = 0,
        }
        let ext = here(&code) + 2;
        code.extend([PEA_PC, rel(ext, string_at(fmt))]);
        let ext = here(&code) + 2;
        code.extend([JSR_PC, rel(ext, PRINT_THUNK)]);
        code.extend([LEA_A7_A7, 4 + pushed]);
    }
    let ext = here(&code) + 2;
    code.extend([BRA_W, rel(ext, TAIL)]);
    code
}

/// A `u` with the test's `strings`, whose passage 0 prints `prints` and whose passage 1 prints
/// every one of `strings` as a format (so the catalogue, which walks the shown passages, holds
/// each of them).
fn passage_exe(strings: &[&[u8]], prints: &[(usize, Option<Push>)]) -> UExe {
    let mut e = exe();
    // move.l d7,d0; tst.w d0; beq case0; subq.w #1,d0; beq case1; bra tail
    poke_words(
        &mut e,
        CHAIN,
        &[
            MOVE_L_D7_D0,
            TST_W_D0,
            BEQ_W,
            rel(CHAIN + 6, CASE0),
            0x5340,
            BEQ_W,
            rel(CHAIN + 12, CASE1),
            BRA_W,
            rel(CHAIN + 16, TAIL),
        ],
    );
    poke_words(&mut e, CASE0, &case_code(CASE0, prints));
    let all: Vec<(usize, Option<Push>)> = (0..strings.len()).map(|i| (i, None)).collect();
    poke_words(&mut e, CASE1, &case_code(CASE1, &all));
    poke_words(&mut e, TAIL, &[MOVEA_L_A5_A0, 0xfff6]);
    poke_words(&mut e, FILL_THUNK, &jmp_abs(0x0010_f898));
    poke_words(&mut e, PRINT_THUNK, &jmp_abs(0x0011_42f0));
    for (i, s) in strings.iter().enumerate() {
        let mut z = s.to_vec();
        z.push(0);
        e.poke(string_at(i), &z);
    }
    e
}

/// Syllable i's cell: column 0 of row 0, its index in rows 1 and 2, column 7 of row 6.
fn rows(i: usize) -> [u8; 8] {
    [0x80, i as u8, (i >> 8) as u8, 0, 0, 0, 0x01, 0]
}

fn table() -> Rc<HangulFont> {
    Rc::new(HangulFont::from_glyphs((0..HANGUL_GLYPHS).map(rows).collect()).unwrap())
}

/// A Korean pack of `u` strings (English, translation).
fn pack(language: Language, strings: &[(&[u8], &str)]) -> Pack {
    Pack::new(language, "test/1", fingerprint([]), [])
        .unwrap()
        .with_program(strings.iter().map(|&(en, text)| {
            (
                hash64(en),
                StringText {
                    len: en.len() as u32,
                    text: text.as_bytes().to_vec(),
                },
            )
        }))
        .unwrap()
}

/// What passage 0 of `e` lays into the passage bitmap (the box's pixels), with `pack` taken as
/// `u` takes it (its catalogue read from `e`), or none (the text context as [`locale::apply`]
/// leaves it, checked to be main's); the player named `name`. Also what was counted.
fn laid(e: UExe, pack: Option<Pack>, name: &[u8]) -> (Vec<u8>, [u32; 2]) {
    laid_with(e, pack, name, |_| {})
}

/// [`laid`], `set` changing the state after [`locale::apply`] (the reference's `wide` context).
fn laid_with(
    e: UExe,
    pack: Option<Pack>,
    name: &[u8],
    set: impl FnOnce(&mut IntroState),
) -> (Vec<u8>, [u32; 2]) {
    let catalogue = Rc::new(Catalogue::from_u(&e));
    let mut st = IntroState::new(e, Files::default());
    keyboard(&mut st);
    st.font = Some(font());
    st.hero.name[..name.len()].copy_from_slice(name);
    let counts = Rc::new(Counts::default());
    st.locale = pack.map(|p| IntroLocale {
        pack: Rc::new(p),
        catalogue,
        hangul: Some(table()),
        counts: Rc::clone(&counts),
    });
    let main = st.text.clone();
    locale::apply(&mut st);
    if st.locale.is_none() {
        assert_eq!(st.text, main, "no pack: the text context as main makes it");
        assert!(
            !st.text.wide && st.text.hangul.is_none(),
            "no pack: not wide"
        );
    }
    set(&mut st);
    st.views.passage = View::screen(Ptr::at(Region::Work));
    let st = drive(&[], move |io| async move {
        passages::passage(&mut st, &io, 0);
        st
    })
    .unwrap();
    let p = st.views.passage;
    assert_eq!(
        (p.x1, p.y1),
        (BOX_X1 as i16, BOX_Y1 as i16),
        "the case's box"
    );
    let pixels = (0..=i32::from(BOX_Y1))
        .flat_map(|y| (0..=i32::from(BOX_X1)).map(move |x| (x, y)))
        .map(|(x, y)| st.video.chip.pixel(p.planes, x, y))
        .collect();
    (pixels, counts.get())
}

/// What passage 0 lays out when it prints `text` as it is (no pack, `wide` on and the Hangul
/// table set by hand): the reference.
fn reference(text: &[u8]) -> Vec<u8> {
    let e = passage_exe(&[text], &[(0, None)]);
    laid_with(e, None, b"", |st| {
        st.text.wide = true;
        st.text.hangul = Some(table());
    })
    .0
}

/// The syllables drawn in `pixels` (the box), each with its cell's position, read row by row: a
/// cell is ink (colour 0) where [`rows`] has a bit.
fn syllables(pixels: &[u8]) -> Vec<(i32, i32, char)> {
    let w = i32::from(BOX_X1) + 1;
    let ink = |x: i32, y: i32| {
        (0..w).contains(&x)
            && (0..=i32::from(BOX_Y1)).contains(&y)
            && pixels[(y * w + x) as usize] == 0
    };
    let bits = |x: i32, y: i32| (0..8).fold(0u8, |b, dx| (b << 1) | u8::from(ink(x + dx, y)));
    let mut out = Vec::new();
    for y in (0..=i32::from(BOX_Y1)).step_by(8) {
        for x in 0..w {
            let r: Vec<u8> = (0..8).map(|r| bits(x, y + r)).collect();
            if r[0] == 0x80 && r[6] == 0x01 && r[3..6] == [0, 0, 0] && r[7] == 0 {
                let i = u32::from(r[1]) | u32::from(r[2]) << 8;
                out.extend(char::from_u32(0xac00 + i).map(|c| (x, y, c)));
            }
        }
    }
    out
}

const STORM: &[u8] = b"The storm rages on and on.\n";
const STORM_KO: &str = "폭풍이 몰아친다 오래도록\n";

#[test]
fn a_passage_format_from_the_pack_shows_in_korean_wrapped_by_glyph() {
    let e = passage_exe(&[STORM], &[(0, None)]);
    let (pixels, counts) = laid(
        e.clone(),
        Some(pack(Language::Korean, &[(STORM, STORM_KO)])),
        b"",
    );
    // 40 px a line: 폭풍이 (24 px), then 몰아친다 (32 px; with the space before it 60 px would not
    // fit), then 오래도록, each a syllable per 8 px.
    let at = |y: i32, s: &str| -> Vec<(i32, i32, char)> {
        s.chars()
            .enumerate()
            .map(|(i, c)| (8 * i as i32, y, c))
            .collect()
    };
    let want: Vec<_> = [at(0, "폭풍이"), at(8, "몰아친다"), at(16, "오래도록")].concat();
    assert_eq!(syllables(&pixels), want);
    assert_eq!(counts, [1, 0], "one format from the pack");
    assert_eq!(pixels, reference(STORM_KO.as_bytes()));
    let (english, _) = laid(e, None, b"");
    assert!(syllables(&english).is_empty() && english != pixels);
}

const HAIL: &[u8] = b"Hail, %s!\n";
const HAIL_KO: &str = "반갑소, %s!\n";
const AVATAR: &[u8] = b"Avatar";

#[test]
fn the_players_name_printed_through_s_is_never_looked_up() {
    // The name typed is a string the catalogue holds and the pack translates: it shows as typed.
    let e = passage_exe(&[HAIL, AVATAR], &[(0, Some(Push::Name))]);
    let p = pack(Language::Korean, &[(HAIL, HAIL_KO), (AVATAR, "성자")]);
    assert!(Catalogue::from_u(&e).of(AVATAR).is_some(), "catalogued");
    let (pixels, counts) = laid(e, Some(p), AVATAR);
    assert_eq!(pixels, reference("반갑소, Avatar!\n".as_bytes()));
    assert_eq!(counts, [1, 0], "the format only");
}

#[test]
fn a_string_of_u_printed_through_s_is_never_looked_up() {
    let e = passage_exe(&[HAIL, AVATAR], &[(0, Some(Push::Str(1)))]);
    let p = pack(Language::Korean, &[(HAIL, HAIL_KO), (AVATAR, "성자")]);
    let (pixels, counts) = laid(e, Some(p), b"");
    assert_eq!(pixels, reference("반갑소, Avatar!\n".as_bytes()));
    assert_eq!(counts, [1, 0], "the format only");
}

const CALM: &[u8] = b"Then calm.\n";

#[test]
fn a_format_the_pack_lacks_stays_english_and_without_a_pack_or_with_the_identity_nothing_changes() {
    let e = passage_exe(&[STORM, CALM], &[(0, None), (1, None)]);
    let ko = pack(Language::Korean, &[(STORM, STORM_KO)]);
    let (pixels, counts) = laid(e.clone(), Some(ko), b"");
    let mut both = STORM_KO.as_bytes().to_vec();
    both.extend_from_slice(CALM);
    assert_eq!(pixels, reference(&both));
    assert_eq!(counts, [1, 1], "a hit and a miss");
    // No pack: the context as main makes it (`wide` off), the English laid out as the original
    // lays it out (the buffer flushed through the word wrapper, here by hand).
    let (none, counts) = laid(e.clone(), None, b"");
    assert_eq!(counts, [0, 0]);
    let mut st = IntroState::new(exe(), Files::default());
    st.font = Some(font());
    st.views.passage = View {
        x1: BOX_X1 as i16,
        y1: BOX_Y1 as i16,
        ..View::screen(Ptr::at(Region::Work))
    };
    let p = st.views.passage;
    crate::intro::mem::fill_view(&mut st.video.chip, &p, 0x1e);
    let mut ctx = text::TextCtx {
        view: p,
        align: 0,
        fg: 0,
        bg: 0x1e,
        ..text::TextCtx::default()
    };
    let f = font();
    for s in [STORM, CALM] {
        text::print_more(&mut st.video.chip, &mut ctx, &f, s, &[]);
    }
    let buf = ctx.buf.clone();
    text::wrap(&mut st.video.chip, &mut ctx, &f, &buf);
    let by_hand: Vec<u8> = (0..=i32::from(BOX_Y1))
        .flat_map(|y| (0..=i32::from(BOX_X1)).map(move |x| (x, y)))
        .map(|(x, y)| st.video.chip.pixel(p.planes, x, y))
        .collect();
    assert_eq!(none, by_hand, "no pack: the English, byte for byte");
    // The identity pack: every catalogued string as itself, English grammar, `wide` on.
    let identity = pack(
        Language::English,
        &[
            (STORM, std::str::from_utf8(STORM).unwrap()),
            (CALM, std::str::from_utf8(CALM).unwrap()),
        ],
    );
    let (same, counts) = laid(e, Some(identity), b"");
    assert_eq!(same, none, "identical to English");
    assert_eq!(counts, [2, 0], "every catalogued string a hit");
}

#[test]
fn the_catalogue_walks_the_shown_passages_for_their_formats_without_the_name_buffer() {
    let e = passage_exe(&[HAIL, AVATAR, b"Hi"], &[(0, Some(Push::Name))]);
    assert_eq!(passages::formats(&e, 0), [string_at(0)]);
    assert_eq!(
        passages::formats(&e, 1),
        [string_at(0), string_at(1), string_at(2)]
    );
    let c = Catalogue::from_u(&e);
    let held: Vec<&[u8]> = c.iter().map(|(_, e)| e.text.as_slice()).collect();
    assert_eq!(
        held,
        [HAIL, AVATAR],
        "a string under 3 bytes is not catalogued"
    );
    assert!(passages::formats(&exe(), 0).is_empty(), "no chain");
    // A case that pushes the name buffer as its format: the walk leaves it out.
    let mut named = exe();
    poke_words(
        &mut named,
        CHAIN,
        &[
            MOVE_L_D7_D0,
            TST_W_D0,
            BEQ_W,
            rel(CHAIN + 6, CASE0),
            BRA_W,
            rel(CHAIN + 10, TAIL),
        ],
    );
    let mut code = vec![0x486c, 0x2bd8];
    let ext = CASE0 + 2 * code.len() as u32 + 2;
    code.extend([JSR_PC, rel(ext, PRINT_THUNK)]);
    poke_words(&mut named, CASE0, &code);
    poke_words(&mut named, TAIL, &[MOVEA_L_A5_A0, 0xfff6]);
    poke_words(&mut named, PRINT_THUNK, &jmp_abs(0x0011_42f0));
    assert!(passages::formats(&named, 0).is_empty());
}

/// `n` lines of four syllables (the passage box holds eight lines of 8 pixels).
fn lines(n: usize) -> String {
    "가나다라\n".repeat(n)
}

#[test]
fn a_passage_taller_than_its_box_in_korean_shows_the_english_each_catalogued_format_a_miss() {
    let e = passage_exe(&[STORM, CALM], &[(0, None), (1, None)]);
    let (english, none) = laid(e.clone(), None, b"");
    assert_eq!(none, [0, 0]);
    // The first format nine lines tall, the second translated to fit: the whole case English.
    let tall = pack(
        Language::Korean,
        &[(STORM, lines(9).as_str()), (CALM, "고요하다.\n")],
    );
    let (pixels, counts) = laid(e.clone(), Some(tall), b"");
    assert_eq!(pixels, english, "the box as English draws it");
    assert_eq!(
        counts,
        [0, 2],
        "each catalogued format a miss, nothing counted twice"
    );
    // A format the pack lacks is a miss too; the measure counts nothing of its own.
    let one = pack(Language::Korean, &[(STORM, lines(9).as_str())]);
    let (pixels, counts) = laid(e, Some(one), b"");
    assert_eq!(pixels, english);
    assert_eq!(counts, [0, 2]);
}

#[test]
fn a_passage_that_just_fits_its_box_in_korean_shows_the_korean() {
    let e = passage_exe(&[STORM], &[(0, None)]);
    let eight = lines(8);
    let (pixels, counts) = laid(
        e.clone(),
        Some(pack(Language::Korean, &[(STORM, eight.as_str())])),
        b"",
    );
    assert_eq!(counts, [1, 0], "from the pack");
    assert_eq!(pixels, reference(eight.as_bytes()));
    let rows: Vec<i32> = syllables(&pixels).iter().map(|&(_, y, _)| y).collect();
    assert_eq!(rows.len(), 32, "every syllable drawn");
    assert_eq!(rows.last(), Some(&56), "the last on the box's last line");
    let (english, _) = laid(e, None, b"");
    assert_ne!(pixels, english);
}

/// Where the print tests' text goes: 40 pixels wide (five syllables), two lines of 8 pixels.
fn two_lines() -> View {
    View {
        planes: Ptr::at(Region::Work),
        x1: 39,
        x0: 0,
        y0: 0,
        y1: 15,
    }
}

/// The pixels of the 40 × 24 area under [`two_lines`] (a line below it too) after `calls` (an
/// entry and the index of one of `strings`, each a string of `u` here) go through the creation's
/// and the transfer's print sites with `pack` (each of `strings` catalogued), or none; also what
/// was counted.
fn printed(strings: &[&[u8]], pack: Option<Pack>, calls: &[(Entry, usize)]) -> (Vec<u8>, [u32; 2]) {
    let mut e = exe();
    for (i, s) in strings.iter().enumerate() {
        e.poke(string_at(i), &[*s, &[0]].concat());
    }
    let catalogue = Catalogue::from_entries(strings.iter().map(|s| (Role::Transfer, s.to_vec())));
    let mut st = IntroState::new(e, Files::default());
    st.font = Some(font());
    let counts = Rc::new(Counts::default());
    st.locale = pack.map(|p| IntroLocale {
        pack: Rc::new(p),
        catalogue: Rc::new(catalogue),
        hangul: Some(table()),
        counts: Rc::clone(&counts),
    });
    st.text.view = two_lines();
    locale::apply(&mut st);
    let calls: Vec<(Entry, u32, &[text::Arg])> = calls
        .iter()
        .map(|&(entry, i)| (entry, string_at(i), &[][..]))
        .collect();
    locale::print_formats(&mut st, &calls);
    let pixels = (0..24)
        .flat_map(|y| (0..40).map(move |x| (x, y)))
        .map(|(x, y)| st.video.chip.pixel(two_lines().planes, x, y))
        .collect();
    (pixels, counts.get())
}

const HAIL_LINE: &[u8] = b"Hail, traveller!\n%P";
const SHEET: &[u8] = b"Ultima\n";
const ONE_WAY: &[u8] = b"One way.\n";
const OR_ANOTHER: &[u8] = b"Or another.\n%P";

#[test]
fn a_creation_or_transfer_print_taller_than_its_view_prints_the_english() {
    let strings = [HAIL_LINE, SHEET, ONE_WAY, OR_ANOTHER];
    for (entry, i, tall, fits) in [
        (Entry::New, 0, "가나\n다라\n마바\n%P", "가나\n다라\n%P"),
        (Entry::Screen, 1, "가\n나\n다", "가\n나\n"),
    ] {
        let calls = [(entry, i)];
        let (english, none) = printed(&strings, None, &calls);
        assert_eq!(none, [0, 0]);
        let ko = |text: &str| pack(Language::Korean, &[(strings[i], text)]);
        let (pixels, counts) = printed(&strings, Some(ko(tall)), &calls);
        assert_eq!(pixels, english, "{entry:?}: the English");
        assert_eq!(counts, [0, 1], "{entry:?}: a miss");
        let (pixels, counts) = printed(&strings, Some(ko(fits)), &calls);
        assert_eq!(counts, [1, 0], "{entry:?}: two lines fit");
        assert_ne!(pixels, english, "{entry:?}: the Korean");
    }
    // Calls that fill one buffer before its flush are one box: the first too tall, the flush in
    // the second, all of them English (the second alone in English would still be too tall).
    let calls = [(Entry::New, 2), (Entry::More, 3)];
    let (english, _) = printed(&strings, None, &calls);
    let both = pack(
        Language::Korean,
        &[(ONE_WAY, "가\n나\n다\n"), (OR_ANOTHER, "라.\n%P")],
    );
    let (pixels, counts) = printed(&strings, Some(both), &calls);
    assert_eq!(pixels, english, "the box as English draws it");
    assert_eq!(counts, [0, 2]);
    let fits = pack(
        Language::Korean,
        &[(ONE_WAY, "가나\n"), (OR_ANOTHER, "다라.\n%P")],
    );
    let (pixels, counts) = printed(&strings, Some(fits), &calls);
    assert_eq!(counts, [2, 0]);
    assert_ne!(pixels, english);
}

#[test]
fn the_layout_measured_first_draws_nothing_and_leaves_the_context_as_it_was() {
    let f = font();
    let ctx = text::TextCtx {
        view: two_lines(),
        wide: true,
        hangul: Some(table()),
        ..text::TextCtx::default()
    };
    let chip = Chip::new();
    let measure = |text: &str| {
        let mut c = chip.clone();
        let tall = text::overflows(&mut c, &ctx, &f, |chip, ctx, font| {
            text::print(chip, ctx, font, text.as_bytes(), &[]);
        });
        assert_eq!(c, chip, "nothing drawn");
        tall
    };
    let before = ctx.clone();
    assert!(measure("가\n나\n다"), "a glyph after the last line feed");
    assert!(
        !measure("가\n나\n"),
        "a line feed at the end draws nothing after it"
    );
    assert!(
        !measure("가\n나\n%Y다"),
        "a cursor moved by %Y is not stuck"
    );
    assert!(
        measure("가나다라마바사아자차카"),
        "wrapped onto a third line"
    );
    assert_eq!(ctx, before, "the context as it was");
    // Drawn, the same text goes over the last line: what the measure is for.
    let mut drawn = ctx.clone();
    let mut c = chip.clone();
    text::print(&mut c, &mut drawn, &f, "가\n나\n다".as_bytes(), &[]);
    assert_eq!(drawn.y, 8, "the cursor stayed on the last line");
    assert_ne!(c, chip);
}
