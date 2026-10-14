//! Integration against the user's own game data. Runs only where
//! ~/Games/ultima6 exists (never in CI: the data is not ours to ship, and it is
//! read here only, never copied). Oracles: greaseweazle ADFs (img/), amitools
//! xdftool unpack (files/), and tools/pp20.py output (unpacked/).
#![allow(clippy::disallowed_methods, clippy::disallowed_types)] // tests may read files and time

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use sha2::{Digest, Sha256};
use u6_intake::{InputFile, intake, read_ofs};

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

    // Same report as the TypeScript intake on this zip (captured from its real-data test run).
    let hfe_line = |n: u8, kbps: u16| {
        format!(
            "Ultima VI Disk {n}.hfe: HFE 83 cyl x 2 sides, {kbps} kbps, encoding 1; \
             sectors 1760/1760 good, bad header checksums 0, bad data checksums 0, \
             wrong-track headers 0"
        )
    };
    let ts_report = [
        "ultima-vi.zip: zip, 3 disk image(s): Ultima VI Disk 1.hfe, Ultima VI Disk 3.hfe, \
         Ultima VI Disk 2.hfe"
            .to_string(),
        hfe_line(1, 252),
        "Ultima VI Disk 1.hfe: volume INSTALL (DOS0), 41 files".to_string(),
        hfe_line(2, 248),
        "Ultima VI Disk 2.hfe: volume DATA1 (DOS0), 81 files".to_string(),
        hfe_line(3, 250),
        "Ultima VI Disk 3.hfe: volume DATA2 (DOS0), 8 files".to_string(),
        "PP20: decrunched 79 file(s); 130 file(s) total".to_string(),
    ];
    assert_eq!(r.report, ts_report);

    // Three disks, every sector good.
    let labels: Vec<&str> = r.disks.iter().map(|d| d.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "Ultima VI Disk 1.hfe",
            "Ultima VI Disk 2.hfe",
            "Ultima VI Disk 3.hfe"
        ]
    );
    let volumes: Vec<Option<&str>> = r.disks.iter().map(|d| d.volume.as_deref()).collect();
    assert_eq!(volumes, [Some("INSTALL"), Some("DATA1"), Some("DATA2")]);
    let sector_lines: Vec<&String> = r.report.iter().filter(|l| l.contains("sectors ")).collect();
    assert_eq!(sector_lines.len(), 3);
    for l in sector_lines {
        assert!(l.contains("sectors 1760/1760 good"), "{l}");
    }

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
        assert_eq!(
            got,
            want.keys().map(String::as_str).collect::<Vec<_>>(),
            "{}",
            disk.label
        );
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
    let mut want = BTreeMap::new();
    for d in ["d1", "d2", "d3"] {
        for (path, full) in list_tree(&game.join("unpacked").join(d)) {
            if !SIDE_CARS.iter().any(|ext| path.ends_with(ext)) {
                want.insert(path, full);
            }
        }
    }
    assert_eq!(
        r.files.keys().collect::<Vec<_>>(),
        want.keys().collect::<Vec<_>>()
    );
    assert_eq!(r.files.len(), 130);
    for (path, full) in &want {
        assert_eq!(
            sha256(&r.files[path]),
            sha256(&std::fs::read(full).unwrap()),
            "{path}"
        );
    }
    let summary = r.report.last().unwrap();
    assert_eq!(summary, "PP20: decrunched 79 file(s); 130 file(s) total");
    assert!(!r.report.iter().any(|l| l.contains("decrunch failed")));

    assert!(elapsed.as_secs() < 60, "intake took {elapsed:?}");
}
