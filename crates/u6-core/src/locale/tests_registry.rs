#![allow(clippy::disallowed_methods)] // Local test input only.

pub(super) fn owner_game() -> Option<Vec<u8>> {
    use u6_testkit::Quiet;
    if !u6_testkit::archive().exists() {
        eprintln!("skipped: the archive (U6_ARCHIVE) not present");
        return None;
    }
    let data = std::fs::read(u6_testkit::archive()).quiet();
    let r = u6_intake::intake(vec![u6_intake::InputFile {
        name: "ultima-vi.zip".into(),
        data,
    }]);
    let game = crate::files::Files::new(r.files)
        .get("game")
        .map(<[u8]>::to_vec);
    assert!(game.is_some(), "the owner's zip gives a game");
    game
}

#[test]
fn registered_readers_preserve_the_image_and_loaded_data_on_the_owners_game() {
    use u6_testkit::Quiet;
    let Some(game) = owner_game() else { return };
    let image = crate::image::GameImage::parse(&game).quiet();
    let data = crate::objects::commands::GameData::from_game(&game).quiet();
    let mut count = 0;
    for (source, _, _) in crate::string_registry::entries() {
        let expected = if source.is_pointed() {
            image.pointer(source.address()).and_then(|p| image.cstr(p))
        } else {
            image.cstr(source.address())
        };
        let actual = source.image(&image);
        assert!(
            actual.is_ok() == expected.is_ok(),
            "same registered read validity"
        );
        if let (Ok(actual), Ok(expected)) = (actual, expected) {
            u6_testkit::assert_same!(actual, expected, "registered image read");
        }
        let address = if source.is_pointed() {
            data.long(source.address())
        } else {
            source.address()
        };
        u6_testkit::assert_same!(
            source.data(&data),
            data.cstr(address),
            "registered loaded read"
        );
        count += 1;
    }
    assert!(
        count > 500,
        "all registered game sources checked without exposing their contents"
    );
}
