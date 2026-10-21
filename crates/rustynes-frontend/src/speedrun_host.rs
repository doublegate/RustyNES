//! v3.5.0 (`T-SPEEDRUN-MEMIF`) — the frontend's speedrun timer and
//! memory-export publisher, shared by the desktop window (`--splits`,
//! `--memif`) and the headless `rustynes speedrun` command.
//!
//! The timer itself is `rustynes_core::speedrun` (`no_std`, deterministic); the
//! block is `crate::memif` (behind the `memif` feature). This module only
//! glues them to a running console: it creates the timer for the loaded
//! cartridge's region, re-creates it when a different ROM is loaded (a new
//! game is a new run), feeds it once per frame, publishes the block, and writes
//! the result document when a run finishes.
//!
//! Everything here READS the console, after its frame, through the
//! side-effect-free peek. The emulation thread calls
//! [`SpeedrunHost::after_frame`] at the point where the frame is final (after
//! run-ahead has rolled back to the persistent timeline), so what is timed and
//! published is exactly what a replay of the session reproduces.

use std::path::PathBuf;

use rustynes_core::Nes;
use rustynes_core::speedrun::{SpeedrunTimer, SplitsDefinition, TimerEvent};

/// What the user asked for on the command line.
#[derive(Clone, Debug, Default)]
pub struct SpeedrunOptions {
    /// `--splits <FILE>`: the splits definition to time against.
    pub splits: Option<PathBuf>,
    /// `--splits-json <FILE>`: where a finished run's result is written.
    pub splits_json: Option<PathBuf>,
    /// `--memif [NAME]`: publish the memory-export block under this name.
    pub memif: Option<String>,
}

impl SpeedrunOptions {
    /// Whether anything was asked for.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.splits.is_some() || self.memif.is_some()
    }
}

/// The per-session speedrun state.
pub struct SpeedrunHost {
    def: Option<SplitsDefinition>,
    timer: Option<SpeedrunTimer>,
    /// The ROM the timer was made for, so a new game starts a new timer.
    rom: Option<[u8; 32]>,
    json_out: Option<PathBuf>,
    #[cfg(feature = "memif")]
    memif: Option<crate::memif::MemoryExport>,
}

impl std::fmt::Debug for SpeedrunHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpeedrunHost")
            .field("timer", &self.timer.as_ref().map(SpeedrunTimer::state))
            .finish_non_exhaustive()
    }
}

impl SpeedrunHost {
    /// Build the host from the options: parse the splits file, create the
    /// block.
    ///
    /// # Errors
    ///
    /// A splits file that cannot be read or parsed (with its line), a block
    /// that cannot be created, or `--memif` on a build without the `memif`
    /// feature. Each message names the file or the option.
    pub fn new(opts: &SpeedrunOptions) -> Result<Self, String> {
        let def = match &opts.splits {
            None => None,
            Some(path) => {
                let text = std::fs::read_to_string(path)
                    .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
                Some(
                    SplitsDefinition::parse(&text)
                        .map_err(|e| format!("{}: {e}", path.display()))?,
                )
            }
        };
        #[cfg(feature = "memif")]
        let memif = match &opts.memif {
            None => None,
            Some(name) => Some(
                crate::memif::MemoryExport::create(name)
                    .map_err(|e| format!("cannot create the memory-export block {name:?}: {e}"))?,
            ),
        };
        #[cfg(not(feature = "memif"))]
        if opts.memif.is_some() {
            return Err(
                "--memif needs a build with the `memif` feature (it is part of `full`)".to_string(),
            );
        }
        Ok(Self {
            def,
            timer: None,
            rom: None,
            json_out: opts.splits_json.clone(),
            #[cfg(feature = "memif")]
            memif,
        })
    }

    /// The live timer, once a game is running and a splits file was given.
    #[must_use]
    pub const fn timer(&self) -> Option<&SpeedrunTimer> {
        self.timer.as_ref()
    }

    /// The block's name, when one is published.
    #[must_use]
    // `const` only in a build without `memif`, where the body is `None`.
    #[allow(clippy::missing_const_for_fn)]
    pub fn memif_name(&self) -> Option<&str> {
        #[cfg(feature = "memif")]
        {
            self.memif.as_ref().map(crate::memif::MemoryExport::name)
        }
        #[cfg(not(feature = "memif"))]
        {
            None
        }
    }

    /// Observe the end of a frame of `nes`: time it, publish it, and on a
    /// finish write the result. Returns the timer's event, if any.
    pub fn after_frame(&mut self, nes: &mut Nes) -> Option<TimerEvent> {
        let rom = *nes.rom_sha256();
        if self.rom != Some(rom)
            || self
                .timer
                .as_ref()
                .is_some_and(|t| t.region() != nes.region())
        {
            self.rom = Some(rom);
            self.timer = self
                .def
                .clone()
                .map(|d| SpeedrunTimer::new(d, nes.region()));
        }
        let event = self.timer.as_mut().and_then(|t| t.observe_nes(nes));
        #[cfg(feature = "memif")]
        if let Some(m) = self.memif.as_mut() {
            m.publish(nes, self.timer.as_ref());
        }
        if event == Some(TimerEvent::Finished)
            && let (Some(path), Some(t)) = (&self.json_out, &self.timer)
            && let Err(e) = crate::atomic_write::write_atomic(path, t.to_json().as_bytes())
        {
            eprintln!("rustynes: cannot write {}: {e}", path.display());
        }
        event
    }

    /// Publish "no game loaded" (the console was unloaded).
    pub fn no_game(&mut self) {
        self.rom = None;
        self.timer = None;
        #[cfg(feature = "memif")]
        if let Some(m) = self.memif.as_mut() {
            m.publish_empty();
        }
    }
}

/// One line describing `event`, for a console or a log.
#[must_use]
pub fn describe(event: TimerEvent, timer: &SpeedrunTimer, nes: &Nes) -> String {
    let at = format!(
        "frame {} master clock {}",
        nes.frame(),
        nes.cpu().master_clock()
    );
    let secs = |c| rustynes_core::speedrun::cycles_to_seconds(c, timer.region());
    match event {
        TimerEvent::Started => format!("start   {at}"),
        TimerEvent::Split(i) => {
            let e = timer.splits()[i].elapsed;
            format!(
                "split   {at}  {:<24} {e} cycles ({:.3} s)",
                timer.definition().splits[i].name,
                secs(e)
            )
        }
        TimerEvent::Finished => {
            let e = timer.elapsed();
            let last = timer
                .definition()
                .splits
                .last()
                .map_or("", |s| s.name.as_str());
            format!("finish  {at}  {last:<24} {e} cycles ({:.3} s)", secs(e))
        }
        TimerEvent::Reset => format!("reset   {at}"),
        TimerEvent::Discontinuity => format!("discontinuity {at} (the run is abandoned)"),
    }
}
