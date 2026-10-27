//! Spec 23's independent baseline on real data: with the message history never scrolled back,
//! three conversations near the start, on the boot's paper, show the shown, published and HD
//! frames of the tree without the feature. The digests below were recorded by this same file at
//! 60f92c6 (the parent commit, before spec 23 existed), so the oracle shares none of the
//! feature's code. Skips where the zip is missing; prints digests and actor slots only, never
//! the dialogue (FNV-1a 64, no pixels).

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

/// The digests of the parent's run ([`digests`]): the shown frame, the published frame, the HD
/// frames at 2× and 4×, each over every tick of the three conversations, and the ticks.
#[cfg(not(feature = "wide"))]
const PARENT: [u64; 5] = [
    0x8d81_d607_43ef_7a2d,
    0x8d81_d607_43ef_7a2d,
    0x2878_9d3f_c18c_1feb,
    0x4198_2404_a5a4_5b0b,
    54,
];
#[cfg(feature = "wide")]
const PARENT: [u64; 5] = [
    0xd7f0_9957_71f2_8d35,
    0xd7f0_9957_71f2_8d35,
    0x25c8_3248_327c_1585,
    0x5010_c5f8_d810_72d0,
    54,
];

/// FNV-1a 64 of each word into `h`.
fn mix(h: &mut u64, words: impl IntoIterator<Item = u64>) {
    for w in words {
        *h ^= w;
        *h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// A frame's colours and its sources, each pixel two words.
fn frame_words(fb: &Framebuffer) -> impl Iterator<Item = u64> + '_ {
    let src = fb.src();
    fb.pixels().iter().enumerate().flat_map(move |(i, &c)| {
        let s = src.map_or(Src::CLASSIC, |p| p[i]);
        let a = (u64::from(c) << 32) | (u64::from(s.kind as u8) << 16) | u64::from(s.flags);
        let b = (u64::from(s.ver) << 32) | (u64::from(s.u) << 16) | u64::from(s.v);
        [a, (u64::from(s.id) << 48) ^ b]
    })
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

/// The digests of the three conversations ([`PARENT`]'s order); `None` without the zip.
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
        let mut hd: Vec<HdView> = [Scale::X2, Scale::X4]
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
                mix(&mut out[2 + k], [u64::from(wrote)]);
                if wrote {
                    mix(&mut out[2 + k], v.rgba().iter().map(|&w| u64::from(w)));
                }
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
fn with_no_scroll_three_conversations_show_the_parents_frames() {
    let Some(got) = digests() else { return };
    println!("{got:#018x?}");
    assert_eq!(got, PARENT, "shown, published, HD 2×, HD 4×, ticks");
}
