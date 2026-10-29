//! Original: end 0x102850 (passage layout and formatting), 0x1038d0 (composition).
//! Addresses and layout operations only: every word comes from the user's executable.
use super::Scene;
use crate::intro::{
    mem::{self, Ptr, Region, View},
    text::{self, Arg, TextCtx},
};

pub const STRINGS: &[u32] = &[
    0x102f88, 0x102fb2, 0x102fec, 0x103024, 0x10305c, 0x10308e, 0x1030ba, 0x1030f8, 0x103118,
    0x103150, 0x103186, 0x1031b8, 0x1031ee, 0x103226, 0x103252, 0x103282, 0x1032ac, 0x1032ca,
    0x103300, 0x10331c, 0x103350, 0x103386, 0x1033c0, 0x1033e4, 0x103424, 0x103430, 0x103458,
    0x10348c, 0x1034c0, 0x1034ea, 0x103520, 0x103558, 0x10359a, 0x1035d2, 0x10360a, 0x103646,
    0x10368c, 0x1036d2, 0x1036e6, 0x10371c, 0x103750, 0x103788, 0x1037b0, 0x1037c2, 0x1037f6,
    0x1037fa, 0x103800, 0x103806, 0x10380c, 0x103812,
];
const LINES: &[&[u32]] = &[
    &[0x102f88],
    &[0x102fb2, 0x102fec, 0x103024, 0x10305c],
    &[0x10308e, 0x1030ba],
    &[0x1030f8],
    &[0x103118, 0x103150, 0x103186, 0x1031b8, 0x1031ee],
    &[0x103226, 0x103252, 0x103282],
    &[0x1032ac],
    &[0x1032ca, 0x103300],
    &[0x10331c, 0x103350, 0x103386, 0x1033c0, 0x1033e4, 0x103424],
    &[0x103430],
    &[0x103458],
    &[0x10348c, 0x1034c0, 0x1034ea],
    &[0x103520, 0x103558, 0x10359a, 0x1035d2],
    &[0x10360a, 0x103646, 0x10368c, 0x1036d2],
    &[0x1036e6, 0x10371c, 0x103750, 0x103788],
    &[0x1037b0, 0x1037c2, 0x1037f6],
];
#[derive(Clone, Copy)]
pub(super) struct Layout {
    pub x: i16,
    pub y: i16,
    pub item: u16,
    pub text_y: i16,
    pub bottom: i16,
    pub centred: bool,
}
fn layout(n: u16) -> Layout {
    let (x, y, item, text_y, bottom) = match n {
        0 | 3 | 6 | 9 => (0, 160, 11, 174, 15),
        1 | 12 | 13 | 14 => (1, 133, 10, 146, 39),
        2 | 5 | 11 => (1, 151, 12, 163, 23),
        4 | 8 => (1, 133, 10, 142, 47),
        7 | 10 => (0, 152, 11, 162, 15),
        _ => (1, 68, 10, 77, 47),
    };
    Layout {
        x,
        y,
        item,
        text_y,
        bottom,
        centred: matches!(n, 0 | 3 | 6 | 9 | 10 | 15),
    }
}
impl Scene {
    pub(super) fn elapsed_parts(&self) -> [Vec<u8>; 3] {
        // game 0x13522a..0x13524a and ultcli 0x100492/0x1004e6 share argv buffers:
        // argv[2] = days, argv[3] = months, argv[4] = years. End formats argv in reverse.
        [
            self.assets.duration(self.elapsed.years, 0x103848, true),
            self.assets.duration(self.elapsed.months, 0x103850, true),
            self.assets.duration(self.elapsed.days, 0x103858, false),
        ]
    }
    pub(super) fn passage(&mut self, n: u16) {
        self.layout = layout(n);
        let view = View {
            x1: 303,
            y1: self.layout.bottom,
            ..View::screen(Ptr::at(Region::Spare))
        };
        mem::fill_view(&mut self.chip, &view, 31);
        let mut ctx = TextCtx {
            view,
            fg: 0,
            bg: 31,
            align: if self.layout.centred { 2 } else { 3 },
            ..Default::default()
        };
        for &addr in LINES[usize::from(n)] {
            let args = if n == 3 || n == 9 {
                vec![Arg::Word(127)]
            } else {
                Vec::new()
            };
            let fmt = self.assets.string(addr);
            text::print_more(&mut self.chip, &mut ctx, &self.assets.font, &fmt, &args);
        }
        if n == 15 {
            let parts = self.elapsed_parts();
            for (i, addr) in [(0, 0x1037fa), (1, 0x103800)] {
                if !parts[i].is_empty() {
                    text::print_more(
                        &mut self.chip,
                        &mut ctx,
                        &self.assets.font,
                        &self.assets.string(addr),
                        &[Arg::Str(parts[i].clone())],
                    );
                }
            }
            if !parts[0].is_empty() || !parts[1].is_empty() {
                text::print_more(
                    &mut self.chip,
                    &mut ctx,
                    &self.assets.font,
                    &self.assets.string(0x103806),
                    &[],
                );
            }
            text::print_more(
                &mut self.chip,
                &mut ctx,
                &self.assets.font,
                &self.assets.string(0x10380c),
                &[Arg::Str(parts[2].clone())],
            );
            text::print_more(
                &mut self.chip,
                &mut ctx,
                &self.assets.font,
                &self.assets.string(0x103812),
                &[],
            );
        }
        let buf = ctx.buf.clone();
        text::wrap(&mut self.chip, &mut ctx, &self.assets.font, &buf);
        self.passage = n;
    }
    pub(super) fn compose(&mut self) {
        let l = self.layout;
        self.item(l.item, l.x, l.y, 199);
        let source = View {
            x1: 303,
            y1: l.bottom,
            ..View::screen(Ptr::at(Region::Spare))
        };
        let dest = View {
            x0: 8,
            y0: l.text_y,
            ..View::screen(Ptr::at(Region::Work))
        };
        mem::copy_view(&mut self.chip, &source, &dest);
    }
}
