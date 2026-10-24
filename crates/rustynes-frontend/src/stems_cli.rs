//! v3.6.0 (`T-AUDIO-STEMS`) — `rustynes stems` and `rustynes nsf-rip`: the
//! headless front ends to the core's stem capture
//! ([`rustynes_core::stems`]) and NSF ripper ([`rustynes_core::nsf_rip`]).
//!
//! `stems` runs a ROM (or an NSF, or an FDS disk with its BIOS) for a number
//! of frames, or replays a movie, and writes one 32-bit float WAV per track
//! beside the console's own mix, all the same length, in one emulation pass.
//! It prints the capture's two checks: the re-rendered mix against the
//! console's output (bit for bit), and the largest `|mix - Σ tracks|`. A
//! failed mix check exits 4: the files are written, but they are not what
//! the console played.
//!
//! `nsf-rip` traces a run, proposes the music driver's `init` and `play`
//! with the evidence for each, and with `--out` writes the NSF. It then plays
//! the NSF it wrote and compares, call for call, the APU writes its `play`
//! makes against the game's; the file is reported verified only when they
//! match. See `docs/audio-stems.md`.
//!
//! Like `verify` and `speedrun`, both are headless: no window, no audio
//! device, no host input, so the result depends only on the inputs and the
//! deterministic core.

use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use rustynes_core::nsf_rip::{self, InitEvidence, NsfInfo, Proposal, Tracer};
use rustynes_core::rustynes_apu::FilterModel;
use rustynes_core::stems::{STEM_COUNT, Stem, StemCapture};
use rustynes_core::{Movie, MoviePlayer, Nes};

/// Exit status: done (and, for `nsf-rip --out`, verified).
pub const EXIT_OK: u8 = 0;
/// Exit status: an error (a file, the ROM, the movie, the capture).
pub const EXIT_ERROR: u8 = 1;
/// Exit status: a check failed.
///
/// For `stems`, the re-rendered mix differs
/// from the console's (the files were written, but are not to be trusted);
/// for `nsf-rip`, no driver was found, the board cannot be imaged, or the
/// NSF did not reproduce the game's writes.
pub const EXIT_CHECK_FAILED: u8 = 4;

/// Frames run when neither a movie nor `--frames` is given: one NTSC minute.
pub const DEFAULT_FRAMES: u64 = 3_600;

/// The sample rate written when `--rate` is not given.
pub const DEFAULT_RATE: u32 = 48_000;

/// `rustynes stems` arguments (parsed by `cli.rs`).
#[derive(Clone, Debug)]
pub struct StemsArgs {
    /// The ROM, NSF or FDS image.
    pub rom: PathBuf,
    /// The FDS BIOS, for an FDS image.
    pub bios: Option<PathBuf>,
    /// The directory to write into (created if missing).
    pub out: PathBuf,
    /// A movie to replay; without one, `frames` input-free frames run.
    pub movie: Option<PathBuf>,
    /// Frames to run without a movie.
    pub frames: u64,
    /// Output sample rate.
    pub rate: u32,
    /// The analog filter model.
    pub filter: FilterModel,
}

/// `rustynes nsf-rip` arguments.
#[derive(Clone, Debug)]
pub struct RipArgs {
    /// The cartridge ROM.
    pub rom: PathBuf,
    /// A movie to replay (to reach the music); without one, `frames`
    /// input-free frames run.
    pub movie: Option<PathBuf>,
    /// Frames to trace without a movie.
    pub frames: u64,
    /// Write the NSF here.
    pub out: Option<PathBuf>,
    /// The NSF's metadata.
    pub info: NsfInfo,
}

/// Parse a `--filter` value.
///
/// # Errors
/// The accepted names, for anything else.
pub fn parse_filter(s: &str) -> Result<FilterModel, String> {
    match s.to_ascii_lowercase().as_str() {
        "nes" | "nes-rf" | "rf" => Ok(FilterModel::NesRf),
        "famicom" | "fc" => Ok(FilterModel::Famicom),
        "clean" => Ok(FilterModel::Clean),
        _ => Err(format!("unknown filter {s:?} (nes, famicom or clean)")),
    }
}

/// A mono 32-bit IEEE-float WAV written as it goes. The sizes in the header
/// are patched by [`Self::finish`].
pub struct WavWriter {
    out: BufWriter<File>,
    frames: u64,
}

/// `WAVE_FORMAT_IEEE_FLOAT`.
const FORMAT_FLOAT: u16 = 3;

impl WavWriter {
    /// Create `path` and write a header for `rate` Hz.
    ///
    /// # Errors
    /// An I/O error.
    pub fn create(path: &Path, rate: u32) -> std::io::Result<Self> {
        let mut out = BufWriter::new(File::create(path)?);
        // RIFF header; the sizes are placeholders until `finish`.
        out.write_all(b"RIFF")?;
        out.write_all(&0u32.to_le_bytes())?;
        out.write_all(b"WAVE")?;
        // fmt: 18 bytes, the extensible-free form with cbSize = 0, which a
        // non-PCM format must carry (Microsoft's WAVEFORMATEX).
        out.write_all(b"fmt ")?;
        out.write_all(&18u32.to_le_bytes())?;
        out.write_all(&FORMAT_FLOAT.to_le_bytes())?;
        out.write_all(&1u16.to_le_bytes())?; // channels
        out.write_all(&rate.to_le_bytes())?;
        out.write_all(&(rate * 4).to_le_bytes())?; // bytes per second
        out.write_all(&4u16.to_le_bytes())?; // block align
        out.write_all(&32u16.to_le_bytes())?; // bits per sample
        out.write_all(&0u16.to_le_bytes())?; // cbSize
        // fact: the sample count, which a non-PCM file carries.
        out.write_all(b"fact")?;
        out.write_all(&4u32.to_le_bytes())?;
        out.write_all(&0u32.to_le_bytes())?;
        out.write_all(b"data")?;
        out.write_all(&0u32.to_le_bytes())?;
        Ok(Self { out, frames: 0 })
    }

    /// Append samples.
    ///
    /// # Errors
    /// An I/O error.
    pub fn write(&mut self, samples: &[f32]) -> std::io::Result<()> {
        for s in samples {
            self.out.write_all(&s.to_le_bytes())?;
        }
        self.frames += samples.len() as u64;
        Ok(())
    }

    /// Patch the header's sizes and flush.
    ///
    /// # Errors
    /// An I/O error, or more audio than a WAV's 32-bit sizes can describe.
    pub fn finish(mut self) -> std::io::Result<()> {
        let too_big = || std::io::Error::other("over 4 GiB of audio: a WAV cannot describe it");
        let data = u32::try_from(self.frames * 4).map_err(|_| too_big())?;
        let frames = u32::try_from(self.frames).map_err(|_| too_big())?;
        let riff = data.checked_add(HEADER_LEN - 8).ok_or_else(too_big)?;
        self.out.flush()?;
        let f = self.out.get_mut();
        f.seek(SeekFrom::Start(4))?;
        f.write_all(&riff.to_le_bytes())?;
        f.seek(SeekFrom::Start(46))?;
        f.write_all(&frames.to_le_bytes())?;
        f.seek(SeekFrom::Start(54))?;
        f.write_all(&data.to_le_bytes())?;
        f.flush()
    }
}

/// Header bytes before the samples: RIFF (12) + fmt (26) + fact (12) +
/// data (8).
const HEADER_LEN: u32 = 58;

/// Load an image the way the desktop does: NSF and NSFE by magic, an FDS disk
/// with its BIOS, otherwise an iNES / NES 2.0 cartridge.
fn load(rom: &Path, bios: Option<&Path>, rate: u32) -> Result<Nes, String> {
    let bytes = crate::dump_check::read_capped(rom)?;
    let what = rom.display();
    if bytes.starts_with(b"NESM\x1A") || bytes.starts_with(b"NSFE") {
        return Nes::from_nsf_with_sample_rate(&bytes, rate).map_err(|e| format!("{what}: {e}"));
    }
    if bytes.starts_with(b"FDS\x1A") || bytes.starts_with(b"\x01*NINTENDO-HVC*") {
        let bios = bios.ok_or_else(|| format!("{what} is an FDS disk: pass --bios"))?;
        let bios_bytes = crate::dump_check::read_capped(bios)?;
        return Nes::from_disk_with_sample_rate(&bytes, &bios_bytes, rate)
            .map_err(|e| format!("{what}: {e}"));
    }
    Nes::from_rom_with_sample_rate(&bytes, rate).map_err(|e| format!("{what}: {e}"))
}

/// Build the per-run state with `start` (once the console is at the run's
/// first frame: after a movie's seek), then call `frame` once per frame: the
/// movie's frames, or `frames` input-free ones. `frame` returns `false` to
/// stop early. Returns the frames run and the state.
fn drive<T>(
    nes: &mut Nes,
    movie: Option<&Path>,
    frames: u64,
    start: impl FnOnce(&mut Nes) -> Result<T, String>,
    mut frame: impl FnMut(&mut Nes, &mut T) -> Result<bool, String>,
) -> Result<(u64, T), String> {
    let mut ran = 0;
    if let Some(path) = movie {
        let bytes = crate::dump_check::read_capped(path)?;
        let movie = Movie::deserialize(&bytes)
            .map_err(|e| format!("{} is not a valid movie: {e}", path.display()))?;
        movie
            .seek_to_start(nes)
            .map_err(|e| format!("{} cannot start on this ROM: {e}", path.display()))?;
        let mut state = start(nes)?;
        let mut player = MoviePlayer::new(&movie);
        while player.apply_next(nes) {
            ran += 1;
            if !frame(nes, &mut state)? {
                break;
            }
        }
        Ok((ran, state))
    } else {
        let mut state = start(nes)?;
        for _ in 0..frames {
            ran += 1;
            if !frame(nes, &mut state)? {
                break;
            }
        }
        Ok((ran, state))
    }
}

/// Peak and RMS of a track, accumulated as it is written.
#[derive(Clone, Copy, Debug, Default)]
struct Level {
    peak: f32,
    sum_sq: f64,
}

impl Level {
    fn add(&mut self, samples: &[f32]) {
        for &s in samples {
            self.peak = self.peak.max(s.abs());
            self.sum_sq = f64::from(s).mul_add(f64::from(s), self.sum_sq);
        }
    }

    #[allow(clippy::cast_precision_loss)] // a report figure
    fn rms(&self, n: u64) -> f64 {
        if n == 0 {
            0.0
        } else {
            (self.sum_sq / n as f64).sqrt()
        }
    }
}

/// Run `rustynes stems`; returns the exit status.
#[must_use]
pub fn run_stems(args: &StemsArgs) -> u8 {
    match stems_inner(args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("rustynes stems: {e}");
            EXIT_ERROR
        }
    }
}

fn stems_inner(args: &StemsArgs) -> Result<u8, String> {
    if !(8_000..=192_000).contains(&args.rate) {
        return Err(format!("--rate {} is outside 8000..=192000", args.rate));
    }
    let mut nes = load(&args.rom, args.bios.as_deref(), args.rate)?;
    nes.set_apu_filter_model(args.filter);
    let chip = nes.expansion_audio_chip();
    std::fs::create_dir_all(&args.out)
        .map_err(|e| format!("cannot create {}: {e}", args.out.display()))?;
    let path = |name: &str| args.out.join(format!("{name}.wav"));
    let io = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    let mix_path = path("mix");
    let mut mix = WavWriter::create(&mix_path, args.rate).map_err(|e| io(&mix_path, e))?;
    let mut tracks = Vec::with_capacity(STEM_COUNT);
    for stem in Stem::ALL {
        let p = path(stem.name());
        tracks.push(WavWriter::create(&p, args.rate).map_err(|e| io(&p, e))?);
    }
    let mut levels = [Level::default(); STEM_COUNT + 1];
    let (frames, cap) = drive(
        &mut nes,
        args.movie.as_deref(),
        args.frames,
        |nes| StemCapture::start(nes, args.rate).map_err(|e| e.to_string()),
        |nes, cap| {
            nes.run_frame();
            let f = cap.after_frame(nes).map_err(|e| e.to_string())?;
            mix.write(f.mix).map_err(|e| io(&mix_path, e))?;
            levels[0].add(f.mix);
            for (i, (w, t)) in tracks.iter_mut().zip(f.tracks).enumerate() {
                w.write(t).map_err(|e| e.to_string())?;
                levels[i + 1].add(t);
            }
            Ok(true)
        },
    )?;
    mix.finish().map_err(|e| io(&mix_path, e))?;
    for w in tracks {
        w.finish().map_err(|e| e.to_string())?;
    }

    let n = cap.samples();
    println!(
        "{}: {frames} frames, {n} samples per track at {} Hz, written to {}",
        args.rom.display(),
        args.rate,
        args.out.display()
    );
    println!("  {:<18} {:>10} {:>10}", "track", "peak", "rms");
    println!(
        "  {:<18} {:>10.6} {:>10.6}",
        "mix",
        levels[0].peak,
        levels[0].rms(n)
    );
    for stem in Stem::ALL {
        let l = levels[stem.index() + 1];
        let label = match (stem, chip) {
            (Stem::Expansion, Some(c)) => format!("expansion ({c})"),
            (Stem::Expansion, None) => "expansion (none)".to_owned(),
            _ => stem.name().to_owned(),
        };
        println!("  {label:<18} {:>10.6} {:>10.6}", l.peak, l.rms(n));
    }
    println!(
        "  mix re-rendered from the trace: {} of {n} samples differ from the console's",
        cap.mix_mismatches()
    );
    println!(
        "  largest |mix - sum of tracks|: {:e} (f32 rounding; the interaction track carries the mixer's non-linearity)",
        cap.max_sum_error()
    );
    Ok(if cap.mix_mismatches() == 0 {
        EXIT_OK
    } else {
        eprintln!(
            "rustynes stems: the re-rendered mix is not the console's; do not trust the stems"
        );
        EXIT_CHECK_FAILED
    })
}

/// Run `rustynes nsf-rip`; returns the exit status.
#[must_use]
pub fn run_rip(args: &RipArgs) -> u8 {
    match rip_inner(args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("rustynes nsf-rip: {e}");
            EXIT_ERROR
        }
    }
}

/// Describe a proposal's evidence for the report.
#[must_use]
pub fn describe(p: &Proposal) -> String {
    let e = &p.play;
    let mut s = format!(
        "play ${:04X}: encloses every APU write in {} of the {} frames that wrote the APU; \
         called in {} frames, {} time(s) per frame; encloses {} of {} writes\n",
        e.addr,
        e.frames_enclosed,
        e.frames_with_writes,
        e.frames_called,
        e.calls_per_frame,
        e.writes_enclosed,
        e.writes_total
    );
    match p.init {
        InitEvidence::Routine {
            addr,
            writes,
            calls,
        } => {
            let _ = writeln!(
                s,
                "init ${addr:04X}: encloses all {writes} APU writes made outside play; called {calls} time(s)"
            );
        }
        InitEvidence::None {
            writes_outside_play,
        } => {
            let _ = writeln!(
                s,
                "init: none found ({writes_outside_play} APU writes outside play, not in one subroutine); \
                 the NSF gets a no-op init"
            );
        }
    }
    if e.calls_per_frame > 1 {
        s.push_str("note: play runs more than once a frame; the NSF is filed at the frame rate\n");
    }
    s
}

fn rip_inner(args: &RipArgs) -> Result<u8, String> {
    let bytes = crate::dump_check::read_capped(&args.rom)?;
    let mut nes = Nes::from_rom(&bytes).map_err(|e| format!("{}: {e}", args.rom.display()))?;
    let (_, game) = drive(
        &mut nes,
        args.movie.as_deref(),
        args.frames,
        |nes| Ok(Tracer::new(nes)),
        |nes, t| Ok(t.run_frame(nes)),
    )?;
    let proposal = match nsf_rip::propose(&game) {
        Ok(p) => p,
        Err(e) => {
            println!("{}: no proposal: {e}", args.rom.display());
            return Ok(EXIT_CHECK_FAILED);
        }
    };
    print!(
        "{}: traced {} frames\n{}",
        args.rom.display(),
        game.frames(),
        describe(&proposal)
    );
    let Some(out) = &args.out else {
        return Ok(EXIT_OK);
    };
    let (nsf, noop) = match nsf_rip::build_nsf(&bytes, &proposal, nes.region(), &args.info) {
        Ok(v) => v,
        Err(e) => {
            println!("not written: {e}");
            return Ok(EXIT_CHECK_FAILED);
        }
    };
    if let Some(n) = noop {
        println!("init points at the RTS at ${:04X}", n.addr);
    }
    crate::atomic_write::write_atomic(out, &nsf)
        .map_err(|e| format!("cannot write {}: {e}", out.display()))?;
    println!("wrote {} ({} bytes)", out.display(), nsf.len());

    // Play the file back and compare its play calls with the game's.
    let mut player =
        Nes::from_nsf(&nsf).map_err(|e| format!("the written NSF does not load: {e}"))?;
    let mut played = Tracer::new(&mut player);
    for _ in 0..game.frames() {
        if !played.run_frame(&mut player) {
            break;
        }
    }
    let ours = game.writes_per_call(proposal.play.addr);
    let theirs = played.writes_per_call(proposal.play.addr);
    let n = ours.len().min(theirs.len());
    let same = n > 0 && ours[..n] == theirs[..n];
    let audible = ours[..n].iter().filter(|c| !c.is_empty()).count();
    if same {
        println!(
            "verified: the NSF's first {n} play calls make the game's APU writes ({audible} of them write)"
        );
        Ok(EXIT_OK)
    } else {
        let first = (0..n).find(|&i| ours[i] != theirs[i]);
        println!(
            "NOT verified: {} of the game's play calls, {} of the NSF's; first difference at call {}",
            ours.len(),
            theirs.len(),
            first.map_or_else(|| "none (no calls)".to_owned(), |i| i.to_string())
        );
        Ok(EXIT_CHECK_FAILED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wav_header_describes_its_samples() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let p = dir.path().join("t.wav");
        let mut w = WavWriter::create(&p, 48_000).expect("create");
        w.write(&[0.5, -0.25, 1.0]).expect("write");
        w.finish().expect("finish");
        let b = std::fs::read(&p).expect("read");
        assert_eq!(b.len(), 58 + 12);
        let u32_at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let u16_at = |o: usize| u16::from_le_bytes(b[o..o + 2].try_into().unwrap());
        assert_eq!(&b[0..4], b"RIFF");
        assert_eq!(u32_at(4) as usize, b.len() - 8);
        assert_eq!(&b[8..12], b"WAVE");
        assert_eq!(&b[12..16], b"fmt ");
        assert_eq!(u32_at(16), 18);
        assert_eq!(u16_at(20), 3, "IEEE float");
        assert_eq!(u16_at(22), 1, "mono");
        assert_eq!(u32_at(24), 48_000);
        assert_eq!(u32_at(28), 192_000);
        assert_eq!(u16_at(32), 4);
        assert_eq!(u16_at(34), 32);
        assert_eq!(&b[38..42], b"fact");
        assert_eq!(u32_at(46), 3, "three sample frames");
        assert_eq!(&b[50..54], b"data");
        assert_eq!(u32_at(54), 12);
        assert_eq!(f32::from_le_bytes(b[58..62].try_into().unwrap()), 0.5);
        assert_eq!(f32::from_le_bytes(b[66..70].try_into().unwrap()), 1.0);
    }

    #[test]
    fn filter_names_parse() {
        assert_eq!(parse_filter("NES"), Ok(FilterModel::NesRf));
        assert_eq!(parse_filter("famicom"), Ok(FilterModel::Famicom));
        assert_eq!(parse_filter("clean"), Ok(FilterModel::Clean));
        assert!(parse_filter("loud").is_err());
    }

    /// The vendored bbbradsmith VRC6 level ROM (see `tests/roms/audio-tests/`).
    fn vrc6_rom() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/roms/audio-tests/db_vrc6a.nes")
    }

    #[test]
    fn stems_writes_eight_equal_files_that_sum_to_the_mix() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let args = StemsArgs {
            rom: vrc6_rom(),
            bios: None,
            out: dir.path().join("out"),
            movie: None,
            frames: 650,
            rate: 44_100,
            filter: FilterModel::NesRf,
        };
        assert_eq!(run_stems(&args), EXIT_OK);
        let read = |name: &str| -> Vec<f32> {
            let b = std::fs::read(args.out.join(format!("{name}.wav"))).expect("written");
            b[58..]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| f32::from_le_bytes(*c))
                .collect()
        };
        let mix = read("mix");
        assert!(mix.len() > 470_000, "{} samples", mix.len());
        let tracks: Vec<Vec<f32>> = Stem::ALL.iter().map(|s| read(s.name())).collect();
        let mut worst = 0.0f32;
        for (i, &m) in mix.iter().enumerate() {
            let sum: f32 = tracks.iter().map(|t| t[i]).sum();
            worst = worst.max((m - sum).abs());
        }
        assert!(worst < 1e-5, "the files sum to the mix: {worst}");
        let ext = &tracks[Stem::Expansion.index()];
        assert!(
            ext.iter().any(|&v| v.abs() > 0.02),
            "the VRC6 is in its stem"
        );
    }

    #[test]
    fn stems_reports_a_missing_rom_as_an_error() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let args = StemsArgs {
            rom: dir.path().join("nope.nes"),
            bios: None,
            out: dir.path().join("out"),
            movie: None,
            frames: 1,
            rate: 48_000,
            filter: FilterModel::NesRf,
        };
        assert_eq!(run_stems(&args), EXIT_ERROR);
        let bad_rate = StemsArgs {
            rom: vrc6_rom(),
            rate: 1_000,
            ..args
        };
        assert_eq!(run_stems(&bad_rate), EXIT_ERROR);
    }

    #[test]
    fn nsf_rip_writes_a_verified_nsf() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let rom = dir.path().join("music.nes");
        std::fs::write(&rom, nsf_rip::example_music_rom()).expect("write the ROM");
        let out = dir.path().join("music.nsf");
        let args = RipArgs {
            rom,
            movie: None,
            frames: 200,
            out: Some(out.clone()),
            info: NsfInfo {
                name: "Example".into(),
                ..NsfInfo::default()
            },
        };
        assert_eq!(run_rip(&args), EXIT_OK);
        let nsf = std::fs::read(&out).expect("written");
        assert_eq!(&nsf[..5], b"NESM\x1A");
        assert_eq!(&nsf[0x0E..0x15], b"Example");
    }

    #[test]
    fn nsf_rip_refuses_a_rom_with_no_music() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let rom = dir.path().join("quiet.nes");
        // An NROM that spins at reset and never writes the APU.
        let mut prg = vec![0u8; 0x4000];
        prg[..3].copy_from_slice(&[0x4C, 0x00, 0xC0]); // JMP $C000
        prg[0x3FFC..0x3FFE].copy_from_slice(&0xC000u16.to_le_bytes());
        let mut bytes = vec![b'N', b'E', b'S', 0x1A, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        bytes.extend_from_slice(&prg);
        bytes.extend(std::iter::repeat_n(0u8, 0x2000));
        std::fs::write(&rom, bytes).expect("write");
        let args = RipArgs {
            rom,
            movie: None,
            frames: 10,
            out: None,
            info: NsfInfo::default(),
        };
        assert_eq!(run_rip(&args), EXIT_CHECK_FAILED);
    }
}
