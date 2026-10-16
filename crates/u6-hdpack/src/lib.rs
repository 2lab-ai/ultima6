//! The HD pack format `u6-hdpack/1` (spec `.prd/07-hd-art.md` Design 2, HD1): a zip of HD
//! images keyed by the classic image they were made from. The local tool `tools/hd` writes it;
//! the page will read it (H2). Platform-free: no I/O, builds for `wasm32-unknown-unknown`.
//!
//! A pack holds, as stored (uncompressed) zip entries:
//! - `manifest.json`: [`Manifest`] — `format` = [`FORMAT`], `tile` = 16, `scale` = 4,
//!   `derived`, `generator`, `base` (the digest of the index, [`base_digest`]) and the count;
//! - `index.bin`: one 24-byte entry per image ([`Entry`], layout at [`INDEX_MAGIC`]);
//! - `atlas-N.webp`: the tile images, [`ATLAS_CELLS`] × [`ATLAS_CELLS`] cells of 64 px, filled
//!   row by row in record order (N from 0; an atlas is as tall as its rows);
//! - `portrait/<letter>-<item>.webp` (the `portrait_ref` pair) and `bitmap/<role>.webp`.
//!
//! **Codec: lossless WebP (VP8L) RGBA**, encoded and decoded by the pure-Rust `image-webp`
//! crate, which builds for wasm and which browsers decode natively; the reader refuses lossy or
//! animated WebP.
//!
//! **Content hash: FNV-1a 64** ([`image_hash`]) over the image's width and height (u16 LE each)
//! and then one byte per pixel, row-major, of the classic image as the engine holds it:
//! - a tile record ([`tile_hash`]): its colour index (0..31), or `0xFF` where the record is
//!   transparent (the AND mask / unwritten rows), so the mask is part of the image;
//! - a portrait: the 56×64 bytes `portrait_pixels` returns; a bitmap: the `width × height`
//!   bytes of `u6_formats::bitmap::Bitmap::pixels` (before the bitmap colour table).
//!
//! The engine computes the same hash of a record's current bytes, so an HD image made from
//! other bytes (another dump, a merged shore record) never matches and the scaler is used.
//!
//! **Scale: ×4** ([`SCALE`]), the page's largest display scale. The imagegen tool keeps its ×8
//! masters in its work dir and writes them here 2 × 2 box-averaged.
//!
//! **Limits** (spec 07 HD1; the reader refuses past them and the writer will not write them):
//! at most [`MAX_ENTRIES`] images, at most [`MAX_DECODED_BYTES`] of RGBA when every image is
//! decoded (counted from the index before anything is decoded), and exact sizes: a tile is
//! 64 × 64, a portrait 224 × 256, a bitmap 4× its classic size (at most the 320 × 200 screen;
//! the classic size itself is checked through the hash, which covers it). Every record, a few
//! hundred portraits and two screen-sized bitmaps fit in the limit (the test
//! `a_full_pack_fits_the_limit`). The zip must hold exactly the entries the index names
//! (checked before any image entry is inflated), and every entry read must match its declared
//! size and CRC-32. Anything else that
//! does not match this description — an unknown or repeated entry, an unreferenced file, a
//! wrong digest, a file whose size differs from its index entry — is refused; a read returns
//! the whole pack or an error, never part of one.

use std::collections::BTreeMap;
use std::fmt;
use std::io::Cursor;

use serde::{Deserialize, Serialize};
use u6_formats::gfx::{TILE_COUNT, TILE_W};
use u6_formats::portrait::{PORTRAIT_HEIGHT, PORTRAIT_WIDTH};
use u6_intake::zip;

mod store;

#[cfg(test)]
mod tests;

/// The manifest's `format`.
pub const FORMAT: &str = "u6-hdpack/1";
/// HD pixels per classic pixel.
pub const SCALE: u32 = 4;
/// Side of a classic tile.
pub const TILE: u32 = TILE_W as u32;
/// Side of an HD tile.
pub const TILE_HD: u32 = TILE * SCALE;
/// Size of an HD portrait.
pub const PORTRAIT_HD: (u32, u32) = (
    PORTRAIT_WIDTH as u32 * SCALE,
    PORTRAIT_HEIGHT as u32 * SCALE,
);
/// The largest classic bitmap: the screen.
pub const BITMAP_MAX: (u32, u32) = (320, 200);
/// Cells per atlas row and column (an atlas is at most 2048 px square).
pub const ATLAS_CELLS: u32 = 32;
/// Most images in a pack.
pub const MAX_ENTRIES: usize = 4096;
/// Most RGBA bytes a pack decodes to (150 MB).
pub const MAX_DECODED_BYTES: u64 = 150_000_000;
/// Most bytes the zip's entries may hold: as much as the decoded images.
pub const MAX_FILE_BYTES: u64 = MAX_DECODED_BYTES;
/// `index.bin`: this magic, `u32 LE` version 1, `u32 LE` count, then `count` entries of
/// [`ENTRY_LEN`] bytes, little-endian: `u8 kind` (1 tile, 2 portrait, 3 bitmap), `u8 flags`
/// (bit 0: the image has a pixel with alpha < 255; no other bit), `u16 id` (record, item or
/// role), `u8 letter` (portraits; else 0), `u8` 0, `u16 atlas` (tiles; else 0xFFFF), `u16 col`,
/// `u16 row` (tiles; else 0), `u16 width`, `u16 height` (HD pixels), `u64 hash`. Entries are in
/// strictly increasing (kind, letter, id) order.
pub const INDEX_MAGIC: &[u8; 8] = b"U6HDIDX\0";
/// Bytes per index entry.
pub const ENTRY_LEN: usize = 24;
const INDEX_HEAD: usize = 16;
const INDEX_VERSION: u32 = 1;
/// The `atlas` of an entry that is a file of its own.
const OWN_FILE: u16 = 0xFFFF;
const FLAG_ALPHA: u8 = 1;
/// Longest generator string.
const MAX_TEXT: usize = 256;
/// Largest `manifest.json` read.
const MAX_MANIFEST: u64 = 64 << 10;

/// What an image is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Tile = 1,
    Portrait = 2,
    Bitmap = 3,
}

/// The panel bitmaps the game draws (spec 07: the paper and the spellbook).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    Paper = 0,
    Spellbook = 1,
}

impl Role {
    pub const ALL: [Role; 2] = [Role::Paper, Role::Spellbook];

    /// The name in `bitmap/<role>.webp`.
    pub fn name(self) -> &'static str {
        match self {
            Role::Paper => "paper",
            Role::Spellbook => "spellbook",
        }
    }

    fn from_code(code: u16) -> Option<Role> {
        Role::ALL.into_iter().find(|r| *r as u16 == code)
    }

    fn from_name(name: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|r| r.name() == name)
    }
}

/// Which image: a tile record, a portrait (archive letter, item) or a panel bitmap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ImageId {
    Tile(u16),
    Portrait { letter: u8, item: u16 },
    Bitmap(Role),
}

impl ImageId {
    pub fn kind(self) -> Kind {
        match self {
            ImageId::Tile(_) => Kind::Tile,
            ImageId::Portrait { .. } => Kind::Portrait,
            ImageId::Bitmap(_) => Kind::Bitmap,
        }
    }

    /// The (kind, letter, id) the index is ordered by.
    fn key(self) -> (u8, u8, u16) {
        match self {
            ImageId::Tile(r) => (Kind::Tile as u8, 0, r),
            ImageId::Portrait { letter, item } => (Kind::Portrait as u8, letter, item),
            ImageId::Bitmap(role) => (Kind::Bitmap as u8, 0, role as u16),
        }
    }

    /// The zip entry of an image that is a file of its own.
    fn file_name(self) -> Option<String> {
        match self {
            ImageId::Tile(_) => None,
            ImageId::Portrait { letter, item } => {
                Some(format!("portrait/{}-{item}.webp", char::from(letter)))
            }
            ImageId::Bitmap(role) => Some(format!("bitmap/{}.webp", role.name())),
        }
    }
}

/// An RGBA8 image, row-major.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Rgba {
    /// A transparent image.
    pub fn new(width: u32, height: u32) -> Rgba {
        Rgba {
            width,
            height,
            pixels: vec![0; width as usize * height as usize * 4],
        }
    }

    /// Some pixel has alpha < 255.
    pub fn has_alpha(&self) -> bool {
        self.pixels.chunks_exact(4).any(|p| p[3] != 255)
    }

    /// The `w × h` rectangle at (x, y), which must lie inside.
    pub fn rect(&self, x: u32, y: u32, w: u32, h: u32) -> Rgba {
        let mut out = Rgba::new(w, h);
        for r in 0..h {
            let from = ((y + r) * self.width + x) as usize * 4;
            let to = (r * w) as usize * 4;
            out.pixels[to..to + w as usize * 4]
                .copy_from_slice(&self.pixels[from..from + w as usize * 4]);
        }
        out
    }

    /// Copies `img` in at (x, y); it must fit.
    pub fn paste(&mut self, img: &Rgba, x: u32, y: u32) {
        for r in 0..img.height {
            let to = ((y + r) * self.width + x) as usize * 4;
            let from = (r * img.width) as usize * 4;
            self.pixels[to..to + img.width as usize * 4]
                .copy_from_slice(&img.pixels[from..from + img.width as usize * 4]);
        }
    }
}

/// One image of a pack: what it is, the content hash of the classic image it was made from,
/// and its HD pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackImage {
    pub id: ImageId,
    pub hash: u64,
    pub image: Rgba,
}

/// Who made the pack.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Generator {
    /// The tool, e.g. `u6-hd`.
    pub name: String,
    pub version: String,
    /// What drew the images, e.g. the image model.
    pub engine: String,
    /// The day the pack was written, `YYYY-MM-DD`.
    pub date: String,
}

/// `manifest.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: String,
    pub tile: u32,
    pub scale: u32,
    /// Made from the owner's game art: never exported, hosted or shared (spec 07 Rights).
    pub derived: bool,
    pub generator: Generator,
    /// [`base_digest`] of the index, 16 lower-case hex digits.
    pub base: String,
    /// Number of images.
    pub images: u32,
}

/// One `index.bin` entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: ImageId,
    /// The image has a pixel with alpha < 255.
    pub alpha: bool,
    /// Tiles: the atlas and the cell (column, row); other images: `None`.
    pub cell: Option<(u16, u16, u16)>,
    pub width: u16,
    pub height: u16,
    pub hash: u64,
}

/// A pack as read: its manifest, and every image decoded (tiles cut out of their atlases), in
/// index order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pack {
    pub manifest: Manifest,
    pub images: Vec<PackImage>,
}

impl Pack {
    /// The image for `id`, if the pack has one.
    pub fn get(&self, id: ImageId) -> Option<&PackImage> {
        self.images
            .binary_search_by(|p| p.id.key().cmp(&id.key()))
            .ok()
            .map(|i| &self.images[i])
    }
}

/// Why a pack was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackError {
    /// The zip does not read (u6-intake's reader and its limits).
    Zip(String),
    /// An entry this format does not have, or a name twice.
    Entry(String),
    /// A file the index needs is missing, or one it does not name is present.
    Files(String),
    Manifest(String),
    Index(String),
    /// A limit of the format is passed.
    Limit(String),
    /// An image does not decode, is lossy or animated, or is not the size its entry says.
    Image(String),
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (what, detail) = match self {
            PackError::Zip(d) => ("zip", d),
            PackError::Entry(d) => ("entry", d),
            PackError::Files(d) => ("files", d),
            PackError::Manifest(d) => ("manifest", d),
            PackError::Index(d) => ("index", d),
            PackError::Limit(d) => ("limit", d),
            PackError::Image(d) => ("image", d),
        };
        write!(f, "HD pack refused ({what}): {detail}")
    }
}

impl std::error::Error for PackError {}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv(h: u64, bytes: impl IntoIterator<Item = u8>) -> u64 {
    bytes
        .into_iter()
        .fold(h, |h, b| (h ^ u64::from(b)).wrapping_mul(FNV_PRIME))
}

/// The content hash of a classic image: FNV-1a 64 over `width` and `height` (u16 LE each) and
/// then `pixels`, one byte per pixel, row-major (see the crate doc for what a pixel byte is).
pub fn image_hash(width: u16, height: u16, pixels: impl IntoIterator<Item = u8>) -> u64 {
    let h = fnv(FNV_OFFSET, width.to_le_bytes());
    let h = fnv(h, height.to_le_bytes());
    fnv(h, pixels)
}

/// The content hash of a 16×16 tile record: its colour index per pixel, `0xFF` where it is
/// transparent (`u6_formats::gfx::Tile::{pixels, transparent}`); `None` unless both hold
/// exactly 256 entries.
pub fn tile_hash(pixels: &[u8], transparent: &[bool]) -> Option<u64> {
    let n = (TILE * TILE) as usize;
    (pixels.len() == n && transparent.len() == n).then(|| {
        image_hash(
            TILE as u16,
            TILE as u16,
            pixels
                .iter()
                .zip(transparent)
                .map(|(&p, &t)| if t { 0xFF } else { p }),
        )
    })
}

/// The manifest's `base`: FNV-1a 64 over (kind, letter, id LE, hash LE) of every entry in index
/// order.
pub fn base_digest(entries: &[Entry]) -> u64 {
    entries.iter().fold(FNV_OFFSET, |h, e| {
        let (kind, letter, id) = e.id.key();
        let h = fnv(h, [kind, letter]);
        let h = fnv(h, id.to_le_bytes());
        fnv(h, e.hash.to_le_bytes())
    })
}

/// The size an image of `id` must have, given its entry's size for a bitmap.
fn expected_size(id: ImageId, width: u32, height: u32) -> Result<(u32, u32), PackError> {
    match id {
        ImageId::Tile(r) => {
            if usize::from(r) >= TILE_COUNT {
                return Err(PackError::Index(format!("tile record {r} out of range")));
            }
            Ok((TILE_HD, TILE_HD))
        }
        ImageId::Portrait { letter, .. } => {
            if !letter.is_ascii_lowercase() {
                return Err(PackError::Index("portrait letter not a-z".into()));
            }
            Ok(PORTRAIT_HD)
        }
        ImageId::Bitmap(_) => {
            let ok = width > 0
                && height > 0
                && width.is_multiple_of(SCALE)
                && height.is_multiple_of(SCALE)
                && width / SCALE <= BITMAP_MAX.0
                && height / SCALE <= BITMAP_MAX.1;
            if !ok {
                return Err(PackError::Index(format!(
                    "bitmap {width}x{height} is not 4x a classic size within the screen"
                )));
            }
            Ok((width, height))
        }
    }
}

/// The checks the reader and the writer share: count, order (so no image twice), sizes, cells
/// (in range, none used twice, atlases numbered without a gap), the decoded total. Returns the
/// rows of each atlas.
fn check_entries(entries: &[Entry]) -> Result<Vec<u32>, PackError> {
    if entries.len() > MAX_ENTRIES {
        return Err(PackError::Limit(format!(
            "{} images, at most {MAX_ENTRIES}",
            entries.len()
        )));
    }
    let mut decoded: u64 = 0;
    let mut atlas_rows: Vec<u32> = Vec::new();
    let mut cells: Vec<(u16, u16, u16)> = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        if i > 0 && entries[i - 1].id.key() >= e.id.key() {
            return Err(PackError::Index(format!(
                "entry {i} is not after entry {} (out of order or repeated)",
                i - 1
            )));
        }
        let (w, h) = (u32::from(e.width), u32::from(e.height));
        if expected_size(e.id, w, h)? != (w, h) {
            return Err(PackError::Index(format!(
                "entry {i}: size {w}x{h} is wrong"
            )));
        }
        match (e.id, e.cell) {
            (ImageId::Tile(_), Some((atlas, col, row))) => {
                if u32::from(col) >= ATLAS_CELLS || u32::from(row) >= ATLAS_CELLS {
                    return Err(PackError::Index(format!("entry {i}: cell out of range")));
                }
                // An atlas is named by a tile of it before any later one is.
                if usize::from(atlas) > atlas_rows.len() {
                    return Err(PackError::Index(format!(
                        "entry {i}: atlas {atlas} skips one"
                    )));
                }
                if usize::from(atlas) == atlas_rows.len() {
                    atlas_rows.push(0);
                }
                let rows = &mut atlas_rows[usize::from(atlas)];
                *rows = (*rows).max(u32::from(row) + 1);
                cells.push((atlas, col, row));
            }
            (ImageId::Tile(_), None) => {
                return Err(PackError::Index(format!("entry {i}: tile without a cell")));
            }
            (_, Some(_)) => {
                return Err(PackError::Index(format!("entry {i}: own file with a cell")));
            }
            (_, None) => decoded += u64::from(w) * u64::from(h) * 4,
        }
    }
    cells.sort_unstable();
    if cells.windows(2).any(|w| w[0] == w[1]) {
        return Err(PackError::Index("two tiles in one atlas cell".into()));
    }
    let side = u64::from(ATLAS_CELLS * TILE_HD);
    decoded += atlas_rows
        .iter()
        .map(|&r| side * u64::from(r * TILE_HD) * 4)
        .sum::<u64>();
    if decoded > MAX_DECODED_BYTES {
        return Err(PackError::Limit(format!(
            "{decoded} bytes decoded, at most {MAX_DECODED_BYTES}"
        )));
    }
    Ok(atlas_rows)
}

fn encode_index(entries: &[Entry]) -> Vec<u8> {
    let mut out = Vec::with_capacity(INDEX_HEAD + ENTRY_LEN * entries.len());
    out.extend_from_slice(INDEX_MAGIC);
    out.extend_from_slice(&INDEX_VERSION.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for e in entries {
        let (kind, letter, id) = e.id.key();
        let (atlas, col, row) = e.cell.unwrap_or((OWN_FILE, 0, 0));
        out.extend_from_slice(&[kind, if e.alpha { FLAG_ALPHA } else { 0 }]);
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&[letter, 0]);
        for v in [atlas, col, row, e.width, e.height] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&e.hash.to_le_bytes());
    }
    out
}

fn decode_index(bytes: &[u8]) -> Result<Vec<Entry>, PackError> {
    let bad = |d: &str| PackError::Index(d.to_string());
    if bytes.len() < INDEX_HEAD || &bytes[..8] != INDEX_MAGIC {
        return Err(bad("no index header"));
    }
    let u16_at = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    let u32_at =
        |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    if u32_at(8) != INDEX_VERSION {
        return Err(bad("unknown index version"));
    }
    let count = u32_at(12) as usize;
    if count > MAX_ENTRIES {
        return Err(PackError::Limit(format!(
            "{count} images, at most {MAX_ENTRIES}"
        )));
    }
    if bytes.len() != INDEX_HEAD + ENTRY_LEN * count {
        return Err(bad("index length does not match its count"));
    }
    let mut entries = Vec::with_capacity(count);
    for i in 0..count {
        let at = INDEX_HEAD + ENTRY_LEN * i;
        let (kind, flags, id, letter) = (bytes[at], bytes[at + 1], u16_at(at + 2), bytes[at + 4]);
        if bytes[at + 5] != 0 || flags & !FLAG_ALPHA != 0 {
            return Err(PackError::Index(format!("entry {i}: reserved bits set")));
        }
        let image = match kind {
            1 if letter == 0 => ImageId::Tile(id),
            2 => ImageId::Portrait { letter, item: id },
            3 if letter == 0 => ImageId::Bitmap(
                Role::from_code(id)
                    .ok_or_else(|| PackError::Index(format!("entry {i}: unknown role")))?,
            ),
            _ => return Err(PackError::Index(format!("entry {i}: unknown kind"))),
        };
        let (atlas, col, row) = (u16_at(at + 6), u16_at(at + 8), u16_at(at + 10));
        let cell = match image {
            ImageId::Tile(_) => Some((atlas, col, row)),
            _ if (atlas, col, row) == (OWN_FILE, 0, 0) => None,
            _ => return Err(PackError::Index(format!("entry {i}: own file with a cell"))),
        };
        let mut hash = [0; 8];
        hash.copy_from_slice(&bytes[at + 16..at + 24]);
        entries.push(Entry {
            id: image,
            alpha: flags & FLAG_ALPHA != 0,
            cell,
            width: u16_at(at + 12),
            height: u16_at(at + 14),
            hash: u64::from_le_bytes(hash),
        });
    }
    Ok(entries)
}

fn hex(v: u64) -> String {
    format!("{v:016x}")
}

fn atlas_name(n: usize) -> String {
    format!("atlas-{n}.webp")
}

/// Whether `name` is an entry this format has (its shape; whether it is referenced is checked
/// against the index).
fn known_name(name: &str) -> bool {
    if name == "manifest.json" || name == "index.bin" {
        return true;
    }
    let number = |s: &str| {
        !s.is_empty()
            && s.len() <= 5
            && s.bytes().all(|b| b.is_ascii_digit())
            && (s == "0" || !s.starts_with('0'))
    };
    let stem = |prefix: &str| {
        name.strip_prefix(prefix)
            .and_then(|s| s.strip_suffix(".webp"))
    };
    if let Some(n) = stem("atlas-") {
        return number(n);
    }
    if let Some(rest) = stem("portrait/") {
        let b = rest.as_bytes();
        return b.len() >= 3 && b[0].is_ascii_lowercase() && b[1] == b'-' && number(&rest[2..]);
    }
    if let Some(role) = stem("bitmap/") {
        return Role::from_name(role).is_some();
    }
    false
}

fn encode_webp(img: &Rgba) -> Result<Vec<u8>, PackError> {
    let mut out = Vec::new();
    image_webp::WebPEncoder::new(&mut out)
        .encode(
            &img.pixels,
            img.width,
            img.height,
            image_webp::ColorType::Rgba8,
        )
        .map_err(|e| PackError::Image(e.to_string()))?;
    Ok(out)
}

/// Decodes a lossless still WebP that must be `width × height`, as RGBA.
fn decode_webp(name: &str, bytes: &[u8], width: u32, height: u32) -> Result<Rgba, PackError> {
    let err = |d: String| PackError::Image(format!("{name}: {d}"));
    let mut dec =
        image_webp::WebPDecoder::new(Cursor::new(bytes)).map_err(|e| err(e.to_string()))?;
    if dec.dimensions() != (width, height) {
        let (w, h) = dec.dimensions();
        return Err(err(format!("{w}x{h}, the index says {width}x{height}")));
    }
    if dec.is_animated() || dec.is_lossy() {
        return Err(err("not a lossless still image".into()));
    }
    let channels = if dec.has_alpha() { 4 } else { 3 };
    let n = width as usize * height as usize;
    dec.set_memory_limit(n * 4 + (1 << 20));
    let mut buf = vec![0; n * channels];
    dec.read_image(&mut buf).map_err(|e| err(e.to_string()))?;
    let pixels = if channels == 4 {
        buf
    } else {
        buf.chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect()
    };
    Ok(Rgba {
        width,
        height,
        pixels,
    })
}

/// Writes a pack of `images` (any order). Refuses what the reader would refuse: a repeated
/// image, a wrong size, a limit passed. `derived` says the images were made from the owner's
/// game art (the imagegen tool always passes `true`).
pub fn write_pack(
    images: &[PackImage],
    generator: &Generator,
    derived: bool,
) -> Result<Vec<u8>, PackError> {
    let mut sorted: Vec<&PackImage> = images.iter().collect();
    sorted.sort_by_key(|p| p.id.key());
    let mut entries = Vec::with_capacity(sorted.len());
    let mut tiles = 0usize;
    for p in &sorted {
        let (w, h) = (p.image.width, p.image.height);
        if p.image.pixels.len() != w as usize * h as usize * 4 {
            return Err(PackError::Image(
                "pixel buffer does not match its size".into(),
            ));
        }
        let size = |v: u32| u16::try_from(v).map_err(|_| PackError::Index("too large".into()));
        let cell = matches!(p.id, ImageId::Tile(_)).then(|| {
            let per = (ATLAS_CELLS * ATLAS_CELLS) as usize;
            let (atlas, k) = (tiles / per, (tiles % per) as u32);
            tiles += 1;
            (
                atlas as u16,
                (k % ATLAS_CELLS) as u16,
                (k / ATLAS_CELLS) as u16,
            )
        });
        entries.push(Entry {
            id: p.id,
            alpha: p.image.has_alpha(),
            cell,
            width: size(w)?,
            height: size(h)?,
            hash: p.hash,
        });
    }
    let atlas_rows = check_entries(&entries)?;
    let g = generator;
    if [&g.name, &g.version, &g.engine, &g.date]
        .iter()
        .any(|t| t.len() > MAX_TEXT)
    {
        return Err(PackError::Manifest("generator text too long".into()));
    }
    let manifest = Manifest {
        format: FORMAT.into(),
        tile: TILE,
        scale: SCALE,
        derived,
        generator: generator.clone(),
        base: hex(base_digest(&entries)),
        images: entries.len() as u32,
    };
    let manifest =
        serde_json::to_vec_pretty(&manifest).map_err(|e| PackError::Manifest(e.to_string()))?;
    let mut files: Vec<(String, Vec<u8>)> = vec![
        ("manifest.json".into(), manifest),
        ("index.bin".into(), encode_index(&entries)),
    ];
    let side = ATLAS_CELLS * TILE_HD;
    let mut atlases: Vec<Rgba> = atlas_rows
        .iter()
        .map(|&rows| Rgba::new(side, rows * TILE_HD))
        .collect();
    for (p, e) in sorted.iter().zip(&entries) {
        if let Some((atlas, col, row)) = e.cell {
            atlases[usize::from(atlas)].paste(
                &p.image,
                u32::from(col) * TILE_HD,
                u32::from(row) * TILE_HD,
            );
        }
    }
    for (n, atlas) in atlases.iter().enumerate() {
        files.push((atlas_name(n), encode_webp(atlas)?));
    }
    for p in &sorted {
        if let Some(name) = p.id.file_name() {
            files.push((name, encode_webp(&p.image)?));
        }
    }
    Ok(store::write_zip(&files))
}

/// Reads a pack: every check of the crate doc, then every image decoded. All or nothing.
pub fn read_pack(bytes: &[u8]) -> Result<Pack, PackError> {
    let zip_err = |e: u6_intake::Error| PackError::Zip(e.to_string());
    if !zip::is_zip(bytes) {
        return Err(PackError::Zip("not a zip".into()));
    }
    // The directory first: names and declared sizes are checked before anything is inflated.
    let zip::Directory::Entries(slices) =
        zip::directory(bytes.len() as u64, 0, bytes, |_| true).map_err(zip_err)?
    else {
        return Err(PackError::Zip("directory outside the file".into()));
    };
    // Each name decoded once, into the one map every lookup below uses (sorted by name).
    let mut names: BTreeMap<String, &zip::Slice> = BTreeMap::new();
    for s in &slices {
        let n = s.name();
        if !known_name(&n) {
            return Err(PackError::Entry(format!("unknown entry {n:?}")));
        }
        if names.insert(n, s).is_some() {
            return Err(PackError::Entry("an entry name twice".into()));
        }
    }
    let declared: u64 = slices.iter().map(zip::Slice::unpacked).sum();
    if declared > MAX_FILE_BYTES {
        return Err(PackError::Limit(format!(
            "entries hold {declared} bytes, at most {MAX_FILE_BYTES}"
        )));
    }
    // An entry is inflated only when it is wanted, one at a time; every one is checked for its
    // declared size and its CRC-32 (u6_intake::zip::entry).
    let read = |name: &str, max: u64| -> Result<Vec<u8>, PackError> {
        let s = *names
            .get(name)
            .ok_or_else(|| PackError::Files(format!("no {name}")))?;
        if s.unpacked() > max {
            return Err(PackError::Limit(format!("{name} is larger than it can be")));
        }
        zip::entry(bytes, s).map_err(zip_err)
    };
    let manifest = read("manifest.json", MAX_MANIFEST)?;
    let manifest: Manifest =
        serde_json::from_slice(&manifest).map_err(|e| PackError::Manifest(e.to_string()))?;
    if manifest.format != FORMAT {
        return Err(PackError::Manifest("format is not u6-hdpack/1".into()));
    }
    if manifest.tile != TILE || manifest.scale != SCALE {
        return Err(PackError::Manifest("tile or scale is not 16 / 4".into()));
    }
    let g = &manifest.generator;
    if [&g.name, &g.version, &g.engine, &g.date]
        .iter()
        .any(|t| t.len() > MAX_TEXT)
    {
        return Err(PackError::Manifest("generator text too long".into()));
    }
    let index = read("index.bin", (INDEX_HEAD + ENTRY_LEN * MAX_ENTRIES) as u64)?;
    let entries = decode_index(&index)?;
    let atlas_rows = check_entries(&entries)?;
    if manifest.images as usize != entries.len() {
        return Err(PackError::Manifest(
            "image count differs from the index".into(),
        ));
    }
    if manifest.base != hex(base_digest(&entries)) {
        return Err(PackError::Manifest(
            "base digest differs from the index".into(),
        ));
    }
    // The zip holds exactly what the index names: nothing else is ever inflated.
    let mut want: Vec<String> = vec!["manifest.json".into(), "index.bin".into()];
    want.extend((0..atlas_rows.len()).map(atlas_name));
    want.extend(entries.iter().filter_map(|e| e.id.file_name()));
    want.sort_unstable();
    if let Some(n) = names.keys().find(|n| want.binary_search(n).is_err()) {
        return Err(PackError::Files(format!("{n} is not in the index")));
    }
    if let Some(n) = want.iter().find(|n| !names.contains_key(n.as_str())) {
        return Err(PackError::Files(format!("no {n}")));
    }
    // One atlas decoded at a time: its tiles are cut out and it is dropped before the next,
    // so the peak is the images, one atlas and one entry's bytes.
    let side = ATLAS_CELLS * TILE_HD;
    let mut images: Vec<Option<PackImage>> = vec![None; entries.len()];
    for (n, &rows) in atlas_rows.iter().enumerate() {
        let name = atlas_name(n);
        let data = read(&name, MAX_FILE_BYTES)?;
        let atlas = decode_webp(&name, &data, side, rows * TILE_HD)?;
        drop(data);
        for (i, e) in entries.iter().enumerate() {
            if let Some((a, col, row)) = e.cell
                && usize::from(a) == n
            {
                let (w, h) = (u32::from(e.width), u32::from(e.height));
                let image = atlas.rect(u32::from(col) * TILE_HD, u32::from(row) * TILE_HD, w, h);
                images[i] = Some(PackImage {
                    id: e.id,
                    hash: e.hash,
                    image,
                });
            }
        }
    }
    for (i, e) in entries.iter().enumerate() {
        if e.cell.is_some() {
            continue;
        }
        let name =
            e.id.file_name()
                .ok_or_else(|| PackError::Index("tile without a cell".into()))?;
        let data = read(&name, MAX_FILE_BYTES)?;
        let image = decode_webp(&name, &data, u32::from(e.width), u32::from(e.height))?;
        images[i] = Some(PackImage {
            id: e.id,
            hash: e.hash,
            image,
        });
    }
    let images: Vec<PackImage> = images
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| PackError::Index("an entry without its image".into()))?;
    for (p, e) in images.iter().zip(&entries) {
        if p.image.has_alpha() != e.alpha {
            return Err(PackError::Image(format!(
                "{:?}: alpha differs from its flag",
                e.id
            )));
        }
    }
    Ok(Pack { manifest, images })
}
