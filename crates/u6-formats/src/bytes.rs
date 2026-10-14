//! Bounds-checked little/big-endian readers. Every parser reads through these so that a short
//! input becomes a [`FormatError::Truncated`] with the field name and offset, never a panic.

use crate::error::{FormatError, Result};

/// Returns `data[offset..offset + n]` or a `Truncated` error naming `what`.
pub(crate) fn slice<'a>(
    data: &'a [u8],
    offset: usize,
    n: usize,
    what: &'static str,
) -> Result<&'a [u8]> {
    offset
        .checked_add(n)
        .and_then(|end| data.get(offset..end))
        .ok_or(FormatError::Truncated {
            what,
            offset,
            needed: n,
            len: data.len(),
        })
}

pub(crate) fn u16_be(data: &[u8], offset: usize, what: &'static str) -> Result<u16> {
    let b = slice(data, offset, 2, what)?;
    Ok(u16::from_be_bytes([b[0], b[1]]))
}

pub(crate) fn u16_le(data: &[u8], offset: usize, what: &'static str) -> Result<u16> {
    let b = slice(data, offset, 2, what)?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

pub(crate) fn u32_le(data: &[u8], offset: usize, what: &'static str) -> Result<u32> {
    let b = slice(data, offset, 4, what)?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Fails with `BadLength` unless `data.len() == expected`.
pub(crate) fn expect_len(data: &[u8], expected: usize, what: &'static str) -> Result<()> {
    if data.len() == expected {
        Ok(())
    } else {
        Err(FormatError::bad_length(
            what,
            expected.to_string(),
            data.len(),
        ))
    }
}

/// Decodes bytes up to the first NUL (or the end) as Latin-1, one char per byte, so that every
/// byte value survives the conversion.
pub(crate) fn latin1_until_nul(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| char::from(b))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readers_check_bounds() {
        let d = [0x12, 0x34, 0x56];
        assert_eq!(u16_be(&d, 0, "t"), Ok(0x1234));
        assert_eq!(u16_le(&d, 1, "t"), Ok(0x5634));
        assert_eq!(
            u16_be(&d, 2, "t"),
            Err(FormatError::Truncated {
                what: "t",
                offset: 2,
                needed: 2,
                len: 3
            })
        );
        assert!(u32_le(&d, 0, "t").is_err());
        assert!(slice(&d, usize::MAX, 2, "t").is_err());
    }

    #[test]
    fn latin1_stops_at_nul() {
        assert_eq!(latin1_until_nul(b"Avatar\0\0x"), "Avatar");
        assert_eq!(latin1_until_nul(&[0x41, 0xE9]), "A\u{e9}");
    }
}
