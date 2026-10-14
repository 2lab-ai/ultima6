//! Real-data checks against the unpacked Amiga files (formats.md "Measured" and "Proof" lines).
//!
//! Runs only when the data directory exists (`U6_DATA_DIR`, default
//! `/Users/zhugehyuk/Games/ultima6/unpacked`); otherwise each test prints a skip line and passes.
//! The game data is third-party content: it is only read here, never copied into the repo, and
//! the tests print numbers only.

use std::path::PathBuf;

use u6_formats::animdata::AnimData;
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
use u6_formats::tileflag::TileFlags;

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

#[test]
fn objblk_every_block() {
    let Some(_) = data_dir() else { return };
    let (mut total, mut max_obj) = (0usize, 0u16);
    let mut hist = [0usize; 4];
    let mut surface_on_map = 0usize;
    for id in ObjBlockId::all() {
        let data = read(D2, &id.file_name()).unwrap();
        let b = ObjBlock::parse(&data).unwrap_or_else(|e| panic!("{}: {e}", id.file_name()));
        assert_eq!(b.objects.len() * 8 + 2, data.len(), "{}", id.file_name());
        assert_eq!(b.to_bytes(), data, "{} re-encodes", id.file_name());
        for o in &b.objects {
            assert!(o.obj_n <= 1023);
            max_obj = max_obj.max(o.obj_n);
            let loc = o.location();
            hist[loc as usize] += 1;
            if loc != ObjLocation::Map {
                continue;
            }
            match id {
                ObjBlockId::Surface { sx, sy } => {
                    surface_on_map += 1;
                    assert_eq!(o.pos.z, 0, "{}", id.file_name());
                    assert_eq!(
                        usize::from(o.pos.x) / 128,
                        usize::from(sx),
                        "{}",
                        id.file_name()
                    );
                    assert_eq!(
                        usize::from(o.pos.y) / 128,
                        usize::from(sy),
                        "{}",
                        id.file_name()
                    );
                }
                ObjBlockId::Dungeon { z } => {
                    assert_eq!(o.pos.z, z, "{}", id.file_name());
                    assert!(o.pos.x < 256 && o.pos.y < 256);
                }
            }
        }
        total += b.objects.len();
    }
    println!(
        "objblk: 69 files, {total} records, max obj_n {max_obj}; map {} container {} inventory {} \
         readied {}; surface on-map {surface_on_map}",
        hist[0], hist[1], hist[2], hist[3]
    );
    assert!(
        max_obj <= 431,
        "formats.md §5: every obj_n <= 431, got {max_obj}"
    );
}

#[test]
fn map_and_chunks() {
    let (Some(m), Some(c)) = (read(D2, "map"), read(D2, "chunks")) else {
        return;
    };
    let map = WorldMap::parse(&m).unwrap();
    let refs: Vec<u16> = map.all_refs().collect();
    assert!(refs.iter().all(|&r| r <= 1023));
    let chunks = Chunks::parse(&c).unwrap();
    assert_eq!(chunks.chunks.len(), 1024);
    println!(
        "map: {} refs (surface {}, dungeon {}), max ref {}; chunks: {}",
        refs.len(),
        map.surface.len(),
        map.dungeons.iter().map(Vec::len).sum::<usize>(),
        refs.iter().max().unwrap(),
        chunks.chunks.len()
    );
    assert_eq!(refs.len(), 21504);
}

#[test]
fn objlist_proof_values() {
    let Some(d) = read(D2, "objlist") else { return };
    let o = ObjList::parse(&d).unwrap();
    assert_eq!(o.to_bytes(), d, "objlist re-encodes byte-identically");
    let bad_obj = o.actors.iter().filter(|a| a.obj_n > 431).count();
    let a1 = o.actors[1];
    assert_eq!((a1.pos.x, a1.pos.y, a1.pos.z, a1.obj_n), (307, 352, 0, 410));
    let exp: Vec<u16> = o.actors[2..5].iter().map(|a| a.exp).collect();
    assert_eq!(exp, [374, 373, 372]);
    assert_eq!(o.clock.year, 161);
    assert_eq!(
        (o.clock.minute, o.clock.hour, o.clock.day, o.clock.month),
        (0, 8, 4, 7)
    );
    assert_eq!(o.moonstones[0], [935, 262, 0]);
    assert!(o.moonstones.iter().all(|m| m[0] <= 1023 && m[1] <= 1023));
    assert_eq!(&o.party.names[..4], ["Avatar", "Dupre", "Shamino", "Iolo"]);
    assert_eq!(o.unknown_1bfb, [0x0007, 0x00c5, 0x00e2, 0x0000]);
    println!(
        "objlist: {} bytes, actors with obj_n > 431: {bad_obj}, party size {}, year {}, \
         karma {}, avatar portrait {}",
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
    let r = sched.records[0];
    assert_eq!(
        (r.hour, r.worktype, r.pos.x, r.pos.y, r.pos.z),
        (8, 0x92, 307, 348, 0)
    );
    let objlist = ObjList::parse(&ol).unwrap();
    assert_eq!(
        objlist.actors[5].pos, r.pos,
        "record 0 = actor 5's objlist position"
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
    assert_eq!(sched.records.len(), 563);
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
    assert_eq!(
        &flags.weight[..8],
        &[0x00, 0x04, 0x0c, 0x18, 0x1b, 0x0a, 0x0f, 0x1a]
    );
    let base = BaseTiles::parse(&bt).unwrap();
    assert_eq!(&base.base[..4], &[0, 512, 513, 514]);
    let max = *base.base.iter().max().unwrap();
    assert_eq!(max, 2040);
    assert!(base.base[..=690].windows(2).all(|w| w[0] <= w[1]));
    assert!(base.base[691..].iter().all(|&b| b == 0));
    let anim = AnimData::parse(&ad).unwrap();
    assert_eq!(anim.count, 29);
    let tiles: Vec<u16> = anim.active().iter().map(|e| e.tile_to_animate).collect();
    for t in [862, 1009, 1020] {
        assert!(tiles.contains(&t), "animdata animates tile {t}");
    }
    let water = (0..2048).filter(|&t| flags.is_water(t)).count();
    let blocked = (0..2048).filter(|&t| flags.is_impassable(t)).count();
    println!(
        "tileflag: {water} water, {blocked} impassable tiles; basetile max {max}; animdata n {}",
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
    assert_eq!(declared, 5914);
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
    assert_eq!(lzw::decode(&reencoded).unwrap(), decoded);
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

/// Item count a u32 U6Lib table implies, computed independently of the parser.
fn implied_u32_items(d: &[u8]) -> usize {
    let mut min = d.len();
    let mut i = 0;
    while (i + 1) * 4 <= min {
        let off = u32::from_le_bytes(d[i * 4..i * 4 + 4].try_into().unwrap()) as usize & 0xFF_FFFF;
        if off != 0 {
            min = min.min(off);
        }
        i += 1;
    }
    min / 4
}

#[test]
fn converse_libs() {
    let (Some(a), Some(b)) = (read(D3, "converse.a"), read(D3, "converse.b")) else {
        return;
    };
    for (name, d) in [("converse.a", a), ("converse.b", b)] {
        let c = ConverseLib::parse(&d).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(c.scripts.len(), implied_u32_items(&d), "{name}");
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
    assert_eq!(b.texts.len(), 128);
}

#[test]
fn fonts() {
    let (Some(ch), Some(set)) = (read(D1, "u6.ch"), read(D1, "u6.set")) else {
        return;
    };
    let f = BitFont::parse(&ch).unwrap();
    assert_eq!(
        f.glyphs[usize::from(b'A')],
        [0x1e, 0x36, 0x66, 0x7e, 0x66, 0x66, 0xc6, 0x00]
    );
    let w = WouFont::from_lzw(&set).unwrap();
    assert_eq!((w.height, w.ink), (8, 15));
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
    assert_eq!(end, 7004);
}

#[test]
fn bitmaps() {
    let Some(_) = data_dir() else { return };
    for (disk, name, w, h) in [
        (D1, "paper.bmp", 320, 200),
        (D3, "newmagic.bmp", 144, 82),
        (D3, "worldmap.bmp", 128, 128),
    ] {
        let b = Bitmap::from_lzw(&read(disk, name).unwrap()).unwrap();
        assert_eq!((b.width, b.height), (w, h), "{name}");
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
        assert_eq!(lzw::decode(&re).unwrap(), out, "{name} round trip");
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
