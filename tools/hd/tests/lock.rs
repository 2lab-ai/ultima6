//! The work dir's lock, in a test binary of its own: a process that spawns children while a
//! test takes and drops the lock lets a child hold a copy of the lock's open handle for the
//! moment between its fork and its exec (measured: 18 of 3000 takes refused beside a thread
//! spawning `/usr/bin/true`), which the library tests, spawning calls in parallel, would see as
//! a holder. Nothing here spawns a process. (For the tool this is moot: a command holds the
//! lock until it exits, and its children exec at once with the handle closed on exec.)

use std::path::Path;

use u6_hdtool::work::Work;

fn work(name: &str) -> Work {
    let dir = std::env::temp_dir().join(format!("u6-hdtool-lock-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let w = Work::open(Some(&dir)).unwrap();
    w.mkdirs().unwrap();
    w
}

#[test]
fn the_work_dir_takes_one_command_at_a_time() {
    let w = work("lock");
    // Held with the file still empty (as between a holder's open and its pid write): refused.
    let first = w.lock().unwrap();
    std::fs::write(w.path(".u6-hd.lock"), b"").unwrap();
    let e = w.lock().unwrap_err();
    assert!(e.contains("another u6-hd is using"), "{e}");
    std::fs::write(w.path(".u6-hd.lock"), std::process::id().to_string()).unwrap();
    let e = w.lock().unwrap_err();
    assert!(
        e.contains(&format!("another u6-hd (pid {})", std::process::id())),
        "{e}"
    );
    // Two handles of the one file, as two processes would hold them: dropping the holder is
    // what frees it, and the file stays where it is (never removed, never re-created).
    let ino = |p: &Path| std::os::unix::fs::MetadataExt::ino(&std::fs::metadata(p).unwrap());
    let before = ino(&w.path(".u6-hd.lock"));
    drop(first);
    assert!(w.path(".u6-hd.lock").exists());
    let again = w.lock().unwrap();
    assert_eq!(ino(&w.path(".u6-hd.lock")), before);
    assert!(w.lock().is_err());
    drop(again);
    // A pid written by a process that is gone, or garbage, holds nothing: only the lock does.
    std::fs::write(w.path(".u6-hd.lock"), "999999999 not a pid").unwrap();
    let held = w.lock().unwrap();
    assert_eq!(
        std::fs::read_to_string(w.path(".u6-hd.lock")).unwrap(),
        std::process::id().to_string()
    );
    drop(held);
    // A symlink in its place is refused, not followed.
    std::fs::remove_file(w.path(".u6-hd.lock")).unwrap();
    let target = w.path("elsewhere");
    std::fs::write(&target, b"keep").unwrap();
    std::os::unix::fs::symlink(&target, w.path(".u6-hd.lock")).unwrap();
    assert!(w.lock().unwrap_err().contains("not a regular file"));
    assert_eq!(std::fs::read(&target).unwrap(), b"keep");
    let _ = std::fs::remove_dir_all(&w.root);
}

#[test]
fn contenders_released_together_get_one_lock() {
    use std::sync::Barrier;
    let w = work("lock-race");
    // Eight contenders released at once, 200 times: exactly one holds the lock each time (the
    // others are refused while it holds it, until the round ends). Before each round the file
    // is left as a holder that died between its open and its pid write would leave it.
    for round in 0..200 {
        std::fs::write(w.path(".u6-hd.lock"), b"").unwrap();
        let start = Barrier::new(8);
        let held: Vec<u6_hdtool::work::Lock> = std::thread::scope(|s| {
            let contenders: Vec<_> = (0..8)
                .map(|_| {
                    s.spawn(|| {
                        start.wait();
                        w.lock().ok()
                    })
                })
                .collect();
            contenders
                .into_iter()
                .filter_map(|c| c.join().unwrap())
                .collect()
        });
        assert_eq!(held.len(), 1, "round {round}: {} holders", held.len());
    }
    let _ = std::fs::remove_dir_all(&w.root);
}
