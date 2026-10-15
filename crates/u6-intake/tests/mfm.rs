//! Port of the former TypeScript test mfm.test.ts.

mod common;

use common::mfm_encoder::{SectorSpec, TRACK_CELLS, pack_lsb_first, write_hfe, write_track};
use u6_intake::{ADF_SIZE, decode_amiga_disk, decode_amiga_track, parse_hfe};

/// Deterministic per-block payload: every byte depends on (lba, offset).
fn payload(lba: usize) -> Vec<u8> {
    let mut x = (lba as u32).wrapping_mul(2_654_435_761);
    if x == 0 {
        x = 1;
    }
    (0..512)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

fn sectors_for(track: usize) -> Vec<SectorSpec> {
    (0..11)
        .map(|s| SectorSpec {
            track: track as u32,
            sector: s as u32,
            data: payload(track * 11 + s),
            bad_data: false,
        })
        .collect()
}

fn rotate(mut cells: Vec<u8>, by: usize) -> Vec<u8> {
    cells.rotate_left(by);
    cells
}

#[test]
fn recovers_all_11_sectors_from_an_encoded_revolution() {
    let cells = pack_lsb_first(&write_track(&sectors_for(5), TRACK_CELLS));
    let dec = decode_amiga_track(&cells, 5);
    assert_eq!(dec.bad_header + dec.bad_data + dec.wrong_track, 0);
    assert_eq!(
        dec.sectors.keys().copied().collect::<Vec<_>>(),
        (0..11).collect::<Vec<_>>()
    );
    for s in 0..11 {
        assert_eq!(dec.sectors[&s], payload(5 * 11 + s));
    }
}

#[test]
fn recovers_a_sector_that_straddles_the_index() {
    // Rotate so the index falls inside sector 3's data area.
    let cells = rotate(write_track(&sectors_for(9), TRACK_CELLS), 3 * 8704 + 4000);
    let dec = decode_amiga_track(&pack_lsb_first(&cells), 9);
    assert_eq!(dec.sectors.len(), 11);
    assert_eq!(dec.sectors[&3], payload(9 * 11 + 3));
}

#[test]
fn finds_sectors_at_any_bit_alignment() {
    let cells = rotate(write_track(&sectors_for(0), TRACK_CELLS), 5);
    assert_eq!(
        decode_amiga_track(&pack_lsb_first(&cells), 0).sectors.len(),
        11
    );
}

#[test]
fn drops_a_sector_with_a_bad_data_checksum_and_counts_it() {
    let mut specs = sectors_for(2);
    specs[4].bad_data = true;
    let dec = decode_amiga_track(&pack_lsb_first(&write_track(&specs, TRACK_CELLS)), 2);
    assert_eq!(dec.bad_data, 1);
    assert!(!dec.sectors.contains_key(&4));
    assert_eq!(dec.sectors.len(), 10);
}

#[test]
fn drops_a_sector_whose_header_checksum_fails() {
    let mut cells = write_track(&sectors_for(2), TRACK_CELLS);
    // First sector: 64 preamble+sync cells, then info; flip a data cell in the label area.
    let at = 64 + 64 + 1;
    cells[at] ^= 1;
    let dec = decode_amiga_track(&pack_lsb_first(&cells), 2);
    assert_eq!(dec.bad_header, 1);
    assert!(!dec.sectors.contains_key(&0));
}

#[test]
fn rejects_headers_that_name_another_track() {
    let dec = decode_amiga_track(
        &pack_lsb_first(&write_track(&sectors_for(7), TRACK_CELLS)),
        8,
    );
    assert_eq!(dec.sectors.len(), 0);
    assert_eq!(dec.wrong_track, 11);
}

fn hfe_tracks(cylinders: usize, skip: Option<(usize, u32)>) -> Vec<Vec<Vec<u8>>> {
    (0..cylinders)
        .map(|cyl| {
            (0..2)
                .map(|head| {
                    let t = cyl * 2 + head;
                    let specs: Vec<SectorSpec> = sectors_for(t)
                        .into_iter()
                        .filter(|s| skip != Some((t, s.sector)))
                        .collect();
                    pack_lsb_first(&write_track(&specs, TRACK_CELLS))
                })
                .collect()
        })
        .collect()
}

#[test]
fn decodes_a_synthetic_80_cylinder_hfe_into_the_expected_adf() {
    let tracks = hfe_tracks(82, None);
    let hfe = parse_hfe(&write_hfe(&tracks, 250)).unwrap();
    assert_eq!((hfe.header.cylinders, hfe.header.sides), (82, 2));
    assert_eq!(
        (hfe.header.track_encoding, hfe.header.bit_rate_kbps),
        (1, 250)
    );
    assert_eq!(hfe.tracks[0][1], tracks[0][1]);
    let dec = decode_amiga_disk(&hfe);
    assert_eq!(dec.found, 1760);
    assert!(dec.missing.is_empty());
    let want: Vec<u8> = (0..1760).flat_map(payload).collect();
    assert_eq!(want.len(), ADF_SIZE);
    assert!(dec.adf == want);
}

#[test]
fn zero_fills_and_lists_missing_sectors() {
    let tracks = hfe_tracks(80, Some((3, 6)));
    let dec = decode_amiga_disk(&parse_hfe(&write_hfe(&tracks, 250)).unwrap());
    assert_eq!(dec.found, 1759);
    assert_eq!(dec.missing, vec![3 * 11 + 6]);
    assert!(
        dec.adf[(3 * 11 + 6) * 512..(3 * 11 + 7) * 512]
            .iter()
            .all(|&b| b == 0)
    );
}

#[test]
fn rejects_non_hfe_input_and_truncated_images() {
    let err = parse_hfe(&[0u8; 1024]).unwrap_err().to_string();
    assert!(err.contains("HXCPICFE"), "{err}");
    let one_cyl = vec![vec![
        pack_lsb_first(&write_track(&sectors_for(0), TRACK_CELLS)),
        pack_lsb_first(&write_track(&sectors_for(1), TRACK_CELLS)),
    ]];
    let one = write_hfe(&one_cyl, 250);
    let err = parse_hfe(&one[..one.len() - 600]).unwrap_err().to_string();
    assert!(err.contains("truncated"), "{err}");
}

#[test]
fn rejects_track_tables_whose_cylinders_overlap() {
    // 100 cylinders that all name the same 32 KiB of track data: 3.2 MB of tracks out of a
    // 33 KiB image. Disjoint track data (any image a writer lays out) never comes to more
    // than the image.
    let mut h = vec![0u8; 1024 + 32 * 1024];
    h[..8].copy_from_slice(b"HXCPICFE");
    h[9] = 100;
    h[10] = 2;
    h[11] = 1;
    h[12] = 250;
    h[18] = 1;
    for cyl in 0..100 {
        let at = 512 + cyl * 4;
        h[at..at + 2].copy_from_slice(&2u16.to_le_bytes());
        h[at + 2..at + 4].copy_from_slice(&(32u16 * 1024).to_le_bytes());
    }
    // (Not `unwrap_err`: on success it would print megabytes of tracks.)
    let err = match parse_hfe(&h) {
        Ok(_) => panic!("the table is refused"),
        Err(err) => err.to_string(),
    };
    assert!(err.contains("overlap"), "{err}");
    h[9] = 1;
    let one = parse_hfe(&h).unwrap();
    assert_eq!(one.tracks.len(), 1);
}
