//! Synthetic files only ([`crate::synthetic`]): every text is the test's own tag.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use u6_core::Files;
use u6_core::converse::disasm::bundle;
use u6_core::locale::strings;

use crate::{Args, Export, parse, run, synthetic, write_private};

const SENTINEL: &str = "Zq5ExportSentinel";

/// A fresh directory outside the repository.
fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("u6-export-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// `files` written under `dir` (paths relative to it).
fn put(dir: &Path, files: &[(&str, &[u8])]) {
    for (name, bytes) in files {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
}

fn files(entries: &[(&str, &[u8])]) -> Files {
    Files::new(
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_vec()))
            .collect::<BTreeMap<_, _>>(),
    )
}

fn args(export: Export, game: &Path, out: &Path) -> Args {
    Args {
        export,
        game: game.into(),
        out: out.into(),
    }
}

fn mode(p: &Path) -> u32 {
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

/// The strings of a folder of extracted files are the u6-core export of the files the page
/// keeps, byte for byte (the files it drops change nothing), and the line names the file and
/// its counts only.
#[test]
fn the_strings_are_the_cores_export_byte_for_byte() {
    let dir = temp("strings");
    let game = synthetic::game("Zq string");
    let script = synthetic::converse(&synthetic::script(SENTINEL));
    put(
        &dir.join("game"),
        &[
            ("U6/GAME", &game),
            ("U6/converse.a", &script),
            ("U6/notes.txt", SENTINEL.as_bytes()),
        ],
    );
    let out = dir.join("out/strings.json");
    let line = run(&args(Export::Strings, &dir.join("game"), &out)).unwrap();
    let written = std::fs::read_to_string(&out).unwrap();
    let core = strings::export(&files(&[("U6/GAME", &game)])).unwrap();
    assert_eq!(written, core.to_json());
    assert!(
        !core.strings.is_empty(),
        "the synthetic game has catalogued strings"
    );
    let back = strings::Export::from_json(&written).unwrap();
    assert_eq!(back.strings.len(), core.strings.len());
    assert_eq!(
        line,
        format!(
            "wrote {} ({} game strings, 0 object names and books, 0 strings of u)",
            out.display(),
            core.strings.len()
        )
    );
    assert!(!line.contains("Zq string") && !line.contains(SENTINEL));
}

/// The bundles of a folder, and of the one library given alone, are the u6-core export of the
/// files the page keeps, byte for byte.
#[test]
fn the_bundles_are_the_cores_export_byte_for_byte() {
    let dir = temp("bundles");
    let lib = synthetic::converse(&synthetic::script(SENTINEL));
    put(
        &dir.join("game"),
        &[
            ("U6/CONVERSE.A", &lib),
            ("U6/look.lzd", &[4]),
            (SENTINEL, &[1, 2, 3]),
        ],
    );
    let core = bundle::to_json(&bundle::export(&files(&[("U6/CONVERSE.A", &lib)])).unwrap());
    for (game, out) in [
        (dir.join("game"), dir.join("a.json")),
        (dir.join("game/U6/CONVERSE.A"), dir.join("b.json")),
    ] {
        let line = run(&args(Export::Bundles, &game, &out)).unwrap();
        let written = std::fs::read_to_string(&out).unwrap();
        assert_eq!(written, core, "{}", game.display());
        assert_eq!(bundle::from_json(&written).unwrap().len(), 1);
        assert_eq!(line, format!("wrote {} (1 bundles)", out.display()));
    }
}

/// A missing or unreadable input fails with the page's sentence for what failed (never a byte or
/// a name of the files), and nothing is written.
#[test]
fn a_missing_input_fails_naming_only_what_failed() {
    let dir = temp("missing");
    put(
        &dir.join("game"),
        &[(SENTINEL, SENTINEL.as_bytes()), ("U6/objlist", &[0; 4])],
    );
    let out = dir.join("out.json");
    let cases = [
        (
            Export::Strings,
            "Game strings: the files hold no game executable",
        ),
        (
            Export::Bundles,
            "NPC bundles: the files hold no conversation archive",
        ),
    ];
    for (export, want) in cases {
        assert_eq!(
            run(&args(export, &dir.join("game"), &out)),
            Err(want.into())
        );
    }
    let bad = temp("unreadable");
    put(
        &bad,
        &[
            ("U6/game", SENTINEL.as_bytes()),
            (
                "U6/converse.b",
                &[&4u32.to_le_bytes()[..], SENTINEL.as_bytes()].concat(),
            ),
        ],
    );
    assert_eq!(
        run(&args(Export::Strings, &bad, &out)),
        Err("Game strings: the game executable does not read".into())
    );
    assert_eq!(
        run(&args(Export::Bundles, &bad, &out)),
        Err("NPC bundles: converse.b does not read".into())
    );
    let gone = dir.join("no-such-game");
    let e = run(&args(Export::Bundles, &gone, &out)).unwrap_err();
    assert!(
        e.starts_with(&format!(
            "the game files cannot be read: {}: ",
            gone.display()
        )),
        "{e}"
    );
    let empty = temp("empty");
    assert_eq!(
        run(&args(Export::Bundles, &empty, &out)),
        Err(format!(
            "the game files cannot be read: {}: no files",
            empty.display()
        ))
    );
    assert!(!out.exists(), "a failed export writes nothing");
}

/// The file is 0600 whatever the umask, the directories made for it 0700 (an existing one keeps
/// its mode); a second export replaces it; a link in its place is refused and left alone.
#[test]
fn the_file_is_0600_in_directories_made_0700() {
    let dir = temp("modes");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = dir.join("new/deeper/x.json");
    assert_eq!(write_private(&out, b"[1]").unwrap(), out);
    assert_eq!(mode(&out), 0o600);
    assert_eq!(mode(&dir.join("new")), 0o700);
    assert_eq!(mode(&dir.join("new/deeper")), 0o700);
    assert_eq!(mode(&dir), 0o755, "an existing directory is left as it is");
    write_private(&out, b"[2]").unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), b"[2]");
    assert_eq!(mode(&out), 0o600);
    let names: Vec<_> = std::fs::read_dir(dir.join("new/deeper"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["x.json"], "no temporary file is left");
    let link = dir.join("link.json");
    std::os::unix::fs::symlink(&out, &link).unwrap();
    let e = write_private(&link, b"[3]").unwrap_err();
    assert!(e.ends_with("exists and is not a plain file"), "{e}");
    assert_eq!(std::fs::read(&out).unwrap(), b"[2]");
}

/// An `--out` that is the one file given as the game is refused, nothing read is changed.
#[test]
fn the_single_game_file_as_out_is_refused_and_left_as_it_is() {
    let dir = temp("same-file");
    let lib = synthetic::converse(&synthetic::script(SENTINEL));
    put(&dir, &[("converse.a", &lib)]);
    let game = dir.join("converse.a");
    for out in [game.clone(), dir.join("./converse.a")] {
        let e = run(&args(Export::Bundles, &game, &out)).unwrap_err();
        assert_eq!(
            e,
            format!(
                "{}: one of the game files read, or inside a folder of them; the export is \
                 written only elsewhere",
                out.display()
            )
        );
        assert!(!e.contains(SENTINEL));
        assert_eq!(
            std::fs::read(&game).unwrap(),
            lib,
            "the game file is unchanged"
        );
    }
}

/// An `--out` inside a folder given as the game (a file of it, a file of a new folder in it,
/// or the same place reached through a link to the folder) is refused: no game file changes and
/// no folder is made in it.
#[test]
fn an_out_inside_the_game_folder_is_refused_and_nothing_changes() {
    let dir = temp("in-folder");
    let game = synthetic::game("Zq string");
    let lib = synthetic::converse(&synthetic::script(SENTINEL));
    put(
        &dir.join("game"),
        &[("U6/GAME", &game), ("U6/converse.a", &lib)],
    );
    std::os::unix::fs::symlink(dir.join("game"), dir.join("alias")).unwrap();
    let cases = [
        (Export::Strings, dir.join("game/U6/GAME")),
        (Export::Bundles, dir.join("game/U6/converse.a")),
        (Export::Bundles, dir.join("game/new/x.json")),
        (Export::Strings, dir.join("alias/U6/GAME")),
    ];
    for (export, out) in cases {
        let e = run(&args(export, &dir.join("game"), &out)).unwrap_err();
        assert!(
            e.starts_with(&format!("{}: one of the game files read", out.display())),
            "{e}"
        );
        assert!(!e.contains(SENTINEL) && !e.contains("Zq string"));
    }
    // A link to the folder, given as the game, guards the folder it names.
    let e = run(&args(
        Export::Bundles,
        &dir.join("alias"),
        &dir.join("game/U6/converse.a"),
    ))
    .unwrap_err();
    assert!(e.contains("one of the game files read"), "{e}");
    assert_eq!(std::fs::read(dir.join("game/U6/GAME")).unwrap(), game);
    assert_eq!(std::fs::read(dir.join("game/U6/converse.a")).unwrap(), lib);
    assert!(!dir.join("game/new").exists(), "no folder is made in it");
    // A `..` after a folder that does not exist is refused before anything is read.
    let out = dir.join("none/../game/U6/converse.a");
    assert_eq!(
        run(&args(Export::Bundles, &dir.join("game"), &out)),
        Err(format!(
            "{}: a `..` after a folder that does not exist is refused",
            out.display()
        ))
    );
    assert!(!dir.join("none").exists());
    // Beside the folder is fine.
    run(&args(
        Export::Bundles,
        &dir.join("game"),
        &dir.join("out.json"),
    ))
    .unwrap();
}

/// Inside a git work tree nothing is read, made or written.
#[test]
fn inside_a_git_work_tree_nothing_is_written() {
    let dir = temp("git");
    std::fs::create_dir_all(dir.join("tree/.git")).unwrap();
    let lib = synthetic::converse(&synthetic::script(SENTINEL));
    put(&dir, &[("game/converse.a", &lib)]);
    let out = dir.join("tree/sub/x.json");
    let e = run(&args(Export::Bundles, &dir.join("game"), &out)).unwrap_err();
    assert!(
        e.contains(&format!(
            "inside the git work tree {}",
            dir.join("tree").display()
        )),
        "{e}"
    );
    assert!(!dir.join("tree/sub").exists());
}

#[test]
fn the_command_line_takes_a_command_a_game_and_an_out_file() {
    let p = |a: &[&str]| parse(a.iter().map(|s| s.to_string()));
    assert_eq!(
        p(&["strings", "--game", "g", "--out", "o"]),
        Ok(args(Export::Strings, Path::new("g"), Path::new("o")))
    );
    assert_eq!(
        p(&["bundles", "--out", "o", "--game", "g"]),
        Ok(args(Export::Bundles, Path::new("g"), Path::new("o")))
    );
    assert_eq!(p(&[]), Err("no command".into()));
    assert_eq!(p(&["texts"]), Err("unknown command \"texts\"".into()));
    assert_eq!(
        p(&["strings", "--out", "o"]),
        Err("give --game PATH (your game files)".into())
    );
    assert_eq!(
        p(&["strings", "--game", "g"]),
        Err("give --out FILE".into())
    );
    assert_eq!(p(&["strings", "--game"]), Err("--game needs a path".into()));
    assert_eq!(
        p(&["strings", "--game", "g", "--out", "o", "--send"]),
        Err("unknown argument \"--send\"".into())
    );
}
