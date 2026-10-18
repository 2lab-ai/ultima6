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
    // The order of the writes, through the seam; and no done while the run's callback runs.
    let order: std::sync::Arc<std::sync::Mutex<Vec<String>>> = Default::default();
    let seen = order.clone();
    let _h = hook(&out, "write", move |name| {
        seen.lock().unwrap().push(name.to_string());
        Ok(())
    });
    let ((), w) = run_in(FIRST, &out, |run: &Run| -> Result<(), String> {
        writes(run)?;
        assert!(!run.dir().join(DONE).exists(), "done while the run writes");
        Ok(())
    })
    .unwrap();
    assert_eq!(*order.lock().unwrap(), ["a.txt", "a.png", DONE]);
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
    // Beside an incomplete run, the collision says it too.
    std::fs::create_dir(out.join(run_name(1, 7))).unwrap();
    let had = tree(&out);
    let e = run_in(FIRST, &out, writes).unwrap_err();
    assert_eq!(
        e,
        format!(
            "{}: File exists (os error 17) (a run dir is never reused); incomplete runs \
             beside it (left as they are): {}",
            w.dir.display(),
            run_name(1, 7)
        )
    );
    assert_eq!(tree(&out), had);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_run_dir_that_cannot_be_made_for_another_reason_is_not_said_to_be_reused() {
    // --out where no dir can be made (read-only): the error is the system's, beside the
    // incomplete run, not "never reused". Needs an unprivileged user: skipped with privilege.
    use std::os::unix::fs::PermissionsExt;
    // SAFETY: geteuid(2) reads the process's ids; no memory involved.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipped: it needs an unprivileged user (euid 0 writes a read-only dir)");
        return;
    }
    let dir = temp("cannot-make");
    let out = dir.join("out");
    std::fs::create_dir_all(out.join(run_name(1, 7))).unwrap();
    std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o555)).unwrap();
    let e = run_in(FIRST, &out, writes).unwrap_err();
    std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(e.contains("Permission denied"), "{e}");
    assert!(!e.contains("never reused"), "{e}");
    assert!(
        e.ends_with(&format!(
            "; incomplete runs beside it (left as they are): {}",
            run_name(1, 7)
        )),
        "{e}"
    );
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
            "incomplete runs beside it (no {DONE}, or refused; left as they are): {}",
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
            e.contains("was moved or removed while the run wrote it"),
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
        e.contains("was moved or removed while the run wrote it"),
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
    let run = only_run(&out);
    // Nothing said to be left: the run never listed a file it did not make.
    assert_eq!(
        e,
        format!(
            "{}: File exists (os error 17); run dir: {}",
            run.join("a.png").display(),
            run.display()
        )
    );
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

#[test]
fn a_name_that_is_not_one_plain_name_is_refused_before_any_file_is_touched() {
    // A sentinel beside --out, and names that would reach it or any other place: each write
    // and read is refused before the seam or the filesystem is reached; the sentinel is as it
    // was, and the run dir holds what the run made of its own names alone.
    let dir = temp("names");
    let out = dir.join("out");
    let sentinel = dir.join("sentinel.txt");
    std::fs::write(&sentinel, b"outside").unwrap();
    let abs = sentinel.display().to_string();
    let bad = [
        "../../sentinel.txt".to_string(),
        "../sentinel.txt".to_string(),
        abs.clone(),
        "a/b.txt".to_string(),
        "a.txt/".to_string(),
        ".".to_string(),
        "..".to_string(),
        String::new(),
    ];
    let touched = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let t = touched.clone();
    let _h = hook(&out, "write", move |name| {
        if name != "a.txt" && name != DONE {
            t.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    });
    let ((), w) = run_in(FIRST, &out, |run: &Run| -> Result<(), String> {
        for name in &bad {
            let e = run.write(name, b"overwritten").unwrap_err();
            assert!(
                e.contains("not a name of a file in the run dir"),
                "{name:?}: {e}"
            );
            let e = run.read(name, 64).unwrap_err();
            assert!(
                e.contains("not a name of a file in the run dir"),
                "{name:?}: {e}"
            );
        }
        run.write("a.txt", b"a")
    })
    .unwrap();
    assert_eq!(touched.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"outside");
    assert_eq!(names(&dir), ["out", "sentinel.txt"]);
    assert_eq!(names(&w.dir), ["a.txt", DONE]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_run_dir_that_cannot_be_read_is_listed_as_such_not_as_incomplete() {
    // The reviewer's P5, turned around: a run that ended, its dir then unreadable (mode 000).
    // The next run lists it as a run dir it cannot read, with why, not as incomplete, and
    // leaves it. Needs an unprivileged user: skipped with privilege.
    use std::os::unix::fs::PermissionsExt;
    // SAFETY: geteuid(2) reads the process's ids; no memory involved.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipped: it needs an unprivileged user (euid 0 reads a mode 000 dir)");
        return;
    }
    let dir = temp("unreadable");
    let out = dir.join("out");
    let ((), done) = run_in(FIRST, &out, writes).unwrap();
    std::fs::set_permissions(&done.dir, std::fs::Permissions::from_mode(0o000)).unwrap();
    let r = run_in(FIRST, &out, writes);
    std::fs::set_permissions(&done.dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let ((), w) = r.unwrap();
    let name = done.dir.file_name().unwrap().to_string_lossy().into_owned();
    assert!(w.incomplete.is_empty(), "{:?}", w.incomplete);
    assert_eq!(w.unreadable.len(), 1, "{:?}", w.unreadable);
    assert!(
        w.unreadable[0].starts_with(&format!("{name} ("))
            && w.unreadable[0].contains("Permission denied"),
        "{:?}",
        w.unreadable
    );
    assert!(
        w.text()
            .contains(&format!("{UNREADABLE}: {}", w.unreadable[0])),
        "{}",
        w.text()
    );
    assert_eq!(names(&done.dir), ["a.png", "a.txt", DONE]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_run_whose_done_cannot_be_written_fails_and_keeps_its_diagnostics_alone() {
    // The reviewer's K1: done not written (no room): the run fails, its outputs are deleted,
    // there is no done, and refused.txt says why.
    let dir = temp("no-done");
    let out = dir.join("out");
    let _h = hook(&out, "write", |name| {
        if name == DONE {
            Err("done: no room left".into())
        } else {
            Ok(())
        }
    });
    let e = run_in(FIRST, &out, |run: &Run| -> Result<(), String> {
        writes(run)?;
        run.write("gates.jsonl", b"snapshot")
    })
    .unwrap_err();
    let run = only_run(&out);
    assert_eq!(e, format!("done: no room left; run dir: {}", run.display()));
    assert_eq!(names(&run), ["gates.jsonl", REFUSED]);
    assert_eq!(
        std::fs::read_to_string(run.join(REFUSED)).unwrap(),
        format!("{e}\n")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_incomplete_runs_are_listed_oldest_first_whatever_order_they_were_made_in() {
    // Five incomplete runs made out of their order (their names, by time: 3, 1, 5, 2, 4): the
    // list is by time, the oldest first.
    let dir = temp("sorted");
    let out = dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    for k in [3u128, 1, 5, 2, 4] {
        std::fs::create_dir(out.join(run_name(k * 1_000_000_000, 7))).unwrap();
    }
    let ((), w) = run_in(FIRST, &out, writes).unwrap();
    let want: Vec<String> = (1..=5u128)
        .map(|k| run_name(k * 1_000_000_000, 7))
        .collect();
    assert_eq!(w.incomplete, want);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_run_whose_dir_is_moved_before_done_fails_and_writes_no_done() {
    // The run dir renamed away (by another hand) at the run's last file, then at done itself:
    // the path the report would name holds nothing of it, so the run fails, says once where
    // the dir is now, and leaves no done in the dir it made; its refusal is written there.
    for at in ["a.png", DONE] {
        let dir = temp(&format!("moved-{at}"));
        let out = dir.join("out");
        let away = dir.join("away");
        let (o, to) = (out.clone(), away.clone());
        let _h = hook(&out, "write", move |name| {
            if name == at {
                std::fs::rename(only_run(&o), &to).unwrap();
            }
            Ok(())
        });
        let e = run_in(FIRST, &out, writes).unwrap_err();
        let now = format!("(it is now at {})", away.canonicalize().unwrap().display());
        assert_eq!(
            e.matches("was moved or removed while the run wrote it")
                .count(),
            1,
            "{at}: {e}"
        );
        assert!(e.contains(&now), "{at}: {e}");
        assert!(names(&out).is_empty(), "{at}: {:?}", names(&out));
        assert_eq!(names(&away), [REFUSED], "{at}");
        let refused = std::fs::read_to_string(away.join(REFUSED)).unwrap();
        assert_eq!(refused, format!("{e}\n"), "{at}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// How many of this process's open fds are the file whose (device, inode) is `id`.
fn held(id: (u64, u64)) -> usize {
    use std::os::fd::BorrowedFd;
    (0..16_384)
        .filter(|&fd| {
            // SAFETY: fcntl(F_GETFD) only asks whether `fd` is open; it touches nothing.
            if unsafe { libc::fcntl(fd, libc::F_GETFD) } < 0 {
                return false;
            }
            // SAFETY: `fd` is open (just asked), and is borrowed for this one call only.
            let b = unsafe { BorrowedFd::borrow_raw(fd) };
            b.try_clone_to_owned()
                .ok()
                .map(File::from)
                .is_some_and(|f| at::id(&f).ok() == Some(id))
        })
        .count()
}

#[test]
fn each_file_the_run_makes_is_held_open_until_it_ends_and_a_new_one_at_its_name_is_left() {
    // The run's files are held open while it runs (one fd each), so their inodes stay
    // allocated: a file another hand makes at a name of the run's, after deleting the run's,
    // is another inode (on any filesystem), and a failed run leaves it, said so. Once the run
    // ends, nothing of it is held.
    let dir = temp("held");
    let out = dir.join("out");
    let o = out.clone();
    let _h = hook(&out, "write", move |name| {
        if name == "gates.jsonl" {
            let run = only_run(&o);
            std::fs::remove_file(run.join("a.txt")).unwrap();
            std::fs::write(run.join("a.txt"), b"the owner's").unwrap();
        }
        Ok(())
    });
    let ids: std::sync::Arc<std::sync::Mutex<Vec<(u64, u64)>>> = Default::default();
    let seen = ids.clone();
    let e = run_in(FIRST, &out, |run: &Run| -> Result<(), String> {
        writes(run)?;
        for m in run.made.borrow().iter() {
            let id = m.id.unwrap();
            assert_eq!(held(id), 1, "{}", m.name);
            seen.lock().unwrap().push(id);
        }
        run.write("gates.jsonl", b"snapshot")?;
        Err("no rows".into())
    })
    .unwrap_err();
    let run = only_run(&out);
    let theirs = std::fs::File::open(run.join("a.txt")).unwrap();
    let theirs_id = at::id(&theirs).unwrap();
    assert!(!ids.lock().unwrap().contains(&theirs_id));
    assert_eq!(std::fs::read(run.join("a.txt")).unwrap(), b"the owner's");
    let left = format!(
        "{} (not the file the run made)",
        run.join("a.txt").display()
    );
    assert!(e.contains(&left), "{e}");
    for &id in ids.lock().unwrap().iter() {
        assert_eq!(held(id), 0, "{id:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_run_out_of_open_files_fails_saying_so_and_still_cleans_up() {
    // The third file cannot be made: the system's "too many open files" (injected at the
    // create). The run fails saying so and how to go on; its two files are deleted (which needs
    // no new fd) and its refusal written.
    let dir = temp("emfile");
    let out = dir.join("out");
    let _h = hook(&out, "create", |name| {
        if name == "c.txt" {
            Err(format!("errno:{}", libc::EMFILE))
        } else {
            Ok(())
        }
    });
    let e = run_in(FIRST, &out, |run: &Run| -> Result<(), String> {
        writes(run)?;
        run.write("c.txt", b"c")
    })
    .unwrap_err();
    let run = only_run(&out);
    let said = format!(
        "{}: Too many open files (os error {}) (a run holds each file it makes open until it \
         ends: raise the limit on open files, ulimit -n); run dir: {}",
        run.join("c.txt").display(),
        libc::EMFILE,
        run.display()
    );
    assert_eq!(e, said);
    assert_eq!(names(&run), [REFUSED]);
    assert_eq!(
        std::fs::read_to_string(run.join(REFUSED)).unwrap(),
        format!("{e}\n")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_link_put_at_a_name_the_run_made_is_said_to_be_another_file() {
    // After the run wrote a.txt, a link put at that name: the check reads the entry itself (no
    // open, the link not followed), so it is another file, left, said so.
    let dir = temp("linked");
    let out = dir.join("out");
    let elsewhere = dir.join("elsewhere.txt");
    std::fs::write(&elsewhere, b"the owner's").unwrap();
    let (o, to) = (out.clone(), elsewhere.clone());
    let _h = hook(&out, "write", move |name| {
        if name == "gates.jsonl" {
            let run = only_run(&o);
            std::fs::remove_file(run.join("a.txt")).unwrap();
            std::os::unix::fs::symlink(&to, run.join("a.txt")).unwrap();
        }
        Ok(())
    });
    let e = run_in(FIRST, &out, fails).unwrap_err();
    let run = only_run(&out);
    let left = format!(
        "{} (not the file the run made)",
        run.join("a.txt").display()
    );
    assert!(e.contains(&left), "{e}");
    assert_eq!(std::fs::read_link(run.join("a.txt")).unwrap(), elsewhere);
    assert_eq!(std::fs::read(&elsewhere).unwrap(), b"the owner's");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_of_the_run_others_may_not_read_is_still_deleted_as_its_own() {
    // After the run wrote a.txt, its mode dropped to 000: the check needs no open, so the file
    // is still known for the run's and deleted. Needs an unprivileged user (the point is a file
    // the user cannot open): skipped with privilege, where the link case above still holds.
    use std::os::unix::fs::PermissionsExt;
    // SAFETY: geteuid(2) reads the process's ids; no memory involved.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipped: it needs an unprivileged user (euid 0 opens a mode 000 file)");
        return;
    }
    let dir = temp("mode-000");
    let out = dir.join("out");
    let o = out.clone();
    let _h = hook(&out, "write", move |name| {
        if name == "gates.jsonl" {
            let p = only_run(&o).join("a.txt");
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o000)).unwrap();
        }
        Ok(())
    });
    let e = run_in(FIRST, &out, fails).unwrap_err();
    let run = only_run(&out);
    assert_eq!(e, format!("no rows; run dir: {}", run.display()));
    assert_eq!(names(&run), ["gates.jsonl", REFUSED]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_whose_identity_cannot_be_read_is_kept_and_said() {
    // a.txt made, its identity not readable (injected at the fstat): it is listed all the same,
    // and the failed run leaves it, said so, rather than deleting a file it cannot name.
    let dir = temp("unidentified");
    let out = dir.join("out");
    let _h = hook(&out, "identify", |name| {
        if name == "a.txt" {
            Err(format!("errno:{}", libc::EIO))
        } else {
            Ok(())
        }
    });
    let e = run_in(FIRST, &out, fails).unwrap_err();
    let run = only_run(&out);
    let left = format!(
        "{} (could not be identified when it was made)",
        run.join("a.txt").display()
    );
    assert!(e.contains(&left), "{e}");
    assert_eq!(names(&run), ["a.txt", "gates.jsonl", REFUSED]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_run_dir_with_refused_and_done_is_listed_as_refused() {
    // A run dir that holds both (a failed run whose done could not be deleted): refused.
    let dir = temp("refused-done");
    let out = dir.join("out");
    let both = out.join(run_name(1, 7));
    std::fs::create_dir_all(&both).unwrap();
    std::fs::write(both.join(DONE), b"").unwrap();
    std::fs::write(both.join(REFUSED), b"no rows\n").unwrap();
    let ((), w) = run_in(FIRST, &out, writes).unwrap();
    assert_eq!(w.incomplete, [format!("{} (refused)", run_name(1, 7))]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_done_that_fails_once_made_is_deleted_with_the_rest() {
    // done made, then its write fails (injected after the create): no done is left, the
    // diagnostics are, and refused.txt says the returned error.
    let dir = temp("done-made");
    let out = dir.join("out");
    let _h = hook(&out, "fill", |name| {
        if name == DONE {
            Err(format!("errno:{}", libc::ENOSPC))
        } else {
            Ok(())
        }
    });
    let e = run_in(FIRST, &out, |run: &Run| -> Result<(), String> {
        writes(run)?;
        run.write("gates.jsonl", b"snapshot")
    })
    .unwrap_err();
    let run = only_run(&out);
    assert!(
        e.starts_with(&format!(
            "{}: No space left on device",
            run.join(DONE).display()
        )),
        "{e}"
    );
    assert_eq!(names(&run), ["gates.jsonl", REFUSED]);
    assert_eq!(
        std::fs::read_to_string(run.join(REFUSED)).unwrap(),
        format!("{e}\n")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_run_dir_is_made_in_the_out_dir_it_opened_never_by_its_path() {
    // --out opened, then (before the run dir is made) moved away and a link to another dir put
    // in its place: the run dir is made in the --out it opened, now away; nothing in the
    // link's target; and the run fails at its end, its dir not at the path it named.
    let dir = temp("out-swapped");
    let out = dir.join("out");
    let (away, elsewhere) = (dir.join("away"), dir.join("elsewhere"));
    std::fs::create_dir_all(&out).unwrap();
    std::fs::create_dir_all(&elsewhere).unwrap();
    let (o, a, el) = (out.clone(), away.clone(), elsewhere.clone());
    let _h = hook(&out, "mkdir", move |_| {
        std::fs::rename(&o, &a).unwrap();
        std::os::unix::fs::symlink(&el, &o).unwrap();
        Ok(())
    });
    let e = run_in(FIRST, &out, writes).unwrap_err();
    assert!(
        e.contains("was moved or removed while the run wrote it"),
        "{e}"
    );
    assert!(names(&elsewhere).is_empty(), "{:?}", names(&elsewhere));
    let runs = names(&away);
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(names(&away.join(&runs[0])), [REFUSED]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_run_dir_that_cannot_be_opened_is_left_empty_and_said() {
    // The run dir made, then not openable (injected at the open): the run fails saying the run
    // dir and that it is left empty with no refused.txt; the next run lists it as incomplete.
    let dir = temp("unopened");
    let out = dir.join("out");
    let e = {
        let _h = hook(&out, "open", |_| Err(format!("errno:{}", libc::ELOOP)));
        run_in(FIRST, &out, writes).unwrap_err()
    };
    let run = only_run(&out);
    assert!(
        e.contains("the run dir is left empty, with no refused.txt"),
        "{e}"
    );
    assert!(e.ends_with(&format!("; run dir: {}", run.display())), "{e}");
    assert!(names(&run).is_empty());
    let name = run.file_name().unwrap().to_string_lossy().into_owned();
    let ((), w) = run_in(FIRST, &out, writes).unwrap();
    assert_eq!(w.incomplete, [name]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_permission_error_is_neither_reuse_nor_incomplete_whatever_the_user() {
    // The two permission cases, injected (they hold under any uid): a run dir that cannot be
    // made (EACCES) is not said to be reused, and a run dir whose done cannot be read is listed
    // as one that cannot be read, not as incomplete.
    let dir = temp("eacces");
    let out = dir.join("out");
    let ((), done) = run_in(FIRST, &out, writes).unwrap();
    let name = done.dir.file_name().unwrap().to_string_lossy().into_owned();
    let n = name.clone();
    let e = {
        let _h = hook(&out, "mkdir", |_| Err(format!("errno:{}", libc::EACCES)));
        run_in(FIRST, &out, writes).unwrap_err()
    };
    assert!(e.contains("Permission denied"), "{e}");
    assert!(!e.contains("never reused"), "{e}");
    let _h = hook(&out, "stat", move |what| {
        if what == n {
            Err(format!("errno:{}", libc::EACCES))
        } else {
            Ok(())
        }
    });
    let ((), w) = run_in(FIRST, &out, writes).unwrap();
    assert!(w.incomplete.is_empty(), "{:?}", w.incomplete);
    assert_eq!(
        w.unreadable,
        [format!(
            "{name} (Permission denied (os error {}))",
            libc::EACCES
        )]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn oldest_first_sorts_any_order_by_time() {
    // Names given out of their order, not made on disk: by time, the oldest first.
    let names: Vec<String> = [4u128, 1, 5, 3, 2]
        .into_iter()
        .map(|k| run_name(k * 1_000_000_000, 9 - k as u32))
        .collect();
    let want: Vec<String> = (1..=5u128)
        .map(|k| run_name(k * 1_000_000_000, 9 - k as u32))
        .collect();
    assert_eq!(oldest_first(names), want);
}
