//! Port-only (spec 07 HD0): the images the source plane names, as the engine holds them now, for
//! the HD compositor ([`u6_render::hd::Compositor`]).
//!
//! - A tile: its record's pixels and transparency ([`crate::gfx::GfxState::records`]) at the
//!   record's version now ([`crate::gfx::cache::RenderCache::version`]).
//! - A portrait and a panel bitmap: decoded as [`crate::converse::draw_portrait`],
//!   [`crate::main_loop::draw_paper`] and [`crate::magic::draw_book`] decode them, through the
//!   bitmap colour table as `blit_bitmap` draws them, opaque, version 0. [`ImageCache`] keeps
//!   each with what it was decoded from (the file name and the colour table), so a composition
//!   decodes it again only when one of those differs.

use u6_formats::gfx::{TILE_H, TILE_W};
use u6_formats::portrait::{PORTRAIT_HEIGHT, PORTRAIT_WIDTH};
use u6_render::hd::{Image, Images};
use u6_render::{BITMAP_PAPER, BITMAP_SPELLBOOK, Kind, portrait_of};

use crate::converse::Msg;
use crate::engine::Io;
use crate::state::State;

/// A portrait or panel bitmap decoded: what from, and its width, height and screen colours
/// (`None`: nothing to show, a missing or bad file).
struct Decoded {
    kind: Kind,
    id: u16,
    from: (Vec<u8>, [u8; 256]),
    image: Option<(usize, usize, Vec<u8>)>,
}

/// The portraits and panel bitmaps decoded so far (one per platform game).
#[derive(Default)]
pub struct ImageCache {
    decoded: Vec<Decoded>,
}

impl ImageCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// The images `state` and `io` hold now (an engine's [`crate::Engine::state`] and
    /// [`crate::Engine::io`] between two ticks), for one composition.
    pub fn images<'a>(&'a mut self, state: &'a State, io: &'a Io) -> EngineImages<'a> {
        EngineImages {
            state,
            io,
            cache: self,
        }
    }
}

/// [`Images`] of one game state between two ticks ([`ImageCache::images`]).
pub struct EngineImages<'a> {
    state: &'a State,
    io: &'a Io,
    cache: &'a mut ImageCache,
}

/// A bitmap's colours as `blit_bitmap` writes them: `colours[byte] & 0x1f`.
fn through(colours: &[u8; 256], pixels: &[u8]) -> Vec<u8> {
    pixels
        .iter()
        .map(|&b| colours[usize::from(b)] & 0x1f)
        .collect()
}

impl EngineImages<'_> {
    /// The file a portrait or a bitmap is decoded from, by name; `None` for no such image.
    fn source(&self, kind: Kind, id: u16) -> Option<Vec<u8>> {
        match (kind, id) {
            (Kind::Portrait, _) => Some(
                self.state
                    .converse
                    .text
                    .as_ref()
                    .map_or_else(Vec::new, |t| t.get(Msg::PortraitFile).to_vec()),
            ),
            (Kind::Bitmap, BITMAP_PAPER) => Some(crate::main_loop::paper_file(self.io).into()),
            (Kind::Bitmap, BITMAP_SPELLBOOK) => Some(crate::magic::book_file(self.io).into()),
            _ => None,
        }
    }

    /// Portrait or bitmap (kind, id) decoded now.
    fn decode(&self, kind: Kind, id: u16) -> Option<(usize, usize, Vec<u8>)> {
        let colours = &self.state.gfx.tables.bitmap_colours;
        match kind {
            Kind::Portrait => {
                let (letter, item) = portrait_of(id);
                let files = self.io.files();
                let pixels =
                    crate::converse::portrait_pixels(self.state, files, letter, u16::from(item))?;
                Some((PORTRAIT_WIDTH, PORTRAIT_HEIGHT, through(colours, &pixels)))
            }
            Kind::Bitmap => {
                let bmp = match id {
                    BITMAP_PAPER => crate::main_loop::paper_bitmap(self.io),
                    _ => crate::magic::book_bitmap(self.io),
                }?;
                let (w, h) = (usize::from(bmp.width), usize::from(bmp.height));
                // A bitmap shorter than its header draws only its first pixels: none is shown HD.
                (bmp.pixels.len() >= w * h).then(|| (w, h, through(colours, &bmp.pixels[..w * h])))
            }
            Kind::Classic | Kind::Tile => None,
        }
    }
}

impl Images for EngineImages<'_> {
    fn image(&mut self, kind: Kind, id: u16) -> Option<Image<'_>> {
        if kind == Kind::Tile {
            let i = usize::from(id);
            let rec = self.state.gfx.records.get(i)?;
            return Some(Image {
                ver: self.state.gfx.cache.version(i),
                width: TILE_W,
                height: TILE_H,
                pixels: &rec.pixels,
                transparent: Some(&rec.transparent),
            });
        }
        let from = (self.source(kind, id)?, self.state.gfx.tables.bitmap_colours);
        let at = self
            .cache
            .decoded
            .iter()
            .position(|d| d.kind == kind && d.id == id);
        let at = match at {
            Some(at) if self.cache.decoded[at].from == from => at,
            _ => {
                let image = self.decode(kind, id);
                let d = Decoded {
                    kind,
                    id,
                    from,
                    image,
                };
                match at {
                    Some(at) => {
                        self.cache.decoded[at] = d;
                        at
                    }
                    None => {
                        self.cache.decoded.push(d);
                        self.cache.decoded.len() - 1
                    }
                }
            }
        };
        let (width, height, pixels) = self.cache.decoded[at].image.as_ref()?;
        Some(Image {
            ver: 0,
            width: *width,
            height: *height,
            pixels,
            transparent: None,
        })
    }
}
