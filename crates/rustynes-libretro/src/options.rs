//! The core options: one table, declared in whichever form the frontend
//! speaks, and read back into one [`OptionValues`] (v3.6.0,
//! `T-LIBRETRO-OPTIONS-V2`).
//!
//! # Two declarations from one table
//!
//! libretro has three ways to declare options. This core uses two:
//!
//! * **Core Options v2** (`RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2`), when
//!   `RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION` reports 2 or more. It
//!   carries what the legacy form cannot: categories (a frontend shows
//!   each as a submenu), a sublabel per option, and a label per value, so
//!   the menu can say "On" while the stored value stays `enabled`.
//! * **`SET_VARIABLES`** (the "v0" form) for everything else, including a
//!   version-1 frontend: v1 adds sublabels but no categories, and every
//!   frontend that speaks v1 also accepts v0. A frontend that does not answer
//!   the version query at all reports 0 (`rust-libretro`'s
//!   `get_core_options_version`), so it gets v0, which is what this core sent
//!   every frontend before v3.6.0.
//!
//! libretro.h states that on a version-2 frontend `SET_CORE_OPTIONS_V2` "is
//! guaranteed to succeed" and that its return value says only whether the
//! frontend shows categories, so a `false` from it is not a refusal and does
//! not trigger the v0 form.
//!
//! Both declarations are built from [`OPTIONS`] and nothing else, so the keys,
//! values and defaults cannot differ between them; a test enumerates the keys
//! each path declares against the v3.5.0 list (`rustynes_four_score`), which
//! must survive unchanged because a frontend stores the user's choices by key.
//! The v0 string of the Four Score option is byte-for-byte the one v3.5.0
//! declared.
//!
//! # Lifetime
//!
//! libretro.h does not say how long a frontend may keep the pointers it is
//! handed, and RetroArch's handling differs per command (`docs/agents/
//! libretro.md`), so every pointer in both declarations points into a
//! process-lifetime [`OnceLock`]: built on first use, never freed, never
//! moved (the `Vec`s are not touched after construction, so their heap
//! buffers stay where the pointers say).

use rust_libretro::sys::{
    RETRO_NUM_CORE_OPTION_VALUES_MAX, retro_core_option_v2_category,
    retro_core_option_v2_definition, retro_core_option_value, retro_core_options_v2,
    retro_environment_t, retro_variable,
};
use std::ffi::{CStr, CString};
use std::sync::OnceLock;

/// The Four Score option's key (v2.8.1). Unchanged since: a frontend stores
/// the user's setting under it.
pub const FOUR_SCORE_KEY: &CStr = c"rustynes_four_score";

/// One option, in the form both declarations are generated from.
pub struct OptionSpec {
    /// The key the frontend stores the value under.
    pub key: &'static CStr,
    /// The menu label without categories (v2 `desc`, and the v0 description).
    pub desc: &'static CStr,
    /// The menu label inside its category (v2 `desc_categorized`).
    pub desc_categorized: &'static CStr,
    /// The sublabel (v2 `info`; v0 has none).
    pub info: &'static CStr,
    /// The category key, one of [`CATEGORIES`].
    pub category: &'static CStr,
    /// `(value, label)`, the default first: v0 takes its default from the
    /// first value, so the order is the v0 contract.
    pub values: &'static [(&'static CStr, &'static CStr)],
}

impl OptionSpec {
    /// The default value: the first, as v0 requires.
    pub const fn default_value(&self) -> &'static CStr {
        self.values[0].0
    }
}

/// `(key, label, sublabel)` of each category.
pub const CATEGORIES: [(&CStr, &CStr, &CStr); 1] = [(
    c"input",
    c"Input",
    c"Controllers and adapters plugged into the console.",
)];

/// Every option the core declares, in menu order.
pub const OPTIONS: [OptionSpec; 1] = [
    OptionSpec {
        key: FOUR_SCORE_KEY,
        // The v0 description, unchanged from v3.5.0 so that frontend's
        // stored label and the v0 string stay what they were.
        desc: c"Four Score (players 3 and 4)",
        desc_categorized: c"Four Score (players 3 and 4)",
        info: c"Plugs in the Four Score adapter, so controllers 3 and 4 reach the game. Off by default, as on the console: a game that does not expect the adapter can read its signature.",
        category: c"input",
        values: &[(c"disabled", c"Off"), (c"enabled", c"On")],
    },
];

/// What the options say, parsed. `Default` is every option at its default,
/// which is also what a frontend that answers no `GET_VARIABLE` gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptionValues {
    /// `rustynes_four_score`.
    pub four_score: bool,
}

impl Default for OptionValues {
    fn default() -> Self {
        Self::STOCK
    }
}

impl OptionValues {
    /// Every option at its default, as a `const` (the `retro_core!`
    /// instance is built in a `static`).
    pub const STOCK: Self = Self { four_score: false };

    /// Read every option through `get`, which answers a key with the
    /// frontend's value or `None`. An unknown value reads as the option's
    /// default rather than as something the menu never offered.
    pub fn read(mut get: impl FnMut(&CStr) -> Option<String>) -> Self {
        Self {
            four_score: get(FOUR_SCORE_KEY).as_deref() == Some("enabled"),
        }
    }
}

/// A `'static` table of raw pointers. The pointers lead only to `'static`
/// C strings and to the heap buffers of `Vec`s that are never touched after
/// construction, and nothing writes through them.
struct Frozen<T>(T);

// SAFETY: see the type's documentation: everything a `Frozen` points at is
// immutable for the life of the process, so sharing it across threads (the
// frontend may call from any one) reads nothing that can change.
unsafe impl<T> Sync for Frozen<T> {}
// SAFETY: as for `Sync`. `OnceLock` is `Sync` only for a `Send + Sync`
// value, hence this impl; the raw pointers inside are never written through
// and lead only to immutable data, so moving the owner to another thread
// hands it nothing that can change under it.
#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl<T> Send for Frozen<T> {}

/// The v2 declaration: categories and definitions, each terminated by an
/// all-null entry, and the struct pointing at both.
struct V2Tables {
    _categories: Vec<retro_core_option_v2_category>,
    _definitions: Vec<retro_core_option_v2_definition>,
    options: retro_core_options_v2,
}

/// The v0 declaration: the `"desc; default|other|..."` strings and the
/// null-key-terminated `retro_variable` array pointing at them.
struct V0Tables {
    _strings: Vec<CString>,
    variables: Vec<retro_variable>,
}

/// An all-null value entry: the terminator of a values list.
const NO_VALUE: retro_core_option_value = retro_core_option_value {
    value: std::ptr::null(),
    label: std::ptr::null(),
};

fn v2_tables() -> &'static V2Tables {
    static TABLES: OnceLock<Frozen<V2Tables>> = OnceLock::new();
    &TABLES
        .get_or_init(|| {
            let mut categories: Vec<_> = CATEGORIES
                .iter()
                .map(|(key, desc, info)| retro_core_option_v2_category {
                    key: key.as_ptr(),
                    desc: desc.as_ptr(),
                    info: info.as_ptr(),
                })
                .collect();
            categories.push(retro_core_option_v2_category {
                key: std::ptr::null(),
                desc: std::ptr::null(),
                info: std::ptr::null(),
            });
            let mut definitions: Vec<_> = OPTIONS
                .iter()
                .map(|spec| {
                    // One slot is kept for the terminator, so a list may hold
                    // at most `MAX - 1` values.
                    assert!(spec.values.len() < RETRO_NUM_CORE_OPTION_VALUES_MAX as usize);
                    let mut values = [NO_VALUE; RETRO_NUM_CORE_OPTION_VALUES_MAX as usize];
                    for (slot, (value, label)) in values.iter_mut().zip(spec.values) {
                        *slot = retro_core_option_value {
                            value: value.as_ptr(),
                            label: label.as_ptr(),
                        };
                    }
                    retro_core_option_v2_definition {
                        key: spec.key.as_ptr(),
                        desc: spec.desc.as_ptr(),
                        desc_categorized: spec.desc_categorized.as_ptr(),
                        info: spec.info.as_ptr(),
                        info_categorized: std::ptr::null(),
                        category_key: spec.category.as_ptr(),
                        values,
                        default_value: spec.default_value().as_ptr(),
                    }
                })
                .collect();
            definitions.push(retro_core_option_v2_definition {
                key: std::ptr::null(),
                desc: std::ptr::null(),
                desc_categorized: std::ptr::null(),
                info: std::ptr::null(),
                info_categorized: std::ptr::null(),
                category_key: std::ptr::null(),
                values: [NO_VALUE; RETRO_NUM_CORE_OPTION_VALUES_MAX as usize],
                default_value: std::ptr::null(),
            });
            // libretro.h declares the two pointers non-const, but the command
            // passes a `const struct retro_core_options_v2 *`: the frontend
            // only reads them.
            let options = retro_core_options_v2 {
                categories: categories.as_mut_ptr(),
                definitions: definitions.as_mut_ptr(),
            };
            Frozen(V2Tables {
                _categories: categories,
                _definitions: definitions,
                options,
            })
        })
        .0
}

fn v0_tables() -> &'static V0Tables {
    static TABLES: OnceLock<Frozen<V0Tables>> = OnceLock::new();
    &TABLES
        .get_or_init(|| {
            let strings: Vec<CString> = OPTIONS.iter().map(v0_string).collect();
            let mut variables: Vec<_> = OPTIONS
                .iter()
                .zip(&strings)
                .map(|(spec, value)| retro_variable {
                    key: spec.key.as_ptr(),
                    value: value.as_ptr(),
                })
                .collect();
            variables.push(retro_variable {
                key: std::ptr::null(),
                value: std::ptr::null(),
            });
            Frozen(V0Tables {
                _strings: strings,
                variables,
            })
        })
        .0
}

/// The v0 form of one option: `"<desc>; <default>|<value>|..."`. libretro.h
/// takes the first value as the default.
pub fn v0_string(spec: &OptionSpec) -> CString {
    let values: Vec<&str> = spec
        .values
        .iter()
        .map(|(value, _)| value.to_str().unwrap_or_default())
        .collect();
    let text = format!(
        "{}; {}",
        spec.desc.to_str().unwrap_or_default(),
        values.join("|")
    );
    // The parts are C string literals, so they hold no interior NUL.
    CString::new(text).unwrap_or_default()
}

/// Which form a declaration took.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Declared {
    /// `SET_CORE_OPTIONS_V2`; `categories` is what the frontend answered
    /// (whether it shows them), not whether it accepted the options.
    V2 { categories: bool },
    /// `SET_VARIABLES`, which the frontend accepted.
    V0,
    /// `SET_VARIABLES`, which the frontend refused.
    Refused,
}

/// Declare every option in the frontend's form: v2 when it reports a core
/// options version of 2 or more, otherwise v0.
///
/// # Safety
///
/// `cb` must be the frontend's environment callback, valid for the duration
/// of the call (as it is inside `retro_set_environment`).
pub unsafe fn declare(cb: retro_environment_t) -> Declared {
    // SAFETY: the caller guarantees `cb`; the version query passes a
    // `*mut c_uint` that `rust-libretro` owns for the call.
    let version = unsafe { rust_libretro::environment::get_core_options_version(cb) };
    if version >= 2 {
        // SAFETY: `cb` as above; the tables are `'static` and never move.
        let categories =
            unsafe { rust_libretro::environment::set_core_options_v2(cb, &v2_tables().options) };
        return Declared::V2 { categories };
    }
    // SAFETY: `cb` as above; a `'static`, null-key-terminated array.
    if unsafe { rust_libretro::environment::set_variables(cb, &v0_tables().variables) } {
        Declared::V0
    } else {
        Declared::Refused
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The v0 string of the one option v3.5.0 declared, byte for byte: a
    /// frontend that stored it compares against it.
    #[test]
    fn the_four_score_v0_string_is_the_v3_5_0_one() {
        assert_eq!(
            v0_string(&OPTIONS[0]).as_c_str(),
            c"Four Score (players 3 and 4); disabled|enabled"
        );
    }

    #[test]
    fn keys_are_unique_and_every_category_exists() {
        let mut keys: Vec<&CStr> = OPTIONS.iter().map(|o| o.key).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), OPTIONS.len(), "duplicate option key");
        for spec in &OPTIONS {
            assert!(
                CATEGORIES.iter().any(|(k, _, _)| *k == spec.category),
                "{:?} names an undeclared category",
                spec.key
            );
            // libretro.h: valid key characters are [a-z, A-Z, 0-9, _, -].
            assert!(
                spec.key
                    .to_bytes()
                    .iter()
                    .all(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
            );
        }
    }

    #[test]
    fn unanswered_and_unknown_values_read_as_the_defaults() {
        assert_eq!(OptionValues::read(|_| None), OptionValues::default());
        assert_eq!(
            OptionValues::read(|_| Some("maybe".to_owned())),
            OptionValues::default()
        );
        assert!(OptionValues::read(|_| Some("enabled".to_owned())).four_score);
    }
}
