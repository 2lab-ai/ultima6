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

/// A list the test and its hooks share.
type Shared<T> = std::sync::Arc<std::sync::Mutex<Vec<T>>>;

/// The env var that marks a child of this test binary started for one test ([`in_child`]): its
/// value is the test's path.
const CHILD: &str = "U6_HDTOOL_OUT_DIR_CHILD";

/// The path of the test `name` of the module `module` (its `module_path!()`), as the test binary
/// names it.
pub(crate) fn test_path(module: &str, name: &str) -> String {
    format!("{}::{name}", module.split_once("::").unwrap().1)
}

/// The path of this module's test `name`.
fn here(name: &str) -> String {
    test_path(module_path!(), name)
}

/// Whether this process is the child started for the test `test` (its path, [`in_child`]).
pub(crate) fn is_child(test: &str) -> bool {
    std::env::var_os(CHILD).is_some_and(|v| v == test)
}

/// Runs the test `test` (its path, [`test_path`]) alone in a child of this test binary
/// (`--exact`, marked by [`CHILD`]), started as `sh -c '<shell>; exec "$0" "$@"'` and set up by
/// `setup`; panics, with the child's output, unless that one test ran and passed.
pub(crate) fn in_child(test: &str, shell: &str, setup: impl FnOnce(&mut std::process::Command)) {
    let mut c = std::process::Command::new("sh");
    c.arg("-c")
        .arg(format!("{shell}; exec \"$0\" \"$@\""))
        .arg(std::env::current_exe().unwrap())
        .args(["--exact", test, "--test-threads=1", "--nocapture"])
        .env(CHILD, test);
    setup(&mut c);
    let out = c.output().unwrap();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.status.success() && said.contains("test result: ok. 1 passed"),
        "{test}, in a child ({}):\n{said}",
        out.status
    );
}

/// For a test that needs an unprivileged user (`test`, its path): with privilege (euid 0), runs
/// it in a child dropped to uid and gid 65534, with a temp dir of its own that it can enter, and
/// gives true (the caller is done); else false, and the caller runs it here (in that child,
/// checked to have no privilege).
pub(crate) fn ran_unprivileged(test: &str) -> bool {
    use std::os::unix::process::CommandExt as _;
    // SAFETY: geteuid(2) reads the process's ids; no memory involved.
    let euid = unsafe { libc::geteuid() };
    if is_child(test) {
        assert_ne!(euid, 0, "the child kept its privilege");
        return false;
    }
    if euid != 0 {
        return false;
    }
    let tmp = temp(&format!("nobody-{}", test.rsplit("::").next().unwrap()));
    std::os::unix::fs::chown(&tmp, Some(65534), Some(65534)).unwrap();
    in_child(test, ":", |c| {
        c.env("TMPDIR", &tmp);
        // SAFETY: between the fork and the exec, only setgroups(2), setgid(2) and setuid(2),
        // each async-signal-safe, and errno read.
        unsafe {
            c.pre_exec(|| {
                let dropped = libc::setgroups(0, std::ptr::null()) == 0
                    && libc::setgid(65534) == 0
                    && libc::setuid(65534) == 0;
                match dropped {
                    true => Ok(()),
                    false => Err(std::io::Error::last_os_error()),
                }
            });
        }
    });
    let _ = std::fs::remove_dir_all(&tmp);
    true
}

/// Every fd this process may still open, taken (each a file `probe` opened), but `spare` let go
/// again: the next `spare` opens of the process succeed, and the one after meets EMFILE.
pub(crate) fn take_fds(probe: &Path, spare: usize) -> Vec<File> {
    let mut taken = Vec::new();
    let full = loop {
        match File::open(probe) {
            Ok(f) => taken.push(f),
            Err(e) => break e,
        }
    };
    assert_eq!(full.raw_os_error(), Some(libc::EMFILE), "{full}");
    taken.truncate(taken.len() - spare);
    taken
}

/// The soft and hard limits on open files of this process now.
fn nofile() -> (u64, u64) {
    // SAFETY: an all-zero `rlimit` is a valid value for the call to fill.
    let mut r: libc::rlimit = unsafe { std::mem::zeroed() };
    // SAFETY: a `rlimit` that outlives the call.
    assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut r) }, 0);
    #[allow(clippy::unnecessary_cast)]
    (r.rlim_cur as u64, r.rlim_max as u64)
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
    // incomplete run, not "never reused". Needs an unprivileged user: with privilege, run in a
    // child that has none.
    use std::os::unix::fs::PermissionsExt;
    if ran_unprivileged(&here(
        "a_run_dir_that_cannot_be_made_for_another_reason_is_not_said_to_be_reused",
    )) {
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
    // leaves it. Needs an unprivileged user: with privilege, run in a child that has none.
    use std::os::unix::fs::PermissionsExt;
    if ran_unprivileged(&here(
        "a_run_dir_that_cannot_be_read_is_listed_as_such_not_as_incomplete",
    )) {
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
        let order: std::sync::Arc<std::sync::Mutex<Vec<String>>> = Default::default();
        let seen = order.clone();
        let _h = hook(&out, "write", move |name| {
            seen.lock().unwrap().push(name.to_string());
            if name == at {
                std::fs::rename(only_run(&o), &to).unwrap();
            }
            Ok(())
        });
        let e = run_in(FIRST, &out, writes).unwrap_err();
        // A dir moved before done: done is never made (the check before it); moved at done:
        // made, then deleted (the check after it).
        let tried = order.lock().unwrap().iter().any(|n| n == DONE);
        assert_eq!(tried, at == DONE, "{at}: {:?}", order.lock().unwrap());
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

/// The fd numbers looked at for this process's open files: fds are given lowest first, and the
/// tests of this process never hold near this many at once.
const FDS: i32 = 16_384;

/// How many of this process's open fds are the file whose (device, inode) is `id`: a `fstat` of
/// each fd number (one not open gives EBADF), with no fd borrowed or made.
fn held(id: (u64, u64)) -> usize {
    (0..FDS)
        .filter(|&fd| {
            // SAFETY: an all-zero `stat` is a valid value for the call to fill.
            let mut st: libc::stat = unsafe { std::mem::zeroed() };
            // SAFETY: a fd number (one not open fails the call) and a `stat` that outlives it.
            let r = unsafe { libc::fstat(fd, &mut st) };
            r == 0 && at::ids(&st) == id
        })
        .count()
}

/// This process's open fds that resolve into `dir` (a file deleted there too: Linux says
/// `<path> (deleted)`, macOS its old path), by their number ([`at::path_of_fd`]), with no fd
/// borrowed or made.
fn open_into(dir: &Path) -> Vec<(i32, PathBuf)> {
    (0..FDS)
        .filter_map(|fd| Some((fd, at::path_of_fd(fd)?)))
        .filter(|(_, p)| p.starts_with(dir))
        .collect()
}

#[test]
fn each_file_the_run_makes_is_held_open_until_it_ends_and_a_new_one_at_its_name_is_left() {
    // The run's files are held open while it runs (one fd each), so their inodes stay
    // allocated: a file another hand makes at a name of the run's, after deleting the run's,
    // is another inode (on any filesystem), and a failed run leaves it, said so. Once the run
    // ends, no fd of the process is in its dir (by path: a number freed may be another file's).
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
            let id = m.id.clone().unwrap();
            assert_eq!(held(id), 1, "{}", m.name);
            seen.lock().unwrap().push(id);
        }
        run.write("gates.jsonl", b"snapshot")?;
        Err("no rows".into())
    })
    .unwrap_err();
    let run = only_run(&out);
    let theirs = {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::symlink_metadata(run.join("a.txt")).unwrap();
        (m.dev(), m.ino())
    };
    assert!(!ids.lock().unwrap().contains(&theirs));
    assert_eq!(std::fs::read(run.join("a.txt")).unwrap(), b"the owner's");
    let left = format!(
        "{} (not the file the run made)",
        run.join("a.txt").display()
    );
    assert!(e.contains(&left), "{e}");
    // The search finds an fd into the run dir when there is one (a file of it held here, then
    // deleted: found by its number all the same), so that it finding none says something.
    {
        use std::os::fd::AsRawFd as _;
        let probe = File::open(run.join(REFUSED)).unwrap();
        std::fs::remove_file(run.join(REFUSED)).unwrap();
        let found = open_into(&run);
        assert!(
            found.iter().any(|(fd, _)| *fd == probe.as_raw_fd()),
            "{found:?}"
        );
    }
    assert!(open_into(&run).is_empty(), "{:?}", open_into(&run));
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
    // the user cannot open): with privilege, run in a child that has none.
    use std::os::unix::fs::PermissionsExt;
    if ran_unprivileged(&here(
        "a_file_of_the_run_others_may_not_read_is_still_deleted_as_its_own",
    )) {
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
fn a_file_whose_identity_cannot_be_read_fails_the_run_which_leaves_it_and_says_so() {
    // a.txt made, its identity not readable (injected at the fstat); every later step goes well
    // (the run's other file, its own end). The run fails all the same, and before done: it
    // could not tell a.txt from a file another hand put at that name, so it leaves it, said so,
    // and deletes its other file. The same at done itself: done is left too, with refused.txt,
    // and the next run lists the dir as refused.
    for at in ["a.txt", DONE] {
        let dir = temp(&format!("unidentified-{at}"));
        let out = dir.join("out");
        let tried: std::sync::Arc<std::sync::Mutex<Vec<String>>> = Default::default();
        let seen = tried.clone();
        let _w = hook(&out, "write", move |name| {
            seen.lock().unwrap().push(name.to_string());
            Ok(())
        });
        let _h = hook(&out, "identify", move |name| match name == at {
            true => Err(format!("errno:{}", libc::EIO)),
            false => Ok(()),
        });
        let e = run_in(FIRST, &out, writes).unwrap_err();
        let run = only_run(&out);
        assert_eq!(
            e,
            format!(
                "could not be identified when it was made, so left: {} ({}); run dir: {}",
                run.join(at).display(),
                std::io::Error::from_raw_os_error(libc::EIO),
                run.display()
            ),
            "{at}"
        );
        // At a.txt, done is never made (the check before it).
        let want: &[&str] = match at == DONE {
            true => &["a.txt", "a.png", DONE, REFUSED],
            false => &["a.txt", "a.png", REFUSED],
        };
        assert_eq!(*tried.lock().unwrap(), want, "{at}");
        let mut want = vec![at.to_string(), REFUSED.to_string()];
        want.sort();
        assert_eq!(names(&run), want, "{at}");
        assert_eq!(
            std::fs::read_to_string(run.join(REFUSED)).unwrap(),
            format!("{e}\n"),
            "{at}"
        );
        let name = run.file_name().unwrap().to_string_lossy().into_owned();
        drop((_w, _h));
        let ((), w) = run_in(FIRST, &out, writes).unwrap();
        assert_eq!(w.incomplete, [format!("{name} (refused)")], "{at}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn a_file_whose_identity_cannot_be_read_is_not_read_back() {
    // gates.jsonl made, its identity not readable: the run cannot tell it from a file another
    // hand put at its name, so it does not read it back; the run fails on that, and leaves it,
    // said so.
    let dir = temp("unidentified-read");
    let out = dir.join("out");
    let _h = hook(&out, "identify", |name| match name == "gates.jsonl" {
        true => Err(format!("errno:{}", libc::EIO)),
        false => Ok(()),
    });
    let e = run_in(FIRST, &out, |run: &Run| -> Result<(), String> {
        run.write("gates.jsonl", b"snapshot")?;
        run.read("gates.jsonl", 64).map(|_| ())
    })
    .unwrap_err();
    let run = only_run(&out);
    let p = run.join("gates.jsonl");
    assert_eq!(
        e,
        format!(
            "{}: not read back (it could not be identified when it was made); could not be \
             identified when it was made, so left: {} ({}); run dir: {}",
            p.display(),
            p.display(),
            std::io::Error::from_raw_os_error(libc::EIO),
            run.display()
        )
    );
    assert_eq!(names(&run), ["gates.jsonl", REFUSED]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_kept_diagnostic_whose_identity_cannot_be_read_is_said_too() {
    // gates.jsonl (a file a failed run keeps) made, its identity not readable; the run then
    // fails for its own reason. Its cleanup leaves the snapshot, as it always does, and its
    // error and refused.txt say it could not be identified.
    let dir = temp("unidentified-kept");
    let out = dir.join("out");
    let _h = hook(&out, "identify", |name| match name == "gates.jsonl" {
        true => Err(format!("errno:{}", libc::EIO)),
        false => Ok(()),
    });
    let e = run_in(FIRST, &out, fails).unwrap_err();
    let run = only_run(&out);
    assert_eq!(
        e,
        format!(
            "no rows; could not be identified when it was made, so left: {} ({}); run dir: {}",
            run.join("gates.jsonl").display(),
            std::io::Error::from_raw_os_error(libc::EIO),
            run.display()
        )
    );
    assert_eq!(names(&run), ["gates.jsonl", REFUSED]);
    assert_eq!(
        std::fs::read_to_string(run.join(REFUSED)).unwrap(),
        format!("{e}\n")
    );
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
    // diagnostics are, and refused.txt says the returned error. done is the first file the
    // cleanup deletes: a cleanup stopped partway leaves no done.
    let dir = temp("done-made");
    let out = dir.join("out");
    let _h = hook(&out, "fill", |name| {
        if name == DONE {
            Err(format!("errno:{}", libc::ENOSPC))
        } else {
            Ok(())
        }
    });
    let order: std::sync::Arc<std::sync::Mutex<Vec<String>>> = Default::default();
    let seen = order.clone();
    let _d = hook(&out, "delete", move |name| {
        seen.lock().unwrap().push(name.to_string());
        Ok(())
    });
    let e = run_in(FIRST, &out, |run: &Run| -> Result<(), String> {
        writes(run)?;
        run.write("gates.jsonl", b"snapshot")
    })
    .unwrap_err();
    let run = only_run(&out);
    assert_eq!(*order.lock().unwrap(), [DONE, "a.txt", "a.png"]);
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
    // The run dir made, then not openable for a reason of the system's (injected at the open:
    // no permission): the run fails saying the run dir and that it is left empty with no
    // refused.txt; the next run lists it as incomplete.
    let dir = temp("unopened");
    let out = dir.join("out");
    let e = {
        let _h = hook(&out, "open", |_| Err(format!("errno:{}", libc::EACCES)));
        run_in(FIRST, &out, writes).unwrap_err()
    };
    let run = only_run(&out);
    assert_eq!(
        e,
        format!(
            "{}: {}: the run dir is left empty, with no refused.txt (it could not be opened to \
             write one); the next run lists it as incomplete; run dir: {}",
            run.display(),
            std::io::Error::from_raw_os_error(libc::EACCES),
            run.display()
        )
    );
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

#[test]
fn where_a_dir_is_now_is_said_only_for_the_identity_held() {
    // A dir held open: where it is now is its path for its own identity, and not known for any
    // other (the identity given is compared, not one read again from the fd).
    let dir = temp("where-now");
    let f = at::open_dir(&dir).unwrap();
    let id = at::id(&f).unwrap();
    assert_eq!(at::where_now(&f, id), Some(dir.canonicalize().unwrap()));
    assert_eq!(at::where_now(&f, (id.0, id.1 ^ 1)), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_raise_goes_up_to_the_lower_of_the_hard_limit_and_the_cap_and_never_down() {
    let inf = libc::RLIM_INFINITY;
    for (cur, max, cap, want) in [
        (64, 1 << 20, 1 << 20, Some(1 << 20)),
        (64, inf, 10240, Some(10240)),
        (64, 512, 10240, Some(512)),
        (64, 64, 10240, None),
        (10240, inf, 10240, None),
        (20000, inf, 10240, None),
        (20000, 30000, 10240, None),
    ] {
        assert_eq!(at::raised(cur, max, cap), want, "{cur} {max} {cap}");
    }
}

#[test]
fn each_file_the_run_made_is_still_held_when_its_delete_is_checked() {
    // At each delete of a failed run (its seam, before the fstatat and the unlinkat), the file
    // the run made is still held open, once: its inode cannot have been freed and its number
    // given to another file.
    let dir = temp("held-at-delete");
    let out = dir.join("out");
    let ids: Shared<(String, (u64, u64))> = Default::default();
    let (seen, looked) = (ids.clone(), ids.clone());
    let checked: Shared<(String, usize)> = Default::default();
    let c = checked.clone();
    let _h = hook(&out, "delete", move |name| {
        let id = looked
            .lock()
            .unwrap()
            .iter()
            .find(|(n, _)| n == name)
            .map(|x| x.1);
        c.lock()
            .unwrap()
            .push((name.to_string(), id.map_or(0, held)));
        Ok(())
    });
    let e = run_in(FIRST, &out, |run: &Run| -> Result<(), String> {
        writes(run)?;
        for m in run.made.borrow().iter() {
            seen.lock()
                .unwrap()
                .push((m.name.clone(), m.id.clone().unwrap()));
        }
        Err("no rows".into())
    })
    .unwrap_err();
    assert_eq!(e, format!("no rows; run dir: {}", only_run(&out).display()));
    assert_eq!(
        *checked.lock().unwrap(),
        [("a.txt".to_string(), 1), ("a.png".to_string(), 1)]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_run_dir_removed_while_the_run_writes_is_said_to_be_nowhere_known() {
    // The run dir removed (not renamed) before the run's second write: the system may still
    // give a path for the dir the run holds (macOS gives the old one, where another dir may be
    // made), but no dir there is the run's, so where it is now is not known.
    let dir = temp("removed-where");
    let out = dir.join("out");
    let o = out.clone();
    let _h = hook(&out, "write", move |name| {
        if name == "b.txt" {
            let run = only_run(&o);
            std::fs::remove_dir_all(&run).unwrap();
            std::fs::create_dir(&run).unwrap();
        }
        Ok(())
    });
    let e = run_in(FIRST, &out, |run: &Run| -> Result<(), String> {
        run.write("a.txt", b"a")?;
        run.write("b.txt", b"b")
    })
    .unwrap_err();
    assert!(e.contains("(where it is now is not known)"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_run_out_of_open_files_for_real_still_writes_its_refusal() {
    // In a child under a limit of 64 open files (soft and hard: the raise cannot lift it), a run
    // of 400 files meets the system's own "too many open files", not one injected. Each file
    // the cleanup deletes lets its fd go, so refused.txt can be made: the run dir holds it alone,
    // and the error says how to go on.
    const NAME: &str = "a_run_out_of_open_files_for_real_still_writes_its_refusal";
    if !is_child(&here(NAME)) {
        return in_child(&here(NAME), "ulimit -n 64", |_| {});
    }
    assert_eq!(nofile(), (64, 64));
    let dir = temp("emfile-real");
    let out = dir.join("out");
    let e = run_in(FIRST, &out, |run: &Run| -> Result<(), String> {
        for k in 0..400 {
            run.write(&format!("f{k}.txt"), b"x")?;
        }
        Ok(())
    })
    .unwrap_err();
    let run = only_run(&out);
    assert!(
        e.contains("Too many open files") && e.contains("ulimit -n"),
        "{e}"
    );
    assert_eq!(names(&run), [REFUSED], "{e}");
    assert_eq!(
        std::fs::read_to_string(run.join(REFUSED)).unwrap(),
        format!("{e}\n")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_low_soft_limit_on_open_files_is_raised_for_the_run() {
    // In a child under a soft limit of 64 open files, the hard one above: the run raises the
    // soft limit to the hard one, as far as the system caps it, so 400 files held at once fit;
    // the hard limit is as it was.
    const NAME: &str = "a_low_soft_limit_on_open_files_is_raised_for_the_run";
    if !is_child(&here(NAME)) {
        return in_child(&here(NAME), "ulimit -Sn 64", |_| {});
    }
    let before = nofile();
    assert_eq!(before.0, 64);
    assert!(
        before.1 > 1024,
        "{before:?}: no room under the hard limit to raise to"
    );
    let dir = temp("raise");
    let out = dir.join("out");
    let r = run_in(FIRST, &out, |run: &Run| -> Result<(), String> {
        for k in 0..400 {
            run.write(&format!("f{k}.txt"), b"x")?;
        }
        Ok(())
    });
    assert!(r.is_ok(), "{:?}", r.err());
    assert_eq!(nofile(), (before.1.min(at::cap()), before.1));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_soft_limit_on_open_files_above_the_cap_is_kept() {
    // In a child whose soft limit is above the system's cap, where the hard limit lets it be
    // (on macOS: OPEN_MAX), else at the hard limit: the raise leaves both as they are.
    const NAME: &str = "a_soft_limit_on_open_files_above_the_cap_is_kept";
    const SOFT: &str = "U6_HDTOOL_SOFT";
    if !is_child(&here(NAME)) {
        let (cap, hard) = (at::cap(), nofile().1);
        let soft = match hard > cap {
            true => cap + (hard - cap).min(1000),
            false => hard,
        };
        return in_child(&here(NAME), &format!("ulimit -Sn {soft}"), |c| {
            c.env(SOFT, soft.to_string());
        });
    }
    let before = nofile();
    assert_eq!(before.0.to_string(), std::env::var(SOFT).unwrap());
    at::raise_open_files();
    assert_eq!(nofile(), before);
}

#[test]
fn a_run_dir_moved_again_while_it_is_cleaned_up_is_said_to_have_moved_once() {
    // The run dir renamed away at the run's last file (the check before done finds it there),
    // then again while the failed run deletes its files: the error says once that it moved,
    // where the check found it, and the refusal is written where the dir is at the end.
    let dir = temp("moved-twice");
    let real = dir.canonicalize().unwrap();
    let out = dir.join("out");
    let (one, two) = (dir.join("one"), dir.join("two"));
    let (o, a) = (out.clone(), one.clone());
    let _w = hook(&out, "write", move |name| {
        if name == "a.png" {
            std::fs::rename(only_run(&o), &a).unwrap();
        }
        Ok(())
    });
    let (a, b) = (one.clone(), two.clone());
    let _d = hook(&out, "delete", move |name| {
        if name == "a.txt" {
            std::fs::rename(&a, &b).unwrap();
        }
        Ok(())
    });
    let e = run_in(FIRST, &out, writes).unwrap_err();
    assert_eq!(
        e.matches("was moved or removed while the run wrote it")
            .count(),
        1,
        "{e}"
    );
    let now = format!("(it is now at {})", real.join("one").display());
    assert!(e.contains(&now), "{e}");
    assert_eq!(names(&two), [REFUSED]);
    assert_eq!(
        std::fs::read_to_string(two.join(REFUSED)).unwrap(),
        format!("{e}\n")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_of_the_run_moved_to_another_name_is_left_and_said() {
    // After the run wrote a.txt, a.png and c.txt, another hand renames a.txt within the run
    // dir, links a.png elsewhere and deletes its name, and deletes c.txt. Each name the run made
    // is gone, but a.txt and a.png still have a name (their link counts, read on the fds the run
    // holds): both are left, said to have moved; c.txt has none, so it is taken as deleted.
    let dir = temp("moved-name");
    let out = dir.join("out");
    let elsewhere = dir.join("elsewhere.png");
    let (o, el) = (out.clone(), elsewhere.clone());
    let _h = hook(&out, "write", move |name| {
        if name == "gates.jsonl" {
            let run = only_run(&o);
            std::fs::rename(run.join("a.txt"), run.join("b.txt")).unwrap();
            std::fs::hard_link(run.join("a.png"), &el).unwrap();
            std::fs::remove_file(run.join("a.png")).unwrap();
            std::fs::remove_file(run.join("c.txt")).unwrap();
        }
        Ok(())
    });
    let e = run_in(FIRST, &out, |run: &Run| -> Result<(), String> {
        writes(run)?;
        run.write("c.txt", b"c")?;
        run.write("gates.jsonl", b"snapshot")?;
        Err("no rows".into())
    })
    .unwrap_err();
    let run = only_run(&out);
    assert_eq!(
        e,
        format!(
            "no rows; left, as it could not be deleted: {} (moved to another name), {} (moved \
             to another name); run dir: {}",
            run.join("a.txt").display(),
            run.join("a.png").display(),
            run.display()
        )
    );
    assert_eq!(names(&run), ["b.txt", "gates.jsonl", REFUSED]);
    assert_eq!(std::fs::read(run.join("b.txt")).unwrap(), b"a");
    assert!(elsewhere.is_file());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_link_at_a_name_the_run_made_to_that_very_file_is_still_another_file() {
    // After the run wrote a.txt, another hand renames it aside and puts a link to it at its
    // name: what the link points at is the run's file (the inode it holds), the link is not.
    // The check reads the entry itself (the link not followed), so the link is left, said so;
    // the file aside is left too (its name is not the run's).
    let dir = temp("linked-same");
    let out = dir.join("out");
    let o = out.clone();
    let _h = hook(&out, "write", move |name| {
        if name == "gates.jsonl" {
            let run = only_run(&o);
            std::fs::rename(run.join("a.txt"), run.join("a.aside")).unwrap();
            std::os::unix::fs::symlink(run.join("a.aside"), run.join("a.txt")).unwrap();
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
    assert_eq!(
        std::fs::read_link(run.join("a.txt")).unwrap(),
        run.join("a.aside")
    );
    assert_eq!(std::fs::read(run.join("a.aside")).unwrap(), b"a");
    assert_eq!(names(&run), ["a.aside", "a.txt", "gates.jsonl", REFUSED]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_incomplete_runs_are_listed_from_the_out_dir_opened_not_by_its_path() {
    // Two incomplete runs in --out; once --out is opened, it is moved away, and a dir put at
    // its path with an incomplete run of another's and, by the name of the first, a run that
    // ended (its done). The runs listed, and each one's done, are those of the dir opened (where
    // the run dir is made), not of the dir now at the path, which is left as it is.
    let dir = temp("listed-opened");
    let out = dir.join("out");
    let away = dir.join("away");
    std::fs::create_dir_all(out.join(run_name(1, 7))).unwrap();
    std::fs::create_dir_all(out.join(run_name(3, 9))).unwrap();
    let (o, a) = (out.clone(), away.clone());
    let _h = hook(&out, "opened", move |_| {
        std::fs::rename(&o, &a).unwrap();
        std::fs::create_dir_all(o.join(run_name(2, 8))).unwrap();
        std::fs::create_dir_all(o.join(run_name(1, 7))).unwrap();
        std::fs::write(o.join(run_name(1, 7)).join(DONE), b"").unwrap();
        Ok(())
    });
    let e = run_in(FIRST, &out, writes).unwrap_err();
    assert!(
        e.ends_with(&format!(
            "; incomplete runs beside it (left as they are): {}, {}",
            run_name(1, 7),
            run_name(3, 9)
        )),
        "{e}"
    );
    assert!(!e.contains(&run_name(2, 8)), "{e}");
    assert_eq!(names(&out), [run_name(1, 7), run_name(2, 8)]);
    assert_eq!(names(&out.join(run_name(1, 7))), [DONE]);
    assert!(names(&out.join(run_name(2, 8))).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_out_dir_moved_into_a_work_tree_once_opened_is_refused() {
    // --out opened, then moved into a git work tree (a dir with a .git) before anything is
    // made in it: where the dir opened is now is protected, so the run fails, saying so, and
    // makes nothing there.
    let dir = temp("opened-protected");
    let (plain, tree) = (dir.join("plain"), dir.join("tree"));
    let out = plain.join("out");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::create_dir_all(tree.join(".git")).unwrap();
    let (o, to) = (out.clone(), tree.join("out"));
    let _h = hook(&out, "opened", move |_| {
        std::fs::rename(&o, &to).unwrap();
        Ok(())
    });
    let e = run_in(FIRST, &out, writes).unwrap_err();
    let tree = tree.canonicalize().unwrap();
    assert_eq!(
        e,
        format!(
            "{}: the dir opened is now at {}, inside the git work tree {}; the tool writes only \
             outside it (spec 07 Rights)",
            out.display(),
            tree.join("out").display(),
            tree.display()
        )
    );
    assert!(names(&tree.join("out")).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_link_put_in_place_of_the_new_run_dir_before_it_is_opened_is_not_followed() {
    // An older incomplete run in --out; the new run dir made, then, before it is opened,
    // replaced by a link to a dir of the owner's (a file of theirs in it). The open refuses
    // the link: the run fails, saying a link is now at its name, with its run dir and the
    // incomplete run beside it; nothing is written where the link points.
    let dir = temp("unopened-link");
    let out = dir.join("out");
    let sentinel = dir.join("sentinel");
    std::fs::create_dir_all(&sentinel).unwrap();
    std::fs::write(sentinel.join("a.txt"), b"the owner's").unwrap();
    std::fs::create_dir_all(out.join(run_name(1, 7))).unwrap();
    let had = tree(&sentinel);
    let (o, to) = (out.clone(), sentinel.clone());
    let _h = hook(&out, "open", move |name| {
        let run = o.canonicalize().unwrap().join(name);
        std::fs::remove_dir(&run).unwrap();
        std::os::unix::fs::symlink(&to, &run).unwrap();
        Ok(())
    });
    let e = run_in(FIRST, &out, writes).unwrap_err();
    let parent = out.canonicalize().unwrap();
    let mine = names(&parent)
        .into_iter()
        .find(|n| *n != run_name(1, 7))
        .unwrap();
    let run = parent.join(&mine);
    // A link opened with O_DIRECTORY | O_NOFOLLOW: ENOTDIR, on macOS and Linux alike
    // (measured; ELOOP is what O_NOFOLLOW alone gives).
    let said = |errno| {
        format!(
            "{}: {}: a link or a file is now at its name; the run's dir was moved or removed; \
             run dir: {}; incomplete runs beside it (left as they are): {}",
            run.display(),
            std::io::Error::from_raw_os_error(errno),
            run.display(),
            run_name(1, 7)
        )
    };
    assert_eq!(e, said(libc::ENOTDIR));
    assert_eq!(tree(&sentinel), had);
    assert_eq!(std::fs::read_link(&run).unwrap(), sentinel);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_the_run_reads_back_is_read_only_while_it_is_the_one_it_made() {
    // The run writes a.txt; another hand deletes it and puts a file of its own at the name. The
    // run's read of a.txt fails, it is not the file the run made, and gives nothing of theirs;
    // a file the run never made is not read either; its own c.txt is.
    let dir = temp("read-identity");
    let out = dir.join("out");
    let e = run_in(FIRST, &out, |run: &Run| -> Result<(), String> {
        run.write("a.txt", b"a")?;
        let a = run.dir().join("a.txt");
        std::fs::remove_file(&a).unwrap();
        std::fs::write(&a, b"theirs").unwrap();
        let b = run.dir().join("b.txt");
        std::fs::write(&b, b"theirs").unwrap();
        run.write("c.txt", b"c")?;
        assert_eq!(
            run.read("a.txt", 64),
            Err(format!("{}: not the file the run made", a.display()))
        );
        assert_eq!(
            run.read("b.txt", 64),
            Err(format!("{}: not a file the run made", b.display()))
        );
        assert_eq!(run.read("c.txt", 64), Ok(b"c".to_vec()));
        Err("no rows".into())
    })
    .unwrap_err();
    assert!(
        e.starts_with("no rows; left, as it could not be deleted: "),
        "{e}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_read_out_of_open_files_says_how_to_go_on() {
    // In a child under a limit of 64 open files: the run's a.txt made, then every fd left taken
    // (by the test). The read back of a.txt, a read of an input, and the list of the masters
    // each meet the system's "too many open files", and each says how to go on.
    const NAME: &str = "a_read_out_of_open_files_says_how_to_go_on";
    if !is_child(&here(NAME)) {
        return in_child(&here(NAME), "ulimit -n 64", |_| {});
    }
    let dir = temp("emfile-reads");
    let out = dir.join("out");
    let input = dir.join("in.txt");
    std::fs::write(&input, b"in").unwrap();
    let e = run_in(FIRST, &out, |run: &Run| -> Result<(), String> {
        run.write("a.txt", b"a")?;
        let mut taken = Vec::new();
        let full = loop {
            match File::open(&input) {
                Ok(f) => taken.push(f),
                Err(e) => break e,
            }
        };
        assert_eq!(full.raw_os_error(), Some(libc::EMFILE), "{full}");
        let said = [
            run.read("a.txt", 64).unwrap_err(),
            read_capped(&input, 64).unwrap_err(),
            crate::calibrate::masters(&dir).unwrap_err(),
        ];
        drop(taken);
        for s in said {
            assert!(
                s.contains("Too many open files") && s.contains("ulimit -n"),
                "{s}"
            );
        }
        Err("no rows".into())
    })
    .unwrap_err();
    assert!(e.starts_with("no rows; run dir: "), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_run_that_cannot_tell_where_its_out_dir_is_writes_nothing() {
    // --out opened, then where it is now not known (as with no /proc and no F_GETPATH; made
    // so at a seam): the tool cannot tell it is outside the repository, so the run fails before
    // anything is made in it.
    let dir = temp("unlocated");
    let out = dir.join("out");
    std::fs::create_dir_all(out.join(run_name(1, 7))).unwrap();
    std::fs::write(out.join("notes.txt"), b"the owner's").unwrap();
    let had = tree(&out);
    let _h = hook(&out, "locate", |_| Err("not known".into()));
    let e = run_in(FIRST, &out, writes).unwrap_err();
    assert_eq!(
        e,
        format!(
            "{}: where the dir opened is now could not be read, so the tool cannot tell it is \
             outside the repository: nothing is written (spec 07 Rights)",
            out.display()
        )
    );
    assert_eq!(tree(&out), had);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_refusal_whose_identity_cannot_be_read_is_said_to_be_left() {
    // The run fails; its refused.txt made, its identity not readable (injected at the fstat):
    // the error it gives says so too (refused.txt holds the error as it was written), and the
    // file is left.
    let dir = temp("unidentified-refused");
    let out = dir.join("out");
    let _h = hook(&out, "identify", |name| match name == REFUSED {
        true => Err(format!("errno:{}", libc::EIO)),
        false => Ok(()),
    });
    let e = run_in(FIRST, &out, fails).unwrap_err();
    let run = only_run(&out);
    let said = format!("no rows; run dir: {}", run.display());
    assert_eq!(
        e,
        format!(
            "{said}; {REFUSED} could not be identified when it was made, so it is left ({})",
            std::io::Error::from_raw_os_error(libc::EIO)
        )
    );
    assert_eq!(names(&run), ["gates.jsonl", REFUSED]);
    assert_eq!(
        std::fs::read_to_string(run.join(REFUSED)).unwrap(),
        format!("{said}\n")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_run_dir_swapped_for_a_link_while_it_is_listed_is_not_read_through() {
    // An incomplete run in --out, and a link of the owner's named as a run dir, to a dir of
    // theirs with no done. The incomplete run's dir is opened to be read, then swapped for a link
    // to another dir of theirs, with a done (at the read of its markers): its markers are read in
    // the dir opened, so it is still incomplete; the link that was there is no run dir, not
    // listed; neither link is followed, and the dirs they point at are as they were.
    let dir = temp("listed-link");
    let out = dir.join("out");
    let (sentinel, empty) = (dir.join("sentinel"), dir.join("empty"));
    std::fs::create_dir_all(&sentinel).unwrap();
    std::fs::create_dir_all(&empty).unwrap();
    std::fs::write(sentinel.join(DONE), b"").unwrap();
    std::fs::create_dir_all(out.join(run_name(1, 7))).unwrap();
    std::os::unix::fs::symlink(&empty, out.join(run_name(2, 8))).unwrap();
    let had = (tree(&sentinel), tree(&empty));
    let (o, aside, to) = (out.clone(), dir.join("aside"), sentinel.clone());
    let n = run_name(1, 7);
    let _h = hook(&out, "stat", move |name| {
        let at = o.join(&n);
        if name == n && !at.is_symlink() {
            std::fs::rename(&at, &aside).unwrap();
            std::os::unix::fs::symlink(&to, &at).unwrap();
        }
        Ok(())
    });
    let ((), w) = run_in(FIRST, &out, writes).unwrap();
    assert_eq!(w.incomplete, [run_name(1, 7)]);
    assert!(w.unreadable.is_empty(), "{:?}", w.unreadable);
    assert_eq!((tree(&sentinel), tree(&empty)), had);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_run_dir_beside_the_run_that_cannot_be_opened_is_said_and_out_of_files_fails_the_run() {
    // Two incomplete runs in --out. One cannot be opened to be read (an error of the system's,
    // injected): it is listed as one that cannot be read, with why. Then out of open files at
    // that open: the run fails before it makes anything, saying how to go on.
    let dir = temp("list-open");
    let out = dir.join("out");
    std::fs::create_dir_all(out.join(run_name(1, 7))).unwrap();
    std::fs::create_dir_all(out.join(run_name(2, 8))).unwrap();
    let errno = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(libc::EIO));
    let (n, k) = (run_name(2, 8), errno.clone());
    let _h = hook(&out, "list", move |name| match name == n {
        true => Err(format!(
            "errno:{}",
            k.load(std::sync::atomic::Ordering::SeqCst)
        )),
        false => Ok(()),
    });
    let ((), w) = run_in(FIRST, &out, writes).unwrap();
    assert_eq!(w.incomplete, [run_name(1, 7)]);
    assert_eq!(
        w.unreadable,
        [format!(
            "{} ({})",
            run_name(2, 8),
            std::io::Error::from_raw_os_error(libc::EIO)
        )]
    );
    errno.store(libc::EMFILE, std::sync::atomic::Ordering::SeqCst);
    let had = tree(&out);
    let e = run_in(FIRST, &out, writes).unwrap_err();
    assert_eq!(
        e,
        format!(
            "{}: {} (a run holds each file it makes open until it ends: raise the limit on open \
             files, ulimit -n)",
            out.canonicalize().unwrap().join(run_name(2, 8)).display(),
            std::io::Error::from_raw_os_error(libc::EMFILE)
        )
    );
    assert_eq!(tree(&out), had);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_run_dir_out_of_open_files_to_be_opened_says_how_to_go_on() {
    // In a child under a limit of 64 open files: the run dir made, then every fd left taken (by
    // the test) before it is opened. The open meets the system's "too many open files": the run
    // fails saying how to go on, with the run dir, that it is left empty, and the incomplete
    // run beside it.
    const NAME: &str = "a_run_dir_out_of_open_files_to_be_opened_says_how_to_go_on";
    if !is_child(&here(NAME)) {
        return in_child(&here(NAME), "ulimit -n 64", |_| {});
    }
    let dir = temp("emfile-open");
    let out = dir.join("out");
    std::fs::create_dir_all(out.join(run_name(1, 7))).unwrap();
    let probe = dir.join("probe.txt");
    std::fs::write(&probe, b"p").unwrap();
    let taken: Shared<File> = Default::default();
    let t = taken.clone();
    let _h = hook(&out, "open", move |_| {
        let mut t = t.lock().unwrap();
        let full = loop {
            match File::open(&probe) {
                Ok(f) => t.push(f),
                Err(e) => break e,
            }
        };
        assert_eq!(full.raw_os_error(), Some(libc::EMFILE), "{full}");
        Ok(())
    });
    let e = run_in(FIRST, &out, writes).unwrap_err();
    taken.lock().unwrap().clear();
    let parent = out.canonicalize().unwrap();
    let run = parent.join(
        names(&parent)
            .into_iter()
            .find(|n| *n != run_name(1, 7))
            .unwrap(),
    );
    assert_eq!(
        e,
        format!(
            "{}: {} (a run holds each file it makes open until it ends: raise the limit on open \
             files, ulimit -n): the run dir is left empty, with no refused.txt (it could not be \
             opened to write one); the next run lists it as incomplete; run dir: {}; incomplete \
             runs beside it (left as they are): {}",
            run.display(),
            std::io::Error::from_raw_os_error(libc::EMFILE),
            run.display(),
            run_name(1, 7)
        )
    );
    assert!(names(&run).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_kept_file_of_the_run_moved_to_another_name_is_said() {
    // gates.jsonl (a file the run keeps when it fails) made, then renamed within the run dir
    // by another hand; the run then fails for its own reason. The snapshot is not at its name:
    // the error and refused.txt say so, and the file aside is left.
    let dir = temp("kept-moved");
    let out = dir.join("out");
    let o = out.clone();
    let _h = hook(&out, "write", move |name| {
        if name == "a.png" {
            let run = only_run(&o);
            std::fs::rename(run.join("gates.jsonl"), run.join("g.aside")).unwrap();
        }
        Ok(())
    });
    let e = run_in(FIRST, &out, |run: &Run| -> Result<(), String> {
        run.write("gates.jsonl", b"snapshot")?;
        writes(run)?;
        Err("no rows".into())
    })
    .unwrap_err();
    let run = only_run(&out);
    assert_eq!(
        e,
        format!(
            "no rows; kept, but not as the run made it: {} (moved to another name); run dir: {}",
            run.join("gates.jsonl").display(),
            run.display()
        )
    );
    assert_eq!(names(&run), ["g.aside", REFUSED]);
    assert_eq!(std::fs::read(run.join("g.aside")).unwrap(), b"snapshot");
    assert_eq!(
        std::fs::read_to_string(run.join(REFUSED)).unwrap(),
        format!("{e}\n")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_another_hand_put_at_a_kept_name_is_said() {
    // gates.jsonl made, then deleted by another hand and a file of theirs put at its name; the
    // run then fails for its own reason. The file at the name is not the run's: it is left, as
    // a kept name always is, and said so.
    let dir = temp("kept-swapped");
    let out = dir.join("out");
    let o = out.clone();
    let _h = hook(&out, "write", move |name| {
        if name == "a.png" {
            let run = only_run(&o);
            std::fs::remove_file(run.join("gates.jsonl")).unwrap();
            std::fs::write(run.join("gates.jsonl"), b"theirs").unwrap();
        }
        Ok(())
    });
    let e = run_in(FIRST, &out, |run: &Run| -> Result<(), String> {
        run.write("gates.jsonl", b"snapshot")?;
        writes(run)?;
        Err("no rows".into())
    })
    .unwrap_err();
    let run = only_run(&out);
    assert_eq!(
        e,
        format!(
            "no rows; kept, but not as the run made it: {} (not the file the run made); run \
             dir: {}",
            run.join("gates.jsonl").display(),
            run.display()
        )
    );
    assert_eq!(names(&run), ["gates.jsonl", REFUSED]);
    assert_eq!(std::fs::read(run.join("gates.jsonl")).unwrap(), b"theirs");
    assert_eq!(
        std::fs::read_to_string(run.join(REFUSED)).unwrap(),
        format!("{e}\n")
    );
    let _ = std::fs::remove_dir_all(&dir);
}
