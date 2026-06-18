//! v1.7.0 "Forge" Workstream C (C2) — per-address memory-access counter +
//! uninitialized-read detection (a Mesen2-`MemoryAccessCounter`-class engine).
//!
//! ## What it tracks
//!
//! For every CPU bus address (`$0000-$FFFF`) it keeps:
//!
//! - read / write / exec **counts** (saturating `u32`s),
//! - the **last-access stamp** (the CPU cycle count at which the address was
//!   last touched), and
//! - an **`UninitRead`** flag — set when an address is *read before it has ever
//!   been written* (the classic uninitialized-RAM bug-finder).
//!
//! Reads + writes come from the existing per-frame bus-access log
//! ([`rustynes_core::Nes::accesses`]); executes come from the per-frame exec log
//! ([`rustynes_core::Nes::exec_log`]). Both are part of the `debug-hooks`
//! per-frame log-replay model — the same machinery the Lua `onRead`/`onWrite`/
//! `onExec` callbacks and the Watch panel use.
//!
//! ## Output-only
//!
//! The counter is a frontend-side array maintained purely by *reading* the
//! per-frame logs. It never feeds back into emulation, so the determinism
//! contract and `AccuracyCoin` are unaffected, and with the core's `debug-hooks`
//! feature OFF (the headless test/bench builds) the hot path is byte-identical.

use rustynes_core::Nes;

/// One address's accumulated access statistics.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AddressCounters {
    /// Number of CPU reads of this address (saturating).
    pub reads: u32,
    /// Number of CPU writes to this address (saturating).
    pub writes: u32,
    /// Number of times an instruction was *fetched* from this address
    /// (saturating). Sourced from the exec log.
    pub execs: u32,
    /// CPU cycle count at the most recent access (read/write/exec), or `0` if
    /// never accessed.
    pub last_stamp: u64,
    /// `true` once this address has been *read before it was ever written*
    /// (an uninitialized read). Sticky — once flagged it stays flagged.
    pub uninit_read: bool,
}

impl AddressCounters {
    /// Whether the address has ever been accessed at all.
    #[must_use]
    pub const fn touched(&self) -> bool {
        self.reads != 0 || self.writes != 0 || self.execs != 0
    }
}

/// The whole-address-space access counter. A boxed `[_; 0x10000]` so it lives on
/// the heap (256 KiB) rather than blowing the stack, indexed directly by CPU
/// address.
pub struct MemoryAccessCounter {
    /// Per-address counters, indexed by CPU address.
    counters: Box<[AddressCounters; 0x10000]>,
    /// Whether tracking is enabled (arms the core's access + exec logs).
    enabled: bool,
    /// Total uninitialized reads observed since the last reset (a quick
    /// summary for the UI).
    uninit_total: u32,
}

impl Default for MemoryAccessCounter {
    fn default() -> Self {
        Self {
            // `vec![..; N].try_into()` builds the array on the heap without a
            // 256 KiB stack temporary.
            counters: vec![AddressCounters::default(); 0x10000]
                .into_boxed_slice()
                .try_into()
                .expect("0x10000-element vec converts to the fixed array"),
            enabled: false,
            uninit_total: 0,
        }
    }
}

impl MemoryAccessCounter {
    /// Whether tracking is enabled.
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    /// Enable / disable tracking. Disabling stops further accumulation but keeps
    /// the gathered counts (the UI can still inspect them); re-enabling resumes.
    pub const fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
    }

    /// Whether the counter wants the core's per-frame access log armed.
    #[must_use]
    pub const fn wants_access_log(&self) -> bool {
        self.enabled
    }

    /// Whether the counter wants the core's per-frame exec log armed (for the
    /// per-address execute counts).
    #[must_use]
    pub const fn wants_exec_log(&self) -> bool {
        self.enabled
    }

    /// The counters for a single address.
    #[must_use]
    pub fn at(&self, addr: u16) -> &AddressCounters {
        &self.counters[addr as usize]
    }

    /// Total uninitialized reads observed since the last [`Self::reset`].
    #[must_use]
    pub const fn uninit_total(&self) -> u32 {
        self.uninit_total
    }

    /// Zero every counter + clear the uninitialized-read state (e.g. on
    /// reset / power-cycle / a user "clear" click).
    pub fn reset(&mut self) {
        for c in self.counters.iter_mut() {
            *c = AddressCounters::default();
        }
        self.uninit_total = 0;
    }

    /// Fold the just-finished frame's access + exec logs into the counters.
    /// Observational — `nes` is only read.
    ///
    /// The end-of-frame CPU cycle count stamps every access from this frame
    /// (the per-access cycle is not retained in the lightweight `AccessRec`, so
    /// the frame's end stamp is the available approximation — sufficient for the
    /// "recently touched" UI sort).
    pub fn replay_frame(&mut self, nes: &Nes) {
        if !self.enabled {
            return;
        }
        let stamp = nes.cpu_snapshot().cycles;

        // Reads + writes (with uninitialized-read detection).
        let accesses: Vec<(bool, u16)> = nes
            .accesses()
            .iter()
            .map(|acc| (acc.write, acc.addr))
            .collect();
        for (write, addr) in accesses {
            let c = &mut self.counters[addr as usize];
            if write {
                c.writes = c.writes.saturating_add(1);
            } else {
                // An uninitialized read: this address has been read but never
                // written (and isn't a known initialized region). Flag it once.
                if !c.uninit_read && c.writes == 0 && is_volatile_ram(addr) {
                    c.uninit_read = true;
                    self.uninit_total = self.uninit_total.saturating_add(1);
                }
                c.reads = c.reads.saturating_add(1);
            }
            c.last_stamp = stamp;
        }

        // Executes (instruction fetches) from the exec log.
        let exec: Vec<u16> = nes.exec_log().to_vec();
        for pc in exec {
            let c = &mut self.counters[pc as usize];
            c.execs = c.execs.saturating_add(1);
            c.last_stamp = stamp;
        }
    }
}

/// Whether an address lives in the volatile RAM regions where an
/// uninitialized read is a real bug (system work RAM `$0000-$1FFF` and the
/// cartridge WRAM window `$6000-$7FFF`). ROM / register / mapper space is
/// excluded — a "read before write" there is normal, not a bug.
const fn is_volatile_ram(addr: u16) -> bool {
    addr <= 0x1FFF || (addr >= 0x6000 && addr <= 0x7FFF)
}

/// Render the access-counter summary + per-address detail section inside the
/// Memory panel. Self-contained so it merges cleanly with the parallel panel
/// work. Reads the counter; toggling the checkbox flips tracking.
pub fn show_access_counter_section(
    ui: &mut egui::Ui,
    counter: &mut MemoryAccessCounter,
    origin: u16,
) {
    egui::CollapsingHeader::new("Access counters")
        .default_open(false)
        .show(ui, |ui| {
            let mut on = counter.enabled();
            if ui
                .checkbox(&mut on, "Track read/write/exec counts")
                .on_hover_text(
                    "Count CPU reads/writes/exec per address + flag \
                     uninitialized reads (arms the debug-hooks access + \
                     exec logs). Output-only.",
                )
                .changed()
            {
                counter.set_enabled(on);
            }
            ui.horizontal(|ui| {
                if ui.small_button("Reset counts").clicked() {
                    counter.reset();
                }
                ui.weak(format!("uninit reads: {}", counter.uninit_total()));
            });
            if !counter.enabled() {
                ui.weak("(tracking off — enable to collect counts)");
                return;
            }
            ui.separator();
            // Show the 16 rows currently in view (the panel's origin window) so
            // the detail tracks what the hex grid above is showing.
            egui::Grid::new("access-counter-grid")
                .striped(true)
                .show(ui, |ui| {
                    ui.monospace("addr");
                    ui.monospace("R");
                    ui.monospace("W");
                    ui.monospace("X");
                    ui.monospace("flags");
                    ui.end_row();
                    for row in 0..16u16 {
                        let addr = origin.wrapping_add(row);
                        let c = counter.at(addr);
                        if !c.touched() && !c.uninit_read {
                            continue;
                        }
                        ui.monospace(format!("${addr:04X}"));
                        ui.monospace(format!("{}", c.reads));
                        ui.monospace(format!("{}", c.writes));
                        ui.monospace(format!("{}", c.execs));
                        if c.uninit_read {
                            ui.colored_label(egui::Color32::from_rgb(0xF0, 0xB0, 0x40), "uninit");
                        } else {
                            ui.label("");
                        }
                        ui.end_row();
                    }
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_counter_is_empty_and_idle() {
        let c = MemoryAccessCounter::default();
        assert!(!c.enabled());
        assert!(!c.wants_access_log());
        assert!(!c.wants_exec_log());
        assert_eq!(c.uninit_total(), 0);
        assert!(!c.at(0x0000).touched());
        assert_eq!(*c.at(0x0200), AddressCounters::default());
    }

    #[test]
    fn enabling_arms_logs() {
        let mut c = MemoryAccessCounter::default();
        c.set_enabled(true);
        assert!(c.enabled());
        assert!(c.wants_access_log());
        assert!(c.wants_exec_log());
    }

    #[test]
    fn reset_clears_counts_and_uninit() {
        let mut c = MemoryAccessCounter::default();
        // Hand-poke a counter to simulate accumulation.
        c.counters[0x0010].reads = 5;
        c.counters[0x0010].uninit_read = true;
        c.uninit_total = 1;
        c.reset();
        assert_eq!(c.at(0x0010).reads, 0);
        assert!(!c.at(0x0010).uninit_read);
        assert_eq!(c.uninit_total(), 0);
    }

    #[test]
    fn volatile_ram_classification() {
        assert!(is_volatile_ram(0x0000), "work RAM start");
        assert!(is_volatile_ram(0x07FF), "work RAM mirror base end");
        assert!(is_volatile_ram(0x1FFF), "work RAM mirror end");
        assert!(is_volatile_ram(0x6000), "WRAM start");
        assert!(is_volatile_ram(0x7FFF), "WRAM end");
        assert!(!is_volatile_ram(0x2000), "PPU registers");
        assert!(!is_volatile_ram(0x8000), "PRG ROM");
        assert!(!is_volatile_ram(0x4016), "controller port");
    }

    #[test]
    fn touched_predicate() {
        let mut c = AddressCounters::default();
        assert!(!c.touched());
        c.reads = 1;
        assert!(c.touched());
        c.reads = 0;
        c.execs = 1;
        assert!(c.touched());
    }
}
