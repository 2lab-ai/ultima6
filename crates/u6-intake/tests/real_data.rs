//! Integration against the user's own game data. Runs only where
//! ~/Games/ultima6 exists (never in CI: the data is not ours to ship, and it is
//! read here only, never copied). Oracles: greaseweazle ADFs (img/), amitools
//! xdftool unpack (files/), tools/pp20.py output (unpacked/) and the HFE images the zip holds
//! (original/). No expected value is copied from them: the report the intake must write is
//! built here from those oracles (headers read at their offsets, files counted), and
//! comparisons of their content name lengths and indices only.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)] // tests may read files and time

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use sha2::{Digest, Sha256};
use u6_intake::{ADF_SIZE, InputFile, intake, read_ofs};
use u6_testkit::assert_same;

const GAME: &str = "/Users/zhugehyuk/Games/ultima6";
const SIDE_CARS: [&str; 3] = [".blkdev", ".bootcode", ".xdfmeta"];

fn sha256(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Every regular file under `root`, keyed by its '/'-joined path relative to `root`.
fn list_tree(root: &Path) -> BTreeMap<String, PathBuf> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let full = entry.unwrap().path();
            if full.is_dir() {
                walk(root, &full, out);
            } else {
                let rel = full.strip_prefix(root).unwrap();
                let key: Vec<&str> = rel.iter().map(|c| c.to_str().unwrap()).collect();
                out.insert(key.join("/"), full);
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

/// The one directory under `dir` (xdftool unpacks a volume into a directory of its name).
fn volume_dir(dir: &Path) -> String {
    let dirs: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| {
            let e = e.unwrap();
            e.path()
                .is_dir()
                .then(|| e.file_name().into_string().unwrap())
        })
        .collect();
    assert_eq!(dirs.len(), 1, "one volume directory in {}", dir.display());
    dirs[0].clone()
}

/// The report line of an HFE image, from its header (HxC HFE v1: the magic, then cylinders at
/// 9, sides at 10, track encoding at 11, the LE bit rate at 12) and a fully decoded disk of
/// `ADF_SIZE / 512` sectors.
fn hfe_line(label: &str, hfe: &[u8]) -> String {
    assert!(hfe.starts_with(b"HXCPICFE"), "{label}: an HFE image");
    let kbps = u16::from_le_bytes([hfe[12], hfe[13]]);
    let n = ADF_SIZE / 512;
    format!(
        "{label}: HFE {} cyl x {} sides, {kbps} kbps, encoding {}; sectors {n}/{n} good, bad \
         header checksums 0, bad data checksums 0, wrong-track headers 0",
        hfe[9], hfe[10], hfe[11]
    )
}

#[test]
fn intake_on_the_real_ultima_vi_zip() {
    let game = Path::new(GAME);
    let zip = game.join("original/ultima-vi.zip");
    if !zip.exists() {
        eprintln!("skipped: {} not present", zip.display());
        return;
    }
    let data = std::fs::read(&zip).unwrap();
    let size = data.len();
    let t0 = Instant::now();
    let r = intake(vec![InputFile {
        name: "ultima-vi.zip".into(),
        data,
    }]);
    let elapsed = t0.elapsed();
    println!(
        "intake(ultima-vi.zip, {size} bytes): {} ms",
        elapsed.as_millis()
    );
    for line in &r.report {
        println!("  {line}");
    }

    // The HFE images of the zip, as archive.org also lists them loose next to it.
    let mut labels: Vec<String> = std::fs::read_dir(game.join("original"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.to_ascii_lowercase().ends_with(".hfe"))
        .collect();
    labels.sort();
    assert!(!labels.is_empty(), "the HFE images next to the zip");
    let got: Vec<String> = r.disks.iter().map(|d| d.label.clone()).collect();
    assert_same!(got, labels, "the disk labels, in order");
    // The zip line names the same images (in the archive's order).
    let (head, list) = r.report[0].split_once(": ").unwrap();
    assert_eq!(head, "ultima-vi.zip", "the zip line first");
    let (count, names) = list.split_once(": ").unwrap();
    assert_eq!(count, format!("zip, {} disk image(s)", labels.len()));
    let mut listed: Vec<String> = names.split(", ").map(str::to_string).collect();
    listed.sort_unstable();
    assert_same!(listed, labels, "the images the zip line names");
    // Then per disk its HFE line and its volume line: the volume xdftool unpacked (its name,
    // the DOS type of the ADF's boot block, its file count), then the PP20 summary.
    let mut want_report = vec![r.report[0].clone()];
    for (i, label) in labels.iter().enumerate() {
        let hfe = std::fs::read(game.join("original").join(label)).unwrap();
        want_report.push(hfe_line(label, &hfe));
        let d = game.join(format!("files/d{}", i + 1));
        let vol = volume_dir(&d);
        let n = list_tree(&d.join(&vol)).len();
        let adf = std::fs::read(game.join(format!("img/disk{}.adf", i + 1))).unwrap();
        assert_eq!(&adf[..3], b"DOS", "{label}: an AmigaDOS boot block");
        let dos = format!("DOS{}", adf[3]);
        want_report.push(format!("{label}: volume {vol} ({dos}), {n} files"));
    }
    // The decrunched files: those xdftool unpacked that start with the PP20 magic.
    let crunched: usize = (1..=labels.len())
        .map(|i| {
            let d = game.join(format!("files/d{i}"));
            list_tree(&d.join(volume_dir(&d)))
                .values()
                .filter(|p| std::fs::read(p).unwrap().starts_with(b"PP20"))
                .count()
        })
        .sum();
    let mut want_files = BTreeMap::new();
    for d in ["d1", "d2", "d3"] {
        for (path, full) in list_tree(&game.join("unpacked").join(d)) {
            if !SIDE_CARS.iter().any(|ext| path.ends_with(ext)) {
                want_files.insert(path, full);
            }
        }
    }
    let summary = format!(
        "PP20: decrunched {crunched} file(s); {} file(s) total",
        want_files.len()
    );
    want_report.push(summary.clone());
    assert_same!(r.report, want_report, "the report lines");

    // Every disk with a volume, every sector good.
    let volumes: Vec<Option<String>> = r.disks.iter().map(|d| d.volume.clone()).collect();
    let want_volumes: Vec<Option<String>> = (1..=labels.len())
        .map(|i| Some(volume_dir(&game.join(format!("files/d{i}")))))
        .collect();
    assert_same!(volumes, want_volumes, "the volume names");

    // Each ADF equals the greaseweazle image byte for byte.
    for (i, disk) in r.disks.iter().enumerate() {
        let want = std::fs::read(game.join(format!("img/disk{}.adf", i + 1))).unwrap();
        assert_eq!(sha256(&disk.adf), sha256(&want), "{}", disk.label);
    }

    // OFS file set and bytes equal xdftool unpack.
    for (i, disk) in r.disks.iter().enumerate() {
        let vol = read_ofs(&disk.adf).unwrap();
        assert!(
            vol.warnings.is_empty(),
            "{}: {:?}",
            disk.label,
            vol.warnings
        );
        let want = list_tree(&game.join(format!("files/d{}", i + 1)).join(&vol.name));
        let mut got: Vec<&str> = vol.files.iter().map(|(p, _)| p.as_str()).collect();
        got.sort_unstable();
        let want_paths: Vec<&str> = want.keys().map(String::as_str).collect();
        assert_same!(got, want_paths, "the files of {}", disk.label);
        for (path, full) in &want {
            let bytes = vol.file(path).unwrap();
            assert_eq!(
                sha256(bytes),
                sha256(&std::fs::read(full).unwrap()),
                "{}/{path}",
                vol.name
            );
        }
    }

    // Intake files equal the decrunched tree (PP20 via tools/pp20.py).
    let got: Vec<&String> = r.files.keys().collect();
    let want: Vec<&String> = want_files.keys().collect();
    assert_same!(got, want, "the intake's file paths");
    for (path, full) in &want_files {
        assert_eq!(
            sha256(&r.files[path]),
            sha256(&std::fs::read(full).unwrap()),
            "{path}"
        );
    }
    assert_eq!(r.report.last(), Some(&summary));
    assert!(!r.report.iter().any(|l| l.contains("decrunch failed")));

    assert!(elapsed.as_secs() < 60, "intake took {elapsed:?}");
}
