//! Pin the `MiSTer` hardware source map's citations to files that actually exist.
//!
//! `ref-docs/2026-08-23-fpga-nes-hardware-source-map.md` is the list of pages the
//! FPGA core's RTL may be written from. Under the provenance firewall (ADR 0037)
//! that list is not a convenience -- it is what makes "written from public
//! documentation" a *checkable* claim rather than an assertion, since the
//! reference cores are black boxes and there is no second source to fall back on.
//!
//! A citation that no longer resolves is therefore not a broken link. It is a
//! behaviour with no permitted source, discovered at the moment someone is trying
//! to implement it and is most inclined to go looking elsewhere. This test exists
//! so the corpus cannot move out from under the map quietly.
//!
//! It found three defects on its first run: `APU_Envelope.xhtml`,
//! `APU_Sweep.xhtml` and `MMC1_pinout.xhtml` were cited as bare filenames, in
//! table cells that listed a second file after a fully-qualified first one.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

const MAP: &str = "ref-docs/2026-08-23-fpga-nes-hardware-source-map.md";

/// Every backticked path-looking span in the document.
///
/// Deliberately extension-driven rather than "anything in backticks": the file
/// also spans identifiers (`v`, `$2005`, `index_framebuffer`, `ppu-state-trace`)
/// that are not paths and never will be. The extension list covers what the
/// corpus actually contains -- and `xhtml` is the load-bearing entry, because the
/// nesdev pages are all `.xhtml` and an earlier hand-run of this same check used
/// a pattern without it. That run reported "4 cited paths, 0 missing" against a
/// file holding 32 citations, and the reassuring number came from a pattern that
/// could not match the extension every real citation uses.
fn cited_paths(text: &str) -> Vec<String> {
    const EXTS: [&str; 4] = [".md", ".txt", ".html", ".xhtml"];
    let mut out: Vec<String> = text
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|s| {
            EXTS.iter().any(|e| s.ends_with(e))
                && !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "._/-".contains(c))
        })
        .map(str::to_owned)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

#[test]
fn every_cited_source_still_exists() {
    let root = workspace_root();
    let text =
        std::fs::read_to_string(root.join(MAP)).unwrap_or_else(|e| panic!("read {MAP}: {e}"));

    let cited = cited_paths(&text);

    // Fail closed. Zero citations means the extraction stopped working, not that
    // the map is clean -- the failure this repository keeps rediscovering.
    assert!(
        cited.len() >= 20,
        "extracted only {} citations from {MAP}; the pattern is wrong, \
         and a pattern that matches nothing looks exactly like a clean document",
        cited.len()
    );

    let missing: Vec<&String> = cited.iter().filter(|p| !root.join(p).exists()).collect();
    assert!(
        missing.is_empty(),
        "{MAP} cites {} source(s) that do not exist: {missing:#?}\n\
         Under ADR 0037 these are the ONLY permitted sources for that behaviour, \
         so a dangling citation is a behaviour with no source -- fix the path or \
         add a dated supplemental file.",
        missing.len()
    );
}

#[test]
fn the_extractor_distinguishes_paths_from_identifiers() {
    // Guards the extractor itself. Without this, widening the filter to "anything
    // in backticks" would still pass the test above right up until the first
    // identifier containing a dot, and the failure would be reported as a missing
    // source file rather than as a broken extractor.
    let sample = "see `nesdev_wiki/PPU_rendering.xhtml` and `docs/ppu-2c02.md`, \
                  but not `v`, `$2005`, `ppu-state-trace` or `index_framebuffer`";
    assert_eq!(
        cited_paths(sample),
        vec![
            "docs/ppu-2c02.md".to_owned(),
            "nesdev_wiki/PPU_rendering.xhtml".to_owned(),
        ]
    );
}
