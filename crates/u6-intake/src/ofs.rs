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

/// Bytes of a name in UTF-8: at most 30 characters (the BCPL length is clamped there), each
/// ISO 8859-1, so U+0000..U+00FF and at most 2 bytes.
pub const MAX_NAME_BYTES: usize = 60;
/// Directory levels below the root the walk enters; a directory deeper than that is not read
/// (one warning per volume). Real Amiga floppies nest a few levels. 64 keeps the walk's
/// recursion shallow on the 1 MiB stack a wasm build gets and a path at most
/// [`MAX_PATH_BYTES`].
pub const MAX_DEPTH: usize = 64;
/// The longest path the walk builds: a file in the deepest directory it enters, 65 names and
/// 64 slashes, 3,964 bytes.
pub const MAX_PATH_BYTES: usize = (MAX_DEPTH + 1) * MAX_NAME_BYTES + MAX_DEPTH;
/// Warnings a volume lists; the rest are counted in one more line.
pub const MAX_WARNINGS: usize = 32;
/// Room the intake needs in front of a path for its key: the volume name and a slash.
const KEY_PREFIX: usize = MAX_NAME_BYTES + 1;
/// What each file, directory and warning the walk keeps is charged besides its bytes and text:
/// its list and map entries, the `String` and `Vec` headers, its block in the visited set, and
/// the same again in the intake's maps. An empty file at the root costs about 270 bytes through
/// the whole intake, its name twice included (tests/budget.rs).
pub const ENTRY_BYTES: usize = 256;

#[derive(Clone, Debug, Default)]
pub struct OfsVolume {
    pub name: String,
    pub dos_type: String,
    /// `"dir/sub/file"` (relative to the volume) -> file bytes, in directory walk order.
    pub files: Vec<(String, Vec<u8>)>,
    /// Directory paths seen (relative to the volume, "" omitted).
    pub dirs: Vec<String>,
    /// Problems found while walking (bad checksums, broken chains): at most [`MAX_WARNINGS`],
    /// then a line for directories past [`MAX_DEPTH`] and one counting the warnings not listed.
    pub warnings: Vec<String>,
    /// What the walk charged against its limit: each file's bytes (as reserved) and its path
    /// twice (the key and an index copy, with room for the intake's volume prefix in front of
    /// each), each directory path and each listed warning, and [`ENTRY_BYTES`] for each of them
    /// (strings are built at their exact length, so the charge is what they hold); and, kept as
    /// spent, a failed read's bytes and a block for each extension table any read walks.
    pub held: usize,
    /// The walk stopped where the next charge would have passed the limit given to
    /// [`read_ofs_within`]: the rest of the volume is not read.
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

/// BCPL string at offset 432, bytes mapped one-to-one to chars (ISO 8859-1), at its exact
/// length (at most [`MAX_NAME_BYTES`]).
fn bcpl_name(img: &[u8], block: usize) -> String {
    let base = block * BS + 432;
    let len = usize::from(img.get(base).copied().unwrap_or(0)).min(30);
    let chars = || (0..len).map(|i| char::from(img.get(base + 1 + i).copied().unwrap_or(0)));
    let mut name = String::with_capacity(chars().map(char::len_utf8).sum());
    chars().for_each(|c| name.push(c));
    name
}

/// `prefix/name` (or `name` at the root) at its exact length: `format!` reserves about twice.
fn join_path(prefix: &str, name: String) -> String {
    if prefix.is_empty() {
        return name;
    }
    let mut path = String::with_capacity(prefix.len() + 1 + name.len());
    path.push_str(prefix);
    path.push('/');
    path.push_str(&name);
    path
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
    /// Warnings past [`MAX_WARNINGS`]: counted, not kept.
    unlisted: usize,
    /// A directory past [`MAX_DEPTH`] was met (and not entered).
    too_deep: bool,
    /// Bytes the lists may hold together ([`OfsVolume::held`]), and hold now.
    limit: usize,
    held: usize,
    limit_reached: bool,
}

impl Reader<'_> {
    fn valid(&self, b: u32) -> bool {
        b > 1 && (b as usize) < self.blocks
    }

    /// Charges `n` bytes, or stops the walk when they would pass the limit.
    fn charge(&mut self, n: usize) -> bool {
        if self.limit_reached || n > self.limit - self.held {
            self.limit_reached = true;
            return false;
        }
        self.held += n;
        true
    }

    /// Lists a warning while fewer than [`MAX_WARNINGS`] are listed; counts it after that.
    fn warn(&mut self, msg: impl FnOnce() -> String) {
        if self.warnings.len() >= MAX_WARNINGS {
            self.unlisted += 1;
            return;
        }
        let mut w = msg();
        w.shrink_to_fit();
        if self.charge(w.capacity() + ENTRY_BYTES) {
            self.warnings.push(w);
        }
    }

    fn check_header(&mut self, b: usize, what: &str) {
        if !block_checksum_ok(self.img, b) {
            self.warn(|| format!("{what}: bad block checksum at {b}"));
        }
    }

    /// Map semantics: a repeated path replaces the bytes but keeps its first position. The
    /// bytes and `key` were charged by [`Reader::read_file`]; a repeated path gives back its
    /// earlier bytes and the key it does not store again.
    fn set_file(&mut self, path: String, bytes: Vec<u8>, key: usize) {
        match self.index.get(&path) {
            Some(&i) => {
                // The earlier copy was charged its byte size, its length.
                let old = std::mem::replace(&mut self.files[i].1, bytes);
                self.held -= old.len() + key;
            }
            None => {
                self.index.insert(path.clone(), self.files.len());
                self.files.push((path, bytes));
            }
        }
    }
}

impl Reader<'_> {
    /// The file's bytes, or `None` when they and `key` (what its path costs) would pass the
    /// limit (nothing is read then), or when the tables it walks do. Both are charged before
    /// the bytes are reserved. A failed read keeps its charge: what it copied counts as spent,
    /// so headers that share one bad chain cannot copy it again and again past the limit.
    fn read_file(&mut self, hdr: usize, path: &str, key: usize) -> Result<Option<Vec<u8>>, Error> {
        let size = u32_at(self.img, hdr * BS + 324) as usize;
        // A file's bytes sit in the image's blocks, at most a block's worth in each, so no file
        // is larger than its image. A larger byte_size is corrupt: refused before any read (a
        // self-linked extension chain would otherwise read up to `blocks` tables of it).
        if size > self.img.len() {
            return Err(Error::new(format!(
                "{path}: byte size {size} passes the image size {}",
                self.img.len()
            )));
        }
        if !self.charge(size + key) {
            return Ok(None);
        }
        self.read_blocks(hdr, path, size)
    }

    fn read_blocks(
        &mut self,
        hdr: usize,
        path: &str,
        size: usize,
    ) -> Result<Option<Vec<u8>>, Error> {
        let img = self.img;
        // Exactly the byte size, so the bytes never move to a larger block as they grow.
        let mut out = Vec::with_capacity(size);
        let per_block = if self.ffs { BS } else { BS - 24 };
        let mut table = hdr as u32;
        let mut hops = 0usize;
        while table != 0 && out.len() < size {
            if !self.valid(table) || hops > self.blocks {
                return Err(Error::new(format!("{path}: broken extension chain")));
            }
            // Each table after the first is a block's worth of work (its checksum reads it
            // whole), charged and kept: empty tables copy nothing, but many headers can walk
            // one long chain of them.
            if hops > 0 && !self.charge(BS) {
                return Ok(None);
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
                        self.warn(|| format!("{path}: bad data block checksum at {db}"));
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

    /// Walks the directory at `dir_block`, `depth` levels below the root (the root is 0).
    fn walk_dir(&mut self, dir_block: usize, prefix: &str, depth: usize) {
        let img = self.img;
        let here = if prefix.is_empty() { "/" } else { prefix };
        for i in 0..TABLE_LONGS {
            let mut b = u32_at(img, dir_block * BS + TABLE_AT + i * 4);
            while b != 0 {
                if self.limit_reached {
                    return;
                }
                if !self.valid(b) {
                    self.warn(|| format!("{here}: bad hash entry {b}"));
                    break;
                }
                let blk = b as usize;
                if !self.seen.insert(blk) {
                    self.warn(|| format!("{here}: loop at block {b}"));
                    break;
                }
                let path = join_path(prefix, bcpl_name(img, blk));
                let kind = s32_at(img, blk * BS);
                let sec = s32_at(img, blk * BS + BS - 4);
                if kind != T_HEADER {
                    self.warn(|| format!("{path}: block {b} has type {kind}"));
                } else {
                    self.check_header(blk, &path);
                    if sec == ST_FILE {
                        let key = 2 * (KEY_PREFIX + path.capacity()) + ENTRY_BYTES;
                        match self.read_file(blk, &path, key) {
                            Ok(Some(bytes)) => self.set_file(path, bytes, key),
                            Ok(None) => return,
                            Err(err) => self.warn(|| err.to_string()),
                        }
                    } else if sec == ST_USERDIR {
                        if depth >= MAX_DEPTH {
                            self.too_deep = true;
                        } else if self.charge(path.capacity() + ENTRY_BYTES) {
                            self.dirs.push(path.clone());
                            self.walk_dir(blk, &path, depth + 1);
                        } else {
                            return;
                        }
                    } else {
                        self.warn(|| format!("{path}: skipped entry with sec_type {sec}"));
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

/// As [`read_ofs`], but what the walk keeps ([`OfsVolume::held`]) comes to at most `limit`
/// bytes: it stops before the first charge that would pass it ([`OfsVolume::limit_reached`]).
/// The intake passes what is left of its budget.
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
        unlisted: 0,
        too_deep: false,
        limit,
        held: 0,
        limit_reached: false,
    };
    r.check_header(root, "root");
    r.walk_dir(root, "", 0);
    // Two fixed lines, not charged.
    let mut warnings = r.warnings;
    if r.too_deep {
        warnings.push(format!(
            "directories nested more than {MAX_DEPTH} levels deep are not read"
        ));
    }
    if r.unlisted > 0 {
        warnings.push(format!("{} more warnings not listed", r.unlisted));
    }
    Ok(OfsVolume {
        name: bcpl_name(img, root),
        dos_type,
        files: r.files,
        dirs: r.dirs,
        warnings,
        held: r.held,
        limit_reached: r.limit_reached,
    })
}
