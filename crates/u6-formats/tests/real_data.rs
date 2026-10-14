//! Real-data checks against the unpacked Amiga files (formats.md "Measured" and "Proof" lines).
//!
//! Runs only when the data directory exists (`U6_DATA_DIR`, default
//! `/Users/zhugehyuk/Games/ultima6/unpacked`); otherwise each test prints a skip line and passes.
//! The game data is third-party content: it is only read here, never copied into the repo, and
//! the tests print numbers only. No expected value is a number taken from the files: each check
//! compares a parser with this file's own minimal readers of the raw bytes at the offsets the
//! format notes give, or asserts a structural invariant (lengths from headers, bounds, runs,
//! agreement between files).

use std::path::PathBuf;

use u6_formats::animdata::{ANIM_SLOTS, AnimData};
use u6_formats::basetile::BaseTiles;
use u6_formats::bitmap::Bitmap;
use u6_formats::book::Books;
use u6_formats::chunks::Chunks;
use u6_formats::converse::{ConverseLib, ScriptStorage};
use u6_formats::font::{BitFont, WouFont};
use u6_formats::look::LookTable;
use u6_formats::lzw;
use u6_formats::map::WorldMap;
use u6_formats::objblk::{ObjBlock, ObjBlockId, ObjLocation};
use u6_formats::objlist::ObjList;
use u6_formats::portrait::{PORTRAIT_LEN, Portraits};
use u6_formats::schedule::Schedules;
use u6_formats::tileflag::{TILE_COUNT, TileFlags};
use u6_testkit::assert_same;

const DEFAULT_DIR: &str = "/Users/zhugehyuk/Games/ultima6/unpacked";
const D1: &str = "d1/INSTALL";
const D2: &str = "d2/DATA1";
const D3: &str = "d3/DATA2";

fn data_dir() -> Option<PathBuf> {
    let dir =
        std::env::var_os("U6_DATA_DIR").map_or_else(|| PathBuf::from(DEFAULT_DIR), PathBuf::from);
    if dir.is_dir() {
        Some(dir)
    } else {
        println!("skip: {} not found", dir.display());
        None
    }
}

/// Reads `<data>/<disk>/<name>`; `None` (skip) when the data directory is absent.
fn read(disk: &str, name: &str) -> Option<Vec<u8>> {
    let path = data_dir()?.join(disk).join(name);
    Some(std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display())))
}

// The test's own readers of raw bytes; they share no code with the parsers under test.

/// The big-endian word at `off`.
fn be16(d: &[u8], off: usize) -> u16 {
    u16::from_be_bytes([d[off], d[off + 1]])
}

/// The little-endian word at `off`.
fn le16(d: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([d[off], d[off + 1]])
}

/// A packed world position (formats.md §5 bytes 1-3) at the start of `b`: x, y, z.
fn packed(b: &[u8]) -> (u16, u16, u8) {
    let [b1, b2, b3] = [b[0], b[1], b[2]].map(u16::from);
    (
        b1 | (b2 & 3) << 8,
        b2 >> 2 | (b3 & 0xf) << 6,
        (b3 >> 4) as u8,
    )
}

/// True when the basetile table gives object type `obj_n` a tile (type 0 is the empty type).
fn typed(base: &BaseTiles, obj_n: u16) -> bool {
    obj_n == 0 || base.base[usize::from(obj_n)] != 0
}

#[test]
fn objblk_every_block() {
    let Some(bt) = read(D1, "basetile") else {
        return;
    };
    let base = BaseTiles::parse(&bt).unwrap();
    let (mut files, mut total, mut max_obj, mut untyped) = (0usize, 0usize, 0u16, 0usize);
    let mut hist = [0usize; 4];
    let mut surface_on_map = 0usize;
    for id in ObjBlockId::all() {
        let name = id.file_name();
        let data = read(D2, &name).unwrap();
        let b = ObjBlock::parse(&data).unwrap_or_else(|e| panic!("{name}: {e}"));
        // formats.md §5: a BE record count, then 8 bytes per record.
        assert_eq!(
            usize::from(be16(&data, 0)),
            b.objects.len(),
            "{name}: count"
        );
        assert_eq!(b.objects.len() * 8 + 2, data.len(), "{name}");
        assert_same!(b.to_bytes(), data, "{name} re-encodes");
        for o in &b.objects {
            assert!(o.obj_n <= 1023);
            max_obj = max_obj.max(o.obj_n);
            untyped += usize::from(!typed(&base, o.obj_n));
            let loc = o.location();
            hist[loc as usize] += 1;
            if loc != ObjLocation::Map {
                continue;
            }
            match id {
                ObjBlockId::Surface { sx, sy } => {
                    surface_on_map += 1;
                    assert!(o.pos.z == 0, "{name}: a surface record off level 0");
                    assert!(
                        usize::from(o.pos.x) / 128 == usize::from(sx)
                            && usize::from(o.pos.y) / 128 == usize::from(sy),
                        "{name}: a map record outside its superchunk"
                    );
                }
                ObjBlockId::Dungeon { z } => {
                    assert!(o.pos.z == z, "{name}: a map record off its level");
                    assert!(o.pos.x < 256 && o.pos.y < 256);
                }
            }
        }
        total += b.objects.len();
        files += 1;
    }
    println!(
        "objblk: {files} files, {total} records, max obj_n {max_obj}; map {} container {} \
         inventory {} readied {}; surface on-map {surface_on_map}; without a base tile {untyped}",
        hist[0], hist[1], hist[2], hist[3]
    );
    // formats.md §5: read in the Amiga's byte order every record names a type that basetile
    // gives a tile (the other byte order leaves many past the used types).
    assert_eq!(untyped, 0, "records whose type has no base tile");
}

#[test]
fn map_and_chunks() {
    let (Some(m), Some(c)) = (read(D2, "map"), read(D2, "chunks")) else {
        return;
    };
    let map = WorldMap::parse(&m).unwrap();
    let refs: Vec<u16> = map.all_refs().collect();
    let chunks = Chunks::parse(&c).unwrap();
    // formats.md §4.1: 8 × 8 tile bytes per chunk; the map packs two 12-bit chunk refs in 3 bytes.
    assert_eq!(chunks.chunks.len(), c.len() / 64);
    assert_eq!(refs.len(), m.len() / 3 * 2);
    assert!(refs.iter().all(|&r| usize::from(r) < chunks.chunks.len()));
    println!(
        "map: {} refs (surface {}, dungeon {}), max ref {}; chunks: {}",
        refs.len(),
        map.surface.len(),
        map.dungeons.iter().map(Vec::len).sum::<usize>(),
        refs.iter().max().unwrap(),
        chunks.chunks.len()
    );
}

#[test]
fn objlist_proof_values() {
    let (Some(d), Some(bt)) = (read(D2, "objlist"), read(D1, "basetile")) else {
        return;
    };
    let base = BaseTiles::parse(&bt).unwrap();
    let o = ObjList::parse(&d).unwrap();
    assert_same!(o.to_bytes(), d, "objlist re-encodes byte-identically");
    // formats.md §6, read at its offsets: the packed positions at 0x100, the BE type words at
    // 0x400 (type in the low 10 bits, frame above), the BE experience words at 0xC00.
    let n = o.actors.len();
    let pos: Vec<_> = o
        .actors
        .iter()
        .map(|a| (a.pos.x, a.pos.y, a.pos.z))
        .collect();
    let raw_pos: Vec<_> = (0..n).map(|a| packed(&d[0x100 + 3 * a..])).collect();
    assert_same!(pos, raw_pos, "the actor positions at 0x100");
    let types: Vec<_> = o.actors.iter().map(|a| (a.obj_n, a.frame)).collect();
    let raw_types: Vec<_> = (0..n)
        .map(|a| {
            let w = be16(&d, 0x400 + 2 * a);
            (w & 0x3ff, (w >> 10) as u8)
        })
        .collect();
    assert_same!(types, raw_types, "the actor type words at 0x400");
    let exp: Vec<u16> = o.actors.iter().map(|a| a.exp).collect();
    let raw_exp: Vec<u16> = (0..n).map(|a| be16(&d, 0xC00 + 2 * a)).collect();
    assert_same!(exp, raw_exp, "the experience words at 0xC00");
    // The clock: minute, hour, day, month at 0x1BF3, the BE year at 0x1BF7.
    let c = o.clock;
    assert_same!(
        [c.minute, c.hour, c.day, c.month],
        d[0x1BF3..0x1BF7],
        "the clock at 0x1BF3"
    );
    assert_same!([c.year], [be16(&d, 0x1BF7)], "the year at 0x1BF7");
    // Eight moonstones of three BE words (x, y, z) at 0x1C1B, on the 10-bit world.
    let raw_moon: Vec<[u16; 3]> = (0..8)
        .map(|m| std::array::from_fn(|k| be16(&d, 0x1C1B + 6 * m + 2 * k)))
        .collect();
    assert_same!(o.moonstones, raw_moon, "the moonstones at 0x1C1B");
    assert!(o.moonstones.iter().all(|m| m[0] <= 1023 && m[1] <= 1023));
    // The names (the user's text): the four at 0xF00 in 14-byte NUL-padded slots, read here
    // straight from the file, one word of letters each.
    let raw_names: Vec<String> = (0..4)
        .map(|i| {
            let slot = &d[0xF00 + 14 * i..][..14];
            let len = slot.iter().position(|&b| b == 0).unwrap_or(14);
            slot[..len].iter().map(|&b| char::from(b)).collect()
        })
        .collect();
    assert_same!(o.party.names[..4], raw_names, "the party names at 0xF00");
    assert!(
        raw_names
            .iter()
            .all(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_alphabetic())),
        "four names of letters"
    );
    let raw_1bfb: Vec<u16> = (0..4).map(|i| be16(&d, 0x1BFB + 2 * i)).collect();
    assert_same!(o.unknown_1bfb, raw_1bfb, "the words at 0x1BFB");
    // formats.md §6 proof: read in the Amiga's byte order every actor's type has a base tile.
    let bad_obj = o.actors.iter().filter(|a| !typed(&base, a.obj_n)).count();
    println!(
        "objlist: {} bytes, actors whose type has no base tile: {bad_obj}, party size {}, \
         year {}, karma {}, avatar portrait {}",
        d.len(),
        o.party.size,
        o.clock.year,
        o.player.karma,
        o.player.avatar_portrait
    );
    assert_eq!(bad_obj, 0);
}

#[test]
fn schedule_matches_objlist() {
    let (Some(s), Some(ol)) = (read(D1, "schedule"), read(D2, "objlist")) else {
        return;
    };
    let sched = Schedules::parse(&s).unwrap();
    // formats.md §4.3: BE first[256] at 0, the BE record count at 512, 5-byte records from 514
    // (byte 0 = hour | day of week << 5, byte 1 = worktype, bytes 2..5 a packed position).
    let total = usize::from(be16(&s, 512));
    assert_eq!(sched.records.len(), total, "the record count at 512");
    assert_eq!(s.len(), 514 + 5 * total);
    let first: Vec<u16> = (0..256).map(|a| be16(&s, 2 * a)).collect();
    assert_same!(sched.first, first, "first[] at 0");
    let got: Vec<_> = sched
        .records
        .iter()
        .map(|r| {
            let p = (r.pos.x, r.pos.y, r.pos.z);
            (r.hour | r.day_of_week << 5, r.worktype, p)
        })
        .collect();
    let want: Vec<_> = (0..total)
        .map(|i| {
            let b = &s[514 + 5 * i..];
            (b[0], b[1], packed(&b[2..]))
        })
        .collect();
    assert_same!(got, want, "the records at 514");
    // Across the files: the first actor with a schedule stands, in the shipped objlist, on the
    // place of its first record.
    let objlist = ObjList::parse(&ol).unwrap();
    let a = (0..256)
        .find(|&a| !sched.for_actor(a).is_empty())
        .expect("an actor with a schedule");
    assert!(
        objlist.actors[a].pos == sched.for_actor(a)[0].pos,
        "actor {a}: the objlist position is its first schedule place"
    );
    assert!(
        sched.first.windows(2).all(|w| w[0] <= w[1]),
        "first[] non-decreasing"
    );
    let per_actor: usize = (0..256).map(|a| sched.for_actor(a).len()).sum();
    let with = (0..256).filter(|&a| !sched.for_actor(a).is_empty()).count();
    println!(
        "schedule: {} records, {per_actor} assigned via first[], {with} actors with a schedule",
        sched.records.len()
    );
}

#[test]
fn tile_tables() {
    let (Some(tf), Some(bt), Some(ad)) = (
        read(D1, "tileflag"),
        read(D1, "basetile"),
        read(D1, "animdata"),
    ) else {
        return;
    };
    let flags = TileFlags::parse(&tf).unwrap();
    // formats.md §4.2: one weight byte per object type at 0x1000..0x1400.
    assert_same!(flags.weight, tf[0x1000..0x1400], "the weights at 0x1000");
    let base = BaseTiles::parse(&bt).unwrap();
    let raw: Vec<u16> = (0..bt.len() / 2).map(|i| be16(&bt, 2 * i)).collect();
    assert_same!(base.base, raw, "the base tiles, BE words");
    // The used types: base tiles non-decreasing from type 0 up to the last type with one, all of
    // them tiles of the 2048 the game draws.
    let last = raw
        .iter()
        .rposition(|&b| b != 0)
        .expect("a type with a tile");
    assert!(base.base[..=last].windows(2).all(|w| w[0] <= w[1]));
    let max = *base.base.iter().max().unwrap();
    assert!(usize::from(max) < TILE_COUNT);
    let anim = AnimData::parse(&ad).unwrap();
    // The BE count at 0, the BE tiles to animate at 2, their BE first frames at 66.
    assert_same!([anim.count], [be16(&ad, 0)], "the animation count at 0");
    let n = usize::from(anim.count);
    assert!(n <= ANIM_SLOTS);
    let slots: Vec<_> = anim
        .active()
        .iter()
        .map(|e| (e.tile_to_animate, e.first_anim_frame))
        .collect();
    let raw_slots: Vec<_> = (0..n)
        .map(|i| (be16(&ad, 2 + 2 * i), be16(&ad, 66 + 2 * i)))
        .collect();
    assert_same!(
        slots,
        raw_slots,
        "the animated tiles and their first frames"
    );
    // Each slot animates a tile with tiles: every frame it can show is one of the 2048.
    for (i, e) in anim.active().iter().enumerate() {
        let frames = (0..=255u32).map(|c| e.frame_tile(c));
        let ok = usize::from(e.tile_to_animate) < TILE_COUNT
            && frames.map(usize::from).all(|t| t < TILE_COUNT);
        assert!(ok, "animation slot {i} stays inside the tiles");
    }
    let water = (0..TILE_COUNT).filter(|&t| flags.is_water(t)).count();
    let blocked = (0..TILE_COUNT).filter(|&t| flags.is_impassable(t)).count();
    println!(
        "tileflag: {water} water, {blocked} impassable tiles; basetile max {max}, last used type \
         {last}; animdata n {}",
        anim.count
    );
}

#[test]
fn look_lzd() {
    let Some(f) = read(D2, "look.lzd") else {
        return;
    };
    let declared = lzw::declared_size(&f).unwrap() as usize;
    let decoded = lzw::decode(&f).unwrap();
    assert_eq!(decoded.len(), declared);
    let t = LookTable::parse(&decoded).unwrap();
    assert!(!t.entries.is_empty());
    for e in &t.entries[..=10] {
        assert!(
            !e.text.is_empty() && e.text.bytes().all(|b| (0x20..0x7f).contains(&b)),
            "entry for tile {} is printable ASCII",
            e.tile
        );
    }
    let terminator_at = decoded.len() - t.trailer.len();
    let reencoded = lzw::encode(&decoded).unwrap();
    assert_same!(
        lzw::decode(&reencoded).unwrap(),
        decoded,
        "look.lzd round trip"
    );
    println!(
        "look.lzd: stored {}, decoded {} (= header), {} entries, last tile {}, terminator at {}, \
         re-encode byte-identical: {}",
        f.len(),
        decoded.len(),
        t.entries.len(),
        t.entries.last().unwrap().tile,
        terminator_at,
        reencoded == f
    );
}

/// Item count a U6Lib table of `width`-byte little-endian offsets implies (the table ends at the
/// smallest non-zero offset; a u32 entry keeps its offset in the low 24 bits), computed
/// independently of the parser.
fn implied_items(d: &[u8], width: usize) -> usize {
    let mut min = d.len();
    let mut i = 0;
    while (i + 1) * width <= min {
        let off = match width {
            2 => usize::from(le16(d, i * 2)),
            _ => u32::from_le_bytes(d[i * 4..i * 4 + 4].try_into().unwrap()) as usize & 0xFF_FFFF,
        };
        if off != 0 {
            min = min.min(off);
        }
        i += 1;
    }
    min / width
}

#[test]
fn converse_libs() {
    let (Some(a), Some(b)) = (read(D3, "converse.a"), read(D3, "converse.b")) else {
        return;
    };
    for (name, d) in [("converse.a", a), ("converse.b", b)] {
        let c = ConverseLib::parse(&d).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(c.scripts.len(), implied_items(&d, 4), "{name}");
        let raw = c
            .scripts
            .iter()
            .flatten()
            .filter(|s| s.storage == ScriptStorage::Raw)
            .count();
        let bytes: usize = c.scripts.iter().flatten().map(|s| s.bytecode.len()).sum();
        println!(
            "{name}: {} items, {} non-empty ({} LZW, {raw} raw), {bytes} script bytes",
            c.scripts.len(),
            c.non_empty(),
            c.non_empty() - raw
        );
    }
}

#[test]
fn portrait_libs() {
    let Some(_) = data_dir() else { return };
    for name in ["portrait.a", "portrait.b", "portrait.z"] {
        let d = read(D3, name).unwrap();
        let p = Portraits::parse(&d).unwrap_or_else(|e| panic!("{name}: {e}"));
        let imgs: Vec<_> = p.images.iter().flatten().collect();
        assert!(imgs.iter().all(|i| i.pixels.len() == PORTRAIT_LEN));
        let max = imgs
            .iter()
            .flat_map(|i| i.pixels.iter())
            .max()
            .copied()
            .unwrap_or(0);
        println!(
            "{name}: {} items, {} images of 56x64, max palette index {max}",
            p.images.len(),
            imgs.len()
        );
    }
}

#[test]
fn book_dat() {
    let Some(d) = read(D3, "book.dat") else {
        return;
    };
    let b = Books::parse(&d).unwrap();
    let present = b.texts.iter().flatten().count();
    println!("book.dat: {} items, {present} texts", b.texts.len());
    // formats.md §2: book.dat is a U6Lib of u16 offsets.
    assert_eq!(b.texts.len(), implied_items(&d, 2));
}

#[test]
fn fonts() {
    let (Some(ch), Some(set)) = (read(D1, "u6.ch"), read(D1, "u6.set")) else {
        return;
    };
    let f = BitFont::parse(&ch).unwrap();
    // The glyphs (the user's art): 'A' is the 8 row bytes at 8 × 0x41, read here straight from
    // the file, and has ink; the space has none.
    let a = usize::from(b'A');
    assert_same!(f.glyphs[a], ch[8 * a..8 * a + 8], "glyph 'A' at 8 x 0x41");
    assert!(f.glyphs[a].iter().any(|&row| row != 0), "glyph 'A' has ink");
    assert!(
        f.glyphs[usize::from(b' ')].iter().all(|&row| row == 0),
        "the space is blank"
    );
    let w = WouFont::from_lzw(&set).unwrap();
    // formats.md §4.5, read from the decoded font: the height at 0, the ink at 2, per glyph c the
    // width at 4 + c and its offset (low byte at 0x104 + c, high at 0x204 + c) of width × height
    // pixel bytes.
    let raw = lzw::decode(&set).unwrap();
    assert_same!(
        [w.height, w.ink],
        [raw[0], raw[2]],
        "the height and the ink"
    );
    for (c, g) in w.glyphs.iter().enumerate() {
        let off = usize::from(u16::from_le_bytes([raw[0x104 + c], raw[0x204 + c]]));
        let n = usize::from(raw[4 + c]) * usize::from(raw[0]);
        assert_same!([g.width], [raw[4 + c]], "the width of glyph {c}");
        assert_same!(g.pixels, raw[off..off + n], "the pixels of glyph {c}");
    }
    let end = w
        .glyphs
        .iter()
        .map(|g| usize::from(g.offset) + g.pixels.len())
        .max()
        .unwrap();
    println!(
        "u6.ch: {} glyphs; u6.set: height {}, ink {}, max glyph end {end}, decoded {}",
        f.glyphs.len(),
        w.height,
        w.ink,
        w.raw.len()
    );
    assert!(end <= w.raw.len(), "every glyph inside the decoded font");
}

#[test]
fn bitmaps() {
    let Some(_) = data_dir() else { return };
    for (disk, name) in [
        (D1, "paper.bmp"),
        (D3, "newmagic.bmp"),
        (D3, "worldmap.bmp"),
    ] {
        let file = read(disk, name).unwrap();
        let b = Bitmap::from_lzw(&file).unwrap();
        // formats.md §4.6, read from the decoded stream: LE width and height, then the pixels.
        let raw = lzw::decode(&file).unwrap();
        let (w, h) = (le16(&raw, 0), le16(&raw, 2));
        assert!((b.width, b.height) == (w, h), "{name}: the size words");
        assert_eq!(raw.len(), 4 + usize::from(w) * usize::from(h), "{name}");
        assert_same!(b.pixels, raw[4..], "the pixels of {name}");
        println!("{name}: {}x{}", b.width, b.height);
    }
}

/// Every whole-file LZW file decodes to its header size (formats.md §1 table and §8.2 `.new`).
#[test]
fn whole_file_lzw() {
    let Some(dir) = data_dir() else { return };
    let mut names: Vec<(&str, String)> = [
        (D2, "look.lzd"),
        (D1, "paper.bmp"),
        (D3, "newmagic.bmp"),
        (D3, "worldmap.bmp"),
        (D1, "u6.set"),
        (D1, "INTRO.PTR"),
        (D2, "masktype.ega"),
        (D1, "STARPOS.DAT"),
        (D2, "end.new"),
    ]
    .iter()
    .map(|&(d, n)| (d, n.to_string()))
    .collect();
    let mut news: Vec<String> = std::fs::read_dir(dir.join(D1))
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.to_ascii_lowercase().ends_with(".new"))
        .collect();
    news.sort();
    names.extend(news.into_iter().map(|n| (D1, n)));
    let mut identical = 0;
    for (disk, name) in &names {
        let f = read(disk, name).unwrap();
        let out = lzw::decode(&f).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(out.len() as u32, lzw::declared_size(&f).unwrap(), "{name}");
        let re = lzw::encode(&out).unwrap();
        assert_same!(lzw::decode(&re).unwrap(), out, "{name} round trip");
        identical += usize::from(re == f);
        println!(
            "lzw {name}: stored {} decoded {} re-encode identical {}",
            f.len(),
            out.len(),
            re == f
        );
    }
    println!(
        "lzw: {} files decoded, {identical} re-encode byte-identical",
        names.len()
    );
}
