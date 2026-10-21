//! v3.5.0 (`T-MOVIE-INPUT-STREAM`, backlog CR-03) — the gate for the full
//! movie input stream: a Zapper movie, a Vs. System movie and an FDS movie
//! each record and replay byte-identically, and each kind of non-controller
//! input the `.rnm` format 7 carries is shown to MATTER to its replay.
//!
//! # The ROMs
//!
//! All three are built here, so the gate runs in the default build with no
//! dump and no BIOS: an NROM cartridge (plain, then with an NES 2.0 Vs.
//! System header) and an FDS disk with a synthetic 8 KiB BIOS. The real
//! `disksys.rom` is never needed: what is under test is the movie machinery
//! carrying the disk events, not the BIOS's disk loader, and the synthetic
//! BIOS reads the drive status register the disk events drive. The
//! committed-test-ROM alternative was looked for and does not exist: no CC0
//! ROM in `tests/roms/` reads the Zapper, the Vs. coin inputs or `$4032`.
//!
//! Each program is the same "input echo" loop: wait for vblank, strobe the
//! controllers, XOR together the input registers that matter for the
//! machine (`$4016` and `$4017`; plus `$4032` on the FDS), and write the
//! result both to the backdrop colour (so it is in the framebuffer) and to
//! the DMC's direct-load DAC `$4011` (so it is in the audio). Every bit an
//! input changes therefore changes both outputs a replay is compared on.
//!
//! # What "byte-identical" is measured on
//!
//! Per frame: FNV-1a over the RGBA framebuffer, then over the frame's audio
//! samples (`Nes::drain_audio`, `f32` little-endian). The per-frame digests
//! are compared as a list, so a failure names the first frame that differs.
//! The machine's full save state at the end is compared too, which catches a
//! divergence no pixel or sample shows (a latched coin the program never
//! reads, a device's internal register).
//!
//! # Sensitivity: each event kind matters
//!
//! A replay gate that passes when an input is lost proves nothing. For every
//! event kind a movie carries, [`strip`] produces the movie a writer that
//! dropped that kind would have produced (every frame's value of that field
//! back to the neutral one the decoder starts from), and the test asserts
//! its replay DIVERGES. The same was checked by mutating the writer itself
//! (`movie_extra::encode_events`), recorded in the CHANGELOG entry.
//!
//! ```bash
//! cargo test -p rustynes-test-harness --test movie_input_stream
//! ```

use rustynes_core::{
    Buttons, DeviceInput, ExtraInput, Movie, MoviePlayer, MovieRecorder, Nes, power_on_for_movie,
};

// ---------------------------------------------------------------------------
// Synthetic ROMs
// ---------------------------------------------------------------------------

/// The "input echo" loop, assembled at `base`.
///
/// `init` runs once after the stack and PPU are set up; `reads` are the
/// absolute addresses `XOR`ed together each frame (the first is loaded, the
/// rest are `EOR`ed in).
fn echo_program(base: u16, init: &[u8], reads: &[u16]) -> Vec<u8> {
    let mut p = vec![
        0x78, // SEI
        0xA2, 0xFF, 0x9A, // LDX #$FF ; TXS
        0xA9, 0x00, 0x8D, 0x00, 0x20, // LDA #0 ; STA $2000 (NMI off)
        0x8D, 0x01, 0x20, // STA $2001 (rendering off: backdrop only)
    ];
    p.extend_from_slice(init);
    let lp = base + u16::try_from(p.len()).unwrap();
    p.extend_from_slice(&[0x2C, 0x02, 0x20, 0x10, 0xFB]); // BIT $2002 ; BPL loop
    p.extend_from_slice(&[0xA9, 0x01, 0x8D, 0x16, 0x40]); // strobe high
    p.extend_from_slice(&[0xA9, 0x00, 0x8D, 0x16, 0x40]); // strobe low
    for (i, &a) in reads.iter().enumerate() {
        let [lo, hi] = a.to_le_bytes();
        p.extend_from_slice(&[if i == 0 { 0xAD } else { 0x4D }, lo, hi]); // LDA / EOR abs
    }
    p.extend_from_slice(&[0x8D, 0x11, 0x40]); // STA $4011 (DMC DAC)
    p.extend_from_slice(&[0x29, 0x3F, 0xAA]); // AND #$3F ; TAX
    p.extend_from_slice(&[0xA9, 0x3F, 0x8D, 0x06, 0x20, 0xA9, 0x00, 0x8D, 0x06, 0x20]); // $2006 = $3F00
    p.extend_from_slice(&[0x8E, 0x07, 0x20]); // STX $2007 (backdrop)
    p.extend_from_slice(&[0xA9, 0x00, 0x8D, 0x06, 0x20, 0x8D, 0x06, 0x20]); // $2006 = $0000
    let [lo, hi] = lp.to_le_bytes();
    p.extend_from_slice(&[0x4C, lo, hi]); // JMP loop
    p
}

/// A 16 KiB NROM image whose program is the echo loop at `$C000`. `vs`
/// gives it an NES 2.0 header declaring Vs. System hardware.
fn echo_nrom(vs: bool) -> Vec<u8> {
    let mut rom = vec![b'N', b'E', b'S', 0x1A, 1, 1, 0, 0];
    if vs {
        // Byte 7: console type 1 (Vs. System) in bits 0-1, NES 2.0 marker
        // (bits 2-3 = 2).
        rom[7] = 0x01 | 0x08;
    }
    rom.extend_from_slice(&[0u8; 8]);
    let mut prg = vec![0u8; 16 * 1024];
    let code = echo_program(0xC000, &[], &[0x4016, 0x4017]);
    prg[..code.len()].copy_from_slice(&code);
    for v in [0x3FFA, 0x3FFC, 0x3FFE] {
        prg[v] = 0x00;
        prg[v + 1] = 0xC0;
    }
    rom.extend_from_slice(&prg);
    rom.extend_from_slice(&[0u8; 8 * 1024]);
    rom
}

const FDS_SIDE_LEN: usize = 65_500;

/// A two-sided fwNES disk image, each side opening with the disk-info
/// block signature (the same shape `tests/fds.rs` builds); the bytes after
/// it differ per side, so the two sides are different media.
fn synth_disk() -> Vec<u8> {
    let sides = 2u8;
    let mut out = vec![0u8; 16 + usize::from(sides) * FDS_SIDE_LEN];
    out[0..4].copy_from_slice(b"FDS\x1A");
    out[4] = sides;
    for s in 0..usize::from(sides) {
        let base = 16 + s * FDS_SIDE_LEN;
        out[base] = 0x01;
        out[base + 1..base + 15].copy_from_slice(b"*NINTENDO-HVC*");
        let salt = if s == 0 { 0x11 } else { 0xA5 };
        for (i, b) in out[base + 16..base + FDS_SIDE_LEN].iter_mut().enumerate() {
            *b = u8::try_from(i % 251).unwrap() ^ salt;
        }
    }
    out
}

/// A synthetic 8 KiB BIOS at `$E000` running the echo loop over `$4016`,
/// `$4017` and the drive status `$4032`, after enabling disk I/O (`$4023`)
/// and starting the motor in read mode (`$4025` = `$24`: bit 1 clear starts
/// the motor, bit 2 selects read).
///
/// What reaches the picture and the audio is the eject (`$4032` bit 0).
/// WHICH side is inserted does not: measured, the value this loop reads is
/// the same for side A and side B, frame by frame (a real BIOS tells them
/// apart by reading the disk header, which this one does not). The side is
/// covered by the per-frame state digest instead (see [`Run`]).
fn synth_bios() -> Vec<u8> {
    let mut bios = vec![0u8; 0x2000];
    let init = [
        0xA9, 0x03, 0x8D, 0x23, 0x40, // LDA #$03 ; STA $4023 (disk + sound I/O)
        0xA9, 0x24, 0x8D, 0x25, 0x40, // LDA #$24 ; STA $4025 (motor on, read mode)
    ];
    let code = echo_program(0xE000, &init, &[0x4016, 0x4017, 0x4032]);
    bios[..code.len()].copy_from_slice(&code);
    for v in [0x1FFA, 0x1FFC, 0x1FFE] {
        bios[v] = 0x00;
        bios[v + 1] = 0xE0;
    }
    bios
}

// ---------------------------------------------------------------------------
// Record, replay, compare
// ---------------------------------------------------------------------------

fn fnv(h: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *h ^= u64::from(b);
        *h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
}

/// The machine's full save state after a frame, hashed.
fn state_digest(nes: &Nes) -> u64 {
    let mut h = 0xCBF2_9CE4_8422_2325;
    fnv(&mut h, &nes.snapshot());
    h
}

/// One frame's output digest: framebuffer, then the frame's audio.
fn frame_digest(nes: &mut Nes) -> u64 {
    let mut h = 0xCBF2_9CE4_8422_2325;
    fnv(&mut h, nes.framebuffer());
    for s in nes.drain_audio() {
        fnv(&mut h, &s.to_le_bytes());
    }
    h
}

/// What a run produced: one output digest per frame, one state digest per
/// frame, and the machine at the end.
///
/// The output digest (framebuffer + audio) is the plan's gate. The state
/// digest (FNV-1a of `Nes::snapshot` after each frame) is stricter: it sees
/// an input the program never shows, such as which FDS side is in the drive,
/// on the frame it changed rather than only if it survives to the end.
struct Run {
    digests: Vec<u64>,
    states: Vec<u64>,
    end_state: Vec<u8>,
    /// For a replay: whether the movie parsed back equal to what was
    /// serialized. Checked AFTER the outputs are compared, so a writer that
    /// loses an input fails on the replay diverging (the property the gate
    /// exists for) and names the frame, not on the structural comparison.
    round_trip_equal: bool,
}

impl Run {
    /// The first frame whose output digest differs, for a readable failure.
    fn first_difference(&self, other: &Self) -> Option<usize> {
        self.digests
            .iter()
            .zip(&other.digests)
            .position(|(a, b)| a != b)
    }

    /// The first frame whose state digest differs.
    fn first_state_difference(&self, other: &Self) -> Option<usize> {
        self.states
            .iter()
            .zip(&other.states)
            .position(|(a, b)| a != b)
    }
}

/// Record `frames` frames from a power-on, the host driving input through
/// `host(frame, nes)` before each, exactly as a frontend does: its input
/// calls, then the recorder's capture, then the frame.
fn record(mut nes: Nes, frames: usize, mut host: impl FnMut(usize, &mut Nes)) -> (Movie, Run) {
    power_on_for_movie(&mut nes);
    let mut rec = MovieRecorder::power_on(&nes);
    let _ = nes.drain_audio();
    let mut digests = Vec::with_capacity(frames);
    let mut states = Vec::with_capacity(frames);
    for i in 0..frames {
        host(i, &mut nes);
        rec.capture(&nes);
        nes.run_frame();
        digests.push(frame_digest(&mut nes));
        states.push(state_digest(&nes));
    }
    let run = Run {
        digests,
        states,
        end_state: nes.snapshot(),
        round_trip_equal: true,
    };
    (rec.finish(), run)
}

/// Serialize `movie`, parse it back, and replay it on `nes` (a fresh
/// machine with none of the recording host's devices attached).
fn replay(movie: &Movie, mut nes: Nes) -> Run {
    let parsed = Movie::deserialize(&movie.serialize()).expect("the movie parses");
    let round_trip_equal = &parsed == movie;
    parsed.seek_to_start(&mut nes).expect("the movie starts");
    let _ = nes.drain_audio();
    let mut player = MoviePlayer::new(&parsed);
    let mut digests = Vec::with_capacity(parsed.len());
    let mut states = Vec::with_capacity(parsed.len());
    while player.apply_next(&mut nes) {
        nes.run_frame();
        digests.push(frame_digest(&mut nes));
        states.push(state_digest(&nes));
    }
    Run {
        digests,
        states,
        end_state: nes.snapshot(),
        round_trip_equal,
    }
}

impl Run {
    /// The outputs and the machine: every digest and the end state.
    fn same_output(&self, other: &Self) -> bool {
        self.digests == other.digests
            && self.states == other.states
            && self.end_state == other.end_state
    }
}

/// The kinds of non-controller input a movie carries.
#[derive(Clone, Copy, Debug)]
enum Kind {
    Devices,
    Microphone,
    VsCoin,
    VsService,
    Disk,
}

/// `movie` as a writer that dropped every event of `kind` would have left it:
/// that field back to the neutral value the decoder starts from, every frame.
fn strip(movie: &Movie, kind: Kind) -> Movie {
    let mut out = movie.clone();
    let neutral = ExtraInput::default();
    for f in &mut out.frames {
        let e = f
            .extra
            .as_mut()
            .expect("a recorded frame carries its extra input");
        match kind {
            Kind::Devices => e.devices = neutral.devices,
            Kind::Microphone => e.microphone = neutral.microphone,
            Kind::VsCoin => e.vs_coin = neutral.vs_coin,
            Kind::VsService => e.vs_service = neutral.vs_service,
            Kind::Disk => e.disk_side = neutral.disk_side,
        }
    }
    out
}

/// The gate: the replay equals the recording, and dropping each listed kind
/// makes it differ.
fn assert_gate(movie: &Movie, recorded: &Run, fresh: impl Fn() -> Nes, kinds: &[Kind]) {
    let replayed = replay(movie, fresh());
    assert_eq!(
        replayed.digests.len(),
        recorded.digests.len(),
        "every recorded frame replays"
    );
    assert!(
        replayed.same_output(recorded),
        "replay diverges from the recording: output at frame {:?}, state at frame {:?}, end state equal: {}",
        replayed.first_difference(recorded),
        replayed.first_state_difference(recorded),
        replayed.end_state == recorded.end_state
    );
    assert!(replayed.round_trip_equal, "the .rnm round-trip is lossless");
    for &kind in kinds {
        let stripped = replay(&strip(movie, kind), fresh());
        assert!(
            !stripped.same_output(recorded),
            "dropping the {kind:?} events left the replay unchanged: the gate cannot see them"
        );
    }
}

const FRAMES: usize = 120;

// ---------------------------------------------------------------------------
// The three movies
// ---------------------------------------------------------------------------

/// A Zapper on port 1, aimed across the screen and off it, its trigger
/// pulled and released, with the Famicom microphone keyed and the pad on
/// port 0 pressed: all of it replays, and losing the device events or the
/// microphone events does not.
#[test]
fn a_zapper_movie_replays_byte_identically() {
    let rom = echo_nrom(false);
    let (movie, recorded) = record(Nes::from_rom(&rom).unwrap(), FRAMES, |i, nes| {
        let (x, y) = if i % 50 >= 45 {
            (u16::MAX, u16::MAX) // aimed off-screen
        } else {
            (
                u16::try_from(i * 7 % 256).unwrap(),
                200 + u16::try_from(i * 3 % 40).unwrap(),
            )
        };
        if (90..100).contains(&i) {
            // Unplugged for ten frames: port 1 reads the standard pad.
            nes.set_expansion_device(1, None);
            nes.set_buttons(1, Buttons::from_bits_truncate(0xFF));
        } else {
            nes.set_zapper(1, x, y, i / 5 % 2 == 1);
        }
        nes.set_microphone(i % 7 < 2);
        nes.set_buttons(
            0,
            Buttons::from_bits_truncate(u8::try_from(i % 256).unwrap()),
        );
    });
    // The recorder saw what the host did.
    let e = movie.frames[5].extra.expect("recorded");
    assert!(matches!(
        e.devices[1],
        DeviceInput::Zapper { trigger: true, .. }
    ));
    assert_eq!(e.devices[0], DeviceInput::None);
    assert!(movie.frames.iter().any(|f| f.extra.unwrap().microphone));
    assert_gate(
        &movie,
        &recorded,
        || Nes::from_rom(&rom).unwrap(),
        &[Kind::Devices, Kind::Microphone],
    );
}

/// A Vs. System machine: coins into both acceptors (latched for three
/// frames, then released, as the frontends do) and the service button held.
#[test]
fn a_vs_system_movie_replays_byte_identically() {
    let rom = echo_nrom(true);
    let nes = Nes::from_rom(&rom).unwrap();
    assert!(
        nes.is_vs_system(),
        "the synthetic header declares Vs. hardware"
    );
    let (movie, recorded) = record(nes, FRAMES, |i, nes| {
        match i {
            10 | 40 => nes.insert_coin(0),
            70 => nes.insert_coin(1),
            13 | 43 | 73 => nes.clear_coin(),
            _ => {}
        }
        nes.set_vs_service((50..56).contains(&i));
    });
    assert_eq!(movie.frames[11].extra.unwrap().vs_coin, 0b01);
    assert_eq!(movie.frames[71].extra.unwrap().vs_coin, 0b10);
    assert!(movie.frames[52].extra.unwrap().vs_service);
    assert_gate(
        &movie,
        &recorded,
        || Nes::from_rom(&rom).unwrap(),
        &[Kind::VsCoin, Kind::VsService],
    );
}

/// An FDS disk ejected, side B inserted, swapped straight back to side A,
/// ejected and re-inserted.
#[test]
fn an_fds_movie_replays_byte_identically() {
    let (disk, bios) = (synth_disk(), synth_bios());
    let fresh = || Nes::from_disk(&disk, &bios).unwrap();
    assert_eq!(fresh().inserted_disk_side(), Some(0), "side A at power-on");
    let (movie, recorded) = record(fresh(), FRAMES, |i, nes| match i {
        20 | 80 => nes.set_disk_side(None),
        30 => nes.set_disk_side(Some(1)),
        60 | 85 => nes.set_disk_side(Some(0)),
        _ => {}
    });
    assert_eq!(movie.frames[25].extra.unwrap().disk_side, None);
    assert_eq!(movie.frames[45].extra.unwrap().disk_side, Some(1));
    assert_eq!(movie.frames[65].extra.unwrap().disk_side, Some(0));
    assert_gate(&movie, &recorded, fresh, &[Kind::Disk]);
    // The eject is visible in the picture itself, not only in the state.
    let blind = replay(&strip(&movie, Kind::Disk), fresh());
    assert_ne!(
        blind.digests, recorded.digests,
        "the eject reaches the output"
    );
}

// ---------------------------------------------------------------------------
// The rest of the stream
// ---------------------------------------------------------------------------

/// Every device kind a port can hold is recorded from the machine and driven
/// back onto a fresh one exactly: after `apply`, a capture of the fresh
/// machine reads the same value. Covers the devices the three movies do not
/// (Vaus, mats, mouse, keyboards, Hyper Shots), and the `.rnm` round-trip of
/// each.
#[test]
fn every_device_kind_captures_applies_and_round_trips() {
    let rom = echo_nrom(false);
    let host: [fn(&mut Nes); 9] = [
        |n| n.set_zapper(1, 100, 120, true),
        |n| n.set_paddle(1, 0x5A, true),
        |n| n.set_power_pad(1, 0x0A5A),
        |n| n.set_family_trainer(1, 0x0123),
        |n| n.set_snes_mouse(1, -7, 12, true, false, 2),
        |n| n.set_family_keyboard(1, [1, 2, 4, 8, 16, 32, 64, 128, 0x55]),
        |n| n.set_subor_keyboard(1, [0x80, 0, 0, 0, 0, 0, 0, 0, 1]),
        |n| n.set_konami_hyper_shot(1, 0x09),
        |n| n.set_bandai_hyper_shot(1, 0xC3),
    ];
    for set in host {
        let (movie, recorded) = record(Nes::from_rom(&rom).unwrap(), 4, |_, nes| set(nes));
        let dev = movie.frames[0].extra.unwrap().devices[1];
        assert_ne!(dev, DeviceInput::None, "the device was recorded");
        let mut fresh = Nes::from_rom(&rom).unwrap();
        movie.frames[0].apply_to(&mut fresh);
        assert_eq!(ExtraInput::capture(&fresh).devices[1], dev, "{dev:?}");
        let replayed = replay(&movie, Nes::from_rom(&rom).unwrap());
        assert!(replayed.same_output(&recorded), "{dev:?} replays");
        assert!(replayed.round_trip_equal, "{dev:?} round-trips");
    }
}

/// A Datach barcode swipe is recorded on the frame it was made before, and
/// only that frame, and replays to the same machine (the reader's state is
/// in the save state compared at the end).
#[test]
fn a_barcode_scan_is_recorded_once_and_replays() {
    // Mapper 157 (Datach), 128 KiB PRG, CHR-RAM. The program never runs
    // anything that matters (PRG is zero: BRK forever); what is compared is
    // the reader's state in the end-of-run save state.
    let mut rom = vec![b'N', b'E', b'S', 0x1A, 8, 0, 0xD0, 0x90];
    rom.extend_from_slice(&[0u8; 8]);
    rom.extend_from_slice(&vec![0u8; 128 * 1024]);
    let nes = Nes::from_rom(&rom).expect("a Datach image builds");
    assert!(nes.has_barcode_reader());
    let code = "4901234567894";
    let (movie, recorded) = record(nes, 8, |i, nes| {
        if i == 3 {
            nes.scan_barcode(code).expect("the code is valid");
        }
    });
    let scans: Vec<usize> = (0..movie.len())
        .filter(|&i| movie.frames[i].extra.unwrap().barcode.is_some())
        .collect();
    assert_eq!(scans, [3], "one scan, on the frame it preceded");
    let replayed = replay(&movie, Nes::from_rom(&rom).unwrap());
    assert!(replayed.same_output(&recorded), "the scan replays");
    assert!(replayed.round_trip_equal, "the scan round-trips");
    // Without the scan the reader ends elsewhere.
    let mut dropped = movie;
    for f in &mut dropped.frames {
        if let Some(e) = f.extra.as_mut() {
            e.barcode = None;
        }
    }
    assert!(
        replay(&dropped, Nes::from_rom(&rom).unwrap()).end_state != recorded.end_state,
        "dropping the scan changes the machine"
    );
}

/// A frame that carries no extra input (an importer's, or a TAS editor's
/// blank frame) leaves the machine's devices and disk alone, instead of
/// unplugging the Zapper or ejecting the disk the way the neutral value would.
#[test]
fn a_frame_without_extra_input_holds_the_machine_as_it_is() {
    let (disk, bios) = (synth_disk(), synth_bios());
    let mut nes = Nes::from_disk(&disk, &bios).unwrap();
    nes.set_zapper(1, 10, 10, true);
    let before = ExtraInput::capture(&nes);
    let blank = rustynes_core::FrameInput::default();
    assert!(blank.extra.is_none());
    blank.apply_to(&mut nes);
    assert_eq!(ExtraInput::capture(&nes), before);
    assert_eq!(nes.inserted_disk_side(), Some(0));
}
