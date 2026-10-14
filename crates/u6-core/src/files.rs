//! The file layer: replaces dos.library file access (hunk 21) with an in-memory, layered map.
//!
//! The base layer is what u6-intake extracted from the user's disks (`"VOLUME/dir/file"` keys,
//! PP20 already decrunched). The game opens names like `savegame/objlist` or `schedule` and
//! AmigaDOS matches names without regard to case; the disks have no `savegame` directory (the
//! new-game copies sit at the volume roots), so a lookup falls back to the bare file name. Mods
//! add layers on top ([`Files::overlay`]); the topmost layer that has a name wins, so a mod
//! replaces or adds a file without touching the engine (spec 03 §5).
//!
//! Lookup of `name` in one layer, first hit wins:
//! 1. the whole path, ignoring ASCII case (`:` counts as `/`, so `DATA1:map` = `DATA1/map`);
//! 2. the bare file name (last path component), ignoring ASCII case. When several paths in the
//!    layer share a bare name, the first in case-folded path order is used.

use std::collections::BTreeMap;

/// One layer: files plus case-folded indexes.
#[derive(Clone, Debug, Default)]
struct Layer {
    name: String,
    files: BTreeMap<String, Vec<u8>>,
    /// Case-folded full path -> key in `files`.
    by_path: BTreeMap<String, String>,
    /// Case-folded bare name -> key in `files` (first in case-folded path order).
    by_name: BTreeMap<String, String>,
}

fn normalize(name: &str) -> String {
    name.replace(['\\', ':'], "/")
        .trim_start_matches('/')
        .to_ascii_lowercase()
}

fn bare(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

impl Layer {
    fn new(name: String, files: BTreeMap<String, Vec<u8>>) -> Self {
        let mut folded: Vec<(String, String)> =
            files.keys().map(|k| (normalize(k), k.clone())).collect();
        folded.sort();
        let mut by_path = BTreeMap::new();
        let mut by_name = BTreeMap::new();
        for (fold, key) in folded {
            by_name
                .entry(bare(&fold).to_string())
                .or_insert_with(|| key.clone());
            by_path.entry(fold).or_insert(key);
        }
        Self {
            name,
            files,
            by_path,
            by_name,
        }
    }

    fn resolve(&self, name: &str) -> Option<&str> {
        let fold = normalize(name);
        self.by_path
            .get(&fold)
            .or_else(|| self.by_name.get(bare(&fold)))
            .map(String::as_str)
    }
}

/// Layered, case-insensitive game file map.
#[derive(Clone, Debug, Default)]
pub struct Files {
    /// Bottom (base) first.
    layers: Vec<Layer>,
}

impl Files {
    /// A file map with one base layer, e.g. `u6_intake::intake(..).files`.
    pub fn new(base: BTreeMap<String, Vec<u8>>) -> Self {
        Self {
            layers: vec![Layer::new("base".into(), base)],
        }
    }

    /// Adds a layer on top of the existing ones.
    pub fn overlay(&mut self, name: impl Into<String>, files: BTreeMap<String, Vec<u8>>) {
        self.layers.push(Layer::new(name.into(), files));
    }

    /// Replaces: 0x1348d0 (open a game data file), 0x134ac4 (read N bytes), 0x134b32 (close)
    ///
    /// The bytes the game would read for `name`, from the topmost layer that has it.
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.layers.iter().rev().find_map(|layer| {
            let key = layer.resolve(name)?;
            layer.files.get(key).map(Vec::as_slice)
        })
    }

    /// Which layer and stored path `name` resolves to.
    pub fn resolve(&self, name: &str) -> Option<(&str, &str)> {
        self.layers
            .iter()
            .rev()
            .find_map(|layer| Some((layer.name.as_str(), layer.resolve(name)?)))
    }

    /// Layer names, bottom first.
    pub fn layers(&self) -> impl Iterator<Item = &str> {
        self.layers.iter().map(|l| l.name.as_str())
    }

    /// Number of stored files over all layers.
    pub fn len(&self) -> usize {
        self.layers.iter().map(|l| l.files.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(entries: &[(&str, &[u8])]) -> BTreeMap<String, Vec<u8>> {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_vec()))
            .collect()
    }

    fn base() -> Files {
        Files::new(map(&[
            ("DATA1/OBJBLKAA", b"aa"),
            ("DATA1/map", b"m"),
            ("DATA1/objlist", b"o"),
            ("INSTALL/schedule", b"s"),
            ("INSTALL/game", b"g"),
        ]))
    }

    #[test]
    fn full_path_then_bare_name_ignoring_case() {
        let f = base();
        assert_eq!(f.get("DATA1/map"), Some(&b"m"[..]));
        assert_eq!(f.get("data1/MAP"), Some(&b"m"[..]));
        assert_eq!(f.get("DATA1:map"), Some(&b"m"[..]));
        assert_eq!(f.get("savegame/objblkaa"), Some(&b"aa"[..]));
        assert_eq!(f.get("savegame/objlist"), Some(&b"o"[..]));
        assert_eq!(f.get("Schedule"), Some(&b"s"[..]));
        assert_eq!(f.get("chunks"), None);
        assert_eq!(
            f.resolve("savegame/objblkaa"),
            Some(("base", "DATA1/OBJBLKAA"))
        );
        assert_eq!(f.len(), 5);
    }

    #[test]
    fn bare_name_collision_takes_first_folded_path() {
        let f = Files::new(map(&[("b/x", b"2"), ("A/X", b"1"), ("c/x", b"3")]));
        assert_eq!(f.get("x"), Some(&b"1"[..]));
        // An exact path still reaches the others.
        assert_eq!(f.get("C/X"), Some(&b"3"[..]));
    }

    #[test]
    fn overlays_replace_and_add_from_the_top() {
        let mut f = base();
        f.overlay(
            "mod-a",
            map(&[("mods/a/MAP", b"M2"), ("mods/a/extra", b"e")]),
        );
        assert_eq!(f.get("DATA1/map"), Some(&b"M2"[..]));
        assert_eq!(f.get("map"), Some(&b"M2"[..]));
        assert_eq!(f.get("extra"), Some(&b"e"[..]));
        assert_eq!(f.get("schedule"), Some(&b"s"[..]));
        f.overlay("mod-b", map(&[("schedule", b"s2")]));
        assert_eq!(f.get("INSTALL/schedule"), Some(&b"s2"[..]));
        assert_eq!(f.resolve("map"), Some(("mod-a", "mods/a/MAP")));
        assert_eq!(f.layers().collect::<Vec<_>>(), ["base", "mod-a", "mod-b"]);
        assert!(!f.is_empty() && Files::default().is_empty());
    }
}
