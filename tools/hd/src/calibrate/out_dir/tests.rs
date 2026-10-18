//! The run dir's rules on made-up runs (files the tests write, nothing else), each race made
//! deterministic by a hook at a step ([`seam`]) and the clock fixed where a name must repeat
//! ([`CLOCK`]).

use std::collections::BTreeMap;

use super::*;

/// A fresh temp dir of the test's own.
fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("u6-hdtool-outdir-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Everything under `d`, by its path from `d` (a dir as `<dir>`), with each file's bytes.
fn tree(d: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut todo = vec![d.to_path_buf()];
    while let Some(at) = todo.pop() {
        for e in std::fs::read_dir(&at).unwrap() {
            let p = e.unwrap().path();
            let rel = p.strip_prefix(d).unwrap().to_string_lossy().into_owned();
            if p.is_dir() {
                out.insert(rel, b"<dir>".to_vec());
                todo.push(p);
            } else {
                out.insert(rel, std::fs::read(&p).unwrap());
            }
        }
    }
    out
}

/// The names in `d`, sorted.
fn names(d: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(d)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

/// A run that writes `a.png` and `a.txt`.
fn writes(run: &Run) -> Result<(), String> {
    run.write("a.txt", b"a")?;
    run.write_png("a.png", &Rgba::new(1, 1))
}

/// A run that writes as [`writes`], then the mode's snapshot, then fails.
fn fails(run: &Run) -> Result<(), String> {
    writes(run)?;
    run.write("gates.jsonl", b"snapshot")?;
    Err("no rows".into())
}

/// The clock of the runs whose `--out` is `key` fixed at `ns`, until dropped.
struct Fixed(PathBuf);

impl Drop for Fixed {
    fn drop(&mut self) {
        CLOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(k, _)| *k != self.0);
    }
}

fn fix_clock(key: &Path, ns: u128) -> Fixed {
    CLOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push((key.to_path_buf(), ns));
    Fixed(key.to_path_buf())
}

#[test]
fn a_run_dir_is_named_by_the_utc_time_and_the_pid() {
    assert_eq!(
        run_name(1_792_319_367_408_836_123, 4321),
        "run-20261018T102927.408836123Z-4321"
    );
    assert_eq!(run_name(0, 1), "run-19700101T000000.000000000Z-1");
    // By the time, the names sort as the runs began.
    assert!(run_name(1_000_000_000, 99) < run_name(1_000_000_001, 1));
}

#[test]
fn a_run_writes_its_own_dir_alone_and_done_last() {
    // A --out not there yet: made; the run's dir in it, nothing else; done last.
    let dir = temp("own-dir");
    let out = dir.join("out");
    let ((), w) = run_in(FIRST, &out, writes).unwrap();
    assert_eq!(w.dir.parent().unwrap(), out.canonicalize().unwrap());
    let run = w.dir.file_name().unwrap().to_string_lossy().into_owned();
    assert!(run.starts_with("run-") && run.ends_with(&format!("-{}", std::process::id())));
    assert_eq!(names(&out), std::slice::from_ref(&run));
    assert_eq!(names(&w.dir), ["a.png", "a.txt", DONE]);
    assert_eq!(names(&dir), ["out"]);
    assert!(
        w.text()
            .starts_with(&format!("run dir: {}\n", w.dir.display()))
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_run_dir_that_is_there_already_is_never_reused() {
    // Two runs at the same instant of the same process: the second's dir is there, so it fails
    // before it writes anything, and the first's dir is as it was.
    let dir = temp("exclusive");
    let out = dir.join("out");
    let _c = fix_clock(&out, 1_792_319_367_408_836_123);
    let ((), w) = run_in(FIRST, &out, writes).unwrap();
    let had = tree(&out);
    let e = run_in(FIRST, &out, |run: &Run| -> Result<(), String> {
        run.write("b.txt", b"b")?;
        Ok(())
    })
    .unwrap_err();
    assert!(e.contains("a run dir is never reused"), "{e}");
    assert!(e.contains(&w.dir.display().to_string()), "{e}");
    assert_eq!(tree(&out), had);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_run_keeps_its_diagnostics_and_deletes_nothing_it_did_not_write() {
    // The run writes a sheet, a text file and its snapshot, and fails; a file of another hand
    // put in its dir meanwhile. Its own sheet and text file are deleted; its snapshot, the
    // refusal and the other file are left. No done.
    let dir = temp("fail");
    let out = dir.join("out");
    let o = out.clone();
    let _h = hook(&out, "write", move |name| {
        if name == "gates.jsonl" {
            let run = o.canonicalize().unwrap();
            let run = run.join(&names(&run)[0]);
            std::fs::write(run.join("theirs.png"), b"not the run's").unwrap();
        }
        Ok(())
    });
    let e = run_in(FIRST, &out, fails).unwrap_err();
    let run = only_run(&out);
    assert_eq!(e, format!("no rows; run dir: {}", run.display()));
    assert_eq!(names(&run), ["gates.jsonl", REFUSED, "theirs.png"]);
    assert_eq!(
        std::fs::read(run.join("theirs.png")).unwrap(),
        b"not the run's"
    );
    assert_eq!(
        std::fs::read_to_string(run.join(REFUSED)).unwrap(),
        format!("{e}\n")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_a_failed_run_cannot_delete_is_said_to_be_left() {
    let dir = temp("left");
    let out = dir.join("out");
    let _h = hook(&out, "delete", |name| {
        if name == "a.png" {
            Err("held".into())
        } else {
            Ok(())
        }
    });
    let e = run_in(FIRST, &out, fails).unwrap_err();
    let run = out.canonicalize().unwrap().join(&names(&out)[0]);
    let left = format!("{} (held)", run.join("a.png").display());
    assert_eq!(
        e,
        format!(
            "no rows; left, as it could not be deleted: {left}; run dir: {}",
            run.display()
        )
    );
    assert_eq!(names(&run), ["a.png", "gates.jsonl", REFUSED]);
    assert_eq!(
        std::fs::read_to_string(run.join(REFUSED)).unwrap(),
        format!("{e}\n")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_run_lists_the_incomplete_runs_beside_it_and_touches_none_of_what_was_there() {
    // In --out before the run: a run that ended (done), a refused run, one killed partway (no
    // done, no refusal), a dir and a file of the owner's. The run lists the two incomplete runs
    // (by name, the refused one said so), in its report and in its error when it fails, and
    // every entry that was there is as it was, byte for byte.
    let dir = temp("before");
    let out = dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let ((), done) = run_in(FIRST, &out, writes).unwrap();
    let refused = run_in(FIRST, &out, fails).unwrap_err();
    let done_name = done.dir.file_name().unwrap().to_string_lossy().into_owned();
    let refused_run = names(&out).into_iter().find(|n| *n != done_name).unwrap();
    let real = out.canonicalize().unwrap();
    assert_eq!(
        refused,
        format!("no rows; run dir: {}", real.join(&refused_run).display())
    );
    let killed = out.join(run_name(1, 7));
    std::fs::create_dir(&killed).unwrap();
    std::fs::write(killed.join("a.png"), b"half").unwrap();
    std::fs::create_dir(out.join("owner")).unwrap();
    std::fs::write(out.join("owner").join("x.txt"), b"x").unwrap();
    std::fs::write(out.join("notes.txt"), b"the owner's").unwrap();
    let had = tree(&out);
    let want = vec![run_name(1, 7), format!("{refused_run} (refused)")];
    let ((), w) = run_in(FIRST, &out, writes).unwrap();
    assert_eq!(w.incomplete, want);
    assert!(
        w.text().contains(&format!(
            "incomplete runs beside it (no {DONE}; left as they are): {}",
            want.join(", ")
        )),
        "{}",
        w.text()
    );
    let mut now = tree(&out);
    let mine = w.dir.file_name().unwrap().to_string_lossy().into_owned();
    now.retain(|k, _| !k.starts_with(&mine));
    assert_eq!(now, had);
    // A failed run says them in its error too, after its own dir, and leaves them.
    let e = run_in(FIRST, &out, fails).unwrap_err();
    let runs: Vec<String> = names(&out)
        .into_iter()
        .filter(|n| n.starts_with("run-") && !had.contains_key(n) && *n != mine)
        .collect();
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(
        e,
        format!(
            "no rows; run dir: {}; incomplete runs beside it (left as they are): {}",
            real.join(&runs[0]).display(),
            want.join(", ")
        )
    );
    let mut now = tree(&out);
    now.retain(|k, _| had.contains_key(k));
    assert_eq!(now, had);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_symlinked_out_dir_is_written_through() {
    let dir = temp("link");
    let (real, link) = (dir.join("real"), dir.join("link"));
    std::fs::create_dir(&real).unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let ((), w) = run_in(FIRST, &link, writes).unwrap();
    assert_eq!(w.dir.parent().unwrap(), real.canonicalize().unwrap());
    assert_eq!(std::fs::read_link(&link).unwrap(), real);
    assert_eq!(names(&w.dir), ["a.png", "a.txt", DONE]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The run dir of the one run under `out`.
fn only_run(out: &Path) -> PathBuf {
    let parent = out.canonicalize().unwrap();
    let runs: Vec<String> = names(&parent)
        .into_iter()
        .filter(|n| n.starts_with("run-"))
        .collect();
    assert_eq!(runs.len(), 1, "{runs:?}");
    parent.join(&runs[0])
}

#[test]
fn a_run_whose_dir_is_removed_fails_and_makes_nothing_again() {
    // The reviewer's P1 and P1b, turned around: the run dir, or --out whole, removed before
    // the run's second write. The write fails, the run fails, and nothing is made again (no
    // run dir, no --out, no done).
    for whole in [false, true] {
        let dir = temp(&format!("removed-{whole}"));
        let out = dir.join("out");
        let o = out.clone();
        let _h = hook(&out, "write", move |name| {
            if name == "b.txt" {
                let gone = if whole { o.clone() } else { only_run(&o) };
                std::fs::remove_dir_all(gone).unwrap();
            }
            Ok(())
        });
        let e = run_in(FIRST, &out, |run: &Run| -> Result<(), String> {
            run.write("a.txt", b"a")?;
            run.write("b.txt", b"b")?;
            run.write("c.txt", b"c")
        })
        .unwrap_err();
        assert!(
            e.contains("b.txt: No such file or directory"),
            "{whole}: {e}"
        );
        assert!(
            e.contains("removed or replaced while the run wrote it"),
            "{whole}: {e}"
        );
        if whole {
            assert!(!out.exists(), "{whole}");
        } else {
            assert!(names(&out).is_empty(), "{whole}: {:?}", names(&out));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn a_run_whose_dir_is_swapped_for_a_link_touches_nothing_where_it_points() {
    // The reviewer's P2, turned around: the live run dir replaced by a link to a dir of the
    // owner's, which holds an a.txt of theirs (a name the run wrote before the swap); the run
    // then fails. Nothing is written there, and their a.txt is as it was.
    let dir = temp("swapped");
    let out = dir.join("out");
    let elsewhere = dir.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::write(elsewhere.join("a.txt"), b"the owner's").unwrap();
    let had = tree(&elsewhere);
    let (o, to) = (out.clone(), elsewhere.clone());
    let _h = hook(&out, "write", move |name| {
        if name == "gates.jsonl" {
            let run = only_run(&o);
            std::fs::remove_dir_all(&run).unwrap();
            std::os::unix::fs::symlink(&to, &run).unwrap();
        }
        Ok(())
    });
    let e = run_in(FIRST, &out, fails).unwrap_err();
    assert!(
        e.contains("removed or replaced while the run wrote it"),
        "{e}"
    );
    assert_eq!(tree(&elsewhere), had);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_at_a_name_the_run_writes_is_never_replaced_or_deleted() {
    // A file of the owner's put at a name before the run writes it (the reviewer's P7, turned
    // around): the write fails (the name is taken), the run fails, and the file, which the run
    // never made, is not deleted with the run's own.
    let dir = temp("taken");
    let out = dir.join("out");
    let o = out.clone();
    let _h = hook(&out, "write", move |name| {
        if name == "a.png" {
            std::fs::write(only_run(&o).join("a.png"), b"the owner's").unwrap();
        }
        Ok(())
    });
    let e = run_in(FIRST, &out, writes).unwrap_err();
    assert!(e.contains("a.png: File exists"), "{e}");
    let run = only_run(&out);
    assert_eq!(names(&run), ["a.png", REFUSED]);
    assert_eq!(std::fs::read(run.join("a.png")).unwrap(), b"the owner's");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_put_at_a_name_the_run_made_is_not_deleted_and_is_said_to_be_left() {
    // After the run wrote a.txt, another hand replaces it with a file of its own at that name
    // (the reviewer's P6, turned around); the run fails. That file is not the one the run
    // made (another inode), so it is left, and said so.
    let dir = temp("replaced-file");
    let out = dir.join("out");
    let o = out.clone();
    let _h = hook(&out, "write", move |name| {
        if name == "gates.jsonl" {
            let run = only_run(&o);
            std::fs::write(run.join("x.tmp"), b"the owner's").unwrap();
            std::fs::rename(run.join("x.tmp"), run.join("a.txt")).unwrap();
        }
        Ok(())
    });
    let e = run_in(FIRST, &out, fails).unwrap_err();
    let run = only_run(&out);
    assert_eq!(std::fs::read(run.join("a.txt")).unwrap(), b"the owner's");
    let left = format!(
        "{} (not the file the run made)",
        run.join("a.txt").display()
    );
    assert!(e.contains(&left), "{e}");
    assert_eq!(names(&run), ["a.txt", "gates.jsonl", REFUSED]);
    let _ = std::fs::remove_dir_all(&dir);
}
