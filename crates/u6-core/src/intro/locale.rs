//! Port-only (spec 08 Design 2, slice K4): the locale pack as `u` reads it.
//!
//! `u` runs on the game state the platform installed the pack on before the first step
//! ([`crate::locale::install`]), so [`super::run`] takes the pack, the catalogue of `u`'s strings
//! read at that install ([`crate::locale::program`]) and the Hangul table from the state as it
//! starts: the text context turns `wide` on and draws Hangul from that table, and the formats
//! game code prints are looked up at the print entry ([`format_in`]). The counts `u` makes go
//! into the state's ([`crate::locale::LocaleState::program`]) as each poll ends.
//!
//! The `u` program's `printf` has no escape layer: no article, plural letter or particle marker
//! to resolve, so a format is put as the pack has it whatever its language (the tool's gate
//! refuses a marker in it, `tools/i18n`).
//!
//! **A box too small for the translation.** The original keeps the cursor on a box's last line
//! when a line feed would leave the box (`text::line_feed`), so a translation that wraps onto
//! more lines than its box holds would draw over that line. With a pack, what fills one box (a
//! passage's case, `super::passages`; a print of the creation or the transfer, or the prints that
//! fill one buffer before its flush, [`print_formats`]) is laid out first without drawing, from
//! the same cursor, in the same view and with the same arguments ([`Pass::Measure`],
//! `text::overflows`); when a glyph would go after a line feed that could not advance, the box is
//! drawn with its English formats instead (the original fits by design: the identity pack's
//! boxes, measured so, never fall back), each catalogued one counted a miss ([`Pass::English`]).
//! Without a pack nothing is measured.

use std::rc::Rc;

use u6_formats::font::HangulFont;

use super::state::IntroState;
use super::text::{self, Arg, Entry};
use crate::locale::program::{self, Catalogue, Lookup};
use crate::locale::{Counts, Pack};
use crate::state::State;

/// The pack, the catalogue of `u`'s strings and the Hangul table of the game state `u` runs on.
#[derive(Clone, Debug)]
pub struct IntroLocale {
    pub pack: Rc<Pack>,
    pub catalogue: Rc<Catalogue>,
    pub hangul: Option<Rc<HangulFont>>,
    /// What this run of `u` counted, not yet in the state's counters (shared with
    /// [`super::run`]'s poll, which moves them).
    pub counts: Rc<Counts>,
}

impl IntroLocale {
    /// The locale layer of `state` as `u` takes it: `None` without an installed pack.
    pub fn of(state: &State) -> Option<IntroLocale> {
        let l = &state.locale;
        Some(IntroLocale {
            pack: l.pack.clone()?,
            catalogue: l.program_catalogue.clone()?,
            hangul: l.hangul.clone(),
            counts: Rc::new(Counts::default()),
        })
    }
}

/// What the pack sets in the text context main makes ([`super::main`]): `wide` on and the Hangul
/// table. Without a pack the context is the original's.
pub fn apply(st: &mut IntroState) {
    if let Some(l) = &st.locale {
        st.text.wide = true;
        st.text.hangul = l.hangul.clone();
    }
}

/// How the formats of one box are put.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pass {
    /// Drawn: a catalogued format as the pack has it (a hit) or in English (a miss).
    Draw,
    /// Laid out without drawing, to learn whether the pack's formats fit: as drawn, nothing
    /// counted.
    Measure,
    /// Drawn in English, the pack's formats not fitting the box: each catalogued one a miss.
    English,
}

/// The format `english` (read from `u`) as the program's print entry puts it in `pass`: the
/// pack's text for a catalogued format the pack has, else `english` (in [`Pass::English`],
/// `english` always). Counted as the pass has it; without a pack `english`, nothing counted.
/// Called only on a format game code prints, never on an argument it prints through `%s` (the
/// player's name, a word of `u`).
pub fn format_in(st: &IntroState, english: Vec<u8>, pass: Pass) -> Vec<u8> {
    let Some(l) = &st.locale else {
        return english;
    };
    let found = program::lookup(&l.pack, &l.catalogue, &english);
    match pass {
        Pass::Draw => found.shown(&l.counts).unwrap_or(english),
        Pass::Measure => match found {
            Lookup::Found(text) => text,
            _ => english,
        },
        Pass::English => {
            found.shown_english(&l.counts);
            english
        }
    }
}

/// `calls` (a print entry, the address of a format of `u`, its arguments) printed in order
/// through main's text context: the creation's and the transfer's print sites, the calls that
/// fill one buffer before its flush given together (one box). Each format is the print entry's
/// ([`format_in`]). With a pack, they are first laid out without drawing from the cursor and in
/// the view they start in; a glyph after a line feed that could not advance prints them all in
/// English instead (see the module doc).
pub fn print_formats(st: &mut IntroState, calls: &[(Entry, u32, &[Arg])]) {
    let english: Vec<Vec<u8>> = calls.iter().map(|&(_, at, _)| st.exe.cstr(at)).collect();
    let too_tall = st.locale.is_some() && {
        let formats: Vec<Vec<u8>> = english
            .iter()
            .map(|e| format_in(st, e.clone(), Pass::Measure))
            .collect();
        st.font.as_ref().is_some_and(|font| {
            text::overflows(&mut st.video.chip, &st.text, font, |chip, ctx, font| {
                for (&(entry, _, args), f) in calls.iter().zip(&formats) {
                    text::print_at(chip, ctx, font, entry, f, args);
                }
            })
        })
    };
    let pass = if too_tall { Pass::English } else { Pass::Draw };
    for (&(entry, _, args), e) in calls.iter().zip(english) {
        let fmt = format_in(st, e, pass);
        if let Some(font) = st.font.as_ref() {
            text::print_at(&mut st.video.chip, &mut st.text, font, entry, &fmt, args);
        }
    }
}
