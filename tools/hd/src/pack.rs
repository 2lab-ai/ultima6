//! `pack`: the `u6-hdpack/1` zip of the images ingest accepted, with the content hashes of the
//! classic images they were made from, `derived: true`. The pack is read back by the strict
//! reader before it is kept. There is no export: a derived pack is never shared (spec 07
//! Rights), so the tool has no command that writes one anywhere but the work dir or `--out`
//! outside the repository.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use u6_hdpack::{Generator, Kind, PackImage, read_pack, write_pack};

use crate::atlas;
use crate::img;
use crate::ingest::Accepted;
use crate::run::MODEL;
use crate::work::{Work, check_outside_repo};

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
    check_outside_repo(&dest)?;
    let list = work.path("ingest.json");
    let text =
        std::fs::read(&list).map_err(|e| format!("{}: {e} (run ingest first)", list.display()))?;
    let accepted: Vec<Accepted> = serde_json::from_slice(&text).map_err(|e| e.to_string())?;
    let mut images = Vec::with_capacity(accepted.len());
    for a in &accepted {
        let id =
            atlas::parse_image(&a.image).ok_or_else(|| format!("bad image id {:?}", a.image))?;
        let hash =
            u64::from_str_radix(&a.hash, 16).map_err(|_| format!("bad hash for {}", a.image))?;
        let image = img::read_png(&work.root.join(&a.slice))?;
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
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let tmp = dest.with_extension("zip.tmp");
    std::fs::write(&tmp, &bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let count = |k: Kind| back.images.iter().filter(|p| p.id.kind() == k).count();
    Ok(PackSummary {
        tiles: count(Kind::Tile),
        portraits: count(Kind::Portrait),
        bitmaps: count(Kind::Bitmap),
        bytes: bytes.len(),
    })
}
