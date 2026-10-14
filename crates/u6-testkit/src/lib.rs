//! Test-only helpers shared by the workspace's tests (a dev-dependency; nothing here is part of
//! a build).
//!
//! Spec 01 §Rights: what the tests print never shows content of the user's files. `assert_eq!`
//! prints both values when it fails, so values read from those files (text, bytes, table rows,
//! records, frames) are compared with [`assert_same!`] instead: its failure names the label,
//! both lengths and the first index where the two differ, never the values.

/// `assert_eq!` for sequences that hold content of the user's files:
/// `assert_same!(left, right, "label {}", arg)`.
///
/// Both sides are anything `AsRef<[T]>` with the same element type (`str`, `String`, `Vec<T>`,
/// arrays, slices; a scalar goes in as `[x]`). The label is required and takes `format!`
/// arguments. On failure it panics with `<label>: lengths <a> and <b>, first difference at
/// index <i>` (for a prefix, `i` is the shorter length).
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
    panic!(
        "{}: lengths {} and {}, first difference at index {at}",
        label(),
        l.len(),
        r.len()
    );
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

    /// The message holds the label, both lengths and the index; none of the values (the
    /// distinctive letters, their numbers, any `Debug` brackets).
    #[test]
    fn a_mismatch_names_the_label_lengths_and_index_only() {
        let left = b"SECRET".to_vec();
        let right = b"SECRXT";
        let m = failure(|| assert_same!(left, right, "record {}", 3)).expect("a failure");
        assert_eq!(m, "record 3: lengths 6 and 6, first difference at index 4");
        for leak in ["SECR", "83", "88", "[", "\""] {
            assert!(!m.contains(leak), "the message shows {leak:?}");
        }
        let m = failure(|| assert_same!("Zhuge", String::from("Zhugx"), "name")).unwrap();
        assert_eq!(m, "name: lengths 5 and 5, first difference at index 4");
        assert!(!m.contains("Zhug"));
    }

    #[test]
    fn a_prefix_differs_at_the_shorter_length() {
        let m = failure(|| assert_same!(vec![1u8, 2], [1, 2, 3], "tail")).unwrap();
        assert_eq!(m, "tail: lengths 2 and 3, first difference at index 2");
    }

    #[test]
    fn nested_sequences_differ_at_the_outer_index() {
        let a = vec![vec![1u8, 2], vec![3]];
        let b = vec![vec![1u8, 2], vec![4]];
        let m = failure(|| assert_same!(a, b, "blocks")).unwrap();
        assert_eq!(m, "blocks: lengths 2 and 2, first difference at index 1");
    }
}
