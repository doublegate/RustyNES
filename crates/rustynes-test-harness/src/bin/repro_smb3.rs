//! SMB3 (MMC3) "Mario flashing in World 1-1" reproduction harness (v1.2.0).
//!
//! NOT part of CI. Diagnostic tool only; uses a gitignored external ROM dump.
//!
//! Scripts the input path from power-on to World 1-1 (tap Start past the title,
//! tap Start/A to start the game + enter the first level), then captures a run
//! of consecutive frames. It dumps a PNG filmstrip (every Nth frame) so the
//! progression can be eyeballed to confirm we reach 1-1, and reports a
//! frame-parity oscillation metric over the capture window: if Mario "flashes"
//! every other frame, the mean per-pixel difference between frame f and f-2
//! (same parity) is small while f vs f-1 (opposite parity) is large.
//!
//! Usage:
//!
//! ```text
//! cargo run -p rustynes-test-harness --features commercial-roms \
//!     --bin repro_smb3 -- <rom-path> <out-dir> [--movie <file.rnm>] [--state <file.rns>]
//! ```
//!
//! With `--movie`, the harness replays a recorded TAS (the player's exact path
//! into 1-1) instead of the scripted taps — the faithful reproduction.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::doc_markdown,
    clippy::too_many_lines
)]

use std::path::Path;

use rustynes_core::{Buttons, Movie, MoviePlayer, Nes};

const W: u32 = 256;
const H: u32 = 240;

fn write_png(path: &Path, fb: &[u8]) {
    let file = std::fs::File::create(path).expect("create png");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), W, H);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().expect("png header");
    writer.write_image_data(fb).expect("png data");
}

fn write_png_sized(path: &Path, rgba: &[u8], width: u32, height: u32) {
    let file = std::fs::File::create(path).expect("create png");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().expect("png header");
    writer.write_image_data(rgba).expect("png data");
}

/// Crop a `cw`x`ch` region at (`cx`,`cy`) and magnify `zoom`x (nearest), so a
/// running-Mario sprite (camera-locked near screen centre) is legible + any
/// per-frame missing-sprite-portion flicker is obvious.
fn crop_zoom(fb: &[u8], cx: usize, cy: usize, cw: usize, ch: usize, zoom: usize) -> Vec<u8> {
    let w = W as usize;
    let out_w = cw * zoom;
    let mut out = vec![0u8; out_w * ch * zoom * 4];
    for y in 0..ch {
        for x in 0..cw {
            let src = ((cy + y) * w + (cx + x)) * 4;
            let px = [fb[src], fb[src + 1], fb[src + 2], fb[src + 3]];
            for zy in 0..zoom {
                for zx in 0..zoom {
                    let dst = (((y * zoom + zy) * out_w) + (x * zoom + zx)) * 4;
                    out[dst..dst + 4].copy_from_slice(&px);
                }
            }
        }
    }
    out
}

/// Mean absolute per-byte difference between two RGBA frames.
fn frame_diff(a: &[u8], b: &[u8]) -> f64 {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0.0;
    }
    let sum: u64 = (0..n).map(|i| u64::from(a[i].abs_diff(b[i]))).sum();
    sum as f64 / n as f64
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: repro_smb3 <rom-path> <out-dir>");
        std::process::exit(2);
    }
    let rom_path = &args[1];
    let out_dir = Path::new(&args[2]);
    std::fs::create_dir_all(out_dir).expect("create out dir");

    // Optional `--state <file>`: restore a save-state (e.g. one taken in the app
    // with F1 while Mario is mid-run and flickering) and capture straight from
    // there, skipping the scripted path to 1-1.
    let state_path = args
        .iter()
        .position(|a| a == "--state")
        .and_then(|i| args.get(i + 1).cloned());

    // Optional `--movie <file.rnm>`: the most faithful repro — replay the exact
    // TAS the player recorded (power-on -> title -> map -> into 1-1, with Mario
    // running at the end). Replays the whole input stream, dumps a filmstrip so
    // we can confirm we reach 1-1, captures the FINAL stretch of the movie (what
    // the player actually saw) for the parity metric, then hands off to the
    // running-stretch capture which keeps Mario moving past the movie's end.
    let movie_path = args
        .iter()
        .position(|a| a == "--movie")
        .and_then(|i| args.get(i + 1).cloned());

    let bytes = std::fs::read(rom_path).unwrap_or_else(|e| panic!("read {rom_path}: {e}"));
    let mut nes = Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("parse {rom_path}: {e}"));

    // Scripted path to World 1-1. Timings are generous taps with idle gaps; the
    // filmstrip lets us verify/refine where we land.
    // (frame budget, buttons-held-for-first-8-frames-of-each-tap)
    // Path to World 1-1 (per the player's known sequence): wait out the title +
    // the long "WORLD 1 MARIO x4" intro card, then on the map press RIGHT once,
    // UP once (to land on the Stage 1 node), then A to enter the level.
    let script: &[(u64, Buttons)] = &[
        (220, Buttons::empty()), // title / curtain demo
        (16, Buttons::START),    // tap Start -> "1/2 PLAYER GAME" select menu
        (80, Buttons::empty()),  // menu appears
        (16, Buttons::START),    // tap Start -> confirm 1-player -> world map + card
        (560, Buttons::empty()), // the intro card auto-clears (~500+ frames) and
        // absorbs inputs while up; wait it out fully before navigating.
        (8, Buttons::RIGHT),    // map: move right one node
        (40, Buttons::empty()), // auto-walk completes
        (8, Buttons::UP),       // map: move up one node -> land on Stage 1
        (40, Buttons::empty()), // auto-walk completes
        (8, Buttons::A),        // enter Stage 1 (World 1-1)
        (40, Buttons::empty()),
        (8, Buttons::B),         // fallback: B enters if A opened the item menu
        (260, Buttons::empty()), // 1-1 loads + Mario idle
    ];

    let mut frame = 0u64;
    let dump_every = 15u64;
    if let Some(mp) = &movie_path {
        // replay_movie's return (the next free frame index) is only needed by
        // the running-stretch capture, which the movie branch doesn't run.
        let _ = replay_movie(&mut nes, mp, out_dir, dump_every);
        // Discriminator: hold NO input and watch whether the per-frame Mario
        // blink STOPS (an invincibility/flash timer expiring => authentic
        // game-driven flicker) or persists indefinitely (suspicious). Prints a
        // present/absent bitstring of Mario's sprite over a long idle window.
        observe_idle(&mut nes, 240);
        return;
    } else if let Some(sp) = &state_path {
        let blob = std::fs::read(sp).unwrap_or_else(|e| panic!("read state {sp}: {e}"));
        nes.restore(&blob)
            .unwrap_or_else(|e| panic!("restore state {sp}: {e:?}"));
        eprintln!("restored save-state from {sp}; capturing from there");
    } else {
        for &(dur, held) in script {
            for i in 0..dur {
                // Hold the button only for the first 8 frames of a tap, then release.
                let buttons = if i < 8 { held } else { Buttons::empty() };
                nes.set_buttons(0, buttons);
                let fb = nes.run_frame();
                if frame % dump_every == 0 {
                    write_png(&out_dir.join(format!("f{frame:04}.png")), fb);
                }
                frame += 1;
            }
        }
    }

    capture_window(&mut nes, out_dir, frame);
}

/// Replay a recorded `.rnm` movie (the player's exact path into 1-1) and report
/// the parity-oscillation metric over its FINAL stretch — the frames where the
/// player saw Mario flicker. Returns the next free frame index so the caller's
/// running-stretch capture continues the numbering.
fn replay_movie(nes: &mut Nes, movie_path: &str, out_dir: &Path, dump_every: u64) -> u64 {
    let blob = std::fs::read(movie_path).unwrap_or_else(|e| panic!("read movie {movie_path}: {e}"));
    let movie = Movie::deserialize(&blob).unwrap_or_else(|e| panic!("parse movie: {e}"));
    movie
        .seek_to_start(nes)
        .unwrap_or_else(|e| panic!("seek movie start (ROM mismatch?): {e}"));
    let total = movie.len();
    eprintln!(
        "replaying movie {movie_path}: {total} frames (region {:?})",
        movie.region
    );

    // Capture the final stretch for the parity metric + a zoomed Mario crop —
    // this is exactly the input the player held, so it is the faithful repro.
    let tail = 60usize;
    let tail_start = total.saturating_sub(tail);
    let (cx, cy, cw, ch, zoom) = (88usize, 96usize, 80usize, 112usize, 3usize);
    let mut frames: Vec<Vec<u8>> = Vec::new();
    let mut crops: Vec<Vec<u8>> = Vec::new();

    let mut player = MoviePlayer::new(&movie);
    let mut frame = 0u64;
    while player.apply_next(nes) {
        let fb = nes.run_frame();
        if frame % dump_every == 0 {
            write_png(&out_dir.join(format!("f{frame:04}.png")), fb);
        }
        let idx = frame as usize;
        if idx >= tail_start {
            let owned = fb.to_vec();
            // OAM diagnostic: is the GAME hiding Mario's sprites on alternate
            // frames (authentic damage/invincibility flash) or keeping them
            // on-screen while the PPU drops them (a render bug)? Each sprite is
            // 4 bytes [Y, tile, attr, X]; Y >= 0xEF is off the visible area.
            let oam = nes.oam();
            // SMB3 uses 8x16 sprites; height matters for per-scanline coverage.
            let spr_h: i32 = 16;
            // Per-scanline sprite coverage count (visible 0..240). The PPU renders
            // at most 8 sprites per scanline (in OAM order); a 9th+ is dropped.
            let mut per_line = [0u32; 240];
            let mut onscreen = 0u32;
            let mut band = 0u32; // sprites in Mario's vertical band (Y 40..96)
                                 // On-screen sprites only, in OAM order, so we can see which compete on
                                 // Mario's scanline and in what order (the drop is order-dependent).
            let mut onlist: Vec<(usize, u8, u8, u8)> = Vec::new();
            for (i, s) in oam.chunks_exact(4).enumerate() {
                let (y, tile, _attr, x) = (s[0], s[1], s[2], s[3]);
                if y < 0xEF {
                    onscreen += 1;
                    if (40..96).contains(&y) {
                        band += 1;
                    }
                    onlist.push((i, y, tile, x));
                    let y0 = i32::from(y) + 1; // sprite shows on scanline Y+1..Y+h
                    for ln in y0..(y0 + spr_h) {
                        if (0..240).contains(&ln) {
                            per_line[ln as usize] += 1;
                        }
                    }
                }
            }
            let (max_line, max_cnt) = per_line
                .iter()
                .enumerate()
                .max_by_key(|(_, c)| **c)
                .map_or((0, 0), |(l, c)| (l, *c));
            // Coverage on Mario's mid scanline (~Y 64) — the 8-limit test.
            let mario_line = 64usize;
            let mario_cov = per_line[mario_line];
            let k = idx - tail_start;
            let list: String = onlist
                .iter()
                .map(|(i, y, t, x)| format!("s{i}:Y{y:02X}T{t:02X}X{x:02X}"))
                .collect::<Vec<_>>()
                .join(" ");
            println!(
                "[oam] mov{k:02} f{idx}: on={onscreen} band={band} maxline={max_line}({max_cnt}) mario_y64_cov={mario_cov} | {list}"
            );
            let crop = crop_zoom(&owned, cx, cy, cw, ch, zoom);
            write_png(&out_dir.join(format!("mov{k:02}.png")), &owned);
            write_png_sized(
                &out_dir.join(format!("movcrop{k:02}.png")),
                &crop,
                (cw * zoom) as u32,
                (ch * zoom) as u32,
            );
            crops.push(crop);
            frames.push(owned);
        }
        frame += 1;
    }

    eprintln!(
        "--- movie tail (last {} frames, what the player saw) ---",
        frames.len()
    );
    let (cadj, cskip, cratio) = parity_ratio(&crops);
    println!("[movie tail] Mario-band crop: adj={cadj:.4} skip={cskip:.4} ratio={cratio:.2}");
    let (adj, skip, ratio) = parity_ratio(&frames);
    println!("[movie tail] full-frame: adj={adj:.4} skip={skip:.4} ratio={ratio:.2}");
    frame
}

/// Hold no input for `n` frames and emit a present/absent bitstring for
/// Mario's centre sprite pair (X 0x78..0x98, Y 0x40..0x90). `#` = present,
/// `.` = absent. If the flicker is a damage/invincibility flash it stops after
/// the timer runs out; if it never stops while Mario is idle, that's a flag.
fn observe_idle(nes: &mut Nes, n: usize) {
    eprintln!("--- idle observation ({n} frames, no input) ---");
    let mut bits = String::with_capacity(n);
    let mut present_run = 0u32;
    let mut last_present_frame = 0usize;
    for f in 0..n {
        nes.set_buttons(0, Buttons::empty());
        nes.run_frame();
        let oam = nes.oam();
        // Mario = the side-by-side centre sprite pair the game draws for small
        // Mario; detect "any on-screen sprite in his centre box".
        // Precise detector: small-Mario's body tiles (0x05/0x07) on-screen,
        // not a loose box (which catches coins/enemies/projectiles as noise).
        let mario_present = oam.chunks_exact(4).any(|s| {
            let (y, tile) = (s[0], s[1]);
            y < 0xEF && matches!(tile, 0x05 | 0x07)
        });
        if mario_present {
            present_run += 1;
            last_present_frame = f;
        }
        bits.push(if mario_present { '#' } else { '.' });
    }
    println!("[idle] Mario presence over {n} idle frames:");
    println!("[idle] {bits}");
    let absent = n - present_run as usize;
    println!(
        "[idle] present={present_run} absent={absent} last_present_at_frame={last_present_frame}"
    );
    // Tail check: is he steady (all-present) by the end?
    let tail = &bits[bits.len().saturating_sub(40)..];
    let tail_absent = tail.chars().filter(|&c| c == '.').count();
    println!("[idle] last-40-frames absent-count={tail_absent} (0 => flicker stopped => authentic flash)");

    // DIAGNOSTIC: per-frame correlation of Mario's OAM presence against the
    // game's sprite-DMA pipeline. Why does the GAME drop Mario from OAM?
    diag_idle(nes, n);
}

/// Mario's body tiles in the OAM/RAM sprite buffer (small Mario, idle).
#[cfg(feature = "debug-hooks")]
fn mario_in_buf(buf: &[u8]) -> bool {
    buf.chunks_exact(4).any(|s| {
        let (y, tile) = (s[0], s[1]);
        y < 0xEF && matches!(tile, 0x05 | 0x07)
    })
}

/// Per-frame instrumentation: correlate Mario-present (OAM) with the sprite
/// DMA pipeline. Reads the CPU RAM sprite buffer ($0200-$02FF) BEFORE the NMI
/// runs (i.e. as it stands at the end of the previous frame), the $2003/$4014
/// write events during the frame, the OAMADDR at vblank, the NMI line, the
/// MMC3 IRQ state, and the CPU cycle. The key discriminator: is Mario in the
/// RAM sprite buffer but absent from OAM (DMA/PPU bug) or absent from the RAM
/// buffer too (game-logic / CPU-timing bug upstream)?
#[cfg(feature = "debug-hooks")]
fn diag_idle(nes: &mut Nes, n: usize) {
    use rustynes_core::EventKind;
    eprintln!("--- diag_idle ({n} frames) ---");
    nes.set_event_logging(true);
    nes.set_access_logging(true);
    println!("[diag] f  oam ram | n2003 n4014 (v4014 dot4014) | nmi mmc3irq cpu_cyc");
    let mut prev_oam = false;
    for f in 0..n {
        nes.set_buttons(0, Buttons::empty());

        // The RAM sprite buffer as it stands at the START of this frame (built
        // by the PREVIOUS frame's game logic; the NMI at the top of THIS frame
        // DMAs it into OAM). $0200-$02FF on page 2 is SMB3's shadow OAM.
        let mut ram_buf = [0u8; 256];
        for (i, b) in ram_buf.iter_mut().enumerate() {
            *b = nes.cpu_bus_peek(0x0200 + i as u16);
        }
        let ram_mario = mario_in_buf(&ram_buf);

        nes.run_frame();

        let oam = nes.oam();
        let oam_mario = mario_in_buf(&oam);

        // Count + summarise the sprite-DMA writes this frame.
        let mut n_2003 = 0u32;
        let mut n_4014 = 0u32;
        let mut last_4014_dot: i32 = -1;
        let mut last_4014_sl: i32 = -1;
        // $2001 (PPUMASK) writes on a render line — the rendering-disable that
        // arms the OAM-row-corruption flag. Record their (scanline,dot).
        let mut mask_writes: Vec<(i16, u16)> = Vec::new();
        for e in nes.events() {
            match e.kind {
                EventKind::PpuWrite if e.addr == 0x2003 => n_2003 += 1,
                EventKind::PpuWrite if e.addr == 0x2001 => {
                    if (0..240).contains(&e.scanline) {
                        mask_writes.push((e.scanline, e.dot));
                    }
                }
                EventKind::ApuWrite if e.addr == 0x4014 => {
                    n_4014 += 1;
                    last_4014_dot = i32::from(e.dot);
                    last_4014_sl = i32::from(e.scanline);
                }
                _ => {}
            }
        }
        let mask_str: String = mask_writes
            .iter()
            .map(|(s, d)| format!("sl{s}d{d}"))
            .collect::<Vec<_>>()
            .join(",");

        // Source page written to $4014 + value written to $2003 (OAMADDR).
        let mut v_4014 = 0xFFu8;
        let mut v_2003 = 0xFFu8;
        for a in nes.accesses() {
            if a.write && a.addr == 0x4014 {
                v_4014 = a.value;
            }
            if a.write && a.addr == 0x2003 {
                v_2003 = a.value;
            }
        }

        let ppu = nes.ppu_snapshot();
        let cpu = nes.cpu_snapshot();
        let apu = nes.apu_snapshot();
        let _ = apu;
        let m = nes.mapper_info();
        let irq: String = m
            .irq_state
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(",");

        // Deep probe on a DROP frame: compare the RAM shadow OAM against the
        // PPU OAM byte-for-byte. If the DMA mis-aligned, OAM will be a SHIFTED
        // copy of RAM (off-by-N) rather than a faithful copy. Report the shift.
        if ram_mario && !oam_mario {
            // Find where Mario's tile pair (0x05 at some +1 offset) sits in RAM.
            let ram_idx = ram_buf
                .chunks_exact(4)
                .position(|s| s[0] < 0xEF && matches!(s[1], 0x05 | 0x07));
            // Detect a global byte-shift: best offset k minimising sum|oam[i]-ram[i-k]|.
            let mut best_k = 0i32;
            let mut best_score = u64::MAX;
            for k in -4i32..=4 {
                let mut score = 0u64;
                for i in 0..256i32 {
                    let j = i - k;
                    if (0..256).contains(&j) {
                        score += u64::from(oam[i as usize].abs_diff(ram_buf[j as usize]));
                    }
                }
                if score < best_score {
                    best_score = score;
                    best_k = k;
                }
            }
            // What is at the Mario slot in OAM (where RAM has him)?
            let at = ram_idx.map_or((0xFFu8, 0xFFu8), |r| (oam[r * 4], oam[r * 4 + 1]));
            // Also read the shadow OAM at the source page $4014 wrote (the page
            // the DMA actually copies from), in case it is NOT $0200.
            let src_base = u16::from(v_4014) << 8;
            let mut src_buf = [0u8; 256];
            for (i, b) in src_buf.iter_mut().enumerate() {
                *b = nes.cpu_bus_peek(src_base + i as u16);
            }
            let src_mario = mario_in_buf(&src_buf);
            // Compare OAM to the SOURCE page (post-frame). If OAM == source page
            // now, the buffer was simply rebuilt after the DMA already ran.
            let mut eq_src = 0u32;
            let mut diffs: Vec<String> = Vec::new();
            for i in 0..256 {
                if oam[i] == src_buf[i] {
                    eq_src += 1;
                } else {
                    diffs.push(format!("[{i}]oam{:02X}!=src{:02X}", oam[i], src_buf[i]));
                }
            }
            if f <= 12 {
                println!("[probe-diff] f{f}: {}", diffs.join(" "));
            }
            println!(
                "[probe] f{f}: v4014={v_4014:02X} v2003={v_2003:02X} ram_mario_slot={ram_idx:?} oam_at_slot=(Y{:02X},T{:02X}) k={best_k} src_page_mario={src_mario} oam==srcpage:{eq_src}/256 score{best_score}",
                at.0, at.1
            );
        }

        let flag = if oam_mario == prev_oam { "" } else { "<--flip" };
        // Highlight the smoking gun: Mario in RAM buffer but not in OAM.
        let drop = if ram_mario && !oam_mario {
            "  RAM-has-Mario-but-OAM-DOESNT"
        } else if !ram_mario {
            "  RAM-buffer-also-missing-Mario"
        } else {
            ""
        };
        println!(
            "[diag] {f:3} {o} {r} | {n_2003} {n_4014} (sl{last_4014_sl} dot{last_4014_dot}) | nmi{nmi} [{irq}] cyc{cyc} mask[{mask_str}]{flag}{drop}",
            o = u8::from(oam_mario),
            r = u8::from(ram_mario),
            nmi = u8::from(ppu.nmi_line),
            cyc = cpu.cycles,
        );
        prev_oam = oam_mario;
    }
    nes.set_event_logging(false);
}

#[cfg(not(feature = "debug-hooks"))]
fn diag_idle(_nes: &mut Nes, _n: usize) {
    eprintln!("[diag] build with --features debug-hooks for per-frame DMA correlation");
}

/// Parity-oscillation metric over a frame sequence: mean adjacent (f vs f-1,
/// opposite parity) vs skip-one (f vs f-2, same parity). A per-other-frame
/// flicker pushes the adj/skip ratio well above 1.
fn parity_ratio(frames: &[Vec<u8>]) -> (f64, f64, f64) {
    let adj: f64 = (1..frames.len())
        .map(|i| frame_diff(&frames[i], &frames[i - 1]))
        .sum::<f64>()
        / (frames.len().saturating_sub(1)).max(1) as f64;
    let skip: f64 = (2..frames.len())
        .map(|i| frame_diff(&frames[i], &frames[i - 2]))
        .sum::<f64>()
        / (frames.len().saturating_sub(2)).max(1) as f64;
    (adj, skip, if skip > 0.0 { adj / skip } else { 0.0 })
}

/// Run Mario right (B = run) for a window — the flicker only shows while he's
/// moving — dumping each full frame + a zoomed Mario-band crop, and report the
/// parity metric for both the full frame and the crop band.
fn capture_window(nes: &mut Nes, out_dir: &Path, start_frame: u64) {
    eprintln!("--- capture window (frame {start_frame}.., running right) ---");
    // Centre band where SMB3 camera-locks a running Mario (~screen-centre X).
    let (cx, cy, cw, ch, zoom) = (88usize, 96usize, 80usize, 112usize, 3usize);
    let mut frames: Vec<Vec<u8>> = Vec::new();
    let mut crops: Vec<Vec<u8>> = Vec::new();
    for k in 0..40 {
        nes.set_buttons(0, Buttons::RIGHT | Buttons::B);
        let fb = nes.run_frame().to_vec();
        write_png(&out_dir.join(format!("win{k:02}.png")), &fb);
        let crop = crop_zoom(&fb, cx, cy, cw, ch, zoom);
        write_png_sized(
            &out_dir.join(format!("crop{k:02}.png")),
            &crop,
            (cw * zoom) as u32,
            (ch * zoom) as u32,
        );
        crops.push(crop);
        frames.push(fb);
    }
    let (cadj, cskip, cratio) = parity_ratio(&crops);
    println!("Mario-band crop: adj={cadj:.4} skip={cskip:.4} ratio={cratio:.2}");
    let (adj, skip, ratio) = parity_ratio(&frames);
    println!("mean adjacent-frame diff   (f vs f-1): {adj:.4}");
    println!("mean skip-one-frame diff   (f vs f-2): {skip:.4}");
    println!("parity-oscillation ratio (adj/skip): {ratio:.2}  (>~2 suggests a 2-frame flicker)");
    eprintln!("wrote filmstrip + window PNGs to {}", out_dir.display());
}
