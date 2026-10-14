//! Amiga `.ega` tile graphics — **not implemented in this module yet**.
//!
//! `maptiles.ega`, `objtiles.ega`, `tileindx.ega`, `masktype.ega` and `animmask.ega` differ from
//! the DOS `.vga` files Nuvie reads; their layout comes from the `game` loaders via REA and is
//! owned by a separate work unit (formats.md §8.1, "pending WU-4 (REA)"). `masktype.ega` is
//! whole-file U6-LZW (→ 2048 bytes), so [`crate::lzw::decode`] already unwraps it; the meaning
//! of its values (0..7 measured) is part of that unit.
