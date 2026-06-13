//! v2.8.0 — opt-in performance logging (the Perf panel's "Logging" checkbox).
//!
//! While enabled, the app writes one CSV file per (session × ROM) under
//! `perf-logs/` in the current working directory (the project root when run
//! via `cargo run`): a `#`-commented header capturing the game + the exact
//! configuration it ran under, then one data row per second sampling the
//! same [`PerfView`] the Performance panel renders (produced / presented /
//! produce-cost interval stats, pacer anomaly counters, audio-queue health,
//! GPU pass time, the active pacing regime and present mode).
//!
//! Default OFF; native-only (file I/O). The checkbox is session state, not
//! config — every launch starts with logging disabled. Loading a different
//! ROM while logging rotates to a fresh file (the header context changed).
//! `perf-logs/` is gitignored: the logs exist to be attached to / analyzed
//! in performance-tuning sessions, not committed.

use std::fs::{self, File};
use std::io::{BufWriter, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use web_time::Instant;

use crate::perf::PerfView;

/// Seconds between CSV rows. The perf rings hold a ~10 s window, so 1 Hz
/// sampling tracks their evolution without redundant rows (a 10-minute
/// soak is ~600 rows, a few tens of KiB).
const ROW_INTERVAL: Duration = Duration::from_secs(1);

/// Static run context written into the log header when a file is started.
/// Built by the app (it owns the config + gfx + ROM identity).
#[derive(Debug, Clone, Default)]
pub struct PerfLogContext {
    /// Display label of the loaded ROM (file stem; `"(no ROM)"` allowed).
    pub rom_label: String,
    /// Hex SHA-256 of the loaded ROM, when one is loaded.
    pub rom_sha256: Option<String>,
    /// `(key, value)` configuration pairs, one `# key = value` header line
    /// each (version, pacing mode, present mode, audio latency/DRC,
    /// run-ahead, monitor refresh, OS, ...).
    pub settings: Vec<(&'static str, String)>,
}

/// An open log file plus its row-rate limiter.
struct ActiveLog {
    w: BufWriter<File>,
    path: PathBuf,
    /// ROM label the header was written for — a change rotates the file.
    rom_label: String,
    started: Instant,
    last_row: Option<Instant>,
}

/// The logger owned by `App`; driven once per produced frame from the
/// housekeeping path (`sync` with the panel checkbox, then `record`).
#[derive(Default)]
pub struct PerfLogger {
    active: Option<ActiveLog>,
    /// Sticky error note (file create/write failure) shown in the panel.
    error: Option<String>,
    /// Row interval (overridable for tests).
    interval: Option<Duration>,
    /// Destination directory (default `perf-logs/` under the cwd;
    /// overridable for tests so they never touch the real tree).
    dir: Option<PathBuf>,
}

impl PerfLogger {
    /// Test constructor with a custom row interval + destination dir.
    #[cfg(test)]
    fn for_test(interval: Duration, dir: PathBuf) -> Self {
        Self {
            interval: Some(interval),
            dir: Some(dir),
            ..Self::default()
        }
    }

    /// True while a log file is open.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// Panel note: the destination path while active, or the sticky error.
    #[must_use]
    pub fn note(&self) -> Option<String> {
        if let Some(err) = &self.error {
            return Some(format!("log error: {err}"));
        }
        self.active
            .as_ref()
            .map(|a| format!("logging to {}", a.path.display()))
    }

    /// True when the next [`Self::sync`] with these arguments will start a
    /// new log file (enable edge, or ROM change while logging). The caller
    /// uses this to build the (allocating) [`PerfLogContext`] only when it
    /// is actually consumed — the steady state costs one `&str` compare.
    #[must_use]
    pub fn wants_start(&self, enabled: bool, rom_label: &str) -> bool {
        enabled
            && self.active.as_ref().map_or_else(
                || self.error.is_none(), // don't retry-create every frame
                |a| a.rom_label != rom_label,
            )
    }

    /// Reconcile with the panel checkbox. Starts a file on enable, closes
    /// it on disable, and rotates to a fresh file when the ROM changed
    /// (the header context is stale). `ctx` is only invoked when
    /// [`Self::wants_start`] holds.
    pub fn sync<F: FnOnce() -> PerfLogContext>(&mut self, enabled: bool, rom_label: &str, ctx: F) {
        if !enabled {
            self.stop();
            self.error = None;
            return;
        }
        if self.wants_start(enabled, rom_label) {
            self.stop();
            self.start(&ctx());
        }
    }

    /// Append a CSV row if the row interval elapsed. No-op while inactive.
    pub fn record(&mut self, view: &PerfView) {
        let interval = self.interval.unwrap_or(ROW_INTERVAL);
        let Some(a) = self.active.as_mut() else {
            return;
        };
        let now = Instant::now();
        if a.last_row.is_some_and(|t| now.duration_since(t) < interval) {
            return;
        }
        a.last_row = Some(now);
        let elapsed = now.duration_since(a.started).as_secs_f32();
        if let Err(e) = write_row(&mut a.w, elapsed, view) {
            self.error = Some(e.to_string());
            self.active = None;
        }
    }

    /// Close the current file (flushes via `BufWriter::drop`).
    pub fn stop(&mut self) {
        if let Some(mut a) = self.active.take() {
            let _ = a.w.flush();
        }
    }

    fn start(&mut self, ctx: &PerfLogContext) {
        let dir = self
            .dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("perf-logs"));
        match open_log_file(&dir, ctx) {
            Ok((w, path)) => {
                eprintln!("rustynes: perf logging to {}", path.display());
                self.active = Some(ActiveLog {
                    w,
                    path,
                    rom_label: ctx.rom_label.clone(),
                    started: Instant::now(),
                    last_row: None,
                });
                self.error = None;
            }
            Err(e) => {
                eprintln!("rustynes: failed to start perf log: {e}");
                self.error = Some(e.to_string());
            }
        }
    }
}

/// Create `<dir>/perf-<rom>-<utc>.csv` and write the header + CSV
/// column row.
fn open_log_file(dir: &Path, ctx: &PerfLogContext) -> std::io::Result<(BufWriter<File>, PathBuf)> {
    fs::create_dir_all(dir)?;
    let path = dir.join(format!(
        "perf-{}-{}.csv",
        sanitize_label(&ctx.rom_label),
        utc_stamp(SystemTime::now())
    ));
    let mut w = BufWriter::new(File::create(&path)?);
    writeln!(w, "# RustyNES v2 performance log")?;
    writeln!(w, "# rom = {}", ctx.rom_label)?;
    if let Some(sha) = &ctx.rom_sha256 {
        writeln!(w, "# rom_sha256 = {sha}")?;
    }
    for (k, v) in &ctx.settings {
        writeln!(w, "# {k} = {v}")?;
    }
    writeln!(w, "# started_utc = {}", utc_stamp(SystemTime::now()))?;
    writeln!(
        w,
        "# row interval ~{}s; stats over a ~10s window",
        ROW_INTERVAL.as_secs()
    )?;
    writeln!(
        w,
        "elapsed_s,fps,\
         produced_mean_ms,produced_p50_ms,produced_p95_ms,produced_p99_ms,produced_max_ms,\
         presented_mean_ms,presented_p50_ms,presented_p95_ms,presented_p99_ms,presented_max_ms,\
         cost_mean_ms,cost_p50_ms,cost_p95_ms,cost_p99_ms,cost_max_ms,\
         catchup_bursts,snap_forwards,\
         audio_queued_ms,audio_queued_samples,audio_sample_rate,underruns,overrun_dropped,\
         gpu_ms,pacing,present_mode"
    )?;
    Ok((w, path))
}

/// One CSV data row from a [`PerfView`] snapshot.
fn write_row(w: &mut BufWriter<File>, elapsed_s: f32, v: &PerfView) -> std::io::Result<()> {
    let fps = if v.produced.mean_ms > 0.0 {
        1000.0 / v.produced.mean_ms
    } else {
        0.0
    };
    let s = |st: &crate::perf::IntervalStats| {
        format!(
            "{:.3},{:.3},{:.3},{:.3},{:.3}",
            st.mean_ms, st.p50_ms, st.p95_ms, st.p99_ms, st.max_ms
        )
    };
    writeln!(
        w,
        "{elapsed_s:.1},{fps:.3},{},{},{},{},{},{:.2},{},{},{},{},{},{},{}",
        s(&v.produced),
        s(&v.presented),
        s(&v.produce_cost),
        v.catchup_bursts,
        v.snap_forwards,
        v.audio.queued_ms(),
        v.audio.queued_samples,
        v.audio.sample_rate,
        v.audio.underruns,
        v.audio.overrun_dropped,
        v.gpu_ms
            .map_or_else(|| "-".to_string(), |g| format!("{g:.3}")),
        csv_text(&v.pacing),
        csv_text(&v.present_mode),
    )
}

/// Filename-safe ROM label: alphanumerics + `-_` kept, everything else `_`,
/// capped at 48 chars.
fn sanitize_label(label: &str) -> String {
    let mut out: String = label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    out.truncate(48);
    if out.is_empty() {
        out.push_str("no-rom");
    }
    out
}

/// Free-text CSV field: strip commas/quotes/newlines so the row stays
/// machine-splittable without a quoting dialect.
fn csv_text(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            ',' | '"' | '\n' | '\r' => ';',
            c => c,
        })
        .collect()
}

/// `YYYYMMDD-HHMMSS` in UTC from a `SystemTime`, with no chrono dependency
/// (Howard Hinnant's `civil_from_days` algorithm for the date part).
fn utc_stamp(t: SystemTime) -> String {
    let secs = t
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let (y, m, d) = civil_from_days(days);
    let tod = secs % 86_400;
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        tod / 3600,
        (tod / 60) % 60,
        tod % 60
    )
}

/// Days-since-epoch (1970-01-01) to (year, month, day), proleptic Gregorian.
const fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097); // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perf::{AudioHealth, IntervalStats};

    #[test]
    fn civil_from_days_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1)); // leap year start
        assert_eq!(civil_from_days(19_782), (2024, 2, 29)); // leap day
        assert_eq!(civil_from_days(20_605), (2026, 6, 1));
    }

    #[test]
    fn utc_stamp_formats_epoch() {
        assert_eq!(
            utc_stamp(SystemTime::UNIX_EPOCH + Duration::from_secs(86_399)),
            "19700101-235959"
        );
    }

    #[test]
    fn sanitize_label_strips_specials() {
        assert_eq!(
            sanitize_label("Super Mario Bros. (U)"),
            "Super_Mario_Bros___U_"
        );
        assert_eq!(sanitize_label(""), "no-rom");
    }

    #[test]
    fn csv_text_strips_separators() {
        assert_eq!(
            csv_text("display-sync, fell back\n"),
            "display-sync; fell back;"
        );
    }

    fn view() -> PerfView {
        PerfView {
            produced: IntervalStats {
                count: 10,
                mean_ms: 16.64,
                p50_ms: 16.6,
                p95_ms: 16.7,
                p99_ms: 16.8,
                max_ms: 17.0,
            },
            audio: AudioHealth {
                queued_samples: 2880,
                sample_rate: 48_000,
                ..AudioHealth::default()
            },
            pacing: "display-sync".into(),
            present_mode: "Fifo".into(),
            ..PerfView::default()
        }
    }

    /// Full lifecycle: enable -> header + rows; ROM change -> rotation to a
    /// second file; disable -> closed. Runs in a temp cwd so the real
    /// project tree is untouched.
    #[test]
    fn logger_lifecycle_writes_header_and_rows() {
        let tmp = std::env::temp_dir().join(format!("rustynes-perflog-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();

        let mut lg = PerfLogger::for_test(Duration::ZERO, tmp.clone());
        let ctx = || PerfLogContext {
            rom_label: "smb".into(),
            rom_sha256: Some("ab".repeat(32)),
            settings: vec![("pacing_mode", "auto".into())],
        };
        lg.sync(true, "smb", ctx);
        assert!(lg.is_active());
        let path = lg.active.as_ref().unwrap().path.clone();
        lg.record(&view());
        lg.record(&view());

        // ROM change rotates to a new file.
        lg.sync(true, "kid_icarus", || PerfLogContext {
            rom_label: "kid_icarus".into(),
            ..PerfLogContext::default()
        });
        let path2 = lg.active.as_ref().unwrap().path.clone();
        assert_ne!(path, path2);
        lg.record(&view());
        lg.sync(false, "kid_icarus", PerfLogContext::default);
        assert!(!lg.is_active());

        let first = std::fs::read_to_string(&path).unwrap();
        assert!(first.contains("# rom = smb"));
        assert!(first.contains("# pacing_mode = auto"));
        assert!(first.contains("elapsed_s,fps,produced_mean_ms"));
        // 2 data rows with the expected fps + audio occupancy fields.
        let rows: Vec<&str> = first
            .lines()
            .filter(|l| !l.starts_with('#') && !l.starts_with("elapsed_s"))
            .collect();
        assert_eq!(rows.len(), 2);
        assert!(rows[0].contains(",60.096,")); // 1000 / 16.64
        assert!(rows[0].contains(",60.00,2880,48000,")); // queued_ms,samples,rate
        assert!(rows[0].ends_with(",display-sync,Fifo"));

        let second = std::fs::read_to_string(&path2).unwrap();
        assert!(second.contains("# rom = kid_icarus"));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
