//! Look (0x1298b8) and what it reads: books and signs (0x12909a, 0x1290dc, 0x12911e, 0x1292d0).
//!
//! Port (spec 08, slice K3): the names Look prints are read to be shown
//! ([`crate::objects::shown_name`], [`crate::objects::shown_look`]): the locale pack's object
//! names when it has them, with no article before a Korean one; a book or sign is the pack's when
//! it has it ([`read_book`]).

use std::cell::RefCell;

use super::seams;
use super::text::{print_at, putc};
use super::world;
use crate::engine::Io;
use crate::objects::iter::{begin_at, first_child, next_at, tile_of};
use crate::objects::{Slot, shown_name};
use crate::state::State;
use crate::ui::printf::Arg;

/// Strings of hunk 15 (image addresses).
const S_THOU_DOST_SEE: u32 = 0x0012_8444;
const S_DARKNESS: u32 = 0x0012_8454;
const S_NOTHING: u32 = 0x0012_8460;
const S_SEARCH_NOTHING: u32 = 0x0012_8468;
const S_IT_WEIGHS: u32 = 0x0012_848c;
const S_THEY_WEIGH: u32 = 0x0012_8498;
const S_STONES: u32 = 0x0012_84a6;
const S_IT: u32 = 0x0012_84b4;
const S_COMMA: u32 = 0x0012_84ba;
const S_AND: u32 = 0x0012_84bc;
const S_DAMAGE: u32 = 0x0012_84c2;
const S_ABSORB: u32 = 0x0012_84e0;
const S_COLON: u32 = 0x0012_8506;
const S_NAME_DASH: u32 = 0x0012_8508;
const S_WORD: u32 = 0x0012_850c;
const S_SPACE_WORD: u32 = 0x0012_8510;
const S_INGREDIENTS: u32 = 0x0012_8514;
const S_NONE: u32 = 0x0012_8524;
const S_REAGENT: u32 = 0x0012_852a;
const S_TIME_IS: u32 = 0x0012_8530;
const S_MIDNIGHT: u32 = 0x0012_8540;
const S_AM: u32 = 0x0012_854e;
const S_NOON: u32 = 0x0012_855c;
const S_PM: u32 = 0x0012_856a;
const S_DOT: u32 = 0x0012_8440;
const S_BLANK_LINES: u32 = 0x0012_83f0;
const S_BOOK_DAT: u32 = 0x0012_83f4;

/// Port-only (spec 08, slice K2): the strings this module passes to `printf` as its format,
/// for the locale layer's catalogue ([`crate::locale::strings`]).
/// The book file's name (read by logic, not printed) is left out.
pub(super) const PRINTED: &[u32] = &[
    S_THOU_DOST_SEE,
    S_DARKNESS,
    S_NOTHING,
    S_SEARCH_NOTHING,
    S_IT_WEIGHS,
    S_THEY_WEIGH,
    S_STONES,
    S_IT,
    S_COMMA,
    S_AND,
    S_DAMAGE,
    S_ABSORB,
    S_COLON,
    S_NAME_DASH,
    S_WORD,
    S_SPACE_WORD,
    S_INGREDIENTS,
    S_NONE,
    S_REAGENT,
    S_TIME_IS,
    S_MIDNIGHT,
    S_AM,
    S_NOON,
    S_PM,
    S_DOT,
    S_BLANK_LINES,
];

/// Original: A4-0x6632 (5 words: the book types), A4-0x6628 (5 words: the sign types)
const BOOK_TYPES: i32 = -0x6632;
const SIGN_TYPES: i32 = -0x6628;
/// Original: A4-0x6f4c / -0x71b4 / -0x70b8 / -0x6d1c / -0x6d4c (spell names, each spell's
/// syllable indices, the syllables, each spell's reagent bits, the reagent names)
const SPELL_NAMES: i32 = -0x6f4c;
const SPELL_WORDS: i32 = -0x71b4;
const SYLLABLES: i32 = -0x70b8;
const SPELL_REAGENTS: i32 = -0x6d1c;
const REAGENT_NAMES: i32 = -0x6d4c;

/// Original: 0x1292d0's buffer (the most bytes of a book or sign it reads).
pub const BOOK_BYTES: usize = 0x2800;

/// Object types Look treats specially.
const HORSE: u16 = 0x1ae;
const SPELLBOOK: u16 = 0x39;
const CLOCK: u16 = 0x9f;
const SUNDIAL: u16 = 0xeb;
/// Tiles of the two halves of a bed (the name and weight come from the head).
const BED_FOOT_V: u16 = 0x4dc;
const BED_FOOT_H: u16 = 0x4dd;
/// Panel mode while the spellbook is open.
const PANEL_SPELLBOOK: u16 = 0xa1;
/// The key read gives this when a cursor confirmed its target.
const TARGET_DONE: u16 = crate::ui::wait::REPEAT;

fn in_list(s: &State, off: i32, kind: u16) -> bool {
    (0..5).any(|i| s.objcmds.data.a4_word(off + 2 * i) == kind)
}

/// Original: 0x12909a (objects: `obj` is a book: its type is one of the 5 at A4-0x6632)
pub fn is_book(s: &State, obj: Slot) -> bool {
    in_list(s, BOOK_TYPES, s.objects.kind(obj))
}

/// Original: 0x1290dc (objects: `obj` is a sign: its type is one of the 5 at A4-0x6628)
pub fn is_sign(s: &State, obj: Slot) -> bool {
    in_list(s, SIGN_TYPES, s.objects.kind(obj))
}

/// Original: 0x12911e (objects: Look may read `obj`: a sign, or a book carried (the selection's
/// status) or within one tile of the leader)
pub fn is_readable(s: &State, obj: Slot) -> bool {
    if !is_book(s, obj) {
        return is_sign(s, obj);
    }
    if s.objects.place(s.ui.input.selection) != 0 {
        return true;
    }
    let leader = leader(s);
    let near = |a: u16, b: u16| (a as i16).wrapping_sub(b as i16).wrapping_abs() <= 1;
    near(s.objects.x(obj), s.objects.x(leader)) && near(s.objects.y(obj), s.objects.y(leader))
}

fn leader(s: &State) -> Slot {
    let p = &s.actors.party;
    Slot::from(p.members[usize::from(p.leader_index).min(15)])
}

/// Original: 0x1292d0 (objects: print book text `obj`'s quality names from `book.dat`)
///
/// Music off (0x110878), two newlines, then the entry: `book.dat` starts with 16-bit
/// little-endian offsets, entry q − 1 for quality q; the text there (at most [`BOOK_BYTES`], to
/// its NUL; [`crate::locale::texts::book_text`]) is printed as a printf format in a paged window;
/// the next key read unpages. Then the area music again (0x110912).
///
/// Port (spec 08, slice K3): the book the locale pack has for that entry, its bytes still the
/// ones read ([`crate::locale::book_at`]), is printed instead, in the pack's grammar and not
/// looked up again ([`crate::ui::printf::format`]); any other prints in English.
pub async fn read_book(state: &RefCell<State>, io: &Io, obj: Slot) {
    seams::music_off(&mut state.borrow_mut());
    let q = (state.borrow().objects.qq_of(obj) >> 8) as u8;
    print_at(state, io, S_BLANK_LINES, &[]).await;
    let name = super::text::string(&state.borrow(), S_BOOK_DAT).to_ascii_lowercase();
    let file = io
        .files()
        .get(&String::from_utf8_lossy(&name))
        .unwrap_or(&[]);
    let index = usize::from(q).wrapping_sub(1);
    let text = crate::locale::texts::book_text(file, index);
    {
        let mut s = state.borrow_mut();
        s.ui.text.win_mut().flags |= crate::ui::text::PAGED;
    }
    let found = crate::locale::book_at(&state.borrow(), file, index, &text);
    match found {
        Some((language, put)) => {
            crate::main_loop::flush_prints(state, io).await;
            let was = std::mem::replace(&mut state.borrow_mut().ui.text.language, language);
            crate::ui::printf::format(state, io, &put, &[]).await;
            state.borrow_mut().ui.text.language = was;
        }
        None => super::text::print(state, io, &text, &[]).await,
    }
    state.borrow_mut().ui.input.unpage = true;
    seams::music_on(&mut state.borrow_mut());
}

/// Original: 0x1298b8 (objects: the Look command on the confirmed target)
///
/// The lead-in (0x128444) in a paged window, the leader turned toward the pointer (0x105016,
/// 0x11ba5e) and the view updated. A dark target: the darkness line (0x128454). No object (or
/// an invisible one): the map tile's article and look name, or the nothing word (0x128460)
/// outside the view, then the full stop and, within one tile of the leader, the found-nothing
/// line (0x128468). An actor or horse
/// (through combat 0x1240fa) or a body: its count / article (unless it is a party member's
/// shown portrait), name, and outside a ship its portrait panel (0x128ce8). Any other object:
/// count / article and name (a bed's foot names its head), the weight with its contents in
/// stones, the damage it does and absorbs, then by kind: a book or sign with text is read
/// (all of them on the tile when it lies on the map), an open spellbook in reach shows the
/// spell picked with its words and ingredients, a clock or a sundial by day tells the time,
/// else ".". Unless a text was read, the spot is searched (0x129544). The window unpages at
/// the next key.
///
/// Port-only (owner, 2026-10-21): a plant by a gatehouse door, looked at within one tile of the
/// leader, may give the gatehouse key ([`super::gatehouse`]).
pub async fn look(state: &RefCell<State>, io: &Io) {
    let mut read = false;
    state.borrow_mut().ui.text.win_mut().flags |= crate::ui::text::PAGED;
    print_at(state, io, S_THOU_DOST_SEE, &[]).await;
    {
        let mut s = state.borrow_mut();
        let (px, py) = s.ui.input.pointer;
        let dir = seams::direction_of(&s, px, py);
        if dir != -1 {
            let l = leader(&s) as u16;
            crate::actors::walk::face(&mut s, l, dir as u16);
            crate::map::view::update_view(&mut s);
        }
    }
    if state.borrow().ui.input.target_void {
        print_at(state, io, S_DARKNESS, &[]).await;
        state.borrow_mut().ui.input.unpage = true;
        return;
    }
    let (lx, ly) = {
        let s = state.borrow();
        let l = leader(&s);
        (s.objects.x(l), s.objects.y(l))
    };
    let sel = state.borrow().ui.input.selection;
    if sel == -1 || state.borrow().objects.status(sel) & 0x02 != 0 {
        look_at_tile(state, io, lx, ly).await;
        state.borrow_mut().ui.input.unpage = true;
        return;
    }
    let kind = state.borrow().objects.kind(sel);
    if sel < 0x100 || kind == HORSE {
        let mut s = state.borrow_mut();
        s.ui.input.selection = seams::rider(&mut s, sel);
    }
    let sel = state.borrow().ui.input.selection;
    if matches!(kind, 0x18d..=0x18f) || sel < 0x100 {
        look_at_actor(state, io, sel).await;
    } else {
        read = look_at_object(state, io, sel, kind, lx, ly).await;
    }
    if !read {
        let mut s = state.borrow_mut();
        let (sel, (tx, ty)) = (s.ui.input.selection, s.ui.input.target);
        world::search(&mut s, sel, tx as u16, ty as u16);
    }
    crate::main_loop::flush_prints(state, io).await;
    state.borrow_mut().ui.input.unpage = true;
}

/// Look with no object: the map tile under the pointer.
///
/// Port-only (owner, 2026-10-21): within one tile of the leader, a plant by a gatehouse door
/// may give the gatehouse key instead of the found-nothing line ([`super::gatehouse`]).
async fn look_at_tile(state: &RefCell<State>, io: &Io, lx: u16, ly: u16) {
    let (px, py) = state.borrow().ui.input.pointer;
    let mut looked = None;
    if px < 0xa8 && py < 0xa8 {
        let name = {
            let mut s = state.borrow_mut();
            let c = s.map.centre;
            let tx = (px >> 4).wrapping_add(c.x as i16).wrapping_sub(5);
            let ty = (py >> 4).wrapping_add(c.y as i16).wrapping_sub(5);
            s.ui.input.target = (tx, ty);
            let tile = s
                .map
                .tile_at(tx as u16 as usize, ty as u16 as usize, usize::from(c.z))
                .unwrap_or(0);
            looked = Some(tile);
            world::article(&mut s, u16::from(tile));
            crate::objects::shown_look(&s, u16::from(tile)).into_bytes()
        };
        super::text::print(state, io, &name, &[]).await;
    } else {
        print_at(state, io, S_NOTHING, &[]).await;
    }
    print_at(state, io, S_DOT, &[]).await;
    let (tx, ty) = state.borrow().ui.input.target;
    let near = |a: i16, b: u16| a.wrapping_sub(b as i16).wrapping_abs() <= 1;
    if near(tx, lx) && near(ty, ly) {
        let gave = looked.is_some_and(|tile| {
            super::gatehouse::give_key(&mut state.borrow_mut(), tx as u16, ty as u16, tile)
        });
        if gave {
            crate::main_loop::flush_prints(state, io).await;
        } else {
            print_at(state, io, S_SEARCH_NOTHING, &[]).await;
        }
    }
}

/// Look at an actor (or a body): prefix, name, portrait.
async fn look_at_actor(state: &RefCell<State>, io: &Io, sel: Slot) {
    let aboard = {
        let s = state.borrow();
        s.actors.party.leader_index == s.actors.party.size
    };
    let name = {
        let mut s = state.borrow_mut();
        let shown = s
            .actors
            .attrs
            .status
            .get(sel as usize)
            .is_some_and(|&b| b & 0x80 != 0);
        if !shown || aboard {
            world::name_prefix(&mut s, sel);
        }
        shown_name(&s, sel).into_bytes()
    };
    super::text::print(state, io, &name, &[]).await;
    putc(state, io, b'\n').await;
    if !aboard {
        crate::objects::show_actor(state, io, sel).await;
    }
}

/// Look at an object; true when a text was read (no search then).
async fn look_at_object(
    state: &RefCell<State>,
    io: &Io,
    sel: Slot,
    kind: u16,
    lx: u16,
    ly: u16,
) -> bool {
    let mut read = false;
    let count = {
        let mut s = state.borrow_mut();
        let s = &mut *s;
        let tile = tile_of(&s.tables, s.objects.tf(sel));
        let count = world::name_prefix(s, sel);
        let (tx, ty) = s.ui.input.target;
        let z = u16::from(s.map.centre.z);
        if tile == BED_FOOT_V {
            s.ui.input.selection = begin_at(
                &mut s.objects,
                &s.tables,
                tx as u16,
                ty.wrapping_sub(1) as u16,
                z,
            );
        } else if tile == BED_FOOT_H {
            s.ui.input.selection = begin_at(
                &mut s.objects,
                &s.tables,
                tx.wrapping_sub(1) as u16,
                ty as u16,
                z,
            );
        }
        count
    };
    let sel = state.borrow().ui.input.selection;
    let name = shown_name(&state.borrow(), sel).into_bytes();
    super::text::print(state, io, &name, &[]).await;
    let wt = {
        let s = state.borrow();
        seams::weight(&s, sel).wrapping_add(seams::contents_weight(&s, sel))
    };
    if wt != 0 && wt < 0xff {
        print_at(
            state,
            io,
            if count == 1 {
                S_IT_WEIGHS
            } else {
                S_THEY_WEIGH
            },
            &[],
        )
        .await;
        let args = [Arg::Word((wt / 10) as u16), Arg::Word((wt % 10) as u16)];
        print_at(state, io, S_STONES, &args).await;
    }
    let (dmg, absorb) = {
        let s = state.borrow();
        (seams::damage(&s, sel), seams::armour(&s, sel))
    };
    if dmg > 0 {
        let lead = if wt == 0 || wt >= 0xff {
            S_IT
        } else if absorb != 0 {
            S_COMMA
        } else {
            S_AND
        };
        print_at(state, io, lead, &[]).await;
        print_at(state, io, S_DAMAGE, &[Arg::Word(dmg as u16)]).await;
    }
    if absorb > 0 {
        print_at(state, io, S_ABSORB, &[Arg::Word(absorb as u16)]).await;
    }
    if is_readable(&state.borrow(), sel) {
        if state.borrow().objects.place(sel) != 0 {
            if state.borrow().objects.qq_of(sel) >> 8 != 0 {
                read_one(state, io, sel, &mut read).await;
            }
        } else {
            let (tx, ty) = state.borrow().ui.input.target;
            let mut c = {
                let mut s = state.borrow_mut();
                let s = &mut *s;
                let z = u16::from(s.map.centre.z);
                begin_at(&mut s.objects, &s.tables, tx as u16, ty as u16, z)
            };
            while c >= 0 {
                if state.borrow().objects.qq_of(c) >> 8 != 0 {
                    read_one(state, io, c, &mut read).await;
                }
                let mut s = state.borrow_mut();
                let s = &mut *s;
                c = next_at(&mut s.objects, &s.tables);
            }
        }
        putc(state, io, b'\n').await;
        return read;
    }
    let book_open = {
        let mut s = state.borrow_mut();
        let (tx, ty) = s.ui.input.target;
        let near = |a: i16, b: u16| a.wrapping_sub(b as i16).wrapping_abs() <= 1;
        kind == SPELLBOOK
            && first_child(&mut s.objects, sel) >= 0
            && ((near(tx, lx) && near(ty, ly)) || s.objects.place(sel) != 0)
    };
    if book_open {
        spellbook(state, io, sel).await;
        return read;
    }
    let hour = state.borrow().actors.calendar.hour;
    if kind == CLOCK || (kind == SUNDIAL && (5..0x14).contains(&hour)) {
        tell_time(state, io).await;
    } else {
        print_at(state, io, S_DOT, &[]).await;
    }
    read
}

/// One readable object: ":" before the first, then its text.
async fn read_one(state: &RefCell<State>, io: &Io, obj: Slot, read: &mut bool) {
    if !*read {
        print_at(state, io, S_COLON, &[]).await;
    }
    read_book(state, io, obj).await;
    *read = true;
}

/// Look's spellbook: the book's owner leads while the book is open (magic 0x115da4, 0x116028,
/// 0x116218, panel mode 0xa1); a spell confirmed with the cursor is described.
async fn spellbook(state: &RefCell<State>, io: &Io, book: Slot) {
    let saved_leader = {
        let mut s = state.borrow_mut();
        seams::open_spellbook(&mut s, book);
        let saved = s.actors.party.leader_index;
        let owner = s.objects.parent(book);
        if let Some(i) = s
            .actors
            .party
            .members
            .iter()
            .take(8)
            .position(|&m| Slot::from(m) == owner)
        {
            s.actors.party.leader_index = i as u8;
        }
        seams::show_spellbook(&mut s, io);
        saved
    };
    crate::magic::draw_book_page(state, io).await;
    let saved_mode = {
        let mut s = state.borrow_mut();
        let mode = s.ui.panel.mode;
        s.ui.panel.mode = PANEL_SPELLBOOK;
        mode
    };
    let key = crate::ui::wait::read_key(state, io).await;
    let sel = state.borrow().ui.input.selection;
    if key & 0xff == TARGET_DONE & 0xff && sel != -1 {
        let spell = seams::spell_in_slot(&state.borrow(), sel);
        describe_spell(state, io, spell).await;
    } else {
        print_at(state, io, S_DOT, &[]).await;
    }
    let mut s = state.borrow_mut();
    s.actors.party.leader_index = saved_leader;
    s.ui.panel.mode = saved_mode;
    s.ui.panel.member = 0;
    s.ui.panel.dirty = true;
}

/// The spell's name, its words and its ingredients.
async fn describe_spell(state: &RefCell<State>, io: &Io, spell: u8) {
    let (name, words, reagents, names) = {
        let s = state.borrow();
        let d = &s.objcmds.data;
        let at = |off: i32, i: u32| super::GameData::a4(off).wrapping_add(4 * i);
        let name = d.pointed(at(SPELL_NAMES, u32::from(spell)));
        let list = d.long(at(SPELL_WORDS, u32::from(spell)));
        let mut words = Vec::new();
        let mut k = 0;
        loop {
            let w = d.byte(list.wrapping_add(k));
            if k > 0 && w == 0 {
                break;
            }
            words.push(d.pointed(at(SYLLABLES, u32::from(w))));
            k += 1;
        }
        let reagents = d.a4_byte(SPELL_REAGENTS + i32::from(spell));
        let names: Vec<Vec<u8>> = (0..8).map(|b| d.pointed(at(REAGENT_NAMES, b))).collect();
        (name, words, reagents, names)
    };
    print_at(state, io, S_BLANK_LINES, &[]).await;
    print_at(state, io, S_NAME_DASH, &[Arg::Str(&name)]).await;
    for (i, w) in words.iter().enumerate() {
        let fmt = if i == 0 { S_WORD } else { S_SPACE_WORD };
        print_at(state, io, fmt, &[Arg::Str(w)]).await;
    }
    print_at(state, io, S_INGREDIENTS, &[]).await;
    if reagents == 0 {
        print_at(state, io, S_NONE, &[]).await;
    }
    for (b, n) in names.iter().enumerate() {
        if (reagents >> b) & 1 != 0 {
            print_at(state, io, S_REAGENT, &[Arg::Str(n)]).await;
        }
    }
}

/// The time line: its lead-in (0x128530), then by hour one of four formats: hour 0 with the
/// minutes (0x128540), 1..=11 as h:mm (0x12854e), 12 with the minutes (0x12855c), later hours
/// as (h - 12):mm (0x12856a).
async fn tell_time(state: &RefCell<State>, io: &Io) {
    print_at(state, io, S_TIME_IS, &[]).await;
    let (h, m) = {
        let c = state.borrow().actors.calendar;
        (u16::from(c.hour), u16::from(c.minute))
    };
    match h {
        0 => print_at(state, io, S_MIDNIGHT, &[Arg::Word(m)]).await,
        1..=11 => print_at(state, io, S_AM, &[Arg::Word(h), Arg::Word(m)]).await,
        12 => print_at(state, io, S_NOON, &[Arg::Word(m)]).await,
        _ => print_at(state, io, S_PM, &[Arg::Word(h - 12), Arg::Word(m)]).await,
    }
}
