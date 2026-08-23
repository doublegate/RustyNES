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

/// Citations with no directory component.
///
/// Extracted for the same reason as `classify`: with the document currently
/// clean, deleting the check in place changes nothing and a mutation of it comes
/// back NOT CAUGHT. A predicate a test can call directly is verifiable whether
/// or not today's document happens to violate it.
fn bare_filenames(cited: &[String]) -> Vec<&String> {
    cited.iter().filter(|p| !p.contains('/')).collect()
}

/// The split between "verified to exist" and "tree absent, shape only".
///
/// Extracted so a test can drive it against a synthetic root. The case that
/// matters is the one this machine cannot reproduce by inspection -- a checkout
/// WITHOUT `nesdev_wiki/`, which is every CI runner -- and reasoning about it is
/// what produced the bug in the first place.
struct Classified<'a> {
    checked: usize,
    unavailable: Vec<&'a String>,
    missing: Vec<&'a String>,
}

fn classify<'a>(root: &Path, cited: &'a [String]) -> Classified<'a> {
    let mut out = Classified {
        checked: 0,
        unavailable: Vec::new(),
        missing: Vec::new(),
    };
    for c in cited {
        let top = c.split('/').next().unwrap_or("");
        if root.join(top).is_dir() {
            out.checked += 1;
            if !root.join(c).exists() {
                out.missing.push(c);
            }
        } else {
            out.unavailable.push(c);
        }
    }
    out
}

#[test]
fn every_cited_source_is_well_formed_and_resolves_where_it_can() {
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

    // ---- Tier 1: shape. Always checked, corpus or no corpus. ----
    //
    // This is the tier that caught the real defects: three citations were bare
    // filenames (`APU_Sweep.xhtml`) in table cells that listed a second file
    // after a fully-qualified first one. A bare filename is ambiguous about
    // which tree it lives in, which is the one thing a source map may not be.
    let bare: Vec<&String> = bare_filenames(&cited);
    assert!(
        bare.is_empty(),
        "{MAP} cites {} source(s) with no directory component: {bare:#?}\n\
         A source map's citations must name the tree they live in.",
        bare.len()
    );

    // ---- Tier 2: existence, for the trees this checkout actually has. ----
    //
    // `nesdev_wiki/` is GITIGNORED -- 3,407 files of upstream corpus that this
    // repository deliberately does not vendor. So it is present on a developer's
    // machine and absent in CI, and an unconditional existence check passes
    // locally and fails every CI run. It did exactly that, on the release PR.
    //
    // The split is by whether the citation's top-level directory exists. Where
    // it does, a missing file is a HARD failure -- that is the whole point of
    // the audit. Where the tree is absent entirely, the citation is
    // shape-checked only, and the count is REPORTED rather than passed over in
    // silence: a check that quietly verifies less than it appears to is the
    // exact failure this file exists to prevent.
    let Classified {
        checked,
        unavailable,
        missing,
    } = classify(&root, &cited);

    assert!(
        missing.is_empty(),
        "{MAP} cites {} source(s) that do not exist, in trees this checkout HAS: {missing:#?}\n\
         Under ADR 0037 these are the ONLY permitted sources for that behaviour, so a \
         dangling citation is a behaviour with no source -- fix the path or add a dated \
         supplemental file.",
        missing.len()
    );

    println!(
        "source map: {} citations, all well-formed; {checked} verified to exist.",
        cited.len()
    );
    if !unavailable.is_empty() {
        // Named, not counted away. If this list ever covers everything, the
        // audit has stopped checking existence at all and should say so loudly.
        println!(
            "  NOT existence-checked here: {} citation(s) in trees absent from this \
             checkout (nesdev_wiki/ is gitignored upstream corpus). Shape only.",
            unavailable.len()
        );
        assert!(
            checked > 0,
            "no citation could be existence-checked at all; if that is genuinely \
             expected, this audit is now shape-only and should say so in its own name"
        );
    }
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

#[test]
fn a_checkout_without_the_upstream_corpus_still_checks_what_it_has() {
    // The CI case, driven directly rather than reasoned about. A synthetic root
    // holding `docs/` but NOT `nesdev_wiki/` is exactly what every runner sees,
    // because `nesdev_wiki/` is gitignored: 3,407 files of upstream corpus this
    // repository deliberately does not vendor.
    //
    // The first version of this audit checked existence unconditionally. It
    // passed here, where the corpus is present, and failed every CI job. This
    // test is the one that would have caught that before pushing.
    let tmp = std::env::temp_dir().join("rustynes-source-map-audit-ci-shape");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("docs")).expect("create synthetic docs/");
    std::fs::write(tmp.join("docs/ppu-2c02.md"), "x").expect("write");

    let cited = vec![
        "docs/ppu-2c02.md".to_owned(), // present -> checked, found
        "docs/absent.md".to_owned(),   // present tree -> checked, MISSING
        "nesdev_wiki/PPU_rendering.xhtml".to_owned(), // tree absent -> unavailable
    ];
    let c = classify(&tmp, &cited);

    assert_eq!(
        c.checked, 2,
        "both docs/ citations are in a tree that exists"
    );
    assert_eq!(
        c.missing.len(),
        1,
        "a missing file in a PRESENT tree must be a failure"
    );
    assert_eq!(c.missing[0], "docs/absent.md");
    assert_eq!(
        c.unavailable.len(),
        1,
        "a citation into an absent tree is reported, not counted as verified"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn a_bare_filename_is_rejected() {
    // The three real defects this audit caught on its first run were exactly
    // this shape: `APU_Sweep.xhtml` and two others, sitting in table cells that
    // listed a second file after a fully-qualified first one. A bare filename is
    // ambiguous about which tree it lives in, and a source map may not be.
    //
    // Driven on synthetic input because the document is now clean: with nothing
    // to fire on, mutating the check in place is invisible.
    let clean = vec![
        "nesdev_wiki/APU_Sweep.xhtml".to_owned(),
        "docs/apu-2a03.md".to_owned(),
    ];
    assert!(bare_filenames(&clean).is_empty());

    let dirty = vec![
        "nesdev_wiki/APU_Pulse.xhtml".to_owned(),
        "APU_Sweep.xhtml".to_owned(),
        "MMC1_pinout.xhtml".to_owned(),
    ];
    let found = bare_filenames(&dirty);
    assert_eq!(
        found.len(),
        2,
        "both bare filenames must be reported, not just the first"
    );
    assert_eq!(found[0], "APU_Sweep.xhtml");
    assert_eq!(found[1], "MMC1_pinout.xhtml");
}
