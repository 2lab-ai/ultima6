//! `pack`: the `u6-hdpack/1` zip of the images ingest accepted, with the content hashes of the
//! classic images they were made from, `derived: true`, at the pack's ×4: each ×8 master in
//! `slices/` box-averaged 2 × 2 (the masters stay in the work dir). The pack is read back by the strict
//! reader before it is kept. There is no export: a derived pack is never shared (spec 07
//! Rights), so the tool has no command that writes one anywhere but the work dir or `--out`
//! outside the repository.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use u6_hdpack::{Generator, Kind, PackImage, SCALE, read_pack, write_pack};

use crate::atlas::{self, MASTER};
use crate::img;
use crate::ingest::Accepted;
use crate::run::MODEL;
use crate::work::{self, Work};

/// The civil date (UTC) of `secs` since the epoch, `YYYY-MM-DD`.
pub fn date(secs: u64) -> String {
    // Days to civil date (H. Hinnant's algorithm).
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct PackSummary {
    pub tiles: usize,
    pub portraits: usize,
    pub bitmaps: usize,
    pub bytes: usize,
}

/// Writes the pack to `out` (default `<work>/pack/u6-hdpack.zip`).
pub fn pack(work: &Work, out: Option<&Path>) -> Result<PackSummary, String> {
    let dest = out.map_or_else(|| work.path("pack/u6-hdpack.zip"), Path::to_path_buf);
    work::check_target(&dest)?;
    let list = work.path("ingest.json");
    let text = img::read_capped_said(&list, 64 << 20, |e| {
        work::said(&list, e, "run ingest first")
    })?;
    let accepted: Vec<Accepted> = serde_json::from_slice(&text).map_err(|e| e.to_string())?;
    let mut images = Vec::with_capacity(accepted.len());
    for a in &accepted {
        let id =
            atlas::parse_image(&a.image).ok_or_else(|| format!("bad image id {:?}", a.image))?;
        let hash =
            u64::from_str_radix(&a.hash, 16).map_err(|_| format!("bad hash for {}", a.image))?;
        let stem = atlas::slice_name(id);
        if a.slice != format!("slices/{stem}.png") {
            return Err(format!("{}: not the slice of {}", a.slice, a.image));
        }
        let master = img::read_png(&work.slice(&stem), a.width, a.height)?;
        let image = img::shrink(&master, MASTER / SCALE);
        images.push(PackImage { id, hash, image });
    }
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let generator = Generator {
        name: "u6-hd".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        engine: format!("{MODEL} image_gen (codex exec, built-in tool)"),
        date: date(secs),
    };
    let bytes = write_pack(&images, &generator, true).map_err(|e| e.to_string())?;
    // The page's reader must take it: read it back before it is kept.
    let back = read_pack(&bytes).map_err(|e| format!("the written pack does not read: {e}"))?;
    if back.images.len() != images.len() {
        return Err("the written pack lost images".into());
    }
    work::write_file(&dest, &bytes)?;
    let count = |k: Kind| back.images.iter().filter(|p| p.id.kind() == k).count();
    Ok(PackSummary {
        tiles: count(Kind::Tile),
        portraits: count(Kind::Portrait),
        bitmaps: count(Kind::Bitmap),
        bytes: bytes.len(),
    })
}
