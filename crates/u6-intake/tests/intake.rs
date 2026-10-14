//! Port of `web/src/intake/__tests__/intake.test.ts` (plus deflate-entry cases).

mod common;

use common::mfm_encoder::{SectorSpec, TRACK_CELLS, pack_lsb_first, write_hfe, write_track};
use common::ofs_builder::{ImageBuilder, Node};
use common::pp20_encoder::crunch_pp20;
use common::text;
use common::zip_writer::zip;
use u6_intake::{InputFile, Intake, intake};

fn game_text() -> Vec<u8> {
    text(&"The Avatar has returned. ".repeat(40))
}

fn volume(name: &str) -> Vec<u8> {
    ImageBuilder::new(false).build(
        name,
        &[
            Node::File("game", crunch_pp20(&game_text())),
            Node::File("plain", text("not crunched")),
            Node::Dir("s", vec![Node::File("startup-sequence", text("ultima6\n"))]),
        ],
    )
}

/// Encode an ADF image as an HFE: 83 cylinders like the archive.org dumps.
fn adf_to_hfe(adf: &[u8]) -> Vec<u8> {
    let tracks: Vec<Vec<Vec<u8>>> = (0..83)
        .map(|cyl| {
            (0..2)
                .map(|head| {
                    let t = cyl * 2 + head;
                    let specs: Vec<SectorSpec> = (0..11)
                        .map(|s| SectorSpec {
                            track: t as u32,
                            sector: s as u32,
                            data: if cyl < 80 {
                                adf[(t * 11 + s) * 512..(t * 11 + s + 1) * 512].to_vec()
                            } else {
                                vec![0; 512]
                            },
                            bad_data: false,
                        })
                        .collect();
                    pack_lsb_first(&write_track(&specs, TRACK_CELLS))
                })
                .collect()
        })
        .collect();
    write_hfe(&tracks, 250)
}

fn file(name: &str, data: Vec<u8>) -> InputFile {
    InputFile {
        name: name.to_string(),
        data,
    }
}

fn has_line(r: &Intake, line: &str) -> bool {
    r.report.iter().any(|l| l == line)
}

#[test]
fn zip_of_hfe_disks_to_adfs_volume_files_pp20_decrunched_scans_ignored() {
    let (d1, d2) = (volume("INSTALL"), volume("DATA1"));
    let (h1, h2) = (adf_to_hfe(&d1), adf_to_hfe(&d2));
    let jpeg = text("\u{ff}\u{d8} not really a jpeg");
    let archive = zip(
        &[
            ("Ultima VI Disk 2.hfe", &h2),
            ("Ultima VI Disk 1.hfe", &h1),
            ("AA_UVI.jpg", &jpeg),
            ("Pauline/Data Disk 1/track00.0.hxcstream", b"flux"),
        ],
        false,
    );
    let r = intake(vec![file("ultima-vi.zip", archive)]);
    let labels: Vec<(&str, Option<&str>)> = r
        .disks
        .iter()
        .map(|d| (d.label.as_str(), d.volume.as_deref()))
        .collect();
    assert_eq!(
        labels,
        [
            ("Ultima VI Disk 1.hfe", Some("INSTALL")),
            ("Ultima VI Disk 2.hfe", Some("DATA1"))
        ]
    );
    assert!(r.disks[0].adf == d1);
    assert!(r.disks[1].adf == d2);
    let keys: Vec<&str> = r.files.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "DATA1/game",
            "DATA1/plain",
            "DATA1/s/startup-sequence",
            "INSTALL/game",
            "INSTALL/plain",
            "INSTALL/s/startup-sequence",
        ]
    );
    assert_eq!(r.files["INSTALL/game"], game_text());
    assert_eq!(r.files["DATA1/plain"], text("not crunched"));
    assert!(has_line(
        &r,
        "ultima-vi.zip: zip, 2 disk image(s): Ultima VI Disk 2.hfe, Ultima VI Disk 1.hfe"
    ));
    assert!(has_line(
        &r,
        "Ultima VI Disk 1.hfe: HFE 83 cyl x 2 sides, 250 kbps, encoding 1; sectors 1760/1760 good, \
         bad header checksums 0, bad data checksums 0, wrong-track headers 0"
    ));
    assert!(has_line(
        &r,
        "Ultima VI Disk 2.hfe: volume DATA1 (DOS0), 3 files"
    ));
    assert!(
        has_line(&r, "PP20: decrunched 2 file(s); 6 file(s) total"),
        "{:#?}",
        r.report
    );
}

#[test]
fn deflated_zip_entries_and_numeric_label_order() {
    let (a, b) = (volume("DATA1"), volume("DATA2"));
    let archive = zip(
        &[
            ("Disk 10.adf", &b),
            ("disks/Disk 2.ADF", &a),
            ("._Disk 3.adf", b"junk"),
        ],
        true,
    );
    let r = intake(vec![file("u6.zip", archive)]);
    let labels: Vec<&str> = r.disks.iter().map(|d| d.label.as_str()).collect();
    assert_eq!(labels, ["Disk 2.ADF", "Disk 10.adf"], "{:#?}", r.report);
    assert!(r.disks[0].adf == a && r.disks[1].adf == b);
    assert_eq!(r.files["DATA2/game"], game_text());
    assert!(has_line(&r, "Disk 10.adf: ADF 901120 bytes"));
}

#[test]
fn unreadable_zip_is_reported() {
    let mut archive = zip(&[("Disk 1.adf", &volume("DATA1"))], true);
    let n = archive.len();
    archive.truncate(n - 30); // cut into the central directory and drop the EOCD
    let r = intake(vec![file("cut.zip", archive)]);
    assert!(r.disks.is_empty());
    assert_eq!(r.report[0], "cut.zip: zip unreadable: invalid zip data");
}

#[test]
fn accepts_a_dropped_adf_directly() {
    let adf = volume("DATA2");
    let r = intake(vec![file("disk3.adf", adf.clone())]);
    assert_eq!(r.disks.len(), 1);
    assert!(r.disks[0].adf == adf);
    assert_eq!(r.files["DATA2/game"], game_text());
}

#[test]
fn passes_loose_files_through_decrunching_pp20_and_keeping_undecodable_ones_raw() {
    let mut broken = crunch_pp20(&game_text())[..24].to_vec();
    broken[20..].copy_from_slice(&[0, 0x10, 0, 3]); // claims 4 KiB output from a few bits
    let r = intake(vec![
        file("DATA1/game", crunch_pp20(&game_text())),
        file("DATA1\\map", text("map bytes")),
        file("./broken", broken.clone()),
    ]);
    assert!(r.disks.is_empty());
    assert_eq!(r.files["DATA1/game"], game_text());
    assert_eq!(r.files["DATA1/map"], text("map bytes"));
    assert_eq!(r.files["broken"], broken);
    assert!(
        r.report
            .iter()
            .any(|l| l.starts_with("broken: PP20 decrunch failed, kept crunched"))
    );
    assert!(
        has_line(&r, "PP20: decrunched 1 file(s), 1 failed; 3 file(s) total"),
        "{:#?}",
        r.report
    );
}

#[test]
fn reports_an_unreadable_disk_instead_of_failing() {
    let hfe = adf_to_hfe(&volume("INSTALL"))[..4096].to_vec();
    let r = intake(vec![file("bad.hfe", hfe)]);
    assert!(r.disks.is_empty());
    assert!(
        r.report[0].starts_with("bad.hfe: HFE unreadable: "),
        "{}",
        r.report[0]
    );
    assert!(r.report[0].contains("truncated"), "{}", r.report[0]);
}

#[test]
fn duplicate_paths_are_reported_and_the_later_copy_kept() {
    let r = intake(vec![file("x/a", text("one")), file("./x/a", text("two"))]);
    assert_eq!(r.files["x/a"], text("two"));
    assert!(has_line(&r, "x/a: duplicate path, later copy kept"));
}
