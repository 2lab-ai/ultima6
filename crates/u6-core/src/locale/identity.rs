//! Test-only (spec 08 Order 3 / Acceptance 5, slices K1 to K5a): the identity pack of the
//! owner's files, built at test time: every run and every table string of the catalog of his
//! conversation scripts, every executable string the engine catalogues from his `game`, every
//! object name of his `look.lzd` and book of his `book.dat`, and every string the engine
//! catalogues from his `u`, each translated as itself, and every keyword of those scripts an
//! alias can stand for given itself as its alias, declaring English grammar. Installed, it turns
//! `wide` on and must show exactly what English shows, every catalogued unit a hit.

use std::rc::Rc;

use u6_formats::converse::ConverseLib;

use super::strings::Catalogue;
use super::{
    MAX_ALIAS_CHARS, Pack, RunText, ScriptKey, StringText, TableText, fingerprint, program, texts,
};
use crate::converse::disasm::catalog::Catalog;
use crate::converse::{ConverseText, Msg};
use crate::files::Files;
use crate::ui::Language;

/// The identity pack of `files` (`None` without both archives or `game`), written as a pack file
/// and read back through the reader.
pub(crate) fn identity(files: &Files) -> Option<Rc<Pack>> {
    let mut scripts = Vec::new();
    for letter in [b'a', b'b'] {
        let lib =
            ConverseLib::parse(files.get(&format!("converse.{}", char::from(letter)))?).ok()?;
        for (item, s) in lib.scripts.into_iter().enumerate() {
            if let Some(s) = s {
                scripts.push((letter, item as u16, s.bytecode));
            }
        }
    }
    let catalog = Catalog::build(scripts.iter().map(|(l, i, b)| (*l, *i, b.as_slice())));
    let bytes = |t: &str| -> Vec<u8> { t.chars().map(|c| c as u8).collect() };
    let (mut runs, mut tables, mut named) = (Vec::new(), Vec::new(), Vec::new());
    // Slice K5a: each keyword as the keyword block keys it (up to its NUL, case-folded), its own
    // alias where it is an alias word (no `*`, space or game delimiter in it, not too long) of a
    // script the pack names.
    let delimiters = ConverseText::from_game(files.get("game")?)
        .ok()?
        .get(Msg::Delimiters)
        .to_vec();
    let mut aliases: Vec<(ScriptKey, Vec<u8>, Vec<Vec<u8>>)> = Vec::new();
    for s in &catalog.scripts {
        let letter = s.archive as u8;
        if !s.runs.is_empty() || !s.strings.is_empty() {
            named.push((letter, s.item, s.hash));
            let mut keys: Vec<Vec<u8>> = s
                .keywords
                .iter()
                .map(|k| {
                    let kw = bytes(&k.text);
                    let len = kw.iter().position(|&b| b == 0).unwrap_or(kw.len());
                    kw[..len].to_ascii_lowercase()
                })
                .filter(|k| {
                    (1..=MAX_ALIAS_CHARS).contains(&k.len())
                        && k.iter().all(|&b| {
                            b.is_ascii_graphic()
                                && !b.is_ascii_uppercase()
                                && !matches!(b, b',' | b'*')
                                && !delimiters.contains(&b)
                        })
                })
                .collect();
            keys.sort();
            keys.dedup();
            aliases.extend(
                keys.into_iter()
                    .map(|k| ((letter, s.item), k.clone(), vec![k])),
            );
        }
        for r in &s.runs {
            runs.push((
                (letter, s.item, r.unit.offset),
                RunText {
                    len: r.unit.len,
                    hash: r.unit.hash,
                    text: bytes(&r.unit.text),
                },
            ));
        }
        // An empty string is no unit (the engine never looks one up).
        for t in s.strings.iter().filter(|t| t.unit.len > 0) {
            tables.push((
                (letter, s.item, t.unit.offset),
                TableText {
                    len: t.unit.len,
                    hash: t.unit.hash,
                    // The catalog marks every string a computed copy can reach; one whose own
                    // English is longer than the slot holds keeps no capacity here (the engine
                    // still holds a copy to its slot's).
                    capacity: t.capacity.filter(|&c| t.unit.len <= c),
                    text: bytes(&t.unit.text),
                },
            ));
        }
    }
    let catalogue = Catalogue::from_game(files.get("game")?);
    let strings = catalogue.iter().map(|(hash, e)| {
        (
            hash,
            StringText {
                len: e.text.len() as u32,
                text: e.text.clone(),
            },
        )
    });
    let itself = |units: Vec<(usize, Vec<u8>)>| -> Vec<(u32, RunText)> {
        units
            .into_iter()
            .map(|(i, en)| {
                let t = RunText {
                    len: en.len() as u32,
                    hash: crate::converse::disasm::catalog::hash64(&en),
                    text: en,
                };
                (i as u32, t)
            })
            .collect()
    };
    let look = files
        .get(texts::LOOK_FILE)
        .and_then(|f| u6_formats::look::LookTable::from_lzd(f).ok())
        .map(|t| itself(texts::look_units(&t)))
        .unwrap_or_default();
    let books = files
        .get(texts::BOOK_FILE)
        .map(|f| itself(texts::book_units(f)))
        .unwrap_or_default();
    let program = program::Catalogue::from_files(files);
    let program = program.iter().map(|(hash, e)| {
        (
            hash,
            StringText {
                len: e.text.len() as u32,
                text: e.text.clone(),
            },
        )
    });
    let made = Pack::new(Language::English, "identity/5", fingerprint(named), runs)
        .inspect_err(|e| eprintln!("identity pack: runs refused: {e:?}"))
        .ok()?
        .with_tables(tables)
        .inspect_err(|e| eprintln!("identity pack: table strings refused: {e:?}"))
        .ok()?
        .with_strings(strings)
        .inspect_err(|e| eprintln!("identity pack: game strings refused: {e:?}"))
        .ok()?
        .with_look(look)
        .inspect_err(|e| eprintln!("identity pack: object names refused: {e:?}"))
        .ok()?
        .with_books(books)
        .inspect_err(|e| eprintln!("identity pack: books refused: {e:?}"))
        .ok()?
        .with_program(program)
        .inspect_err(|e| eprintln!("identity pack: u strings refused: {e:?}"))
        .ok()?
        .with_aliases(aliases)
        .inspect_err(|e| eprintln!("identity pack: keyword aliases refused: {e:?}"))
        .ok()?;
    let read = Pack::parse(&made.to_bytes()).ok()?;
    (read == made).then(|| Rc::new(read))
}
