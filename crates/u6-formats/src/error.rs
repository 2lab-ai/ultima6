//! Parse error type shared by every parser in this crate.

use std::fmt;

/// Error returned by every parser. Each variant names the field being read (`what`) and the byte
/// offset into the parser's input where the problem was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    /// The input ended before `needed` bytes could be read at `offset`.
    Truncated {
        what: &'static str,
        offset: usize,
        needed: usize,
        len: usize,
    },
    /// The input has a length the format does not allow.
    BadLength {
        what: &'static str,
        expected: String,
        actual: usize,
    },
    /// A field at `offset` holds a value the format does not allow.
    Invalid {
        what: &'static str,
        offset: usize,
        detail: String,
    },
    /// Item `index` of a container (stored at `offset` in the container file) failed to parse;
    /// `source` carries offsets relative to the item.
    InItem {
        what: &'static str,
        index: usize,
        offset: usize,
        source: Box<FormatError>,
    },
}

impl FormatError {
    pub(crate) fn invalid(what: &'static str, offset: usize, detail: impl Into<String>) -> Self {
        FormatError::Invalid {
            what,
            offset,
            detail: detail.into(),
        }
    }

    pub(crate) fn in_item(
        what: &'static str,
        index: usize,
        offset: usize,
        source: FormatError,
    ) -> Self {
        FormatError::InItem {
            what,
            index,
            offset,
            source: Box::new(source),
        }
    }

    pub(crate) fn bad_length(
        what: &'static str,
        expected: impl Into<String>,
        actual: usize,
    ) -> Self {
        FormatError::BadLength {
            what,
            expected: expected.into(),
            actual,
        }
    }
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormatError::Truncated {
                what,
                offset,
                needed,
                len,
            } => write!(
                f,
                "{what}: truncated at offset {offset:#x} (need {needed} bytes, input is {len})"
            ),
            FormatError::BadLength {
                what,
                expected,
                actual,
            } => write!(f, "{what}: length {actual} bytes, expected {expected}"),
            FormatError::Invalid {
                what,
                offset,
                detail,
            } => write!(f, "{what}: invalid at offset {offset:#x}: {detail}"),
            FormatError::InItem {
                what,
                index,
                offset,
                source,
            } => write!(f, "{what} item {index} (at {offset:#x}): {source}"),
        }
    }
}

impl std::error::Error for FormatError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FormatError::InItem { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}

/// Result alias used by every parser.
pub type Result<T> = std::result::Result<T, FormatError>;
