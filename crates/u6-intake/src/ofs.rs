//! AmigaDOS filesystem reader (OFS "DOS\0", plus FFS "DOS\1" data blocks) for
//! 880 KB double-density images. Block = 512 bytes, big-endian longs.
//!
//! Header blocks (root, user dir, file header, file extension) share a layout:
//! type @0, header_key @4, high_seq @8, table size @12, first_data @16,
//! checksum @20, hash/data-block table @24..311, byte_size @324, name (BCPL) @432,
//! hash_chain @496, parent @500, extension @504, sec_type @508.

use std::collections::{BTreeMap, BTreeSet};

use crate::Error;

const BS: usize = 512;
const ROOT_BLOCK_DD: usize = 880;
const TABLE_AT: usize = 24;
/// 72 on 512-byte blocks.
const TABLE_LONGS: usize = (BS - 200 - TABLE_AT) / 4;

const T_HEADER: i32 = 2;
const T_DATA: i32 = 8;
const T_LIST: i32 = 16;
const ST_ROOT: i32 = 1;
const ST_USERDIR: i32 = 2;
const ST_FILE: i32 = -3;

#[derive(Clone, Debug, Default)]
pub struct OfsVolume {
    pub name: String,
    pub dos_type: String,
    /// `"dir/sub/file"` (relative to the volume) -> file bytes, in directory walk order.
    pub files: Vec<(String, Vec<u8>)>,
    /// Directory paths seen (relative to the volume, "" omitted).
    pub dirs: Vec<String>,
    /// Problems found while walking (bad checksums, broken chains).
    pub warnings: Vec<String>,
    /// The walk stopped at a file whose bytes would take `files` past the limit given to
    /// [`read_ofs_within`]: that file and the ones after it are not read.
    pub limit_reached: bool,
}

impl OfsVolume {
    /// Bytes of the file at `path`, if the walk found it.
    pub fn file(&self, path: &str) -> Option<&[u8]> {
        self.files
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, d)| d.as_slice())
    }
}

fn u32_at(img: &[u8], at: usize) -> u32 {
    let b = |i: usize| img.get(at + i).copied().unwrap_or(0);
    u32::from_be_bytes([b(0), b(1), b(2), b(3)])
}

fn s32_at(img: &[u8], at: usize) -> i32 {
    u32_at(img, at) as i32
}

fn block_checksum_ok(img: &[u8], block: usize) -> bool {
    let base = block * BS;
    (0..BS)
        .step_by(4)
        .fold(0u32, |sum, i| sum.wrapping_add(u32_at(img, base + i)))
        == 0
}

/// BCPL string at offset 432, bytes mapped one-to-one to chars (ISO 8859-1).
fn bcpl_name(img: &[u8], block: usize) -> String {
    let base = block * BS + 432;
    let len = usize::from(img.get(base).copied().unwrap_or(0)).min(30);
    (0..len)
        .map(|i| char::from(img.get(base + 1 + i).copied().unwrap_or(0)))
        .collect()
}

/// `"DOS<n>"` when the boot block carries the DOS signature.
pub fn adf_dos_type(img: &[u8]) -> Option<String> {
    if img.len() < 4 || img[..3] != *b"DOS" {
        return None;
    }
    Some(format!("DOS{}", img[3]))
}

struct Reader<'a> {
    img: &'a [u8],
    blocks: usize,
    ffs: bool,
    seen: BTreeSet<usize>,
    files: Vec<(String, Vec<u8>)>,
    index: BTreeMap<String, usize>,
    dirs: Vec<String>,
    warnings: Vec<String>,
    /// Bytes `files` may hold together, and hold now.
    limit: usize,
    total: usize,
    limit_reached: bool,
}

impl Reader<'_> {
    fn valid(&self, b: u32) -> bool {
        b > 1 && (b as usize) < self.blocks
    }

    fn check_header(&mut self, b: usize, what: &str) {
        if !block_checksum_ok(self.img, b) {
            self.warnings
                .push(format!("{what}: bad block checksum at {b}"));
        }
    }

    /// Map semantics: a repeated path replaces the bytes but keeps its first position.
    fn set_file(&mut self, path: String, bytes: Vec<u8>) {
        self.total += bytes.len();
        match self.index.get(&path) {
            Some(&i) => {
                let old = std::mem::replace(&mut self.files[i].1, bytes);
                self.total -= old.len();
            }
            None => {
                self.index.insert(path.clone(), self.files.len());
                self.files.push((path, bytes));
            }
        }
    }
}

impl Reader<'_> {
    /// The file's bytes, or `None` when they would take the files read past the limit (nothing
    /// is read then).
    fn read_file(&mut self, hdr: usize, path: &str) -> Result<Option<Vec<u8>>, Error> {
        let img = self.img;
        let size = u32_at(img, hdr * BS + 324) as usize;
        // A file's bytes sit in the image's blocks, at most a block's worth in each, so no file
        // is larger than its image. A larger byte_size is corrupt: refused before any read (a
        // self-linked extension chain would otherwise read up to `blocks` tables of it).
        if size > img.len() {
            return Err(Error::new(format!(
                "{path}: byte size {size} passes the image size {}",
                img.len()
            )));
        }
        // At most byte_size bytes are read, so this keeps the files within the limit.
        if size > self.limit - self.total {
            return Ok(None);
        }
        // Reserves at most the image (byte_size was checked against it above).
        let mut out = Vec::with_capacity(size.min(self.blocks * BS));
        let per_block = if self.ffs { BS } else { BS - 24 };
        let mut table = hdr as u32;
        let mut hops = 0usize;
        while table != 0 && out.len() < size {
            if !self.valid(table) || hops > self.blocks {
                return Err(Error::new(format!("{path}: broken extension chain")));
            }
            hops += 1;
            let t = table as usize;
            if t != hdr {
                if s32_at(img, t * BS) != T_LIST {
                    return Err(Error::new(format!(
                        "{path}: block {t} is not an extension block"
                    )));
                }
                self.check_header(t, path);
            }
            let count = (u32_at(img, t * BS + 8) as usize).min(TABLE_LONGS);
            for i in 0..count {
                if out.len() >= size {
                    break;
                }
                let db = u32_at(img, t * BS + TABLE_AT + (TABLE_LONGS - 1 - i) * 4);
                if !self.valid(db) {
                    return Err(Error::new(format!("{path}: bad data block pointer {db}")));
                }
                let mut from = db as usize * BS;
                let mut n = per_block.min(size - out.len());
                if !self.ffs {
                    if s32_at(img, from) != T_DATA || u32_at(img, from + 4) != hdr as u32 {
                        return Err(Error::new(format!(
                            "{path}: block {db} is not a data block of this file"
                        )));
                    }
                    if !block_checksum_ok(img, db as usize) {
                        self.warnings
                            .push(format!("{path}: bad data block checksum at {db}"));
                    }
                    n = (u32_at(img, from + 12) as usize).min(n);
                    from += 24;
                }
                // db < blocks, so from + n stays inside the image.
                out.extend_from_slice(&img[from..from + n]);
            }
            table = u32_at(img, t * BS + BS - 8);
        }
        if out.len() != size {
            return Err(Error::new(format!(
                "{path}: short read {}/{size}",
                out.len()
            )));
        }
        Ok(Some(out))
    }

    fn walk_dir(&mut self, dir_block: usize, prefix: &str) {
        let img = self.img;
        let here = if prefix.is_empty() { "/" } else { prefix };
        for i in 0..TABLE_LONGS {
            let mut b = u32_at(img, dir_block * BS + TABLE_AT + i * 4);
            while b != 0 {
                if self.limit_reached {
                    return;
                }
                if !self.valid(b) {
                    self.warnings.push(format!("{here}: bad hash entry {b}"));
                    break;
                }
                let blk = b as usize;
                if !self.seen.insert(blk) {
                    self.warnings.push(format!("{here}: loop at block {b}"));
                    break;
                }
                let name = bcpl_name(img, blk);
                let path = if prefix.is_empty() {
                    name
                } else {
                    format!("{prefix}/{name}")
                };
                let kind = s32_at(img, blk * BS);
                let sec = s32_at(img, blk * BS + BS - 4);
                if kind != T_HEADER {
                    self.warnings
                        .push(format!("{path}: block {b} has type {kind}"));
                } else {
                    self.check_header(blk, &path);
                    if sec == ST_FILE {
                        match self.read_file(blk, &path) {
                            Ok(Some(bytes)) => self.set_file(path, bytes),
                            Ok(None) => {
                                self.limit_reached = true;
                                return;
                            }
                            Err(err) => self.warnings.push(err.to_string()),
                        }
                    } else if sec == ST_USERDIR {
                        self.dirs.push(path.clone());
                        self.walk_dir(blk, &path);
                    } else {
                        self.warnings
                            .push(format!("{path}: skipped entry with sec_type {sec}"));
                    }
                }
                b = u32_at(img, blk * BS + BS - 16);
            }
        }
    }
}

pub fn read_ofs(img: &[u8]) -> Result<OfsVolume, Error> {
    read_ofs_within(img, usize::MAX)
}

/// As [`read_ofs`], but the files read come to at most `limit` bytes together: the walk stops
/// before the first file that would pass it ([`OfsVolume::limit_reached`]). The intake passes
/// what is left of its budget.
pub fn read_ofs_within(img: &[u8], limit: usize) -> Result<OfsVolume, Error> {
    let dos_type = adf_dos_type(img)
        .ok_or_else(|| Error::new("not an AmigaDOS image (boot block lacks DOS signature)"))?;
    let flags = img[3];
    if flags > 7 {
        return Err(Error::new(format!("unsupported AmigaDOS type {dos_type}")));
    }
    let blocks = img.len() / BS;
    let root = ROOT_BLOCK_DD;
    if root >= blocks {
        return Err(Error::new(
            "image too small for a double-density root block",
        ));
    }
    if s32_at(img, root * BS) != T_HEADER || s32_at(img, root * BS + BS - 4) != ST_ROOT {
        return Err(Error::new(format!("block {root} is not a root block")));
    }

    let mut r = Reader {
        img,
        blocks,
        ffs: flags & 1 == 1,
        seen: BTreeSet::new(),
        files: Vec::new(),
        index: BTreeMap::new(),
        dirs: Vec::new(),
        warnings: Vec::new(),
        limit,
        total: 0,
        limit_reached: false,
    };
    r.check_header(root, "root");
    r.walk_dir(root, "");
    Ok(OfsVolume {
        name: bcpl_name(img, root),
        dos_type,
        files: r.files,
        dirs: r.dirs,
        warnings: r.warnings,
        limit_reached: r.limit_reached,
    })
}
