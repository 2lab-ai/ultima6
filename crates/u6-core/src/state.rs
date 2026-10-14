//! `State`: every game global, as one aggregate of typed domain parts (spec 03 §2).
//!
//! Ownership: this file owns the aggregate and the load orchestration; each domain type lives in
//! its module directory (`objects/`, `actors/`, `map/`, `ui/`, `converse/`) and is extended
//! there. Static tables loaded at boot sit in [`Tables`]. Save/load serializes `State`.

use std::fmt;

use u6_formats::FormatError;
use u6_formats::animdata::AnimData;
use u6_formats::basetile::BaseTiles;
use u6_formats::chunks::Chunks;
use u6_formats::gfx::{self, PALETTE_LEN, Rgb};
use u6_formats::look::LookTable;
use u6_formats::map::WorldMap;
use u6_formats::objblk::{ObjBlock, ObjBlockId};
use u6_formats::objlist::ObjList;
use u6_formats::schedule::Schedules as ScheduleFile;
use u6_formats::tileflag::TileFlags;

use crate::actors::{Actors, Calendar, GameTables, Party, Schedules};
use crate::converse::{ConverseState, ConverseText};
use crate::files::Files;
use crate::gfx::GfxState;
use crate::main_loop::MainState;
use crate::map::MapState;
use crate::objects::{ACTOR_SLOTS, Objects};
use crate::rng::Rng;
use crate::ui::UiState;

/// objlist offset of the 0x82-byte block 0x105350 reads to A4-0x7b84 (quest flag .. 0x1C73).
const OBJLIST_TAIL: usize = 0x1BF1;
const OBJLIST_TAIL_LEN: usize = 0x82;
/// objlist offset of the leader index (A4-0x7b7a).
const OBJLIST_LEADER_INDEX: usize = 0x1BFB;
/// objlist offsets of arrays u6-formats leaves undecoded.
const OBJLIST_UNKNOWN_18F1: usize = 0x18F1;
const OBJLIST_UNKNOWN_1AF1: usize = 0x1AF1;
/// Object types whose base tile starts with its animation off (the words at 0x13ab8a, 0x13ab80
/// and 0x13ab18 = basetile table 0x13a940 + 2 × type).
const ANIM_OFF_TYPES: [usize; 3] = [0x125, 0x120, 0xec];

/// Static tables loaded at boot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tables {
    /// Original: 0x13a940 (word per object type: base tile; `basetile`)
    pub basetile: BaseTiles,
    /// Original: 0x13b140 flags1 / 0x13b940 flags2 / 0x13a540 weight / 0x13c140 flags3
    /// (`tileflag`, read by 0x134f08)
    pub tileflags: TileFlags,
    /// Original: A4+0x5dca count, A4+0x5dcc tiles, A4+0x5e0c first frames, A4+0x5e4c masks,
    /// A4+0x5e6c shifts (`animdata`, read by 0x134f08)
    pub anim: AnimData,
    /// Original: `look.lzd`, read on demand by 0x10ee76 (the port keeps it decoded)
    pub look: LookTable,
    /// Original: 0x15812c (A4-0x35c4; 32 OCS colour words in the data hunk of `game`)
    pub palette: [Rgb; PALETTE_LEN],
}

impl Default for Tables {
    fn default() -> Self {
        Self {
            basetile: BaseTiles { base: Vec::new() },
            tileflags: TileFlags {
                flags1: Vec::new(),
                flags2: Vec::new(),
                weight: Vec::new(),
                flags3: Vec::new(),
            },
            anim: AnimData {
                count: 0,
                slots: Vec::new(),
            },
            look: LookTable {
                entries: Vec::new(),
                trailer: Vec::new(),
            },
            palette: [Rgb::default(); PALETTE_LEN],
        }
    }
}

/// All game state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub tables: Tables,
    pub objects: Objects,
    pub actors: Actors,
    pub map: MapState,
    pub ui: UiState,
    pub converse: ConverseState,
    /// World-view graphics: tile records, the animation tables and the display globals.
    pub gfx: GfxState,
    /// Globals of the main hunk (command loop, boot).
    pub main: MainState,
    /// Original: 0x151ee0 (RNG state longword)
    pub rng: Rng,
    pub combat: crate::combat::CombatState,
    pub objcmds: crate::objects::commands::ObjCommands,
}

/// A game file is missing or malformed.
#[derive(Debug)]
pub enum LoadError {
    Missing {
        file: String,
    },
    Format {
        file: String,
        error: FormatError,
    },
    /// A table read out of a file failed its checks (the reader's own message).
    Table {
        file: String,
        detail: String,
    },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Missing { file } => write!(f, "{file}: not found in the dropped files"),
            LoadError::Format { file, error } => write!(f, "{file}: {error}"),
            LoadError::Table { file, detail } => write!(f, "{file}: {detail}"),
        }
    }
}

impl std::error::Error for LoadError {}

fn load<T>(
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

/// Original: 0x134f08 (load animdata / tileflag tables) [partial]
///
/// The file reads only; the memory setup at its start is the platform's.
fn load_boot_tables(files: &Files) -> Result<(AnimData, TileFlags), LoadError> {
    let anim = load(files, "animdata", AnimData::parse)?;
    let tileflags = load(files, "tileflag", TileFlags::parse)?;
    Ok((anim, tileflags))
}

impl State {
    /// A state with no world loaded (tests and tools).
    pub fn empty() -> Self {
        Self::default()
    }

    /// Original: 0x105350 (load state: objlist, schedules, base tiles; place party) [partial]
    ///
    /// The boot loads first: tile graphics (0x152e50, [`GfxState::load`]), boot step 3
    /// (0x134f08), the font `u6.ch` and the tables the modules read out of `game` (ui, map,
    /// objects, actors, converse; spec 03 §5). Then 0x105350's reads in the original order:
    /// `schedule`, `basetile`, `savegame/objlist` (25 reads of the 0x1C73 bytes the Amiga uses),
    /// the leader position and the animation flags. The port also loads whole files the original
    /// reads later per region or on demand (`map`, `chunks`, the 69 `savegame/objblk??`,
    /// `look.lzd`, the palette inside `game`). 0x105350's remaining calls (0x10edb4, 0x106506,
    /// the region loads 0x10e1ce / 0x108ac4, 0x108f90, 0x10cda8, 0x109e74) run from `main_loop`
    /// ([`crate::main_loop::finish_load_state`]). The RNG keeps its unseeded state; the engine
    /// seeds it.
    pub fn new_game(files: &Files) -> Result<State, LoadError> {
        let gfx = GfxState::load(files)?;
        let (anim, tileflags) = load_boot_tables(files)?;
        let schedule = load(files, "schedule", ScheduleFile::parse)?;
        let basetile = load(files, "basetile", BaseTiles::parse)?;
        let objlist = load(files, "savegame/objlist", ObjList::parse)?;
        let world = load(files, "map", WorldMap::parse)?;
        let chunks = load(files, "chunks", Chunks::parse)?;
        let blocks = ObjBlockId::all()
            .map(|id| {
                let name = format!("savegame/{}", id.file_name().to_ascii_lowercase());
                load(files, &name, ObjBlock::parse).map(|block| (id, block))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let look = load(files, "look.lzd", LookTable::from_lzd)?;
        let palette = load(files, "game", gfx::palette)?;
        let ui = UiState::load(files)?;
        let table = |detail: String| LoadError::Table {
            file: "game".into(),
            detail,
        };
        let game = files.get("game").ok_or_else(|| LoadError::Missing {
            file: "game".into(),
        })?;
        let actor_tables = GameTables::from_game(game).map_err(|error| LoadError::Format {
            file: "game".into(),
            error,
        })?;
        let converse_text = ConverseText::from_game(game).map_err(table)?;

        let mut state = State::empty();
        state.gfx = gfx;
        state.ui = ui;
        state.map.load_tables(files)?;
        state
            .objects
            .load_defs(files)
            .map_err(|e| table(e.to_string()))?;
        state.actors.game = actor_tables;
        state.converse.text = Some(converse_text);
        state.combat.load(game).map_err(|error| LoadError::Format {
            file: "game".into(),
            error,
        })?;
        state.objcmds = crate::objects::commands::ObjCommands::load(files).map_err(table)?;
        state.tables = Tables {
            basetile,
            tileflags,
            anim,
            look,
            palette,
        };
        state.objects.blocks = blocks;
        state.map.world = world;
        state.map.chunks = chunks;
        state.actors.schedules = Schedules {
            first: schedule.first,
            total: schedule.records.len() as u16,
            entries: schedule.records,
        };
        state.apply_objlist(&objlist);
        state.place_party();
        state.init_anim_flags();
        Ok(state)
    }

    /// Spreads `objlist` over the tables 0x105350 reads it into.
    fn apply_objlist(&mut self, list: &ObjList) {
        let raw = &list.raw;
        let (objects, actors) = (&mut self.objects, &mut self.actors);
        let attrs = &mut actors.attrs;
        for (a, r) in list.actors.iter().enumerate().take(ACTOR_SLOTS) {
            objects.obj_status[a] = r.obj_flags;
            objects.location[a] = r.pos;
            objects.type_frame[a] = (r.obj_n & 0x3ff) | u16::from(r.frame & 0x3f) << 10;
            objects.qq[a] = r.unknown_0600;
            attrs.status[a] = r.status;
            attrs.strength[a] = r.strength;
            attrs.dexterity[a] = r.dexterity;
            attrs.intelligence[a] = r.intelligence;
            attrs.exp[a] = r.exp;
            attrs.hp[a] = r.hp;
            attrs.level[a] = r.level;
            attrs.unknown_10f1[a] = r.unknown_10f1;
            attrs.combat_mode[a] = r.combat_mode;
            attrs.magic[a] = r.magic;
            attrs.base_type_frame[a] =
                (r.base_obj_n & 0x3ff) | u16::from(r.base_frame & 0x3f) << 10;
            attrs.talk_flags[a] = r.talk_flags;
            attrs.unknown_18f1[a] = raw[OBJLIST_UNKNOWN_18F1 + a];
            attrs.movement_flags[a] = r.movement_flags;
            attrs.unknown_1af1[a] = raw[OBJLIST_UNKNOWN_1AF1 + a];
            actors.movement_points[a] = r.movement_points;
            actors.worktype[a] = r.worktype;
        }
        actors.party = Party {
            names: list.party.names.clone(),
            members: list.party.roster,
            size: list.party.size,
            leader_index: raw[OBJLIST_LEADER_INDEX],
        };
        let c = list.clock;
        actors.calendar = Calendar {
            minute: c.minute,
            hour: c.hour,
            day: c.day,
            month: c.month,
            year: c.year,
        };
        actors.quest_flag = list.player.quest_flag;
        actors.rest_counter = list.player.rest_counter;
        actors.karma = list.player.karma;
        actors.wind = list.player.wind_direction;
        actors.timers = list.timers;
        actors.objlist_tail = raw[OBJLIST_TAIL..OBJLIST_TAIL + OBJLIST_TAIL_LEN].to_vec();
    }

    /// 0x105350 after the reads: copy the quest flag to A4-0x2fd2 and centre the view on the
    /// leader's location (x = 10 bits, y = 10 bits, z = 4 bits of the 3-byte entry).
    fn place_party(&mut self) {
        self.actors.quest_flag_copy = u16::from(self.actors.quest_flag);
        let leader = self.actors.party.leader();
        if let Some(pos) = leader.and_then(|id| self.objects.location(id)) {
            self.map.centre = pos;
        }
    }

    /// 0x105350's loop over the animdata slots (A4+0x5dca of them): a slot whose tile is the
    /// base tile of object type 0x125, 0x120 or 0xec starts disabled, every other slot enabled.
    fn init_anim_flags(&mut self) {
        let base = &self.tables.basetile.base;
        let off: Vec<u16> = ANIM_OFF_TYPES
            .iter()
            .filter_map(|&t| base.get(t).copied())
            .collect();
        let anim = &self.tables.anim;
        self.map.anim_enabled = anim
            .slots
            .iter()
            .take(usize::from(anim.count))
            .map(|slot| u8::from(!off.contains(&slot.tile_to_animate)))
            .collect();
    }

    /// Location of the actor the view follows.
    pub fn leader_position(&self) -> Option<u6_formats::position::Position> {
        self.objects.location(self.actors.party.leader()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use u6_formats::animdata::AnimEntry;
    use u6_formats::position::Position;

    #[test]
    fn place_party_centres_on_the_indexed_member() {
        let mut s = State::empty();
        s.objects.location[7] = Position {
            x: 300,
            y: 350,
            z: 0,
        };
        s.objects.location[9] = Position { x: 10, y: 20, z: 3 };
        s.actors.party.members[..2].copy_from_slice(&[7, 9]);
        s.actors.party.size = 2;
        s.actors.quest_flag = 0xab;
        s.place_party();
        assert_eq!(
            s.map.centre,
            Position {
                x: 300,
                y: 350,
                z: 0
            }
        );
        assert_eq!(s.actors.quest_flag_copy, 0xab);
        s.actors.party.leader_index = 1;
        s.place_party();
        assert_eq!(s.map.centre, Position { x: 10, y: 20, z: 3 });
        assert_eq!(s.leader_position(), Some(s.map.centre));
    }

    #[test]
    fn anim_flags_clear_slots_on_the_three_base_tiles() {
        let mut s = State::empty();
        let mut base = vec![0u16; 1024];
        base[0x125] = 500;
        base[0x120] = 600;
        base[0xec] = 700;
        s.tables.basetile = BaseTiles { base };
        let slot = |tile| AnimEntry {
            tile_to_animate: tile,
            ..AnimEntry::default()
        };
        s.tables.anim = AnimData {
            count: 4,
            slots: vec![slot(600), slot(1), slot(700), slot(500), slot(2)],
        };
        s.init_anim_flags();
        // Only the first `count` slots get a flag.
        assert_eq!(s.map.anim_enabled, [0, 1, 0, 0]);
    }

    #[test]
    fn missing_file_is_reported_by_name() {
        let err = State::new_game(&Files::default()).unwrap_err();
        assert_eq!(
            err.to_string(),
            "maptiles.ega: not found in the dropped files"
        );
    }
}
