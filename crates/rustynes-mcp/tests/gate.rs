//! The plan's gate for `T-MCP-SERVER` (v3.5.0 plan, D33 rows):
//!
//! 1. **A scripted agent session's forks replay byte-identically from their
//!    recorded inputs.** [`scripted_session_forks_replay_byte_identically`]
//!    drives a session the way an agent would (observe, step, fork, run
//!    different inputs on each fork, diff), then replays every fork's
//!    exported `.rnm` on a fresh machine that sees only the ROM and the
//!    movie's bytes, and requires the save state, work RAM and framebuffer to
//!    match the live fork exactly.
//! 2. **A write outside the declared tools is refused** — see `refusals.rs`.
//!
//! Each assertion here was shown to fail against a mutation of the thing it
//! guards; `docs/mcp-server.md` § "Gate" lists the mutations and outcomes.

mod common;

use base64::Engine as _;
use common::{AFTER_READ_PC, LOOP_END_PC, call, host, ok, p1};
use rustynes_mcp::session::replay_rnm;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Export a source's movie through the tool and replay it OUTSIDE the server,
/// returning the replayed machine's save-state hash and the movie's frame count.
fn independent_replay(h: &rustynes_mcp::Host, source: &str) -> (String, u64) {
    let out = ok(h, "movie_export", json!({ "source": source }));
    let rnm = base64::engine::general_purpose::STANDARD
        .decode(out["rnm_base64"].as_str().expect("base64 movie"))
        .expect("valid base64");
    assert_eq!(
        out["sha256"],
        sha(&rnm),
        "the reported digest is of the bytes sent"
    );
    let nes = replay_rnm(&common::input_rom(), &rnm).expect("movie replays");
    (sha(&nes.snapshot()), out["frames"].as_u64().unwrap())
}

#[test]
fn scripted_session_forks_replay_byte_identically() {
    let h = host();

    // Turn 1: settle, look.
    ok(&h, "step_frames", json!({ "frames": 10 }));
    ok(&h, "observe", json!({}));

    // Turn 2: branch three experiments from the same point.
    let f1 = ok(&h, "fork_create", json!({}))["fork"]
        .as_str()
        .unwrap()
        .to_owned();
    let f2 = ok(&h, "fork_create", json!({}))["fork"]
        .as_str()
        .unwrap()
        .to_owned();
    let f3 = ok(&h, "fork_create", json!({ "from": "main" }))["fork"]
        .as_str()
        .unwrap()
        .to_owned();
    // A fork begins exactly where its origin is.
    for f in [&f1, &f2, &f3] {
        let d = ok(&h, "fork_diff", json!({ "a": "main", "b": f }));
        assert_eq!(d["identical_state"], true, "{f} did not start at main: {d}");
    }

    // Turn 3: different inputs on each.
    let r1 = ok(
        &h,
        "fork_run",
        json!({ "fork": f1, "inputs": [
            { "frames": 20, "p1": ["A"] },
            { "frames": 15, "p1": ["Right", "B"] },
            { "frames": 5 },
        ]}),
    );
    let r2 = ok(
        &h,
        "fork_run",
        json!({ "fork": f2, "inputs": [{ "frames": 40, "p1": ["Start"] }] }),
    );
    let r3 = ok(
        &h,
        "fork_run",
        json!({ "fork": f3, "inputs": [{ "frames": 40 }] }),
    );
    assert_eq!(r1["movie_frames"], 40);

    // Turn 4: a second run appends to f1's movie, and f4 branches from f1.
    ok(
        &h,
        "fork_run",
        json!({ "fork": f1, "inputs": [{ "frames": 7, "p1": ["Up"] }] }),
    );
    let f4 = ok(&h, "fork_create", json!({ "from": f1 }))["fork"]
        .as_str()
        .unwrap()
        .to_owned();
    ok(
        &h,
        "fork_run",
        json!({ "fork": f4, "inputs": [{ "frames": 12, "p1": ["Down"] }] }),
    );

    // The experiments diverged, visibly, in RAM and on screen ...
    let d = ok(&h, "fork_diff", json!({ "a": f1, "b": f2 }));
    assert_eq!(d["identical_state"], false);
    assert!(d["wram_bytes_differing"].as_u64().unwrap() > 0, "{d}");
    assert!(
        d["framebuffer_pixels_differing"].as_u64().unwrap() > 0,
        "{d}"
    );
    // ... and the running sum is where the program put it.
    let sum_f2 = ok(&h, "fork_diff", json!({ "a": f3, "b": f2 }));
    let listed = sum_f2["wram_differences"].as_array().unwrap();
    assert!(
        listed.iter().any(|e| e["addr"] == "$0012"),
        "the input sum at $12 differs: {sum_f2}"
    );
    assert_ne!(
        r2["state"]["snapshot_sha256"],
        r3["state"]["snapshot_sha256"]
    );

    // The gate: every fork replays from its recorded inputs, byte for byte —
    // through the tool, and independently of the server.
    for f in [&f1, &f2, &f3, &f4] {
        let v = ok(&h, "movie_verify", json!({ "source": f }));
        assert_eq!(v["identical"], true, "{f}: {v}");
        assert_eq!(v["mismatches"], json!([]));

        let live = v["live"]["snapshot_sha256"].as_str().unwrap().to_owned();
        let (replayed, frames) = independent_replay(&h, f);
        assert_eq!(
            replayed, live,
            "{f}: an outside replay reached another state"
        );
        assert!(frames > 0, "{f}: the movie recorded no frames");
    }
}

/// The same scripted session in two servers produces byte-identical movies:
/// the record depends on the inputs and nothing else (no clock, no ids).
#[test]
fn the_same_session_twice_records_the_same_bytes() {
    let run = || {
        let h = host();
        ok(
            &h,
            "step_frames",
            json!({ "frames": 5, "input": p1(&["A"]) }),
        );
        let f = ok(&h, "fork_create", json!({}))["fork"]
            .as_str()
            .unwrap()
            .to_owned();
        ok(
            &h,
            "fork_run",
            json!({ "fork": f, "inputs": [{ "frames": 30, "p1": ["Left", "A"] }] }),
        );
        let m = ok(&h, "movie_export", json!({ "source": f }));
        (m["sha256"].clone(), m["frames"].clone())
    };
    let (a, fa) = run();
    let (b, fb) = run();
    assert_eq!(a, b);
    assert_eq!(fa, 30);
    assert_eq!(fa, fb);
}

/// The main timeline's journal replays exactly across everything that is not
/// input: a RAM write, a mid-frame instruction step, a breakpoint stop, a
/// reset and an adopted fork each start a new segment, and the segment in
/// force afterwards still replays to the live state.
#[test]
fn the_main_journal_replays_across_every_non_input_change() {
    let h = host();
    let verify_main = |what: &str| {
        let v = ok(&h, "movie_verify", json!({ "source": "main" }));
        assert_eq!(v["identical"], true, "after {what}: {v}");
    };
    let segment = || {
        ok(&h, "info", json!({}))["journal"]["segment"]
            .as_u64()
            .unwrap()
    };

    ok(
        &h,
        "step_frames",
        json!({ "frames": 12, "input": p1(&["B"]) }),
    );
    verify_main("plain frames");
    let s0 = segment();

    ok(&h, "write_ram", json!({ "addr": 0x12, "bytes": [0x55] }));
    assert_eq!(segment(), s0 + 1, "a RAM write starts a segment");
    ok(
        &h,
        "step_frames",
        json!({ "frames": 9, "input": p1(&["A"]) }),
    );
    verify_main("write_ram then frames");

    // Step instructions through this frame's controller read (input A), then
    // change the input and finish the frame. A replay applies a frame's input
    // at the frame's START, so unless the step began a new segment the replay
    // would read Start where the live machine read A. (Stepping alone, with
    // the input unchanged, replays either way: run_frame finishes the frame.)
    let mut steps = 0;
    while ok(&h, "cpu_state", Value::Null)["pc"] != AFTER_READ_PC {
        ok(&h, "step_instructions", json!({ "count": 1 }));
        steps += 1;
        assert!(steps < 20_000, "never reached the read loop's end");
    }
    let read = ok(&h, "read_memory", json!({ "addr": 0x10, "len": 1 }));
    assert_eq!(read["hex"], "80", "the stepped read saw A: {read}");
    ok(
        &h,
        "step_frames",
        json!({ "frames": 4, "input": p1(&["Start"]) }),
    );
    verify_main("a mid-frame instruction step, an input change, then frames");

    ok(&h, "breakpoint_add", json!({ "pc": LOOP_END_PC }));
    let stop = ok(&h, "step_frames", json!({ "frames": 5 }));
    assert_eq!(stop["break_hit"], format!("${LOOP_END_PC:04X}"), "{stop}");
    ok(&h, "breakpoint_remove", json!({ "pc": LOOP_END_PC }));
    ok(
        &h,
        "step_frames",
        json!({ "frames": 6, "input": p1(&["Select"]) }),
    );
    verify_main("a breakpoint stop then frames");

    ok(&h, "reset", json!({ "kind": "soft" }));
    ok(&h, "step_frames", json!({ "frames": 3 }));
    verify_main("a soft reset then frames");

    let f = ok(&h, "fork_create", json!({}))["fork"]
        .as_str()
        .unwrap()
        .to_owned();
    ok(
        &h,
        "fork_run",
        json!({ "fork": f, "inputs": [{ "frames": 8, "p1": ["Up"] }] }),
    );
    ok(&h, "fork_adopt", json!({ "fork": f }));
    let d = ok(&h, "fork_diff", json!({ "a": "main", "b": f }));
    assert_eq!(
        d["identical_state"], true,
        "adopting takes the fork's state: {d}"
    );
    ok(&h, "step_frames", json!({ "frames": 2 }));
    verify_main("an adopted fork then frames");
}

#[test]
fn every_declared_tool_dispatches_and_nothing_else_does() {
    let names: Vec<String> = rustynes_mcp::tools::catalogue()
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    assert_eq!(
        names,
        rustynes_mcp::tools::TOOL_NAMES,
        "catalogue and name list agree"
    );
    let h = host();
    for name in rustynes_mcp::tools::TOOL_NAMES {
        // Empty arguments: most tools refuse them as invalid, which is fine;
        // what must not happen is "unknown tool".
        let r = call(&h, name, Value::Null);
        assert!(
            !matches!(r, Err(rustynes_mcp::ToolError::UnknownTool(_))),
            "{name} is declared but not dispatched"
        );
    }
    let _ = call(&h, "pause", Value::Null);
}

#[test]
fn observation_layers_describe_the_machine() {
    let h = host();
    ok(
        &h,
        "step_frames",
        json!({ "frames": 3, "input": p1(&["A", "Start"]) }),
    );

    let shot = call(&h, "screenshot", Value::Null).unwrap();
    let png = shot.png.expect("an image");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");

    let nt = call(&h, "nametable", json!({ "index": 2 })).unwrap();
    let text = &nt.texts[0];
    assert!(text.starts_with("nametable 2 ($2800)"), "{text}");
    assert_eq!(
        text.lines()
            .filter(|l| l.len() > 4 && l.as_bytes()[2] == b':')
            .count(),
        30
    );

    let oam = call(&h, "oam", Value::Null).unwrap();
    assert!(
        oam.texts[0].contains("sprites off-screen"),
        "{}",
        oam.texts[0]
    );

    let all = call(&h, "observe", Value::Null).unwrap();
    assert!(all.png.is_some());
    assert_eq!(all.texts.len(), 3, "nametable, oam, cpu");

    // A/Start pressed for 3 frames: $10 holds A|Start with A at bit 7.
    let mem = ok(&h, "read_memory", json!({ "addr": 0x10, "len": 3 }));
    assert_eq!(&mem["hex"].as_str().unwrap()[..2], "90", "{mem}");

    // read_memory has no side effects: a $2002 read twice gives one answer
    // and leaves the machine as it was.
    let before = common::main_snapshot(&h);
    ok(&h, "read_memory", json!({ "addr": 0x2002, "len": 1 }));
    ok(
        &h,
        "read_memory",
        json!({ "space": "ppu", "addr": 0x3F00, "len": 32 }),
    );
    ok(
        &h,
        "read_memory",
        json!({ "space": "oam", "addr": 0, "len": 256 }),
    );
    assert_eq!(common::main_snapshot(&h), before);
}

#[test]
fn real_time_running_pauses_and_stops_at_breakpoints() {
    let h = host();
    ok(&h, "set_input", p1(&["Right"]));
    ok(&h, "resume", json!({ "max_frames": 30 }));
    assert!(
        matches!(
            call(&h, "step_frames", json!({ "frames": 1 })),
            Err(rustynes_mcp::ToolError::Refused(_))
        ),
        "stepping while running is refused"
    );
    // Observation still answers while running.
    ok(&h, "cpu_state", Value::Null);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while h.is_running() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let p = ok(&h, "pause", Value::Null);
    assert_eq!(p["realtime_frames"], 30, "{p}");
    assert_eq!(p["last_stop"], "ran max_frames (30)");
    let v = ok(&h, "movie_verify", json!({ "source": "main" }));
    assert_eq!(v["identical"], true, "real-time frames are journalled: {v}");

    ok(&h, "breakpoint_add", json!({ "pc": LOOP_END_PC }));
    ok(&h, "resume", json!({}));
    while h.is_running() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let p = ok(&h, "pause", Value::Null);
    assert_eq!(
        p["last_stop"],
        format!("breakpoint ${LOOP_END_PC:04X}"),
        "{p}"
    );
}
