//! Fuzzing the strict reader (ported from the whole-unit review's probes; synthetic images
//! only): no input panics it; a flipped bit of a written pack never loads with other pixels
//! (every entry's CRC-32 is checked); odd names, duplicates and WebP variants are refused.

use std::panic::{AssertUnwindSafe, catch_unwind};

use u6_hdpack::*;
use u6_intake::zip;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn generator() -> Generator {
    Generator {
        name: "t".into(),
        version: "0".into(),
        engine: "none".into(),
        date: "2026-10-16".into(),
    }
}

fn img(w: u32, h: u32, seed: u8, alpha: bool) -> Rgba {
    let mut r = Rgba::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let a = if alpha && x < 9 && y < 9 { 0 } else { 255 };
            r.pixels[i..i + 4].copy_from_slice(&[
                (x as u8).wrapping_mul(seed),
                (y as u8) ^ seed,
                seed,
                a,
            ]);
        }
    }
    r
}

fn sample() -> Vec<PackImage> {
    let mut v: Vec<PackImage> = (0..6u16)
        .map(|r| PackImage {
            id: ImageId::Tile(r * 3),
            hash: u64::from(r),
            image: img(TILE_HD, TILE_HD, r as u8 + 1, r % 2 == 0),
        })
        .collect();
    v.push(PackImage {
        id: ImageId::Portrait {
            letter: b'a',
            item: 2,
        },
        hash: 9,
        image: img(PORTRAIT_HD.0, PORTRAIT_HD.1, 7, false),
    });
    v.push(PackImage {
        id: ImageId::Bitmap(Role::Paper),
        hash: 10,
        image: img(16 * SCALE, 8 * SCALE, 3, true),
    });
    v
}

/// A stored zip of `files` with their CRC-32s (so entries can be changed freely).
fn write_zip(files: &[(String, Vec<u8>)], flags: u16) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let offset = out.len() as u32;
        let crc = crc32fast::hash(data);
        let len = data.len() as u32;
        let common = |o: &mut Vec<u8>| {
            for v in [20u16, flags, 0, 0, 0x21] {
                o.extend_from_slice(&v.to_le_bytes());
            }
            for v in [crc, len, len] {
                o.extend_from_slice(&v.to_le_bytes());
            }
            o.extend_from_slice(&(name.len() as u16).to_le_bytes());
            o.extend_from_slice(&0u16.to_le_bytes());
        };
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        common(&mut out);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        common(&mut central);
        central.extend_from_slice(&[0; 10]);
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let at = out.len() as u32;
    let size = central.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    let n = files.len() as u16;
    for v in [0u16, 0, n, n] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&at.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn read(bytes: &[u8]) -> Result<Result<Pack, PackError>, ()> {
    catch_unwind(AssertUnwindSafe(|| read_pack(bytes))).map_err(|_| ())
}

fn mutate(rng: &mut Rng, b: &mut Vec<u8>) {
    for _ in 0..1 + rng.below(6) {
        if b.is_empty() {
            return;
        }
        match rng.below(6) {
            0 | 1 => {
                let i = rng.below(b.len());
                b[i] ^= 1 << rng.below(8);
            }
            2 => {
                let i = rng.below(b.len());
                b[i] = [0, 0xff, 0x7f, 0x80, 1][rng.below(5)];
            }
            3 => {
                let n = rng.below(b.len());
                b.truncate(n);
            }
            4 => {
                let i = rng.below(b.len());
                for _ in 0..rng.below(16) + 1 {
                    b.insert(i, rng.next() as u8);
                }
            }
            _ => {
                // a random little-endian u32 at a random place (sizes, offsets, dims)
                if b.len() > 4 {
                    let i = rng.below(b.len() - 4);
                    let v = [0u32, 1, 0xffff_ffff, 0x7fff_ffff, 0x3fff, 0x4000, 128][rng.below(7)];
                    b[i..i + 4].copy_from_slice(&v.to_le_bytes());
                }
            }
        }
    }
}

#[test]
fn whole_pack_mutation_never_panics() {
    let pack = write_pack(&sample(), &generator(), true).unwrap();
    assert!(read_pack(&pack).is_ok());
    let mut rng = Rng(0x1234_5678_9abc_def1);
    let (mut ok, mut err, mut panics) = (0, 0, 0);
    for _ in 0..4000 {
        let mut b = pack.clone();
        mutate(&mut rng, &mut b);
        match read(&b) {
            Ok(Ok(_)) => ok += 1,
            Ok(Err(_)) => err += 1,
            Err(()) => panics += 1,
        }
    }
    println!("whole-pack mutation: ok {ok}, err {err}, panics {panics}");
    assert_eq!(panics, 0);
}

#[test]
fn webp_entry_mutation_never_panics() {
    let pack = write_pack(&sample(), &generator(), true).unwrap();
    let files = zip::read_zip(&pack, |_| true).unwrap();
    let webps: Vec<usize> = files
        .iter()
        .enumerate()
        .filter(|(_, (n, _))| n.ends_with(".webp"))
        .map(|(i, _)| i)
        .collect();
    let mut rng = Rng(0xdead_beef_cafe_f00d);
    let (mut ok, mut err, mut panics) = (0, 0, 0);
    for _ in 0..6000 {
        let mut f = files.clone();
        let at = webps[rng.below(webps.len())];
        mutate(&mut rng, &mut f[at].1);
        let b = write_zip(&f, 0);
        match read(&b) {
            Ok(Ok(_)) => ok += 1,
            Ok(Err(_)) => err += 1,
            Err(()) => panics += 1,
        }
    }
    println!("webp mutation: ok {ok}, err {err}, panics {panics}");
    assert_eq!(panics, 0);
}

fn chunk(tag: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = tag.to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
    out
}

fn riff(chunks: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = chunks.concat();
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(4 + body.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WEBP");
    out.extend_from_slice(&body);
    out
}

fn u24(v: u32) -> [u8; 3] {
    [v as u8, (v >> 8) as u8, (v >> 16) as u8]
}

/// The VP8L chunk body of a lossless still of `image` (as the crate encodes it).
fn vp8l_body(image: &Rgba) -> Vec<u8> {
    let mut out = Vec::new();
    image_webp::WebPEncoder::new(&mut out)
        .encode(
            &image.pixels,
            image.width,
            image.height,
            image_webp::ColorType::Rgba8,
        )
        .unwrap();
    // RIFF(12) + "VP8L" + len
    assert_eq!(&out[12..16], b"VP8L");
    let len = u32::from_le_bytes(out[16..20].try_into().unwrap()) as usize;
    out[20..20 + len].to_vec()
}

fn replace_entry(pack: &[u8], name: &str, data: Vec<u8>) -> Vec<u8> {
    let mut files = zip::read_zip(pack, |_| true).unwrap();
    let at = files.iter().position(|(n, _)| n == name).unwrap();
    files[at].1 = data;
    write_zip(&files, 0)
}

#[test]
fn animated_and_extended_webp() {
    let images = sample();
    let pack = write_pack(&images, &generator(), true).unwrap();
    let p = images
        .iter()
        .find(|p| matches!(p.id, ImageId::Portrait { .. }))
        .unwrap();
    let (w, h) = (p.image.width, p.image.height);
    let body = vp8l_body(&p.image);
    // VP8X still with the VP8L: lossless, accepted (same pixels).
    let mut x = vec![0u8]; // no alpha flag (the image is opaque)
    x.extend_from_slice(&[0; 3]);
    x.extend_from_slice(&u24(w - 1));
    x.extend_from_slice(&u24(h - 1));
    let still = riff(&[chunk(b"VP8X", &x), chunk(b"VP8L", &body)]);
    let r = read_pack(&replace_entry(&pack, "portrait/a-2.webp", still)).unwrap();
    assert_eq!(
        r.images,
        read_pack(&pack).unwrap().images,
        "a VP8X still is the same image"
    );
    // Animated: VP8X animation flag, ANIM, two ANMF frames of VP8L.
    let mut xa = vec![0b0000_0010u8];
    xa.extend_from_slice(&[0; 3]);
    xa.extend_from_slice(&u24(w - 1));
    xa.extend_from_slice(&u24(h - 1));
    let anim = chunk(b"ANIM", &[0, 0, 0, 0, 0, 0]);
    let mut f = Vec::new();
    f.extend_from_slice(&u24(0));
    f.extend_from_slice(&u24(0));
    f.extend_from_slice(&u24(w - 1));
    f.extend_from_slice(&u24(h - 1));
    f.extend_from_slice(&u24(100));
    f.push(0);
    f.extend_from_slice(&chunk(b"VP8L", &body));
    let anmf = chunk(b"ANMF", &f);
    let animated = riff(&[chunk(b"VP8X", &xa), anim, anmf.clone(), anmf]);
    let r = read(&replace_entry(&pack, "portrait/a-2.webp", animated));
    assert!(matches!(r, Ok(Err(PackError::Image(_)))), "animated: {r:?}");
    // Lossy with alpha (VP8X + ALPH + VP8): refused as lossy.
    let mut xl = vec![0b0001_0000u8];
    xl.extend_from_slice(&[0; 3]);
    xl.extend_from_slice(&u24(w - 1));
    xl.extend_from_slice(&u24(h - 1));
    let mut vp8 = vec![0x10, 0x02, 0x00, 0x9d, 0x01, 0x2a];
    vp8.extend_from_slice(&(w as u16).to_le_bytes());
    vp8.extend_from_slice(&(h as u16).to_le_bytes());
    vp8.extend_from_slice(&[0; 16]);
    let lossy = riff(&[
        chunk(b"VP8X", &xl),
        chunk(b"ALPH", &[0; 8]),
        chunk(b"VP8 ", &vp8),
    ]);
    let r = read(&replace_entry(&pack, "portrait/a-2.webp", lossy));
    assert!(
        matches!(r, Ok(Err(PackError::Image(_)))),
        "lossy with alpha: {r:?}"
    );
    // Huge VP8X canvas that matches nothing: refused by size, before decoding.
    let mut xh = vec![0u8];
    xh.extend_from_slice(&[0; 3]);
    xh.extend_from_slice(&u24(0xff_ffff));
    xh.extend_from_slice(&u24(0));
    let huge = riff(&[chunk(b"VP8X", &xh), chunk(b"VP8L", &body)]);
    let r = read(&replace_entry(&pack, "portrait/a-2.webp", huge));
    assert!(
        matches!(r, Ok(Err(PackError::Image(_)))),
        "huge canvas: {r:?}"
    );
}

#[test]
fn names_and_duplicates() {
    let pack = write_pack(&sample(), &generator(), true).unwrap();
    let files = zip::read_zip(&pack, |_| true).unwrap();
    type Files = Vec<(String, Vec<u8>)>;
    let cases: Vec<(&str, Files, u16)> = {
        let mut v = Vec::new();
        for extra in [
            "../manifest.json",
            "/manifest.json",
            "manifest.json/",
            "Manifest.json",
            "portrait/../index.bin",
            "portrait/a-2.webp/../../x",
            "portrait/",
            "atlas-00.webp",
            "atlas-1.webp",
            "portrait/a-99999.webp",
            "bitmap/paper.webp\0",
            "C:\\x",
        ] {
            let mut f = files.clone();
            f.push((extra.to_string(), b"x".to_vec()));
            v.push((extra, f, 0u16));
        }
        let mut dup = files.clone();
        dup.push(files[0].clone());
        v.push(("dup manifest", dup, 0));
        v
    };
    for (what, f, flags) in cases {
        let r = read(&write_zip(&f, flags));
        assert!(matches!(r, Ok(Err(_))), "{what:?}: {r:?}");
    }
    // The pack's own entries with the UTF-8 flag set: the same names, the same pack.
    let r = read(&write_zip(&files, 0x800));
    assert!(matches!(r, Ok(Ok(_))), "{r:?}");
}

#[test]
fn rezipped_image_mutations_never_panic() {
    // A changed image re-zipped with its new CRC is another pack, not a damaged one: it may
    // load (with its own pixels), but nothing panics.
    let pack = write_pack(&sample(), &generator(), true).unwrap();
    let files = zip::read_zip(&pack, |_| true).unwrap();
    let mut rng = Rng(0x0bad_cafe_1234_5678);
    for _ in 0..3000 {
        let mut f = files.clone();
        let webps: Vec<usize> = (0..f.len())
            .filter(|&i| f[i].0.ends_with(".webp"))
            .collect();
        let at = webps[rng.below(webps.len())];
        let i = rng.below(f[at].1.len());
        f[at].1[i] ^= 1 << rng.below(8);
        assert!(read(&write_zip(&f, 0)).is_ok(), "a panic");
    }
}

#[test]
fn in_place_flips_of_a_written_pack() {
    let pack = write_pack(&sample(), &generator(), true).unwrap();
    let orig = read_pack(&pack).unwrap();
    let mut rng = Rng(0x5eed_5eed_5eed_5eed);
    let (mut ok, mut changed) = (0, 0);
    for _ in 0..3000 {
        let mut b = pack.clone();
        let i = rng.below(b.len());
        b[i] ^= 1 << rng.below(8);
        if let Ok(Ok(p)) = read(&b) {
            ok += 1;
            if p.images != orig.images {
                changed += 1;
            }
        }
    }
    println!("in-place single-bit flips of a written pack: accepted {ok}, pixels differ {changed}");
    // Flips the CRC-32 cannot see are outside the data (dates, version fields): never pixels.
    assert_eq!(changed, 0);
}
