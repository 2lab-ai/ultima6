//! Save-only transfer containers. No engine, source game files, pack or model is accessed.
//! The browser validates the manifest schema and SHA-256; this boundary checks ZIP structure,
//! budgets and the original save formats. A stale sight sidecar is rejected, not removed:
//! this stricter transfer policy does not change the engine's usual optional-sight loading.

use std::collections::{BTreeMap, BTreeSet};
use u6_core::actors::{PARTY_SIZE_MAX, PARTY_SLOTS};
use u6_core::gfx::cache::{MAX_SIGHT_FILE_BYTES, SightFileError, read_sight};
use u6_core::saveload::{OBJLIST_FILE, SIGHT_FILE, objlist};
use u6_intake::zip::{self, Directory};
use wasm_bindgen::prelude::*;

use crate::{MAX_RECORDS, MAX_SAVE_BYTES, MAX_SAVE_FILE_BYTES};

const ARCHIVE_LIMIT: usize = 16 << 20;
const MANIFEST_LIMIT: usize = 32 << 10;
const ENTRY_LIMIT: usize = 72;
const MANIFEST: &str = "manifest.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Error {
    Size,
    Entries,
    Zip,
    Path,
    Duplicate,
    Manifest,
    ObjlistMissing,
    Objlist,
    Block,
    Sight,
    SightMismatch,
    Index,
}

impl Error {
    fn code(self) -> &'static str {
        match self {
            Self::Size => "save-transfer-too-large",
            Self::Entries => "save-transfer-entry-limit",
            Self::Zip => "save-transfer-invalid-zip",
            Self::Path => "save-transfer-path",
            Self::Duplicate => "save-transfer-duplicate",
            Self::Manifest => "save-transfer-manifest",
            Self::ObjlistMissing => "save-transfer-missing-objlist",
            Self::Objlist => "save-transfer-invalid-objlist",
            Self::Block => "save-transfer-invalid-block",
            Self::Sight => "save-transfer-invalid-sight",
            Self::SightMismatch => "save-transfer-sight-mismatch",
            Self::Index => "save-transfer-index",
        }
    }
}

fn js(error: Error) -> JsValue {
    JsValue::from_str(error.code())
}

fn canonical(path: &str) -> Result<String, Error> {
    if !path.is_ascii() || path.len() > 32 {
        return Err(Error::Path);
    }
    let path = path.to_ascii_lowercase();
    let block = path.strip_prefix("savegame/objblk").is_some_and(|suffix| {
        let b = suffix.as_bytes();
        b.len() == 2
            && ((b'a'..=b'h').contains(&b[0]) && (b'a'..=b'h').contains(&b[1])
                || (b'a'..=b'e').contains(&b[0]) && b[1] == b'i')
    });
    if matches!(path.as_str(), MANIFEST | OBJLIST_FILE | SIGHT_FILE) || block {
        Ok(path)
    } else {
        Err(Error::Path)
    }
}

fn limit(path: &str) -> usize {
    match path {
        MANIFEST => MANIFEST_LIMIT,
        SIGHT_FILE => MAX_SIGHT_FILE_BYTES,
        _ => MAX_SAVE_FILE_BYTES,
    }
}

#[derive(Default)]
struct Budget {
    ordinary: usize,
    total: usize,
}

impl Budget {
    fn add(&mut self, path: &str, size: usize) -> Result<(), Error> {
        if size > limit(path) {
            return Err(Error::Size);
        }
        self.total = self.total.checked_add(size).ok_or(Error::Size)?;
        if path != MANIFEST && path != SIGHT_FILE {
            self.ordinary = self.ordinary.checked_add(size).ok_or(Error::Size)?;
        }
        if self.ordinary > MAX_SAVE_BYTES
            || self.total > MAX_SAVE_BYTES + MAX_SIGHT_FILE_BYTES + MANIFEST_LIMIT
        {
            return Err(Error::Size);
        }
        Ok(())
    }
}

fn word(data: &[u8], at: usize) -> Result<u32, Error> {
    let bytes = data
        .get(at..at.checked_add(4).ok_or(Error::Zip)?)
        .ok_or(Error::Zip)?;
    Ok(u32::from_le_bytes(
        bytes.try_into().map_err(|_| Error::Zip)?,
    ))
}

fn short(data: &[u8], at: usize) -> Result<usize, Error> {
    let bytes = data
        .get(at..at.checked_add(2).ok_or(Error::Zip)?)
        .ok_or(Error::Zip)?;
    Ok(usize::from(u16::from_le_bytes([bytes[0], bytes[1]])))
}

/// This engine's exported ZIP: one disk, no ZIP64, comments, extra fields or hidden bytes.
/// Deflate and a matching 12/16-byte data descriptor are accepted as well as stored entries.
/// All declared sizes and paths are checked here, before entry allocation or inflation.
fn envelope(data: &[u8]) -> Result<(), Error> {
    if data.len() > ARCHIVE_LIMIT {
        return Err(Error::Size);
    }
    let end = data.len().checked_sub(22).ok_or(Error::Zip)?;
    if word(data, end)? != 0x0605_4b50
        || short(data, end + 4)? != 0
        || short(data, end + 6)? != 0
        || short(data, end + 20)? != 0
    {
        return Err(Error::Zip);
    }
    let count = short(data, end + 10)?;
    if count > ENTRY_LIMIT {
        return Err(Error::Entries);
    }
    if short(data, end + 8)? != count {
        return Err(Error::Zip);
    }
    let central = word(data, end + 16)? as usize;
    if central.checked_add(word(data, end + 12)? as usize) != Some(end) {
        return Err(Error::Zip);
    }
    let (mut at, mut ranges, mut paths, mut budget) =
        (central, Vec::new(), BTreeSet::new(), Budget::default());
    for _ in 0..count {
        if word(data, at)? != 0x0201_4b50 || short(data, at + 34)? != 0 {
            return Err(Error::Zip);
        }
        let flags = short(data, at + 8)?;
        let method = short(data, at + 10)?;
        if flags & !0x080e != 0 || !matches!(method, 0 | 8) {
            return Err(Error::Zip);
        }
        let crc = word(data, at + 16)?;
        let packed = word(data, at + 20)? as usize;
        let full = word(data, at + 24)? as usize;
        let name_len = short(data, at + 28)?;
        if short(data, at + 30)? != 0 || short(data, at + 32)? != 0 {
            return Err(Error::Zip);
        }
        let name_end = at.checked_add(46 + name_len).ok_or(Error::Zip)?;
        if name_end > end {
            return Err(Error::Zip);
        }
        let name = data.get(at + 46..name_end).ok_or(Error::Zip)?;
        let path = canonical(std::str::from_utf8(name).map_err(|_| Error::Path)?)?;
        if !paths.insert(path.clone()) {
            return Err(Error::Duplicate);
        }
        budget.add(&path, full)?;
        if method == 0 && packed != full {
            return Err(Error::Zip);
        }
        let local = word(data, at + 42)? as usize;
        if local > central
            || word(data, local)? != 0x0403_4b50
            || short(data, local + 6)? != flags
            || short(data, local + 8)? != method
            || short(data, local + 26)? != name_len
            || short(data, local + 28)? != 0
        {
            return Err(Error::Zip);
        }
        let body = local.checked_add(30 + name_len).ok_or(Error::Zip)?;
        if data.get(local + 30..body) != Some(name) {
            return Err(Error::Zip);
        }
        let mut finish = body.checked_add(packed).ok_or(Error::Zip)?;
        if finish > central {
            return Err(Error::Zip);
        }
        if flags & 8 == 0 {
            if word(data, local + 14)? != crc
                || word(data, local + 18)? as usize != packed
                || word(data, local + 22)? as usize != full
            {
                return Err(Error::Zip);
            }
        } else {
            if word(data, finish)? == 0x0807_4b50 {
                finish += 4;
            }
            if word(data, finish)? != crc
                || word(data, finish + 4)? as usize != packed
                || word(data, finish + 8)? as usize != full
            {
                return Err(Error::Zip);
            }
            finish += 12;
        }
        if finish > central {
            return Err(Error::Zip);
        }
        ranges.push((local, finish));
        at = name_end;
    }
    if at != end || !paths.contains(MANIFEST) {
        return Err(Error::Manifest);
    }
    if !paths.contains(OBJLIST_FILE) {
        return Err(Error::ObjlistMissing);
    }
    ranges.sort_unstable();
    let mut next = 0;
    for (start, finish) in ranges {
        if start != next {
            return Err(Error::Zip);
        }
        next = finish;
    }
    if next != central {
        return Err(Error::Zip);
    }
    Ok(())
}

/// Bounded, detached save files. File accessors include manifest.json when present.
#[wasm_bindgen]
#[derive(Default)]
pub struct SaveTransfer {
    files: BTreeMap<String, Vec<u8>>,
}

impl SaveTransfer {
    pub(crate) fn insert(&mut self, path: &str, bytes: &[u8]) -> Result<(), &'static str> {
        self.insert_inner(path, bytes).map_err(Error::code)
    }

    fn insert_inner(&mut self, path: &str, bytes: &[u8]) -> Result<(), Error> {
        let path = canonical(path)?;
        if self.files.contains_key(&path) {
            return Err(Error::Duplicate);
        }
        if self.files.len() == ENTRY_LIMIT {
            return Err(Error::Entries);
        }
        let mut budget = Budget::default();
        for (path, bytes) in &self.files {
            budget.add(path, bytes.len())?;
        }
        budget.add(&path, bytes.len())?;
        self.files.insert(path, bytes.to_vec());
        Ok(())
    }

    pub(crate) fn check(&self) -> Result<(), &'static str> {
        self.check_inner().map_err(Error::code)
    }

    fn check_inner(&self) -> Result<(), Error> {
        let objlist = self.files.get(OBJLIST_FILE).ok_or(Error::ObjlistMissing)?;
        if objlist.len() < objlist::OBJLIST_LEN {
            return Err(Error::Objlist);
        }
        let size = objlist[0x0ff0];
        let leader = objlist[objlist::TAIL + 0x0a];
        // The engine caps the count at PARTY_SIZE_MAX, reserving a roster slot for a vessel.
        // The vessel's leader index equals size; 0x10 is the engine's leader sentinel.
        if !(1..=PARTY_SIZE_MAX).contains(&size)
            || (leader > size && usize::from(leader) != PARTY_SLOTS)
        {
            return Err(Error::Objlist);
        }
        for (path, bytes) in &self.files {
            if path.starts_with("savegame/objblk") {
                let count = bytes
                    .first_chunk::<2>()
                    .map(|b| usize::from(u16::from_be_bytes(*b)))
                    .ok_or(Error::Block)?;
                if count > MAX_RECORDS || bytes.len() != 2 + count * 8 {
                    return Err(Error::Block);
                }
            }
        }
        if let Some(sight) = self.files.get(SIGHT_FILE) {
            read_sight(sight, objlist).map_err(|e| match e {
                SightFileError::OtherSave => Error::SightMismatch,
                _ => Error::Sight,
            })?;
        }
        Ok(())
    }

    fn decode(bytes: &[u8]) -> Result<Self, Error> {
        envelope(bytes)?;
        let Directory::Entries(entries) =
            zip::directory(bytes.len() as u64, 0, bytes, |_| true).map_err(|_| Error::Zip)?
        else {
            return Err(Error::Zip);
        };
        let mut transfer = Self::default();
        for entry in entries {
            let path = canonical(&entry.name())?;
            if entry.full > limit(&path) as u64 || entry.unpacked() > limit(&path) as u64 {
                return Err(Error::Size);
            }
            let data = zip::entry(bytes, &entry).map_err(|_| Error::Zip)?;
            transfer.insert_inner(&path, &data)?;
        }
        transfer.check_inner()?;
        Ok(transfer)
    }
}

#[wasm_bindgen]
impl SaveTransfer {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }

    /// An archive exported by this engine, with a manifest and strictly bounded save files.
    pub fn from_zip(bytes: &[u8]) -> Result<SaveTransfer, JsValue> {
        Self::decode(bytes).map_err(js)
    }

    pub fn add_file(&mut self, path: &str, bytes: &[u8]) -> Result<(), JsValue> {
        self.insert(path, bytes).map_err(JsValue::from_str)
    }

    /// Builders may omit a manifest; imported ZIPs may not. No live game is touched.
    pub fn validate(&self) -> Result<(), JsValue> {
        self.check().map_err(JsValue::from_str)
    }

    pub fn count(&self) -> u32 {
        self.files.len() as u32
    }

    pub fn file_path(&self, index: u32) -> Result<String, JsValue> {
        self.files
            .keys()
            .nth(index as usize)
            .cloned()
            .ok_or_else(|| js(Error::Index))
    }

    pub fn file_bytes(&self, index: u32) -> Result<Vec<u8>, JsValue> {
        self.files
            .values()
            .nth(index as usize)
            .cloned()
            .ok_or_else(|| js(Error::Index))
    }
}

#[wasm_bindgen]
pub fn max_save_archive_bytes() -> f64 {
    ARCHIVE_LIMIT as f64
}

#[cfg(test)]
mod tests;
