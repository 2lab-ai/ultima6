//! The host's audio object: the MED routine on an emulated A500 audio machine, fed with the
//! game's calls and pulled for samples.
//!
//! Host loop (native or web): after each `Engine::tick`, `driver.sync(&engine.state().sound,
//! engine.io().files())`; from the audio callback (or a ring buffer it drains),
//! `driver.render(&mut buf, sample_rate)`. Rendering never touches the engine, so audio cannot
//! change the game; given the same calls at the same sample positions it is bit-reproducible.
//! One driver serves every program of a launcher run: the launcher marks each hand-over from
//! `u` to a new `game` process in the call log ([`MedCall::NewProcess`]; a relaunch's new State
//! goes on with the old one's log, [`super::SoundState::continue_calls`]), and the driver
//! starts the player over where it applies that mark.

use u6_audio::{FilterModel, Machine};

use super::med::MedPlayer;
use super::songs::MODULE_BUFFER;
use super::tables::{self, MedTables};
use super::{MedCall, SoundState};
use crate::files::Files;
use crate::image::GameImage;
use crate::state::LoadError;

/// Chip memory address of the module buffer (A4-0x35cc). 0..16 holds the null sample.
pub const CHIP_BUFFER: u32 = 0x10;
/// Chip memory address of the sound-effect module (hunk 28 of `game`).
pub const CHIP_EFFECT: u32 = CHIP_BUFFER + MODULE_BUFFER as u32;

/// The MED routine and the machine it plays on.
#[derive(Clone, Debug)]
pub struct AudioDriver {
    pub machine: Machine,
    pub player: MedPlayer,
    /// The sound-effect module as the image holds it (each new `game` process starts from it).
    effect: Vec<u8>,
    /// Number of the next game call to apply.
    applied: u64,
}

impl AudioDriver {
    /// The audio part of setup 0x14ef06: the effect module in chip memory, InitPlayer
    /// (0x100f7c) and the effect module's relocation (0x15355a; the port reads offsets).
    pub fn new(files: &Files) -> Result<Self, LoadError> {
        let game = files.get("game").ok_or_else(|| LoadError::Missing {
            file: "game".into(),
        })?;
        let format = |error| LoadError::Format {
            file: "game".into(),
            error,
        };
        let img = GameImage::parse(game).map_err(format)?;
        let tables = MedTables::from_image(&img).map_err(format)?;
        let effect = tables::effect_module(&img).map_err(format)?;
        Ok(Self::with_tables(tables, &effect))
    }

    /// A driver from already-read tables and effect module bytes (tests, tools).
    pub fn with_tables(tables: MedTables, effect: &[u8]) -> Self {
        let mut machine = Machine::new(CHIP_EFFECT as usize + effect.len());
        machine.chip[CHIP_EFFECT as usize..].copy_from_slice(effect);
        let mut player = MedPlayer::new(tables);
        player.init(&mut machine);
        Self {
            machine,
            player,
            effect: effect.to_vec(),
            applied: 0,
        }
    }

    /// Applies the game calls this driver has not seen yet, in order. Calls that dropped out of
    /// the log (more than [`super::CALL_LOG`] since the last sync) are lost. The numbering never
    /// starts over: a new `game` process goes on with the log ([`SoundState::continue_calls`]),
    /// and a host builds a new driver for a new engine (a log numbered from 0 again would have
    /// every call skipped here, which debug builds stop at).
    pub fn sync(&mut self, sound: &SoundState, files: &Files) {
        let from = self.applied;
        debug_assert!(
            sound.next_call >= from,
            "the sound log is numbered back to {} below the {from} calls this driver applied: \
             one driver serves one engine",
            sound.next_call
        );
        for (n, call) in &sound.calls {
            if *n >= from {
                self.apply(call, files);
            }
        }
        self.applied = sound.next_call;
    }

    /// Carries out one call.
    pub fn apply(&mut self, call: &MedCall, files: &Files) {
        match call {
            MedCall::Stop => self.player.stop(&mut self.machine),
            MedCall::Load(name) => {
                // A file the dropped disks lack leaves the buffer as it was (the original asks
                // for the disk).
                if let Some(bytes) = files.get(name) {
                    let n = bytes.len().min(MODULE_BUFFER);
                    let at = CHIP_BUFFER as usize;
                    self.machine.chip[at..at + n].copy_from_slice(&bytes[..n]);
                }
            }
            MedCall::Poke { offset, value } => {
                if (*offset as usize) < MODULE_BUFFER {
                    self.machine.chip[(CHIP_BUFFER + offset) as usize] = *value;
                }
            }
            MedCall::PlayBuffer => self.player.play(&mut self.machine, CHIP_BUFFER),
            MedCall::PlayEffect => self.player.play(&mut self.machine, CHIP_EFFECT),
            MedCall::NewProcess => self.new_process(),
        }
    }

    /// Fills `out` (interleaved stereo f32, −1..1) at `sample_rate`.
    pub fn render(&mut self, out: &mut [f32], sample_rate: u32) {
        self.machine.render(&mut self.player, out, sample_rate);
    }

    /// [`AudioDriver::render`] with each channel's DAC level (4 per frame) in `taps`.
    pub fn render_taps(&mut self, out: &mut [f32], taps: &mut [f32], sample_rate: u32) {
        self.machine
            .render_taps(&mut self.player, out, taps, sample_rate);
    }

    /// Output stage: A500 (default) or raw.
    pub fn set_filter(&mut self, model: FilterModel) {
        self.machine.filter = model;
    }

    /// RemPlayer (0x100f96), as the shutdown 0x14ee70 calls it.
    pub fn remove(&mut self) {
        self.player.remove(&mut self.machine);
    }

    /// [`MedCall::NewProcess`]: the end of one program (`u` or `game`) and the start of a new
    /// `game` process. The old one's exit (`u` 0x113b68, `game` 0x14ee70: its memory freed,
    /// RemPlayer) leaves nothing playing, the new one's setup (0x14ef06) sets the player up on
    /// its own memory: the effect module as the image holds it, a module buffer of its own,
    /// zero here (the original's is not cleared; see [`MedCall::NewProcess`]). The output stage
    /// stays the host's, the count of applied calls the log's.
    fn new_process(&mut self) {
        let (filter, applied) = (self.machine.filter, self.applied);
        let effect = std::mem::take(&mut self.effect);
        *self = Self::with_tables(self.player.tables.clone(), &effect);
        self.machine.filter = filter;
        self.applied = applied;
    }
}
