//! Restore 0x1052c8 and load state 0x105350.

use std::cell::RefCell;

use u6_formats::basetile::BaseTiles;
use u6_formats::schedule::Schedules as ScheduleFile;

use super::save::MODE_PLAY;
use super::{OBJLIST_FILE, SAVE_LAYER, SIGHT_FILE, SavedSight, objlist, seams};
use crate::actors::Schedules;
use crate::engine::Io;
use crate::files::Files;
use crate::gfx::cache::SightMemory;
use crate::state::{LoadError, State};

/// Combat-mode byte of the A4-0x7b84 block (A4-0x7b0c).
const TAIL_COMBAT: usize = 0x78;
/// Panel mode 0x1052c8 selects (A4-0x7a30): the party roster.
const PANEL_ROSTER: u16 = 0x91;
/// No command underlined (0x106506's argument).
const NO_COMMAND: u8 = 0xff;

fn parse<T>(
    files: &Files,
    name: &str,
    parse: impl FnOnce(&[u8]) -> u6_formats::Result<T>,
) -> Result<T, LoadError> {
    let bytes = files.get(name).ok_or_else(|| LoadError::Missing {
        file: name.to_string(),
    })?;
    parse(bytes).map_err(|error| LoadError::Format {
        file: name.to_string(),
        error,
    })
}

/// `savegame/objlist` as the boot reads it: missing, or shorter than the 0x1C73 bytes the
/// game reads, is an error (the original would play on with whatever memory held).
pub fn boot_objlist(files: &Files) -> Result<&[u8], LoadError> {
    let bytes = files.get(OBJLIST_FILE).ok_or_else(|| LoadError::Missing {
        file: OBJLIST_FILE.to_string(),
    })?;
    if bytes.len() < objlist::OBJLIST_LEN {
        return Err(LoadError::Table {
            file: OBJLIST_FILE.to_string(),
            detail: format!(
                "{} bytes, the game reads {}",
                bytes.len(),
                objlist::OBJLIST_LEN
            ),
        });
    }
    Ok(bytes)
}

/// The three files 0x105350 reads, parsed before anything changes.
pub struct StateFiles {
    schedule: ScheduleFile,
    basetile: BaseTiles,
    objlist: Vec<u8>,
}

/// 0x105350's reads: `schedule` (0x202 bytes, then 5 per entry), `basetile` (0x800) and
/// `savegame/objlist`: the one the last Save wrote, else the platform's file. The remembered sight
/// saved with that objlist is read by [`load_state`] (spec 18).
pub fn read_state_files(s: &State, files: &Files) -> Result<StateFiles, LoadError> {
    let schedule = parse(files, "schedule", ScheduleFile::parse)?;
    let basetile = parse(files, "basetile", BaseTiles::parse)?;
    let objlist = match &s.saveload.objlist {
        Some(saved) => saved.clone(),
        None => boot_objlist(files)?.to_vec(),
    };
    Ok(StateFiles {
        schedule,
        basetile,
        objlist,
    })
}

/// Port-only (spec 18, saved with the game): the remembered sight saved with `objlist`, the
/// objlist load state reads. The file goes with the objlist: once this session wrote the objlist
/// (a Save, or the character `u`), the one the last Save wrote ([`super::SaveLoad::sight`]; none
/// after a Save that wrote none, and none after `u`, which clears it), else the platform's
/// [`SIGHT_FILE`] in the save's layer ([`SAVE_LAYER`], the files [`super::attach`] laid over): a
/// file of its name in another layer (the game files, a mod) is not the save's. Read only when it
/// was written with these objlist bytes and is well formed ([`crate::gfx::cache::read_sight`]);
/// otherwise none, and the memory stays empty.
fn saved_sight(s: &State, files: &Files, objlist: &[u8]) -> Option<SightMemory> {
    let file = match &s.saveload.objlist {
        Some(_) => s.saveload.sight.as_ref().map(SavedSight::bytes),
        None => files.get_in(SAVE_LAYER, SIGHT_FILE).map(|(_, bytes)| bytes),
    };
    crate::gfx::cache::read_sight(file?, objlist).ok()
}

/// Port-only (spec 18, saved with the game): load state's read of the remembered sight at the
/// boot (`game_main`, before the tail of 0x105350 it runs): the sight saved with the objlist the
/// boot read ([`State::new_game`]'s, or `u`'s character this run: [`saved_sight`]). A boot
/// without one keeps the empty memory it starts with.
pub fn boot_sight(s: &mut State, files: &Files) {
    let objlist = match &s.saveload.objlist {
        Some(saved) => Some(saved.as_slice()),
        None => boot_objlist(files).ok(),
    };
    if let Some(m) = objlist.and_then(|o| saved_sight(s, files, o)) {
        s.gfx.cache.set_sight(m);
    }
}

/// Original: 0x105350 (saveload: load state: schedule, base tiles, objlist; place the party)
///
/// The window-cache key cleared (A4-0x7958 .. -0x7954 = -1); the schedules, base tiles and the
/// 25 objlist tables ([`objlist::decode`]) from `files`; the quest-flag copy and the view
/// centre on the leader, the animation flags (both in `State`); then the actors linked
/// (0x10edb4), the last command underlined (0x106506), the region's objects and map (0x10e1ce,
/// 0x108ac4), A4-0x784e = 1, the window cache (0x108f90), the members back into the vehicle
/// when aboard (0x10cda8) and the view flags (0x109e74) ([`crate::main_loop::finish_load_state`],
/// which the boot runs too). Port (spec 18): the remembered sight is forgotten
/// ([`crate::gfx::cache::RenderCache::forget_sight`]), and the one saved with this objlist read
/// right after ([`saved_sight`]: the file [`read_state_files`]'s objlist goes with, as the save
/// state still names it), so the old memory is let go before the new one is made (Bounds).
/// No slide crosses the load (spec 20): its source copies belonged to the prior sight.
pub fn load_state(s: &mut State, io: &Io, files: StateFiles) {
    s.map.window_key = [-1; 3];
    s.gfx.cache.forget_sight();
    io.break_motion();
    if let Some(m) = saved_sight(s, io.files(), &files.objlist) {
        s.gfx.cache.set_sight(m);
    }
    s.actors.schedules = Schedules {
        first: files.schedule.first,
        total: files.schedule.records.len() as u16,
        entries: files.schedule.records,
    };
    s.tables.basetile = files.basetile;
    objlist::decode(s, &files.objlist);
    s.place_party();
    s.init_anim_flags();
    crate::main_loop::finish_load_state(s, io);
}

/// Original: 0x1052c8 (saveload: Restore: drop the unsaved blocks, read the save again)
///
/// A4-0x7afb = 0, A4-0x7af8 = 1, cursor mode off (A4-0x7a60), the roster panel for member 0
/// redrawn (A4-0x7a30 = 0x91, A4-0x7a3e = 0, A4-0x7afe = 1), save mode play; the `.tmp`
/// files deleted (0x10d4e8) and their flags cleared, the slots reset (0x10ed44), the chunk
/// cache emptied (0x108f66), no command underlined (0x106506(0xff)); [`load_state`]; then the
/// after-load fixes (0x1217d4), the area song (0x110912), combat begun or broken off by the
/// saved combat byte (0x121c64 / 0x121d78), the clock at +0 minutes (0x10368a) and the prompt
/// reprinted (A4-0x7afd).
///
/// The files are read and parsed first: when one is unreadable nothing changes and the error
/// is returned (the original has no such case; its disks are always there). 0x105350 does not
/// write the save mode, so the region's block reads in [`load_state`] run in play mode.
pub fn restore(s: &mut State, io: &Io) -> Result<(), LoadError> {
    let files = read_state_files(s, io.files())?;
    s.map.flag_7afb = 0;
    s.gfx.window_shown = 1;
    // The same word (A4-0x7af8) as objects keeps it.
    s.objects.saved_blocks = 1;
    s.ui.input.targeting = 0;
    s.ui.panel.mode = PANEL_ROSTER;
    s.ui.panel.member = 0;
    s.ui.panel.dirty = true;
    // Spec 14 Design 3 (Wide): the inventory view's kept bag and page, and its slot's cells,
    // name slots of the game before; the next draw starts them again.
    #[cfg(feature = "wide")]
    {
        s.ui.panel.slots = Default::default();
    }
    s.objects.save_mode = MODE_PLAY;
    crate::objects::delete_tmp_files(&mut s.objects);
    s.objects.tmp_flag.fill(0);
    crate::objects::init_slots(&mut s.objects);
    s.map.reset_chunk_cache();
    crate::ui::cursor::select_command(s, io, NO_COMMAND);
    s.ui.panel.dirty = true;
    load_state(s, io, files);
    crate::actors::turn::after_load(s);
    seams::play_area_song(s);
    if s.actors.tail(TAIL_COMBAT) != 0 {
        seams::begin_combat(s);
    } else {
        seams::break_off_combat(s);
    }
    crate::actors::clock::tick(s, 0);
    s.actors.work.prompt = 1;
    s.saveload.restores = s.saveload.restores.wrapping_add(1);
    Ok(())
}

/// The Restore command's handler as the command loop calls it (control-R after the question's
/// Y, and the loop's end when A4-0x637a is 'R'). An unreadable save changes nothing.
pub async fn restore_game(state: &RefCell<State>, io: &Io) {
    let _ = restore(&mut state.borrow_mut(), io);
}
