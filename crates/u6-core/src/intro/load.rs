//! File loading of `u` (hunk 19, the dos.library layer replaced by [`Files`]).

use crate::files::Files;

use super::mem::{Chip, Ptr};

fn name_str(name: &[u8]) -> String {
    String::from_utf8_lossy(name).into_owned()
}

/// Original: u 0x11508a (load the U6-LZW file `name` into new memory; 0x10f72c returns it)
///
/// Replaces the open with disk prompt 0x10f596: a missing or undecodable file gives `None`
/// (the original asks for the disk until it is inserted).
pub fn load_lzw(files: &Files, name: &[u8]) -> Option<Vec<u8>> {
    let data = files.get(&name_str(name))?;
    u6_formats::lzw::decode(data).ok()
}

/// Original: u 0x115132 (load the U6-LZW file `name` decoded into the memory at `dest`)
///
/// The decoder 0x1151a8 writes the whole stream; the port clips it to the region. Returns false
/// when the file is missing or does not decode (nothing is written).
pub fn load_into(chip: &mut Chip, files: &Files, name: &[u8], dest: Ptr) -> bool {
    match load_lzw(files, name) {
        Some(bytes) => {
            chip.write(dest, &bytes);
            true
        }
        None => false,
    }
}

/// Original: u 0x10f548 (read the whole raw file `name` into new memory)
pub fn load_raw(files: &Files, name: &[u8]) -> Option<Vec<u8>> {
    files.get(&name_str(name)).map(<[u8]>::to_vec)
}
