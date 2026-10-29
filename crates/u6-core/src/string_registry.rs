//! Registered text sources of the game executable (spec 16 B13).
//! Runtime reads and locale traversal use these same records. A Text cannot be constructed
//! with unregistered storage; fixed callers construct it in const contexts. Binary readers,
//! conversation bytecode, and the separate u/end executables are outside this address space.

use crate::image::{A4, GameImage};
use crate::objects::commands::GameData;
use u6_formats::FormatError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Budget {
    None,
    English,
    Fixed(u16),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Use {
    Printed,
    Label,
}
#[derive(Clone, Copy)]
pub(crate) struct Group {
    pub addresses: &'static [u32],
    pub pointed: bool,
    pub usage: Use,
    pub budget: Budget,
}
const fn group(addresses: &'static [u32], pointed: bool, usage: Use, budget: Budget) -> Group {
    Group {
        addresses,
        pointed,
        usage,
        budget,
    }
}
pub(crate) const GROUPS: &[Group] = &[
    group(
        crate::main_loop::commands::PRINTED,
        false,
        Use::Printed,
        Budget::None,
    ),
    group(
        crate::objects::commands::PRINTED[0],
        false,
        Use::Printed,
        Budget::None,
    ),
    group(
        crate::objects::commands::PRINTED[1],
        false,
        Use::Printed,
        Budget::None,
    ),
    group(
        crate::objects::commands::PRINTED[2],
        false,
        Use::Printed,
        Budget::None,
    ),
    group(
        crate::objects::commands::PRINTED[3],
        false,
        Use::Printed,
        Budget::None,
    ),
    group(
        crate::objects::commands::PRINTED[4],
        false,
        Use::Printed,
        Budget::None,
    ),
    group(
        crate::objects::commands::PRINTED[5],
        false,
        Use::Printed,
        Budget::None,
    ),
    group(
        crate::objects::commands::PRINTED[6],
        false,
        Use::Printed,
        Budget::None,
    ),
    group(
        crate::objects::commands::PRINTED[7],
        false,
        Use::Printed,
        Budget::None,
    ),
    group(
        crate::objects::commands::PRINTED[8],
        false,
        Use::Printed,
        Budget::None,
    ),
    group(crate::usecmd::PRINTED[0], false, Use::Printed, Budget::None),
    group(crate::usecmd::PRINTED[1], false, Use::Printed, Budget::None),
    group(crate::usecmd::PRINTED[2], false, Use::Printed, Budget::None),
    group(crate::usecmd::PRINTED[3], false, Use::Printed, Budget::None),
    group(crate::usecmd::PRINTED[4], false, Use::Printed, Budget::None),
    group(
        crate::actors::rest::PRINTED,
        false,
        Use::Printed,
        Budget::None,
    ),
    group(
        crate::combat::attack::PRINTED,
        false,
        Use::Printed,
        Budget::None,
    ),
    group(
        crate::combat::damage::PRINTED,
        false,
        Use::Printed,
        Budget::None,
    ),
    group(crate::inventory::PRINTED, false, Use::Printed, Budget::None),
    group(crate::ui::click::PRINTED, false, Use::Printed, Budget::None),
    group(crate::magic::PRINTED[0], false, Use::Printed, Budget::None),
    group(crate::magic::PRINTED[1], false, Use::Printed, Budget::None),
    group(crate::magic::PRINTED[2], false, Use::Printed, Budget::None),
    group(crate::magic::PRINTED[3], false, Use::Printed, Budget::None),
    group(crate::magic::PRINTED[4], false, Use::Printed, Budget::None),
    group(crate::magic::PRINTED[5], false, Use::Printed, Budget::None),
    group(crate::magic::PRINTED[6], false, Use::Printed, Budget::None),
    group(crate::magic::PRINTED[7], false, Use::Printed, Budget::None),
    group(crate::magic::PRINTED[8], false, Use::Printed, Budget::None),
    group(crate::magic::PRINTED[9], false, Use::Printed, Budget::None),
    group(
        crate::actors::clock::PRINTED,
        false,
        Use::Printed,
        Budget::None,
    ),
    group(
        crate::actors::turn::PRINTED,
        false,
        Use::Printed,
        Budget::None,
    ),
    group(
        crate::actors::party::PRINTED,
        false,
        Use::Printed,
        Budget::None,
    ),
    group(
        crate::actors::npc::PRINTED,
        false,
        Use::Printed,
        Budget::None,
    ),
    group(
        crate::actors::walk::PRINTED,
        false,
        Use::Printed,
        Budget::None,
    ),
    group(
        crate::objects::PRINTED[0],
        false,
        Use::Printed,
        Budget::None,
    ),
    group(
        crate::objects::PRINTED[1],
        false,
        Use::Printed,
        Budget::None,
    ),
    group(
        crate::ui::tables::PRINTED,
        false,
        Use::Printed,
        Budget::English,
    ),
    group(
        crate::magic::PAGE_PRINTED,
        false,
        Use::Printed,
        Budget::English,
    ),
    group(
        crate::map::seams::PRINTED,
        false,
        Use::Printed,
        Budget::None,
    ),
    group(
        crate::inventory::PANEL_PRINTED,
        false,
        Use::Printed,
        Budget::English,
    ),
    group(
        crate::locale::strings::NAME_PRINTED,
        false,
        Use::Printed,
        Budget::None,
    ),
    group(
        &crate::map::seams::PLACE_NAMES,
        true,
        Use::Printed,
        Budget::None,
    ),
    group(
        crate::main_loop::commands::LABELS,
        true,
        Use::Label,
        Budget::None,
    ),
    group(
        &crate::locale::strings::SHARED_MESSAGES,
        true,
        Use::Label,
        Budget::None,
    ),
    group(
        crate::objects::commands::LABELS,
        true,
        Use::Label,
        Budget::None,
    ),
    group(crate::inventory::LABELS, true, Use::Label, Budget::None),
    group(crate::magic::LABELS, true, Use::Label, Budget::None),
    group(crate::actors::clock::LABELS, true, Use::Label, Budget::None),
    group(
        crate::ui::tables::LABELS,
        true,
        Use::Label,
        Budget::Fixed(crate::ui::tables::MODE_CELLS),
    ),
    group(
        &crate::magic::NAME_LABELS,
        true,
        Use::Label,
        Budget::Fixed(crate::magic::NAME_CELLS),
    ),
    group(
        &crate::magic::CIRCLE_LABELS,
        true,
        Use::Label,
        Budget::Fixed(crate::magic::CIRCLE_CELLS),
    ),
    group(
        crate::ui::tables::SHOWN_AT,
        false,
        Use::Label,
        Budget::English,
    ),
    group(
        crate::actors::clock::SHOWN_AT,
        false,
        Use::Label,
        Budget::None,
    ),
    group(
        crate::combat::attack::SHOWN_AT,
        false,
        Use::Label,
        Budget::None,
    ),
];

/// An exclusion is still a registration: it states why these bytes are not translated.
struct Exclusion {
    start: u32,
    count: u32,
    pointed: bool,
    english: bool,
    reason: &'static str,
    first: usize,
}
const fn excluded(
    start: u32,
    count: u32,
    pointed: bool,
    english: bool,
    reason: &'static str,
) -> Exclusion {
    Exclusion {
        start,
        count,
        pointed,
        english,
        reason,
        first: 0,
    }
}
const EXCLUDED: &[Exclusion] = &[
    excluded(
        0x0012_83e2,
        1,
        false,
        true,
        "object-name article; Korean omits it",
    ),
    excluded(
        0x0012_83e6,
        1,
        false,
        true,
        "object-name article; Korean omits it",
    ),
    excluded(
        0x0012_83ea,
        1,
        false,
        true,
        "object-name article; Korean omits it",
    ),
    excluded(
        0x0010_a4c6,
        1,
        false,
        false,
        "empty look entry; catalogued by the same bytes elsewhere",
    ),
    excluded(
        0x0012_83fe,
        1,
        false,
        false,
        "quantity format; no word of its own",
    ),
    excluded(0x0012_83f4, 1, false, false, "books file name; logic only"),
    excluded(
        0x0010_4ffe,
        1,
        false,
        false,
        "keyword delimiters; logic only",
    ),
    excluded(
        0x0011_0ba4,
        1,
        false,
        false,
        "panel field with no word of its own",
    ),
    excluded(
        0x0011_0bae,
        1,
        false,
        false,
        "panel field with no word of its own",
    ),
    excluded(
        0x0011_0bd0,
        1,
        false,
        false,
        "panel field with no word of its own",
    ),
    excluded(
        0x0011_0be0,
        1,
        false,
        false,
        "panel field with no word of its own",
    ),
    excluded(
        0x0011_0be4,
        1,
        false,
        false,
        "panel field with no word of its own",
    ),
    excluded(
        0x0011_0bf4,
        1,
        false,
        false,
        "panel field with no word of its own",
    ),
    excluded(A4 - 0x61de, 1, true, false, "spellbook bitmap file name"),
    excluded(A4 - 0x61e2, 1, true, false, "paper bitmap file name"),
    excluded(A4 - 0x7796, 13, true, false, "song file names"),
    excluded(
        A4 - 0x71b4,
        128,
        true,
        false,
        "spell incantation letters; logic only",
    ),
    Exclusion {
        first: 0x41,
        ..excluded(
            A4 - 0x70b8 + 4 * 0x41,
            26,
            true,
            true,
            "incantation syllables intentionally stay English",
        )
    },
    excluded(
        A4 - 0x6c7c,
        4,
        true,
        false,
        "spell alignment names; logic only",
    ),
    excluded(
        A4 - 0x65ce,
        20,
        true,
        false,
        "wish words compared with input",
    ),
    excluded(A4 - 0x660e, 8, true, false, "mantras compared with input"),
];

/// A closed registered source. Fields are private; the catalogue and runtime share its registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Text {
    address: u32,
    pointed: bool,
}

const fn contains(values: &[u32], address: u32) -> bool {
    let mut i = 0;
    while i < values.len() {
        if values[i] == address {
            return true;
        }
        i += 1;
    }
    false
}
const fn registered(address: u32, pointed: bool) -> bool {
    let mut i = 0;
    while i < GROUPS.len() {
        let g = &GROUPS[i];
        if g.pointed == pointed && contains(g.addresses, address) {
            return true;
        }
        i += 1;
    }
    i = 0;
    while i < EXCLUDED.len() {
        let e = &EXCLUDED[i];
        assert!(!e.reason.is_empty(), "every exclusion needs a reason");
        let stride = if e.pointed { 4 } else { 1 };
        if e.pointed == pointed
            && address >= e.start
            && address - e.start < e.count * stride
            && (address - e.start).is_multiple_of(stride)
        {
            return true;
        }
        i += 1;
    }
    false
}
/// The port's conventional spelling: absolute image address, A4 minus a positive offset,
/// or A4 plus a negative offset. Conversion happens before creating the checked handle.
const fn address(value: i64) -> u32 {
    if value < 0 {
        A4.wrapping_add_signed(value as i32)
    } else if value < 0x0010_0000 {
        A4.wrapping_sub(value as u32)
    } else {
        value as u32
    }
}
/// Construct in a const context at a fixed site; an unknown address fails compilation.
pub const fn fixed(value: i64) -> Text {
    let address = address(value);
    assert!(
        registered(address, false),
        "unregistered fixed game text: add its catalogue disposition/reason to string_registry"
    );
    Text {
        address,
        pointed: false,
    }
}
/// A pointer slot must be an exact registered slot, including bounds/alignment of excluded tables.
pub const fn pointed(value: i64) -> Text {
    let address = address(value);
    assert!(
        registered(address, true),
        "unregistered game text pointer: use a registered table slot"
    );
    Text {
        address,
        pointed: true,
    }
}
impl Text {
    #[cfg(test)]
    pub(crate) const fn is_pointed(self) -> bool {
        self.pointed
    }

    /// Original data address for content-free tests and binary layout references.
    pub const fn address(self) -> u32 {
        self.address
    }
    #[allow(clippy::disallowed_methods)] // The sole game-image cstr boundary; handle already registered.
    pub(crate) fn image(self, image: &GameImage<'_>) -> Result<Vec<u8>, FormatError> {
        let address = if self.pointed {
            image.pointer(self.address)?
        } else {
            self.address
        };
        image.cstr(address)
    }
    #[allow(clippy::disallowed_methods)] // The same registered source in the loaded, relocated game image.
    pub(crate) fn data(self, data: &GameData) -> Vec<u8> {
        let address = if self.pointed {
            data.long(self.address)
        } else {
            self.address
        };
        data.cstr(address)
    }
    /// The wand's zero-terminated binary list keeps its original 64-byte, zero-filled bound.
    pub(crate) fn byte_list(self, io: &crate::engine::Io) -> Vec<u8> {
        let Some(image) = io.game_image() else {
            return Vec::new();
        };
        let Ok(address) = image.pointer(self.address) else {
            return Vec::new();
        };
        image
            .read_zero_filled(address, 64)
            .unwrap_or_default()
            .into_iter()
            .take_while(|&b| b != 0)
            .collect()
    }
    pub(crate) fn io(self, io: &crate::engine::Io) -> Vec<u8> {
        io.game_image()
            .and_then(|image| self.image(&image).ok())
            .unwrap_or_default()
    }
}
/// English-only exclusions, for the catalogue's unchanged uncatalogued counter.
pub(crate) fn english() -> impl Iterator<Item = Text> {
    EXCLUDED.iter().filter(|e| e.english).flat_map(|e| {
        (0..e.count).map(move |i| Text {
            address: e.start + i * if e.pointed { 4 } else { 1 },
            pointed: e.pointed,
        })
    })
}
/// Ordered catalogue records; this is the same registration that validates runtime handles.
pub(crate) fn entries() -> impl Iterator<Item = (Text, Use, Budget)> {
    GROUPS.iter().flat_map(|g| {
        g.addresses.iter().map(move |&address| {
            (
                Text {
                    address,
                    pointed: g.pointed,
                },
                g.usage,
                g.budget,
            )
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tables_use_exact_registered_slots_and_refuse_outside_indices() {
        let tables = TABLES.iter().copied().chain(
            EXCLUDED
                .iter()
                .enumerate()
                .filter(|(_, e)| e.pointed)
                .map(|(i, e)| Table {
                    slots: Slots::Excluded(i),
                    first: e.first,
                }),
        );
        for table in tables {
            assert!(table.len() > 0);
            assert!(table.get(usize::MAX).is_none());
            assert!(table.get(table.first + table.len()).is_none());
            if table.first > 0 {
                assert!(table.get(table.first - 1).is_none());
            }
            for offset in 0..table.len() {
                let source = table.get(table.first + offset).unwrap();
                assert!(registered(source.address, true));
                assert_eq!(source.address, table.slot(offset));
            }
        }
    }
    #[test]
    fn conversation_records_supply_every_runtime_message_and_catalogue_role() {
        let mut order = std::collections::BTreeSet::new();
        let mut messages = std::collections::BTreeSet::new();
        for entry in CONVERSATIONS {
            assert!(
                messages.insert(entry.message),
                "one disposition per message"
            );
            match entry.disposition {
                ConversationUse::Shown { order: index, how } => {
                    assert!(order.insert(index), "unique catalogue position");
                    assert_eq!(CONVERSATION_SHOWN[index], (entry.message, how));
                }
                ConversationUse::Excluded(reason) => assert!(!reason.is_empty()),
            }
        }
        assert_eq!(
            order.into_iter().collect::<Vec<_>>(),
            (0..CONVERSATION_SHOWN.len()).collect::<Vec<_>>()
        );
        assert_eq!(messages.len(), CONVERSATION_MESSAGES.len());
    }
    #[test]
    fn every_exclusion_has_a_reason_and_is_bounded() {
        assert_eq!(
            fixture(1, false).address(),
            1,
            "synthetic queue IDs stay literal"
        );
        assert_eq!(
            fixture(0, false).address(),
            0,
            "argument-format sentinel stays zero"
        );
        assert_eq!(fixture(-0x7000, false).address(), A4 - 0x7000);
        for e in EXCLUDED {
            assert!(!e.reason.is_empty() && e.count > 0);
            assert!(registered(e.start, e.pointed));
            let stride = if e.pointed { 4 } else { 1 };
            assert!(registered(e.start + (e.count - 1) * stride, e.pointed));
        }
        assert!(!registered(0x00ff_ffff, false));
        assert!(!registered(A4 - 0x70b8 + 4 * 0x41 + 1, true));
        assert!(std::panic::catch_unwind(|| fixed(0x00ff_ffff)).is_err());
        assert!(std::panic::catch_unwind(|| pointed(0x00ff_ffff)).is_err());
    }
}

/// The exact registered slots, never a guessed range. Excluded tables share their bounds
/// with EXCLUDED; translated tables use the same address lists as the catalogue above.
#[derive(Clone, Copy)]
enum Slots {
    Listed(&'static [u32]),
    Excluded(usize),
}
#[derive(Clone, Copy)]
pub struct Table {
    slots: Slots,
    first: usize,
}
const fn section<const N: usize>(addresses: &[u32], from: usize) -> [u32; N] {
    let mut out = [0; N];
    let mut i = 0;
    while i < N {
        out[i] = addresses[from + i];
        i += 1;
    }
    out
}
const TABLES: &[Table] = &[
    Table {
        slots: Slots::Listed(&section::<13>(crate::magic::LABELS, 0)),
        first: 0,
    },
    Table {
        slots: Slots::Listed(&section::<8>(crate::magic::LABELS, 13)),
        first: 0,
    },
    Table {
        slots: Slots::Listed(&section::<8>(crate::magic::LABELS, 21)),
        first: 0,
    },
    Table {
        slots: Slots::Listed(&crate::magic::NAME_LABELS),
        first: 0,
    },
    Table {
        slots: Slots::Listed(&crate::magic::CIRCLE_LABELS),
        first: 0,
    },
    Table {
        slots: Slots::Listed(crate::objects::commands::LABELS),
        first: 0,
    },
    Table {
        slots: Slots::Listed(&crate::map::seams::PLACE_NAMES),
        first: 1,
    },
    Table {
        slots: Slots::Listed(crate::actors::clock::LABELS),
        first: 0,
    },
    Table {
        slots: Slots::Listed(crate::ui::tables::LABELS),
        first: 2,
    },
    Table {
        slots: Slots::Listed(crate::main_loop::commands::LABELS),
        first: 0x82,
    },
];
/// A known table's base selects its registered descriptor. The table owns the exact slot list.
pub const fn table(value: i64) -> Table {
    let base = address(value);
    let mut i = 0;
    while i < TABLES.len() {
        let t = TABLES[i];
        if t.slot(0) - 4 * t.first as u32 == base {
            return t;
        }
        i += 1;
    }
    i = 0;
    while i < EXCLUDED.len() {
        let e = &EXCLUDED[i];
        if e.pointed && e.start - 4 * e.first as u32 == base {
            return Table {
                slots: Slots::Excluded(i),
                first: e.first,
            };
        }
        i += 1;
    }
    panic!("unregistered game text table")
}
impl Table {
    const fn len(self) -> usize {
        match self.slots {
            Slots::Listed(a) => a.len(),
            Slots::Excluded(i) => EXCLUDED[i].count as usize,
        }
    }
    const fn slot(self, offset: usize) -> u32 {
        match self.slots {
            Slots::Listed(a) => a[offset],
            Slots::Excluded(i) => EXCLUDED[i].start + 4 * offset as u32,
        }
    }
    pub const fn get(self, index: usize) -> Option<Text> {
        if index < self.first || index - self.first >= self.len() {
            return None;
        }
        Some(Text {
            address: self.slot(index - self.first),
            pointed: true,
        })
    }
    pub(crate) fn image(self, image: &GameImage<'_>) -> Result<Vec<Vec<u8>>, FormatError> {
        (self.first..self.first + self.len())
            .map(|i| self.get(i).expect("registered table slot").image(image))
            .collect()
    }
}

/// Actor printing's existing sentinel: the first string argument is the format.
pub(crate) const ARGUMENT: Text = Text {
    address: 0,
    pointed: false,
};
impl Text {
    pub(crate) const fn is_argument(self) -> bool {
        self.address == 0
    }
    pub(crate) fn latin1<E>(
        self,
        read: impl Fn(u32, usize) -> Result<Vec<u8>, E>,
    ) -> Result<String, E> {
        assert!(
            !self.pointed,
            "object definitions read a fixed registered string"
        );
        let mut out = String::new();
        for i in 0..256 {
            match read(self.address + i, 1)?[0] {
                0 => break,
                b => out.push(char::from(b)),
            }
        }
        Ok(out)
    }
}

/// Select one of a site's closed, const-checked alternatives. The numeric selector never
/// becomes a source: only a handle in `choices` can be returned.
pub(crate) fn choose(value: i64, choices: &[Text]) -> Text {
    let address = address(value);
    choices
        .iter()
        .copied()
        .find(|text| text.address == address)
        .expect("text choice outside its registered alternatives")
}

/// Fixed-site constructors perform registration checks during compilation.
macro_rules! text {
    ($at:literal) => {
        const { $crate::string_registry::fixed($at) }
    };
    ($at:expr) => {
        const { $crate::string_registry::fixed(($at) as i64) }
    };
}
macro_rules! pointer {
    ($at:literal) => {
        const { $crate::string_registry::pointed($at) }
    };
    ($at:expr) => {
        const { $crate::string_registry::pointed(($at) as i64) }
    };
}
macro_rules! text_table {
    ($at:literal) => {
        const { $crate::string_registry::table($at) }
    };
    ($at:expr) => {
        const { $crate::string_registry::table(($at) as i64) }
    };
}
pub(crate) use {pointer, text, text_table};

/// Independent test oracles and synthetic images may read their own fixture addresses.
/// Positive fixture values are literal addresses/IDs, not abbreviated production A4 offsets.
/// Negative fixture values name A4-relative test strings. Absent from production builds.
#[cfg(test)]
pub(crate) const fn fixture(value: i64, pointed: bool) -> Text {
    Text {
        address: if value < 0 {
            A4.wrapping_add_signed(value as i32)
        } else {
            value as u32
        },
        pointed,
    }
}

// Conversation messages use the same registry, with their original hunk-wide NUL bound.
// Order is explicit because catalogue order differs from the runtime table's enum order.
use crate::converse::text::{Msg, Shown};
struct Conversation {
    message: Msg,
    address: u32,
    disposition: ConversationUse,
}
enum ConversationUse {
    Shown { order: usize, how: Shown },
    Excluded(&'static str),
}
const fn conversation_record(
    message: Msg,
    address: u32,
    disposition: ConversationUse,
) -> Conversation {
    Conversation {
        message,
        address,
        disposition,
    }
}
const CONVERSATIONS: &[Conversation] = &[
    // [0x1125b8]
    conversation_record(
        Msg::Milady,
        0x0011_2758,
        ConversationUse::Shown {
            order: 0,
            how: Shown::Label,
        },
    ),
    // [0x1125b8]
    conversation_record(
        Msg::Milord,
        0x0011_2760,
        ConversationUse::Shown {
            order: 1,
            how: Shown::Label,
        },
    ),
    // [0x1125b8]
    conversation_record(
        Msg::Morning,
        0x0011_2790,
        ConversationUse::Shown {
            order: 2,
            how: Shown::Label,
        },
    ),
    // [0x1125b8]
    conversation_record(
        Msg::Afternoon,
        0x0011_2798,
        ConversationUse::Shown {
            order: 3,
            how: Shown::Label,
        },
    ),
    // [0x1125b8]
    conversation_record(
        Msg::Evening,
        0x0011_27a2,
        ConversationUse::Shown {
            order: 4,
            how: Shown::Label,
        },
    ),
    // [0x1127c4]
    conversation_record(
        Msg::Shrine,
        0x0011_27aa,
        ConversationUse::Shown {
            order: 6,
            how: Shown::Printed,
        },
    ),
    // [0x1127c4]
    conversation_record(
        Msg::Statue,
        0x0011_27b2,
        ConversationUse::Shown {
            order: 7,
            how: Shown::Printed,
        },
    ),
    // [0x1127c4]
    conversation_record(
        Msg::Nothing,
        0x0011_27ba,
        ConversationUse::Shown {
            order: 8,
            how: Shown::Printed,
        },
    ),
    // [0x1128d0] strtok delimiters
    conversation_record(
        Msg::Delimiters,
        0x0011_2982,
        ConversationUse::Excluded("token delimiters; logic only"),
    ),
    // [0x112b20]
    conversation_record(
        Msg::Undefined,
        0x0011_2984,
        ConversationUse::Shown {
            order: 9,
            how: Shown::Printed,
        },
    ),
    // [0x1130a2] op 0xc0
    conversation_record(
        Msg::SelectItem,
        0x0011_2992,
        ConversationUse::Shown {
            order: 10,
            how: Shown::Printed,
        },
    ),
    // [0x1130a2] op 0x93
    conversation_record(
        Msg::DivideError,
        0x0011_29a2,
        ConversationUse::Shown {
            order: 11,
            how: Shown::Printed,
        },
    ),
    // [0x1130a2] stack left over
    conversation_record(
        Msg::ErrorOccured,
        0x0011_29be,
        ConversationUse::Shown {
            order: 12,
            how: Shown::Printed,
        },
    ),
    // [0x11404c] op 0xdf
    conversation_record(
        Msg::You,
        0x0011_29ce,
        ConversationUse::Shown {
            order: 5,
            how: Shown::Label,
        },
    ),
    // [0x11404c]
    conversation_record(
        Msg::UnknownCommand,
        0x0011_29d2,
        ConversationUse::Shown {
            order: 13,
            how: Shown::Printed,
        },
    ),
    // [0x1148f0]
    conversation_record(
        Msg::ControlChar,
        0x0011_29e4,
        ConversationUse::Shown {
            order: 14,
            how: Shown::Printed,
        },
    ),
    // [0x1149d6] empty input
    conversation_record(
        Msg::Bye,
        0x0011_2a06,
        ConversationUse::Excluded("empty-input command; logic only"),
    ),
    // [0x1149d6] [0x114afa]
    conversation_record(
        Msg::Newline,
        0x0011_2a0a,
        ConversationUse::Excluded("line break; no word of its own"),
    ),
    // [0x114afa]
    conversation_record(
        Msg::NotOnScreen,
        0x0011_2a0c,
        ConversationUse::Shown {
            order: 15,
            how: Shown::Printed,
        },
    ),
    // [0x114afa]
    conversation_record(
        Msg::NotInSolo,
        0x0011_2a1e,
        ConversationUse::Shown {
            order: 16,
            how: Shown::Printed,
        },
    ),
    // [0x114afa]
    conversation_record(
        Msg::DeepMoan,
        0x0011_2a32,
        ConversationUse::Shown {
            order: 17,
            how: Shown::Printed,
        },
    ),
    // [0x114afa]
    conversation_record(
        Msg::NoResponseNpc,
        0x0011_2a4a,
        ConversationUse::Shown {
            order: 18,
            how: Shown::Printed,
        },
    ),
    // [0x114afa]
    conversation_record(
        Msg::NoResponse,
        0x0011_2a5a,
        ConversationUse::Shown {
            order: 19,
            how: Shown::Printed,
        },
    ),
    // [0x114afa]
    conversation_record(
        Msg::TalkingToYourself,
        0x0011_2a68,
        ConversationUse::Shown {
            order: 20,
            how: Shown::Printed,
        },
    ),
    // [0x114afa]
    conversation_record(
        Msg::FunnyNoResponse,
        0x0011_2a80,
        ConversationUse::Shown {
            order: 21,
            how: Shown::Printed,
        },
    ),
    // [0x114afa]
    conversation_record(
        Msg::YouSeeFirst,
        0x0011_2a96,
        ConversationUse::Shown {
            order: 22,
            how: Shown::Printed,
        },
    ),
    // [0x114afa]
    conversation_record(
        Msg::YouSay,
        0x0011_2aa2,
        ConversationUse::Shown {
            order: 23,
            how: Shown::Printed,
        },
    ),
    // [0x114afa]
    conversation_record(
        Msg::TwoNewlines,
        0x0011_2aac,
        ConversationUse::Excluded("line breaks; no word of their own"),
    ),
    // [0x114afa]
    conversation_record(
        Msg::FormatNumber,
        0x0011_2ab0,
        ConversationUse::Excluded("numeric format; no word of its own"),
    ),
    // [0x114afa]
    conversation_record(
        Msg::FormatString,
        0x0011_2ab6,
        ConversationUse::Excluded("string format; no word of its own"),
    ),
    // [0x114afa]
    conversation_record(
        Msg::FormatChar,
        0x0011_2abc,
        ConversationUse::Excluded("character format; no word of its own"),
    ),
    // [0x114afa]
    conversation_record(
        Msg::KeyCommandExpected,
        0x0011_2ac0,
        ConversationUse::Shown {
            order: 24,
            how: Shown::Printed,
        },
    ),
    // [0x114afa]
    conversation_record(
        Msg::LogicalError,
        0x0011_2ae2,
        ConversationUse::Shown {
            order: 25,
            how: Shown::Printed,
        },
    ),
    // [0x114afa]
    conversation_record(
        Msg::Look,
        0x0011_2afa,
        ConversationUse::Excluded("input keyword; logic only"),
    ),
    // [0x114afa]
    conversation_record(
        Msg::YouSee,
        0x0011_2b00,
        ConversationUse::Shown {
            order: 26,
            how: Shown::Printed,
        },
    ),
    // [0x133e5e] A4-0x63ac
    conversation_record(
        Msg::ConverseFile,
        0x0015_5344,
        ConversationUse::Excluded("conversation file name"),
    ),
    // [0x133d3e] A4-0x63b7
    conversation_record(
        Msg::PortraitFile,
        0x0015_5339,
        ConversationUse::Excluded("portrait file name"),
    ),
    // [0x108376] bytes that end an `@` keyword
    conversation_record(
        Msg::WordEnds,
        0x0010_4ffe,
        ConversationUse::Excluded("keyword delimiters; logic only"),
    ),
];
const fn conversation_messages() -> [(Msg, u32); 38] {
    let mut out = [(Msg::Milady, 0); 38];
    let mut i = 0;
    while i < CONVERSATIONS.len() {
        out[i] = (CONVERSATIONS[i].message, CONVERSATIONS[i].address);
        i += 1;
    }
    out
}
const fn conversation_shown() -> [(Msg, Shown); 27] {
    let mut out = [(Msg::Milady, Shown::Printed); 27];
    let mut i = 0;
    while i < CONVERSATIONS.len() {
        let c = &CONVERSATIONS[i];
        match c.disposition {
            ConversationUse::Shown { order, how } => out[order] = (c.message, how),
            ConversationUse::Excluded(reason) => assert!(!reason.is_empty()),
        }
        i += 1;
    }
    out
}
/// The conversation runtime table, derived from registered dispositions.
pub const CONVERSATION_MESSAGES: [(Msg, u32); 38] = conversation_messages();
/// The catalogue's original order, derived from those same records.
pub const CONVERSATION_SHOWN: [(Msg, Shown); 27] = conversation_shown();

/// Hunk count of the `game` executable this table was measured on.
const GAME_HUNKS: usize = 35;
/// (hunk, image base) of the hunks the messages live in.
const HUNKS: [(usize, u32); 4] = [
    (2, 0x0010_4fb0),
    (9, 0x0011_25b8),
    (10, 0x0011_28d0),
    (33, 0x0015_36f0),
];

pub(crate) fn conversation(game: &[u8]) -> Result<Vec<(Msg, Vec<u8>)>, String> {
    let img = GameImage::parse(game).map_err(|e| format!("game: {e}"))?;
    if img.segment_count() != GAME_HUNKS {
        return Err(format!(
            "game: {} hunks, expected {GAME_HUNKS}",
            img.segment_count()
        ));
    }
    for &(n, base) in &HUNKS {
        if img.segment_base(n) != Some(base) {
            return Err(format!("game: hunk {n} is not at {base:#x}"));
        }
    }
    let mut pairs = Vec::with_capacity(CONVERSATION_MESSAGES.len());
    for (msg, addr) in CONVERSATION_MESSAGES {
        // The last known hunk that starts at or below the address holds it, if anywhere.
        let (n, base) = HUNKS
            .iter()
            .copied()
            .filter(|&(_, base)| addr >= base)
            .max_by_key(|&(_, base)| base)
            .ok_or_else(|| format!("{msg:?}: {addr:#x} outside the known hunks"))?;
        let hunk = img.loaded(n).unwrap_or_default();
        let bytes = hunk
            .get((addr - base) as usize..)
            .filter(|b| !b.is_empty())
            .ok_or_else(|| format!("{msg:?}: {addr:#x} past the loaded bytes of hunk {n}"))?;
        let end = bytes
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| format!("{msg:?}: no NUL inside hunk {n}"))?;
        pairs.push((msg, bytes[..end].to_vec()));
    }
    Ok(pairs)
}
