//! Port-only (spec 07 HD0): the images the source plane names, as the engine holds them now, for
//! the HD compositor ([`u6_render::hd::Compositor`]).
//!
//! - A tile: its record's pixels and transparency ([`crate::gfx::GfxState::records`]) at the
//!   record's version now ([`crate::gfx::cache::RenderCache::version`]). Spec 18: an id from
//!   [`crate::gfx::cache::FROZEN_BASE`] on names a copy the remembered sight keeps of a merged
//!   shore record's pixels ([`crate::gfx::cache::RenderCache::frozen_image`]), version 0, which
//!   a pack keys as that record ([`Images::pack_id`]).
//! - A portrait and a panel bitmap: decoded as [`crate::converse::draw_portrait`],
//!   [`crate::main_loop::draw_paper`] and [`crate::magic::draw_book`] decode them, through the
//!   bitmap colour table as `blit_bitmap` draws them, opaque, version 0. [`ImageCache`] keeps
//!   each with what it was decoded from (the file name and the colour table), so a composition
//!   decodes it again only when one of those differs.
//! - Each one's content key (spec 07 HD1, asked only with a pack set): `u6_hdpack`'s content
//!   hash of the classic image as the engine holds it now, the same function the HD tool keys
//!   the pack with, so an image whose bytes changed (a merged shore record) matches no pack image
//!   and is shown by MMPX: a tile's record pixels and transparency ([`u6_hdpack::tile_hash`]); a
//!   portrait's bytes as `portrait_pixels` reads them ([`u6_hdpack::portrait_hash`]) and a
//!   bitmap's as the file holds them ([`u6_hdpack::bitmap_hash`]), both before the colour table.
//!
//! [`HdView`] is the HD frame a platform shows of its engine: the switch, the scale, the pack
//! ([`HdPack`], spec 07 H2), the HD font mode (spec 11) and when to compose (the web page's
//! `Game::set_hd`, `Game::set_hd_pack`, `Game::set_hd_font` and `Game::present` are it). It is
//! composed from the published frame, without the pointer sprite (spec 11 Design 7).

use std::rc::Rc;

use u6_formats::gfx::{TILE_H, TILE_W};
use u6_formats::portrait::{PORTRAIT_HEIGHT, PORTRAIT_WIDTH};
use u6_hdpack::{ImageId, Role, bitmap_hash, portrait_hash, tile_hash};
use u6_render::hd::{
    Compositor, HdGlyph, Image, Images, NoImages, PACK_SCALE, Pack, PackHd, Scale,
};
use u6_render::{BITMAP_PAPER, BITMAP_SPELLBOOK, Framebuffer, Kind, portrait_id, portrait_of};

use crate::converse::Msg;
use crate::engine::{Engine, Io};
use crate::state::State;

/// Port-only (spec 07 HD0): the HD frame of an engine's published frame, on or off, at 2× or
/// 4×. On, [`HdView::present`] composes it when the screen frame ([`Engine::screen_version`]:
/// the published frame, spec 23's history view and spec 17's menu over it, and the view's stamp)
/// or the setting changed since the frame it holds; off, it does nothing (the pack included:
/// nothing of it runs). Spec 11 Design 7: the published frame is the screen without the pointer
/// sprite ([`Engine::published_frame`]), so the HD frame and its glyph list are the screen as the
/// game drew it, and a move of the sprite alone composes nothing; the platform draws the sprite
/// over the HD frame ([`Engine::sprite`]). Spec 17 Presentation and spec 23 Decision 12: the
/// frame composed is the screen frame ([`Engine::screen_frame`]), the published frame with the
/// message history the player scrolled back to over it and the context menu over that, the one
/// composition the shown frame is made from too.
pub struct HdView {
    on: bool,
    scale: Scale,
    /// Moves at every change of `on`, `scale`, the pack or the font mode.
    version: u64,
    compositor: Compositor,
    images: ImageCache,
    /// What the frame holds: the screen frame's version and the message history's view stamp
    /// ([`Engine::screen_version`], [`Engine::view_version`]; `None` for the black frame before a
    /// start) and `version`; `None` before the first composition and after a start.
    holds: Option<(Option<(u64, u64)>, u64)>,
}

impl Default for HdView {
    fn default() -> Self {
        Self::new()
    }
}

impl HdView {
    /// Off, at 2×.
    pub fn new() -> Self {
        Self {
            on: false,
            scale: Scale::X2,
            version: 0,
            compositor: Compositor::new(),
            images: ImageCache::new(),
            holds: None,
        }
    }

    /// Switches the HD frame on or off and sets its scale; true when either changed (the next
    /// [`HdView::present`] composes, when on).
    pub fn set(&mut self, on: bool, scale: Scale) -> bool {
        if (on, scale) == (self.on, self.scale) {
            return false;
        }
        self.on = on;
        self.scale = scale;
        self.version += 1;
        true
    }

    pub fn on(&self) -> bool {
        self.on
    }

    /// Moves at every change of the switch, the scale, the pack or the font mode: what the
    /// images of one composition are made under.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Spec 07 H2: the pack whose images go in front of MMPX (`None`: none). The setting moves,
    /// so the next [`HdView::present`] composes (when on), exactly as a new view with this pack
    /// would.
    pub fn set_pack(&mut self, pack: Option<Rc<HdPack>>) {
        self.compositor.set_pack(pack.map(|p| p as Rc<dyn Pack>));
        self.version += 1;
    }

    /// Whether a pack is set.
    pub fn has_pack(&self) -> bool {
        self.compositor.has_pack()
    }

    /// Spec 11: the HD font mode on (`hd`) or off ([`Compositor::set_font_mode`]); true when it
    /// changed (the setting moves, so the next [`HdView::present`] composes, when on).
    pub fn set_font(&mut self, hd: bool) -> bool {
        if self.compositor.font_mode() == hd {
            return false;
        }
        self.compositor.set_font_mode(hd);
        self.version += 1;
        true
    }

    /// Whether the HD font mode is on ([`HdView::set_font`]).
    pub fn font(&self) -> bool {
        self.compositor.font_mode()
    }

    /// Spec 11: the cells the HD frame lists for the HD font, its letters and syllables and the
    /// whole spaces between them ([`Compositor::glyphs`]): those of
    /// its last composition while HD and the HD font mode are on, else none (the frame shown then
    /// is not one this view composed with the mode).
    pub fn glyphs(&self) -> &[HdGlyph] {
        if self.on && self.compositor.font_mode() {
            self.compositor.glyphs()
        } else {
            &[]
        }
    }

    pub fn scale(&self) -> Scale {
        self.scale
    }

    /// A new engine (a start): its versions start again, so its first frame is composed whole,
    /// and its images are decoded again.
    pub fn restart(&mut self) {
        self.holds = None;
        self.compositor.forget();
        self.images = ImageCache::new();
    }

    /// With HD on: composes the screen frame of `engine` ([`Engine::screen_frame`]: the published
    /// frame, the message history while the player scrolls it back and the context menu, without
    /// the pointer sprite; `blank` before a start) when the HD frame does not hold it at the
    /// setting now. Returns whether a block of the HD frame changed.
    pub fn present(&mut self, engine: Option<&Engine>, blank: &Framebuffer) -> bool {
        if !self.on {
            return false;
        }
        let key = (
            engine.map(|e| (e.screen_version(), e.view_version().1)),
            self.version,
        );
        if self.holds == Some(key) {
            return false;
        }
        let wrote = match engine {
            Some(e) => {
                let state = e.state();
                let mut images = self.images.images(&state, e.io());
                self.compositor
                    .compose(e.screen_frame(), &mut images, self.scale)
            }
            None => self.compositor.compose(blank, &mut NoImages, self.scale),
        };
        self.holds = Some(key);
        wrote
    }

    /// Port-only (spec 20 Design 5): composes a layer of a slide of `engine`'s map
    /// ([`crate::gfx::motion::slide_layers`]) at the scale set, in the published frame's palette,
    /// from the engine's images ([`Compositor::compose_layer`]) into `out`; nothing (`out` empty)
    /// with HD off.
    pub fn compose_layer(
        &mut self,
        engine: &Engine,
        layer: &u6_render::motion::LayerPixels,
        out: &mut Vec<u32>,
    ) {
        out.clear();
        if !self.on {
            return;
        }
        let state = engine.state();
        let mut images = self.images.images(&state, engine.io());
        let palette = engine.published_frame().palette;
        self.compositor
            .compose_layer(layer, &palette, &mut images, self.scale, out);
    }

    /// The HD frame: `WIDTH·S × HEIGHT·S` packed RGBA words (`u6_render::hd`), the last
    /// composition's (empty before the first).
    pub fn rgba(&self) -> &[u32] {
        self.compositor.rgba()
    }

    /// The rect the compositions since the last call wrote, in screen pixels
    /// ([`Compositor::take_dirty`]).
    pub fn take_dirty(&mut self) -> Option<[usize; 4]> {
        self.compositor.take_dirty()
    }

    /// The compositor (its cache's counts).
    pub fn compositor(&self) -> &Compositor {
        &self.compositor
    }
}

/// A portrait or panel bitmap decoded: what from, its width, height and screen colours and its
/// content key (`None`: nothing to show, a missing or bad file).
struct Decoded {
    kind: Kind,
    id: u16,
    from: (Vec<u8>, [u8; 256]),
    image: Option<(usize, usize, Vec<u8>, u64)>,
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

    /// Portrait or bitmap (kind, id) decoded now, with its content key (of its bytes before the
    /// colour table).
    fn decode(&self, kind: Kind, id: u16) -> Option<(usize, usize, Vec<u8>, u64)> {
        let colours = &self.state.gfx.tables.bitmap_colours;
        match kind {
            Kind::Portrait => {
                let (letter, item) = portrait_of(id);
                let files = self.io.files();
                let pixels =
                    crate::converse::portrait_pixels(self.state, files, letter, u16::from(item))?;
                let key = portrait_hash(&pixels)?;
                Some((
                    PORTRAIT_WIDTH,
                    PORTRAIT_HEIGHT,
                    through(colours, &pixels),
                    key,
                ))
            }
            Kind::Bitmap => {
                let bmp = match id {
                    BITMAP_PAPER => crate::main_loop::paper_bitmap(self.io),
                    _ => crate::magic::book_bitmap(self.io),
                }?;
                let (w, h) = (usize::from(bmp.width), usize::from(bmp.height));
                // A bitmap shorter than its header draws only its first pixels: none is shown HD.
                let key = bitmap_hash(bmp.width, bmp.height, &bmp.pixels)?;
                Some((w, h, through(colours, &bmp.pixels[..w * h]), key))
            }
            Kind::Classic | Kind::Tile | Kind::Glyph => None,
        }
    }
}

impl Images for EngineImages<'_> {
    fn image(&mut self, kind: Kind, id: u16) -> Option<Image<'_>> {
        if kind == Kind::Tile {
            if let Some((pixels, transparent)) = self.state.gfx.cache.frozen_image(id) {
                return Some(Image {
                    ver: 0,
                    width: TILE_W,
                    height: TILE_H,
                    pixels,
                    transparent: Some(transparent),
                });
            }
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
        let (width, height, pixels, _) = self.cache.decoded[at].image.as_ref()?;
        Some(Image {
            ver: 0,
            width: *width,
            height: *height,
            pixels,
            transparent: None,
        })
    }

    /// Spec 18: a copy the remembered sight keeps ([`crate::gfx::cache::FROZEN_BASE`] on) is the
    /// pack's image of the record it was taken from.
    fn pack_id(&self, kind: Kind, id: u16) -> (Kind, u16) {
        match self.state.gfx.cache.frozen_record(id) {
            Some(record) if kind == Kind::Tile => (Kind::Tile, record),
            _ => (kind, id),
        }
    }

    /// The content key of (kind, id) as [`EngineImages::image`] just gave it.
    fn key(&mut self, kind: Kind, id: u16) -> Option<u64> {
        if kind == Kind::Tile {
            if let Some((pixels, transparent)) = self.state.gfx.cache.frozen_image(id) {
                return tile_hash(pixels, transparent);
            }
            let rec = self.state.gfx.records.get(usize::from(id))?;
            return tile_hash(&rec.pixels, &rec.transparent);
        }
        let d = self
            .cache
            .decoded
            .iter()
            .find(|d| d.kind == kind && d.id == id)?;
        d.image.as_ref().map(|i| i.3)
    }
}

/// The content key image (kind, id) has in `engine`'s game now ([`Images::key`], as the
/// compositor asks it); `None` when it has none.
pub fn image_key(engine: &Engine, kind: Kind, id: u16) -> Option<u64> {
    let state = engine.state();
    let mut cache = ImageCache::new();
    let mut images = cache.images(&state, engine.io());
    images.image(kind, id)?;
    images.key(kind, id)
}

/// Spec 07 H2: a pack the platform loaded (`u6_hdpack::read_pack`: checked whole, every image
/// decoded), as the compositor looks its images up ([`Pack`]): by the image (kind, id) the source
/// plane names and the content key it has now, which must be the one the pack's image was made
/// from.
pub struct HdPack {
    pack: u6_hdpack::Pack,
    /// The index in `pack.images` of each image, by tile record and by (kind, id) for the others.
    tiles: Vec<Option<u32>>,
    others: Vec<((Kind, u16), u32)>,
}

/// What a pack holds and what of it a game matches, by kind: tiles, portraits, panel bitmaps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PackCounts {
    pub tiles: usize,
    pub portraits: usize,
    pub bitmaps: usize,
}

impl HdPack {
    pub fn new(pack: u6_hdpack::Pack) -> HdPack {
        let mut tiles = Vec::new();
        let mut others = Vec::new();
        for (i, p) in pack.images.iter().enumerate() {
            match plane_id(p.id) {
                Some((Kind::Tile, r)) => {
                    let r = usize::from(r);
                    if tiles.len() <= r {
                        tiles.resize(r + 1, None);
                    }
                    tiles[r] = Some(i as u32);
                }
                Some(at) => others.push((at, i as u32)),
                None => {}
            }
        }
        HdPack {
            pack,
            tiles,
            others,
        }
    }

    /// The pack image of the plane's (kind, id), whatever its key, and its index.
    fn get(&self, kind: Kind, id: u16) -> Option<(u32, &u6_hdpack::PackImage)> {
        let index = match kind {
            Kind::Tile => self.tiles.get(usize::from(id)).copied().flatten(),
            _ => self
                .others
                .iter()
                .find(|(at, _)| *at == (kind, id))
                .map(|&(_, i)| i),
        }?;
        Some((index, &self.pack.images[index as usize]))
    }

    /// The images the pack holds, by kind.
    pub fn counts(&self) -> PackCounts {
        let mut c = PackCounts::default();
        for p in &self.pack.images {
            match p.id {
                ImageId::Tile(_) => c.tiles += 1,
                ImageId::Portrait { .. } => c.portraits += 1,
                ImageId::Bitmap(_) => c.bitmaps += 1,
            }
        }
        c
    }

    /// The RGBA bytes the decoded images hold.
    pub fn decoded_bytes(&self) -> usize {
        self.pack.images.iter().map(|p| p.image.pixels.len()).sum()
    }

    /// The pack's images that `engine`'s game would show now, by kind: those whose content key
    /// is the one its image of that (kind, id) has now (looked up as the compositor looks them
    /// up). A portrait the plane cannot name (an item past 255) is not shown, so not counted.
    pub fn matches(&self, engine: &Engine) -> PackCounts {
        let state = engine.state();
        let mut cache = ImageCache::new();
        let mut images = cache.images(&state, engine.io());
        let mut c = PackCounts::default();
        for p in &self.pack.images {
            let Some((kind, id)) = plane_id(p.id) else {
                continue;
            };
            let size = images.image(kind, id).map(|i| (i.width, i.height));
            let shown = size.zip(images.key(kind, id)).is_some_and(|((w, h), key)| {
                self.hd(kind, id, key)
                    .is_some_and(|a| (a.width, a.height) == (w * PACK_SCALE, h * PACK_SCALE))
            });
            let count = match kind {
                Kind::Tile => &mut c.tiles,
                Kind::Portrait => &mut c.portraits,
                _ => &mut c.bitmaps,
            };
            *count += usize::from(shown);
        }
        c
    }
}

/// The (kind, id) a source plane names pack image `id` by; `None` for a portrait it cannot name
/// (an item past 255, which no slot reaches).
fn plane_id(id: ImageId) -> Option<(Kind, u16)> {
    match id {
        ImageId::Tile(r) => Some((Kind::Tile, r)),
        ImageId::Portrait { letter, item } => {
            portrait_id(letter, item).map(|p| (Kind::Portrait, p))
        }
        ImageId::Bitmap(Role::Paper) => Some((Kind::Bitmap, BITMAP_PAPER)),
        ImageId::Bitmap(Role::Spellbook) => Some((Kind::Bitmap, BITMAP_SPELLBOOK)),
    }
}

impl Pack for HdPack {
    fn hd(&self, kind: Kind, id: u16, key: u64) -> Option<PackHd<'_>> {
        let (index, p) = self.get(kind, id)?;
        (p.hash == key).then(|| PackHd {
            index,
            width: p.image.width as usize,
            height: p.image.height as usize,
            rgba: &p.image.pixels,
        })
    }
}
