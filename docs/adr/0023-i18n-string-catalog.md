# 23. i18n via a compile-time string catalog (not Fluent/ICU)

Date: 2026-06-19

## Status

Accepted (v1.7.0 "Forge", Workstream H5 — i18n framework).

## Context

RustyNES's one systemic UI gap was that every user-facing string was a hard-
coded literal — there was no localization layer anywhere. Workstream H5 adds
one. The constraints that shape the choice:

- **Frontend-only.** The deterministic core / chip stack / test harness must not
  be touched, and AccuracyCoin must stay 139/139. i18n is a presentation
  concern, so it lives entirely in `rustynes-frontend`.
- **English must stay byte-identical by default.** With the default locale the
  shipped UI must render the exact same strings it does today, so the English
  catalog values are the verbatim current literals and English is the default.
- **wasm size budget.** The browser build is gated by
  `scripts/wasm_size_budget.sh` (5 MiB gzip). A localization library that drags
  in `fluent`, `unic-langid`, ICU message-format parsing, or runtime catalog
  loading would risk blowing that budget. The layer must be cheap.
- **Determinism.** No runtime file I/O on the core's path, no hidden non-
  determinism. (i18n only affects rendered labels, never emulation, so this is
  trivially satisfied — but the "no runtime I/O" property also keeps the wasm
  build self-contained.)

## Decision

### A compile-time string catalog, hand-rolled — not Fluent/ICU, not `rust-i18n`

`crates/rustynes-frontend/src/i18n.rs` defines a `Key` enum (one variant per
translatable string) and one `const fn` catalog per locale that `match`es a
`Key` to a `&'static str`. Resolution is `tr(key) -> &'static str` (current
process-global locale) plus a `t!(Key)` convenience macro; `tr_in(locale, key)`
is the explicit form.

Rejected alternatives and why:

- **Fluent (`fluent` + `unic-langid` + `intl-memoizer`).** The industry-standard
  rich-i18n stack (gender/plural/grammar via FTL message syntax). It pulls a
  multi-crate dependency tree with a runtime parser and a langid matcher — far
  more than a fixed-string emulator UI needs, and the wrong direction for the
  wasm size gate. Not adopted.
- **`rust-i18n`.** Lighter than Fluent and macro-driven, but it loads YAML/TOML
  locale files (typically at runtime / via a build step) and brings its own
  global + formatter. The runtime-load model is awkward on wasm (no filesystem)
  and adds a dependency for what a plain `match` does for free. Not adopted.
- **`include_str!` of per-locale TOML/RON parsed at startup.** Was on the table
  (the task allowed it). It still requires a TOML/RON parser run at boot and
  carries the embedded files in the binary. A `const fn` `match` table is
  strictly cheaper: the strings are read-only data the linker can place and
  dead-strip, there is *zero* startup parsing, and there is no embedded blob to
  ship. Not adopted.

The hand-rolled catalog costs only a few KiB of string bytes and no new
dependency — the conservative choice for the wasm budget. The measured trunk
release bundle stayed comfortably inside the 5 MiB gzip gate (see the PR).

### English is the default and the universal fallback

`Locale::English` is `#[derive(Default)]`. The English catalog (`english`) must
define a value for *every* `Key`; non-English catalogs (`spanish`) return
`Option<&'static str>` and may return `None` for an untranslated key, which
`tr_in` resolves to the English value. So:

- A `[ui] locale` config that omits the field (every pre-H5 config) deserializes
  to English via `#[serde(default)]`, and the UI is byte-identical to v1.6.0.
- A partially-translated locale degrades gracefully to English per missing key
  rather than showing an empty or placeholder string. (`Shaders`/`Audio` are
  deliberately left untranslated in Spanish — both are loanwords used verbatim —
  which keeps the fallback path live and unit-tested.)

### Locale selection + persistence

The active locale is a process-global `AtomicU8` (`CURRENT_LOCALE`).
`set_locale` is called once at startup from `[ui] locale` and again whenever the
Settings → Video → "Language" picker changes it; the render loop also republishes
`config.ui.locale` to the global every frame (a relaxed atomic store, negligible
cost). Because egui re-renders the whole shell each frame and every converted
call site reads `tr(..)` fresh, a language change takes effect on the very next
frame with no explicit cache invalidation. The choice persists to the TOML config
like any other `[ui]` field.

### Incremental conversion

Only the high-visibility surfaces are wired through `tr!`/`t!` in this change:
the menu bar (top-level menus + the common File/View items), the Settings window
title + tab strip + the Display/Accessibility/Theme/Language labels, and the
status-bar state words. Deeper panels (debugger internals, the per-subsystem
tool panels) keep their literals for now. Converting a string is a two-step
pattern — add a `Key` variant with its verbatim English value (and translations)
to `i18n.rs`, then replace the literal with `crate::t!(TheKey)` at the call site
— documented in `docs/frontend.md`. Conversion proceeds incrementally; nothing
breaks while a string is still a literal.

## Consequences

- A lightweight, dependency-free, wasm-safe localization layer; adding a locale
  is one enum variant + one catalog `match` arm, and adding a string is one
  `Key` + its catalog entries.
- English is the default and fallback, so the shipped default UI is byte-
  identical to v1.6.0 and AccuracyCoin stays 139/139 (i18n never touches the
  core).
- Conversion is incremental: the framework + the high-visibility surfaces land
  now; the remaining panels follow the documented `tr!` pattern over time. Until
  a string is converted it simply renders its literal as before.
- The hand-rolled catalog trades Fluent's grammatical richness (plurals,
  gender, ordered placeholders) for size and simplicity. RustyNES's UI strings
  are overwhelmingly fixed labels, so this is the right trade today; if rich
  message formatting is ever needed for a specific string, that single string
  can format its own arguments without changing the catalog mechanism.
