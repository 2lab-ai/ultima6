//! Spec 16 item 1 (2026-10-27): the static scan of the print sites. Every call in the library's
//! sources (less `intro/`, `locale/` and test code, as each build, with the `wide` feature and
//! without, compiles them) that hands a string's image address to a reader or a print wrapper
//! ([`READERS`]) resolves to an address the catalogue reads ([`listed`]) or has an entry in
//! [`NOT_CATALOGUED`] with its reason; so has an argument the scan cannot resolve, naming the
//! consts or slots it takes (each checked listed) or why it needs none. Every entry, form entry,
//! glob entry and reason must be needed. Addresses and the port's code only.
//!
//! The contract (2026-10-27, round 5):
//! 1. **Exact for the crate's own idioms.** For every form the library uses today, a reader call
//!    either resolves to a listed address or is a finding.
//! 2. **Exotic forms are rejected outright.** Each is one syntactic check and a finding where it
//!    appears in the scanned library, with no modeling of what it would mean: `include!` (any of
//!    its forms) and `#[path = …]` on a module; a path with a leading `::`;
//!    `#![no_implicit_prelude]`, and any item named `From` or `Into` that the library defines
//!    itself; a block or an expression as a generic argument on a callee path (`f::<{ … }>`;
//!    `f::<N + 1>` does not parse); `use … as <name>`, where `<name>` is also a module name in
//!    scope.
//! 3. **What a MUST is.** A MUST needs either a site in current production, or a form the crate
//!    uses today that a new place could plausibly use. Anything that takes deliberately unusual
//!    code is at most a SHOULD.
//!
//! Under it, the scan is fail-closed by three rules (2026-10-27, rounds 4 and 5):
//! 1. A reader is found wherever its name appears: a path naming one anywhere but a call's callee
//!    (a value, an argument, a macro's argument), a `use` that renames one or `Arg`, a variant of
//!    `Arg` brought in by its bare name, and a glob that brings `Arg`'s variants or a reader's
//!    name in ([`GLOBS`] the ones whose calls keep the reader's name) are findings; a pattern
//!    binding of a reader's name is no reader where it binds: its scope is its fn's, block's,
//!    closure's, arm's, `for` loop's, or `if let`'s or `while let`'s (a `let`'s initializer is
//!    read before it binds; a nested fn does not see it). A callee no plain path that names one
//!    is a form.
//! 2. A value comes only from a proven, unique binding: every binding of a name in the scopes it
//!    is read from agrees, a pattern binding or a glob between it and the read that could bind
//!    it hides it, a path's head is looked up from the innermost scope out (a fn's `use` of a
//!    module before the module's child module of that name), `GameData::a4` only through
//!    `objects::commands::data`'s `GameData`, a primitive's `from` and bounds only with no item
//!    of its name in scope, a module path two `cfg` variants share names nothing, and no value
//!    (an empty array) is `None`, not the no-string 0. A `static` binds as a const does (a
//!    `static mut` with no value); associated consts are not read: a site naming one is not
//!    resolved (a finding). An unsuffixed literal takes the integer type its context gives it
//!    (a const's declared type); with none, an operation whose result depends on the width
//!    (`<< >> % & |`) is `None`. An entry names a const by its binding, not by its spelling.
//! 3. Every integer const (any width, an array's elements each) whose value lands on a byte of a
//!    complete executable string (a run between NULs the catalogue holds or counts uncatalogued)
//!    is listed at that address, named in an entry or has its reason ([`const_pass`], on the
//!    owner's game), and so is every integer literal of an image address outside consts and
//!    sites ([`LITERAL_REASONS`]); every integer const the scan cannot evaluate has its reason,
//!    and every string const (`S_`, `STR_`, `MSG_`, `FMT_`) is listed, named or has its reason
//!    (both offline). The blind spot: a string that follows code with no NUL before it is no
//!    complete string.
//!
//! What the scan cannot read exactly (a source that does not parse, a value out of its type, a
//! call in no reader's form) is a finding, never a wrong value or a skipped site.
#![allow(clippy::disallowed_methods)] // the test reads the crate's sources and the owner's zip

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use proc_macro2::{Span, TokenStream, TokenTree};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Attribute, Expr, Item, Meta, Stmt, Token, UseTree};

use crate::image::{A4, IMAGE_BASE};

/// The top modules the scan leaves out: the `u` program's (its own catalogue,
/// [`super::program`]) and the locale layer itself.
const SKIPPED: [&str; 2] = ["intro", "locale"];

/// A function that takes a string's image address, a reader or a print wrapper: the file it is
/// local to (empty: any), its name (`Arg::` names the variants of `actors::seams::Arg`), the
/// arguments a call passes, whether it is a method, which argument is the address (an image
/// address, an offset from A4 or a pointer slot's, by its value and type: [`address`]), and
/// whether 0 there means no string.
pub(super) struct Reader {
    scope: &'static str,
    name: &'static str,
    arity: usize,
    method: bool,
    args: &'static [usize],
    zero: bool,
}

const fn reader(name: &'static str, arity: usize, method: bool, args: &'static [usize]) -> Reader {
    Reader {
        scope: "",
        name,
        arity,
        method,
        args,
        zero: false,
    }
}

/// The readers.
pub(super) const READERS: &[Reader] = &[
    reader("game_string", 2, false, &[1]),
    reader("a4_string", 2, false, &[1]),
    reader("pointed_string", 2, false, &[1]),
    reader("say", 4, false, &[2]),
    reader("say", 3, false, &[1]),
    reader("say", 2, true, &[0, 1]),
    reader("print_at", 4, false, &[2]),
    reader("queue_at", 3, false, &[1]),
    reader("print", 2, true, &[0]),
    // `actors::seams::print` (0: the format is its first argument) and `objects::seams::print`.
    Reader {
        zero: true,
        ..reader("print", 3, false, &[1])
    },
    reader("print_a4", 1, true, &[0]),
    reader("print_image", 2, false, &[1]),
    reader("string", 2, false, &[1]),
    reader("string", 1, true, &[0]),
    reader("cstr", 1, true, &[0]),
    reader("pointed", 1, true, &[0]),
    reader("line", 2, false, &[1]),
    reader("message", 2, false, &[1]),
    reader("a4_pointed", 2, false, &[1]),
    reader("a4_pointed", 3, false, &[1]),
    reader("a4_pointed_list", 3, false, &[1]),
    reader("Arg::Text", 1, false, &[0]),
    reader("Arg::Image", 1, false, &[0]),
    reader("Arg::Pointer", 1, false, &[0]),
    Reader {
        scope: "ui/tables.rs",
        ..reader("s", 1, false, &[0])
    },
];

/// The readers no call uses yet (a variant of `actors::seams::Arg` no code builds): scanned for
/// when one does.
const UNBUILT: [&str; 1] = ["Arg::Text"];

/// Calls with a reader's name in a form no reader takes, which take no address: the files (each
/// must hold such a call), the name, the arguments, whether a method, and why.
pub(super) type FormEntry = (
    &'static [&'static str],
    &'static str,
    usize,
    bool,
    &'static str,
);

pub(super) const OTHER_FORMS: &[FormEntry] = &[
    (
        &[
            "converse/eval.rs",
            "converse/exec.rs",
            "converse/party.rs",
            "converse/run.rs",
            "converse/start.rs",
        ],
        "print",
        2,
        false,
        "converse::seams::print: the conversation's runs, and its own messages (cx.msg), looked up through converse::text::SHOWN",
    ),
    (
        &["converse/disasm/bundle.rs"],
        "line",
        0,
        true,
        "the JSON reader's error position",
    ),
    (
        &["magic/effects.rs"],
        "message",
        1,
        true,
        "a result message of the loaded table, a label (magic::LABELS)",
    ),
    (
        &[
            "objects/commands/drop.rs",
            "objects/commands/get.rs",
            "objects/commands/keys.rs",
            "objects/commands/look.rs",
            "objects/commands/push.rs",
            "objects/commands/text.rs",
        ],
        "print",
        4,
        false,
        "objects::commands::text::print: bytes (a name) as the format",
    ),
];

/// The glob `use`s that bring a reader's name in, each with why: by file and the reader. A
/// call through one is a callee of the reader's own name, which the scan reads.
pub(super) const GLOBS: &[(&str, &str, &str)] = &[(
    "actors/rest/command.rs",
    "say",
    "actors::rest's say, which Rest calls by its own name",
)];

/// Why a site's address is not catalogued, or what an argument the scan cannot resolve takes.
#[derive(Clone, Copy, Debug)]
pub(super) enum Why {
    /// No word of its own or under 3 bytes: never catalogued (checked on the owner's game).
    NoWord,
    /// Its bytes are a listed string's: catalogued by them (checked on the owner's game).
    Same(&'static str),
    /// Logic reads it (a file's name, a table compared with typed text): never shown.
    Logic(&'static str),
    /// Shown, but in English on purpose (the uncatalogued count leaves it out too:
    /// `strings::ENGLISH_AT`, `strings::english_slots`), with the reason.
    English(&'static str),
    /// Not resolved: a loader's read into a table, with where the table is shown.
    Table(&'static str),
    /// Not resolved: the wrapper's own parameter, handed on; its callers are the sites
    /// ([`READERS`] holds each wrapper named).
    Forwarded(&'static [&'static str]),
    /// Not resolved: a variable that takes these consts of the file (each checked listed; the
    /// const sweep finds one it could take that is not).
    Consts(&'static [&'static str]),
    /// Not resolved: a pointer table's slot or the string one points at, the first slot and how
    /// many (each checked listed).
    Slots(u32, u32),
}

impl Why {
    /// The reason an entry writes out, `None` for one the scan checks by itself.
    fn text(self) -> Option<&'static str> {
        match self {
            Why::Same(t) | Why::Logic(t) | Why::English(t) | Why::Table(t) => Some(t),
            Why::NoWord | Why::Forwarded(_) | Why::Consts(_) | Why::Slots(..) => None,
        }
    }
}

const ARTICLE: &str =
    "an article: object names stay English, and in Korean none is put (objects::names)";
const SYLLABLE: &str = "an incantation syllable: the game's magic words stay English";
/// The direction names' slots (A4-0x61be + 4 d, d 0..8: magic::LABELS).
const DIRECTIONS: Why = Why::Slots(A4.wrapping_sub(0x61be), 8);

/// The sites whose address is not catalogued, each with its reason, and the arguments the scan
/// cannot resolve, each with what it takes: by file, callee and argument (its code, each run of
/// white space one space).
pub(super) const NOT_CATALOGUED: &[(&str, &str, &str, Why)] = &[
    // Resolved, not catalogued.
    (
        "magic/book.rs",
        "a4_pointed",
        "BOOK_BMP",
        Why::Logic("the spellbook bitmap's file name"),
    ),
    (
        "main_loop.rs",
        "pointed_string",
        "A4.wrapping_sub(0x61e2)",
        Why::Logic("the paper bitmap's file name"),
    ),
    (
        "objects/commands/look.rs",
        "string",
        "S_BOOK_DAT",
        Why::Logic("the books' file name, lower-cased and opened"),
    ),
    (
        "objects/defs.rs",
        "string",
        "STR_NOTHING_LOWER",
        Why::Same("the look text of no entry (objects::names::look_text)"),
    ),
    ("objects/defs.rs", "string", "STR_A", Why::English(ARTICLE)),
    ("objects/defs.rs", "string", "STR_AN", Why::English(ARTICLE)),
    (
        "objects/defs.rs",
        "string",
        "STR_THE",
        Why::English(ARTICLE),
    ),
    (
        "usecmd/misc.rs",
        "a4_pointed_list",
        "defs::WAND_SPELLS",
        Why::Logic("a wand spell's incantation letters, which index the syllables"),
    ),
    (
        "usecmd/misc.rs",
        "a4_pointed",
        "defs::SPELL_NAMES",
        Why::English(SYLLABLE),
    ),
    (
        "usecmd/misc.rs",
        "a4_pointed",
        "WISHES",
        Why::Logic("the words the well compares typed text with"),
    ),
    (
        "usecmd/misc.rs",
        "a4_pointed",
        "MANTRAS",
        Why::Logic("the mantras compared with typed text"),
    ),
    (
        "ui/tables.rs",
        "s",
        "0x104ffe",
        Why::Logic("the bytes that end a keyword"),
    ),
    ("ui/tables.rs", "s", "0x110ba4", Why::NoWord),
    ("ui/tables.rs", "s", "0x110bae", Why::NoWord),
    ("ui/tables.rs", "s", "0x110bd0", Why::NoWord),
    ("ui/tables.rs", "s", "0x110be0", Why::NoWord),
    ("ui/tables.rs", "s", "0x110be4", Why::NoWord),
    ("ui/tables.rs", "s", "0x110bf4", Why::NoWord),
    // Not resolved.
    (
        "actors/clock.rs",
        "Arg::Image",
        "name",
        Why::Consts(&["RING_NAME", "CLOAK_NAME"]),
    ),
    (
        "actors/clock.rs",
        "Arg::Pointer",
        "wind_offset(s.actors.wind as i8)",
        Why::Slots(A4.wrapping_sub(0x7ae4).wrapping_sub(4), 9),
    ),
    (
        "actors/rest/command.rs",
        "say",
        "msg",
        Why::Consts(&[
            "STR_COMBAT",
            "STR_NO_REPAIR",
            "STR_WILDERNESS",
            "STR_FOES",
            "STR_OTHERS",
            "STR_SOLO",
            "STR_ROOM",
        ]),
    ),
    (
        "actors/rest/mod.rs",
        "game_string",
        "addr",
        Why::Forwarded(&["say"]),
    ),
    (
        "actors/seams.rs",
        "game_string",
        "crate::image::A4.wrapping_add_signed(off)",
        Why::Forwarded(&["Arg::Text"]),
    ),
    (
        "actors/seams.rs",
        "game_string",
        "at",
        Why::Forwarded(&["Arg::Image"]),
    ),
    (
        "actors/seams.rs",
        "pointed_string",
        "crate::image::A4.wrapping_add_signed(off)",
        Why::Forwarded(&["Arg::Pointer"]),
    ),
    (
        "actors/seams.rs",
        "game_string",
        "crate::image::A4.wrapping_add_signed(text)",
        Why::Forwarded(&["print"]),
    ),
    (
        "actors/seams.rs",
        "Arg::Image",
        "at",
        Why::Forwarded(&["print_image"]),
    ),
    (
        "combat/attack.rs",
        "say",
        "at",
        Why::Consts(&["SHIP_CANNONS", "BARE_HANDS"]),
    ),
    (
        "combat/present.rs",
        "game_string",
        "at",
        Why::Forwarded(&["say"]),
    ),
    (
        "inventory/ready.rs",
        "game_string",
        "addr",
        Why::Forwarded(&["say"]),
    ),
    (
        "inventory/ready.rs",
        "say",
        "msg",
        Why::Consts(&["STR_CANT", "STR_HEAVY", "STR_NO_PLACE"]),
    ),
    (
        "magic/book.rs",
        "pointed_string",
        "A4.wrapping_sub(off)",
        Why::Forwarded(&["a4_pointed"]),
    ),
    (
        "magic/book.rs",
        "game_string",
        "addr",
        Why::Forwarded(&["print_at"]),
    ),
    (
        "magic/cast.rs",
        "game_string",
        "addr",
        Why::Forwarded(&["print_at"]),
    ),
    (
        "magic/cast.rs",
        "print_at",
        "fmt",
        Why::Consts(&["FMT_S", "FMT_SPACE_S"]),
    ),
    (
        "magic/cast.rs",
        "print_at",
        "refusal",
        Why::Consts(&["MSG_LEVEL_LOW", "MSG_NO_POINTS", "MSG_NO_REAGENTS"]),
    ),
    (
        "magic/effects.rs",
        "game_string",
        "addr",
        Why::Forwarded(&["print_at"]),
    ),
    (
        "magic/effects.rs",
        "print_at",
        "fmt",
        Why::Forwarded(&["say"]),
    ),
    ("magic/items.rs", "pointed_string", "at", DIRECTIONS),
    (
        "magic/items.rs",
        "print_at",
        "fmt",
        Why::Consts(&["FMT_S", "FMT_SPACE_S"]),
    ),
    ("magic/spells.rs", "pointed_string", "at", DIRECTIONS),
    (
        "magic/tables.rs",
        "cstr",
        "p",
        Why::Table(
            "the spells' tables: names, circles, messages and potions are labels \
             (magic::NAME_LABELS, CIRCLE_LABELS, LABELS), words and syllables stay English, \
             alignments are never shown",
        ),
    ),
    (
        "main_loop.rs",
        "cstr",
        "addr",
        Why::Forwarded(&["game_string"]),
    ),
    (
        "main_loop.rs",
        "game_string",
        "A4.wrapping_sub(off)",
        Why::Forwarded(&["a4_string"]),
    ),
    (
        "main_loop.rs",
        "cstr",
        "p",
        Why::Forwarded(&["pointed_string"]),
    ),
    ("main_loop.rs", "a4_string", "off", Why::Forwarded(&["say"])),
    (
        "main_loop/commands.rs",
        "game_string",
        "addr",
        Why::Forwarded(&["print_at"]),
    ),
    (
        "main_loop/commands.rs",
        "pointed_string",
        "name_pointer(code)",
        Why::Table("a command's name: main_loop::commands::LABELS holds the 7 shown"),
    ),
    (
        "map/seams.rs",
        "pointed_string",
        "A4.wrapping_sub(0x6a60) + 4 * u32::from(q)",
        Why::Slots(A4.wrapping_sub(0x6a60) + 4, 20),
    ),
    (
        "objects/blocks.rs",
        "print",
        "p",
        Why::Consts(&["STR_REGION_FULL"]),
    ),
    (
        "objects/commands/data.rs",
        "cstr",
        "self.long(addr)",
        Why::Forwarded(&["pointed"]),
    ),
    ("objects/commands/eggs.rs", "pointed", "at", DIRECTIONS),
    (
        "objects/commands/get.rs",
        "pointed",
        "super::GameData::a4(off)",
        Why::Forwarded(&["message"]),
    ),
    (
        "objects/commands/light.rs",
        "queue_at",
        "n",
        Why::Consts(&[
            "S_CANDLE",
            "S_CANDELABRA",
            "S_FIREPLACE",
            "S_BRAZIER",
            "S_CAMPFIRE",
        ]),
    ),
    (
        "objects/commands/look.rs",
        "print_at",
        "lead",
        Why::Consts(&["S_IT", "S_COMMA", "S_AND"]),
    ),
    (
        "objects/commands/look.rs",
        "pointed",
        "at(SPELL_NAMES, u32::from(spell))",
        Why::Slots(A4.wrapping_sub(0x6f4c), 0x80),
    ),
    (
        "objects/commands/look.rs",
        "pointed",
        "at(SYLLABLES, u32::from(w))",
        Why::English(SYLLABLE),
    ),
    (
        "objects/commands/look.rs",
        "pointed",
        "at(REAGENT_NAMES, b)",
        Why::Slots(A4.wrapping_sub(0x6d4c), 8),
    ),
    (
        "objects/commands/look.rs",
        "print_at",
        "fmt",
        Why::Consts(&["S_WORD", "S_SPACE_WORD"]),
    ),
    ("objects/commands/push.rs", "pointed", "at", DIRECTIONS),
    (
        "objects/commands/text.rs",
        "cstr",
        "addr",
        Why::Forwarded(&["string"]),
    ),
    (
        "objects/commands/text.rs",
        "cstr",
        "super::GameData::a4(off)",
        Why::Forwarded(&["a4_string"]),
    ),
    (
        "objects/commands/text.rs",
        "string",
        "addr",
        Why::Forwarded(&["queue_at", "print_at"]),
    ),
    (
        "objects/commands/world.rs",
        "queue_at",
        "addr",
        Why::Forwarded(&["line"]),
    ),
    (
        "objects/commands/world.rs",
        "line",
        "at",
        Why::English(ARTICLE),
    ),
    (
        "objects/seams.rs",
        "game_string",
        "fmt",
        Why::Forwarded(&["print"]),
    ),
    (
        "sound/tables.rs",
        "cstr",
        "img.pointer(at)?",
        Why::Logic("a song's file name"),
    ),
    (
        "ui/seams.rs",
        "game_string",
        "addr",
        Why::Forwarded(&["game_string"]),
    ),
    ("ui/tables.rs", "cstr", "addr", Why::Forwarded(&["s"])),
    (
        "ui/tables.rs",
        "cstr",
        "ptr",
        Why::Slots(A4.wrapping_sub(0x7230) + 8, 7),
    ),
    (
        "usecmd/defs.rs",
        "pointed_string",
        "a4(off).wrapping_add(4 * i as u32)",
        Why::Forwarded(&["a4_pointed"]),
    ),
    (
        "usecmd/food.rs",
        "print",
        "msg",
        Why::Consts(&["STR_WATER_HEAVY", "STR_MILK_HEAVY"]),
    ),
    (
        "usecmd/mech.rs",
        "print",
        "msg",
        Why::Consts(&[
            "STR_STRANGE",
            "STR_NOISE",
            "STR_CANT_CLOSE",
            "STR_FOUNTAIN",
            "STR_NUGGET",
        ]),
    ),
    (
        "usecmd/misc.rs",
        "print",
        "fmt",
        Why::Consts(&["FMT_STR", "FMT_SP_STR"]),
    ),
    (
        "usecmd/mod.rs",
        "game_string",
        "fmt",
        Why::Forwarded(&["print"]),
    ),
    (
        "usecmd/mod.rs",
        "print",
        "A4.wrapping_sub(off)",
        Why::Forwarded(&["print_a4"]),
    ),
    (
        "usecmd/mod.rs",
        "pointed_string",
        "A4.wrapping_sub(ptr)",
        Why::Forwarded(&["say"]),
    ),
    ("usecmd/mod.rs", "print", "fmt", Why::Forwarded(&["say"])),
    (
        "usecmd/travel.rs",
        "print",
        "m",
        Why::Consts(&[
            "STR_NOT_BOARDABLE",
            "STR_LEADER_9",
            "STR_LEADER_A4",
            "STR_ON_HORSE",
        ]),
    ),
    (
        "usecmd/travel.rs",
        "print",
        "msg",
        Why::Consts(&[
            "STR_NO_BAG",
            "STR_NO_BASKET",
            "STR_NO_CAULDRON",
            "STR_NO_ROPE",
        ]),
    ),
];

/// The consts no list reads and no entry names that the sweep (a string const's name), the const
/// pass (a value naming a byte of an executable string) or the evaluation (an integer const the
/// scan cannot evaluate) finds, each with its reason: by file and name.
pub(super) const CONST_REASONS: &[(&str, &str, Why)] = &[
    (
        "actors/clock.rs",
        "LABELS",
        Why::Logic("the catalogue's list of the wind names' slots (const fn calls), a list itself"),
    ),
    (
        "actors/tables.rs",
        "AMBUSH_KINDS",
        Why::Logic("the code address of the ambush's kind moves, read as words"),
    ),
    (
        "actors/tables.rs",
        "CREATURE_TYPES",
        Why::Logic("the creature types' word table, read as numbers, no text"),
    ),
    (
        "combat/peer.rs",
        "AT",
        Why::Logic("the overview's top-left in pixels, no address"),
    ),
    (
        "combat/peer.rs",
        "SHOWN_PX",
        Why::Logic("the shown map's rectangle in pixels, no address"),
    ),
    (
        "combat/present.rs",
        "NO_DIRTY",
        Why::Logic("the flight's no-change rectangle in pixels, no address"),
    ),
    (
        "combat/present.rs",
        "SHOWN_PX",
        Why::Logic("the shown map's rectangle in pixels, no address"),
    ),
    (
        "combat/tables.rs",
        "BLAST_DX",
        Why::Logic("a blast's sprite offsets, a byte table read as numbers, no text"),
    ),
    (
        "combat/tables.rs",
        "CREATURE_STATS",
        Why::Logic("the six creature stat byte tables, read as numbers, no text"),
    ),
    (
        "combat/tables.rs",
        "CREATURE_STATUS",
        Why::Logic("a new creature's status byte table, read as numbers, no text"),
    ),
    (
        "combat/tables.rs",
        "LYING",
        Why::Logic("a table of 64 words, read as numbers"),
    ),
    (
        "combat/tables.rs",
        "MISSILE_CX",
        Why::Logic("a missile's flight table, read as numbers, no text"),
    ),
    (
        "combat/tables.rs",
        "MISSILE_CY",
        Why::Logic("a missile's flight table, read as numbers, no text"),
    ),
    (
        "combat/tables.rs",
        "MISSILE_STEP",
        Why::Logic("a missile's flight table, read as numbers, no text"),
    ),
    (
        "combat/tables.rs",
        "RANGE_VALUES",
        Why::Logic("the weapons' range byte table, read as numbers, no text"),
    ),
    (
        "combat/tables.rs",
        "SPREAD",
        Why::Logic("the triple crossbow's bolt byte table, read as numbers, no text"),
    ),
    (
        "combat/tables.rs",
        "WEAPON_DAMAGE",
        Why::Logic("the weapons' damage byte table, read as numbers, no text"),
    ),
    (
        "converse/disasm/bundle.rs",
        "ARCHIVES",
        Why::Logic("the conversation archives' letters (bytes), no address"),
    ),
    (
        "converse/eval.rs",
        "STR_TAG",
        Why::Logic("a tag on the conversation's stack, no address"),
    ),
    (
        "converse/script.rs",
        "PORTRAIT_LETTERS",
        Why::Logic("the portrait archives' letters (bytes), no address"),
    ),
    (
        "converse/text.rs",
        "HUNKS",
        Why::Logic("the message hunks' numbers and image bases, read to locate them, no text"),
    ),
    (
        "geom.rs",
        "LEAD",
        Why::Logic("a window offset in cells from another crate's chunk size, no address"),
    ),
    (
        "geom.rs",
        "SHOWN",
        Why::Logic("the shown map's rectangle in frame pixels, no address"),
    ),
    (
        "geom.rs",
        "SLOTS",
        Why::Logic("the side panel slots' frame origins in pixels (none without wide), no address"),
    ),
    (
        "gfx/blit.rs",
        "ROW_BYTES",
        Why::Logic("a frame row's bytes from another crate's width, no address"),
    ),
    (
        "gfx/sprite.rs",
        "PIXELS",
        Why::Logic("a tile's pixels from another crate's tile width, no address"),
    ),
    (
        "image.rs",
        "IMAGE_BASE",
        Why::Logic("the image's base address, where its first hunk's code starts"),
    ),
    (
        "inventory/tables.rs",
        "COLUMNS",
        Why::Logic("the digits' x table and its length, read as numbers, no text"),
    ),
    (
        "inventory/tables.rs",
        "GLYPHS",
        Why::Logic("the digits' glyph word table and its length, read as numbers, no text"),
    ),
    (
        "inventory/tables.rs",
        "HAND_TILES",
        Why::Logic("the weapon hand's tile word table and its length, read as numbers, no text"),
    ),
    (
        "inventory/tables.rs",
        "HUNK33_BASE",
        Why::Logic("the data hunk's base, checked against the file's layout"),
    ),
    (
        "inventory/tables.rs",
        "POINTER_ORIGIN",
        Why::Logic("the pointer origin's two bytes, read as numbers, no text"),
    ),
    (
        "magic/seams.rs",
        "NO_RECT",
        Why::Logic("combat's no-change rectangle in pixels, no address"),
    ),
    (
        "magic/tables.rs",
        "CIRCLE_LABELS",
        Why::Logic("the catalogue's list of the circles' name slots (a const fn), a list itself"),
    ),
    (
        "magic/tables.rs",
        "MAGIC_TILES",
        Why::Logic("the magical tiles' word table, read as numbers, no text"),
    ),
    (
        "magic/tables.rs",
        "MASKS",
        Why::Logic("the spells' reagent bit masks, read as numbers, no text"),
    ),
    (
        "magic/tables.rs",
        "NAME_LABELS",
        Why::Logic("the catalogue's list of the spells' name slots (a const fn), a list itself"),
    ),
    (
        "main_loop/commands.rs",
        "COMMAND_NAMES",
        Why::Logic("the command names' pointer table, whose shown slots are labels"),
    ),
    (
        "main_loop/commands.rs",
        "LABELS",
        Why::Logic(
            "the catalogue's list of the command names' slots (const fn calls), a list itself",
        ),
    ),
    (
        "main_loop/commands.rs",
        "NOT_IMPLEMENTED",
        Why::English(
            "the port's own English line (its bytes, no address), reached only through the debug teleport, which spec 16 leaves out of scope",
        ),
    ),
    (
        "map/seams.rs",
        "PLACE_NAMES",
        Why::Logic("the catalogue's list of the place names' slots (a const fn), a list itself"),
    ),
    (
        "objects/commands/data.rs",
        "HUNK33_BASE",
        Why::Logic("the data hunk's base, checked against the file's layout"),
    ),
    (
        "objects/commands/eggs.rs",
        "HP",
        Why::Logic("the creatures' hit point byte table, read as numbers, no text"),
    ),
    (
        "objects/commands/eggs.rs",
        "STATUS",
        Why::Logic("a new creature's status byte table, read as numbers, no text"),
    ),
    (
        "objects/commands/look.rs",
        "SIGN_TYPES",
        Why::Logic("the sign types' word table, read as numbers, no text"),
    ),
    (
        "objects/commands/look.rs",
        "SPELL_REAGENTS",
        Why::Logic("the spells' reagent bit masks, read as numbers, no text"),
    ),
    (
        "objects/commands/mod.rs",
        "LABELS",
        Why::Logic(
            "the catalogue's list of the reagents' name slots (const fn calls), a list itself",
        ),
    ),
    ("objects/commands/world.rs", "S_A", Why::English(ARTICLE)),
    ("objects/commands/world.rs", "S_AN", Why::English(ARTICLE)),
    ("objects/commands/world.rs", "S_THE", Why::English(ARTICLE)),
    (
        "objects/defs.rs",
        "HUNK33_BASE",
        Why::Logic("the data hunk's base, checked against the file's layout"),
    ),
    (
        "objects/defs.rs",
        "MONSTER_TYPES",
        Why::Logic("the monster types' word table, read as numbers, no text"),
    ),
    (
        "sound/tables.rs",
        "OCTAVES",
        Why::Logic("the samples' octave shift table, read as numbers, no text"),
    ),
    (
        "sound/tables.rs",
        "ST_TEMPOS",
        Why::Logic("a table of 10 tempo words, read as numbers"),
    ),
    (
        "ui/pointer.rs",
        "HIDDEN_Y",
        Why::Logic("a pixel row past the frame from another crate's height, no address"),
    ),
    (
        "ui/printf.rs",
        "PREFIXES",
        Why::Logic("the escape layer's prefix bytes, no address"),
    ),
    (
        "ui/tables.rs",
        "LABELS",
        Why::Logic(
            "the catalogue's list of the combat-mode name slots (const fn calls), a list itself",
        ),
    ),
    (
        "usecmd/defs.rs",
        "CHAR_CLASS",
        Why::Logic("the characters' class byte table, read as numbers, no text"),
    ),
];

/// The image-address literals outside consts and sites ([`Walk::visit_lit_int`]) that name a byte
/// inside a complete string no list holds, each with why: by file and address.
pub(super) const LITERAL_REASONS: &[(&str, u32, Why)] = &[
    (
        "converse/text.rs",
        0x0011_2982,
        Why::Logic("the conversation's word delimiters, which split the typed input"),
    ),
    (
        "converse/text.rs",
        0x0011_2a06,
        Why::Logic("the word an empty input stands for, compared as typed text"),
    ),
    (
        "converse/text.rs",
        0x0011_2afa,
        Why::Logic("the word typed input is compared with"),
    ),
    (
        "converse/text.rs",
        0x0015_5339,
        Why::Logic("the portraits' file name"),
    ),
    (
        "converse/text.rs",
        0x0015_5344,
        Why::Logic("the conversations' file name"),
    ),
    (
        "gfx/tables.rs",
        0x0015_36f0,
        Why::Logic("the data hunk's base, checked against the file's layout"),
    ),
];

/// Every address the catalogue reads: each printed string's, each pointer slot of one printed
/// through it and of a label, each label's own at its address, and each conversation message's
/// it shows ([`crate::converse::text::SHOWN`], read at its [`crate::converse::text::MESSAGES`]
/// address).
pub(super) fn listed() -> BTreeSet<u32> {
    use crate::converse::text::{MESSAGES, SHOWN};
    let converse = SHOWN
        .iter()
        .filter_map(|(m, _)| MESSAGES.iter().find(|(n, _)| n == m).map(|&(_, addr)| addr));
    super::strings::printed()
        .chain(super::strings::pointed())
        .chain(super::strings::labels())
        .chain(super::strings::shown_at())
        .chain(converse)
        .collect()
}

/// One source file: its path under `src/`, its text, and its parse (a failure's line).
pub(super) struct Source {
    pub path: String,
    text: String,
    file: Result<syn::File, usize>,
}

impl Source {
    /// A source of `text` at `path` (tests: the scan's own snippets).
    pub(super) fn new(path: &str, text: &str) -> Source {
        Source {
            path: path.into(),
            text: text.into(),
            file: syn::parse_file(text).map_err(|e| e.span().start().line),
        }
    }
}

/// Whether a `cfg` predicate holds in a build of the crate's library, with the `wide` feature
/// or without: `test` and `doc` never; `feature = "wide"` as `wide`, another feature never;
/// `not`, `all` and `any` of those; `None` (both builds may hold it: kept) for another
/// (`debug_assertions`, a target).
fn cfg_holds(m: &Meta, wide: bool) -> Option<bool> {
    let parts = |l: &syn::MetaList| {
        l.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
            .map(|p| {
                p.into_iter()
                    .map(|m| cfg_holds(&m, wide))
                    .collect::<Vec<_>>()
            })
            .ok()
    };
    match m {
        Meta::Path(p) if p.is_ident("test") || p.is_ident("doc") => Some(false),
        Meta::NameValue(nv) if nv.path.is_ident("feature") => match &nv.value {
            Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) => Some(s.value() == "wide" && wide),
            _ => None,
        },
        Meta::List(l) if l.path.is_ident("not") => {
            let p = parts(l)?;
            let [x] = p.as_slice() else {
                return None;
            };
            x.map(|b| !b)
        }
        Meta::List(l) if l.path.is_ident("all") => {
            let p = parts(l)?;
            if p.contains(&Some(false)) {
                Some(false)
            } else {
                p.iter().all(|x| *x == Some(true)).then_some(true)
            }
        }
        Meta::List(l) if l.path.is_ident("any") => {
            let p = parts(l)?;
            if p.contains(&Some(true)) {
                Some(true)
            } else {
                p.iter().all(|x| *x == Some(false)).then_some(false)
            }
        }
        _ => None,
    }
}

/// Whether `attrs` (outer or inner) hold a `cfg` that does not hold in the build (`wide` or not):
/// the item is not compiled there.
fn cfg_off(attrs: &[Attribute], wide: bool) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("cfg")
            && a.parse_args::<Meta>()
                .is_ok_and(|m| cfg_holds(&m, wide) == Some(false))
    })
}

/// Whether `attrs` keep an item out of both builds.
fn cfg_never(attrs: &[Attribute]) -> bool {
    cfg_off(attrs, false) && cfg_off(attrs, true)
}

/// The module path of a source (`objects/commands/world`, `objects/commands`; `lib.rs`: empty).
fn module_of(path: &str) -> &str {
    if path == "lib.rs" {
        return "";
    }
    path.strip_suffix("/mod.rs")
        .or_else(|| path.strip_suffix(".rs"))
        .unwrap_or(path)
}

/// The modules `items` declare from files (`mod name;`) that build `on` keeps, each with the
/// directory its file is in, inline modules' through their directory.
fn declared(items: &[Item], dir: &str, on: &dyn Fn(&[Attribute]) -> bool, out: &mut Vec<String>) {
    for item in items {
        let Item::Mod(m) = item else { continue };
        if !on(&m.attrs) {
            continue;
        }
        let name = m.ident.to_string();
        match &m.content {
            None => out.push(format!("{dir}{name}")),
            Some((_, inner)) => declared(inner, &format!("{dir}{name}/"), on, out),
        }
    }
}

/// The library's source files, from `lib.rs` through each `mod name;` either build compiles,
/// less [`SKIPPED`].
pub(super) fn sources() -> Vec<Source> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    let mut todo = vec!["lib.rs".to_string()];
    while let Some(path) = todo.pop() {
        let text = std::fs::read_to_string(root.join(&path)).expect("a source file reads");
        let src = Source::new(&path, &text);
        let dir = match module_of(&path) {
            "" => String::new(),
            m => format!("{m}/"),
        };
        let mut mods = Vec::new();
        if let Ok(f) = &src.file
            && !cfg_never(&f.attrs)
        {
            declared(&f.items, &dir, &|a| !cfg_never(a), &mut mods);
        }
        for m in mods {
            if path == "lib.rs" && SKIPPED.contains(&m.as_str()) {
                continue;
            }
            let file = format!("{m}.rs");
            todo.push(if root.join(&file).exists() {
                file
            } else {
                format!("{m}/mod.rs")
            });
        }
        out.push(src);
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// A const of a scope: its scope, declared type and value.
#[derive(Clone)]
struct Const {
    scope: usize,
    ty: syn::Type,
    value: Expr,
    /// A `static mut`: no value the scan reads.
    mutable: bool,
}

/// A scope of names: a module (no parent), or a fn's, a block's, a closure's, an arm's, a `for`
/// loop's, or an `if`'s or a `while`'s condition with what it guards (its parent the scope it is
/// in), with the consts declared in it (a `static` is one, a `static mut` with no value), the
/// names and globs its `use`s bring in, the names its patterns bind (a fn's parameters, `let`s,
/// arms), which hide a const of that name, and its other items. Associated consts are not
/// declared: a site naming one is not resolved (a finding).
#[derive(Default)]
struct Scope {
    parent: Option<usize>,
    /// An item's own scope (a fn's): the pattern bindings of the scopes it is in are not its.
    boundary: bool,
    module: String,
    src: usize,
    consts: BTreeMap<String, Vec<Const>>,
    uses: BTreeMap<String, Vec<Vec<String>>>,
    globs: Vec<Vec<String>>,
    locals: BTreeSet<String>,
    /// The other items declared in it (types, fns, traits): names a `use` or a glob may bring.
    items: BTreeSet<String>,
}

/// A call of the sources: its scope, source and line, its callee (a path's last segment, `Arg::`
/// with a variant's, or a method's name; `form` when the callee is no plain path), whether a
/// method, and its arguments with their code.
struct RawCall {
    scope: usize,
    src: usize,
    line: usize,
    name: String,
    form: bool,
    method: bool,
    args: Vec<(Expr, String)>,
}

/// A `use` tree's leaves under the path `base`: each path with the name it binds (`None`: a
/// glob) and its line.
fn use_tree(base: &[String], t: &UseTree, out: &mut Vec<(Vec<String>, Option<String>, usize)>) {
    let with = |s: String| base.iter().cloned().chain([s]).collect::<Vec<String>>();
    match t {
        UseTree::Path(p) => use_tree(&with(p.ident.to_string()), &p.tree, out),
        UseTree::Name(n) if n.ident == "self" => {
            if let Some(last) = base.last() {
                out.push((
                    base.to_vec(),
                    Some(last.clone()),
                    n.ident.span().start().line,
                ));
            }
        }
        UseTree::Name(n) => {
            let name = n.ident.to_string();
            out.push((with(name.clone()), Some(name), n.ident.span().start().line));
        }
        UseTree::Rename(r) => {
            let path = if r.ident == "self" {
                base.to_vec()
            } else {
                with(r.ident.to_string())
            };
            out.push((
                path,
                Some(r.rename.to_string()),
                r.rename.span().start().line,
            ));
        }
        UseTree::Glob(g) => out.push((base.to_vec(), None, g.star_token.span.start().line)),
        UseTree::Group(g) => {
            for t in &g.items {
                use_tree(base, t, out);
            }
        }
    }
}

/// An item's name and attributes (an item that declares a name).
fn item_ident(item: &Item) -> Option<(&syn::Ident, &[Attribute])> {
    Some(match item {
        Item::Const(x) => (&x.ident, &x.attrs),
        Item::Static(x) => (&x.ident, &x.attrs),
        Item::Fn(x) => (&x.sig.ident, &x.attrs),
        Item::Struct(x) => (&x.ident, &x.attrs),
        Item::Enum(x) => (&x.ident, &x.attrs),
        Item::Type(x) => (&x.ident, &x.attrs),
        Item::Trait(x) => (&x.ident, &x.attrs),
        Item::TraitAlias(x) => (&x.ident, &x.attrs),
        Item::Union(x) => (&x.ident, &x.attrs),
        Item::Mod(x) => (&x.ident, &x.attrs),
        _ => return None,
    })
}

/// The reader name a path gives: `Arg::` with a variant's after `Arg`, else its last segment.
fn reader_name(segs: &[String]) -> String {
    let last = segs.last().cloned().unwrap_or_default();
    match segs.len() {
        2.. if segs[segs.len() - 2] == "Arg" => format!("Arg::{last}"),
        _ => last,
    }
}

/// Whether `name` is a [`READERS`] name in source `file`.
fn is_reader(name: &str, file: &str) -> bool {
    READERS
        .iter()
        .any(|r| r.name == name && (r.scope.is_empty() || r.scope == file))
}

/// The identifiers of `tokens`, through every group.
fn idents(tokens: TokenStream, out: &mut Vec<String>) {
    for t in tokens {
        match t {
            TokenTree::Ident(i) => out.push(i.to_string()),
            TokenTree::Group(g) => idents(g.stream(), out),
            _ => {}
        }
    }
}

/// The [`READERS`] a token stream of source `file` names (by a reader's last segment).
fn readers_named(tokens: TokenStream, file: &str) -> Vec<&'static str> {
    let mut words = Vec::new();
    idents(tokens, &mut words);
    readers_of(&words, file)
}

/// The [`READERS`] the code `text` of source `file` names.
fn readers_in(text: &str, file: &str) -> Vec<&'static str> {
    let words: Vec<String> = text
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect();
    readers_of(&words, file)
}

/// The [`READERS`] of source `file` whose last segment is one of `words`.
fn readers_of(words: &[String], file: &str) -> Vec<&'static str> {
    READERS
        .iter()
        .filter(|r| r.scope.is_empty() || r.scope == file)
        .filter(|r| {
            let bare = r.name.rsplit("::").next().unwrap_or(r.name);
            words.iter().any(|w| w == bare)
        })
        .map(|r| r.name)
        .collect()
}

/// The source text of `span` in `text`, each run of white space one space.
fn text_of(text: &str, span: Span) -> String {
    let r = span.byte_range();
    text.get(r)
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The walk of one source: its scopes, calls, and what it finds in macros.
struct Walk<'s> {
    wide: bool,
    src: usize,
    file: &'s str,
    text: &'s str,
    scopes: &'s mut Vec<Scope>,
    modules: &'s mut BTreeMap<String, usize>,
    calls: &'s mut Vec<RawCall>,
    found: &'s mut Vec<String>,
    ambiguous: &'s mut BTreeSet<String>,
    /// Each glob `use` (its scope, line and path), checked once every module is known.
    globs: &'s mut Vec<(usize, usize, Vec<String>)>,
    /// Each `use … as name` (its scope, line and name), checked once every module is known.
    renames: &'s mut Vec<(usize, usize, String)>,
    /// Each integer literal of an image address outside an integer const's value and a site's
    /// address argument: its source, line and value.
    literals: &'s mut Vec<(usize, usize, u32)>,
    /// Where the `::` of each qualified self type's path is (`<T>::f`, `<T as U>::f`): no leading
    /// colon.
    qself: BTreeSet<usize>,
    /// How deep the walk is in a site's address argument, and in an integer const's value.
    in_site: usize,
    in_const: usize,
    stack: Vec<usize>,
}

impl Walk<'_> {
    /// Whether `attrs` keep an item out of this build.
    fn off(&self, attrs: &[Attribute]) -> bool {
        cfg_off(attrs, self.wide)
    }

    fn here(&self) -> usize {
        *self.stack.last().expect("a scope")
    }

    fn open(&mut self, module: Option<String>) -> usize {
        let parent = if module.is_some() {
            None
        } else {
            Some(self.here())
        };
        let module = module.unwrap_or_else(|| self.scopes[self.here()].module.clone());
        let id = self.scopes.len();
        self.scopes.push(Scope {
            parent,
            module: module.clone(),
            src: self.src,
            ..Scope::default()
        });
        if parent.is_none() && self.modules.insert(module.clone(), id).is_some() {
            // Two modules of one path (`cfg` variants): neither is the one a path names.
            self.ambiguous.insert(module);
        }
        self.stack.push(id);
        id
    }

    /// The consts and `use`s `items` declare, into the current scope (visible in all of it).
    fn declare(&mut self, items: &[&Item]) {
        let id = self.here();
        for item in items {
            if let Some((ident, attrs)) = item_ident(item)
                && !self.off(attrs)
                && (ident == "From" || ident == "Into")
            {
                self.rejected(ident.span().start().line, &ident.to_string());
            }
            match item {
                Item::Const(c) if !self.off(&c.attrs) => {
                    self.scopes[id]
                        .consts
                        .entry(c.ident.to_string())
                        .or_default()
                        .push(Const {
                            scope: id,
                            ty: (*c.ty).clone(),
                            value: (*c.expr).clone(),
                            mutable: false,
                        });
                }
                Item::Static(c) if !self.off(&c.attrs) => {
                    self.scopes[id]
                        .consts
                        .entry(c.ident.to_string())
                        .or_default()
                        .push(Const {
                            scope: id,
                            ty: (*c.ty).clone(),
                            value: (*c.expr).clone(),
                            mutable: matches!(c.mutability, syn::StaticMutability::Mut(_)),
                        });
                }
                Item::Use(u) if !self.off(&u.attrs) => {
                    if u.leading_colon.is_some() {
                        self.rejected(u.use_token.span.start().line, "::path");
                    }
                    let mut leaves = Vec::new();
                    use_tree(&[], &u.tree, &mut leaves);
                    for (path, bound, line) in leaves {
                        self.imported(id, path, bound, line);
                    }
                }
                Item::Fn(f) if !self.off(&f.attrs) => {
                    self.scopes[id].items.insert(f.sig.ident.to_string());
                }
                Item::Struct(x) if !self.off(&x.attrs) => {
                    self.scopes[id].items.insert(x.ident.to_string());
                }
                Item::Enum(x) if !self.off(&x.attrs) => {
                    self.scopes[id].items.insert(x.ident.to_string());
                }
                Item::Type(x) if !self.off(&x.attrs) => {
                    self.scopes[id].items.insert(x.ident.to_string());
                }
                Item::Trait(x) if !self.off(&x.attrs) => {
                    self.scopes[id].items.insert(x.ident.to_string());
                }
                Item::Union(x) if !self.off(&x.attrs) => {
                    self.scopes[id].items.insert(x.ident.to_string());
                }
                _ => {}
            }
        }
    }

    /// A `use` leaf into scope `id`: the name it binds (or its glob), and a finding when it brings
    /// a reader in under another name than the scan finds calls by: a reader renamed, `Arg`
    /// renamed, a variant of `Arg` by its bare name, or `Arg`'s variants by a glob.
    fn imported(&mut self, id: usize, path: Vec<String>, bound: Option<String>, line: usize) {
        let file = self.file;
        match bound {
            None => {
                if path.last().is_some_and(|l| l == "Arg") {
                    self.found.push(format!(
                        "{file}:{line} use: brings Arg's variants in by a glob"
                    ));
                }
                self.globs.push((id, line, path.clone()));
                self.scopes[id].globs.push(path);
            }
            Some(name) => {
                let r = reader_name(&path);
                if is_reader(&r, file) && r != name {
                    self.found.push(format!(
                        "{file}:{line} use: brings the reader {r} in as {name}"
                    ));
                } else if path.last().is_some_and(|l| l == "Arg") && name != "Arg" {
                    self.found.push(format!(
                        "{file}:{line} use: brings the reader Arg in as {name}"
                    ));
                }
                let renamed = path.last() != Some(&name);
                if renamed && (name == "From" || name == "Into") {
                    self.rejected(line, &name);
                }
                if renamed {
                    self.renames.push((id, line, name.clone()));
                }
                self.scopes[id].uses.entry(name).or_default().push(path);
            }
        }
    }

    /// Whether `name` is a pattern binding in the current scope or one it is in, up to the
    /// item's own scope it is in (a fn's).
    fn is_local(&self, name: &str) -> bool {
        let mut s = Some(self.here());
        while let Some(id) = s {
            if self.scopes[id].locals.contains(name) {
                return true;
            }
            if self.scopes[id].boundary {
                break;
            }
            s = self.scopes[id].parent;
        }
        false
    }

    /// A const's or a static's value walked; an integer one's literals are [`const_pass`]'s.
    fn walk_const(&mut self, ty: &syn::Type, e: &Expr) {
        let int = usize::from(integer(ty));
        self.in_const += int;
        self.visit_expr(e);
        self.in_const -= int;
    }

    /// A path's `::` when it follows a qualified self type: no leading colon.
    fn qualified(&mut self, q: Option<&syn::QSelf>, p: &syn::Path) {
        if let (Some(_), Some(c)) = (q, p.leading_colon) {
            self.qself.insert(c.spans[0].byte_range().start);
        }
    }

    /// A fn's own scope, opened.
    fn open_item(&mut self) {
        let id = self.open(None);
        self.scopes[id].boundary = true;
    }

    /// A form the scan rejects outright ([the contract](self), rule 2), at `line`.
    fn rejected(&mut self, line: usize, form: &str) {
        self.found.push(format!(
            "{}:{line} {form}: a form the scan rejects",
            self.file
        ));
    }

    /// A callee path's generic arguments: a block or an expression but a literal is a form the
    /// scan rejects.
    fn generic_args(&mut self, line: usize, args: &syn::AngleBracketedGenericArguments) {
        let other = args
            .args
            .iter()
            .any(|a| matches!(a, syn::GenericArgument::Const(e) if !matches!(e, Expr::Lit(_))));
        if other {
            self.rejected(line, "a generic argument");
        }
    }

    /// The arguments of a call of `name` (`method` or not): each one a reader of that form takes
    /// as its address (a site's, which the site check reads) walked as such.
    fn visit_args(&mut self, name: &str, method: bool, args: &[&Expr]) {
        let read: BTreeSet<usize> = READERS
            .iter()
            .filter(|r| r.name == name && (r.scope.is_empty() || r.scope == self.file))
            .filter(|r| r.arity == args.len() && r.method == method)
            .flat_map(|r| r.args.iter().copied())
            .collect();
        for (k, a) in args.iter().enumerate() {
            let site = read.contains(&k);
            self.in_site += usize::from(site);
            self.visit_expr(a);
            self.in_site -= usize::from(site);
        }
    }

    /// A macro invoked (not `macro_rules!`): its body read as expressions and walked; one that
    /// does not read so and names a reader is a finding.
    fn invoked(&mut self, mac: &syn::Macro) {
        let line = mac
            .path
            .segments
            .first()
            .map_or(0, |s| s.ident.span().start().line);
        let name = mac
            .path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_default();
        if mac.path.leading_colon.is_some() {
            self.rejected(line, "::path");
        }
        if name == "include" {
            self.rejected(line, "include!");
            return;
        }
        if name == "macro_rules" {
            if let Some(r) = readers_named(mac.tokens.clone(), self.file).first() {
                self.found.push(format!(
                    "{}:{line} macro_rules!: names the reader {r}",
                    self.file
                ));
            }
            return;
        }
        match mac.parse_body_with(Punctuated::<Expr, Token![,]>::parse_terminated) {
            Ok(exprs) => {
                for e in &exprs {
                    self.visit_expr(e);
                }
            }
            Err(_) => {
                if let Some(r) = readers_named(mac.tokens.clone(), self.file).first() {
                    self.found.push(format!(
                        "{}:{line} {name}!: names the reader {r}, its body no expressions",
                        self.file
                    ));
                }
            }
        }
    }

    fn call(&mut self, name: String, form: bool, method: bool, line: usize, args: Vec<&Expr>) {
        let args = args
            .into_iter()
            .map(|e| (e.clone(), text_of(self.text, e.span())))
            .collect();
        self.calls.push(RawCall {
            scope: self.here(),
            src: self.src,
            line,
            name,
            form,
            method,
            args,
        });
    }
}

impl<'ast> Visit<'ast> for Walk<'_> {
    fn visit_item(&mut self, i: &'ast Item) {
        match i {
            Item::Mod(m) => {
                if self.off(&m.attrs) {
                    return;
                }
                let line = m.ident.span().start().line;
                if m.attrs.iter().any(|a| a.path().is_ident("path")) {
                    self.rejected(line, "#[path]");
                }
                if m.attrs
                    .iter()
                    .any(|a| a.path().is_ident("no_implicit_prelude"))
                {
                    self.rejected(line, "no_implicit_prelude");
                }
                if let Some((_, items)) = &m.content {
                    let parent = self.scopes[self.here()].module.clone();
                    let name = m.ident.to_string();
                    let module = if parent.is_empty() {
                        name
                    } else {
                        format!("{parent}/{name}")
                    };
                    self.open(Some(module));
                    self.declare(&items.iter().collect::<Vec<_>>());
                    for item in items {
                        self.visit_item(item);
                    }
                    self.stack.pop();
                }
            }
            Item::Fn(f) => {
                if self.off(&f.attrs) {
                    return;
                }
                self.open_item();
                visit::visit_item_fn(self, f);
                self.stack.pop();
            }
            Item::Macro(m) => {
                if !self.off(&m.attrs) {
                    self.invoked(&m.mac);
                }
            }
            Item::Const(c) => {
                if !self.off(&c.attrs) {
                    self.walk_const(&c.ty, &c.expr);
                }
            }
            Item::Static(c) => {
                if !self.off(&c.attrs) {
                    self.walk_const(&c.ty, &c.expr);
                }
            }
            Item::Impl(x) if self.off(&x.attrs) => {}
            Item::Trait(x) if self.off(&x.attrs) => {}
            Item::Struct(x) if self.off(&x.attrs) => {}
            Item::Enum(x) if self.off(&x.attrs) => {}
            // A `use` is declared ([`Walk::declare`]); its paths are no expressions.
            Item::Use(_) => {}
            _ => visit::visit_item(self, i),
        }
    }

    fn visit_impl_item(&mut self, i: &'ast syn::ImplItem) {
        match i {
            syn::ImplItem::Fn(f) if !self.off(&f.attrs) => {
                self.open_item();
                visit::visit_impl_item_fn(self, f);
                self.stack.pop();
            }
            syn::ImplItem::Macro(m) if !self.off(&m.attrs) => self.invoked(&m.mac),
            syn::ImplItem::Const(c) if !self.off(&c.attrs) => self.visit_expr(&c.expr),
            _ => {}
        }
    }

    fn visit_trait_item(&mut self, i: &'ast syn::TraitItem) {
        match i {
            syn::TraitItem::Fn(f) if !self.off(&f.attrs) => {
                self.open_item();
                visit::visit_trait_item_fn(self, f);
                self.stack.pop();
            }
            syn::TraitItem::Macro(m) if !self.off(&m.attrs) => self.invoked(&m.mac),
            syn::TraitItem::Const(c) if !self.off(&c.attrs) => {
                if let Some((_, e)) = &c.default {
                    self.visit_expr(e);
                }
            }
            _ => {}
        }
    }

    fn visit_block(&mut self, b: &'ast syn::Block) {
        self.open(None);
        let items: Vec<&Item> = b
            .stmts
            .iter()
            .filter_map(|s| match s {
                Stmt::Item(i) => Some(i),
                _ => None,
            })
            .collect();
        self.declare(&items);
        for s in &b.stmts {
            self.visit_stmt(s);
        }
        self.stack.pop();
    }

    fn visit_stmt(&mut self, s: &'ast Stmt) {
        match s {
            Stmt::Local(l) if self.off(&l.attrs) => {}
            Stmt::Macro(m) if self.off(&m.attrs) => {}
            Stmt::Macro(m) => self.invoked(&m.mac),
            _ => visit::visit_stmt(self, s),
        }
    }

    /// A `let`: its initializer (and a `let … else`'s block) before its pattern binds.
    fn visit_local(&mut self, l: &'ast syn::Local) {
        if let Some(init) = &l.init {
            self.visit_expr(&init.expr);
            if let Some((_, e)) = &init.diverge {
                self.visit_expr(e);
            }
        }
        self.visit_pat(&l.pat);
    }

    /// A `let` of a condition: its value before its pattern binds (in the condition's scope).
    fn visit_expr_let(&mut self, l: &'ast syn::ExprLet) {
        self.visit_expr(&l.expr);
        self.visit_pat(&l.pat);
    }

    fn visit_expr_closure(&mut self, c: &'ast syn::ExprClosure) {
        self.open(None);
        visit::visit_expr_closure(self, c);
        self.stack.pop();
    }

    fn visit_arm(&mut self, a: &'ast syn::Arm) {
        self.open(None);
        visit::visit_arm(self, a);
        self.stack.pop();
    }

    /// A `for` loop: what it iterates outside its pattern's scope.
    fn visit_expr_for_loop(&mut self, f: &'ast syn::ExprForLoop) {
        self.visit_expr(&f.expr);
        self.open(None);
        self.visit_pat(&f.pat);
        self.visit_block(&f.body);
        self.stack.pop();
    }

    fn visit_expr_while(&mut self, w: &'ast syn::ExprWhile) {
        self.open(None);
        self.visit_expr(&w.cond);
        self.visit_block(&w.body);
        self.stack.pop();
    }

    /// An `if`: its condition's bindings over its first branch, not its `else`.
    fn visit_expr_if(&mut self, i: &'ast syn::ExprIf) {
        self.open(None);
        self.visit_expr(&i.cond);
        self.visit_block(&i.then_branch);
        self.stack.pop();
        if let Some((_, e)) = &i.else_branch {
            self.visit_expr(e);
        }
    }

    /// A path anywhere (a type's, a pattern's, an attribute's too): a leading `::` is a form
    /// the scan rejects.
    fn visit_path(&mut self, p: &'ast syn::Path) {
        let at = p.leading_colon.map(|c| c.spans[0].byte_range().start);
        if at.is_some_and(|a| !self.qself.contains(&a)) {
            let line = p
                .segments
                .first()
                .map_or(0, |s| s.ident.span().start().line);
            self.rejected(line, "::path");
        }
        visit::visit_path(self, p);
    }

    fn visit_type_path(&mut self, t: &'ast syn::TypePath) {
        self.qualified(t.qself.as_ref(), &t.path);
        visit::visit_type_path(self, t);
    }

    fn visit_expr_struct(&mut self, s: &'ast syn::ExprStruct) {
        self.qualified(s.qself.as_ref(), &s.path);
        visit::visit_expr_struct(self, s);
    }

    fn visit_pat_struct(&mut self, s: &'ast syn::PatStruct) {
        self.qualified(s.qself.as_ref(), &s.path);
        visit::visit_pat_struct(self, s);
    }

    fn visit_pat_tuple_struct(&mut self, s: &'ast syn::PatTupleStruct) {
        self.qualified(s.qself.as_ref(), &s.path);
        visit::visit_pat_tuple_struct(self, s);
    }

    /// An integer literal of an image address outside an integer const's value and a site's
    /// address argument (each checked by its own pass): recorded for [`const_pass`].
    fn visit_lit_int(&mut self, l: &'ast syn::LitInt) {
        if self.in_site == 0 && self.in_const == 0 {
            let v = l.base10_parse::<u128>().ok();
            if let Some(v) = v.and_then(|v| u32::try_from(v).ok())
                && (IMAGE_BASE..0x0020_0000).contains(&v)
            {
                let line = l.span().start().line;
                self.literals.push((self.src, line, v));
            }
        }
    }

    fn visit_pat_ident(&mut self, p: &'ast syn::PatIdent) {
        let id = self.here();
        self.scopes[id].locals.insert(p.ident.to_string());
        visit::visit_pat_ident(self, p);
    }

    fn visit_expr_macro(&mut self, m: &'ast syn::ExprMacro) {
        self.invoked(&m.mac);
    }

    fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
        let args: Vec<&Expr> = c.args.iter().collect();
        match &*c.func {
            Expr::Path(p) if p.qself.is_none() => {
                let segs: Vec<String> = p
                    .path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect();
                let last = segs.last().cloned().unwrap_or_default();
                let name = if segs.len() >= 2 && segs[segs.len() - 2] == "Arg" {
                    format!("Arg::{last}")
                } else {
                    last
                };
                let line = p
                    .path
                    .segments
                    .first()
                    .map_or(0, |s| s.ident.span().start().line);
                if p.path.leading_colon.is_some() {
                    self.rejected(line, "::path");
                }
                for s in &p.path.segments {
                    if let syn::PathArguments::AngleBracketed(a) = &s.arguments {
                        self.generic_args(line, a);
                        self.visit_angle_bracketed_generic_arguments(a);
                    }
                }
                self.call(name.clone(), false, false, line, args.clone());
                self.visit_args(&name, false, &args);
            }
            other => {
                let line = other.span().start().line;
                let named = readers_in(&text_of(self.text, other.span()), self.file);
                for r in &named {
                    self.call(r.to_string(), true, false, line, args.clone());
                }
                if named.is_empty() {
                    self.visit_expr(other);
                }
                for a in &c.args {
                    self.visit_expr(a);
                }
            }
        }
    }

    /// A path in any place but a call's callee (a value, an argument, a macro's argument): one
    /// naming a reader is a finding, but a pattern binding of its name.
    fn visit_expr_path(&mut self, p: &'ast syn::ExprPath) {
        self.qualified(p.qself.as_ref(), &p.path);
        let segs: Vec<String> = p
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect();
        let name = reader_name(&segs);
        let local = p.qself.is_none() && segs.len() == 1 && self.is_local(&segs[0]);
        if !local && is_reader(&name, self.file) {
            let line = p
                .path
                .segments
                .first()
                .map_or(0, |s| s.ident.span().start().line);
            self.found.push(format!(
                "{}:{line} {name}: a reader named where no call reads it",
                self.file
            ));
        }
        visit::visit_expr_path(self, p);
    }

    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        let line = m.method.span().start().line;
        let args: Vec<&Expr> = m.args.iter().collect();
        let name = m.method.to_string();
        if let Some(t) = &m.turbofish {
            self.generic_args(line, t);
            self.visit_angle_bracketed_generic_arguments(t);
        }
        self.call(name.clone(), false, true, line, args.clone());
        self.visit_expr(&m.receiver);
        self.visit_args(&name, true, &args);
    }
}

/// The scan's resolver: the sources, their scopes and calls, and what the walk found.
pub(super) struct Scan<'a> {
    sources: &'a [Source],
    scopes: Vec<Scope>,
    modules: BTreeMap<String, usize>,
    /// Module paths two module scopes have (`cfg` variants): a path naming one is not resolved.
    ambiguous: BTreeSet<String>,
    calls: Vec<RawCall>,
    walked: Vec<String>,
    /// The integer literals of image addresses outside integer consts and sites' addresses
    /// ([`Walk::visit_lit_int`]).
    literals: Vec<(usize, usize, u32)>,
}

/// An integer type of the sources, or a literal's type still open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ty {
    U8,
    U16,
    U32,
    U64,
    Usize,
    I8,
    I16,
    I32,
    I64,
    Isize,
    Lit,
}

impl Ty {
    fn parse(s: &str) -> Option<Ty> {
        Some(match s.trim() {
            "u8" => Ty::U8,
            "u16" => Ty::U16,
            "u32" => Ty::U32,
            "u64" => Ty::U64,
            "usize" => Ty::Usize,
            "i8" => Ty::I8,
            "i16" => Ty::I16,
            "i32" => Ty::I32,
            "i64" => Ty::I64,
            "isize" => Ty::Isize,
            _ => return None,
        })
    }

    /// Its bits and whether signed (a literal: none; `usize` as the host's 64).
    fn width(self) -> Option<(u32, bool)> {
        Some(match self {
            Ty::U8 => (8, false),
            Ty::U16 => (16, false),
            Ty::U32 => (32, false),
            Ty::U64 | Ty::Usize => (64, false),
            Ty::I8 => (8, true),
            Ty::I16 => (16, true),
            Ty::I32 => (32, true),
            Ty::I64 | Ty::Isize => (64, true),
            Ty::Lit => return None,
        })
    }

    /// `v` if it is in the type's range (a literal: an `i64`'s or a `u64`'s).
    fn fit(self, v: i128) -> Option<i128> {
        let (lo, hi) = match self.width() {
            Some((bits, true)) => (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1),
            Some((bits, false)) => (0, (1i128 << bits) - 1),
            None => (i128::from(i64::MIN), i128::from(u64::MAX)),
        };
        (lo..=hi).contains(&v).then_some(v)
    }

    /// `v` wrapped into the type (two's complement); a literal: `None`.
    fn wrap(self, v: i128) -> Option<i128> {
        let (bits, signed) = self.width()?;
        let m = v.rem_euclid(1i128 << bits);
        Some(if signed && m >= 1i128 << (bits - 1) {
            m - (1i128 << bits)
        } else {
            m
        })
    }
}

/// A value and its type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Val {
    v: i128,
    ty: Ty,
}

/// The type both sides of a binary operation take: a literal takes the other's; two others
/// must be the same.
fn unify(a: Ty, b: Ty) -> Option<Ty> {
    match (a, b) {
        (Ty::Lit, t) | (t, Ty::Lit) => Some(t),
        (x, y) if x == y => Some(x),
        _ => None,
    }
}

/// Every value of `a` with every value of `b` through `f`; `None` when one is.
fn each(a: &[Val], b: &[Val], f: impl Fn(Val, Val) -> Option<Val>) -> Option<Vec<Val>> {
    let mut out = Vec::new();
    for &x in a {
        for &y in b {
            let v = f(x, y)?;
            if !out.contains(&v) {
                out.push(v);
            }
        }
    }
    Some(out)
}

/// `x op y` as Rust computes it on their type: `+ - * / % | &` on one type, each operand fit to
/// it first (a division truncated; by 0, or a signed minimum by -1, `None`), out of its range
/// `None`; a shift (`<`, `>`) on the left's type by an amount under its bits, the bits shifted
/// out of the type dropped. Two literals (a type still open) take `+ - * /` only, out of an
/// `i64`'s and a `u64`'s range `None` (a literal expression that overflows its type does not
/// compile: `arithmetic_overflow`); their width-dependent `% | &` and a literal's shift are
/// `None`.
fn arith(x: Val, y: Val, op: char) -> Option<Val> {
    if matches!(op, '<' | '>') {
        let ty = x.ty;
        let (bits, _) = ty.width()?;
        let n = u32::try_from(y.v).ok().filter(|&n| n < bits)?;
        let v = match op {
            '<' => x.v.checked_shl(n)?,
            _ => x.v >> n,
        };
        return Some(Val { v: ty.wrap(v)?, ty });
    }
    let ty = unify(x.ty, y.ty)?;
    if ty == Ty::Lit && matches!(op, '%' | '|' | '&') {
        return None;
    }
    let (a, b) = (operand(x, ty)?, operand(y, ty)?);
    if let Some((bits, true)) = ty.width()
        && matches!(op, '/' | '%')
        && a == -(1i128 << (bits - 1))
        && b == -1
    {
        return None;
    }
    let v = match op {
        '+' => a.checked_add(b)?,
        '-' => a.checked_sub(b)?,
        '*' => a.checked_mul(b)?,
        '/' => a.checked_div(b)?,
        '%' => a.checked_rem(b)?,
        '|' => a | b,
        '&' => a & b,
        _ => return None,
    };
    Some(Val { v: ty.fit(v)?, ty })
}

/// What a path's value is taken as: the const's own value, every element of an array or slice
/// const (an index the scan does not read), or one element of a tuple const.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sel {
    Scalar,
    Elems,
    Field(usize),
}

/// The integer type a `syn` type names, `None` for another.
fn int_ty(t: &syn::Type) -> Option<Ty> {
    match t {
        syn::Type::Path(p) if p.qself.is_none() => Ty::parse(&p.path.get_ident()?.to_string()),
        syn::Type::Paren(p) => int_ty(&p.elem),
        syn::Type::Group(g) => int_ty(&g.elem),
        _ => None,
    }
}

/// The element type of an array or slice const's type (`[T; N]`, `&[T]`), `None` for another.
fn elem_ty(t: &syn::Type) -> Option<Ty> {
    match t {
        syn::Type::Array(a) => int_ty(&a.elem),
        syn::Type::Slice(s) => int_ty(&s.elem),
        syn::Type::Reference(r) => elem_ty(&r.elem),
        _ => None,
    }
}

/// An integer literal's value, negated first when `neg`, fit to its suffix's type.
fn lit_val(l: &syn::LitInt, neg: bool) -> Option<Val> {
    let v = i128::try_from(l.base10_parse::<u128>().ok()?).ok()?;
    let v = if neg { v.checked_neg()? } else { v };
    let ty = if l.suffix().is_empty() {
        Ty::Lit
    } else {
        Ty::parse(l.suffix())?
    };
    Some(Val { v: ty.fit(v)?, ty })
}

/// The one expression a block holds (an `if` arm's), `None` for another block.
fn block_expr(b: &syn::Block) -> Option<&Expr> {
    match b.stmts.as_slice() {
        [Stmt::Expr(e, None)] => Some(e),
        _ => None,
    }
}

/// `y` as an operand of type `ty`: a literal fit to it, another of that type as it is.
fn operand(y: Val, ty: Ty) -> Option<i128> {
    match y.ty {
        Ty::Lit => ty.fit(y.v),
        t if t == ty => Some(y.v),
        _ => None,
    }
}

/// `x.m(args)` for the methods the sources use on addresses and sizes, on `x`'s type (an operand
/// fit to it, `wrapping_add_signed`'s to its signed counterpart): the `wrapping_` ones, and an
/// unsigned's `next_power_of_two` and `div_ceil` (out of the type `None`); another method, or a
/// literal receiver (its type open), is `None`.
fn method(x: Val, m: &str, args: &[Vec<Val>]) -> Option<Vec<Val>> {
    let ty = x.ty;
    let (bits, signed) = ty.width()?;
    let counterpart = match (bits, signed) {
        (8, false) => Ty::I8,
        (16, false) => Ty::I16,
        (32, false) => Ty::I32,
        (64, false) if ty == Ty::Usize => Ty::Isize,
        (64, false) => Ty::I64,
        _ => return None,
    };
    let one = |f: &dyn Fn(Val) -> Option<i128>| -> Option<Vec<Val>> {
        let [a] = args else { return None };
        a.iter()
            .map(|&y| f(y).and_then(|v| ty.wrap(v)).map(|v| Val { v, ty }))
            .collect()
    };
    match m {
        "wrapping_sub" => one(&|y| x.v.checked_sub(operand(y, ty)?)),
        "wrapping_add" => one(&|y| x.v.checked_add(operand(y, ty)?)),
        "wrapping_add_signed" => one(&|y| x.v.checked_add(operand(y, counterpart)?)),
        "next_power_of_two" if args.is_empty() && !signed => {
            let v = u128::try_from(x.v).ok()?.checked_next_power_of_two()?;
            Some(vec![Val {
                v: ty.fit(i128::try_from(v).ok()?)?,
                ty,
            }])
        }
        "div_ceil" if !signed => one(&|y| {
            let d = operand(y, ty).filter(|&d| d > 0)?;
            Some((x.v + d - 1) / d)
        }),
        "wrapping_neg" if args.is_empty() => Some(vec![Val {
            v: ty.wrap(x.v.checked_neg()?)?,
            ty,
        }]),
        _ => None,
    }
}

/// The image address a value names as a reader's address argument: an image address (`u32` or
/// a literal inside the image), else an offset from A4 (a `u32` or a positive literal under
/// 0x8000: A4 minus it, the wrappers that take `A4 - off`; a signed or a negative literal from
/// -0x8000: A4 plus it); `Some(None)` for 0 where the reader takes it as no string; `None` for
/// another value.
fn address(x: Val, zero: bool) -> Option<Option<u32>> {
    let a4 = i128::from(A4);
    let unsigned = matches!(x.ty, Ty::U32 | Ty::Lit);
    let signed = matches!(x.ty, Ty::I32 | Ty::I16 | Ty::Lit);
    let at = if x.v == 0 && zero {
        return Some(None);
    } else if unsigned && (i128::from(IMAGE_BASE)..0x0020_0000).contains(&x.v) {
        x.v
    } else if unsigned && (1..0x8000).contains(&x.v) {
        a4 - x.v
    } else if signed && (-0x8000..0).contains(&x.v) {
        a4 + x.v
    } else {
        return None;
    };
    u32::try_from(at).ok().map(Some)
}

/// The sources build `wide` (or not) compiles: from `lib.rs` through each `mod name;` it keeps;
/// every source when there is no `lib.rs` (a test's snippets).
fn compiled(sources: &[Source], wide: bool) -> BTreeSet<String> {
    let all = || sources.iter().map(|s| s.path.clone()).collect();
    let by_path: BTreeMap<&str, &Source> = sources.iter().map(|s| (s.path.as_str(), s)).collect();
    if !by_path.contains_key("lib.rs") {
        return all();
    }
    let mut out = BTreeSet::new();
    let mut todo = vec!["lib.rs".to_string()];
    while let Some(path) = todo.pop() {
        let Some(src) = by_path.get(path.as_str()) else {
            continue;
        };
        let Ok(f) = &src.file else {
            out.insert(path);
            continue;
        };
        if cfg_off(&f.attrs, wide) {
            continue;
        }
        let dir = match module_of(&path) {
            "" => String::new(),
            m => format!("{m}/"),
        };
        let mut mods = Vec::new();
        declared(&f.items, &dir, &|a| !cfg_off(a, wide), &mut mods);
        for m in mods {
            let file = format!("{m}.rs");
            todo.push(if by_path.contains_key(file.as_str()) {
                file
            } else {
                format!("{m}/mod.rs")
            });
        }
        out.insert(path);
    }
    out
}

impl<'a> Scan<'a> {
    /// The scan of `sources` as the library's build with the `wide` feature or without compiles
    /// them ([`cfg_holds`]).
    pub(super) fn new(sources: &'a [Source], wide: bool) -> Scan<'a> {
        let (mut scopes, mut modules, mut calls, mut walked) =
            (Vec::new(), BTreeMap::new(), Vec::new(), Vec::new());
        let mut ambiguous = BTreeSet::new();
        let (mut globs, mut renames, mut literals) = (Vec::new(), Vec::new(), Vec::new());
        let compiled = compiled(sources, wide);
        for (k, src) in sources.iter().enumerate() {
            if !compiled.contains(&src.path) {
                continue;
            }
            let file = match &src.file {
                Ok(f) => f,
                Err(line) => {
                    walked.push(format!("{}:{line}: does not parse", src.path));
                    continue;
                }
            };
            if cfg_off(&file.attrs, wide) {
                continue;
            }
            for a in &file.attrs {
                if a.path().is_ident("no_implicit_prelude") {
                    let line = a.span().start().line;
                    walked.push(format!(
                        "{}:{line} no_implicit_prelude: a form the scan rejects",
                        src.path
                    ));
                }
            }
            let mut w = Walk {
                wide,
                src: k,
                file: &src.path,
                text: &src.text,
                scopes: &mut scopes,
                modules: &mut modules,
                calls: &mut calls,
                found: &mut walked,
                ambiguous: &mut ambiguous,
                globs: &mut globs,
                renames: &mut renames,
                literals: &mut literals,
                in_site: 0,
                in_const: 0,
                qself: BTreeSet::new(),
                stack: Vec::new(),
            };
            w.open(Some(module_of(&src.path).to_string()));
            w.declare(&file.items.iter().collect::<Vec<_>>());
            for item in &file.items {
                w.visit_item(item);
            }
        }
        let mut scan = Scan {
            sources,
            scopes,
            modules,
            ambiguous,
            calls,
            walked,
            literals,
        };
        let more = scan.glob_findings(&globs);
        scan.walked.extend(more);
        // A `use … as name` where `name` is a module's in scope (a child module of the scope's).
        for (scope, line, name) in renames {
            let here = &scan.scopes[scope].module;
            let child = if here.is_empty() {
                name.clone()
            } else {
                format!("{here}/{name}")
            };
            if scan.modules.contains_key(&child) || scan.ambiguous.contains(&child) {
                let file = &scan.sources[scan.scopes[scope].src].path;
                let f = format!("{file}:{line} use as {name}: a form the scan rejects");
                scan.walked.push(f);
            }
        }
        scan
    }

    /// Each glob `use` of the crate's own modules that brings reader names in (its module
    /// declares a fn of a reader's name), or names a module of the crate the scan cannot read.
    fn glob_findings(&self, globs: &[(usize, usize, Vec<String>)]) -> Vec<String> {
        let mut out = Vec::new();
        let mut used = vec![false; GLOBS.len()];
        for (scope, line, path) in globs {
            let file = &self.sources[self.scopes[*scope].src].path;
            // The crate's own module: by `crate`, `self` or `super`, or a child module (one the scan
            // knows, or one two `cfg` variants share).
            let here = &self.scopes[*scope].module;
            let own = path.first().is_some_and(|f| {
                let child = if here.is_empty() {
                    f.clone()
                } else {
                    format!("{here}/{f}")
                };
                ["crate", "self", "super"].contains(&f.as_str())
                    || self.modules.contains_key(&child)
                    || self.ambiguous.contains(&child)
            });
            match self.module(path, *scope, 0) {
                Some(m) => {
                    let names: Vec<&str> = READERS
                        .iter()
                        .filter(|r| r.scope.is_empty() || r.scope == *file)
                        .map(|r| r.name)
                        .filter(|n| self.scopes[m].items.contains(*n))
                        .collect();
                    for n in names {
                        let entry = GLOBS.iter().position(|&(f, r, _)| f == file && r == n);
                        if let Some(k) = entry {
                            used[k] = true;
                        } else {
                            out.push(format!(
                                "{file}:{line} use: brings the reader {n} in by a glob"
                            ));
                        }
                    }
                }
                // A glob of `Arg`'s variants is the walk's finding already.
                None if own && path.last().is_some_and(|l| l != "Arg") => out.push(format!(
                    "{file}:{line} use: a glob of a module the scan cannot read"
                )),
                None => {}
            }
        }
        // Only for the crate's own sources (a test's snippets have no `lib.rs`).
        if self.sources.iter().any(|s| s.path == "lib.rs") {
            for (k, u) in used.iter().enumerate() {
                if !u {
                    let (f, r, _) = GLOBS[k];
                    out.push(format!("{f} {r}: a glob entry no use matches"));
                }
            }
        }
        out
    }

    /// The values of `e` in scope `scope`, `None` when it does not resolve to consts exactly or
    /// to no value (an empty array's element is no value). `want`: the integer type its context
    /// gives it (a const's declared type, through `+ - * / % | &`, a shift's left side, a
    /// bracket, a block and an `if`), which an unsuffixed literal there takes, as Rust infers it;
    /// a cast's operand, a method's receiver and a shift's amount get none.
    fn eval(&self, e: &Expr, scope: usize, depth: usize, want: Option<Ty>) -> Option<Vec<Val>> {
        let v = self.eval_inner(e, scope, depth, want)?;
        let v: Vec<Val> = match want {
            Some(t) => v
                .into_iter()
                .map(|x| match x.ty {
                    Ty::Lit => t.fit(x.v).map(|v| Val { v, ty: t }),
                    _ => Some(x),
                })
                .collect::<Option<_>>()?,
            None => v,
        };
        (!v.is_empty()).then_some(v)
    }

    fn eval_inner(
        &self,
        e: &Expr,
        scope: usize,
        depth: usize,
        want: Option<Ty>,
    ) -> Option<Vec<Val>> {
        if depth > 24 {
            return None;
        }
        let d = depth + 1;
        match e {
            Expr::Lit(l) => match &l.lit {
                syn::Lit::Int(i) => Some(vec![lit_val(i, false)?]),
                _ => None,
            },
            Expr::Unary(u) if matches!(u.op, syn::UnOp::Neg(_)) => {
                if let Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Int(i),
                    ..
                }) = &*u.expr
                {
                    return Some(vec![lit_val(i, true)?]);
                }
                self.eval(&u.expr, scope, d, want)?
                    .into_iter()
                    .map(|x| {
                        x.ty.width().is_none_or(|(_, s)| s).then_some(())?;
                        let v = x.ty.fit(x.v.checked_neg()?)?;
                        Some(Val { v, ty: x.ty })
                    })
                    .collect()
            }
            Expr::Reference(r) => self.eval(&r.expr, scope, d, want),
            Expr::Paren(p) => self.eval(&p.expr, scope, d, want),
            Expr::Group(g) => self.eval(&g.expr, scope, d, want),
            Expr::Binary(b) => {
                let op = match b.op {
                    syn::BinOp::Add(_) => '+',
                    syn::BinOp::Sub(_) => '-',
                    syn::BinOp::Mul(_) => '*',
                    syn::BinOp::Div(_) => '/',
                    syn::BinOp::Rem(_) => '%',
                    syn::BinOp::BitOr(_) => '|',
                    syn::BinOp::BitAnd(_) => '&',
                    syn::BinOp::Shl(_) => '<',
                    syn::BinOp::Shr(_) => '>',
                    _ => return None,
                };
                let shift = matches!(op, '<' | '>');
                let (l, r) = (
                    self.eval(&b.left, scope, d, want)?,
                    self.eval(&b.right, scope, d, if shift { None } else { want })?,
                );
                each(&l, &r, |x, y| arith(x, y, op))
            }
            Expr::Cast(c) => {
                let ty = int_ty(&c.ty)?;
                self.eval(&c.expr, scope, d, None)?
                    .into_iter()
                    .map(|x| ty.wrap(x.v).map(|v| Val { v, ty }))
                    .collect()
            }
            Expr::MethodCall(m) if m.turbofish.is_none() => {
                let x = self.eval(&m.receiver, scope, d, None)?;
                let args = m
                    .args
                    .iter()
                    .map(|a| self.eval(a, scope, d, None))
                    .collect::<Option<Vec<_>>>()?;
                let m = m.method.to_string();
                Some(
                    x.iter()
                        .map(|&x| method(x, &m, &args))
                        .collect::<Option<Vec<Vec<Val>>>>()?
                        .concat(),
                )
            }
            Expr::Call(c) => {
                let Expr::Path(p) = &*c.func else { return None };
                let path = plain(p)?;
                let args = c
                    .args
                    .iter()
                    .map(|a| self.eval(a, scope, d, None))
                    .collect::<Option<Vec<_>>>()?;
                self.call(&path, &args, scope, d)
            }
            Expr::Path(p) => self.resolve(&plain(p)?, scope, Sel::Scalar, d),
            Expr::Index(i) => {
                let Expr::Path(p) = &*i.expr else { return None };
                self.resolve(&plain(p)?, scope, Sel::Elems, d)
            }
            Expr::Field(f) => {
                let (Expr::Path(p), syn::Member::Unnamed(i)) = (&*f.base, &f.member) else {
                    return None;
                };
                self.resolve(&plain(p)?, scope, Sel::Field(i.index as usize), d)
            }
            Expr::If(i) => {
                let (_, other) = i.else_branch.as_ref()?;
                let mut v = self.eval(block_expr(&i.then_branch)?, scope, d, want)?;
                for x in self.eval(other, scope, d, want)? {
                    if !v.contains(&x) {
                        v.push(x);
                    }
                }
                Some(v)
            }
            Expr::Block(b) if b.label.is_none() => self.eval(block_expr(&b.block)?, scope, d, want),
            _ => None,
        }
    }

    /// The values of const `c` as `sel` takes it (an array's elements, a tuple's element), typed
    /// as declared.
    fn eval_const(&self, c: &Const, sel: Sel, depth: usize) -> Option<Vec<Val>> {
        if c.mutable {
            return None;
        }
        let (ty, values) = if let Sel::Field(i) = sel {
            let syn::Type::Tuple(t) = &c.ty else {
                return None;
            };
            let Expr::Tuple(v) = &c.value else {
                return None;
            };
            let (ty, e) = (int_ty(t.elems.iter().nth(i)?)?, v.elems.iter().nth(i)?);
            (ty, self.eval(e, c.scope, depth, Some(ty))?)
        } else if sel == Sel::Elems {
            let ty = elem_ty(&c.ty)?;
            let mut body = &c.value;
            while let Expr::Reference(r) = body {
                body = &r.expr;
            }
            let Expr::Array(a) = body else { return None };
            let mut v = Vec::new();
            for e in &a.elems {
                v.extend(self.eval(e, c.scope, depth, Some(ty))?);
            }
            (ty, v)
        } else {
            let ty = int_ty(&c.ty)?;
            (ty, self.eval(&c.value, c.scope, depth, Some(ty))?)
        };
        let v: Vec<Val> = values
            .into_iter()
            .map(|x| {
                Some(Val {
                    v: ty.fit(operand(x, ty)?)?,
                    ty,
                })
            })
            .collect::<Option<_>>()?;
        (!v.is_empty()).then_some(v)
    }

    /// The scopes from `scope` out to its module, innermost first.
    fn chain(&self, scope: usize) -> Vec<usize> {
        let mut out = vec![scope];
        while let Some(p) = self.scopes[*out.last().expect("a scope")].parent {
            out.push(p);
        }
        out
    }

    /// The source of the module `segs` names from `scope` (`crate`, `super`, `self`, a child
    /// module, or a name a `use` in scope brings in); `None` when it is no module of the sources.
    fn module(&self, segs: &[String], scope: usize, depth: usize) -> Option<usize> {
        if depth > 24 {
            return None;
        }
        let here: Vec<&str> = self.scopes[scope]
            .module
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        let (mut base, mut rest) = (here.clone(), segs);
        match segs.first().map(String::as_str)? {
            "crate" => {
                base.clear();
                rest = &segs[1..];
            }
            "self" => rest = &segs[1..],
            "super" => {
                while rest.first().map(String::as_str) == Some("super") {
                    base.pop()?;
                    rest = &rest[1..];
                }
            }
            first => {
                let child = here
                    .iter()
                    .copied()
                    .chain([first])
                    .collect::<Vec<_>>()
                    .join("/");
                if self.ambiguous.contains(&child) {
                    return None;
                }
                // As Rust looks a path's head up: from the innermost scope out, a fn's or a
                // block's `use` of the name that can be a module (or a glob that could bring one)
                // before the module's child module; at the module, the child module (a `use` of
                // its name there is of another namespace's item), else its one `use`.
                for s in self.chain(scope) {
                    let sc = &self.scopes[s];
                    let top = sc.parent.is_none();
                    if top && self.modules.contains_key(&child) {
                        break;
                    }
                    let bound: Vec<&Vec<String>> = sc
                        .uses
                        .get(first)
                        .into_iter()
                        .flatten()
                        .filter(|p| !self.no_module(p, s, depth + 1))
                        .collect();
                    if let [path] = bound.as_slice() {
                        let full: Vec<String> = path.iter().chain(&segs[1..]).cloned().collect();
                        return self.module(&full, s, depth + 1);
                    }
                    if !bound.is_empty() || top {
                        return None;
                    }
                    for g in &sc.globs {
                        let could = self.module(g, s, depth + 1).and_then(|m| {
                            if self.has_module(m, first) {
                                return Some(true);
                            }
                            self.binds(m, first, depth + 1)
                        });
                        if could != Some(false) {
                            return None;
                        }
                    }
                }
            }
        }
        let path: Vec<&str> = base
            .into_iter()
            .chain(rest.iter().map(String::as_str))
            .collect();
        let path = path.join("/");
        if self.ambiguous.contains(&path) {
            return None;
        }
        self.modules.get(&path).copied()
    }

    /// Whether module scope `m` has a child module `name`.
    fn has_module(&self, m: usize, name: &str) -> bool {
        let here = &self.scopes[m].module;
        let path = if here.is_empty() {
            name.to_string()
        } else {
            format!("{here}/{name}")
        };
        self.modules.contains_key(&path) || self.ambiguous.contains(&path)
    }

    /// Whether the `use` path `p` of scope `s` names no module: its parent is a module of the
    /// sources that declares its last name as another item (a fn, a type, a const) and has no
    /// child module or `use` of it.
    fn no_module(&self, p: &[String], s: usize, depth: usize) -> bool {
        let Some((leaf, parent)) = p.split_last() else {
            return false;
        };
        if parent.is_empty() || depth > 24 {
            return false;
        }
        let Some(m) = self.module(parent, s, depth + 1) else {
            return false;
        };
        let sc = &self.scopes[m];
        (sc.items.contains(leaf) || sc.consts.contains_key(leaf))
            && !sc.uses.contains_key(leaf)
            && !self.has_module(m, leaf)
    }

    /// The values `path` names from `scope`, taken as `sel` says.
    fn resolve(&self, path: &[String], scope: usize, sel: Sel, depth: usize) -> Option<Vec<Val>> {
        if depth > 24 {
            return None;
        }
        let (name, prefix) = path.split_last()?;
        if let ([t], "MAX" | "MIN", Sel::Scalar) = (prefix, name.as_str(), sel) {
            // A primitive's bound, when no item of its name is in scope.
            if let Some(ty) = Ty::parse(t).filter(|_| !self.shadowed(t, scope, depth)) {
                let (bits, signed) = ty.width()?;
                let v = match (name.as_str(), signed) {
                    ("MAX", true) => (1i128 << (bits - 1)) - 1,
                    ("MAX", false) => (1i128 << bits) - 1,
                    (_, true) => -(1i128 << (bits - 1)),
                    _ => 0,
                };
                return Some(vec![Val { v, ty }]);
            }
        }
        if prefix.is_empty() {
            self.name(name, &self.chain(scope), sel, depth)
        } else {
            let m = self.module(prefix, scope, depth)?;
            self.name(name, &[m], sel, depth)
        }
    }

    /// The values `name` has in `scopes` (innermost first): every const and `use` binding of it
    /// there, all of one value; a pattern binding of it there hides it (`None`); a glob in a
    /// scope inside the first one that binds it explicitly, which could bind it too (its module
    /// defines it, or is not known), hides it (`None`); with no explicit binding, the one value
    /// the globs that bind it give, `None` when a glob's module is not known. A glob outside an
    /// explicit binding, or beside one in its scope, is shadowed by it. `None` when the binding is
    /// not one the scan proves.
    fn name(&self, name: &str, scopes: &[usize], sel: Sel, depth: usize) -> Option<Vec<Val>> {
        if depth > 24 {
            return None;
        }
        let mut found: Option<Vec<Val>> = None;
        let mut agree = |v: Vec<Val>| -> Option<()> {
            if found.as_ref().is_some_and(|f| *f != v) {
                return None;
            }
            found = Some(v);
            Some(())
        };
        let (mut bound, mut hidden, mut unknown) = (false, false, false);
        let mut globbed = Vec::new();
        let mut outside = false;
        for &s in scopes {
            let sc = &self.scopes[s];
            if !outside && sc.locals.contains(name) {
                return None;
            }
            outside |= sc.boundary;
            if sc.consts.contains_key(name) || sc.uses.contains_key(name) {
                if hidden {
                    return None;
                }
                for c in sc.consts.get(name).into_iter().flatten() {
                    agree(self.eval_const(c, sel, depth + 1)?)?;
                }
                for p in sc.uses.get(name).into_iter().flatten() {
                    agree(self.resolve(p, s, sel, depth + 1)?)?;
                }
                bound = true;
                continue;
            }
            if bound {
                continue;
            }
            for g in &sc.globs {
                match self
                    .module(g, s, depth + 1)
                    .and_then(|m| Some((m, self.binds(m, name, depth + 1)?)))
                {
                    None => (hidden, unknown) = (true, true),
                    Some((m, true)) => {
                        hidden = true;
                        globbed.push(m);
                    }
                    Some((_, false)) => {}
                }
            }
        }
        if bound {
            return found;
        }
        if unknown {
            return None;
        }
        for m in globbed {
            agree(self.name(name, &[m], sel, depth + 1)?)?;
        }
        found
    }

    /// Whether module scope `m` binds `name` (a const, an item or a `use` of it, or a glob that
    /// does); `None` when a glob there names a module the scan does not know.
    fn binds(&self, m: usize, name: &str, depth: usize) -> Option<bool> {
        if depth > 24 {
            return None;
        }
        let sc = &self.scopes[m];
        if sc.consts.contains_key(name) || sc.uses.contains_key(name) || sc.items.contains(name) {
            return Some(true);
        }
        for g in &sc.globs {
            let target = self.module(g, m, depth + 1)?;
            if self.binds(target, name, depth + 1)? {
                return Some(true);
            }
        }
        Some(false)
    }

    /// The item (a type, a fn, a trait) `segs` names from `scope`: the scope that declares it and
    /// its name ([`Scan::bound`]).
    fn item(&self, segs: &[String], scope: usize, depth: usize) -> Option<(usize, String)> {
        self.bound(segs, scope, depth, false)
    }

    /// The const (or static) `segs` names from `scope`: the scope that declares it and its name
    /// ([`Scan::bound`], a pattern binding of it hiding it).
    fn binding(&self, segs: &[String], scope: usize, depth: usize) -> Option<(usize, String)> {
        self.bound(segs, scope, depth, true)
    }

    /// The declaration `segs` names from `scope` (a const's when `value`, else an item's): the
    /// scope that declares it and its name, through `use`s and globs under [`Scan::name`]'s
    /// rules; `None` when the binding is not one the scan proves.
    fn bound(
        &self,
        segs: &[String],
        scope: usize,
        depth: usize,
        value: bool,
    ) -> Option<(usize, String)> {
        if depth > 24 {
            return None;
        }
        let (name, prefix) = segs.split_last()?;
        let scopes = if prefix.is_empty() {
            self.chain(scope)
        } else {
            vec![self.module(prefix, scope, depth + 1)?]
        };
        let declares = |sc: &Scope| {
            if value {
                sc.consts.contains_key(name)
            } else {
                sc.items.contains(name)
            }
        };
        let mut found: Option<(usize, String)> = None;
        let mut agree = |v: (usize, String)| -> Option<()> {
            if found.as_ref().is_some_and(|f| *f != v) {
                return None;
            }
            found = Some(v);
            Some(())
        };
        let (mut bound, mut hidden, mut unknown) = (false, false, false);
        let mut globbed = Vec::new();
        let mut outside = false;
        for s in scopes {
            let sc = &self.scopes[s];
            if value && !outside && sc.locals.contains(name) {
                return None;
            }
            outside |= sc.boundary;
            if declares(sc) || sc.uses.contains_key(name) {
                if hidden {
                    return None;
                }
                if declares(sc) {
                    agree((s, name.clone()))?;
                }
                for p in sc.uses.get(name).into_iter().flatten() {
                    agree(self.bound(p, s, depth + 1, value)?)?;
                }
                bound = true;
                continue;
            }
            if bound {
                continue;
            }
            for g in &sc.globs {
                match self
                    .module(g, s, depth + 1)
                    .and_then(|m| Some((m, self.binds(m, name, depth + 1)?)))
                {
                    None => (hidden, unknown) = (true, true),
                    Some((m, true)) => {
                        hidden = true;
                        globbed.push(m);
                    }
                    Some((_, false)) => {}
                }
            }
        }
        if bound {
            return found;
        }
        if unknown {
            return None;
        }
        for m in globbed {
            agree(self.bound(std::slice::from_ref(name), m, depth + 1, value)?)?;
        }
        found
    }

    /// Whether `name` is bound in `scope`'s chain by an item, a `use` or a glob that could bind
    /// it (a type of the sources' that would hide a primitive's name).
    fn shadowed(&self, name: &str, scope: usize, depth: usize) -> bool {
        self.chain(scope).into_iter().any(|s| {
            let sc = &self.scopes[s];
            sc.items.contains(name)
                || sc.uses.contains_key(name)
                || sc.globs.iter().any(|g| {
                    self.module(g, s, depth + 1)
                        .and_then(|m| self.binds(m, name, depth + 1))
                        != Some(false)
                })
        })
    }

    /// The functions the sources call on addresses: the real `GameData::a4(off)` (A4 + off, `off`
    /// an `i32`; its path must name `objects::commands::data`'s `GameData`) and a primitive
    /// type's lossless `from` (no item of that name in scope); another is `None`.
    fn call(
        &self,
        path: &[String],
        args: &[Vec<Val>],
        scope: usize,
        depth: usize,
    ) -> Option<Vec<Val>> {
        let [a] = args else { return None };
        let (f, ty) = path.split_last()?;
        match (f.as_str(), ty) {
            ("a4", [_, ..]) => {
                let data = *self.modules.get("objects/commands/data")?;
                let real = self.item(ty, scope, depth + 1)? == (data, "GameData".to_string());
                real.then_some(())?;
                a.iter()
                    .map(|&y| {
                        let off = operand(y, Ty::I32)?;
                        let v = Ty::U32.wrap(i128::from(A4).checked_add(off)?)?;
                        Some(Val { v, ty: Ty::U32 })
                    })
                    .collect()
            }
            ("from", [t]) => {
                let ty = Ty::parse(t)?;
                (!self.shadowed(t, scope, depth)).then_some(())?;
                a.iter()
                    .map(|y| ty.fit(y.v).map(|v| Val { v, ty }))
                    .collect()
            }
            _ => None,
        }
    }

    /// Every site: each call of a [`READERS`] name, its address argument resolved, or its form
    /// found.
    pub(super) fn sites(&self) -> Vec<Site> {
        let mut out = Vec::new();
        for c in &self.calls {
            let file = &self.sources[c.src].path;
            let here: Vec<usize> = (0..READERS.len())
                .filter(|&r| {
                    let r = &READERS[r];
                    r.name == c.name && (r.scope.is_empty() || r.scope == *file)
                })
                .collect();
            if here.is_empty() {
                continue;
            }
            let site = |reader, arg: String, kind| Site {
                reader,
                scope: c.scope,
                file: file.clone(),
                line: c.line,
                callee: c.name.clone(),
                arg,
                kind,
            };
            let all = || {
                c.args
                    .iter()
                    .map(|(_, t)| t.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let matching: Vec<usize> = here
                .iter()
                .copied()
                .filter(|&r| {
                    !c.form && READERS[r].arity == c.args.len() && READERS[r].method == c.method
                })
                .collect();
            if matching.is_empty() {
                out.push(site(None, all(), Kind::Form(c.args.len(), c.method)));
                continue;
            }
            for r in matching {
                for &a in READERS[r].args {
                    let (expr, text) = &c.args[a];
                    let at = self.eval(expr, c.scope, 0, None).and_then(|v| {
                        v.into_iter()
                            .map(|x| address(x, READERS[r].zero))
                            .collect::<Option<Vec<Option<u32>>>>()
                    });
                    let at = at.map(|v| v.into_iter().flatten().collect::<Vec<u32>>());
                    // A 0 where the reader takes it as no string: no site.
                    if at.as_ref().is_some_and(Vec::is_empty) {
                        continue;
                    }
                    out.push(site(Some(r), text.clone(), Kind::Arg(at)));
                }
            }
        }
        out
    }

    /// Every const of the sources: its file, name, scope's entry.
    fn consts(&self) -> impl Iterator<Item = (&str, &str, &Const)> {
        self.scopes.iter().flat_map(move |s| {
            s.consts.iter().flat_map(move |(n, cs)| {
                cs.iter()
                    .map(move |c| (self.sources[s.src].path.as_str(), n.as_str(), c))
            })
        })
    }

    /// Every integer `e` of type `ty` holds (an integer; an array's, a slice's, a reference's or a
    /// tuple's, each element by its type: a literal of them, or a path to an integer array or
    /// slice const), typed as declared; `None` when one does not evaluate, or for no value.
    fn leaves(&self, ty: &syn::Type, e: &Expr, scope: usize, depth: usize) -> Option<Vec<Val>> {
        if depth > 24 {
            return None;
        }
        let d = depth + 1;
        if let Some(t) = int_ty(ty) {
            let v: Vec<Val> = self
                .eval(e, scope, d, Some(t))?
                .into_iter()
                .map(|x| {
                    Some(Val {
                        v: t.fit(operand(x, t)?)?,
                        ty: t,
                    })
                })
                .collect::<Option<_>>()?;
            return (!v.is_empty()).then_some(v);
        }
        let mut e = e;
        while let Expr::Reference(r) = e {
            e = &r.expr;
        }
        let v = match ty {
            syn::Type::Reference(r) => self.leaves(&r.elem, e, scope, d)?,
            syn::Type::Paren(p) => self.leaves(&p.elem, e, scope, d)?,
            syn::Type::Group(g) => self.leaves(&g.elem, e, scope, d)?,
            syn::Type::Array(_) | syn::Type::Slice(_) => {
                let elem = match ty {
                    syn::Type::Array(a) => &*a.elem,
                    syn::Type::Slice(s) => &*s.elem,
                    _ => return None,
                };
                match e {
                    Expr::Array(a) => {
                        let mut v = Vec::new();
                        for el in &a.elems {
                            v.extend(self.leaves(elem, el, scope, d)?);
                        }
                        v
                    }
                    Expr::Path(p) if int_ty(elem).is_some() => {
                        self.resolve(&plain(p)?, scope, Sel::Elems, d)?
                    }
                    _ => return None,
                }
            }
            syn::Type::Tuple(t) => {
                let Expr::Tuple(v) = e else {
                    return None;
                };
                if t.elems.len() != v.elems.len() {
                    return None;
                }
                let mut out = Vec::new();
                for (ty, el) in t.elems.iter().zip(&v.elems) {
                    out.extend(self.leaves(ty, el, scope, d)?);
                }
                out
            }
            _ => return None,
        };
        (!v.is_empty()).then_some(v)
    }

    /// The value of a scalar const, `None` when it does not resolve to one.
    fn const_value(&self, c: &Const) -> Option<Val> {
        match self.eval_const(c, Sel::Scalar, 0)?.as_slice() {
            [x] => Some(*x),
            _ => None,
        }
    }
}

/// The path `p` names, its segments' identifiers (a turbofish left out); `None` for a qualified
/// self type.
fn plain(p: &syn::ExprPath) -> Option<Vec<String>> {
    p.qself.is_none().then(|| {
        p.path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect()
    })
}

/// What a site is: a reader's address argument (resolved to addresses or not), or a call with a
/// reader's name in no reader's form (a callee no plain path is one too).
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Kind {
    Arg(Option<Vec<u32>>),
    Form(usize, bool),
}

/// One site of the scan: its reader row, its scope, its file under `src/`, line, callee,
/// argument (the code, each run of white space one space) and what it is.
#[derive(Clone, Debug)]
pub(super) struct Site {
    pub reader: Option<usize>,
    pub scope: usize,
    pub file: String,
    pub line: usize,
    pub callee: String,
    pub arg: String,
    pub kind: Kind,
}

/// What the scan finds wrong in `sites`: each site that resolves to an address not listed and
/// has no entry; each it cannot resolve with no entry, or an entry whose consts or slots are not
/// all listed (or whose wrapper is no reader); each call in a form no reader takes and no
/// [`OTHER_FORMS`] entry names; each entry that is not needed or matches no site, each form
/// entry no call matches, and each reader no call matches.
pub(super) fn findings(
    scan: &Scan<'_>,
    sites: &[Site],
    listed: &BTreeSet<u32>,
    allowed: &[(&str, &str, &str, Why)],
    forms: &[FormEntry],
) -> Vec<String> {
    let mut out = Vec::new();
    let mut used = vec![false; allowed.len()];
    let mut forms_used: Vec<Vec<bool>> = forms.iter().map(|f| vec![false; f.0.len()]).collect();
    let mut readers = vec![false; READERS.len()];
    for s in sites {
        let at = format!("{}:{} {}({})", s.file, s.line, s.callee, s.arg);
        let v = match &s.kind {
            Kind::Form(n, method) => {
                let entry = forms.iter().enumerate().find_map(|(k, &(fs, c, a, m, _))| {
                    let here = fs.iter().position(|f| *f == s.file)?;
                    (c == s.callee && a == *n && m == *method).then_some((k, here))
                });
                match entry {
                    Some((k, f)) => forms_used[k][f] = true,
                    None => out.push(format!("{at}: a form no reader takes")),
                }
                continue;
            }
            Kind::Arg(v) => v,
        };
        if let Some(r) = s.reader {
            readers[r] = true;
        }
        let entry = allowed
            .iter()
            .position(|&(f, c, a, _)| f == s.file && c == s.callee && a == s.arg);
        if let Some(k) = entry {
            used[k] = true;
        }
        let why = entry.map(|k| allowed[k].3);
        if why.is_some_and(|w| w.text().is_some_and(str::is_empty)) {
            out.push(format!("{at}: its entry gives no reason"));
        }
        match (v, why) {
            (Some(v), None) => {
                for a in v.iter().filter(|a| !listed.contains(a)) {
                    out.push(format!("{at} -> {a:#x}: not catalogued"));
                }
            }
            (Some(v), Some(why)) => {
                let resolved_reason = matches!(
                    why,
                    Why::NoWord | Why::Same(_) | Why::Logic(_) | Why::English(_)
                );
                if !resolved_reason {
                    out.push(format!("{at}: resolved, but its entry is for a variable"));
                } else if v.iter().all(|a| listed.contains(a)) {
                    out.push(format!("{at}: listed, its entry not needed"));
                }
            }
            (None, None) => out.push(format!("{at}: not resolved, no entry")),
            (None, Some(Why::NoWord | Why::Same(_))) => {
                out.push(format!("{at}: not resolved, its entry needs an address"));
            }
            (None, Some(why)) => {
                for f in checked(scan, why, &scan.chain(s.scope), listed) {
                    out.push(format!("{at}: {f}"));
                }
            }
        }
    }
    for (k, u) in used.iter().enumerate() {
        if !u {
            let (f, c, a, _) = allowed[k];
            out.push(format!("{f} {c}({a}): an entry no site matches"));
        }
    }
    for (k, used) in forms_used.iter().enumerate() {
        let (fs, c, a, _, _) = forms[k];
        for (f, u) in fs.iter().zip(used) {
            if !u {
                out.push(format!(
                    "{f} {c} of {a} arguments: a form entry no call matches"
                ));
            }
        }
    }
    for (k, u) in readers.iter().enumerate() {
        if !u && !UNBUILT.contains(&READERS[k].name) {
            let r = &READERS[k];
            out.push(format!(
                "reader {} of {} arguments: no call",
                r.name, r.arity
            ));
        }
    }
    out
}

/// What is wrong with a reason that names what a variable takes, read in `scopes`: a wrapper
/// named that is no reader, a const that does not resolve or names an address not listed, a
/// slot not listed; nothing for a reason with its own text.
fn checked(scan: &Scan<'_>, why: Why, scopes: &[usize], listed: &BTreeSet<u32>) -> Vec<String> {
    let mut out = Vec::new();
    match why {
        Why::Forwarded([]) => out.push("names no wrapper".to_string()),
        Why::Consts([]) => out.push("names no const".to_string()),
        Why::Slots(_, 0) => out.push("names no slot".to_string()),
        Why::Forwarded(ws) => {
            for w in ws.iter().filter(|w| !READERS.iter().any(|r| r.name == **w)) {
                out.push(format!("{w} is no reader"));
            }
        }
        Why::Consts(names) => {
            for n in names {
                let v = scan.name(n, scopes, Sel::Scalar, 0).and_then(|v| {
                    v.into_iter()
                        .map(|x| address(x, false).flatten())
                        .collect::<Option<Vec<u32>>>()
                });
                match v {
                    None => out.push(format!("{n} does not resolve")),
                    Some(v) => {
                        for a in v.iter().filter(|a| !listed.contains(a)) {
                            out.push(format!("{n} -> {a:#x}: not catalogued"));
                        }
                    }
                }
            }
        }
        Why::Slots(first, n) => {
            for a in (0..n).map(|i| first + 4 * i) {
                if !listed.contains(&a) {
                    out.push(format!("slot {a:#x}: not catalogued"));
                }
            }
        }
        _ => {}
    }
    out
}

/// The consts the entries name, by their binding (the scope that declares one and its name): each
/// path whose value is the argument of a site an entry matches ([`value_paths`]), and each const
/// its [`Why::Consts`] names,
/// resolved from the site's scope ([`Scan::binding`]); a spelling alone names nothing.
fn named(scan: &Scan<'_>, allowed: &[(&str, &str, &str, Why)]) -> BTreeSet<(usize, String)> {
    let mut out = BTreeSet::new();
    for s in scan.sites() {
        let entry = allowed
            .iter()
            .find(|&&(f, c, a, _)| f == s.file && c == s.callee && a == s.arg);
        let Some(&(_, _, _, why)) = entry else {
            continue;
        };
        let mut paths = Vec::new();
        if let Ok(e) = syn::parse_str::<Expr>(&s.arg) {
            value_paths(&e, &mut paths);
        }
        if let Why::Consts(ns) = why {
            paths.extend(ns.iter().map(|n| vec![(*n).to_string()]));
        }
        for p in paths {
            if let Some(b) = scan.binding(&p, s.scope, 0) {
                out.insert(b);
            }
        }
    }
    out
}

/// The paths whose value an expression is (the expression itself, through brackets, a reference,
/// a cast, an index, a tuple field, a block and each branch of an `if`); a path inside an
/// operation (`A4 - off`, a method's receiver) is not one: the expression's value is not its.
fn value_paths(e: &Expr, out: &mut Vec<Vec<String>>) {
    match e {
        Expr::Path(p) => out.extend(plain(p)),
        Expr::Paren(x) => value_paths(&x.expr, out),
        Expr::Group(x) => value_paths(&x.expr, out),
        Expr::Reference(x) => value_paths(&x.expr, out),
        Expr::Cast(x) => value_paths(&x.expr, out),
        Expr::Index(x) => value_paths(&x.expr, out),
        Expr::Field(x) => value_paths(&x.base, out),
        Expr::Block(b) => {
            if let Some(x) = block_expr(&b.block) {
                value_paths(x, out);
            }
        }
        Expr::If(i) => {
            if let Some(x) = block_expr(&i.then_branch) {
                value_paths(x, out);
            }
            if let Some((_, x)) = &i.else_branch {
                value_paths(x, out);
            }
        }
        _ => {}
    }
}

/// The addresses a const's value can name: an image address, or an offset from A4 (positive:
/// A4 minus it; negative: A4 plus it), whatever its integer type.
fn candidates(x: Val) -> Vec<u32> {
    let a4 = i128::from(A4);
    let mut out = Vec::new();
    if (i128::from(IMAGE_BASE)..0x0020_0000).contains(&x.v) {
        out.push(x.v);
    }
    if (1..0x8000).contains(&x.v) {
        out.push(a4 - x.v);
    }
    if (-0x8000..0).contains(&x.v) {
        out.push(a4 + x.v);
    }
    out.into_iter()
        .filter_map(|v| u32::try_from(v).ok())
        .collect()
}

/// What is wrong with `reasons` (static, no game): a reason for no const of the scan; one with
/// no text, or whose consts or slots do not check out; one for a const a list or an entry
/// already covers.
fn reason_findings(
    scan: &Scan<'_>,
    listed: &BTreeSet<u32>,
    allowed: &[(&str, &str, &str, Why)],
    reasons: &[(&str, &str, Why)],
) -> Vec<String> {
    let mut out = Vec::new();
    let names = named(scan, allowed);
    for &(file, name, why) in reasons {
        let consts: Vec<&Const> = scan
            .consts()
            .filter(|&(f, n, _)| f == file && n == name)
            .map(|(_, _, c)| c)
            .collect();
        let Some(&c) = consts.first() else {
            out.push(format!("{file} {name}: a reason no const matches"));
            continue;
        };
        if why.text().is_some_and(str::is_empty) {
            out.push(format!("{file} {name}: its reason gives no text"));
        }
        if matches!(why, Why::NoWord | Why::Same(_)) {
            out.push(format!("{file} {name}: its reason needs a text"));
        }
        for f in checked(scan, why, &scan.chain(c.scope), listed) {
            out.push(format!("{file} {name}: {f}"));
        }
        let covered = consts.iter().any(|c| {
            values_of(scan, c).is_some_and(|v| {
                let at: Vec<u32> = v.into_iter().flat_map(candidates).collect();
                !at.is_empty() && at.iter().all(|a| listed.contains(a))
            })
        });
        let entry = consts
            .iter()
            .any(|c| names.contains(&(c.scope, name.to_string())));
        if covered || entry {
            out.push(format!("{file} {name}: covered, its reason not needed"));
        }
    }
    out
}

/// The string consts' sweep, the offline floor: every `S_`, `STR_`, `MSG_` or `FMT_` const of the
/// sources typed `u32` or `i32` names a listed address, is named in an entry of its file
/// ([`named`]), or has its own reason in `reasons`, which [`reason_findings`] checks.
pub(super) fn sweep(
    scan: &Scan<'_>,
    listed: &BTreeSet<u32>,
    allowed: &[(&str, &str, &str, Why)],
    reasons: &[(&str, &str, Why)],
) -> Vec<String> {
    let mut out = reason_findings(scan, listed, allowed, reasons);
    let names = named(scan, allowed);
    let prefixed = |n: &str| {
        ["S_", "STR_", "MSG_", "FMT_"]
            .iter()
            .any(|p| n.starts_with(p))
    };
    for (file, name, c) in scan.consts() {
        let typed = matches!(int_ty(&c.ty), Some(Ty::U32 | Ty::I32));
        if !prefixed(name) || !typed {
            continue;
        }
        let x = scan.const_value(c);
        let at = x.and_then(|x| address(x, false).flatten());
        let listed_here = at.is_some_and(|a| listed.contains(&a));
        let reason = reasons.iter().any(|&(f, n, _)| f == file && n == name);
        if listed_here || reason || names.contains(&(c.scope, name.to_string())) {
            continue;
        }
        match at {
            Some(a) => out.push(format!(
                "{file} {name} -> {a:#x}: no list or entry names it"
            )),
            None => out.push(format!("{file} {name}: names no image address, no reason")),
        }
    }
    out.sort();
    out
}

/// Whether `ty` is an integer const's type: an integer, or an array or slice of one.
fn integer(ty: &syn::Type) -> bool {
    match ty {
        syn::Type::Array(a) => integer(&a.elem),
        syn::Type::Slice(s) => integer(&s.elem),
        syn::Type::Reference(r) => integer(&r.elem),
        syn::Type::Paren(p) => integer(&p.elem),
        syn::Type::Group(g) => integer(&g.elem),
        syn::Type::Tuple(t) => !t.elems.is_empty() && t.elems.iter().all(integer),
        _ => int_ty(ty).is_some(),
    }
}

/// The values of an integer const: every integer it holds, through its arrays, slices,
/// references and tuples; `None` when the scan cannot evaluate it.
fn values_of(scan: &Scan<'_>, c: &Const) -> Option<Vec<Val>> {
    scan.leaves(&c.ty, &c.value, c.scope, 0)
}

/// Each integer const of the sources the scan cannot evaluate and no reason names (static, no
/// game): the findings, and how many it cannot evaluate.
pub(super) fn unevaluated(scan: &Scan<'_>, reasons: &[(&str, &str, Why)]) -> (Vec<String>, usize) {
    let (mut out, mut count) = (Vec::new(), 0);
    for (file, name, c) in scan.consts() {
        if !integer(&c.ty) || values_of(scan, c).is_some() {
            continue;
        }
        count += 1;
        if !reasons.iter().any(|&(f, n, _)| f == file && n == name) {
            out.push(format!(
                "{file} {name}: an integer const the scan cannot evaluate, no reason"
            ));
        }
    }
    out.sort();
    (out, count)
}

/// Whether address `a` is inside one of `img`'s complete executable strings: a run of a loaded
/// segment from its start or a NUL to the next NUL in it, [`super::strings::MAX_BYTES`] or
/// shorter, that `catalogue` holds or counts uncatalogued, `a` on one of its bytes (not on its
/// NUL).
fn inside_string(
    img: &crate::image::GameImage<'_>,
    catalogue: &super::strings::Catalogue,
    a: u32,
) -> bool {
    let max = super::strings::MAX_BYTES;
    (0..img.segment_count()).any(|n| {
        let (Some(base), Some(seg)) = (img.segment_base(n), img.loaded(n)) else {
            return false;
        };
        let Some(o) = a
            .checked_sub(base)
            .map(|o| o as usize)
            .filter(|&o| o < seg.len())
        else {
            return false;
        };
        if seg[o] == 0 {
            return false;
        }
        let start = seg[..o].iter().rposition(|&b| b == 0).map_or(0, |z| z + 1);
        let Some(len) = seg[start..].iter().position(|&b| b == 0) else {
            return false;
        };
        let text = &seg[start..start + len];
        len <= max && (catalogue.of(text).is_some() || catalogue.uncatalogued(text))
    })
}

/// What is wrong with `literal_reasons` (static, no game): a reason for no literal of the scan
/// ([`Walk::visit_lit_int`]), one with no text of its own, one for a listed address.
fn literal_reason_findings(
    scan: &Scan<'_>,
    listed: &BTreeSet<u32>,
    literal_reasons: &[(&str, u32, Why)],
) -> Vec<String> {
    let mut out = Vec::new();
    for &(file, a, why) in literal_reasons {
        let here = scan
            .literals
            .iter()
            .any(|&(src, _, v)| v == a && scan.sources[src].path == file);
        if !here {
            out.push(format!(
                "{file} {a:#x}: a literal reason no literal matches"
            ));
        }
        if why.text().is_none_or(str::is_empty) || matches!(why, Why::NoWord | Why::Same(_)) {
            out.push(format!("{file} {a:#x}: its reason gives no text"));
        }
        if listed.contains(&a) {
            out.push(format!("{file} {a:#x}: covered, its reason not needed"));
        }
    }
    out
}

/// The const pass on a `game` (no names): every const of the sources of an integer type (an
/// array's or a slice's elements each) whose value names (as an image address or an offset from
/// A4, [`candidates`]) a byte inside one of the executable's complete strings
/// ([`inside_string`]) is listed at each such address, named in an entry, or has its reason;
/// every integer const the scan cannot evaluate has its reason ([`unevaluated`]); and every
/// image-address literal outside consts and sites ([`Walk::visit_lit_int`]) that names such a
/// byte is listed or has its reason in `literal_reasons`. Its blind spot: a string that follows
/// code with no NUL before it is no complete string (the catalogue's uncatalogued set has the
/// same). The findings, how many consts name such a string, how many the scan cannot evaluate,
/// and how many literals name such a string.
pub(super) fn const_pass(
    scan: &Scan<'_>,
    img: &crate::image::GameImage<'_>,
    catalogue: &super::strings::Catalogue,
    listed: &BTreeSet<u32>,
    allowed: &[(&str, &str, &str, Why)],
    reasons: &[(&str, &str, Why)],
    literal_reasons: &[(&str, u32, Why)],
) -> (Vec<String>, usize, usize, usize) {
    let (mut out, uneval) = unevaluated(scan, reasons);
    let names = named(scan, allowed);
    let mut naming = 0;
    for (file, name, c) in scan.consts() {
        if !integer(&c.ty) {
            continue;
        }
        // One the scan cannot evaluate is [`unevaluated`]'s.
        let Some(values) = values_of(scan, c) else {
            continue;
        };
        let strings: BTreeSet<u32> = values
            .into_iter()
            .flat_map(candidates)
            .filter(|&a| inside_string(img, catalogue, a))
            .collect();
        if strings.is_empty() {
            continue;
        }
        naming += 1;
        let reason = reasons.iter().any(|&(f, n, _)| f == file && n == name);
        if reason || names.contains(&(c.scope, name.to_string())) {
            continue;
        }
        for a in strings.into_iter().filter(|a| !listed.contains(a)) {
            out.push(format!(
                "{file} {name} -> {a:#x}: names a string no list or entry names"
            ));
        }
    }
    let mut literals = 0;
    for &(src, line, v) in &scan.literals {
        if !inside_string(img, catalogue, v) {
            continue;
        }
        literals += 1;
        let file = &scan.sources[src].path;
        let reason = literal_reasons.iter().any(|&(f, a, _)| f == file && a == v);
        if !listed.contains(&v) && !reason {
            out.push(format!(
                "{file}:{line} {v:#x}: a literal naming a string no list names"
            ));
        }
    }
    out.sort();
    (out, naming, uneval, literals)
}

/// What the walk found: each source that does not parse, each `macro_rules!` whose body names a
/// reader, each macro invoked whose body is no expressions and names one.
pub(super) fn macros(scan: &Scan<'_>) -> Vec<String> {
    scan.walked.clone()
}

/// The two builds' findings merged: each found in either build (in the first's order, then the
/// second's new ones), but one that a table entry matches nothing (an entry, a form entry, a
/// reason, a reader) only when it matches nothing in both.
fn merged(per: [Vec<String>; 2]) -> Vec<String> {
    let unused = |f: &String| {
        f.ends_with("an entry no site matches")
            || f.ends_with("a form entry no call matches")
            || f.ends_with("a reason no const matches")
            || f.ends_with("a glob entry no use matches")
            || f.ends_with("a literal reason no literal matches")
            || (f.starts_with("reader ") && f.ends_with(": no call"))
    };
    let [a, b] = per;
    let mut out: Vec<String> = Vec::new();
    for f in a.iter().chain(&b) {
        let keep = !unused(f) || (a.contains(f) && b.contains(f));
        if keep && !out.contains(f) {
            out.push(f.clone());
        }
    }
    out
}

/// Every finding of one build's scan of the crate: its sites, string consts, consts it cannot
/// evaluate, and its walk.
fn crate_findings(scan: &Scan<'_>, listed: &BTreeSet<u32>) -> Vec<String> {
    let mut found = findings(scan, &scan.sites(), listed, NOT_CATALOGUED, OTHER_FORMS);
    found.extend(sweep(scan, listed, NOT_CATALOGUED, CONST_REASONS));
    found.extend(unevaluated(scan, CONST_REASONS).0);
    found.extend(literal_reason_findings(scan, listed, LITERAL_REASONS));
    found.extend(macros(scan));
    found
}

/// The crate's sites: every one catalogued or with its entry, every form a reader's or named,
/// every string const listed, named or with its reason, every source parsed, no macro naming a
/// reader, every entry and reason needed.
#[test]
fn every_print_site_resolves_to_a_catalogued_string_or_to_an_entry_with_its_reason() {
    let sources = sources();
    let listed = listed();
    let builds = [Scan::new(&sources, false), Scan::new(&sources, true)];
    let found = merged([0, 1].map(|b| crate_findings(&builds[b], &listed)));
    let sites = builds.each_ref().map(|s| s.sites().len());
    assert!(
        found.is_empty(),
        "{} findings, {sites:?} sites in {} files:\n{}",
        found.len(),
        sources.len(),
        found.join("\n")
    );
    let scan = &builds[0];
    let prefixed = scan
        .consts()
        .filter(|(_, n, c)| {
            ["S_", "STR_", "MSG_", "FMT_"]
                .iter()
                .any(|p| n.starts_with(p))
                && matches!(int_ty(&c.ty), Some(Ty::U32 | Ty::I32))
        })
        .count();
    let uneval = builds.each_ref().map(|s| unevaluated(s, CONST_REASONS).1);
    eprintln!(
        "{sites:?} sites (without wide, with) in {} files, {} entries, {} form entries; {} consts, {prefixed} string consts, {uneval:?} not evaluated, {} reasons: every site catalogued or with its reason: pass",
        sources.len(),
        NOT_CATALOGUED.len(),
        OTHER_FORMS.len(),
        scan.consts().count(),
        CONST_REASONS.len()
    );
}

/// The articles the uncatalogued count leaves out (`strings::ENGLISH_AT`) are the consts the
/// object commands print them by and the object tables' loader reads, as the scan resolves them.
#[test]
fn the_articles_left_out_are_the_object_commands_and_loaders_consts() {
    let sources = sources();
    let scan = Scan::new(&sources, false);
    let at = |module: &str, names: [&str; 3]| -> Option<Vec<u32>> {
        let m = *scan.modules.get(module)?;
        names
            .iter()
            .map(|n| match scan.name(n, &[m], Sel::Scalar, 0)?.as_slice() {
                [x] => address(*x, false).flatten(),
                _ => None,
            })
            .collect()
    };
    let english = Some(super::strings::ENGLISH_AT.to_vec());
    assert_eq!(
        at("objects/commands/world", ["S_A", "S_AN", "S_THE"]),
        english,
        "the object commands' articles"
    );
    assert_eq!(
        at("objects/defs", ["STR_A", "STR_AN", "STR_THE"]),
        english,
        "the loader's articles"
    );
}

/// The findings of the scan's site checks (sites, forms, the walk) over `files` (path, code)
/// with `listed` and `allowed`, the readers no call uses left out.
fn found(
    files: &[(&str, &str)],
    listed: &[u32],
    allowed: &[(&str, &str, &str, Why)],
) -> Vec<String> {
    let sources: Vec<Source> = files.iter().map(|(p, t)| Source::new(p, t)).collect();
    let listed: BTreeSet<u32> = listed.iter().copied().collect();
    merged([false, true].map(|wide| {
        let scan = Scan::new(&sources, wide);
        let mut out: Vec<String> = findings(&scan, &scan.sites(), &listed, allowed, &[])
            .into_iter()
            .filter(|f| !f.starts_with("reader "))
            .collect();
        out.extend(macros(&scan));
        out
    }))
}

/// The const pass's findings over `files` with a synthetic `game` holding `parts` and its own
/// catalogue.
fn game_findings(
    files: &[(&str, &str)],
    parts: &[(u32, &[u8])],
    listed: &[u32],
    allowed: &[(&str, &str, &str, Why)],
    reasons: &[(&str, &str, Why)],
) -> Vec<String> {
    let sources: Vec<Source> = files.iter().map(|(p, t)| Source::new(p, t)).collect();
    let game = super::strings::test_game(parts);
    let img = crate::image::GameImage::parse(&game).expect("the test's game reads");
    let catalogue = super::strings::Catalogue::from_game(&game);
    let listed: BTreeSet<u32> = listed.iter().copied().collect();
    merged([false, true].map(|wide| {
        let scan = Scan::new(&sources, wide);
        const_pass(&scan, &img, &catalogue, &listed, allowed, reasons, &[]).0
    }))
}

/// A string const no list holds, for the fixtures.
const UNLISTED: &str = "const S_B: u32 = 0x0012_0010;\n";

/// The scan on snippets of its own (no game code): a site of a listed const passes, one of a
/// const not listed is found, both arms of an `if` are read, a site in a comment, a string or a
/// test module is no site, a variable is found until an entry names what it takes, and an
/// entry naming a const not listed, or matching no site, is found.
#[test]
fn the_scan_finds_a_site_not_catalogued_and_a_variable_with_no_entry() {
    let text = "const S_A: u32 = 0x0012_0000;
const S_B: u32 = 0x0012_0010;
const OFF: u32 = 0x7000;
fn f(s: &mut State, io: &Io, k: bool, n: u32) {
    queue_at(s, S_A, Vec::new());
    queue_at(s, if k { S_A } else { S_B }, Vec::new());
    // queue_at(s, S_B, Vec::new());
    let t = \"queue_at(s, S_B, v)\";
    game_string(io, n);
    a4_string(io, OFF);
}
#[cfg(test)]
mod tests {
    fn g() {
        queue_at(s, S_B, Vec::new());
    }
}
";
    let listed = [0x0012_0000, A4 - 0x7000];
    assert_eq!(
        found(&[("x/y.rs", text)], &listed, &[]),
        [
            "x/y.rs:6 queue_at(if k { S_A } else { S_B }) -> 0x120010: not catalogued",
            "x/y.rs:9 game_string(n): not resolved, no entry",
        ],
        "the const not listed and the variable"
    );
    let allowed = [
        (
            "x/y.rs",
            "queue_at",
            "if k { S_A } else { S_B }",
            Why::Logic("test"),
        ),
        ("x/y.rs", "game_string", "n", Why::Consts(&["S_A"])),
    ];
    assert!(
        found(&[("x/y.rs", text)], &listed, &allowed).is_empty(),
        "each with its entry"
    );
    let wrong = [
        ("x/y.rs", "game_string", "n", Why::Consts(&["S_B"])),
        ("x/y.rs", "queue_at", "S_A", Why::NoWord),
        ("x/y.rs", "queue_at", "S_C", Why::NoWord),
    ];
    assert_eq!(
        found(&[("x/y.rs", text)], &listed, &wrong),
        [
            "x/y.rs:5 queue_at(S_A): listed, its entry not needed",
            "x/y.rs:6 queue_at(if k { S_A } else { S_B }) -> 0x120010: not catalogued",
            "x/y.rs:9 game_string(n): S_B -> 0x120010: not catalogued",
            "x/y.rs queue_at(S_C): an entry no site matches",
        ],
        "a const not listed, an entry not needed, an entry of no site"
    );
}

#[test]
fn fixture_a_a_space_or_a_newline_before_the_bracket_is_still_a_call() {
    let text = format!(
        "{UNLISTED}fn f(s: &mut State) {{\n    queue_at (s, S_B, Vec::new());\n    queue_at\n        (s, S_B, Vec::new());\n}}\n"
    );
    assert_eq!(
        found(&[("x/y.rs", &text)], &[], &[]),
        [
            "x/y.rs:3 queue_at(S_B) -> 0x120010: not catalogued",
            "x/y.rs:4 queue_at(S_B) -> 0x120010: not catalogued",
        ],
        "both calls"
    );
    assert!(
        found(&[("x/y.rs", &text)], &[0x0012_0010], &[]).is_empty(),
        "listed"
    );
}

#[test]
fn fixture_b_a_reader_called_in_no_known_form_is_found() {
    let text = format!("{UNLISTED}fn f(s: &mut State) {{\n    queue_at(s, S_B);\n}}\n");
    assert_eq!(
        found(&[("x/y.rs", &text)], &[], &[]),
        ["x/y.rs:3 queue_at(s, S_B): a form no reader takes"],
        "the call of an unknown form"
    );
    let sources = [Source::new("x/y.rs", &text)];
    let scan = Scan::new(&sources, false);
    let named: [FormEntry; 1] = [(&["x/y.rs"], "queue_at", 2, false, "test")];
    let f = findings(&scan, &scan.sites(), &BTreeSet::new(), &[], &named);
    assert!(
        f.iter().all(|f| f.starts_with("reader ")),
        "a form entry names it: {f:?}"
    );
    assert_eq!(
        found(&[("x/y.rs", &text)], &[0x0012_0010], &[]),
        ["x/y.rs:3 queue_at(s, S_B): a form no reader takes"],
        "its address listed: still a form"
    );
}

#[test]
fn fixture_c_a_generic_argument_does_not_split_the_call() {
    let text = format!(
        "{UNLISTED}fn f(io: &Io) {{\n    game_string(io, conv::<u32, u8>(S_B));\n    game_string::<u8>(io, S_B);\n}}\n"
    );
    let conv = "x/y.rs:3 game_string(conv::<u32, u8>(S_B)): not resolved, no entry";
    assert_eq!(
        found(&[("x/y.rs", &text)], &[], &[]),
        [
            conv,
            "x/y.rs:4 game_string(S_B) -> 0x120010: not catalogued"
        ],
        "both calls, the turbofish one resolved"
    );
    assert_eq!(
        found(&[("x/y.rs", &text)], &[0x0012_0010], &[]),
        [conv],
        "listed"
    );
}

#[test]
fn fixture_d_binary_and_octal_literals_are_read_in_their_base() {
    let text = "fn f(io: &Io) {\n    game_string(io, 0b1);\n    game_string(io, 0o7);\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[], &[]),
        [
            "x/y.rs:2 game_string(0b1) -> 0x15b6ef: not catalogued",
            "x/y.rs:3 game_string(0o7) -> 0x15b6e9: not catalogued",
        ],
        "A4 - 1 and A4 - 7"
    );
    assert!(
        found(&[("x/y.rs", text)], &[A4 - 1, A4 - 7], &[]).is_empty(),
        "listed"
    );
}

#[test]
fn fixture_e_wrapping_and_casts_keep_their_width() {
    let text = "fn f(io: &Io) {\n    game_string(io, 0u32.wrapping_sub(1));\n    game_string(io, (-0x10i32) as u32);\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[A4 - 1, A4 - 0x10], &[]),
        [
            "x/y.rs:2 game_string(0u32.wrapping_sub(1)): not resolved, no entry",
            "x/y.rs:3 game_string((-0x10i32) as u32): not resolved, no entry",
        ],
        "neither is an offset from A4"
    );
    let fits = "fn f(io: &Io) {\n    game_string(io, 0x0012_0010u32.wrapping_sub(0x10));\n}\n";
    assert!(
        found(&[("x/y.rs", fits)], &[0x0012_0000], &[]).is_empty(),
        "in range, listed"
    );
    let casts = "fn f(io: &Io) {\n    game_string(io, 0x0012_0010i64 as u32);\n    game_string(io, 0x1_0012_0010u64 as u32);\n}\n";
    assert_eq!(
        found(&[("x/y.rs", casts)], &[], &[]),
        [
            "x/y.rs:2 game_string(0x0012_0010i64 as u32) -> 0x120010: not catalogued",
            "x/y.rs:3 game_string(0x1_0012_0010u64 as u32) -> 0x120010: not catalogued",
        ],
        "a cast in range, and one that wraps into an address"
    );
    assert!(
        found(&[("x/y.rs", casts)], &[0x0012_0010], &[]).is_empty(),
        "listed"
    );
}

#[test]
fn fixture_f_a_super_path_names_the_parent_modules_const() {
    let parent = "const S_X: u32 = 0x0012_0010;\nmod y;\n";
    let child =
        "const S_X: u32 = 0x0012_0000;\nfn f(io: &Io) {\n    game_string(io, super::S_X);\n}\n";
    let files = [("x/mod.rs", parent), ("x/y.rs", child)];
    assert_eq!(
        found(&files, &[0x0012_0000], &[]),
        ["x/y.rs:3 game_string(super::S_X) -> 0x120010: not catalogued"],
        "the parent's, resolved"
    );
    assert!(
        found(&files, &[0x0012_0010], &[]).is_empty(),
        "the parent's listed"
    );
    let unknown = "fn f(io: &Io) {\n    game_string(io, elsewhere::S_X);\n}\n";
    assert_eq!(
        found(&[("x/y.rs", unknown)], &[0x0012_0000], &[]),
        ["x/y.rs:2 game_string(elsewhere::S_X): not resolved, no entry"],
        "a path the scan cannot bind"
    );
}

#[test]
fn fixture_g_only_a_cfg_that_needs_test_strips_code() {
    let text = format!(
        "{UNLISTED}#[cfg(all(testing))]\nfn g(io: &Io) {{\n    game_string(io, S_B);\n}}\n#[cfg( test )]\nfn h(io: &Io) {{\n    game_string(io, S_B);\n}}\n#[cfg(all(feature = \"wide\", test))]\nfn k(io: &Io) {{\n    game_string(io, S_B);\n}}\n#[cfg(any(test, feature = \"wide\"))]\nfn m(io: &Io) {{\n    game_string(io, S_B);\n}}\n"
    );
    assert_eq!(
        found(&[("x/y.rs", &text)], &[], &[]),
        [
            "x/y.rs:4 game_string(S_B) -> 0x120010: not catalogued",
            "x/y.rs:16 game_string(S_B) -> 0x120010: not catalogued",
        ],
        "the two not only under test"
    );
    assert!(
        found(&[("x/y.rs", &text)], &[0x0012_0010], &[]).is_empty(),
        "listed"
    );
}

#[test]
fn fixture_h_a_macro_body_naming_a_reader_is_found() {
    let text = format!(
        "{UNLISTED}macro_rules! m {{\n    ($a:expr) => {{\n        let f = game_string;\n        f(io, $a)\n    }};\n}}\nfn g(io: &Io) {{\n    m!(S_B);\n}}\n"
    );
    assert_eq!(
        found(&[("x/y.rs", &text)], &[], &[]),
        ["x/y.rs:2 macro_rules!: names the reader game_string"],
        "the macro"
    );
    assert_eq!(
        found(&[("x/y.rs", &text)], &[0x0012_0010], &[]),
        ["x/y.rs:2 macro_rules!: names the reader game_string"],
        "its address listed: still the macro"
    );
    let none =
        format!("{UNLISTED}macro_rules! m {{\n    ($a:expr) => {{\n        $a\n    }};\n}}\n");
    assert!(
        found(&[("x/y.rs", &none)], &[], &[]).is_empty(),
        "a macro naming no reader"
    );
}

#[test]
fn fixture_i_malformed_code_and_overflow_are_found_not_panicked() {
    let unclosed = "fn g(io: &Io) {\n    game_string(";
    let f = found(&[("x/y.rs", unclosed)], &[], &[]);
    assert_eq!(f.len(), 1, "one finding: {f:?}");
    assert!(
        f[0].starts_with("x/y.rs:") && f[0].ends_with(": does not parse"),
        "the unclosed call, by its source: {f:?}"
    );
    let big = "fn g(io: &Io) {\n    game_string(io, 0x4000_0000_0000_0000 * 4);\n}\n";
    assert_eq!(
        found(&[("x/y.rs", big)], &[], &[]),
        ["x/y.rs:2 game_string(0x4000_0000_0000_0000 * 4): not resolved, no entry"],
        "the overflow"
    );
    let fits = "fn g(io: &Io) {\n    game_string(io, 0x0009_0000 * 2);\n}\n";
    assert!(
        found(&[("x/y.rs", fits)], &[0x0012_0000], &[]).is_empty(),
        "in range, listed"
    );
}

#[test]
fn fixture_sweep_a_string_const_no_list_or_entry_names_is_found() {
    let text = "const STR_OLD: u32 = 0x0012_0000;\nconst STR_NEW: u32 = 0x0012_0010;\nfn refusal(k: bool) -> Option<u32> {\n    if k {\n        return Some(STR_NEW);\n    }\n    Some(STR_OLD)\n}\nfn f(io: &Io, msg: u32) {\n    game_string(io, msg);\n}\n";
    let allowed = [("x/y.rs", "game_string", "msg", Why::Consts(&["STR_OLD"]))];
    assert!(
        found(&[("x/y.rs", text)], &[0x0012_0000], &allowed).is_empty(),
        "the site passes on its entry"
    );
    let sources = [Source::new("x/y.rs", text)];
    let scan = Scan::new(&sources, false);
    let listed = BTreeSet::from([0x0012_0000]);
    let f = sweep(&scan, &listed, &allowed, &[]);
    assert_eq!(
        f,
        ["x/y.rs STR_NEW -> 0x120010: no list or entry names it"],
        "the new const"
    );
    let reasons = [("x/y.rs", "STR_NEW", Why::Logic("test"))];
    assert!(
        sweep(&scan, &listed, &allowed, &reasons).is_empty(),
        "with its reason"
    );
    let both = BTreeSet::from([0x0012_0000, 0x0012_0010]);
    assert!(sweep(&scan, &both, &allowed, &[]).is_empty(), "listed");
}

/// Round 3's counterexamples (2026-10-27): each with the exact findings and, beside it, the same
/// form with its address listed giving none.
#[test]
fn counter_1_an_inner_cfg_test_strips_only_its_module() {
    let text = "const S_B: u32 = 0x0012_0010;\nmod tests {\n    #![cfg(test)]\n    fn g(io: &Io) {\n        game_string(io, S_B);\n    }\n}\nfn f(io: &Io) {\n    game_string(io, S_B);\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[], &[]),
        ["x/y.rs:9 game_string(S_B) -> 0x120010: not catalogued"],
        "the site outside the test module"
    );
    assert!(
        found(&[("x/y.rs", text)], &[0x0012_0010], &[]).is_empty(),
        "listed"
    );
}

#[test]
fn counter_2_a_name_two_fns_import_differently_is_each_fns_own() {
    let parent = "mod a;\nmod b;\nmod y;\n";
    let a = "pub const S_X: u32 = 0x0012_0000;\n";
    let b = "pub const S_X: u32 = 0x0012_0010;\n";
    let y = "fn f(io: &Io) {\n    use super::a::S_X as N;\n    game_string(io, N);\n}\nfn g(io: &Io) {\n    use super::b::S_X as N;\n    game_string(io, N);\n}\n";
    let files = [
        ("x/mod.rs", parent),
        ("x/a.rs", a),
        ("x/b.rs", b),
        ("x/y.rs", y),
    ];
    assert_eq!(
        found(&files, &[0x0012_0000], &[]),
        ["x/y.rs:7 game_string(N) -> 0x120010: not catalogued"],
        "g's N is b's"
    );
    assert!(
        found(&files, &[0x0012_0000, 0x0012_0010], &[]).is_empty(),
        "listed"
    );
}

#[test]
fn counter_3_a_nested_use_group_binds_its_name() {
    let a = "pub const S_B: u32 = 0x0012_0010;\n";
    let x = "use crate::{a::{S_B}};\nfn f(io: &Io) {\n    game_string(io, S_B);\n}\n";
    let files = [("a.rs", a), ("x.rs", x)];
    assert_eq!(
        found(&files, &[], &[]),
        ["x.rs:3 game_string(S_B) -> 0x120010: not catalogued"],
        "a's const"
    );
    assert!(found(&files, &[0x0012_0010], &[]).is_empty(), "listed");
}

#[test]
fn counter_4_a_reader_called_through_brackets_is_a_form() {
    let text = "fn f(off: u32) {\n    let v = [(Arg::Image)(off)];\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[], &[]),
        ["x/y.rs:2 Arg::Image(off): a form no reader takes"],
        "the bracketed callee"
    );
    let plain = "const S_A: u32 = 0x0012_0000;\nfn f() {\n    let v = [Arg::Image(S_A)];\n}\n";
    assert!(
        found(&[("x/y.rs", plain)], &[0x0012_0000], &[]).is_empty(),
        "listed"
    );
}

#[test]
fn counter_5_a_turbofish_with_spaces_or_a_fn_type_is_read() {
    let text = format!(
        "{UNLISTED}fn f(io: &Io) {{\n    game_string :: <u32, u32>(io, S_B);\n    game_string::<fn() -> u32, u32>(io, S_B);\n}}\n"
    );
    assert_eq!(
        found(&[("x/y.rs", &text)], &[], &[]),
        [
            "x/y.rs:3 game_string(S_B) -> 0x120010: not catalogued",
            "x/y.rs:4 game_string(S_B) -> 0x120010: not catalogued",
        ],
        "both calls"
    );
    assert!(
        found(&[("x/y.rs", &text)], &[0x0012_0010], &[]).is_empty(),
        "listed"
    );
}

#[test]
fn counter_6_a_parameter_is_not_another_fns_block_const() {
    let text = "fn a(io: &Io, S_X: u32) {\n    game_string(io, S_X);\n}\nfn b() {\n    const S_X: u32 = 0x0012_0000;\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[0x0012_0000], &[]),
        ["x/y.rs:2 game_string(S_X): not resolved, no entry"],
        "the parameter"
    );
    let own = "fn b(io: &Io) {\n    const S_X: u32 = 0x0012_0000;\n    game_string(io, S_X);\n}\n";
    assert!(
        found(&[("x/y.rs", own)], &[0x0012_0000], &[]).is_empty(),
        "the fn's own const, listed"
    );
}

#[test]
fn counter_7_a_spaced_macro_rules_naming_a_reader_is_found() {
    let text = "macro_rules ! m {\n    ($a:expr) => {\n        game_string(io, $a)\n    };\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[], &[]),
        ["x/y.rs:1 macro_rules!: names the reader game_string"],
        "the macro"
    );
    let none = "macro_rules ! m {\n    ($a:expr) => {\n        $a + 1\n    };\n}\n";
    assert!(
        found(&[("x/y.rs", none)], &[], &[]).is_empty(),
        "a macro naming no reader"
    );
}

#[test]
fn counter_8_a_negative_literal_fits_its_type_after_the_sign() {
    let text = "fn f(s: &State) {\n    a4_string(s, -128i8 as i32);\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[], &[]),
        ["x/y.rs:2 a4_string(-128i8 as i32) -> 0x15b670: not catalogued"],
        "A4 - 128"
    );
    assert!(
        found(&[("x/y.rs", text)], &[A4 - 128], &[]).is_empty(),
        "listed"
    );
}

#[test]
fn counter_9_a_signed_operand_out_of_its_type_is_not_resolved() {
    let text =
        "fn f(io: &Io) {\n    game_string(io, 1u32.wrapping_add_signed(0x1_0000_0000));\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[A4 - 1], &[]),
        ["x/y.rs:2 game_string(1u32.wrapping_add_signed(0x1_0000_0000)): not resolved, no entry"],
        "no value"
    );
    let fits =
        "fn f(io: &Io) {\n    game_string(io, 0x0012_0010u32.wrapping_add_signed(-0x10));\n}\n";
    assert!(
        found(&[("x/y.rs", fits)], &[0x0012_0000], &[]).is_empty(),
        "in range, listed"
    );
}

#[test]
fn counter_10_an_unprefixed_or_narrow_const_naming_a_string_is_found_on_the_game() {
    let text = "const STR_OLD: u32 = 0x0012_0000;\nconst WORD: u32 = 0x0012_0010;\nconst OFF: u16 = 0x7303;\nfn refusal(k: bool, j: bool) -> Option<u32> {\n    if k {\n        return Some(WORD);\n    }\n    if j {\n        return Some(A4 - OFF as u32);\n    }\n    Some(STR_OLD)\n}\nfn f(io: &Io, msg: u32) {\n    game_string(io, msg);\n}\n";
    let parts: [(u32, &[u8]); 3] = [
        (0x0012_0000, b"Zc0 old"),
        (0x0012_0010, b"Zc1 word"),
        (A4 - 0x7303, b"Zc2 off"),
    ];
    let allowed = [("x/y.rs", "game_string", "msg", Why::Consts(&["STR_OLD"]))];
    assert_eq!(
        game_findings(&[("x/y.rs", text)], &parts, &[0x0012_0000], &allowed, &[]),
        [
            "x/y.rs OFF -> 0x1543ed: names a string no list or entry names",
            "x/y.rs WORD -> 0x120010: names a string no list or entry names",
        ],
        "both"
    );
    let listed = [0x0012_0000, 0x0012_0010, A4 - 0x7303];
    assert!(
        game_findings(&[("x/y.rs", text)], &parts, &listed, &allowed, &[]).is_empty(),
        "listed"
    );
}

#[test]
fn counter_11_a_const_reason_has_content_and_is_needed() {
    let text = "const STR_OLD: u32 = 0x0012_0000;\nconst STR_NEW: u32 = 0x0012_0010;\nfn f(io: &Io, msg: u32) {\n    game_string(io, msg);\n}\n";
    let allowed = [("x/y.rs", "game_string", "msg", Why::Consts(&["STR_OLD"]))];
    let sources = [Source::new("x/y.rs", text)];
    let scan = Scan::new(&sources, false);
    let listed = BTreeSet::from([0x0012_0000]);
    let empty = [("x/y.rs", "STR_NEW", Why::Logic(""))];
    assert_eq!(
        sweep(&scan, &listed, &allowed, &empty),
        ["x/y.rs STR_NEW: its reason gives no text"],
        "an empty reason"
    );
    let redundant = [
        ("x/y.rs", "STR_NEW", Why::Logic("test")),
        ("x/y.rs", "STR_OLD", Why::Logic("test")),
    ];
    assert_eq!(
        sweep(&scan, &listed, &allowed, &redundant),
        ["x/y.rs STR_OLD: covered, its reason not needed"],
        "a reason for a listed const"
    );
    let fine = [("x/y.rs", "STR_NEW", Why::Logic("test"))];
    assert!(
        sweep(&scan, &listed, &allowed, &fine).is_empty(),
        "a needed reason with text"
    );
}

/// Round 4's rules (2026-10-27): each fixture with the exact findings and, beside it, the case
/// the rule lets pass.
#[test]
fn rule1_a_reader_passed_to_a_macro_is_found() {
    let text = "macro_rules! m {\n    ($f:path, $io:expr, $a:expr) => {\n        $f($io, $a)\n    };\n}\nfn g(io: &Io) {\n    m!(game_string, io, 0x0012_0010);\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[0x0012_0010], &[]),
        ["x/y.rs:7 game_string: a reader named where no call reads it"],
        "the macro's argument"
    );
    let called = "fn g(io: &Io) {\n    let v = vec![game_string(io, 0x0012_0010)];\n}\n";
    assert!(
        found(&[("x/y.rs", called)], &[0x0012_0010], &[]).is_empty(),
        "called, listed"
    );
}

#[test]
fn rule1_a_reader_renamed_by_a_use_is_found() {
    let text =
        "use crate::main_loop::game_string as gs;\nfn f(io: &Io) {\n    gs(io, 0x0012_0010);\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[0x0012_0010], &[]),
        ["x/y.rs:1 use: brings the reader game_string in as gs"],
        "the rename"
    );
    let arg =
        "use crate::actors::seams::Arg as A;\nfn f(x: u32) {\n    let v = [A::Image(x)];\n}\n";
    assert_eq!(
        found(&[("x/y.rs", arg)], &[], &[]),
        ["x/y.rs:1 use: brings the reader Arg in as A"],
        "the enum renamed"
    );
    let plain = "use crate::main_loop::game_string;\nfn f(io: &Io) {\n    game_string(io, 0x0012_0010);\n}\n";
    assert!(
        found(&[("x/y.rs", plain)], &[0x0012_0010], &[]).is_empty(),
        "its own name, listed"
    );
}

#[test]
fn rule1_a_glob_of_args_variants_is_found() {
    let text = "use crate::actors::seams::Arg::*;\nfn f(x: u32) {\n    let v = [Image(x)];\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[], &[]),
        ["x/y.rs:1 use: brings Arg's variants in by a glob"],
        "the glob"
    );
    let one = "use crate::actors::seams::Arg::Image;\nfn f(x: u32) {\n    let v = [Image(x)];\n}\n";
    assert_eq!(
        found(&[("x/y.rs", one)], &[], &[]),
        ["x/y.rs:1 use: brings the reader Arg::Image in as Image"],
        "one variant by its bare name"
    );
    let enum_only = "use crate::actors::seams::Arg;\nconst S_A: u32 = 0x0012_0000;\nfn f() {\n    let v = [Arg::Image(S_A)];\n}\n";
    assert!(
        found(&[("x/y.rs", enum_only)], &[0x0012_0000], &[]).is_empty(),
        "the enum by its name, listed"
    );
}

#[test]
fn rule1_a_glob_bringing_a_readers_name_is_found() {
    let text = "mod t {\n    pub fn game_string(io: &Io, a: u32) {}\n}\nuse self::t::*;\nfn f(io: &Io) {\n    game_string(io, 0x0012_0010);\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[0x0012_0010], &[]),
        ["x/y.rs:4 use: brings the reader game_string in by a glob"],
        "the glob"
    );
    let none = "mod t {\n    pub fn other(a: u32) {}\n}\nuse self::t::*;\nfn f(io: &Io) {\n    game_string(io, 0x0012_0010);\n}\n";
    assert!(
        found(&[("x/y.rs", none)], &[0x0012_0010], &[]).is_empty(),
        "a glob of no reader"
    );
}

#[test]
fn rule1_a_reader_taken_as_a_value_is_found_but_not_a_local_of_its_name() {
    let text = "fn f(io: &Io) {\n    let f = game_string;\n    f(io, 0x0012_0010);\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[0x0012_0010], &[]),
        ["x/y.rs:2 game_string: a reader named where no call reads it"],
        "the value"
    );
    let local = "fn f(line: u32) {\n    let v = [line];\n}\n";
    assert!(
        found(&[("x/y.rs", local)], &[], &[]).is_empty(),
        "a local of a reader's name"
    );
}

#[test]
fn rule2_a4_only_through_the_real_gamedata() {
    let text = "mod other {\n    pub struct GameData;\n    impl GameData {\n        pub fn a4(_: i32) -> u32 {\n            0x120010\n        }\n    }\n}\nfn f(io: &Io) {\n    game_string(io, other::GameData::a4(0));\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[A4], &[]),
        ["x/y.rs:10 game_string(other::GameData::a4(0)): not resolved, no entry"],
        "another GameData"
    );
    let data = "pub struct GameData;\nimpl GameData {\n    pub const fn a4(off: i32) -> u32 {\n        A4.wrapping_add_signed(off)\n    }\n}\n";
    let user = "use super::data::GameData;\nfn f(io: &Io) {\n    game_string(io, GameData::a4(-0x10));\n}\n";
    let files = [
        ("objects/commands/data.rs", data),
        ("objects/commands/user.rs", user),
    ];
    assert!(
        found(&files, &[A4 - 0x10], &[]).is_empty(),
        "the real one, listed"
    );
}

#[test]
fn rule2_an_inner_glob_hides_an_outer_name() {
    let text = "const N: u32 = 0x0012_0000;\nmod a {\n    pub const N: u32 = 0x0012_0010;\n}\nfn f(io: &Io) {\n    use self::a::*;\n    game_string(io, N);\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[0x0012_0000], &[]),
        ["x/y.rs:7 game_string(N): not resolved, no entry"],
        "the glob could bind it"
    );
    let other = "const N: u32 = 0x0012_0000;\nmod a {\n    pub const M: u32 = 0x0012_0010;\n}\nfn f(io: &Io) {\n    use self::a::*;\n    game_string(io, N);\n}\n";
    assert!(
        found(&[("x/y.rs", other)], &[0x0012_0000], &[]).is_empty(),
        "a glob whose module does not define it"
    );
}

#[test]
fn rule2_an_empty_set_is_no_value() {
    let text = "const EMPTY: [u32; 0] = [];\nfn f(io: &Io, n: usize) {\n    game_string(io, EMPTY[n]);\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[], &[]),
        ["x/y.rs:3 game_string(EMPTY[n]): not resolved, no entry"],
        "no value"
    );
    let one = "const ONE: [u32; 1] = [0x0012_0010];\nfn f(io: &Io, n: usize) {\n    game_string(io, ONE[n]);\n}\n";
    assert!(
        found(&[("x/y.rs", one)], &[0x0012_0010], &[]).is_empty(),
        "one value, listed"
    );
}

#[test]
fn rule2_each_value_a_const_names_is_checked_on_its_own() {
    let text = "const BOTH: [u32; 2] = [0x0012_0000, 0x0012_0010];\n";
    let parts: [(u32, &[u8]); 2] = [(0x0012_0000, b"Zm1 one"), (0x0012_0010, b"Zm2 two")];
    assert_eq!(
        game_findings(&[("x/y.rs", text)], &parts, &[0x0012_0000], &[], &[]),
        ["x/y.rs BOTH -> 0x120010: names a string no list or entry names"],
        "the unlisted one"
    );
    let both = [0x0012_0000, 0x0012_0010];
    assert!(
        game_findings(&[("x/y.rs", text)], &parts, &both, &[], &[]).is_empty(),
        "both listed"
    );
}

#[test]
fn rule3_a_const_inside_a_string_is_found() {
    let text = "const TAIL: u32 = 0x0012_0014;\n";
    let parts: [(u32, &[u8]); 1] = [(0x0012_0010, b"Zc1 word")];
    assert_eq!(
        game_findings(&[("x/y.rs", text)], &parts, &[], &[], &[]),
        ["x/y.rs TAIL -> 0x120014: names a string no list or entry names"],
        "the suffix"
    );
    assert!(
        game_findings(&[("x/y.rs", text)], &parts, &[0x0012_0014], &[], &[]).is_empty(),
        "listed"
    );
}

#[test]
fn rule3_an_integer_const_the_scan_cannot_evaluate_is_found() {
    let text = "const RING: u32 = 0x0010_0000 ^ 0x1ef6;\nconst fn base() -> u32 {\n    0x0012_0000\n}\nconst CALLED: u32 = base();\n";
    let parts: [(u32, &[u8]); 1] = [(0x0012_0010, b"Zc1 word")];
    assert_eq!(
        game_findings(&[("x/y.rs", text)], &parts, &[], &[], &[]),
        [
            "x/y.rs CALLED: an integer const the scan cannot evaluate, no reason",
            "x/y.rs RING: an integer const the scan cannot evaluate, no reason",
        ],
        "both"
    );
    let reasons = [
        ("x/y.rs", "CALLED", Why::Logic("test")),
        ("x/y.rs", "RING", Why::Logic("test")),
    ];
    assert!(
        game_findings(&[("x/y.rs", text)], &parts, &[], &[], &reasons).is_empty(),
        "each with its reason"
    );
}

#[test]
fn rule3_a_bit_or_const_is_evaluated_and_checked() {
    let text = "const RING: u32 = 0x0010_0000 | 0x0002_0010;\n";
    let parts: [(u32, &[u8]); 1] = [(0x0012_0010, b"Zc1 word")];
    assert_eq!(
        game_findings(&[("x/y.rs", text)], &parts, &[], &[], &[]),
        ["x/y.rs RING -> 0x120010: names a string no list or entry names"],
        "its value"
    );
    assert!(
        game_findings(&[("x/y.rs", text)], &parts, &[0x0012_0010], &[], &[]).is_empty(),
        "listed"
    );
}

#[test]
fn rule2_the_modelled_operators_keep_rusts_values() {
    let text = "const P: (u32, u16) = (0x0012_0010, 0);\nconst H: usize = 37 / 2;\nfn f(io: &Io) {\n    game_string(io, 0x0024_0020 / 2);\n    game_string(io, u32::MAX & 0x0012_0010);\n    game_string(io, 0x0009_0008u32 << 1);\n    game_string(io, 0x0024_0021u32 >> 1);\n    game_string(io, P.0);\n    game_string(io, 0x0012_0000 + H as u32 - 2);\n    game_string(io, 0x0012_0000 + 9u32.next_power_of_two());\n    game_string(io, 0x0012_0000 + 33u32.div_ceil(2) - 1);\n    game_string(io, 0x0112_0010u32 % 0x0100_0000);\n    game_string(io, 0x0010_0000u32 | 0x0002_0010);\n    game_string(io, 1u32 << 32);\n    game_string(io, 7u32 % 0);\n}\n";
    let none = [
        "x/y.rs:14 game_string(1u32 << 32): not resolved, no entry",
        "x/y.rs:15 game_string(7u32 % 0): not resolved, no entry",
    ];
    let mut each: Vec<String> = [
        (4, "0x0024_0020 / 2"),
        (5, "u32::MAX & 0x0012_0010"),
        (6, "0x0009_0008u32 << 1"),
        (7, "0x0024_0021u32 >> 1"),
        (8, "P.0"),
        (9, "0x0012_0000 + H as u32 - 2"),
        (10, "0x0012_0000 + 9u32.next_power_of_two()"),
        (11, "0x0012_0000 + 33u32.div_ceil(2) - 1"),
        (12, "0x0112_0010u32 % 0x0100_0000"),
        (13, "0x0010_0000u32 | 0x0002_0010"),
    ]
    .iter()
    .map(|(line, arg)| format!("x/y.rs:{line} game_string({arg}) -> 0x120010: not catalogued"))
    .collect();
    each.extend(none.map(String::from));
    assert_eq!(
        found(&[("x/y.rs", text)], &[], &[]),
        each,
        "each the one value, unlisted; a shift past the type's bits and a remainder by 0 none"
    );
    assert_eq!(
        found(&[("x/y.rs", text)], &[0x0012_0010], &[]),
        none,
        "each listed"
    );
}

#[test]
fn audit_a_qualified_path_naming_a_reader_is_found() {
    let text = "fn f(io: &Io) {\n    let f = <T>::game_string;\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[], &[]),
        ["x/y.rs:2 game_string: a reader named where no call reads it"],
        "the qualified path"
    );
    let other = "fn f(io: &Io) {\n    let f = <T>::len;\n    let n = <[u8]>::to_vec(&[]);\n}\n";
    assert!(
        found(&[("x/y.rs", other)], &[], &[]).is_empty(),
        "qualified paths of no reader"
    );
}

#[test]
fn audit_a_traits_const_default_is_walked() {
    let text = "trait T {\n    const X: u32 = game_string(io, 0x0012_0010);\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[], &[]),
        ["x/y.rs:2 game_string(0x0012_0010) -> 0x120010: not catalogued"],
        "the default's call"
    );
    assert!(
        found(&[("x/y.rs", text)], &[0x0012_0010], &[]).is_empty(),
        "listed"
    );
}

#[test]
fn audit_a_tuple_or_nested_const_naming_a_string_is_found() {
    let text = "const PAIR: (u32, u16) = (0x0012_0010, 3);\nconst NESTED: [[u32; 1]; 1] = [[0x0012_0010]];\n";
    let parts: [(u32, &[u8]); 1] = [(0x0012_0010, b"Zc1 word")];
    assert_eq!(
        game_findings(&[("x/y.rs", text)], &parts, &[], &[], &[]),
        [
            "x/y.rs NESTED -> 0x120010: names a string no list or entry names",
            "x/y.rs PAIR -> 0x120010: names a string no list or entry names",
        ],
        "both"
    );
    assert!(
        game_findings(&[("x/y.rs", text)], &parts, &[0x0012_0010], &[], &[]).is_empty(),
        "listed"
    );
}

#[test]
fn audit_a_glob_of_a_module_two_variants_share_is_found() {
    let text = "#[cfg(debug_assertions)]\nmod c {\n    pub fn game_string(io: &Io, a: u32) {}\n}\n#[cfg(not(debug_assertions))]\nmod c {\n    pub fn other() {}\n}\nuse c::*;\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[], &[]),
        ["x/y.rs:9 use: a glob of a module the scan cannot read"],
        "the glob"
    );
    let one = "#[cfg(debug_assertions)]\nmod c {\n    pub fn other() {}\n}\nuse c::*;\nfn f(io: &Io) {\n    game_string(io, 0x0012_0010);\n}\n";
    assert!(
        found(&[("x/y.rs", one)], &[0x0012_0010], &[]).is_empty(),
        "a glob of a module one variant has, listed"
    );
}

#[test]
fn counter_11b_a_reason_for_a_const_an_entry_names_is_redundant() {
    let text = "const STR_OLD: u32 = 0x0012_0000;\nconst STR_NEW: u32 = 0x0012_0010;\nfn f(io: &Io, msg: u32) {\n    game_string(io, msg);\n}\n";
    let allowed = [("x/y.rs", "game_string", "msg", Why::Consts(&["STR_NEW"]))];
    let sources = [Source::new("x/y.rs", text)];
    let scan = Scan::new(&sources, false);
    let listed = BTreeSet::from([0x0012_0000]);
    let named = [("x/y.rs", "STR_NEW", Why::Logic("test"))];
    assert_eq!(
        sweep(&scan, &listed, &allowed, &named),
        ["x/y.rs STR_NEW: covered, its reason not needed"],
        "named in an entry, not listed"
    );
    let other = [("x/y.rs", "game_string", "msg", Why::Consts(&["STR_OLD"]))];
    assert!(
        sweep(&scan, &listed, &other, &named).is_empty(),
        "an entry naming another const: the reason needed"
    );
}

#[test]
fn audit_a_type_or_a_discriminant_naming_a_reader_is_found() {
    let text = "enum E {\n    A = { let _g = game_string; 0 },\n}\nstruct S {\n    f: [u8; { let _g = game_string; 1 }],\n}\ntype T = [u8; { let _g = game_string; 1 }];\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[], &[]),
        [
            "x/y.rs:2 game_string: a reader named where no call reads it",
            "x/y.rs:5 game_string: a reader named where no call reads it",
            "x/y.rs:7 game_string: a reader named where no call reads it",
        ],
        "the discriminant, the field's length and the alias's length"
    );
    let none = "enum E {\n    A = 1,\n}\nstruct S {\n    f: [u8; 2],\n}\nfn f(io: &Io) {\n    game_string(io, 0x0012_0010);\n}\n";
    assert!(
        found(&[("x/y.rs", none)], &[0x0012_0010], &[]).is_empty(),
        "no reader in them, the call listed"
    );
}

/// Round 5's items (2026-10-27): each fixture with the exact findings where round 4 gave none or
/// a wrong value and, beside it, a listed-address control.
#[test]
fn r5_scope_a_for_loops_binding_ends_with_the_loop() {
    let text = "fn f(s: &State, rows: &[u32]) {\n    for line in rows {}\n    let f = line;\n    f(s, 0x0012_0010);\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[0x0012_0010], &[]),
        ["x/y.rs:3 line: a reader named where no call reads it"],
        "the reader after the loop"
    );
    let call = "fn f(s: &State, rows: &[u32]) {\n    for line in rows {\n        let v = [line];\n    }\n    line(s, 0x0012_0010);\n}\n";
    assert_eq!(
        found(&[("x/y.rs", call)], &[], &[]),
        ["x/y.rs:5 line(0x0012_0010) -> 0x120010: not catalogued"],
        "the loop's own binding no reader, the call after it read"
    );
    assert!(
        found(&[("x/y.rs", call)], &[0x0012_0010], &[]).is_empty(),
        "listed"
    );
}

#[test]
fn r5_scope_a_let_reads_its_initializer_before_it_binds() {
    let text = "fn f(io: &Io) {\n    let game_string = game_string;\n    let f = game_string;\n    f(io, 0x0012_0010);\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[0x0012_0010], &[]),
        ["x/y.rs:2 game_string: a reader named where no call reads it"],
        "the initializer's reader"
    );
    let call = "fn f(io: &Io) {\n    let game_string = game_string(io, 0x0012_0010);\n    let v = [game_string];\n}\n";
    assert_eq!(
        found(&[("x/y.rs", call)], &[], &[]),
        ["x/y.rs:2 game_string(0x0012_0010) -> 0x120010: not catalogued"],
        "the initializer's call, then the binding no reader"
    );
    assert!(
        found(&[("x/y.rs", call)], &[0x0012_0010], &[]).is_empty(),
        "listed"
    );
}

#[test]
fn r5_scope_a_closures_parameter_ends_with_the_closure() {
    let text = "fn f(io: &Io) {\n    let _ = |game_string: u32| game_string;\n    let f = game_string;\n    f(io, 0x0012_0010);\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[0x0012_0010], &[]),
        ["x/y.rs:3 game_string: a reader named where no call reads it"],
        "the reader after the closure"
    );
    let call = "fn f(io: &Io) {\n    let _ = |game_string: u32| game_string;\n    game_string(io, 0x0012_0010);\n}\n";
    assert_eq!(
        found(&[("x/y.rs", call)], &[], &[]),
        ["x/y.rs:3 game_string(0x0012_0010) -> 0x120010: not catalogued"],
        "the call after the closure"
    );
    assert!(
        found(&[("x/y.rs", call)], &[0x0012_0010], &[]).is_empty(),
        "listed"
    );
}

#[test]
fn r5_scope_an_arm_an_if_let_a_while_let_and_a_nested_fn_end_their_bindings() {
    let text = "fn a(o: Option<u32>) {\n    match o {\n        Some(line) => {}\n        None => {}\n    }\n    let f = line;\n}\nfn b(o: Option<u32>) {\n    if let Some(line) = o {\n    } else {\n        let f = line;\n    }\n    let g = line;\n}\nfn c(mut o: Option<u32>) {\n    while let Some(line) = o {}\n    let f = line;\n}\nfn d(line: u32) {\n    fn inner() {\n        let f = line;\n    }\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[], &[]),
        [
            "x/y.rs:6 line: a reader named where no call reads it",
            "x/y.rs:11 line: a reader named where no call reads it",
            "x/y.rs:13 line: a reader named where no call reads it",
            "x/y.rs:17 line: a reader named where no call reads it",
            "x/y.rs:21 line: a reader named where no call reads it",
        ],
        "after the arm, in the else and after the if, after the loop, in a nested fn"
    );
    let inside = "fn a(o: Option<u32>) {\n    match o {\n        Some(line) => {\n            let f = line;\n        }\n        None => {}\n    }\n    if let Some(line) = o {\n        let f = line;\n    }\n    while let Some(line) = o {\n        let f = line;\n    }\n}\n";
    assert!(
        found(&[("x/y.rs", inside)], &[], &[]).is_empty(),
        "each binding inside its own scope"
    );
}

#[test]
fn r5_arith_a_shift_of_an_unsuffixed_literal_is_not_resolved() {
    let text = "fn f(io: &Io) {\n    game_string(io, ((1 << 31) >> 31) as u32);\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[A4 - 1], &[]),
        ["x/y.rs:2 game_string(((1 << 31) >> 31) as u32): not resolved, no entry"],
        "its width unknown"
    );
    let typed = "fn f(io: &Io) {\n    game_string(io, ((1u32 << 31) >> 31) as u32);\n}\n";
    assert_eq!(
        found(&[("x/y.rs", typed)], &[], &[]),
        ["x/y.rs:2 game_string(((1u32 << 31) >> 31) as u32) -> 0x15b6ef: not catalogued"],
        "a u32's, A4 - 1"
    );
    assert!(
        found(&[("x/y.rs", typed)], &[A4 - 1], &[]).is_empty(),
        "listed"
    );
}

#[test]
fn r5_arith_a_signed_minimum_remainder_by_minus_one_is_not_resolved() {
    let text = "fn f(io: &Io) {\n    game_string(io, 0x0012_0010 + (-128i8 % -1i8) as u32);\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[0x0012_0010], &[]),
        ["x/y.rs:2 game_string(0x0012_0010 + (-128i8 % -1i8) as u32): not resolved, no entry"],
        "an overflow in Rust"
    );
    let fits = "fn f(io: &Io) {\n    game_string(io, 0x0012_0010 + (-127i8 % -1i8) as u32);\n}\n";
    assert_eq!(
        found(&[("x/y.rs", fits)], &[], &[]),
        ["x/y.rs:2 game_string(0x0012_0010 + (-127i8 % -1i8) as u32) -> 0x120010: not catalogued"],
        "in range"
    );
    assert!(
        found(&[("x/y.rs", fits)], &[0x0012_0010], &[]).is_empty(),
        "listed"
    );
}

#[test]
fn r5_arith_an_operand_out_of_the_unified_type_is_not_resolved() {
    let text = "fn f(io: &Io) {\n    game_string(io, 0x0012_0010 + (0u32 & 0x1_0000_0000));\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[0x0012_0010], &[]),
        ["x/y.rs:2 game_string(0x0012_0010 + (0u32 & 0x1_0000_0000)): not resolved, no entry"],
        "no u32"
    );
    let fits = "fn f(io: &Io) {\n    game_string(io, 0x0012_0010 + (0u32 & 0xffff_ffff));\n}\n";
    assert_eq!(
        found(&[("x/y.rs", fits)], &[], &[]),
        ["x/y.rs:2 game_string(0x0012_0010 + (0u32 & 0xffff_ffff)) -> 0x120010: not catalogued"],
        "in range"
    );
    assert!(
        found(&[("x/y.rs", fits)], &[0x0012_0010], &[]).is_empty(),
        "listed"
    );
}

#[test]
fn r5_named_an_entry_names_the_const_its_site_binds() {
    let text = "mod a {\n    pub const WORD: u32 = 0x0012_0000;\n    pub fn f(io: &Io, msg: u32) {\n        game_string(io, msg);\n    }\n}\nmod b {\n    pub const WORD: u32 = 0x0012_0010;\n}\n";
    let parts: [(u32, &[u8]); 2] = [(0x0012_0000, b"Zc0 old"), (0x0012_0010, b"Zc1 word")];
    let allowed = [("x/y.rs", "game_string", "msg", Why::Consts(&["WORD"]))];
    assert_eq!(
        game_findings(&[("x/y.rs", text)], &parts, &[0x0012_0000], &allowed, &[]),
        ["x/y.rs WORD -> 0x120010: names a string no list or entry names"],
        "b's, which the entry does not name"
    );
    let both = [0x0012_0000, 0x0012_0010];
    assert!(
        game_findings(&[("x/y.rs", text)], &parts, &both, &allowed, &[]).is_empty(),
        "listed"
    );
}

#[test]
fn r5_reason_an_empty_structured_reason_is_found() {
    let text = "const STR_OLD: u32 = 0x0012_0000;\nconst STR_NEW: u32 = 0x0012_0010;\nfn f(io: &Io, msg: u32) {\n    game_string(io, msg);\n}\n";
    let listed = [0x0012_0000];
    for (why, what) in [
        (Why::Consts(&[]), "names no const"),
        (Why::Forwarded(&[]), "names no wrapper"),
        (Why::Slots(0x0012_0000, 0), "names no slot"),
    ] {
        let allowed = [("x/y.rs", "game_string", "msg", why)];
        assert_eq!(
            found(&[("x/y.rs", text)], &listed, &allowed),
            [format!("x/y.rs:4 game_string(msg): {what}")],
            "the site's"
        );
    }
    let sources = [Source::new("x/y.rs", text)];
    let scan = Scan::new(&sources, false);
    let set = BTreeSet::from([0x0012_0000]);
    let allowed = [("x/y.rs", "game_string", "msg", Why::Consts(&["STR_OLD"]))];
    let empty = [("x/y.rs", "STR_NEW", Why::Consts(&[]))];
    assert_eq!(
        sweep(&scan, &set, &allowed, &empty),
        ["x/y.rs STR_NEW: names no const"],
        "the const's"
    );
    assert!(
        found(&[("x/y.rs", text)], &listed, &allowed).is_empty(),
        "an entry naming a listed const"
    );
}

#[test]
fn r5_module_an_inner_use_beats_a_child_module() {
    let text = "mod a {\n    pub const S_X: u32 = 0x0012_0000;\n}\nmod c {\n    pub mod a {\n        pub const S_X: u32 = 0x0012_0010;\n    }\n}\nfn f(io: &Io) {\n    use self::c::a;\n    game_string(io, a::S_X);\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[0x0012_0000], &[]),
        ["x/y.rs:11 game_string(a::S_X) -> 0x120010: not catalogued"],
        "the fn's use, c's a"
    );
    assert!(
        found(&[("x/y.rs", text)], &[0x0012_0010], &[]).is_empty(),
        "listed"
    );
    let other = "mod a {\n    pub const S_X: u32 = 0x0012_0000;\n}\nmod c {\n    pub fn a() {}\n}\nfn f(io: &Io) {\n    use self::c::a;\n    game_string(io, a::S_X);\n}\n";
    assert_eq!(
        found(&[("x/y.rs", other)], &[], &[]),
        ["x/y.rs:9 game_string(a::S_X) -> 0x120000: not catalogued"],
        "a use of a fn of the name: the child module"
    );
    assert!(
        found(&[("x/y.rs", other)], &[0x0012_0000], &[]).is_empty(),
        "listed"
    );
}

#[test]
fn r5_static_a_static_binds_like_a_const() {
    let text =
        "const S: u32 = 1;\nfn f() {\n    static S: u32 = 2;\n    let v = [Arg::Image(S)];\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[A4 - 1, A4 - 2], &[]),
        ["x/y.rs:4 Arg::Image(S): not resolved, no entry"],
        "the fn's static against the module's const, as an inner const would be"
    );
    let alone = "static S: u32 = 2;\nfn f() {\n    let v = [Arg::Image(S)];\n}\n";
    assert_eq!(
        found(&[("x/y.rs", alone)], &[], &[]),
        ["x/y.rs:3 Arg::Image(S) -> 0x15b6ee: not catalogued"],
        "the static's value, A4 - 2"
    );
    assert!(
        found(&[("x/y.rs", alone)], &[A4 - 2], &[]).is_empty(),
        "listed"
    );
    let changing = "static mut S: u32 = 2;\nfn f() {\n    let v = [Arg::Image(S)];\n}\n";
    assert_eq!(
        found(&[("x/y.rs", changing)], &[A4 - 2], &[]),
        ["x/y.rs:3 Arg::Image(S): not resolved, no entry"],
        "a static mut has no value"
    );
}

#[test]
fn r5_reject_include() {
    let text = "include!(\"a.rs\");\nfn f() {\n    include!(\"b.rs\");\n    let v = std::include!(\"c.rs\");\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[], &[]),
        [
            "x/y.rs:1 include!: a form the scan rejects",
            "x/y.rs:3 include!: a form the scan rejects",
            "x/y.rs:4 include!: a form the scan rejects",
        ],
        "each form"
    );
    let other = "fn f(io: &Io) {\n    let v = vec![game_string(io, 0x0012_0010)];\n}\n";
    assert!(
        found(&[("x/y.rs", other)], &[0x0012_0010], &[]).is_empty(),
        "another macro, listed"
    );
}

#[test]
fn r5_reject_a_path_attribute() {
    let text = "#[path = \"other.rs\"]\nmod m;\n#[path = \"x\"]\nmod n {\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[], &[]),
        [
            "x/y.rs:2 #[path]: a form the scan rejects",
            "x/y.rs:4 #[path]: a form the scan rejects",
        ],
        "both modules"
    );
    let plain = "mod n {\n    fn f(io: &Io) {\n        game_string(io, 0x0012_0010);\n    }\n}\n";
    assert!(
        found(&[("x/y.rs", plain)], &[0x0012_0010], &[]).is_empty(),
        "a module of no path, listed"
    );
}

#[test]
fn r5_reject_a_leading_colon() {
    let text = "use ::std::fmt;\nfn f(io: &Io) {\n    ::x::game_string(io, 0x0012_0010);\n    let v: ::std::vec::Vec<u8> = Vec::new();\n    let n = ::x::S_B;\n    ::std::println!();\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[0x0012_0010], &[]),
        [
            "x/y.rs:1 ::path: a form the scan rejects",
            "x/y.rs:3 ::path: a form the scan rejects",
            "x/y.rs:4 ::path: a form the scan rejects",
            "x/y.rs:5 ::path: a form the scan rejects",
            "x/y.rs:6 ::path: a form the scan rejects",
        ],
        "each path"
    );
    let plain = "use std::fmt;\nfn f(io: &Io) {\n    x::game_string(io, 0x0012_0010);\n}\n";
    assert!(
        found(&[("x/y.rs", plain)], &[0x0012_0010], &[]).is_empty(),
        "no leading colon, listed"
    );
}

#[test]
fn r5_reject_no_implicit_prelude_and_a_from_or_into_of_the_library() {
    let text = "#![no_implicit_prelude]\nfn f() {}\n";
    let items = "#[no_implicit_prelude]\nmod m {}\nstruct From;\ntrait Into {}\nfn g() {\n    enum From {}\n}\nuse x::y as Into;\n";
    let mut f = found(&[("x/y.rs", text), ("x/z.rs", items)], &[], &[]);
    f.sort();
    assert_eq!(
        f,
        [
            "x/y.rs:1 no_implicit_prelude: a form the scan rejects",
            "x/z.rs:2 no_implicit_prelude: a form the scan rejects",
            "x/z.rs:3 From: a form the scan rejects",
            "x/z.rs:4 Into: a form the scan rejects",
            "x/z.rs:6 From: a form the scan rejects",
            "x/z.rs:8 Into: a form the scan rejects",
        ],
        "the attribute and each item"
    );
    let std = "use std::convert::From;\nfn f(io: &Io) {\n    game_string(io, 0x0012_0000 + u32::from(0x10u16));\n}\n";
    assert_eq!(
        found(&[("x/y.rs", std)], &[], &[]),
        ["x/y.rs:3 game_string(0x0012_0000 + u32::from(0x10u16)) -> 0x120010: not catalogued"],
        "std's From"
    );
    assert!(
        found(&[("x/y.rs", std)], &[0x0012_0010], &[]).is_empty(),
        "std's From, listed"
    );
}

#[test]
fn r5_reject_a_block_or_an_expression_as_a_generic_argument() {
    let text = "fn f(io: &Io, x: T) {\n    game_string::<{ 1 }>(io, 0x0012_0010);\n    let n = g::<{ N + 1 }>();\n    let m = x.h::<{ 2 }>();\n}\n";
    assert_eq!(
        found(&[("x/y.rs", text)], &[0x0012_0010], &[]),
        [
            "x/y.rs:2 a generic argument: a form the scan rejects",
            "x/y.rs:3 a generic argument: a form the scan rejects",
            "x/y.rs:4 a generic argument: a form the scan rejects",
        ],
        "each callee"
    );
    let unbraced = "fn f() {\n    let n = g::<N + 1>();\n}\n";
    assert_eq!(
        found(&[("x/y.rs", unbraced)], &[], &[]),
        ["x/y.rs:2: does not parse"],
        "an unbraced expression"
    );
    let literal = "fn f(io: &Io) {\n    let w = words::<8>(io);\n    game_string::<u32>(io, 0x0012_0010);\n}\n";
    assert!(
        found(&[("x/y.rs", literal)], &[0x0012_0010], &[]).is_empty(),
        "a literal and a type, listed"
    );
}

#[test]
fn r5_reject_a_use_renamed_to_a_modules_name() {
    let text = "mod a {\n    pub const S_X: u32 = 0x0012_0000;\n}\nmod c {\n    pub const S_X: u32 = 0x0012_0010;\n}\nfn f(io: &Io) {\n    use self::c as a;\n    game_string(io, a::S_X);\n}\n";
    let f = found(&[("x/y.rs", text)], &[0x0012_0000], &[]);
    assert!(
        f.contains(&"x/y.rs:8 use as a: a form the scan rejects".to_string()),
        "the rename: {f:?}"
    );
    let other = "mod a {\n    pub const S_X: u32 = 0x0012_0000;\n}\nmod c {\n    pub const S_X: u32 = 0x0012_0010;\n}\nfn f(io: &Io) {\n    use self::c as d;\n    game_string(io, d::S_X);\n}\n";
    assert_eq!(
        found(&[("x/y.rs", other)], &[0x0012_0000], &[]),
        ["x/y.rs:9 game_string(d::S_X) -> 0x120010: not catalogued"],
        "a rename to no module's name: c's"
    );
    assert!(
        found(&[("x/y.rs", other)], &[0x0012_0010], &[]).is_empty(),
        "listed"
    );
}

#[test]
fn r5_literal_an_image_address_literal_naming_a_string_is_found() {
    let text = "fn f(img: &GameImage, io: &Io) {\n    let b = img.bytes(0x0012_0014, 4);\n    game_string(io, 0x0012_0010);\n}\nconst W: u32 = 0x0012_0010;\n";
    let parts: [(u32, &[u8]); 1] = [(0x0012_0010, b"Zc1 word")];
    assert_eq!(
        game_findings(&[("x/y.rs", text)], &parts, &[0x0012_0010], &[], &[]),
        ["x/y.rs:2 0x120014: a literal naming a string no list names"],
        "the accessor's literal (the site's and the const's checked by their own passes)"
    );
    let listed = [0x0012_0010, 0x0012_0014];
    assert!(
        game_findings(&[("x/y.rs", text)], &parts, &listed, &[], &[]).is_empty(),
        "listed"
    );
    let sources = [Source::new("x/y.rs", text)];
    let game = super::strings::test_game(&parts);
    let img = crate::image::GameImage::parse(&game).expect("the test's game reads");
    let catalogue = super::strings::Catalogue::from_game(&game);
    let scan = Scan::new(&sources, false);
    let set = BTreeSet::from([0x0012_0010]);
    let reasons = [("x/y.rs", 0x0012_0014, Why::Logic("test"))];
    let pass = const_pass(&scan, &img, &catalogue, &set, &[], &[], &reasons);
    assert_eq!(
        (pass.0.len(), pass.3),
        (0, 1),
        "with its reason: {:?}",
        pass.0
    );
    assert!(
        literal_reason_findings(&scan, &set, &reasons).is_empty(),
        "a needed reason"
    );
    let wrong = [
        ("x/y.rs", 0x0012_0018, Why::Logic("test")),
        ("x/y.rs", 0x0012_0010, Why::Logic("")),
    ];
    assert_eq!(
        literal_reason_findings(&scan, &set, &wrong),
        [
            "x/y.rs 0x120018: a literal reason no literal matches",
            "x/y.rs 0x120010: a literal reason no literal matches",
            "x/y.rs 0x120010: its reason gives no text",
            "x/y.rs 0x120010: covered, its reason not needed",
        ],
        "a reason of no literal, of no text, of a listed address"
    );
}

#[test]
fn r5_forms_a_form_entry_names_files_not_a_directory() {
    let dirs = OTHER_FORMS
        .iter()
        .filter(|e| e.0.iter().any(|f| f.ends_with('/')))
        .count();
    assert_eq!(dirs, 0, "entries naming a directory");
    let text = "fn f(s: &mut State) {\n    queue_at(s, 0x0012_0010);\n}\n";
    let sources = [
        Source::new("x/y.rs", text),
        Source::new("x/w.rs", text),
        Source::new("x/z.rs", "fn g() {}\n"),
    ];
    let scan = Scan::new(&sources, false);
    let files = |e: &[FormEntry]| -> Vec<String> {
        findings(&scan, &scan.sites(), &BTreeSet::new(), &[], e)
            .into_iter()
            .filter(|f| !f.starts_with("reader "))
            .collect()
    };
    assert_eq!(
        files(&[(&["x/y.rs", "x/z.rs"], "queue_at", 2, false, "test")]),
        [
            "x/w.rs:2 queue_at(s, 0x0012_0010): a form no reader takes",
            "x/z.rs queue_at of 2 arguments: a form entry no call matches",
        ],
        "a file the entry does not list is scanned, and a file it lists with no call is found"
    );
    assert!(
        files(&[(&["x/w.rs", "x/y.rs"], "queue_at", 2, false, "test")]).is_empty(),
        "each file listed, each with its call"
    );
}

/// The owner's zip.
const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";

/// The owner's `game`: `None` (skipped) without the zip; a zip whose intake gives no `game` fails.
pub(super) fn owner_game() -> Option<Vec<u8>> {
    use u6_testkit::Quiet;
    if !Path::new(ZIP).exists() {
        eprintln!("skipped: {ZIP} not present");
        return None;
    }
    let data = std::fs::read(ZIP).quiet();
    let r = u6_intake::intake(vec![u6_intake::InputFile {
        name: "ultima-vi.zip".into(),
        data,
    }]);
    let game = crate::files::Files::new(r.files)
        .get("game")
        .map(<[u8]>::to_vec);
    assert!(game.is_some(), "the owner's zip gives a game");
    game
}

/// Real data: the scan's entries hold on the owner's game at every site an entry matches: a site
/// of no word reads no word (or under 3 bytes), a site whose bytes are a listed string's is
/// catalogued by them. Prints counts and pass/fail only, a failure an address, never a text.
#[test]
fn the_scans_reasons_hold_on_the_owners_game() {
    use super::strings::{Catalogue, MIN_BYTES, has_words};
    use u6_testkit::Quiet;
    let Some(game) = owner_game() else { return };
    let img = crate::image::GameImage::parse(&game).quiet();
    let c = Catalogue::from_game(&game);
    let sources = sources();
    let builds = [Scan::new(&sources, false), Scan::new(&sources, true)];
    let sites: Vec<Site> = builds.iter().flat_map(Scan::sites).collect();
    let (mut no_word, mut same) = (0, 0);
    for (file, callee, arg, why) in NOT_CATALOGUED {
        let matched = sites
            .iter()
            .filter(|s| s.file == *file && s.callee == *callee && s.arg == *arg);
        for s in matched {
            let Kind::Arg(Some(at)) = &s.kind else {
                continue;
            };
            for &a in at {
                let text = img.cstr(a).quiet();
                match why {
                    Why::NoWord => {
                        assert!(
                            text.len() < MIN_BYTES || !has_words(&text),
                            "{file}:{} {callee}({arg}) {a:#x} reads no word",
                            s.line
                        );
                        no_word += 1;
                    }
                    Why::Same(_) => {
                        assert!(
                            c.of(&text).is_some(),
                            "{file}:{} {callee}({arg}) {a:#x} is catalogued by its bytes",
                            s.line
                        );
                        same += 1;
                    }
                    _ => {}
                }
            }
        }
    }
    assert!(
        no_word > 0 && same > 0,
        "each kind checked: {no_word} of no word, {same} of a listed string's bytes"
    );
    eprintln!(
        "the scan's {no_word} sites of entries of no word and {same} of a listed string's bytes hold: pass"
    );
}

/// Real data (no names): every const of the sources, of any integer type, whose value names a
/// byte inside a complete string of the owner's executable is listed at it, named in an entry,
/// or has its reason, and so is every image-address literal outside consts and sites
/// ([`const_pass`]). Prints counts and pass/fail only.
#[test]
fn every_const_naming_a_string_of_the_owners_game_is_listed_named_or_reasoned() {
    use u6_testkit::Quiet;
    let Some(game) = owner_game() else { return };
    let img = crate::image::GameImage::parse(&game).quiet();
    let catalogue = super::strings::Catalogue::from_game(&game);
    let sources = sources();
    let listed = listed();
    let builds = [Scan::new(&sources, false), Scan::new(&sources, true)];
    let passes = builds.each_ref().map(|scan| {
        const_pass(
            scan,
            &img,
            &catalogue,
            &listed,
            NOT_CATALOGUED,
            CONST_REASONS,
            LITERAL_REASONS,
        )
    });
    let found = merged(passes.clone().map(|p| p.0));
    assert!(
        found.is_empty(),
        "{} consts:\n{}",
        found.len(),
        found.join("\n")
    );
    let (naming, uneval, literals) = (
        passes.each_ref().map(|p| p.1),
        passes.each_ref().map(|p| p.2),
        passes.each_ref().map(|p| p.3),
    );
    assert!(
        naming.iter().chain(&literals).all(|&n| n > 0),
        "the pass reads strings: {naming:?} consts, {literals:?} literals"
    );
    eprintln!(
        "{} consts, {naming:?} naming a string of the game, {uneval:?} not evaluated, {literals:?} literals naming one (without wide, with): each listed, named or with its reason: pass",
        builds[0].consts().count(),
    );
}
