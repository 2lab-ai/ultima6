//! `book.dat` — formats.md §2 (U6Lib, u16 LE offsets; N: core/book.cpp:42-57,
//! usecode/u6_usecode.cpp:2415-2418). 128 items of NUL-terminated ASCII text, indexed by the
//! quality byte of signs, books, scrolls and tombstones (formats.md §5).

use crate::error::Result;
use crate::u6lib::{EntryWidth, U6Lib};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// The book texts.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Books {
    /// One entry per lib item (`None` = empty item). The text is the item decoded as Latin-1
    /// with one trailing NUL removed; any other byte, NUL included, is kept.
    pub texts: Vec<Option<String>>,
}

impl Books {
    /// Parses `book.dat`.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let lib = U6Lib::parse(data, EntryWidth::U16)?;
        let texts = lib
            .items
            .iter()
            .map(|it| {
                (!it.is_empty()).then(|| {
                    let body = it.data.strip_suffix(&[0]).unwrap_or(&it.data);
                    body.iter().map(|&b| char::from(b)).collect()
                })
            })
            .collect();
        Ok(Books { texts })
    }

    /// Text for book index `i` (the object's quality), if present.
    pub fn text(&self, i: usize) -> Option<&str> {
        self.texts.get(i)?.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_texts() {
        let mut d = vec![6, 0, 0, 0, 10, 0];
        d.extend_from_slice(b"Hi!\0Ok\0");
        let b = Books::parse(&d).unwrap();
        assert_eq!(b.texts, [Some("Hi!".into()), None, Some("Ok".into())]);
        assert_eq!(b.text(2), Some("Ok"));
        assert_eq!(b.text(1), None);
        assert_eq!(b.text(9), None);
        assert!(Books::parse(&[0, 0]).is_err());
    }
}
