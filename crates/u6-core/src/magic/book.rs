//! The spellbook: its spells (0x115da4), the panel picture (0x116028), the page (0x116218) with
//! the reagent counts (0x116084, 0x115d1a), the current spell (0x1067d8, 0x11612e) and the
//! redraw after a page step (0x1078d2).

use std::cell::RefCell;

use u6_formats::bitmap::Bitmap;

use super::seams;
use super::{MagicState, SPELL};
use crate::engine::Io;
use crate::main_loop::pointed_string;
use crate::objects::{Objects, Slot};
use crate::state::State;
use crate::ui::panels::{select_window, set_cursor};
use crate::ui::print::put_raw;
use crate::ui::printf::{Arg, printf};
use crate::ui::text::WindowId;

/// Image strings of hunk 11 the page prints.
const FMT_S: u32 = 0x0011_58b0;
const FMT_D: u32 = 0x0011_58ac;
const MSG_LEVEL: u32 = 0x0011_58b4;
/// Port-only (spec 08, slice K2; 2026-10-24): the formats of the page's names and counts, for
/// the locale layer's catalogue ([`super::PRINTED`]; of no word of their own, it leaves them
/// out). The page's other strings are drawn at fixed cells of the status window, each within
/// its field (2026-10-25): its level word is [`PAGE_PRINTED`], and the spells' and the circle's
/// names, put through `%s`, are read for display ([`super::tables::NAME_LABELS`],
/// [`super::tables::CIRCLE_LABELS`]).
pub(super) const PRINTED: &[u32] = &[FMT_S, FMT_D];
/// Port-only (spec 08, 2026-10-25): the page's level word ([`MSG_LEVEL`]), which it passes to
/// `printf` as its format at status-window cell (10, 11), under the circle's name: for the
/// locale layer's catalogue, within its English's cells (a panel's string, as
/// `ui::tables::PRINTED`'s are). The `printf` lookup finds it; the page needs nothing more.
pub(crate) const PAGE_PRINTED: &[u32] = &[MSG_LEVEL];
/// A4-0x61de: pointer to the file name of the book picture.
const BOOK_BMP: u32 = 0x61de;
/// The marker glyph of the current spell's line.
pub(super) const MARKER: i32 = 0x1a;

/// Quality byte of a slot (the high byte of its A4+0x250c word).
pub(crate) fn quality(o: &Objects, slot: Slot) -> u8 {
    (o.qq_of(slot) >> 8) as u8
}

/// Sets the quality byte of a slot, keeping its quantity.
pub(crate) fn set_quality(o: &mut Objects, slot: Slot, q: u8) {
    if let Some(w) = usize::try_from(slot).ok().and_then(|i| o.qq.get_mut(i)) {
        *w = (*w & 0x00ff) | u16::from(q) << 8;
    }
}

/// Collects the spells of one book into `known`; the first new spell found, and whether a
/// spell 0xff (every spell) was met.
fn collect(s: &mut State, book: Slot) -> (u8, bool) {
    let mut first = 0u8;
    let mut o = crate::objects::first_child(&mut s.objects, book);
    while o >= 0 {
        let q = quality(&s.objects, o);
        if q == 0xff {
            return (first, true);
        }
        let (circle, bit) = (usize::from(q >> 4), q & 0xf);
        // Circles 8..15 would write past the 8 words (into A4+0x181a..); the port drops them.
        if let Some(word) = s.magic.known.get_mut(circle)
            && *word >> bit & 1 == 0
        {
            if first == 0 {
                first = q;
            }
            *word |= 1 << bit;
        }
        o = crate::objects::next_child(&mut s.objects);
    }
    (first, false)
}

/// A book whose quality is 0 and holds no spell 0 gets the first spell found as its quality.
fn default_current(s: &mut State, book: Slot, first: u8) {
    if quality(&s.objects, book) == 0
        && crate::objects::find_child(&mut s.objects, book, SPELL as i16, 0) == -1
    {
        set_quality(&mut s.objects, book, first);
    }
}

/// Original: 0x115da4 (magic: the spells in the readied books, the current spell, the page)
///
/// `known` gets a bit per spell inside either book (a spell of quality 0xff fills every
/// circle); each book whose quality is 0 without a spell 0 inside takes the first spell found;
/// the bits are cut to the 10 spells of a circle. The current spell is the first book's
/// quality (the second's when there is no first). `page_first` = spells before the current
/// one's circle, plus 8 when the current spell is past that circle's first page.
pub fn load_book(s: &mut State) {
    s.magic.known = [0; 8];
    let [b1, b2] = s.magic.books;
    let (first, mut all) = collect(s, Slot::from(b1));
    default_current(s, Slot::from(b1), first);
    if b2 != 0 {
        let (first, all2) = collect(s, Slot::from(b2));
        all |= all2;
        default_current(s, Slot::from(b2), first);
    }
    let m = &mut s.magic;
    if all {
        m.known = [0xffff; 8];
    }
    for w in &mut m.known {
        *w &= 0x3ff;
    }
    let book = if b1 != 0 { b1 } else { b2 };
    s.magic.current = quality(&s.objects, Slot::from(book));
    let m = &mut s.magic;
    m.page_first = 0xff;
    let circle = usize::from(m.current / 16);
    let mut count = 0u8;
    for c in 0..=circle.min(7) {
        for n in 0..16u8 {
            if m.known[c] >> n & 1 == 0 {
                continue;
            }
            if c == circle {
                if m.page_first == 0xff {
                    m.page_first = count;
                }
                if u16::from(m.current) == u16::from(n) + 16 * c as u16 {
                    if count >= m.page_first.wrapping_add(8) {
                        m.page_first = m.page_first.wrapping_add(8);
                    }
                    break;
                }
            }
            count = count.wrapping_add(1);
        }
    }
}

/// Original: 0x11612e (magic: the page line showing spell `spell`, -1 if none)
pub fn page_line(m: &MagicState, spell: u16) -> i16 {
    m.page
        .iter()
        .position(|&p| u16::from(p) == spell)
        .map_or(-1, |i| i as i16)
}

/// Original: 0x1067d8 (ui: make `spell` the current spell and the quality of both books)
pub fn set_current_spell(s: &mut State, spell: u8) {
    s.magic.current = spell;
    for book in s.magic.books {
        if book != 0 {
            set_quality(&mut s.objects, Slot::from(book), spell);
        }
    }
}

/// Original: 0x115d1a (magic: the quantity of `kind` with quality `quality` that `actor`
/// carries: the objects after the actor in the list while they are carried (status bits
/// 0x18), contents of bags included)
pub fn carried_count(o: &Objects, actor: Slot, kind: u16, quality: u8) -> i16 {
    let mut n = 0i16;
    let mut slot = o.next_of(actor);
    while slot >= 0 && o.status(slot) & 0x18 != 0 {
        if o.kind(slot) == kind && self::quality(o, slot) == quality {
            n = n.wrapping_add(i16::from(o.qq_of(slot) as u8));
        }
        slot = o.next_of(slot);
    }
    n
}

/// The A4 string pointer at `off` (a file name of the data hunk).
fn a4_pointed(io: &Io, off: crate::string_registry::Text) -> String {
    String::from_utf8_lossy(&pointed_string(io, off)).into_owned()
}

/// Original: 0x116028 (magic: clear the side panel to the paper colour and draw the book
/// picture over it)
///
/// The picture is the LZW file named at A4-0x61de (A4-0x61e8 = 1 around the read selects the
/// compressed path of 0x134776), drawn at (0xa8, 6) by 0x153384. A missing or bad file draws
/// nothing. The source plane names it the spellbook.
///
/// Port (spec 07 HD0): the clear names the paper as the UI's fills do
/// ([`crate::ui::text::fill_rect`]), in the buffer it fills; a colour past 15 writes nothing.
pub fn draw_book(s: &mut State, io: &Io) {
    let paper = u16::from(s.ui.tables.colours.paper);
    crate::gfx::blit::set_plot_colour(&mut s.gfx, paper);
    seams::fill_rect(s, io, 0xa8, 6, 0x137, 0x67);
    if paper <= 15 {
        let map = s.ui.paper.as_ref();
        let name = |fb: &mut u6_render::Framebuffer| {
            crate::ui::text::name_paper(fb, (0xa8, 6, 0x137, 0x67), map);
        };
        if s.gfx.plot_back {
            io.draw_back(name);
        } else {
            io.draw(name);
        }
    }
    let Some(bmp) = book_bitmap(io) else {
        return;
    };
    let colours = &s.gfx.tables.bitmap_colours;
    let role = u6_render::BITMAP_SPELLBOOK;
    io.draw(|fb| crate::gfx::blit::blit_bmp(colours, fb, &bmp, 0xa8, 6, role));
}

/// The bitmap [`draw_book`] draws, decoded; `None` for a missing or bad file.
pub(crate) fn book_bitmap(io: &Io) -> Option<Bitmap> {
    io.files()
        .get(&book_file(io))
        .and_then(|f| Bitmap::from_lzw(f).ok())
}

/// The name of the file [`book_bitmap`] reads (named at A4-0x61de).
pub(crate) fn book_file(io: &Io) -> String {
    a4_pointed(io, crate::string_registry::pointer!(BOOK_BMP))
}

async fn print_at(
    state: &RefCell<State>,
    io: &Io,
    addr: crate::string_registry::Text,
    args: &[Arg<'_>],
) {
    let fmt = crate::main_loop::game_string(io, addr);
    printf(state, io, &fmt, args).await;
}

/// 0x116084's count of the casts of `spell` the carried reagents of `actor` allow: the least
/// count over its reagents (quality 0), 99 when it needs none; Help (7) and Armageddon (0x70)
/// always 1. `None` for a spell without reagents other than those two (0x116084 then prints
/// nothing and leaves the line's count as it was).
pub(crate) fn casts_of(s: &State, actor: Slot, spell: u16) -> Option<i16> {
    let mask = s.magic.tables.mask(usize::from(spell));
    if mask == 0 && spell != 7 && spell != 0x70 {
        return None;
    }
    let mut count = 99i16;
    for i in 0..8 {
        if mask >> i & 1 != 0 {
            let kind = s.magic.tables.reagents[i];
            count = count.min(carried_count(&s.objects, actor, kind, 0));
        }
    }
    if spell == 7 || spell == 0x70 {
        count = 1;
    }
    Some(count)
}

/// Original: 0x116084 (magic: casts of `spell` the carried reagents allow, kept for page line
/// `line` and printed)
///
/// The least count over the spell's reagents (quality 0), 99 when it needs none; Help (7) and
/// Armageddon (0x70) always show 1. A spell without reagents other than those two prints
/// nothing and leaves the line's count as it was.
pub async fn reagent_count(state: &RefCell<State>, io: &Io, actor: Slot, spell: u16, line: i16) {
    let count = {
        let mut s = state.borrow_mut();
        let Some(count) = casts_of(&s, actor, spell) else {
            return;
        };
        if let Some(b) = usize::try_from(line)
            .ok()
            .and_then(|i| s.magic.page_reagents.get_mut(i))
        {
            *b = count as u8;
        }
        count
    };
    print_at(
        state,
        io,
        crate::string_registry::text!(FMT_D),
        &[Arg::Word(count as u16)],
    )
    .await;
}

/// The page lines 0x116218 lays out: per line the spell and whether it is the current one.
pub(super) fn layout(m: &mut MagicState) -> (Vec<u16>, i16) {
    m.prev_circle = 0xff;
    m.prev_page = 0xff;
    m.next_page = 0xff;
    m.next_circle = 0xff;
    let (mut first_circle, mut page_circle, mut last_n, mut last_c) = (-1i16, -1i16, -1i16, -1i16);
    let mut count = 0i16;
    let mut lines = Vec::new();
    for c in 0..8i16 {
        for n in 0..16i16 {
            if m.known[c as usize] >> n & 1 == 0 {
                continue;
            }
            if count < i16::from(m.page_first) {
                count += 1;
                if first_circle == -1 {
                    first_circle = c;
                }
                (last_n, last_c) = (n, c);
                continue;
            }
            if page_circle == -1 {
                page_circle = c;
            }
            if lines.len() < 8 && c == page_circle {
                let first_line = lines.is_empty();
                if last_c == c && n > last_n && first_line {
                    m.prev_page = 2;
                }
                if c > first_circle && first_line {
                    let back = if m.prev_page == 0xff { 0 } else { m.prev_page };
                    m.prev_circle = (last_c as u8).wrapping_sub(back);
                }
                if m.prev_circle != 0xff && m.prev_page == 0xff {
                    m.prev_page = 1;
                }
                lines.push((c * 16 + n) as u16);
                (last_c, last_n) = (c, n);
            } else {
                if m.next_page == 0xff && c == last_c && n > last_n {
                    m.next_page = 2;
                }
                if m.next_page == 0xff {
                    m.next_page = 1;
                }
                if m.next_circle == 0xff && c > last_c {
                    m.next_circle = c as u8;
                }
            }
        }
    }
    (lines, page_circle)
}

/// Original: 0x116218 (magic: draw the shown page of the spellbook: up to 8 spells of one
/// circle with their reagent counts, the current spell marked, the page arrows, the circle)
///
/// The text area (0xb0, 0xe)..(0x12f, 0x4f) is cleared to the paper colour; in the status window
/// each line has the name at column 1 and the count at column 14 (0x116084), the marker glyph 0x1a
/// at column 0 of the current spell (whose count gets bit 7). The page arrows are tiles 0x19c /
/// 0x19d where a previous / next page exists (else the blank 0x19b) at x = 0xc0 / 0xd0, y = 0x58;
/// then the circle name at (11, 10) and the level word (0x1158b4) at (10, 11), and the message
/// scroll is current again.
///
/// Port (spec 08, 2026-10-25): the spells' and the circle's names are read for display
/// ([`crate::locale::shown`]: the pack's form when it has one within its field,
/// [`super::tables::NAME_CELLS`] and [`super::tables::CIRCLE_CELLS`]); the counts keep their
/// column. The level word is a `printf` format its lookup finds ([`PAGE_PRINTED`]).
pub async fn draw_book_page(state: &RefCell<State>, io: &Io) {
    let (lines, circle, caster) = {
        let mut s = state.borrow_mut();
        let paper = u16::from(s.ui.tables.colours.paper);
        crate::gfx::blit::set_plot_colour(&mut s.gfx, paper);
        seams::fill_rect(&mut s, io, 0xb0, 0xe, 0x12f, 0x4f);
        let (lines, circle) = layout(&mut s.magic);
        (lines, circle, super::cast::caster(&s) as Slot)
    };
    select_window(state, io, WindowId::Status).await;
    for (i, &spell) in lines.iter().enumerate() {
        let row = i as i16 + 1;
        state.borrow_mut().magic.page[i] = spell as u8;
        set_cursor(state, io, 1, row).await;
        let name = {
            let s = state.borrow();
            crate::locale::shown(&s, s.magic.tables.name(usize::from(spell)).to_vec())
        };
        print_at(
            state,
            io,
            crate::string_registry::text!(FMT_S),
            &[Arg::Str(&name)],
        )
        .await;
        set_cursor(state, io, 14, row).await;
        reagent_count(state, io, caster, spell, i as i16).await;
        if spell == state.borrow().magic.current_spell() {
            set_cursor(state, io, 0, row).await;
            put_raw(state, io, MARKER).await;
            state.borrow_mut().magic.page_reagents[i] |= 0x80;
        }
    }
    {
        let mut s = state.borrow_mut();
        s.magic.page_lines = lines.len() as u8;
        let flags = [s.magic.prev_page, s.magic.next_page];
        let st = &*s;
        io.draw(|fb| {
            for (i, &f) in (1u16..).zip(&flags) {
                let tile = if f == 0xff { 0x19b } else { 0x19b + i };
                crate::gfx::blit::blit_front(&st.gfx, fb, tile, (2 * i + 0x16) * 8, 0x58);
            }
        });
    }
    set_cursor(state, io, 11, 10).await;
    let name = {
        let s = state.borrow();
        let english = usize::try_from(circle)
            .ok()
            .and_then(|c| s.magic.tables.circles.get(c).cloned())
            .unwrap_or_default();
        crate::locale::shown(&s, english)
    };
    print_at(
        state,
        io,
        crate::string_registry::text!(FMT_S),
        &[Arg::Str(&name)],
    )
    .await;
    set_cursor(state, io, 10, 11).await;
    print_at(state, io, crate::string_registry::text!(MSG_LEVEL), &[]).await;
    select_window(state, io, WindowId::Messages).await;
}

/// Original: 0x1078d2 (ui: redraw the book page after a page step; when the step moved the
/// book (A4-0x7a5c), its first line becomes the current spell and gets the marker)
pub async fn redraw_after_page_step(state: &RefCell<State>, io: &Io) {
    draw_book_page(state, io).await;
    let moved = {
        let mut s = state.borrow_mut();
        let moved = s.magic.page_moved != 0;
        if moved {
            s.magic.page_moved = 0;
            let first = s.magic.page[0];
            set_current_spell(&mut s, first);
        }
        moved
    };
    if moved {
        select_window(state, io, WindowId::Status).await;
        set_cursor(state, io, 0, 1).await;
        put_raw(state, io, MARKER).await;
        select_window(state, io, WindowId::Messages).await;
    }
}
