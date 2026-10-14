//! Test-only encoders (ports of `web/src/intake/__tests__/*`), so every decoder
//! stage round-trips without game data. Each test binary uses a subset.
#![allow(dead_code)]

pub mod mfm_encoder;
pub mod ofs_builder;
pub mod pp20_encoder;
pub mod zip_writer;

pub fn text(s: &str) -> Vec<u8> {
    s.as_bytes().to_vec()
}
