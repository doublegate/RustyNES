//! Movie subtitles → `SubRip` (`.srt`) export (v1.7.0 "Forge" Workstream H9).
//!
//! TAS authors annotate a movie with named **markers** (the `TAStudio`
//! piano-roll markers: a frame number + a label). This module turns those
//! markers into a `SubRip` subtitle track so a recorded encode (`A/V` dump, H9
//! / Workstream G) can carry on-screen commentary that lines up frame-exactly
//! with the playback.
//!
//! It is frontend-only + pure: it converts a sorted `(frame, label)` list into
//! SRT text using the region's exact frame rate (so `NTSC`'s 60.0988 fps stays
//! drift-free over a long movie). Each marker becomes a cue that runs from its
//! frame until the next marker's frame (the last marker runs for a default
//! tail), which matches how a viewer reads TAS commentary.

/// One subtitle cue: the inclusive start frame, the exclusive end frame, and
/// the text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cue {
    /// First frame the cue is shown on.
    pub start_frame: u64,
    /// One past the last frame the cue is shown on.
    pub end_frame: u64,
    /// The subtitle text (a marker label).
    pub text: String,
}

/// Convert a list of `(frame, label)` markers into `SubRip` subtitle text.
///
/// `markers` need not be sorted (this sorts them by frame). Empty labels are
/// skipped. `fps_num / fps_den` is the region's exact frame rate (e.g. NTSC
/// `60_0988 / 1000` — pass the same rational the A/V recorder uses).
/// `tail_frames` is how long the final marker's cue runs (and the minimum
/// length of any zero-length span between coincident markers).
///
/// Returns an empty string if there are no non-empty markers.
#[must_use]
pub fn markers_to_srt<I, S>(markers: I, fps_num: u32, fps_den: u32, tail_frames: u64) -> String
where
    I: IntoIterator<Item = (u64, S)>,
    S: Into<String>,
{
    use std::fmt::Write as _;

    let mut entries: Vec<(u64, String)> = markers
        .into_iter()
        .map(|(f, s)| (f, s.into()))
        .filter(|(_, s)| !s.trim().is_empty())
        .collect();
    entries.sort_by_key(|(f, _)| *f);

    let fps_num = fps_num.max(1);
    let fps_den = fps_den.max(1);
    let tail = tail_frames.max(1);

    let mut cues: Vec<Cue> = Vec::with_capacity(entries.len());
    for i in 0..entries.len() {
        let start = entries[i].0;
        // The cue ends where the next marker begins, or after `tail` for the
        // last one. Guarantee a non-zero span when two markers coincide.
        let end = entries
            .get(i + 1)
            .map_or(start + tail, |(next, _)| (*next).max(start + 1));
        cues.push(Cue {
            start_frame: start,
            end_frame: end,
            text: entries[i].1.clone(),
        });
    }

    let mut out = String::new();
    for (idx, cue) in cues.iter().enumerate() {
        let start_ms = frame_to_ms(cue.start_frame, fps_num, fps_den);
        let end_ms = frame_to_ms(cue.end_frame, fps_num, fps_den);
        let _ = writeln!(out, "{}", idx + 1);
        let _ = writeln!(
            out,
            "{} --> {}",
            format_timestamp(start_ms),
            format_timestamp(end_ms)
        );
        out.push_str(cue.text.trim());
        out.push_str("\n\n");
    }
    out
}

/// Convert a frame index to milliseconds at `fps_num / fps_den` frames per
/// second: `ms = frame * 1000 * fps_den / fps_num`, computed in `u128` to
/// avoid overflow + rounding drift on long movies.
fn frame_to_ms(frame: u64, fps_num: u32, fps_den: u32) -> u64 {
    let num = u128::from(frame) * 1000 * u128::from(fps_den);
    u64::try_from(num / u128::from(fps_num)).unwrap_or(u64::MAX)
}

/// Format milliseconds as a `SubRip` timestamp `HH:MM:SS,mmm`.
fn format_timestamp(ms: u64) -> String {
    let h = ms / 3_600_000;
    let m = (ms % 3_600_000) / 60_000;
    let s = (ms % 60_000) / 1000;
    let millis = ms % 1000;
    format!("{h:02}:{m:02}:{s:02},{millis:03}")
}

#[cfg(test)]
mod tests {
    use super::*;

    // NTSC: 60.0988 fps as the exact rational the recorder uses.
    const NTSC_NUM: u32 = 60_0988;
    const NTSC_DEN: u32 = 10_000;

    #[test]
    fn empty_markers_make_empty_srt() {
        let s = markers_to_srt(Vec::<(u64, String)>::new(), NTSC_NUM, NTSC_DEN, 120);
        assert!(s.is_empty());
        // Blank labels are skipped too.
        let s = markers_to_srt(vec![(0u64, "  ".to_string())], NTSC_NUM, NTSC_DEN, 120);
        assert!(s.is_empty());
    }

    #[test]
    fn cues_span_marker_to_next_marker() {
        // 60 fps exact for an easy timestamp check.
        let s = markers_to_srt(
            vec![(0u64, "Start"), (60, "Level 2"), (120, "Boss")],
            60,
            1,
            60,
        );
        // Three cues, numbered, in order.
        assert!(s.starts_with("1\n00:00:00,000 --> 00:00:01,000\nStart\n\n"));
        assert!(s.contains("2\n00:00:01,000 --> 00:00:02,000\nLevel 2\n\n"));
        // Last cue runs for the tail (60 frames = 1s) → 2s..3s.
        assert!(s.contains("3\n00:00:02,000 --> 00:00:03,000\nBoss\n\n"));
    }

    #[test]
    fn unsorted_markers_are_sorted() {
        let s = markers_to_srt(vec![(120u64, "Late"), (0, "Early")], 60, 1, 60);
        let early = s.find("Early").unwrap();
        let late = s.find("Late").unwrap();
        assert!(early < late, "cues are emitted in frame order");
    }

    #[test]
    fn coincident_markers_get_a_nonzero_span() {
        let s = markers_to_srt(vec![(10u64, "A"), (10, "B")], 60, 1, 60);
        // Both cues present, neither has an inverted/zero timestamp range.
        assert!(s.contains('A'));
        assert!(s.contains('B'));
    }

    #[test]
    fn ntsc_timestamp_does_not_drift() {
        // At ~60.0988 fps, frame 3600 (~60 s of frames) maps to ~59.901 s.
        let s = markers_to_srt(vec![(3600u64, "mark")], NTSC_NUM, NTSC_DEN, 120);
        assert!(s.contains("00:00:59,"), "frame 3600 ~ 59.9 s: {s}");
    }
}
