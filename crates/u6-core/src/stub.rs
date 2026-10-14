//! Stub-hit registry behind [`todo_stub!`](crate::todo_stub).
//!
//! A stub point is an original function the port has not written yet. `todo_stub!` records every
//! hit in the registry of the [`Engine`](crate::Engine) that is currently polling (or loading),
//! queues one log line the first time an id is hit, and evaluates the caller's explicit fallback.
//! The engine exposes the counters ([`Engine::stub_hits`](crate::Engine::stub_hits)) and drains
//! the log lines ([`Engine::take_log`](crate::Engine::take_log)); the platform prints them.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

/// Hit counters and pending log lines for one engine.
#[derive(Debug, Default)]
pub struct StubLog {
    hits: RefCell<BTreeMap<&'static str, u64>>,
    lines: RefCell<Vec<String>>,
}

impl StubLog {
    /// Counts a hit; the first hit of an id also queues a log line.
    pub fn record(&self, id: &'static str) {
        let mut hits = self.hits.borrow_mut();
        let n = hits.entry(id).or_insert(0);
        if *n == 0 {
            self.lines.borrow_mut().push(format!("stub: {id}"));
        }
        *n += 1;
    }

    /// Every stub id hit so far with its hit count, sorted by id.
    pub fn hits(&self) -> BTreeMap<&'static str, u64> {
        self.hits.borrow().clone()
    }

    /// Log lines queued since the previous call.
    pub fn take_lines(&self) -> Vec<String> {
        std::mem::take(&mut *self.lines.borrow_mut())
    }
}

thread_local! {
    static ACTIVE: RefCell<Option<Rc<StubLog>>> = const { RefCell::new(None) };
    /// Hits outside any engine scope (unit tests calling ported code directly).
    static DETACHED: Rc<StubLog> = Rc::new(StubLog::default());
}

/// Records a hit of `id` (called by `todo_stub!`).
pub fn hit(id: &'static str) {
    let active = ACTIVE.with(|a| a.borrow().clone());
    match active {
        Some(log) => log.record(id),
        None => DETACHED.with(|log| log.record(id)),
    }
}

/// Hits recorded on this thread outside any engine scope.
pub fn detached_hits() -> BTreeMap<&'static str, u64> {
    DETACHED.with(|log| log.hits())
}

/// Runs `f` with `log` as the active registry, restoring the previous one afterwards (also when
/// `f` panics).
pub(crate) fn scoped<R>(log: &Rc<StubLog>, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<Rc<StubLog>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let prev = self.0.take();
            ACTIVE.with(|a| *a.borrow_mut() = prev);
        }
    }
    let prev = ACTIVE.with(|a| a.borrow_mut().replace(Rc::clone(log)));
    let _restore = Restore(prev);
    f()
}

/// Marks an original function that is not ported yet: records the hit (logged once per id) and
/// evaluates to the explicit fallback.
///
/// ```
/// let tiles: u16 = u6_core::todo_stub!("0x000000 example", 0);
/// assert_eq!(tiles, 0);
/// ```
///
/// The id must start with the original address (`"0xADDR what"`); `tools/rea/port_map.py` reads
/// it from the source to list the function as stubbed (examples and tests use addresses below the
/// image base 0x100000 so they never match a real function).
#[macro_export]
macro_rules! todo_stub {
    ($id:literal, $fallback:expr $(,)?) => {{
        $crate::stub::hit($id);
        $fallback
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_once_and_counts_every_hit_in_the_active_scope() {
        let log = Rc::new(StubLog::default());
        let v = scoped(&log, || {
            for _ in 0..3 {
                let _: u8 = crate::todo_stub!("0x000001 test stub", 7);
            }
            crate::todo_stub!("0x000002 other", 9)
        });
        assert_eq!(v, 9);
        let hits = log.hits();
        assert_eq!(hits.get("0x000001 test stub"), Some(&3));
        assert_eq!(hits.get("0x000002 other"), Some(&1));
        assert_eq!(
            log.take_lines(),
            ["stub: 0x000001 test stub", "stub: 0x000002 other"]
        );
        assert!(log.take_lines().is_empty());
        // Outside the scope, hits land in the detached registry, not in `log`.
        crate::todo_stub!("0x000001 test stub", ());
        assert_eq!(log.hits().get("0x000001 test stub"), Some(&3));
        assert_eq!(detached_hits().get("0x000001 test stub"), Some(&1));
    }
}
