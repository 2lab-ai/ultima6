//! The Cast command (0x11af10), casting from the books (0x11abd0) and the reagent use
//! (0x11615a).

use std::cell::RefCell;

use super::book::{draw_book, draw_book_page, load_book, page_line, set_quality};
use super::{BOOK_PANEL, SPELLBOOK};
use crate::engine::Io;
use crate::main_loop::game_string;
use crate::objects::{READIED, Slot, WHERE_MASK};
use crate::state::State;
use crate::ui::panels::{select_window, set_cursor};
use crate::ui::print::put_raw;
use crate::ui::printf::{Arg, printf};
use crate::ui::text::WindowId;
use crate::ui::wait::REPEAT;

/// Image strings of hunk 11.
const MSG_NO_BOOK: u32 = 0x0011_5b36;
const MSG_NO_MAGIC: u32 = 0x0011_5a9a;
const MSG_NO_SPELLS: u32 = 0x0011_5ab2;
const MSG_NOTHING: u32 = 0x0011_5ad0;
pub(super) const FMT_PAREN: u32 = 0x0011_5ada;
const FMT_NAME_QUOTE: u32 = 0x0011_5ae0;
const FMT_S: u32 = 0x0011_58b0;
const FMT_SPACE_S: u32 = 0x0011_5a62;
const MSG_QUOTE: u32 = 0x0011_5ae6;
const MSG_LEVEL_LOW: u32 = 0x0011_5ae8;
const MSG_NO_POINTS: u32 = 0x0011_5b0a;
const MSG_NO_REAGENTS: u32 = 0x0011_5b26;

/// Port-only (spec 08, slice K2; 2026-10-24): the strings Cast and the books pass to `printf` as
/// its format, for the locale layer's catalogue ([`super::PRINTED`]): the no-book, no-magic,
/// no-spells and nothing lines, the announcement's formats and closing quote, and the three
/// refusals of the checks. The announcement puts the spell's name and syllables through `%s`:
/// the name is read for display (2026-10-25, [`super::tables::NAME_LABELS`]), the syllables (the
/// game's magic words) stay English.
pub(super) const PRINTED: &[u32] = &[
    MSG_NO_BOOK,
    MSG_NO_MAGIC,
    MSG_NO_SPELLS,
    MSG_NOTHING,
    FMT_PAREN,
    FMT_NAME_QUOTE,
    FMT_S,
    FMT_SPACE_S,
    MSG_QUOTE,
    MSG_LEVEL_LOW,
    MSG_NO_POINTS,
    MSG_NO_REAGENTS,
];
/// A4-0x7b65 (timer 13): while it runs no spell can be cast (Negate Magic sets it).
const NEGATE_TIMER: usize = 13;
/// The spell Help, which uses no reagent from the page count.
const HELP: u16 = 7;

/// The caster: the party member the view follows (A4+0x47ec[A4-0x7b7a]); with the leader index
/// 16, party slot 16 (A4+0x47fc), where a spellbook's or spell object's hit puts its attacker for
/// the cast (asm 0x12467a..0x124688): [`crate::actors::party::leader_actor`].
pub(crate) fn caster(s: &State) -> u16 {
    crate::actors::party::leader_actor(s)
}

async fn print_at(state: &RefCell<State>, io: &Io, addr: u32, args: &[Arg<'_>]) {
    let fmt = game_string(io, addr);
    printf(state, io, &fmt, args).await;
}

/// Original: 0x11af10 (magic: Cast command: the caster's readied spellbooks, then 0x11abd0)
///
/// The caster's contents are walked (0x10ae52 / 0x10aed6); readied objects of type 0x39 are
/// the books, the first and the second found. Without one: the no-book line (0x115b36).
pub async fn cast(state: &RefCell<State>, io: &Io) {
    let books = {
        let mut s = state.borrow_mut();
        let actor = Slot::from(caster(&s) as i16);
        let mut found: Vec<Slot> = Vec::new();
        let mut o = crate::objects::first_child(&mut s.objects, actor);
        while o >= 0 {
            if s.objects.status(o) & WHERE_MASK == READIED && s.objects.kind(o) == SPELLBOOK {
                if found.is_empty() {
                    found.push(o);
                } else {
                    // A third book replaces the second, as the original's second register.
                    found.truncate(1);
                    found.push(o);
                }
            }
            o = crate::objects::next_child(&mut s.objects);
        }
        found
    };
    match books.as_slice() {
        [] => print_at(state, io, MSG_NO_BOOK, &[]).await,
        [a] => book_cast(state, io, *a, 0).await,
        [a, b, ..] => book_cast(state, io, *a, *b).await,
    }
}

/// Restores the panel mode the book replaced and redraws the panel.
async fn close_book(state: &RefCell<State>, io: &Io) {
    {
        let mut s = state.borrow_mut();
        s.ui.panel.mode = s.magic.saved_panel;
        s.ui.panel.dirty = true;
    }
    crate::ui::panels::redraw_panel(state, io).await;
}

/// Prints the spell's name and incantation: `(Name)` when it was typed with the cursor in a
/// mode (the typed syllables are cleared), else `Name`, a newline and the syllables in quotes.
/// Port (spec 08, 2026-10-25): the name, in either form, is read for display
/// ([`crate::locale::shown`]: the pack's form when it has one); the syllables are not.
async fn announce(state: &RefCell<State>, io: &Io, spell: u16) {
    let (typed, name, word) = {
        let s = state.borrow();
        let m = &s.magic;
        (
            s.ui.input.targeting != 0 && m.typed != 0,
            crate::locale::shown(&s, m.tables.name(usize::from(spell)).to_vec()),
            m.tables.word(usize::from(spell)).to_vec(),
        )
    };
    if typed {
        print_at(state, io, FMT_PAREN, &[Arg::Str(&name)]).await;
        state.borrow_mut().magic.typed = 0;
        return;
    }
    print_at(state, io, FMT_NAME_QUOTE, &[Arg::Str(&name)]).await;
    for (i, &c) in word.iter().enumerate() {
        let syl = state.borrow().magic.tables.syllable(c).to_vec();
        let fmt = if i == 0 { FMT_S } else { FMT_SPACE_S };
        print_at(state, io, fmt, &[Arg::Str(&syl)]).await;
    }
    print_at(state, io, MSG_QUOTE, &[]).await;
}

/// Moves the book's marker to the chosen line: blanks on every marked line, the glyph on line
/// `line`; the books remember the spell.
async fn mark_line(state: &RefCell<State>, io: &Io, line: i16, spell: u16) {
    select_window(state, io, WindowId::Status).await;
    for i in 0..8 {
        let marked = state.borrow().magic.page_reagents[i] & 0x80 != 0;
        if marked {
            set_cursor(state, io, 0, i as i16 + 1).await;
            put_raw(state, io, 0x20).await;
        }
    }
    set_cursor(state, io, 0, line.wrapping_add(1)).await;
    put_raw(state, io, super::book::MARKER).await;
    select_window(state, io, WindowId::Messages).await;
    let mut s = state.borrow_mut();
    for book in s.magic.books {
        if book != 0 {
            set_quality(&mut s.objects, Slot::from(book), spell as u8);
        }
    }
}

/// Original: 0x11abd0 (magic: cast from the readied books `book1` / `book2` (0 = none))
///
/// Refused while the Negate Magic timer (A4-0x7b65) runs or when the books hold no spell. The book
/// is drawn (0x115da4, 0x116028, 0x116218) in panel mode 0xa1 and one key is read: a confirmed
/// target (0x8e) with a page line selected picks that line's spell; a map target (A4-0x2f02 /
/// -0x2f00 set) casts the current spell on it at once; anything else says the nothing line
/// (0x115ad0). The choice is announced, the marker moves to the line and the books keep the spell,
/// the panel comes back. Then the checks in order: the caster's level against the circle, the magic
/// points, the reagents of the line; on success the reagents (0x11615a), the points (one per
/// circle) and 3 × circle + 10 movement points (0x11c650) are spent and the spell runs (0x11a586).
pub async fn book_cast(state: &RefCell<State>, io: &Io, book1: Slot, book2: Slot) {
    let actor = {
        let mut s = state.borrow_mut();
        s.magic.books = [book1, book2];
        if s.actors.timers[NEGATE_TIMER] != 0 {
            None
        } else {
            Some(caster(&s))
        }
    };
    let Some(actor) = actor else {
        print_at(state, io, MSG_NO_MAGIC, &[]).await;
        return;
    };
    let any = {
        let mut s = state.borrow_mut();
        load_book(&mut s);
        s.magic.known.iter().any(|&w| w != 0)
    };
    if !any {
        print_at(state, io, MSG_NO_SPELLS, &[]).await;
        return;
    }
    draw_book(&mut state.borrow_mut(), io);
    draw_book_page(state, io).await;
    {
        let mut s = state.borrow_mut();
        s.magic.saved_panel = s.ui.panel.mode;
        s.ui.panel.mode = BOOK_PANEL;
    }
    let key = crate::ui::wait::read_key(state, io).await;
    let (sel, target) = {
        let s = state.borrow();
        (s.ui.input.selection, s.ui.input.target)
    };
    let confirmed = key & 0xff == REPEAT;
    if !(confirmed && sel != -1) && target.0 == -1 {
        print_at(state, io, MSG_NOTHING, &[]).await;
        let mut s = state.borrow_mut();
        s.ui.panel.mode = s.magic.saved_panel;
        s.ui.panel.dirty = true;
        return;
    }
    let (spell, at_target) = {
        let s = state.borrow();
        if target.0 != -1 && target.1 != -1 {
            (s.magic.current_spell(), true)
        } else {
            let line = usize::try_from(sel).unwrap_or(0).min(7);
            (u16::from(s.magic.page[line]), false)
        }
    };
    announce(state, io, spell).await;
    let circle = spell / 16 + 1;
    if !at_target {
        mark_line(state, io, sel, spell).await;
    }
    close_book(state, io).await;
    if !checks_pass(state, io, actor, spell, circle).await {
        return;
    }
    {
        let mut s = state.borrow_mut();
        let sel = s.ui.input.selection;
        use_reagents(&mut s, Slot::from(actor as i16), spell, sel);
        let a = usize::from(actor);
        s.actors.attrs.magic[a] = s.actors.attrs.magic[a].wrapping_sub(circle as u8);
        crate::actors::walk::spend_mp(&mut s, actor, (circle * 4 - circle + 10) as i16);
    }
    super::dispatch(state, io, spell, at_target).await;
    close_book(state, io).await;
}

/// The level, magic-point and reagent checks of 0x11abd0, with their messages.
async fn checks_pass(state: &RefCell<State>, io: &Io, actor: u16, spell: u16, circle: u16) -> bool {
    let (level, points, reagents) = {
        let s = state.borrow();
        let a = usize::from(actor);
        let line = page_line(&s.magic, spell);
        // Line -1 reads the byte before the counts (the last page byte, A4+0x17ff).
        let count = if line < 0 {
            s.magic.page[7]
        } else {
            s.magic.page_reagents[line as usize]
        };
        (
            u16::from(s.actors.attrs.level[a]),
            u16::from(s.actors.attrs.magic[a]),
            count & 0x7f,
        )
    };
    let refusal = if level < circle {
        MSG_LEVEL_LOW
    } else if points < circle {
        MSG_NO_POINTS
    } else if reagents == 0 {
        MSG_NO_REAGENTS
    } else {
        return true;
    };
    print_at(state, io, refusal, &[]).await;
    false
}

/// Original: 0x11615a (magic: use one of each reagent spell `spell` needs from `actor`, and
/// count one cast off page line `line`)
///
/// For each reagent bit, the list after the actor is searched (to the end of the list, not
/// only the inventory) for the reagent's type; at the first match one is taken from the actor
/// (0x10d16e, any depth). Help (7) leaves the page count alone. The original decrements the
/// count byte at A4+0x1800 + `line` for any `line` (a map target leaves the target object
/// there); the port only touches lines 0..8.
pub fn use_reagents(s: &mut State, actor: Slot, spell: u16, line: i16) {
    let mask = s.magic.tables.mask(usize::from(spell));
    for i in 0..8 {
        if mask >> i & 1 == 0 {
            continue;
        }
        let kind = s.magic.tables.reagents[i];
        let mut o = s.objects.next_of(actor);
        while o >= 0 {
            if s.objects.kind(o) == kind {
                crate::objects::take_from_owner(s, actor, kind, 1);
                break;
            }
            o = s.objects.next_of(o);
        }
    }
    if spell != HELP
        && let Some(b) = usize::try_from(line)
            .ok()
            .and_then(|i| s.magic.page_reagents.get_mut(i))
    {
        *b = b.wrapping_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::caster;
    use crate::state::State;

    #[test]
    fn the_caster_is_party_slot_16_while_the_leader_index_is_16() {
        // 0x124232 writes the attacker to A4+0x47fc, the byte after the 16 members, and the leader
        // index 16 around a spellbook's or spell object's cast (asm 0x12467a..0x124688); past it
        // the roster reads on into A4+0x47fd, as 0x1095bc's index 0xff does.
        let mut s = State::empty();
        s.actors.party.members[0] = 1;
        s.actors.party.members[15] = 2;
        s.actors.party.slot16 = 0x10;
        s.actors.attrs.unknown_1af1[0xee] = 9;
        assert_eq!(caster(&s), 1);
        s.actors.party.leader_index = 16;
        assert_eq!(caster(&s), 0x10);
        s.actors.party.leader_index = 0xff;
        assert_eq!(caster(&s), 9, "A4+0x47ec + 0xff");
        // The panel's member 16 (Pickpocket leaves the leader index there) is slot 16 too.
        assert_eq!(crate::ui::panels::member_actor(&s, 16), 0x10);
    }
}
