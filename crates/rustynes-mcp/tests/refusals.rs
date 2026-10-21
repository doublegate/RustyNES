//! Gate half 2: **a write outside the declared tools is refused.**
//!
//! Every route by which an agent could change the machine (or the host)
//! other than through a declared tool's declared arguments is tried here, and
//! each must be refused with the machine's save state byte-identical
//! afterwards:
//!
//! - an undeclared tool name (including names of real core debug writes that
//!   are deliberately not exposed);
//! - an undeclared argument on a declared tool;
//! - `write_ram` outside console work RAM;
//! - every write path in the Lua API, under the server's write lock, and the
//!   Lua sandbox's closed doors (`io`, `os`, `require`, `load`, `dofile`);
//! - a file write: with no `--out-dir`, with a path in the name, and over an
//!   existing file.

mod common;

use common::{call, host, host_with, main_snapshot, ok};
use rustynes_mcp::ToolError;
use serde_json::{Value, json};

#[test]
fn undeclared_tools_are_refused_and_change_nothing() {
    let h = host();
    ok(&h, "step_frames", json!({ "frames": 4 }));
    let before = main_snapshot(&h);
    for name in [
        "poke_ppu",
        "debug_poke_ppu",
        "poke_oam_byte",
        "debug_set_cpu_state",
        "write_memory",
        "write_prg",
        "load_rom",
        "load_state",
        "restore",
        "set_breakpoints_enabled",
        "Write_Ram",
        "write_ram ",
        "",
    ] {
        let r = call(&h, name, json!({ "addr": 0, "value": 1, "bytes": [1] }));
        assert_eq!(
            r.err(),
            Some(ToolError::UnknownTool(name.to_owned())),
            "{name:?} must be an unknown tool"
        );
    }
    assert_eq!(main_snapshot(&h), before);
}

#[test]
fn undeclared_arguments_are_refused_not_ignored() {
    let h = host();
    let before = main_snapshot(&h);
    for (tool, args) in [
        (
            "write_ram",
            json!({ "addr": 0, "bytes": [1], "space": "ppu" }),
        ),
        (
            "write_ram",
            json!({ "addr": 0, "bytes": [1], "force": true }),
        ),
        (
            "step_frames",
            json!({ "frames": 1, "poke": { "addr": 0, "value": 1 } }),
        ),
        (
            "fork_run",
            json!({ "fork": "f1", "inputs": [], "state": "AAAA" }),
        ),
        ("lua_eval", json!({ "source": "", "writes_locked": false })),
        (
            "movie_export",
            json!({ "source": "main", "path": "/tmp/x.rnm" }),
        ),
        ("set_input", json!({ "p5": ["A"] })),
        ("info", json!({ "x": 1 })),
    ] {
        let r = call(&h, tool, args.clone());
        assert!(
            matches!(r, Err(ToolError::InvalidArgs(ref m)) if m.contains("unknown field")),
            "{tool}({args}) -> {r:?}"
        );
    }
    assert_eq!(main_snapshot(&h), before);
}

#[test]
fn write_ram_writes_work_ram_and_nothing_else() {
    let h = host();
    ok(&h, "step_frames", json!({ "frames": 2 }));
    let before = main_snapshot(&h);
    for (addr, bytes) in [
        (0x0800, vec![1]),    // the first mirror
        (0x07FF, vec![1, 2]), // straddles the end
        (0x2000, vec![0x80]), // PPU registers
        (0x4014, vec![0x02]), // OAM DMA
        (0x6000, vec![1]),    // cartridge RAM
        (0x8000, vec![0]),    // PRG / mapper registers
        (0x0000, vec![]),     // nothing at all
    ] {
        let r = call(&h, "write_ram", json!({ "addr": addr, "bytes": bytes }));
        assert!(
            matches!(r, Err(ToolError::Refused(_))),
            "${addr:04X}: {r:?}"
        );
    }
    assert_eq!(
        main_snapshot(&h),
        before,
        "a refused write changed the machine"
    );

    // ... and inside it, it does write.
    ok(
        &h,
        "write_ram",
        json!({ "addr": 0x07FE, "bytes": [0xAB, 0xCD] }),
    );
    let m = ok(&h, "read_memory", json!({ "addr": 0x07FE, "len": 2 }));
    assert_eq!(m["hex"], "abcd");
}

#[test]
fn lua_writes_are_dropped_and_the_machine_is_unchanged() {
    let h = host();
    ok(
        &h,
        "step_frames",
        json!({ "frames": 6, "input": common::p1(&["A"]) }),
    );
    let before = main_snapshot(&h);
    let script = r"
        emu.onFrame(function()
            emu.log('read=' .. emu.read(0x12))
            emu.write(0x12, 0x99)
            memory:poke(0x13, 0x77)
            memory:write_range(0x20, { 1, 2, 3 })
            emu:save_state(1)
            emu.log('loaded=' .. tostring(emu:load_state(1)))
            emu.setInput(0, 0x09)
            emu.log('after=' .. emu.read(0x12))
        end)
    ";
    let out = call(&h, "lua_eval", json!({ "source": script })).expect("the script runs");
    assert_eq!(out.json["ok"], true, "{}", out.json);
    assert_eq!(out.json["state_unchanged"], true, "{}", out.json);
    let log = out.texts.join("");
    assert!(log.contains("read="), "the script did run: {log}");
    assert!(log.contains("loaded=false"), "load_state is refused: {log}");
    let read_line = log.lines().find(|l| l.starts_with("read=")).unwrap();
    let after_line = log.lines().find(|l| l.starts_with("after=")).unwrap();
    assert_eq!(
        read_line.trim_start_matches("read="),
        after_line.trim_start_matches("after="),
        "the write never reached the machine: {log}"
    );
    assert_eq!(main_snapshot(&h), before, "Lua changed the machine");
    // And the next frame runs on the held input, not on the script's.
    let held = ok(&h, "info", Value::Null)["held"].clone();
    assert_eq!(held["p1"], json!(["A"]));
}

#[test]
fn the_lua_sandbox_has_no_way_out() {
    let h = host();
    let probe = r"
        emu.log('io=' .. type(io))
        emu.log('os=' .. type(os))
        emu.log('require=' .. type(require))
        emu.log('load=' .. type(load))
        emu.log('dofile=' .. type(dofile))
        emu.log('package=' .. type(package))
        emu.log('debug=' .. type(debug))
    ";
    let out = call(&h, "lua_eval", json!({ "source": probe })).unwrap();
    let log = out.texts.join("");
    for lib in ["io", "os", "require", "load", "dofile", "package", "debug"] {
        assert!(
            log.contains(&format!("{lib}=nil")),
            "{lib} is reachable: {log}"
        );
    }

    // A runaway loop is stopped by the instruction budget, not by the host.
    let out = call(&h, "lua_eval", json!({ "source": "while true do end" })).unwrap();
    assert_eq!(out.json["ok"], false, "{}", out.json);
    assert!(
        out.json["error"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .contains("budget"),
        "{}",
        out.json
    );

    // Oversized source is refused before the VM sees it.
    let big = "-".repeat(rustynes_mcp::limits::MAX_LUA_SOURCE + 1);
    assert!(matches!(
        call(&h, "lua_eval", json!({ "source": big })),
        Err(ToolError::Refused(_))
    ));
}

#[test]
fn files_are_written_only_where_and_as_the_operator_allows() {
    // No --out-dir: no file write at all.
    let h = host();
    let r = call(
        &h,
        "movie_export",
        json!({ "source": "main", "name": "run" }),
    );
    assert!(
        matches!(r, Err(ToolError::Refused(ref m)) if m.contains("--out-dir")),
        "{r:?}"
    );

    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "mcp-out-{}-{}",
        std::process::id(),
        line!()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let h = host_with(Some(dir.clone()));
    for bad in [
        "../escape",
        "a/b",
        "/abs",
        "x.rnm",
        "",
        "dot.",
        "spa ce",
        &"x".repeat(65),
    ] {
        let r = call(&h, "movie_export", json!({ "source": "main", "name": bad }));
        assert!(matches!(r, Err(ToolError::Refused(_))), "{bad:?} -> {r:?}");
    }
    let out = ok(
        &h,
        "movie_export",
        json!({ "source": "main", "name": "run_1" }),
    );
    let path = dir.join("run_1.rnm");
    assert_eq!(out["written"], path.display().to_string());
    let first = std::fs::read(&path).unwrap();
    // An export never overwrites.
    ok(&h, "step_frames", json!({ "frames": 2 }));
    let r = call(
        &h,
        "movie_export",
        json!({ "source": "main", "name": "run_1" }),
    );
    assert!(matches!(r, Err(ToolError::Refused(_))), "{r:?}");
    assert_eq!(std::fs::read(&path).unwrap(), first);
    let entries: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
    assert_eq!(entries.len(), 1, "exactly one file was written");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn limits_are_refused_not_truncated() {
    let h = host();
    let f = ok(&h, "fork_create", Value::Null)["fork"]
        .as_str()
        .unwrap()
        .to_owned();
    let too_many = rustynes_mcp::limits::MAX_FORK_RUN_FRAMES + 1;
    let r = call(
        &h,
        "fork_run",
        json!({ "fork": f, "inputs": [{ "frames": too_many }] }),
    );
    assert!(matches!(r, Err(ToolError::Refused(_))), "{r:?}");
    let r = call(&h, "step_frames", json!({ "frames": 0 }));
    assert!(matches!(r, Err(ToolError::InvalidArgs(_))), "{r:?}");
    let r = call(&h, "read_memory", json!({ "addr": 0, "len": 0x1001 }));
    assert!(matches!(r, Err(ToolError::InvalidArgs(_))), "{r:?}");
    let r = call(
        &h,
        "fork_run",
        json!({ "fork": f, "inputs": [{ "frames": 1, "p1": ["Strat"] }] }),
    );
    assert!(
        matches!(r, Err(ToolError::InvalidArgs(ref m)) if m.contains("Strat")),
        "{r:?}"
    );
    // A refused run recorded nothing.
    let info = ok(&h, "info", Value::Null);
    assert_eq!(info["forks"][0]["frames"], 0, "{info}");
    for _ in 1..rustynes_mcp::limits::MAX_FORKS {
        ok(&h, "fork_create", Value::Null);
    }
    assert!(matches!(
        call(&h, "fork_create", Value::Null),
        Err(ToolError::Refused(_))
    ));
}
