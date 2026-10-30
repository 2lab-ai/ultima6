//! Conversation frame golden for the owner's 2026-10-29 choices update (spec 22).
//! The original spec 23 D2 hashes at 17c09b6 were first verified unchanged by the #7/#8-only
//! follow-up snapshot. The choice grid, compact portrait and restricted-answer row deliberately
//! change visible frames; these hashes were recorded after inspecting Original/Wide and Korean
//! private captures and validating click, page, used, reset, full preview and answer behavior.
//! The synthetic history baseline in ui/tests/scrollback_baseline.rs remains unchanged.
//! Each digest covers colour indices, sources, palette and original-pixel HD frames at 2×/4×.
//! The retired platform-font outputs are removed; all pixel-frame golden hashes are unchanged.
//! Only counts and digests are printed; no game text or images enter repository/logs.
//! Missing archive skips unless U6_REQUIRE_ARCHIVE=1, which fails closed.

use u6_render::hd::Scale;
use u6_render::{Framebuffer, Src};

use super::party::name_from_script;
use super::tests_real::{files, with_text};
use super::text::ConverseText;
use super::{Cx, Msg, talk};
use crate::engine::{Engine, InputEvent, Key};
use crate::hd::HdView;
use crate::objects::ObjectId;
use crate::state::State;
use u6_testkit::Quiet;

/// The visually checked choice UI digests ([`digests`]): the shown frame, the published frame, the HD
/// frames at 2× and 4× ([`HD`]), each over every tick of the three conversations, and the ticks.
#[cfg(not(feature = "wide"))]
const CHOICES: [u64; 5] = [
    0xca87841d744767c1,
    0xca87841d744767c1,
    0xd927294d4ed15cc3,
    0x9c6f11510ca4d641,
    54,
];
#[cfg(feature = "wide")]
const CHOICES: [u64; 5] = [
    0x6a7941ae8515ff99,
    0x6a7941ae8515ff99,
    0x7f52d73fd5108331,
    0xac7e4af1b202c7a9,
    54,
];

/// Pixel-font HD views. The removed platform-font branch is no longer a rendering option.
const HD: [Scale; 2] = [Scale::X2, Scale::X4];

/// FNV-1a 64 of each word into `h`.
fn mix(h: &mut u64, words: impl IntoIterator<Item = u64>) {
    for w in words {
        *h ^= w;
        *h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// A frame's colours and its sources, each pixel two words, then its palette, a word a colour.
fn frame_words(fb: &Framebuffer) -> impl Iterator<Item = u64> + '_ {
    let src = fb.src();
    let pixels = fb.pixels().iter().enumerate().flat_map(move |(i, &c)| {
        let s = src.map_or(Src::CLASSIC, |p| p[i]);
        let a = (u64::from(c) << 32) | (u64::from(s.kind as u8) << 16) | u64::from(s.flags);
        let b = (u64::from(s.ver) << 32) | (u64::from(s.u) << 16) | u64::from(s.v);
        [a, (u64::from(s.id) << 48) ^ b]
    });
    let palette = fb
        .palette
        .iter()
        .map(|c| (u64::from(c.r) << 16) | (u64::from(c.g) << 8) | u64::from(c.b));
    pixels.chain(palette)
}

/// What an HD view shows after a present that returned `wrote`: whether it wrote, its RGBA when it
/// did (else the same as before), with the original pixel-only golden format's zero marker.
fn hd_words(v: &HdView, wrote: bool) -> Vec<u64> {
    let mut out = vec![u64::from(wrote)];
    if wrote {
        out.extend(v.rgba().iter().map(|&w| u64::from(w)));
    }
    // Keep the original pixel-only golden format's zero metadata count. Glyph metadata
    // is now always available, but is independently covered by the glyph/plane tests.
    out.push(0);
    out
}

/// The scroll's lines so far, the one being written last (as the transcript test reads them).
fn lines_of(s: &State) -> Vec<String> {
    let t = &s.ui.text;
    let mut lines = s.ui.messages.clone();
    let mut last: String = t
        .transcript_line
        .iter()
        .map(|&b| crate::ui::wide::latin_char(b))
        .collect();
    let pending = &t.line[usize::from(t.line_start)..usize::from(t.line_len)];
    last.extend(pending.iter().map(|&b| crate::ui::wide::latin_char(b)));
    lines.push(last);
    lines
}

/// The actors on the leader's level, nearest first (party members and empty slots aside).
fn near(base: &State) -> Vec<i16> {
    let leader = base.leader_position().quiet();
    let members: Vec<u16> = base.actors.party.active().map(|id| id.0).collect();
    let mut near: Vec<(u32, i16)> = (2..0xe0i16)
        .filter(|a| !members.contains(&(*a as u16)))
        .filter_map(|a| {
            let p = base.objects.location(ObjectId(a as u16))?;
            if p.z != leader.z || base.objects.obj_type(ObjectId(a as u16)) == 0 {
                return None;
            }
            let d = (i32::from(p.x) - i32::from(leader.x)).unsigned_abs()
                + (i32::from(p.y) - i32::from(leader.y)).unsigned_abs();
            Some((d, a))
        })
        .collect();
    near.sort();
    near.into_iter().map(|(_, a)| a).collect()
}

/// Talk (0x1127c4) on `npc` over the boot's paper: at each prompt for the player's words the next
/// of name, job and bye typed, at any other wait Space, N, Y in turn; `each` sees the engine
/// after every tick.
fn converse(state: &State, files: &crate::files::Files, npc: i16, each: &mut impl FnMut(&Engine)) {
    let leader = state.actors.party.leader_index;
    let text: &ConverseText = state.converse.text.as_ref().quiet();
    let prompt: String = text
        .get(Msg::YouSay)
        .iter()
        .map(|&b| char::from(b))
        .collect();
    let prompt = prompt.trim().to_string();
    let mut e = Engine::with_game(state.clone(), files.clone(), move |st, io| async move {
        crate::main_loop::draw_paper(&mut st.borrow_mut(), &io);
        talk(&Cx::new(&st, &io), leader, npc, true).await;
    });
    e.enable_src_plane();
    let mut lines = ["name", "job", "bye"].iter();
    let (mut tries, mut seen) = (0, Vec::new());
    for _ in 0..200 {
        // Ticks without input until the game waits for a key with nothing queued.
        for _ in 0..32 {
            let before = lines_of(&e.state());
            e.tick(&[], 16);
            each(&e);
            if e.finished() || (e.io().pending_events() == 0 && lines_of(&e.state()) == before) {
                break;
            }
        }
        if e.finished() {
            break;
        }
        let now = lines_of(&e.state());
        if now != seen {
            tries = 0;
            seen = now;
        }
        let last = seen.last().cloned().unwrap_or_default();
        let input = if e.state().ui.input.line_input || last.trim_end().ends_with(&prompt) {
            let Some(line) = lines.next() else { break };
            tries = 0;
            let mut keys: Vec<InputEvent> = line
                .chars()
                .map(|c| InputEvent::Key(Key::Char(c)))
                .collect();
            keys.push(InputEvent::Key(Key::Enter));
            keys
        } else {
            tries += 1;
            vec![InputEvent::Key(Key::Char([' ', 'n', 'y'][(tries - 1) % 3]))]
        };
        e.tick(&input, 16);
        each(&e);
    }
}

/// The digests of the three conversations ([`CHOICES`]'s order); `None` without the zip.
fn digests() -> Option<[u64; 5]> {
    let files = files()?;
    let base = with_text(&files);
    let blank = Framebuffer::new();
    let mut out = [0xcbf2_9ce4_8422_2325u64; 5];
    let mut ticks = 0u64;
    let mut talked = 0;
    for npc in near(&base) {
        let mut s = base.clone();
        if name_from_script(&mut s, &files, npc).0.is_empty() {
            continue;
        }
        let mut hd: Vec<HdView> = HD
            .into_iter()
            .map(|scale| {
                let mut v = HdView::new();
                v.set(true, scale);
                v
            })
            .collect();
        converse(&base, &files, npc, &mut |e| {
            mix(&mut out[0], frame_words(e.frame()));
            mix(&mut out[1], frame_words(&e.published_frame()));
            for (k, v) in hd.iter_mut().enumerate() {
                let wrote = v.present(Some(e), &blank);
                mix(&mut out[2 + k], hd_words(v, wrote));
            }
            ticks += 1;
        });
        println!("actor slot {npc:#x} talked");
        talked += 1;
        if talked == 3 {
            break;
        }
    }
    assert_eq!(talked, 3, "three conversations");
    out[4] = ticks;
    Some(out)
}

#[test]
#[allow(clippy::disallowed_methods)] // a test may read the environment
fn with_no_scroll_three_conversations_show_choice_ui_frames() {
    let Some(got) = digests() else {
        let required = std::env::var("U6_REQUIRE_ARCHIVE").is_ok_and(|v| v == "1");
        assert!(
            !required,
            "U6_REQUIRE_ARCHIVE=1: the user's zip is required"
        );
        println!("converse baseline: skipped (no game files)");
        return;
    };
    println!("converse baseline: ran ({} ticks)", got[4]);
    println!("{got:#018x?}");
    assert_eq!(
        got, CHOICES,
        "shown, published, original-pixel HD 2×, original-pixel HD 4×, ticks"
    );
}
