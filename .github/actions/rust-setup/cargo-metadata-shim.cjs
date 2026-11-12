#!/usr/bin/env node
// The `cmd-format` wrapper Swatinem/rust-cache runs its cargo and rustc
// commands through (`.github/actions/rust-setup/action.yml`).
//
// WHY THIS EXISTS. rust-cache decides what to KEEP from the dependency list of
// `cargo metadata --all-features --format-version 1`. This workspace cannot
// resolve `--all-features`: `rustynes-frontend` depends on `rustynes-script`
// twice, under two names (the native mlua backend, and the wasm32-only
// `rustynes-script-wasm` alias for the piccolo backend), and cargo refuses
// "depends on crate `rustynes-script` multiple times with different names"
// the moment both are switched on. The same failure is why cargo-nextest was
// rejected (ci.yml's `test` job) and why `--all-features` is banned in
// AGENTS.md.
//
// rust-cache does not fail on that. It prints the error inside its own
// collapsed post step, carries on with an EMPTY dependency list, and so
// deletes every dependency artifact from `target/` and every crate from the
// registry before saving. Measured on 2026-11-12: 150 rust-cache entries in
// this repository held 65 MB between them, about 20 KB each apart from the
// fuzz job's cargo-fuzz binary; the `lint` job's entry was 19,970 bytes and
// "Restored ... full match: true" every run. Every Rust job therefore compiled
// its whole dependency graph cold on every run, behind a green "Cache restored
// successfully" line. This had been the case since the alias arrived in v1.2.0.
//
// WHAT IT DOES. Every command rust-cache formats is passed through unchanged,
// with one exception: `cargo metadata --all-features ...` WITHOUT `--no-deps`
// (the dependency listing). That is first run exactly as asked, so if the
// workspace ever becomes resolvable nothing here changes behaviour. Only if it
// fails is it re-run with `--all-features` replaced by an explicit
// `--features` list: every feature of every workspace member (read from
// `cargo metadata --no-deps`, which needs no resolution and works today)
// minus the `--exclude` list. The composite excludes exactly
// `rustynes-frontend/script-wasm`, the one feature that switches the alias on.
// Nothing is lost by that: measured on 2026-11-12 the explicit list resolves
// 707 packages, the `script-wasm`-only resolution 698, and its union with the
// explicit list is still 707, because `rustynes-script/script-wasm` (a
// different, still-listed feature) already reaches piccolo's tree.
//
// rust-cache executes this with `@actions/exec` and no shell, on Linux, macOS
// and Windows runners, so it is Node (always present on GitHub-hosted images,
// and checked by the composite before use) rather than a shell script.
//
// Modes:
//   cargo-metadata-shim.cjs [--exclude=a/b,c/d] <cmd> [args...]
//       the wrapper, as `cmd-format: "node <this> --exclude=... {0}"`
//   cargo-metadata-shim.cjs --self-test [--exclude=...]
//       cheap check of the fallback's feature list (no network, no
//       resolution): fails if it is empty or still contains an excluded
//       feature. The composite runs it before handing the shim to rust-cache.
//
// A failure of the FALLBACK is reported in the job summary, because
// rust-cache swallows this command's stderr and exit status: that silence is
// exactly how the empty caches went unnoticed.

"use strict";

const { spawnSync } = require("node:child_process");
const fs = require("node:fs");

// Metadata output for this workspace is several MB; the default 1 MB
// maxBuffer would truncate it into invalid JSON.
const MAX_BUFFER = 512 * 1024 * 1024;

function parseArgs(argv) {
  let exclude = [];
  let selfTest = false;
  const rest = [];
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (rest.length === 0 && a.startsWith("--exclude=")) {
      exclude = a
        .slice("--exclude=".length)
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean);
    } else if (rest.length === 0 && a === "--self-test") {
      selfTest = true;
    } else {
      rest.push(a);
    }
  }
  return { exclude, selfTest, rest };
}

function capture(cmd, args) {
  return spawnSync(cmd, args, {
    stdio: ["ignore", "pipe", "pipe"],
    maxBuffer: MAX_BUFFER,
  });
}

// Every `<member>/<feature>` of the workspace, minus the excluded ones.
// `cargo metadata --no-deps` lists members only, so it never resolves the
// dependency graph and cannot hit the duplicate-name error.
function memberFeatures(exclude) {
  const r = capture("cargo", ["metadata", "--no-deps", "--format-version", "1"]);
  if (r.error || r.status !== 0) {
    throw new Error(
      `cargo metadata --no-deps failed (${r.error ? r.error.message : `exit ${r.status}`}): ${String(r.stderr).trim()}`,
    );
  }
  const meta = JSON.parse(r.stdout);
  const skip = new Set(exclude);
  const out = [];
  for (const pkg of meta.packages) {
    for (const feature of Object.keys(pkg.features || {})) {
      const qualified = `${pkg.name}/${feature}`;
      if (!skip.has(qualified)) out.push(qualified);
    }
  }
  return out;
}

function summary(line) {
  const file = process.env.GITHUB_STEP_SUMMARY;
  if (!file) return;
  try {
    fs.appendFileSync(file, `${line}\n`);
  } catch {
    // A summary that cannot be written must not change the command's result.
  }
}

// Exit codes are SET, never forced with `process.exit()`: Node writes to a pipe
// asynchronously on macOS and Windows, so exiting straight after writing
// several MB of metadata JSON can truncate it -- which rust-cache would parse
// as no dependencies at all, the very failure this file exists to remove.
function statusOf(r) {
  if (r.error) {
    process.stderr.write(`cargo-metadata-shim: ${r.error.message}\n`);
    return 127;
  }
  return r.status === null ? 1 : r.status;
}

function selfTest(exclude) {
  if (exclude.length === 0) {
    throw new Error("--self-test needs --exclude=...; with nothing excluded the fallback cannot differ from --all-features");
  }
  const features = memberFeatures(exclude);
  if (features.length === 0) {
    throw new Error("the fallback feature list is empty");
  }
  for (const e of exclude) {
    if (features.includes(e)) throw new Error(`excluded feature ${e} is still listed`);
  }
  const all = memberFeatures([]);
  for (const e of exclude) {
    if (!all.includes(e)) {
      // An exclusion naming a feature that no longer exists means the reason
      // for this shim has moved; say so rather than excluding nothing.
      throw new Error(`excluded feature ${e} is not a feature of any workspace member`);
    }
  }
  process.stdout.write(
    `cargo-metadata-shim self-test: ${features.length} of ${all.length} member features kept, excluded: ${exclude.join(", ")}\n`,
  );
}

// Returns the exit status; see `statusOf` for why nothing calls process.exit().
function main() {
  const { exclude, selfTest: wantSelfTest, rest } = parseArgs(process.argv.slice(2));
  if (wantSelfTest) {
    selfTest(exclude);
    return 0;
  }
  if (rest.length === 0) {
    process.stderr.write("cargo-metadata-shim: no command given\n");
    return 2;
  }
  const [cmd, ...args] = rest;
  const isDependencyListing =
    cmd === "cargo" && args[0] === "metadata" && args.includes("--all-features") && !args.includes("--no-deps");

  if (!isDependencyListing) {
    // rustc -vV, rustup toolchain list, cargo metadata --no-deps: unchanged.
    return statusOf(spawnSync(cmd, args, { stdio: "inherit" }));
  }

  const first = capture(cmd, args);
  if (!first.error && first.status === 0) {
    process.stdout.write(first.stdout);
    process.stderr.write(first.stderr);
    return 0;
  }

  let fallback;
  try {
    const features = memberFeatures(exclude);
    const fallbackArgs = args.filter((a) => a !== "--all-features").concat(["--features", features.join(",")]);
    fallback = capture(cmd, fallbackArgs);
  } catch (e) {
    fallback = { error: e };
  }
  if (!fallback.error && fallback.status === 0) {
    process.stdout.write(fallback.stdout);
    return 0;
  }

  const why = fallback.error ? fallback.error.message : String(fallback.stderr).trim().split("\n").slice(-3).join(" / ");
  summary(
    "**rust-cache could not list this workspace's dependencies, so the cache it saves is EMPTY.** " +
      `\`cargo metadata --all-features\` failed and so did the fallback (${why}). ` +
      "See `.github/actions/rust-setup/cargo-metadata-shim.cjs`.",
  );
  process.stderr.write(first.stderr);
  process.stderr.write(`cargo-metadata-shim: fallback failed: ${why}\n`);
  return first.status === null ? 1 : first.status;
}

try {
  process.exitCode = main();
} catch (e) {
  process.stderr.write(`cargo-metadata-shim: ${e.message}\n`);
  process.exitCode = 1;
}
