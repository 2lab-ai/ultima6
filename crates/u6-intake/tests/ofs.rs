//! Port of the former TypeScript test ofs.test.ts.

mod common;

use common::ofs_builder::{ImageBuilder, Node, amiga_hash, nested_dirs, shared_block_volume};
use common::text;
use u6_intake::{ADF_SIZE, read_ofs, read_ofs_within};

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

#[test]
fn a_limit_stops_the_walk_before_the_file_that_would_pass_it() {
    // "a", "b", "c" hash to consecutive slots, so the walk reads them in that order.
    let img = ImageBuilder::new(false).build(
        "LIMIT",
        &[
            Node::File("a", bytes(1000, 1)),
            Node::File("b", bytes(2000, 2)),
            Node::File("c", bytes(10, 3)),
        ],
    );
    // Each file is charged its bytes, its path twice with 31 bytes of room, and 256 bytes.
    let charge = |n: usize| n + 2 * (31 + 1) + 256;
    let whole = read_ofs(&img).unwrap();
    assert_eq!(whole.held, charge(1000) + charge(2000) + charge(10));
    let all = read_ofs_within(&img, whole.held).unwrap();
    assert!(!all.limit_reached);
    assert_eq!(all.files.len(), 3);
    let vol = read_ofs_within(&img, charge(1000) + charge(2000) - 1).unwrap();
    assert!(vol.limit_reached);
    assert_eq!(vol.held, charge(1000));
    let paths: Vec<&str> = vol.files.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(paths, ["a"]);
    assert_eq!(vol.file("a"), Some(bytes(1000, 1).as_slice()));
    assert!(vol.warnings.is_empty(), "{:?}", vol.warnings);
    assert!(!read_ofs(&img).unwrap().limit_reached);
}

#[test]
fn a_directory_chain_as_deep_as_the_disk_reads_on_a_small_stack() {
    // 1,757 levels, every block of the disk; 512 KiB is half the stack a wasm build runs on.
    let img = nested_dirs("DEEP", 1757, false);
    let vol = std::thread::Builder::new()
        .stack_size(512 << 10)
        .spawn(move || read_ofs(&img).unwrap())
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(vol.dirs.len(), 64);
    assert_eq!(
        vol.warnings,
        ["directories nested more than 64 levels deep are not read"]
    );
}

#[test]
fn a_file_keeps_no_more_capacity_than_its_bytes() {
    // An image 300 bytes longer than its 1,760 blocks, holding an FFS file of 901,400 bytes
    // read out of one data block (its header is its own extension block): longer than the
    // blocks, within the image.
    let mut img = shared_block_volume(1, 901_400);
    img.extend_from_slice(&[0; 300]);
    let vol = read_ofs(&img).unwrap();
    let kept = &vol.files.iter().find(|(p, _)| p == "s000").expect("read").1;
    assert_eq!(kept.len(), 901_400);
    assert_eq!(kept.capacity(), kept.len());
}
