//! Test-only (spec 08 Order 3 / Acceptance 5, slices K1 and K2): the identity pack of the
//! owner's files, built at test time: every run and every table string of the catalog of his
//! conversation scripts, and every executable string the engine catalogues from his `game`,
//! each translated as itself, declaring English grammar. Installed, it turns `wide` on and
//! must show exactly what English shows, every catalogued unit a hit.

use std::rc::Rc;

use u6_formats::converse::ConverseLib;

use super::strings::Catalogue;
use super::{Pack, RunText, StringText, TableText, fingerprint};
use crate::converse::disasm::catalog::Catalog;
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
    for s in &catalog.scripts {
        let letter = s.archive as u8;
        if !s.runs.is_empty() || !s.strings.is_empty() {
            named.push((letter, s.item, s.hash));
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
    let made = Pack::new(Language::English, "identity/2", fingerprint(named), runs)
        .inspect_err(|e| eprintln!("identity pack: runs refused: {e:?}"))
        .ok()?
        .with_tables(tables)
        .inspect_err(|e| eprintln!("identity pack: table strings refused: {e:?}"))
        .ok()?
        .with_strings(strings)
        .inspect_err(|e| eprintln!("identity pack: game strings refused: {e:?}"))
        .ok()?;
    let read = Pack::parse(&made.to_bytes()).ok()?;
    (read == made).then(|| Rc::new(read))
}
