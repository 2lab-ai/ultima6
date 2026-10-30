//! Synthetic containers only; no game data is embedded in these fixtures.
use super::*;

fn objlist() -> Vec<u8> {
    let mut bytes = vec![0; objlist::OBJLIST_LEN];
    bytes[0xfe0] = 1;
    bytes[0xff0] = 1;
    bytes
}

fn crc(bytes: &[u8]) -> u32 {
    let mut value = !0u32;
    for &byte in bytes {
        value ^= u32::from(byte);
        for _ in 0..8 {
            value = value >> 1 ^ if value & 1 != 0 { 0xedb8_8320 } else { 0 };
        }
    }
    !value
}

fn put16(bytes: &mut [u8], at: usize, value: usize) {
    bytes[at..at + 2].copy_from_slice(&(value as u16).to_le_bytes());
}

fn put32(bytes: &mut [u8], at: usize, value: usize) {
    bytes[at..at + 4].copy_from_slice(&(value as u32).to_le_bytes());
}

fn zip_files(files: &[(&str, &[u8])], deflate: bool, descriptor: bool) -> Vec<u8> {
    let (mut zip, mut central) = (Vec::new(), Vec::new());
    for (name, bytes) in files {
        let mut packed = Vec::new();
        if deflate {
            assert!(bytes.len() <= u16::MAX as usize);
            packed.push(1); // A final, uncompressed DEFLATE block.
            packed.extend((bytes.len() as u16).to_le_bytes());
            packed.extend((!(bytes.len() as u16)).to_le_bytes());
        }
        packed.extend_from_slice(bytes);
        let start = zip.len();
        let flags = 0x800 | if descriptor { 8 } else { 0 };
        let method = if deflate { 8 } else { 0 };
        let mut local = vec![0; 30];
        put32(&mut local, 0, 0x0403_4b50);
        put16(&mut local, 4, 20);
        put16(&mut local, 6, flags);
        put16(&mut local, 8, method);
        if !descriptor {
            put32(&mut local, 14, crc(bytes) as usize);
            put32(&mut local, 18, packed.len());
            put32(&mut local, 22, bytes.len());
        }
        put16(&mut local, 26, name.len());
        zip.extend(local);
        zip.extend(name.as_bytes());
        zip.extend(&packed);
        if descriptor {
            for n in [
                0x0807_4b50,
                crc(bytes),
                packed.len() as u32,
                bytes.len() as u32,
            ] {
                zip.extend(n.to_le_bytes());
            }
        }
        let mut entry = vec![0; 46];
        put32(&mut entry, 0, 0x0201_4b50);
        put16(&mut entry, 4, 20);
        put16(&mut entry, 6, 20);
        put16(&mut entry, 8, flags);
        put16(&mut entry, 10, method);
        put32(&mut entry, 16, crc(bytes) as usize);
        put32(&mut entry, 20, packed.len());
        put32(&mut entry, 24, bytes.len());
        put16(&mut entry, 28, name.len());
        put32(&mut entry, 42, start);
        central.extend(entry);
        central.extend(name.as_bytes());
    }
    let central_start = zip.len();
    let central_len = central.len();
    zip.extend(central);
    let mut end = vec![0; 22];
    put32(&mut end, 0, 0x0605_4b50);
    put16(&mut end, 8, files.len());
    put16(&mut end, 10, files.len());
    put32(&mut end, 12, central_len);
    put32(&mut end, 16, central_start);
    zip.extend(end);
    zip
}

fn valid_zip(deflate: bool, descriptor: bool) -> Vec<u8> {
    zip_files(
        &[
            (MANIFEST, b"{}"),
            (OBJLIST_FILE, &objlist()),
            ("savegame/objblkaa", &[0, 0]),
        ],
        deflate,
        descriptor,
    )
}

#[test]
fn save_transfer_stored_deflate_and_descriptors_preserve_every_payload_byte() {
    for deflate in [false, true] {
        for descriptor in [false, true] {
            let transfer = SaveTransfer::decode(&valid_zip(deflate, descriptor)).unwrap();
            assert_eq!(transfer.files[OBJLIST_FILE], objlist());
            assert_eq!(transfer.files[MANIFEST], b"{}");
            assert_eq!(transfer.files["savegame/objblkaa"], [0, 0]);
            assert_eq!(transfer.count(), 3);
        }
    }
}

#[test]
fn save_transfer_paths_and_casefold_duplicates_are_rejected_before_payload_reading() {
    for path in [
        "../savegame/objlist",
        "/savegame/objlist",
        "savegame\\objlist",
        "savegame/a/../objlist",
        "savegame/objblkfi",
        "savegame/objlist.tmp",
        "data1/objlist",
        "packs/test",
        "models/test",
        "savegame/",
        "manifest.json/",
    ] {
        let data = zip_files(&[(MANIFEST, b"{}"), (path, b"not a payload")], false, false);
        assert!(
            matches!(SaveTransfer::decode(&data), Err(Error::Path)),
            "path refused"
        );
    }
    let data = zip_files(
        &[
            (MANIFEST, b"{}"),
            (OBJLIST_FILE, &objlist()),
            ("SAVEGAME/OBJLIST", &objlist()),
        ],
        false,
        false,
    );
    assert!(matches!(SaveTransfer::decode(&data), Err(Error::Duplicate)));
    let mut builder = SaveTransfer::new();
    builder.insert("SAVEGAME/OBJLIST", &objlist()).unwrap();
    assert_eq!(
        builder.insert(OBJLIST_FILE, b"replacement"),
        Err(Error::Duplicate.code())
    );
    assert_eq!(
        builder.files[OBJLIST_FILE],
        objlist(),
        "refused insert leaves existing bytes intact"
    );
    for i in 0..u6_core::objects::BLOCK_COUNT {
        builder
            .insert(&u6_core::saveload::block_file(i), &[0, 0])
            .unwrap();
    }
    assert_eq!(builder.check(), Ok(()));
}

#[test]
fn save_transfer_preflights_entry_archive_and_unpacked_budgets() {
    assert!(matches!(
        SaveTransfer::decode(&vec![0; ARCHIVE_LIMIT + 1]),
        Err(Error::Size)
    ));
    let mut zip = valid_zip(false, false);
    let end = zip.len() - 22;
    put16(&mut zip, end + 8, ENTRY_LIMIT + 1);
    put16(&mut zip, end + 10, ENTRY_LIMIT + 1);
    assert!(matches!(SaveTransfer::decode(&zip), Err(Error::Entries)));
    for (path, size) in [
        (MANIFEST, MANIFEST_LIMIT + 1),
        (OBJLIST_FILE, MAX_SAVE_FILE_BYTES + 1),
        (SIGHT_FILE, MAX_SIGHT_FILE_BYTES + 1),
    ] {
        let mut zip = zip_files(&[(path, b"x")], true, false);
        let end = zip.len() - 22;
        let central = word(&zip, end + 16).unwrap() as usize;
        put32(&mut zip, central + 24, size);
        // Even a broken CRC cannot defer the declared-size rejection until inflation.
        put32(&mut zip, central + 16, 0);
        assert!(matches!(SaveTransfer::decode(&zip), Err(Error::Size)));
    }
    let mut budget = Budget {
        ordinary: MAX_SAVE_BYTES,
        total: MAX_SAVE_BYTES,
    };
    assert_eq!(budget.add(OBJLIST_FILE, 1), Err(Error::Size));
}

#[test]
fn save_transfer_rejects_crc_mismatch_truncation_hidden_entries_and_trailing_bytes() {
    let zip = valid_zip(false, false);
    for len in [0, 3, 21, zip.len() - 1] {
        assert!(SaveTransfer::decode(&zip[..len]).is_err());
    }
    let mut bad = zip.clone();
    bad[30 + MANIFEST.len()] ^= 1;
    assert!(matches!(SaveTransfer::decode(&bad), Err(Error::Zip)));
    let mut bad = zip.clone();
    bad.extend(b"hidden bytes");
    assert!(matches!(SaveTransfer::decode(&bad), Err(Error::Zip)));
    // One local file is no longer in the directory, but still occupies the archive.
    let end = zip.len() - 22;
    let central = word(&zip, end + 16).unwrap() as usize;
    let hidden_entry = 46 + "savegame/objblkaa".len();
    let mut hidden = zip[..central].to_vec();
    hidden.extend_from_slice(&zip[central..end - hidden_entry]);
    let mut eocd = zip[end..].to_vec();
    put16(&mut eocd, 8, 2);
    put16(&mut eocd, 10, 2);
    put32(&mut eocd, 12, end - central - hidden_entry);
    hidden.extend(eocd);
    assert!(matches!(SaveTransfer::decode(&hidden), Err(Error::Zip)));
    let mut bad = zip.clone();
    put32(&mut bad, central + 42, u32::MAX as usize);
    assert!(matches!(SaveTransfer::decode(&bad), Err(Error::Zip)));
    let mut bad = zip;
    put16(&mut bad, 6, 0x801);
    put16(&mut bad, central + 8, 0x801);
    assert!(matches!(SaveTransfer::decode(&bad), Err(Error::Zip)));
}

#[test]
fn save_transfer_requires_manifest_and_objlist_and_validates_exact_blocks() {
    let no_manifest = zip_files(&[(OBJLIST_FILE, &objlist())], false, false);
    assert!(matches!(
        SaveTransfer::decode(&no_manifest),
        Err(Error::Manifest)
    ));
    let no_objlist = zip_files(&[(MANIFEST, b"{}")], false, false);
    assert!(matches!(
        SaveTransfer::decode(&no_objlist),
        Err(Error::ObjlistMissing)
    ));
    for block in [vec![], vec![0], vec![0, 1], vec![0, 0, 0], vec![0xff, 0xff]] {
        let zip = zip_files(
            &[
                (MANIFEST, b"{}"),
                (OBJLIST_FILE, &objlist()),
                ("savegame/objblkei", &block),
            ],
            false,
            false,
        );
        assert!(matches!(SaveTransfer::decode(&zip), Err(Error::Block)));
    }
    let mut max_block = vec![0; MAX_SAVE_FILE_BYTES];
    max_block[..2].copy_from_slice(&(MAX_RECORDS as u16).to_be_bytes());
    let mut builder = SaveTransfer::new();
    builder.insert(OBJLIST_FILE, &objlist()).unwrap();
    builder.insert("savegame/objblkei", &max_block).unwrap();
    assert_eq!(builder.check(), Ok(()), "maximum valid block accepted");
}

#[test]
fn save_transfer_accepts_handoff_normal_vehicle_and_sentinel_rosters() {
    let mut state = u6_core::State::empty();
    state.actors.party.names = vec![String::new(); PARTY_SLOTS];
    u6_core::intro::handoff::apply(&mut state, &objlist());
    let mut builder = SaveTransfer::new();
    builder
        .insert(OBJLIST_FILE, state.saveload.objlist.as_ref().unwrap())
        .unwrap();
    assert_eq!(
        builder.check(),
        Ok(()),
        "character handoff's bytes accepted"
    );
    for (size, leader) in [(1, 0), (1, 1), (4, 2), (15, 15), (4, 16)] {
        state.actors.party.size = size;
        state.actors.party.leader_index = leader;
        let mut builder = SaveTransfer::new();
        builder
            .insert(OBJLIST_FILE, &objlist::encode(&state))
            .unwrap();
        assert_eq!(builder.check(), Ok(()));
    }
    for (size, leader) in [(0, 0), (16, 0), (1, 2), (4, 17)] {
        let mut bytes = objlist();
        bytes[0xff0] = size;
        bytes[objlist::TAIL + 0xa] = leader;
        let mut builder = SaveTransfer::new();
        builder.insert(OBJLIST_FILE, &bytes).unwrap();
        assert_eq!(builder.check(), Err(Error::Objlist.code()));
    }
    let mut builder = SaveTransfer::new();
    builder.insert(OBJLIST_FILE, &[0; 2]).unwrap();
    assert_eq!(builder.check(), Err(Error::Objlist.code()));
}

#[test]
fn save_transfer_checks_sight_structure_and_objlist_binding_without_dropping_it() {
    let objlist = objlist();
    let digest = objlist.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    let mut sight = vec![0; 44];
    sight[..8].copy_from_slice(b"u6sight\0");
    put32(&mut sight, 8, 1);
    put32(&mut sight, 12, objlist.len());
    sight[16..24].copy_from_slice(&digest.to_le_bytes());
    for (bytes, error) in [
        (sight.clone(), None),
        (
            {
                let mut b = sight.clone();
                b[16] ^= 1;
                b
            },
            Some(Error::SightMismatch),
        ),
        (
            {
                let mut b = sight;
                b[8] = 2;
                b
            },
            Some(Error::Sight),
        ),
    ] {
        let mut builder = SaveTransfer::new();
        builder.insert(OBJLIST_FILE, &objlist).unwrap();
        builder.insert(SIGHT_FILE, &bytes).unwrap();
        assert_eq!(builder.check_inner(), error.map_or(Ok(()), Err));
        assert_eq!(
            builder.files[SIGHT_FILE], bytes,
            "validation never changes the payload"
        );
    }
}
