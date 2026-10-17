//! A full-size pack of the scaler's own images, for timing the page with a pack loaded (spec 07
//! Acceptance 5): every classic image of the owner's files as loaded (the 2048 tile records,
//! every portrait, the paper and spellbook bitmaps) as MMPX ×4 (the compositor's scaler) in the
//! game's palette, keyed by its content hash, so every image the page draws from its original
//! bytes is a pack hit; a record the game rewrites as it runs (the merged shore records) has other
//! bytes then, matches no entry and is drawn by MMPX, as with any pack. It is
//! made from the owner's art, so it is `derived: true`, and it is written only outside the
//! repository (the tool's own check, `work::check_target`), like every output of the tool.
//!
//! ```text
//! cargo run -p u6-hdtool --release --example scaler_pack -- --game PATH --out FILE
//! ```
//!
//! Prints counts and the file size only.

use std::path::PathBuf;
use std::process::ExitCode;

use u6_hdpack::{Generator, ImageId, Kind, PackImage, Rgba, Role, write_pack};
use u6_hdtool::game::Game;
use u6_hdtool::img::Classic;
use u6_hdtool::work;
use u6_render::hd::{Scale, mmpx};

/// MMPX ×4 of a classic image, transparent pixels transparent.
fn scaled(c: &Classic) -> Rgba {
    let input: Vec<u32> =
        c.px.iter()
            .map(|p| p.map_or(0, |[r, g, b]| u32::from_le_bytes([r, g, b, 255])))
            .collect();
    let mut out = Vec::new();
    mmpx(
        &input,
        c.width as usize,
        c.height as usize,
        Scale::X4,
        &mut out,
    );
    Rgba {
        width: c.width * 4,
        height: c.height * 4,
        pixels: out.iter().flat_map(|w| w.to_le_bytes()).collect(),
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let (mut game, mut out) = (None, None);
    while let Some(a) = args.next() {
        let mut value = || args.next().ok_or(format!("{a} needs a path"));
        match a.as_str() {
            "--game" => game = Some(PathBuf::from(value()?)),
            "--out" => out = Some(PathBuf::from(value()?)),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    let (Some(game), Some(out)) = (game, out) else {
        return Err("usage: scaler_pack --game PATH --out FILE".into());
    };
    work::check_target(&out)?;
    let g = Game::load(&game).map_err(|f| f.category.to_string())?;
    let ids = (0..g.records.len() as u16)
        .map(ImageId::Tile)
        .chain(
            g.portraits
                .iter()
                .map(|(letter, item, _)| ImageId::Portrait {
                    letter: *letter,
                    item: *item,
                }),
        )
        .chain(Role::ALL.into_iter().map(ImageId::Bitmap));
    let mut images = Vec::new();
    for id in ids {
        if let (Some(c), Some(hash)) = (g.classic(id), g.hash(id)) {
            images.push(PackImage {
                id,
                hash,
                image: scaled(&c),
            });
        }
    }
    let generator = Generator {
        name: "u6-hd scaler_pack".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        engine: "MMPX x4".into(),
        date: u6_hdtool::pack::date(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
        ),
    };
    let bytes = write_pack(&images, &generator, true).map_err(|e| e.to_string())?;
    if let Some(dir) = out.parent() {
        work::ensure_dir(dir)?;
    }
    work::write_file(&out, &bytes)?;
    let count = |k: Kind| images.iter().filter(|p| p.id.kind() == k).count();
    let decoded: usize = images.iter().map(|p| p.image.pixels.len()).sum();
    println!(
        "scaler pack: {} tiles, {} portraits, {} bitmaps; {} bytes ({} decoded)",
        count(Kind::Tile),
        count(Kind::Portrait),
        count(Kind::Bitmap),
        bytes.len(),
        decoded
    );
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("scaler_pack: {e}");
            ExitCode::FAILURE
        }
    }
}
