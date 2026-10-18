//! The out dir's steps on made-up runs (files the tests write, nothing else), each race made
//! deterministic by a hook at a step ([`seam`]).

use std::collections::BTreeMap;
use std::sync::Arc;

use super::*;

/// A fresh temp dir of the test's own.
fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("u6-hdtool-outdir-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The files of `d` (name → bytes; a directory in it → `"<dir>"`), or `None` when nothing is
/// at `d`.
fn contents(d: &Path) -> Option<BTreeMap<String, Vec<u8>>> {
    let entries = std::fs::read_dir(d).ok()?;
    Some(
        entries
            .map(|e| {
                let e = e.unwrap();
                let name = e.file_name().to_string_lossy().into_owned();
                let bytes = if e.path().is_dir() {
                    b"<dir>".to_vec()
                } else {
                    std::fs::read(e.path()).unwrap()
                };
                (name, bytes)
            })
            .collect(),
    )
}

/// The names in `d` that start with `.<name>.` (staging and moved-aside dirs of the out dir
/// `name`).
fn beside_of(d: &Path, name: &str) -> Vec<PathBuf> {
    let prefix = format!(".{name}.");
    let mut v: Vec<PathBuf> = std::fs::read_dir(d)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(&prefix)
        })
        .collect();
    v.sort();
    v
}

/// A run that writes `a.txt` holding `text`.
fn writes(text: &'static str) -> impl FnOnce(&Path, &Path) -> Result<(), String> {
    move |dir: &Path, _: &Path| work::write_file(&dir.join("a.txt"), text.as_bytes())
}

/// What a run of [`writes`]`(text)` leaves at the out dir.
fn output(text: &str) -> BTreeMap<String, Vec<u8>> {
    BTreeMap::from([
        (FIRST.marker.to_string(), b"calibrate\na.txt\n".to_vec()),
        ("a.txt".to_string(), text.as_bytes().to_vec()),
    ])
}

#[test]
fn an_unmarked_dir_put_at_the_out_dir_during_the_run_survives_whole() {
    // The old output there when the run begins (it passes the check before the run), then the
    // owner's dir put in its place before the out dir is replaced: moved aside, found unmarked,
    // put back as it was, and the run refused.
    let dir = temp("unmarked-ran");
    let out = dir.join("out");
    run_in(FIRST, &out, writes("old")).unwrap();
    let owners = BTreeMap::from([("notes.txt".to_string(), b"the owner's".to_vec())]);
    let (o, d) = (out.clone(), dir.clone());
    let _h = hook(&out, "ran", move |_| {
        std::fs::rename(&o, d.join("kept")).unwrap();
        std::fs::create_dir(&o).unwrap();
        std::fs::write(o.join("notes.txt"), b"the owner's").unwrap();
        Ok(())
    });
    let e = run_in(FIRST, &out, writes("new")).unwrap_err();
    let said = "(no marker .calibrate-first-pass), so it is not replaced: move it away, or pass another --out";
    assert!(e.contains(said), "{e}");
    assert_eq!(contents(&out), Some(owners));
    assert!(
        beside_of(&dir, "out").is_empty(),
        "{:?}",
        beside_of(&dir, "out")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_unmarked_dir_put_at_the_out_dir_once_it_is_checked_survives_whole() {
    // The old output moved aside and checked, then the owner's dir put at the out dir before
    // the new one is: the rename that cannot replace fails, the owner's dir stays as it was,
    // and the old output, which cannot go back, is said to be where it is.
    let dir = temp("unmarked-checked");
    let out = dir.join("out");
    run_in(FIRST, &out, writes("old")).unwrap();
    let owners = BTreeMap::from([("notes.txt".to_string(), b"the owner's".to_vec())]);
    let o = out.clone();
    let _h = hook(&out, "checked", move |_| {
        std::fs::create_dir(&o).unwrap();
        std::fs::write(o.join("notes.txt"), b"the owner's").unwrap();
        Ok(())
    });
    let e = run_in(FIRST, &out, writes("new")).unwrap_err();
    assert!(
        e.contains("so it is not replaced: move it away, or pass another --out"),
        "{e}"
    );
    assert_eq!(contents(&out), Some(owners));
    let left = beside_of(&dir, "out");
    assert_eq!(left.len(), 1, "{left:?}");
    let at = left[0].file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        e.contains("the old output is at ") && e.ends_with(&at),
        "{e}"
    );
    assert_eq!(contents(&left[0]), Some(output("old")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_empty_dir_put_at_an_absent_out_dir_survives_and_the_run_is_refused() {
    // No out dir when the run begins; an empty dir made there before the new one is put in
    // place: a plain rename would replace it, the one used cannot.
    let dir = temp("empty-appears");
    let out = dir.join("out");
    let o = out.clone();
    let _h = hook(&out, "checked", move |_| {
        std::fs::create_dir(&o).unwrap();
        Ok(())
    });
    let e = run_in(FIRST, &out, writes("new")).unwrap_err();
    assert!(
        e.contains("so it is not replaced: move it away, or pass another --out"),
        "{e}"
    );
    assert_eq!(contents(&out), Some(BTreeMap::new()));
    assert!(
        beside_of(&dir, "out").is_empty(),
        "{:?}",
        beside_of(&dir, "out")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_crash_between_any_two_steps_leaves_the_old_output_or_the_new() {
    // At each step the files are what a crash there would leave: the out dir the old output,
    // or the new, whole; or, between moving the old aside and putting the new in place, no out
    // dir and the old output whole beside it. Never a mix, never neither.
    let dir = temp("crash");
    let out = dir.join("out");
    run_in(FIRST, &out, writes("old")).unwrap();
    let seen: Arc<std::sync::Mutex<Vec<(&'static str, &'static str)>>> = Arc::default();
    let mut hooks = Vec::new();
    for step in ["ran", "moved", "checked", "published"] {
        let (o, d, seen) = (out.clone(), dir.clone(), seen.clone());
        hooks.push(hook(&out, step, move |_| {
            let at_out = contents(&o);
            let old_aside = beside_of(&d, "out")
                .iter()
                .any(|p| contents(p) == Some(output("old")));
            let state = if at_out == Some(output("old")) {
                "old"
            } else if at_out == Some(output("new")) {
                "new"
            } else if at_out.is_none() && old_aside {
                "old beside"
            } else {
                panic!("{step}: at the out dir {at_out:?}, the old output beside it: {old_aside}")
            };
            seen.lock().unwrap().push((step, state));
            Ok(())
        }));
    }
    run_in(FIRST, &out, writes("new")).unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        [
            ("ran", "old"),
            ("moved", "old beside"),
            ("checked", "old beside"),
            ("published", "new")
        ]
    );
    assert_eq!(contents(&out), Some(output("new")));
    assert!(
        beside_of(&dir, "out").is_empty(),
        "{:?}",
        beside_of(&dir, "out")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_symlinked_out_dir_is_refused_and_left_as_it_is() {
    // The out dir a link to an out dir this mode wrote: a rename would move the link, so it is
    // refused, before the run and when one is put there during it; the link and the dir it
    // points to stay as they are.
    let dir = temp("symlink");
    let (real, out) = (dir.join("real"), dir.join("out"));
    run_in(FIRST, &real, writes("old")).unwrap();
    std::os::unix::fs::symlink(&real, &out).unwrap();
    let said = "a symlink (a rename would move the link, not the directory it points to), so it \
                is not replaced: pass the directory it points to as --out, or another --out";
    let e = run_in(FIRST, &out, writes("new")).unwrap_err();
    assert!(e.contains(said), "{e}");
    let linked = |p: &Path| std::fs::read_link(p).ok();
    assert_eq!(linked(&out), Some(real.clone()));
    assert_eq!(contents(&real), Some(output("old")));
    assert!(
        beside_of(&dir, "out").is_empty(),
        "{:?}",
        beside_of(&dir, "out")
    );
    // A dir there when the run begins, a link put in its place before it is replaced.
    std::fs::remove_file(&out).unwrap();
    run_in(FIRST, &out, writes("old")).unwrap();
    let (o, d, r) = (out.clone(), dir.clone(), real.clone());
    let _h = hook(&out, "ran", move |_| {
        std::fs::rename(&o, d.join("kept")).unwrap();
        std::os::unix::fs::symlink(&r, &o).unwrap();
        Ok(())
    });
    let e = run_in(FIRST, &out, writes("new")).unwrap_err();
    assert!(e.contains(said), "{e}");
    assert_eq!(linked(&out), Some(real.clone()));
    assert_eq!(contents(&real), Some(output("old")));
    assert!(
        beside_of(&dir, "out").is_empty(),
        "{:?}",
        beside_of(&dir, "out")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_error_names_the_out_dir_and_the_dir_the_run_wrote_first_only_after() {
    // A run that fails naming a file of the dir it writes: the error, and refused.txt, name
    // the out dir's file; the dir written first is said once, at the end.
    let dir = temp("named");
    let out = dir.join("out");
    let e = run_in(FIRST, &out, |d: &Path, _: &Path| -> Result<(), String> {
        Err(format!("{}: no room left", d.join("a.txt").display()))
    })
    .unwrap_err();
    let real = dir.canonicalize().unwrap().join("out");
    let want = format!(
        "{}: no room left (the run wrote to ",
        real.join("a.txt").display()
    );
    assert!(e.starts_with(&want), "{e}");
    assert!(e.ends_with(", which is not kept)"), "{e}");
    assert_eq!(e.matches(".out.staging-").count(), 1, "{e}");
    let refused = std::fs::read_to_string(out.join("refused.txt")).unwrap();
    assert_eq!(refused, format!("{e}\n"));
    // An error that names no file of it is as it was.
    let e = run_in(FIRST, &out, |_: &Path, _: &Path| -> Result<(), String> {
        Err("no rows".into())
    })
    .unwrap_err();
    assert_eq!(e, "no rows");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_marked_out_dir_holding_a_file_the_run_did_not_write_is_refused_and_left_whole() {
    // The marker lists what the run wrote; a file put in after is named, and the dir kept as
    // it is, before the run and when one is put there during it.
    let dir = temp("foreign");
    let out = dir.join("out");
    run_in(FIRST, &out, writes("old")).unwrap();
    std::fs::write(out.join("mine.txt"), b"the owner's").unwrap();
    let mut had = output("old");
    had.insert("mine.txt".into(), b"the owner's".to_vec());
    let said = "it holds what calibrate did not write: mine.txt, so it is not replaced: move \
                those out of it, or pass another --out";
    let e = run_in(FIRST, &out, writes("new")).unwrap_err();
    assert!(e.contains(said), "{e}");
    assert_eq!(contents(&out), Some(had.clone()));
    std::fs::remove_file(out.join("mine.txt")).unwrap();
    let o = out.clone();
    let _h = hook(&out, "ran", move |_| {
        std::fs::write(o.join("mine.txt"), b"the owner's").unwrap();
        Ok(())
    });
    let e = run_in(FIRST, &out, writes("new")).unwrap_err();
    assert!(e.contains(said), "{e}");
    assert_eq!(contents(&out), Some(had));
    assert!(
        beside_of(&dir, "out").is_empty(),
        "{:?}",
        beside_of(&dir, "out")
    );
    let _ = std::fs::remove_dir_all(&dir);
}
