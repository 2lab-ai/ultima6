//! Port of the former TypeScript test pp20.test.ts.

mod common;

use std::path::Path;

use common::pp20_encoder::crunch_pp20;
use common::text;
use u6_intake::{decrunch_pp20, is_pp20};

fn lcg(n: usize, seed: u32) -> Vec<u8> {
    let mut x = seed;
    (0..n)
        .map(|_| {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (x >> 24) as u8
        })
        .collect()
}

fn vectors() -> Vec<(&'static str, Vec<u8>)> {
    let base = lcg(1500, 7);
    let mixed = (0..4000)
        .map(|i| {
            if i % 1700 < 1500 {
                base[i % 1500]
            } else {
                i as u8
            }
        })
        .collect();
    vec![
        ("one byte", vec![0x42]),
        ("two equal bytes", vec![7, 7]),
        (
            "text with repeats",
            text(&"Synthetic text: a long line that repeats, so it crunches well. ".repeat(12)),
        ),
        ("long run (length extension chunks)", vec![0xaa; 2000]),
        ("random (literal runs only)", lcg(700, 1)),
        ("mixed far/near matches", mixed),
    ]
}

#[test]
fn round_trips_every_vector() {
    for (name, plain) in vectors() {
        let packed = crunch_pp20(&plain);
        assert!(is_pp20(&packed), "{name}");
        assert_eq!(
            decrunch_pp20(&packed).as_deref(),
            Ok(plain.as_slice()),
            "{name}"
        );
    }
}

#[test]
fn compresses_repetitive_data() {
    // The encoder really emits matches, so the decoder's match path is exercised.
    let plain = &vectors()[2].1;
    assert!(crunch_pp20(plain).len() < plain.len() / 4);
}

#[test]
fn rejects_non_pp20_input_and_truncated_streams() {
    let err = decrunch_pp20(b"FORM0000ILBM").unwrap_err().to_string();
    assert!(err.contains("not a PP20"), "{err}");
    let packed = crunch_pp20(&vectors()[2].1);
    let cut: Vec<u8> = [&packed[..8], &packed[packed.len() - 20..]].concat();
    let err = decrunch_pp20(&cut).unwrap_err().to_string();
    assert!(err.contains("PP20"), "{err}");
}

#[test]
fn hostile_headers_error_instead_of_panicking() {
    // Offset widths of 255 bits and a skip count past the stream end.
    let mut wide = crunch_pp20(&vectors()[2].1);
    wide[4..8].copy_from_slice(&[255, 255, 255, 255]);
    assert!(decrunch_pp20(&wide).is_err());
    let mut skip = crunch_pp20(b"abc");
    let n = skip.len();
    skip[n - 1] = 255;
    assert_eq!(
        decrunch_pp20(&skip).unwrap_err().to_string(),
        "PP20: out of source bits"
    );
    assert_eq!(
        decrunch_pp20(b"PP20").unwrap_err().to_string(),
        "PP20 file truncated"
    );
}

#[test]
#[allow(clippy::disallowed_methods)] // test-only file exchange with the Python reference
fn agrees_with_tools_pp20_py_on_every_vector() {
    let reference = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/pp20.py");
    if !reference.exists() {
        eprintln!("skipped: {} not present", reference.display());
        return;
    }
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("pp20-reference");
    std::fs::create_dir_all(&dir).unwrap();
    for (i, (name, plain)) in vectors().into_iter().enumerate() {
        let src = dir.join(format!("v{i}.pp"));
        let dst = dir.join(format!("v{i}.out"));
        let packed = crunch_pp20(&plain);
        std::fs::write(&src, &packed).unwrap();
        let status = std::process::Command::new("python3")
            .arg("-I")
            .arg(&reference)
            .arg(&src)
            .arg(&dst)
            .output()
            .expect("python3 runs");
        assert!(
            status.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&status.stderr)
        );
        assert_eq!(
            std::fs::read(&dst).unwrap(),
            decrunch_pp20(&packed).unwrap(),
            "{name}"
        );
    }
}
