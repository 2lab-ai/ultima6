//! Unchanged row spans must still observe the images they reference. Synthetic pixels only.

use super::*;

fn check(
    compositor: &mut Compositor,
    frame: &Framebuffer,
    pics: &mut Pics,
    scale: Scale,
    dirty: Option<[usize; 4]>,
    asks: usize,
) {
    let asked = pics.asked;
    assert_eq!(compositor.compose(frame, pics, scale), dirty.is_some());
    assert_eq!(compositor.take_dirty(), dirty, "exact changed input bounds");
    assert_eq!(pics.asked - asked, asks, "one query per referenced image");
    let mut fresh = Compositor::new();
    fresh.set_pack(compositor.pack.clone());
    fresh.compose(frame, &mut pics.clone(), scale);
    assert!(compositor.rgba() == fresh.rgba(), "fresh RGBA oracle");
    assert_eq!(
        compositor.glyphs(),
        fresh.glyphs(),
        "diagnostics stay current"
    );
    assert!(compositor.cache().bytes() <= compositor.cache().cap());
}

#[test]
fn consecutive_spans_observe_missing_version_and_shape_changes_without_frame_writes() {
    for scale in [Scale::X2, Scale::X4] {
        let mut pics = Pics::default();
        pics.put(Kind::Tile, 7, (48, 8), art(87, 48, 8, 5), None);
        let mut frame = plane();
        frame.palette = palette(7);
        draw(&mut frame, &pics, Kind::Tile, 7, (32, 16));
        let image = pics.images.remove(0);
        let mut c = Compositor::new();
        let region = Some([32, 16, 48, 8]);
        check(
            &mut c,
            &frame,
            &mut pics,
            scale,
            Some([0, 0, WIDTH, HEIGHT]),
            1,
        );
        check(&mut c, &frame, &mut pics, scale, None, 1);
        let missing = c.rgba().to_vec();
        pics.images.push(image);
        check(&mut c, &frame, &mut pics, scale, region, 1);
        assert!(
            c.rgba() != missing,
            "the fixture really shows HD image blocks"
        );
        check(&mut c, &frame, &mut pics, scale, None, 1);

        // Version alone invalidates the old drawing, even with the same HD token/content.
        pics.images[0].2 += 1;
        check(&mut c, &frame, &mut pics, scale, region, 1);
        assert!(c.rgba() == missing, "stale image pixels become classic");
        check(&mut c, &frame, &mut pics, scale, None, 1);
        pics.images[0].2 -= 1;
        check(&mut c, &frame, &mut pics, scale, region, 1);

        // Same pixel vector, different extent: the right half now lies outside the image.
        pics.images[0].3 = 24;
        pics.images[0].4 = 16;
        check(&mut c, &frame, &mut pics, scale, region, 1);
        let pixel = pics.images[0].5.pop().unwrap();
        check(&mut c, &frame, &mut pics, scale, Some([32, 16, 24, 8]), 1);
        check(&mut c, &frame, &mut pics, scale, None, 1);
        pics.images[0].5.push(pixel);
        check(&mut c, &frame, &mut pics, scale, Some([32, 16, 24, 8]), 1);
    }
}

#[test]
fn consecutive_clear_glyph_spans_follow_paper_content_pack_and_forget() {
    for scale in [Scale::X2, Scale::X4] {
        let mut pics = Pics::default();
        pics.put(Kind::Bitmap, BITMAP_PAPER, (64, 8), vec![1; 64 * 8], None);
        let key = key_of(&pics, Kind::Bitmap, BITMAP_PAPER);
        let mut pack = TestPack::default();
        pack.put(Kind::Bitmap, BITMAP_PAPER, key, (256, 32), 31);
        let mut frame = plane();
        frame.palette = palette(3);
        for y in 0..8 {
            for x in 0..64 {
                frame.set_src(
                    x,
                    y,
                    1,
                    Src {
                        kind: Kind::Glyph,
                        id: SPACE,
                        u: (x % 8) as u16,
                        v: y as u16,
                        flags: FLAG_PAPER,
                        ..Src::CLASSIC
                    },
                );
            }
        }
        let mut c = Compositor::new();
        c.set_pack(Some(Rc::new(pack.clone())));
        let region = Some([0, 0, 64, 8]);
        check(
            &mut c,
            &frame,
            &mut pics,
            scale,
            Some([0, 0, WIDTH, HEIGHT]),
            1,
        );
        check(&mut c, &frame, &mut pics, scale, None, 1);
        assert_eq!(c.glyphs().len(), 8);
        pics.images[0].5.fill(2);
        check(&mut c, &frame, &mut pics, scale, region, 1);
        check(&mut c, &frame, &mut pics, scale, None, 1);
        pics.images[0].5.fill(1);
        check(&mut c, &frame, &mut pics, scale, region, 1);
        let image = pics.images.remove(0);
        check(&mut c, &frame, &mut pics, scale, region, 1);
        check(&mut c, &frame, &mut pics, scale, None, 1);
        pics.images.push(image);
        check(&mut c, &frame, &mut pics, scale, region, 1);
        pack.put(Kind::Bitmap, BITMAP_PAPER, key, (256, 32), 32);
        c.set_pack(Some(Rc::new(pack)));
        check(&mut c, &frame, &mut pics, scale, region, 1);
        c.forget();
        check(
            &mut c,
            &frame,
            &mut pics,
            scale,
            Some([0, 0, WIDTH, HEIGHT]),
            1,
        );
        frame.palette[1] = Rgb::new(100, 200, 10);
        check(
            &mut c,
            &frame,
            &mut pics,
            scale,
            Some([0, 0, WIDTH, HEIGHT]),
            1,
        );
        c.set_pack(None);
        check(&mut c, &frame, &mut pics, scale, region, 1);
    }
}

#[test]
fn mixed_spans_and_evicted_images_keep_their_pixels_and_exact_dirty_bounds() {
    for cap in [HD_CACHE_BYTES, 1] {
        let mut pics = Pics::default();
        for id in [1, 2] {
            pics.put(Kind::Tile, id, (16, 8), art(u64::from(id), 16, 8, 5), None);
        }
        let mut frame = plane();
        frame.palette = palette(11);
        draw(&mut frame, &pics, Kind::Tile, 1, (32, 16));
        draw(&mut frame, &pics, Kind::Tile, 1, (48, 16));
        draw(&mut frame, &pics, Kind::Tile, 2, (64, 16));
        // Two image dependencies within the first 16-pixel span.
        for y in 0..8 {
            for x in 0..8 {
                frame.set_src(
                    40 + x,
                    16 + y,
                    pics.images[1].5[y * 16 + x],
                    tile(2, 0, (x, y), 0),
                );
            }
        }
        let mut c = Compositor::with_cap(cap);
        let all = Some([32, 16, 48, 8]);
        let unchanged = if cap == 1 { all } else { None };
        check(
            &mut c,
            &frame,
            &mut pics,
            Scale::X2,
            Some([0, 0, WIDTH, HEIGHT]),
            2,
        );
        check(&mut c, &frame, &mut pics, Scale::X2, unchanged, 2);
        check(&mut c, &frame, &mut pics, Scale::X2, unchanged, 2);
        pics.images[1].5 = art(77, 16, 8, 5);
        let changed = if cap == 1 { all } else { Some([40, 16, 40, 8]) };
        check(&mut c, &frame, &mut pics, Scale::X2, changed, 2);
        check(
            &mut c,
            &frame,
            &mut pics,
            Scale::X4,
            Some([0, 0, WIDTH, HEIGHT]),
            2,
        );
    }
}

#[test]
fn a_plane_free_frame_reuses_only_identical_classic_sources() {
    let mut pics = Pics::default();
    pics.put(Kind::Tile, 7, (48, 8), art(9, 48, 8, 5), None);
    let mut frame = plane();
    frame.palette = palette(9);
    draw(&mut frame, &pics, Kind::Tile, 7, (32, 16));
    let mut c = Compositor::new();
    check(
        &mut c,
        &frame,
        &mut pics,
        Scale::X2,
        Some([0, 0, WIDTH, HEIGHT]),
        1,
    );
    let mut classic = Framebuffer::new();
    classic.palette = frame.palette;
    classic.blit(0, 0, WIDTH, HEIGHT, frame.pixels(), None);
    assert!(classic.src().is_none());
    check(
        &mut c,
        &classic,
        &mut pics,
        Scale::X2,
        Some([32, 16, 48, 8]),
        0,
    );
    check(&mut c, &classic, &mut pics, Scale::X2, None, 0);
    frame.reset_src();
    frame.set_src(
        5,
        5,
        0,
        Src {
            id: 77,
            ..Src::CLASSIC
        },
    );
    check(&mut c, &frame, &mut pics, Scale::X2, Some([5, 5, 1, 1]), 0);
    check(
        &mut c,
        &classic,
        &mut pics,
        Scale::X2,
        Some([5, 5, 1, 1]),
        0,
    );
    check(&mut c, &classic, &mut pics, Scale::X2, None, 0);
}
