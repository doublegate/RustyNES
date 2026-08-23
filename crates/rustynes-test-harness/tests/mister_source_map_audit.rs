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

/// Extensions the corpus actually contains. `xhtml` is the load-bearing entry:
/// the nesdev pages are all `.xhtml`, and an earlier hand-run of this check used
/// a pattern without it, reporting "4 cited paths, 0 missing" against a file
/// holding 32 citations.
const EXTS: [&str; 4] = [".md", ".txt", ".html", ".xhtml"];

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
    let mut out: Vec<String> = spans(text)
        .filter(|s| EXTS.iter().any(|e| s.ends_with(e)))
        .map(str::to_owned)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Backticked spans that could be a path: the allowed character set and a dot.
///
/// Deliberately does NOT require a `/`. `cited_paths` is built on this, and a
/// slash requirement there makes the bare-filename check below structurally
/// incapable of firing -- the extractor would drop exactly the defect the check
/// exists to catch, and the check would then pass over an empty list forever.
///
/// That is not hypothetical: it was introduced in this very file while
/// refactoring for `unrecognised_extensions`, which DOES want a slash, and it
/// survived a mutation pass because the bare-filename test fed a hand-built
/// vector straight to the predicate instead of through the pipeline.
fn spans(text: &str) -> impl Iterator<Item = &str> {
    text.split('`').skip(1).step_by(2).filter(|s: &&str| {
        !s.is_empty()
            && s.contains('.')
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "._/-".contains(c))
    })
}

/// Path-like spans whose extension `cited_paths` does not recognise.
///
/// Without this the extractor FAILS OPEN. A new citation such as
/// `ref-docs/board.pdf` is silently dropped, the existing 32 still clear the
/// minimum-count check, and the audit reports success over a source nobody
/// verified. That is the same shape as the `.xhtml` omission this file already
/// records -- a pattern that cannot match looks exactly like content that is
/// not there -- so the fix is to make the unmatched case LOUD rather than to
/// widen the pattern and hope.
fn unrecognised_extensions(text: &str) -> Vec<&str> {
    spans(text)
        // The slash requirement lives HERE, not in `spans`. Guessing whether an
        // arbitrary backticked token is a path needs it; extracting citations
        // must not, or bare filenames vanish before anything can object to them.
        .filter(|s| s.contains('/') && !EXTS.iter().any(|e| s.ends_with(e)))
        .collect()
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
            // `is_file`, not `exists`: a citation is a document. `exists()`
            // returns true for a directory, so a path that happened to match a
            // directory name would report as a verified source when nothing
            // readable is there.
            if !root.join(c).is_file() {
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

    // ---- Tier 0: the extractor saw everything path-shaped. ----
    let unknown = unrecognised_extensions(&text);
    assert!(
        unknown.is_empty(),
        "{MAP} contains {} path-like span(s) whose extension the extractor does \
         not recognise: {unknown:#?}\n\
         They are being SILENTLY DROPPED, so they are never existence-checked \
         while the citation count still clears its minimum. Add the extension to \
         EXTS, or stop citing it as a path.",
        unknown.len()
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
    // Per-process, not a fixed global path. Cargo runs tests concurrently, and a
    // previously aborted run can leave the directory behind -- either way a fixed
    // name makes this test fail for a reason that has nothing to do with its
    // subject, which is the worst kind of flake in an audit.
    let tmp = std::env::temp_dir().join(format!(
        "rustynes-source-map-audit-ci-shape-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("docs")).expect("create synthetic docs/");
    std::fs::write(tmp.join("docs/ppu-2c02.md"), "x").expect("write");

    // A citation naming a DIRECTORY. `exists()` would call this verified;
    // `is_file()` reports it. Without this case the distinction is untestable --
    // the real document cites no directory, so mutating `is_file` back to
    // `exists` comes back NOT CAUGHT, which is how the weaker check survived
    // review in the first place.
    std::fs::create_dir_all(tmp.join("docs/subdir")).expect("create synthetic dir");
    let cited = vec![
        "docs/ppu-2c02.md".to_owned(), // present -> checked, found
        "docs/absent.md".to_owned(),   // present tree -> checked, MISSING
        "docs/subdir".to_owned(),      // a DIRECTORY -> must be MISSING
        "nesdev_wiki/PPU_rendering.xhtml".to_owned(), // tree absent -> unavailable
    ];
    let c = classify(&tmp, &cited);

    assert_eq!(
        c.checked, 3,
        "all three docs/ citations are in a tree that exists"
    );
    assert_eq!(
        c.missing.len(),
        2,
        "a missing file AND a directory-shaped citation must both be failures"
    );
    assert!(c.missing.iter().any(|m| *m == "docs/absent.md"));
    assert!(c.missing.iter().any(|m| *m == "docs/subdir"));
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

#[test]
fn a_citation_with_an_unrecognised_extension_is_reported_not_dropped() {
    // The failure mode CodeRabbit named on #447, driven directly: a new citation
    // whose extension is outside the allow-list must be LOUD, not invisible.
    // Before this, adding `ref-docs/board.pdf` left the 32 known citations still
    // clearing the >= 20 threshold, so the audit passed over an unverified
    // source.
    let text = "see `nesdev_wiki/PPU_rendering.xhtml` and `ref-docs/board.pdf`";
    assert_eq!(unrecognised_extensions(text), vec!["ref-docs/board.pdf"]);

    // And a span that is not path-shaped must not be reported as one.
    let prose = "the `v` register, `$2005`, and `ppu-state-trace`";
    assert!(unrecognised_extensions(prose).is_empty());

    // A VERSION STRING is the realistic false positive, and these documents are
    // full of them. `v2.5.3` passes the character set and contains dots, so
    // without the slash requirement in `unrecognised_extensions` it would be
    // reported as a citation with an unknown extension -- a hard failure on a
    // document that is perfectly correct.
    let versions = "shipped in `v2.5.3`, built on `v2.4.9`";
    assert!(
        unrecognised_extensions(versions).is_empty(),
        "a version string is not a path-like citation"
    );
    assert!(
        cited_paths(versions).is_empty(),
        "and it is not a citation either"
    );
}

#[test]
fn a_bare_filename_survives_extraction_and_is_then_reported() {
    // END TO END, through `cited_paths` -- not a hand-built vector fed to the
    // predicate. That distinction is the whole finding: with a slash filter in
    // the shared extractor, `cited_paths` silently dropped bare filenames, so
    // `bare_filenames` evaluated an empty list and passed forever. The audit was
    // structurally blind to the exact defect it claims to catch, and the
    // predicate-level test could not see it because it bypassed the extractor.
    let doc = "| Pulse | `nesdev_wiki/APU_Pulse.xhtml`, `APU_Sweep.xhtml` |";
    let cited = cited_paths(doc);
    assert!(
        cited.iter().any(|c| c == "APU_Sweep.xhtml"),
        "the extractor must SURFACE a bare filename, not drop it: {cited:?}"
    );
    assert_eq!(
        bare_filenames(&cited),
        vec![&"APU_Sweep.xhtml".to_owned()],
        "and the shape check must then report it"
    );

    // The real table cell that produced the original three defects has exactly
    // this shape: a fully-qualified path followed by a bare one.
    assert!(cited.iter().any(|c| c == "nesdev_wiki/APU_Pulse.xhtml"));
}
