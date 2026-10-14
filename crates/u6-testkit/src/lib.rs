//! Test-only helpers shared by the workspace's tests (a dev-dependency; nothing here is part of
//! a build).
//!
//! Spec 01 §Rights: what the tests print never shows content of the user's files. `assert_eq!`
//! prints both values when it fails, so values read from those files (text, bytes, table rows,
//! records, frames) are compared with [`assert_same!`] instead: its failure names the label,
//! whether the lengths differ and the first index where the two differ, never the values (nor
//! the lengths: the length of a list of the user's records or paths is a count of them).

/// `assert_eq!` for sequences that hold content of the user's files:
/// `assert_same!(left, right, "label {}", arg)`.
///
/// Both sides are anything `AsRef<[T]>` with the same element type (`str`, `String`, `Vec<T>`,
/// arrays, slices; a scalar goes in as `[x]`). The label is required and takes `format!`
/// arguments. On failure it panics with `<label>: lengths differ, first difference at index
/// <i>` or `<label>: lengths equal, first difference at index <i>` (for a prefix, `i` is where
/// the shorter one ends).
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
    let at = l
        .iter()
        .zip(r)
        .position(|(a, b)| a != b)
        .unwrap_or(l.len().min(r.len()));
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

    #[test]
    fn a_prefix_differs_at_the_shorter_length() {
        let m = failure(|| assert_same!(vec![1u8, 2], [1, 2, 3], "tail")).unwrap();
        assert_eq!(m, "tail: lengths differ, first difference at index 2");
        assert!(!m.contains('3'), "the longer length is not shown");
    }

    #[test]
    fn nested_sequences_differ_at_the_outer_index() {
        let a = vec![vec![1u8, 2], vec![3]];
        let b = vec![vec![1u8, 2], vec![4]];
        let m = failure(|| assert_same!(a, b, "blocks")).unwrap();
        assert_eq!(m, "blocks: lengths equal, first difference at index 1");
    }
}
