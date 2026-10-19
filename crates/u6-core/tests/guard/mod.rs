//! Shared by the repository guards (`no_*_in_repo.rs`): the string tokens of a file that starts as
//! JSON but does not parse, for the format names a guard looks for.
//!
//! A full parse can fail on a file that still holds one of the tool's outputs (malformed after
//! it, or nested past the parser's depth), and the format name may be spelled with JSON escapes
//! (`\/`, `u`), so a byte search for it is not enough. [`holds_string`] walks the bytes once,
//! without recursion: each `"…"` token is read with its escapes decoded (`\" \\ \/ \b \f \n \r
//! \t` and `\uXXXX`, a surrogate pair joined), and its value compared with the names. Only a
//! token's first bytes up to the longest name are kept, so the work is bounded by the file's
//! length and the memory by the names'.

/// Whether a string token of `bytes`, its escapes decoded, equals one of `names`.
pub fn holds_string(bytes: &[u8], names: &[&str]) -> bool {
    let longest = names.iter().map(|n| n.len()).max().unwrap_or(0);
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'"' {
            i += 1;
            continue;
        }
        // One token: its decoded value (up to `longest + 1` bytes, enough to tell a longer one).
        let mut value: Vec<u8> = Vec::new();
        let keep = |v: &mut Vec<u8>, b: &[u8]| {
            if v.len() <= longest {
                v.extend_from_slice(b);
            }
        };
        let mut broken = false;
        i += 1;
        while i < bytes.len() && bytes[i] != b'"' {
            if bytes[i] != b'\\' {
                keep(&mut value, &bytes[i..=i]);
                i += 1;
                continue;
            }
            let code = match bytes.get(i + 1) {
                Some(b'"') => Some(u32::from(b'"')),
                Some(b'\\') => Some(u32::from(b'\\')),
                Some(b'/') => Some(u32::from(b'/')),
                Some(b'b') => Some(0x08),
                Some(b'f') => Some(0x0c),
                Some(b'n') => Some(u32::from(b'\n')),
                Some(b'r') => Some(u32::from(b'\r')),
                Some(b't') => Some(u32::from(b'\t')),
                Some(b'u') => {
                    let unit = hex4(bytes, i + 2);
                    let low = (bytes.get(i + 6..i + 8) == Some(b"\\u"))
                        .then(|| hex4(bytes, i + 8))
                        .flatten();
                    match (unit, low) {
                        (Some(h @ 0xd800..=0xdbff), Some(l @ 0xdc00..=0xdfff)) => {
                            i += 10;
                            Some(0x10000 + ((h - 0xd800) << 10) + (l - 0xdc00))
                        }
                        (Some(u), _) => {
                            i += 4;
                            Some(u)
                        }
                        (None, _) => None,
                    }
                }
                _ => None,
            };
            match code.and_then(char::from_u32) {
                Some(c) => keep(&mut value, c.encode_utf8(&mut [0; 4]).as_bytes()),
                // A lone surrogate or an escape JSON does not have: no name.
                None => broken = true,
            }
            i += 2;
        }
        i += 1;
        if !broken && names.iter().any(|n| value == n.as_bytes()) {
            return true;
        }
    }
    false
}

/// The four hex digits at `at`, as a number.
fn hex4(bytes: &[u8], at: usize) -> Option<u32> {
    let digits = std::str::from_utf8(bytes.get(at..at + 4)?).ok()?;
    if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(digits, 16).ok()
}
