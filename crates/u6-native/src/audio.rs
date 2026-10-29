//! The native sound output: the game's MED calls replayed on the emulated A500 audio machine
//! (`u6_core::sound::AudioDriver`), in step with the engine.
//!
//! After every engine tick the host hands the driver the calls the tick made ([`Audio::sync`])
//! and renders samples: headless exactly the tick's length (so `--wav` is the boot's audio,
//! bit-reproducible), in the window enough to keep the device's ring [`AHEAD_MS`] ahead (the
//! device thread only drains the ring; it never touches the engine). Above [`LOUD_UP_TO`] the
//! window's sound fades out over [`FADE_MS`] and comes back the same way at [`LOUD_UP_TO`] or
//! less (spec 06 §5: the music plays in wall time, so at those speeds it no longer follows the
//! game).

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use u6_core::Engine;
use u6_core::pacer::Speed;
use u6_core::sound::AudioDriver;

/// The headless render rate.
pub const WAV_RATE: u32 = 44_100;
/// Audio the window keeps rendered ahead of the device.
pub const AHEAD_MS: u32 = 120;

/// The driver and the fraction of a sample frame the last render left over.
pub struct Audio {
    driver: AudioDriver,
    rate: u32,
    /// Sample frames owed but not rendered yet, in 1/1000 frames (tick lengths are whole ms).
    owed: u64,
}

impl Audio {
    pub fn new(engine: &Engine, rate: u32) -> Result<Self, String> {
        let driver = AudioDriver::new(engine.io().files()).map_err(|e| e.to_string())?;
        Ok(Self {
            driver,
            rate,
            owed: 0,
        })
    }

    /// The window's sound, or none (said on stderr) when the driver cannot be built from the
    /// user's `game` (its MED tables fail the checks): the game then runs silent instead of not
    /// at all.
    pub fn or_silent(engine: &Engine, rate: u32) -> Option<Self> {
        Self::new(engine, rate)
            .map_err(|e| eprintln!("audio: {e}; sound is off"))
            .ok()
    }

    /// Replays the calls the last tick made.
    pub fn sync(&mut self, engine: &Engine) {
        self.driver.sync(&engine.state().sound, engine.io().files());
    }

    /// Renders `ms` of audio (interleaved stereo) onto `out`.
    pub fn render_ms(&mut self, ms: u32, out: &mut Vec<f32>) {
        self.owed += u64::from(ms) * u64::from(self.rate);
        let frames = (self.owed / 1000) as usize;
        self.owed %= 1000;
        self.render_frames(frames, out);
    }

    /// Renders `frames` sample frames onto `out`.
    pub fn render_frames(&mut self, frames: usize, out: &mut Vec<f32>) {
        let at = out.len();
        out.resize(at + 2 * frames, 0.0);
        self.driver.render(&mut out[at..], self.rate);
    }
}

/// RMS and peak of interleaved samples.
pub fn levels(samples: &[f32]) -> (f32, f32) {
    if samples.is_empty() {
        return (0.0, 0.0);
    }
    let sum: f64 = samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
    let peak = samples.iter().fold(0f32, |m, &s| m.max(s.abs()));
    ((sum / samples.len() as f64).sqrt() as f32, peak)
}

/// Writes interleaved stereo f32 samples as a 16-bit PCM WAV file.
pub fn write_wav(path: &Path, samples: &[f32], rate: u32) -> Result<(), String> {
    let err = |e: std::io::Error| format!("{}: {e}", path.display());
    let mut w = BufWriter::new(File::create(path).map_err(err)?);
    let data = (samples.len() * 2) as u32;
    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(36 + data).to_le_bytes());
    header.extend_from_slice(b"WAVEfmt ");
    header.extend_from_slice(&16u32.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes()); // PCM
    header.extend_from_slice(&2u16.to_le_bytes()); // stereo
    header.extend_from_slice(&rate.to_le_bytes());
    header.extend_from_slice(&(rate * 4).to_le_bytes());
    header.extend_from_slice(&4u16.to_le_bytes());
    header.extend_from_slice(&16u16.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&data.to_le_bytes());
    w.write_all(&header).map_err(err)?;
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        w.write_all(&v.to_le_bytes()).map_err(err)?;
    }
    w.flush().map_err(err)
}

/// The fastest speed the window's sound plays at (spec 06 §5).
pub const LOUD_UP_TO: f64 = 4.0;
/// Wall milliseconds of the fade from full gain to silence, and back.
pub const FADE_MS: f32 = 50.0;

/// The gain the window's sound heads for at `speed`: full up to [`LOUD_UP_TO`], silence above
/// it and at Max.
pub fn loudness(speed: Speed) -> f32 {
    match speed {
        Speed::Finite(x) if x <= LOUD_UP_TO => 1.0,
        _ => 0.0,
    }
}

/// The window's output gain: before each sample frame it moves toward its target by the share
/// of [`FADE_MS`] one frame lasts, so a change of speed across [`LOUD_UP_TO`] fades instead of
/// clicking.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gain {
    now: f32,
    target: f32,
}

impl Default for Gain {
    fn default() -> Self {
        Gain {
            now: 1.0,
            target: 1.0,
        }
    }
}

impl Gain {
    /// Heads for the [`loudness`] of `speed`.
    pub fn set_speed(&mut self, speed: Speed) {
        self.target = loudness(speed);
    }

    /// Scales interleaved stereo `samples` rendered at `rate` frames a second. At full gain they
    /// stay as they are.
    pub fn apply(&mut self, samples: &mut [f32], rate: u32) {
        if self.now == 1.0 && self.target == 1.0 {
            return;
        }
        let step = 1000.0 / (FADE_MS * rate.max(1) as f32);
        for frame in samples.chunks_mut(2) {
            self.now = if self.now < self.target {
                (self.now + step).min(self.target)
            } else {
                (self.now - step).max(self.target)
            };
            for s in frame {
                *s *= self.now;
            }
        }
    }
}

/// The samples the device thread drains (interleaved stereo).
pub type Ring = Arc<Mutex<VecDeque<f32>>>;

/// The window's output: the device stream and the ring it drains. `None` when the machine has
/// no output device (the game runs silent).
pub struct Output {
    pub ring: Ring,
    pub rate: u32,
    gain: Gain,
    _stream: cpal::Stream,
}

impl Output {
    pub fn open() -> Result<Self, String> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let device = cpal::default_host()
            .default_output_device()
            .ok_or("no audio output device")?;
        let config = device.default_output_config().map_err(|e| e.to_string())?;
        if config.sample_format() != cpal::SampleFormat::F32 {
            return Err(format!(
                "output format {} (not f32)",
                config.sample_format()
            ));
        }
        let rate = config.sample_rate();
        let channels = usize::from(config.channels());
        let ring: Ring = Arc::new(Mutex::new(VecDeque::new()));
        let feed = Arc::clone(&ring);
        let stream = device
            .build_output_stream(
                config.into(),
                move |out: &mut [f32], _| drain(&feed, out, channels),
                |e| eprintln!("audio: {e}"),
                None,
            )
            .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Self {
            ring,
            rate,
            gain: Gain::default(),
            _stream: stream,
        })
    }

    /// The game's speed changed: the sound fades out above [`LOUD_UP_TO`], back in at or
    /// below it ([`Gain`]; it takes effect behind the [`AHEAD_MS`] already in the ring).
    pub fn set_speed(&mut self, speed: Speed) {
        self.gain.set_speed(speed);
    }

    /// Tops the ring up to [`AHEAD_MS`] of audio, at the [`Gain`].
    pub fn top_up(&mut self, audio: &mut Audio) {
        let want = (self.rate * AHEAD_MS / 1000) as usize * 2;
        let have = self.ring.lock().map_or(want, |r| r.len());
        if have < want {
            let mut buf = Vec::with_capacity(want - have);
            audio.render_frames((want - have) / 2, &mut buf);
            self.gain.apply(&mut buf, self.rate);
            if let Ok(mut r) = self.ring.lock() {
                r.extend(buf);
            }
        }
    }
}

/// The device callback: stereo frames from the ring onto `channels` output channels (silence
/// when the ring runs dry).
fn drain(ring: &Ring, out: &mut [f32], channels: usize) {
    let Ok(mut r) = ring.lock() else {
        out.fill(0.0);
        return;
    };
    for frame in out.chunks_mut(channels.max(1)) {
        let l = r.pop_front().unwrap_or(0.0);
        let rr = r.pop_front().unwrap_or(l);
        for (c, s) in frame.iter_mut().enumerate() {
            *s = if c % 2 == 0 { l } else { rr };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use u6_core::pacer::STEP_MS;
    use u6_testkit::Quiet;

    /// The boot's first 1.92 s in ticks of the fixed step, as the headless runner ticks.
    const BOOT_TICKS: u32 = 1920 / STEP_MS;

    #[test]
    fn levels_and_the_wav_header() {
        assert_eq!(levels(&[]), (0.0, 0.0));
        let (rms, peak) = levels(&[0.5, -0.5, 0.5, -0.5]);
        assert!((rms - 0.5).abs() < 1e-6 && (peak - 0.5).abs() < 1e-6);
        let path = std::env::temp_dir().join(format!("u6-native-wav-{}.wav", std::process::id()));
        write_wav(&path, &[1.0, -1.0, 0.0, 0.25], 22_050).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(bytes.len(), 44 + 8);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(
            u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
            22_050
        );
        let pcm: Vec<i16> = bytes[44..]
            .chunks(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        assert_eq!(pcm, [32767, -32767, 0, 8192]);
    }

    #[test]
    fn files_without_a_usable_game_leave_the_window_silent() {
        let engine = Engine::with_game(
            u6_core::State::default(),
            u6_core::Files::default(),
            |_, _| async {},
        );
        assert!(Audio::new(&engine, WAV_RATE).is_err());
        assert!(Audio::or_silent(&engine, WAV_RATE).is_none());
    }

    /// The user's `game` with its period table broken in memory: the MED checks fail, the game
    /// boots and runs, the window has no sound. Skips without the archive.
    #[test]
    #[allow(clippy::disallowed_methods)] // tests may read files
    fn a_game_whose_med_tables_fail_the_checks_runs_silent() {
        let zip = u6_testkit::archive();
        if !zip.exists() {
            eprintln!("skipped: {} not present", zip.display());
            return;
        }
        let r = u6_intake::intake(vec![u6_intake::InputFile {
            name: "ultima-vi.zip".into(),
            data: std::fs::read(zip).quiet(),
        }]);
        let mut files = u6_core::Files::new(r.files);
        let mut game = files.get("game").quiet().to_vec();
        let at = u6_core::image::GameImage::parse(&game)
            .quiet()
            .file_offset(u6_core::sound::tables::PERIODS)
            .quiet();
        game[at..at + 2 * 96].fill(0);
        files.overlay("broken", [("game".to_string(), game)].into());
        let mut engine = Engine::game_first(files, 0).quiet();
        assert!(Audio::or_silent(&engine, WAV_RATE).is_none());
        for _ in 0..BOOT_TICKS {
            engine.tick(&[], STEP_MS);
        }
        assert!(!engine.finished());
        assert!(
            engine.state().sound.next_call > 0,
            "the game's music calls go on"
        );
    }

    #[test]
    fn the_sound_is_full_up_to_4x_and_silent_above() {
        let at: Vec<f32> = [1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 8.0]
            .map(Speed::Finite)
            .into_iter()
            .chain([Speed::Max])
            .map(loudness)
            .collect();
        assert_eq!(at, [1.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0]);
    }

    /// The gain on a constant stereo signal: untouched at 1×; at 6× a fade to silence over
    /// [`FADE_MS`] with no frame-to-frame jump; back at 4× the same fade up, then untouched.
    #[test]
    fn the_gain_fades_out_above_4x_and_back_in() {
        const RATE: u32 = 1000; // a frame a millisecond
        let fade = FADE_MS as usize;
        let mut gain = Gain::default();
        // Left 0.5, right -0.25: each frame's gain read from both sides, which must agree.
        let run = |gain: &mut Gain, frames: usize| {
            let mut buf = [0.5f32, -0.25].repeat(frames);
            gain.apply(&mut buf, RATE);
            buf.chunks(2)
                .map(|f| {
                    let (l, r) = (f[0] / 0.5, f[1] / -0.25);
                    assert!((l - r).abs() < 1e-6, "left {l} and right {r}");
                    l
                })
                .collect::<Vec<f32>>()
        };
        let smooth = |g: &[f32]| {
            g.windows(2)
                .all(|w| (w[1] - w[0]).abs() <= 1.0 / FADE_MS + 1e-6)
        };
        gain.set_speed(Speed::Finite(1.0));
        assert!(
            run(&mut gain, 100).iter().all(|&g| g == 1.0),
            "1×: as rendered"
        );
        gain.set_speed(Speed::Finite(6.0));
        let down = run(&mut gain, 2 * fade);
        println!(
            "6×: gain {:.2} after 1 ms, {:.2} after 25 ms",
            down[0], down[24]
        );
        assert!(
            (down[0] - (1.0 - 1.0 / FADE_MS)).abs() < 1e-6,
            "no jump at the switch"
        );
        assert!((down[24] - 0.5).abs() < 1e-3, "half way at half the fade");
        assert!(
            smooth(&down) && down[fade..].iter().all(|&g| g == 0.0),
            "{down:?}"
        );
        assert!(
            run(&mut gain, 10).iter().all(|&g| g == 0.0),
            "silent while above 4×"
        );
        gain.set_speed(Speed::Finite(4.0));
        let up = run(&mut gain, 2 * fade);
        assert!(
            smooth(&up) && up[0] > 0.0 && up[fade..].iter().all(|&g| g == 1.0),
            "{up:?}"
        );
        assert_eq!(gain, Gain::default(), "back to full: untouched again");
        // A switch half way through the fade turns it around where it is.
        gain.set_speed(Speed::Max);
        let mut both = run(&mut gain, fade / 2);
        gain.set_speed(Speed::Finite(2.0));
        both.extend(run(&mut gain, fade));
        assert!(smooth(&both) && *both.last().unwrap() == 1.0, "{both:?}");
        // One render equals the same frames rendered in pieces, across both fades: 50 frames
        // at 6× (the fade out), then 100 at 1× (the fade in, then full).
        let signal: Vec<f32> = (0..3 * fade)
            .flat_map(|i| [i as f32, -(i as f32) / 2.0])
            .collect();
        let (mut whole, mut pieces) = (signal.clone(), signal);
        let mut gain = Gain::default();
        gain.set_speed(Speed::Finite(6.0));
        gain.apply(&mut whole[..2 * fade], RATE);
        gain.set_speed(Speed::Finite(1.0));
        gain.apply(&mut whole[2 * fade..], RATE);
        let mut gain = Gain::default();
        let mut at = 0;
        for (speed, frames) in [(6.0, &[1, 7, 13, 4, 25][..]), (1.0, &[30, 20, 50][..])] {
            gain.set_speed(Speed::Finite(speed));
            for &n in frames {
                gain.apply(&mut pieces[at..at + 2 * n], RATE);
                at += 2 * n;
            }
        }
        assert_eq!(at, whole.len());
        assert_eq!(pieces, whole, "pieces as one render");
        // At a device rate the fade takes FADE_MS of samples.
        let mut gain = Gain::default();
        gain.set_speed(Speed::Finite(8.0));
        let mut buf = vec![1.0f32; 2 * 48_000 / 10];
        gain.apply(&mut buf, 48_000);
        let silent = buf.chunks(2).position(|f| f[0] == 0.0).unwrap();
        assert!(silent.abs_diff(2400) <= 1, "{silent} frames at 48 kHz");
    }

    #[test]
    fn drain_spreads_stereo_over_the_device_channels() {
        let ring: Ring = Arc::new(Mutex::new(VecDeque::from(vec![0.1, 0.2, 0.3, 0.4])));
        let mut out = [9.0f32; 6];
        drain(&ring, &mut out, 1);
        assert_eq!(
            out,
            [0.1, 0.3, 0.0, 0.0, 0.0, 0.0],
            "mono takes the left side"
        );
        let ring: Ring = Arc::new(Mutex::new(VecDeque::from(vec![0.1, 0.2])));
        let mut out = [9.0f32; 4];
        drain(&ring, &mut out, 2);
        assert_eq!(out, [0.1, 0.2, 0.0, 0.0], "silence once the ring is dry");
    }

    /// The boot's music on the user's files (`game` started at once): the game's calls replayed after each tick render
    /// sound (the opening fight starts the combat song). Skips without the archive.
    #[test]
    #[allow(clippy::disallowed_methods)] // tests may read files
    fn the_boot_renders_music() {
        let zip = u6_testkit::archive();
        if !zip.exists() {
            eprintln!("skipped: {} not present", zip.display());
            return;
        }
        let r = u6_intake::intake(vec![u6_intake::InputFile {
            name: "ultima-vi.zip".into(),
            data: std::fs::read(zip).quiet(),
        }]);
        let mut engine = Engine::game_first(u6_core::Files::new(r.files), 0).quiet();
        let mut audio = Audio::new(&engine, WAV_RATE).quiet();
        let mut samples = Vec::new();
        for _ in 0..BOOT_TICKS {
            engine.tick(&[], STEP_MS);
            audio.sync(&engine);
            audio.render_ms(STEP_MS, &mut samples);
        }
        assert!(samples.len() == 2 * (1920 * WAV_RATE as usize / 1000));
        let (rms, peak) = levels(&samples);
        assert!(rms > 0.001 && peak <= 1.0, "audible and in range");
    }
}
