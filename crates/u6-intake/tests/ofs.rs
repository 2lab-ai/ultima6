//! Port of `web/src/intake/__tests__/ofs.test.ts`.

mod common;

use common::ofs_builder::{ImageBuilder, Node, amiga_hash};
use common::text;
use u6_intake::{ADF_SIZE, read_ofs};

const BS: usize = 512;

fn bytes(n: usize, seed: usize) -> Vec<u8> {
    (0..n)
        .map(|i| ((i * 31 + seed * 7 + (i >> 8)) & 0xff) as u8)
        .collect()
}

fn tree() -> Vec<Node> {
    vec![
        Node::File("game", bytes(1000, 1)),
        Node::File("big", bytes(40_000, 2)), // > 72 data blocks: needs an extension block
        Node::File("empty", Vec::new()),
        Node::File("disk.info", bytes(488, 3)), // exactly one OFS data block
        Node::Dir("s", vec![Node::File("startup-sequence", text("ultima6\n"))]),
        Node::Dir("Trashcan", vec![]),
        // "an" and "ba" (like "game" and "disk.info") share a hash slot -> exercises hash_chain.
        Node::File("an", text("first")),
        Node::File("ba", text("second")),
    ]
}

#[test]
fn chained_names_really_collide() {
    assert_eq!(amiga_hash("an"), amiga_hash("ba"));
    assert_eq!(amiga_hash("game"), amiga_hash("disk.info"));
}

fn reads_tree(ffs: bool) {
    let img = ImageBuilder::new(ffs).build("DATA1", &tree());
    let vol = read_ofs(&img).unwrap();
    assert_eq!(vol.name, "DATA1");
    assert_eq!(vol.dos_type, if ffs { "DOS1" } else { "DOS0" });
    assert!(vol.warnings.is_empty(), "{:?}", vol.warnings);
    let mut dirs = vol.dirs.clone();
    dirs.sort();
    assert_eq!(dirs, ["Trashcan", "s"]);
    let want: Vec<(&str, Vec<u8>)> = vec![
        ("game", bytes(1000, 1)),
        ("big", bytes(40_000, 2)),
        ("empty", Vec::new()),
        ("disk.info", bytes(488, 3)),
        ("s/startup-sequence", text("ultima6\n")),
        ("an", text("first")),
        ("ba", text("second")),
    ];
    let mut got_keys: Vec<&str> = vol.files.iter().map(|(p, _)| p.as_str()).collect();
    let mut want_keys: Vec<&str> = want.iter().map(|(p, _)| *p).collect();
    got_keys.sort_unstable();
    want_keys.sort_unstable();
    assert_eq!(got_keys, want_keys);
    for (path, data) in &want {
        assert_eq!(vol.file(path), Some(data.as_slice()), "{path}");
    }
}

#[test]
fn reads_files_subdirectories_extension_blocks_and_hash_chains_ofs() {
    reads_tree(false);
}

#[test]
fn reads_files_subdirectories_extension_blocks_and_hash_chains_ffs() {
    reads_tree(true);
}

#[test]
fn reports_a_bad_data_block_checksum_but_still_returns_the_file() {
    let mut img = ImageBuilder::new(false).build("INSTALL", &[Node::File("game", bytes(100, 9))]);
    img[883 * BS + 100] ^= 0xff; // block 882 = header, 883 = data
    let vol = read_ofs(&img).unwrap();
    assert_eq!(vol.warnings, ["game: bad data block checksum at 883"]);
    assert_eq!(vol.file("game").map(<[u8]>::len), Some(100));
}

#[test]
fn rejects_images_without_a_dos_boot_block_or_root_block() {
    let err = read_ofs(&vec![0u8; ADF_SIZE]).unwrap_err().to_string();
    assert!(err.contains("DOS signature"), "{err}");
    let mut img = vec![0u8; ADF_SIZE];
    img[..4].copy_from_slice(b"DOS\0");
    let err = read_ofs(&img).unwrap_err().to_string();
    assert!(err.contains("root block"), "{err}");
}

#[test]
fn reports_hash_loops_instead_of_recursing_forever() {
    let mut img = ImageBuilder::new(false).build("LOOP", &[Node::File("an", text("first"))]);
    // Point the file header's hash_chain at itself (Rust-only case: the walk must terminate).
    img[882 * BS + BS - 16..882 * BS + BS - 12].copy_from_slice(&882u32.to_be_bytes());
    let vol = read_ofs(&img).unwrap();
    assert!(
        vol.warnings.iter().any(|w| w == "/: loop at block 882"),
        "{:?}",
        vol.warnings
    );
    assert_eq!(vol.file("an"), Some(&b"first"[..]));
}
