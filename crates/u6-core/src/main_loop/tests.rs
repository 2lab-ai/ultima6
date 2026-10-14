//! Main-hunk helpers and the command loop on synthetic states, and the boot on the user's
//! files (skipped when the archive is absent; reads it, copies nothing).

use std::path::Path;

use super::*;
use crate::files::Files;

const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";

#[allow(clippy::disallowed_methods)] // tests may read files
fn files() -> Option<Files> {
    if !Path::new(ZIP).exists() {
        eprintln!("skipped: {ZIP} not present");
        return None;
    }
    let data = std::fs::read(ZIP).unwrap();
    let r = u6_intake::intake(vec![u6_intake::InputFile {
        name: "ultima-vi.zip".into(),
        data,
    }]);
    Some(Files::new(r.files))
}

#[test]
#[ignore]
fn dump_tail() {
    let Some(files) = files() else { return };
    let s = State::new_game(&files).unwrap();
    for (i, b) in s.actors.objlist_tail.iter().enumerate() {
        print!("{i:02x}:{b:02x} ");
    }
    println!();
}
