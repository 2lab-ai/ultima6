//! Real data probe.
#![allow(clippy::disallowed_methods)]

use std::path::Path;

use u6_intake::{InputFile, intake};

use crate::files::Files;
use crate::state::State;

const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";

fn files() -> Option<Files> {
    if !Path::new(ZIP).exists() {
        return None;
    }
    let data = std::fs::read(ZIP).unwrap();
    let r = intake(vec![InputFile {
        name: "ultima-vi.zip".into(),
        data,
    }]);
    Some(Files::new(r.files))
}

#[test]
fn probe() {
    let Some(files) = files() else { return };
    let mut s = State::new_game(&files).unwrap();
    crate::main_loop::load_state_tail(&mut s);
    let c = s.actors.calendar;
    eprintln!("time {}:{} day {} month {} light {}", c.hour, c.minute, c.day, c.month, s.actors.objlist_tail[0x0b]);
    let p = &s.actors.party;
    eprintln!("party {:?} size {} leader {}", &p.members[..p.size as usize], p.size, p.leader_index);
    for a in p.members.iter().take(p.size as usize) {
        let a = *a as usize;
        eprintln!("actor {a} lvl {} mp {} hp {} int {} status {:#x}", s.actors.attrs.level[a], s.actors.attrs.magic[a], s.actors.attrs.hp[a], s.actors.attrs.intelligence[a], s.actors.attrs.status[a]);
        let mut n = s.objects.next_of(a as i16);
        while n >= 0 && s.objects.obj_status[n as usize] & 0x18 != 0 {
            let par = s.objects.parent(n);
            eprintln!("  slot {n} kind {:#x} st {:#x} qq {:#06x} parent {par}", s.objects.kind(n), s.objects.obj_status[n as usize], s.objects.qq[n as usize]);
            n = s.objects.next_of(n);
        }
    }
}
