//! Test-only helpers shared by the workspace's tests (a dev-dependency; nothing here is part of
//! a build).
//!
//! Spec 01 §Rights: what the tests print never shows content of the user's files. `assert_eq!`
//! prints both values when it fails, so values read from those files (text, bytes, table rows,
//! records, frames) are compared with [`assert_same!`] instead: its failure names the label,
//! whether the lengths differ and the first index where the two differ, never the values nor
//! the lengths (the length of a list of the user's records or paths is a count of them, so a
//! prefix is reported without the index where the shorter one ends).

/// The owner's local data tree. The environment override makes real-data gates portable;
/// the fallback retains the documented `~/Games/ultima6` layout.
pub fn data_path(relative: &str) -> std::path::PathBuf {
    data_path_from(relative, |key| std::env::var_os(key))
}

fn data_path_from(
    relative: &str,
    env: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> std::path::PathBuf {
    let root = env("U6_DATA_ROOT").unwrap_or_else(|| {
        let home = env("HOME")
            .or_else(|| env("USERPROFILE"))
            .expect("set U6_DATA_ROOT when the home directory is unavailable");
        std::path::PathBuf::from(home)
            .join("Games/ultima6")
            .into_os_string()
    });
    std::path::PathBuf::from(root).join(relative)
}

/// A specific asset override, otherwise a path under [`data_path`].
pub fn data_path_with(key: &str, relative: &str) -> std::path::PathBuf {
    std::env::var_os(key).map_or_else(|| data_path(relative), std::path::PathBuf::from)
}

/// Optional real-data inputs may skip locally, but an explicitly required gate must fail.
pub fn available(present: bool, label: &str) -> bool {
    assert!(
        present
            || std::env::var_os("U6_REQUIRE_ARCHIVE").as_deref() != Some(std::ffi::OsStr::new("1")),
        "U6_REQUIRE_ARCHIVE=1: required local input is missing ({label})"
    );
    present
}

/// The original archive, read locally only. Required-data gates cannot silently skip it.
pub fn archive() -> std::path::PathBuf {
    let path = data_path_with("U6_ARCHIVE", "original/ultima-vi.zip");
    available(path.is_file(), "U6_ARCHIVE/U6_DATA_ROOT archive");
    path
}

/// `assert_eq!` for sequences that hold content of the user's files:
/// `assert_same!(left, right, "label {}", arg)`.
///
/// Both sides are anything `AsRef<[T]>` with the same element type (`str`, `String`, `Vec<T>`,
/// arrays, slices; a scalar goes in as `[x]`). The label is required and takes `format!`
/// arguments. On failure it panics with `<label>: lengths differ, first difference at index
/// <i>` or `<label>: lengths equal, first difference at index <i>` when an element differs, and
/// with `<label>: lengths differ, one is a prefix of the other` when none does (no index: it
/// would be the shorter length).
#[macro_export]
macro_rules! assert_same {
    ($left:expr, $right:expr, $($label:tt)+) => {
        $crate::check_same(&$left, &$right, || ::std::format!($($label)+))
    };
}

/// The comparison behind [`assert_same!`]; the label is built only on failure.
#[track_caller]
pub fn check_same<T, L, R>(left: &L, right: &R, label: impl FnOnce() -> String)
where
    T: PartialEq,
    L: AsRef<[T]> + ?Sized,
    R: AsRef<[T]> + ?Sized,
{
    let (l, r) = (left.as_ref(), right.as_ref());
    if l == r {
        return;
    }
    let Some(at) = l.iter().zip(r).position(|(a, b)| a != b) else {
        panic!("{}: lengths differ, one is a prefix of the other", label());
    };
    let lengths = if l.len() == r.len() {
        "equal"
    } else {
        "differ"
    };
    panic!(
        "{}: lengths {lengths}, first difference at index {at}",
        label()
    );
}

/// `unwrap` for steps on the user's files: a parser's or loader's error can quote the bytes it
/// refused (a value, a word at an offset), and `unwrap` / `expect` print it. `quiet` panics with
/// a fixed message and the caller's place only. For `Result` and `Option` alike.
pub trait Quiet<T> {
    /// The value, or a panic naming no content.
    #[track_caller]
    fn quiet(self) -> T;
}

impl<T, E> Quiet<T> for Result<T, E> {
    #[track_caller]
    fn quiet(self) -> T {
        match self {
            Ok(v) => v,
            Err(_) => panic!("a step on the user's files failed (its error is not shown)"),
        }
    }
}

impl<T> Quiet<T> for Option<T> {
    #[track_caller]
    fn quiet(self) -> T {
        match self {
            Some(v) => v,
            None => panic!("a value the user's files should give is missing"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    #[test]
    fn data_paths_use_the_selected_root_before_the_home_fallback() {
        let resolve = |pairs: &[(&str, &str)]| {
            super::data_path_from("original/game.zip", |key| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| (*v).into())
            })
        };
        assert_eq!(
            resolve(&[("U6_DATA_ROOT", "/owned/data"), ("HOME", "/home/player")]),
            std::path::PathBuf::from("/owned/data/original/game.zip")
        );
        assert_eq!(
            resolve(&[("HOME", "/home/player")]),
            std::path::PathBuf::from("/home/player/Games/ultima6/original/game.zip")
        );
        assert_eq!(
            resolve(&[("USERPROFILE", "profile")]),
            std::path::PathBuf::from("profile/Games/ultima6/original/game.zip")
        );
    }

    /// The panic message of `f`, or `None` when it returned.
    fn failure(f: impl FnOnce()) -> Option<String> {
        let payload = catch_unwind(AssertUnwindSafe(f)).err()?;
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
    }

    #[test]
    fn equal_sequences_pass() {
        assert_same!(String::from("same"), "same", "text");
        assert_same!(vec![1u8, 2, 3], [1, 2, 3], "bytes");
        assert_same!(vec![vec![0u8], vec![]], [vec![0], vec![]], "nested");
        assert_same!([7u16], [7], "a scalar as a one-element array");
    }

    /// The message holds the label, whether the lengths differ and the index; none of the
    /// values (the distinctive letters, their numbers, any `Debug` brackets) nor the lengths.
    #[test]
    fn a_mismatch_names_the_label_lengths_and_index_only() {
        let left = b"SECRET".to_vec();
        let right = b"SECRXT";
        let m = failure(|| assert_same!(left, right, "record {}", 3)).expect("a failure");
        assert_eq!(m, "record 3: lengths equal, first difference at index 4");
        for leak in ["SECR", "83", "88", "6", "[", "\""] {
            assert!(!m.contains(leak), "the message shows {leak:?}");
        }
        let m = failure(|| assert_same!("Zhuge", String::from("Zhugx"), "name")).unwrap();
        assert_eq!(m, "name: lengths equal, first difference at index 4");
        assert!(!m.contains("Zhug") && !m.contains('5'));
    }

    #[test]
    fn quiet_shows_no_error() {
        use super::Quiet;
        assert_eq!(Ok::<u8, String>(3).quiet(), 3);
        assert_eq!(Some(4u8).quiet(), 4);
        let m = failure(|| {
            Err::<u8, String>("byte 0x5A at 0x1F00".into()).quiet();
        })
        .unwrap();
        assert!(!m.contains("5A") && !m.contains("1F00"), "{m}");
        assert!(
            failure(|| {
                None::<u8>.quiet();
            })
            .is_some()
        );
    }

    /// A prefix names neither length nor the index where the shorter one ends (both would be
    /// the shorter length); an element that differs before the end still gets its index.
    #[test]
    fn a_prefix_is_named_without_an_index() {
        let m = failure(|| assert_same!(vec![1u8, 2], [1, 2, 3], "tail")).unwrap();
        assert_eq!(m, "tail: lengths differ, one is a prefix of the other");
        let m = failure(|| assert_same!([7u8, 8, 9, 4, 5], [7, 8, 9], "files")).unwrap();
        assert_eq!(m, "files: lengths differ, one is a prefix of the other");
        assert!(!m.contains('3') && !m.contains('5'), "no length is shown");
        let m = failure(|| assert_same!(vec![1u8, 9], [1, 2, 3], "early")).unwrap();
        assert_eq!(m, "early: lengths differ, first difference at index 1");
    }

    #[test]
    fn nested_sequences_differ_at_the_outer_index() {
        let a = vec![vec![1u8, 2], vec![3]];
        let b = vec![vec![1u8, 2], vec![4]];
        let m = failure(|| assert_same!(a, b, "blocks")).unwrap();
        assert_eq!(m, "blocks: lengths equal, first difference at index 1");
    }
}
