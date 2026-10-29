//! The core options: one table, declared in whichever form the frontend
//! speaks, and read back into one [`OptionValues`] (v3.6.0,
//! `T-LIBRETRO-OPTIONS-V2`; the Vs. DIP switches, LR-03; the CPU overclock
//! and the sprite limit, `T-LIBRETRO-EMU-OPTIONS`), and the record of them a
//! save state carries ([`split_state`]).
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
/// The CPU-multiplier overclock (`Nes::set_cpu_overclock`), v3.6.0.
pub const CPU_OVERCLOCK_KEY: &CStr = c"rustynes_cpu_overclock";
/// Draw the sprites beyond the eighth on a line
/// (`Nes::set_sprite_limit_disabled`), v3.6.0.
pub const SPRITE_LIMIT_KEY: &CStr = c"rustynes_remove_sprite_limit";
/// The eight Vs. System DIP switches (LR-03), switch `n` =
/// `rustynes_vs_dip_<n>` = bit `n - 1` of `Nes::set_vs_dip`.
pub const VS_DIP_KEYS: [&CStr; 8] = [
    c"rustynes_vs_dip_1",
    c"rustynes_vs_dip_2",
    c"rustynes_vs_dip_3",
    c"rustynes_vs_dip_4",
    c"rustynes_vs_dip_5",
    c"rustynes_vs_dip_6",
    c"rustynes_vs_dip_7",
    c"rustynes_vs_dip_8",
];

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
pub const CATEGORIES: [(&CStr, &CStr, &CStr); 3] = [
    (
        c"input",
        c"Input",
        c"Controllers and adapters plugged into the console.",
    ),
    (
        c"emulation",
        c"Emulation",
        c"Changes to what the console does. A save state records them, so a loaded state, a netplay peer and a replay run with the setting it was made with.",
    ),
    (
        c"vs_system",
        c"Vs. System",
        c"The arcade cabinet's DIP switches. Only Vs. System games read them.",
    ),
];

/// The values of a Vs. DIP-switch option: the game database's factory
/// setting (the default, and what every release before v3.6.0 applied), or
/// the switch forced off or on. Three values per switch rather than one
/// 256-value list: a list is capped at 127 values
/// (`RETRO_NUM_CORE_OPTION_VALUES_MAX` less its terminator), and a menu of
/// eight on/off switches is how the cabinet's own bank reads.
const DIP_VALUES: &[(&CStr, &CStr)] = &[
    (c"database", c"Game default"),
    (c"off", c"Off"),
    (c"on", c"On"),
];

/// The values of the CPU overclock, `1..=MAX_CPU_OVERCLOCK`.
const OVERCLOCK_VALUES: &[(&CStr, &CStr)] = &[
    (c"1", c"x1 (stock)"),
    (c"2", c"x2"),
    (c"3", c"x3"),
    (c"4", c"x4"),
];

// The option offers exactly the multipliers the core accepts: a fifth value
// would be clamped to four by `Nes::set_cpu_overclock`, and a missing one
// would be unreachable.
const _: () = assert!(OVERCLOCK_VALUES.len() == rustynes_core::MAX_CPU_OVERCLOCK as usize);

/// A Vs. DIP-switch option.
const fn dip(
    key: &'static CStr,
    desc: &'static CStr,
    desc_categorized: &'static CStr,
) -> OptionSpec {
    OptionSpec {
        key,
        desc,
        desc_categorized,
        info: c"What each switch does depends on the game (its manual or operator card lists them). A game may read the switches only when it starts, so restart it after a change.",
        category: c"vs_system",
        values: DIP_VALUES,
    }
}

/// Every option the core declares, in menu order.
pub const OPTIONS: [OptionSpec; 11] = [
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
    OptionSpec {
        key: CPU_OVERCLOCK_KEY,
        desc: c"CPU Overclock",
        desc_categorized: c"CPU Overclock",
        info: c"Runs the CPU 2 to 4 times faster against an unchanged picture and sound, so a game that slows down gets more time per frame. No real console does this. It changes how a game plays, so a save state records it.",
        category: c"emulation",
        values: OVERCLOCK_VALUES,
    },
    OptionSpec {
        key: SPRITE_LIMIT_KEY,
        desc: c"Remove Sprite Limit",
        desc_categorized: c"Remove Sprite Limit",
        info: c"Draws the sprites beyond the eighth on a line, removing flicker. Picture only: the game sees the stock behaviour. Boards whose pattern reads have effects (MMC2, MMC4 and a few others) keep the limit.",
        category: c"emulation",
        values: &[(c"disabled", c"Off (8 per line)"), (c"enabled", c"On")],
    },
    dip(VS_DIP_KEYS[0], c"Vs. DIP Switch 1", c"DIP Switch 1"),
    dip(VS_DIP_KEYS[1], c"Vs. DIP Switch 2", c"DIP Switch 2"),
    dip(VS_DIP_KEYS[2], c"Vs. DIP Switch 3", c"DIP Switch 3"),
    dip(VS_DIP_KEYS[3], c"Vs. DIP Switch 4", c"DIP Switch 4"),
    dip(VS_DIP_KEYS[4], c"Vs. DIP Switch 5", c"DIP Switch 5"),
    dip(VS_DIP_KEYS[5], c"Vs. DIP Switch 6", c"DIP Switch 6"),
    dip(VS_DIP_KEYS[6], c"Vs. DIP Switch 7", c"DIP Switch 7"),
    dip(VS_DIP_KEYS[7], c"Vs. DIP Switch 8", c"DIP Switch 8"),
];

/// What the options say, parsed. `Default` is every option at its default,
/// which is also what a frontend that answers no `GET_VARIABLE` gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptionValues {
    /// `rustynes_four_score`.
    pub four_score: bool,
    /// `rustynes_cpu_overclock`, `1..=MAX_CPU_OVERCLOCK`.
    pub cpu_overclock: u8,
    /// `rustynes_remove_sprite_limit`.
    pub sprite_limit_disabled: bool,
    /// The switches forced by `rustynes_vs_dip_*`; bit `n` = switch `n + 1`.
    pub dip_mask: u8,
    /// The forced switches' positions, under `dip_mask` (bits outside it are
    /// zero).
    pub dip_value: u8,
}

impl Default for OptionValues {
    fn default() -> Self {
        Self::STOCK
    }
}

impl OptionValues {
    /// Every option at its default, as a `const` (the `retro_core!`
    /// instance is built in a `static`).
    pub const STOCK: Self = Self {
        four_score: false,
        cpu_overclock: 1,
        sprite_limit_disabled: false,
        dip_mask: 0,
        dip_value: 0,
    };

    /// Read every option through `get`, which answers a key with the
    /// frontend's value or `None`. An unknown value reads as the option's
    /// default rather than as something the menu never offered.
    pub fn read(mut get: impl FnMut(&CStr) -> Option<String>) -> Self {
        let mut out = Self::STOCK;
        out.four_score = get(FOUR_SCORE_KEY).as_deref() == Some("enabled");
        out.cpu_overclock = get(CPU_OVERCLOCK_KEY)
            .and_then(|v| v.parse::<u8>().ok())
            .filter(|k| (1..=rustynes_core::MAX_CPU_OVERCLOCK).contains(k))
            .unwrap_or(1);
        out.sprite_limit_disabled = get(SPRITE_LIMIT_KEY).as_deref() == Some("enabled");
        for (bit, key) in VS_DIP_KEYS.iter().enumerate() {
            match get(key).as_deref() {
                Some("off") => out.dip_mask |= 1 << bit,
                Some("on") => {
                    out.dip_mask |= 1 << bit;
                    out.dip_value |= 1 << bit;
                }
                _ => {}
            }
        }
        out
    }

    /// The emulation options in words, for the log: the ones away from
    /// their defaults, or "stock".
    pub fn describe(self) -> String {
        let mut parts = Vec::new();
        if self.cpu_overclock != 1 {
            parts.push(format!("CPU overclock x{}", self.cpu_overclock));
        }
        if self.sprite_limit_disabled {
            parts.push("sprite limit removed".to_owned());
        }
        if self.four_score {
            parts.push("Four Score".to_owned());
        }
        if self.dip_mask != 0 {
            parts.push(format!(
                "Vs. DIP switches forced {:#04x} of {:#04x}",
                self.dip_value, self.dip_mask
            ));
        }
        if parts.is_empty() {
            "stock".to_owned()
        } else {
            parts.join(", ")
        }
    }

    /// The DIP-switch byte a cartridge whose database setting is `base` runs
    /// with: `base`, with the forced switches replaced.
    pub const fn dip_for(self, base: u8) -> u8 {
        (base & !self.dip_mask) | (self.dip_value & self.dip_mask)
    }
}

/// Bytes the options record adds to the end of a save state.
pub const STATE_TRAILER_LEN: usize = 16;

/// The record's first eight bytes. Chosen to be unlike anything the core's
/// state ends with: a single console's state ends in zero padding, and a
/// cabinet's in section bytes.
const TRAILER_MAGIC: &[u8; 8] = b"RNLROPTS";

/// The record's layout version, refused when unknown.
const TRAILER_VERSION: u8 = 1;

impl OptionValues {
    /// The record of the options that change what the console does, as the
    /// last [`STATE_TRAILER_LEN`] bytes of a save state:
    ///
    /// | bytes | content |
    /// | --- | --- |
    /// | 0-7 | `RNLROPTS` |
    /// | 8 | layout version, 1 |
    /// | 9 | bit 0: Four Score, bit 1: sprite limit removed; others 0 |
    /// | 10 | CPU overclock, 1-4 |
    /// | 11 | Vs. DIP switches forced (mask) |
    /// | 12 | their positions (within the mask) |
    /// | 13-15 | 0 |
    ///
    /// Every field is an option a netplay peer or a replay must share: what
    /// `rustynes_core::HardwareOptions` records for the desktop's movies and
    /// netplay handshake, restricted to what this core lets a user set.
    pub fn trailer(self) -> [u8; STATE_TRAILER_LEN] {
        let mut out = [0u8; STATE_TRAILER_LEN];
        out[..8].copy_from_slice(TRAILER_MAGIC);
        out[8] = TRAILER_VERSION;
        out[9] = u8::from(self.four_score) | (u8::from(self.sprite_limit_disabled) << 1);
        out[10] = self.cpu_overclock;
        out[11] = self.dip_mask;
        out[12] = self.dip_value;
        out
    }

    /// Decode a record [`Self::trailer`] wrote, refusing any value the
    /// options cannot hold: the record comes from a file, or a netplay
    /// peer, and a value no menu offers is not one to run with.
    fn from_trailer(t: &[u8; STATE_TRAILER_LEN]) -> Result<Self, &'static str> {
        if t[8] != TRAILER_VERSION {
            return Err("an unknown options-record version");
        }
        if t[9] & !0b11 != 0 {
            return Err("an unknown options flag");
        }
        if !(1..=rustynes_core::MAX_CPU_OVERCLOCK).contains(&t[10]) {
            return Err("a CPU overclock outside 1-4");
        }
        if t[12] & !t[11] != 0 {
            return Err("a DIP switch position outside the forced mask");
        }
        if t[13..] != [0, 0, 0] {
            return Err("non-zero reserved bytes");
        }
        Ok(Self {
            four_score: t[9] & 1 != 0,
            sprite_limit_disabled: t[9] & 2 != 0,
            cpu_overclock: t[10],
            dip_mask: t[11],
            dip_value: t[12],
        })
    }
}

/// Split a save state into the core's own state and the options record at
/// its end.
///
/// `Ok((state, None))` for a state with no record, which is every state
/// written before v3.6.0: the whole slice is handed to the core, as before,
/// and the options stay as the menu has them. A record is recognised by its
/// magic in the last [`STATE_TRAILER_LEN`] bytes; an old single-console
/// state ends in zero padding there and a cabinet's in section data, so
/// neither can be mistaken for one. A record that is recognised but invalid
/// refuses the whole state.
///
/// # Errors
///
/// What is wrong with a recognised but invalid record.
pub fn split_state(slice: &[u8]) -> Result<(&[u8], Option<OptionValues>), &'static str> {
    let Some(at) = slice.len().checked_sub(STATE_TRAILER_LEN) else {
        return Ok((slice, None));
    };
    let (state, tail) = slice.split_at(at);
    let Ok(tail) = <&[u8; STATE_TRAILER_LEN]>::try_from(tail) else {
        return Ok((slice, None));
    };
    if &tail[..8] != TRAILER_MAGIC {
        return Ok((slice, None));
    }
    OptionValues::from_trailer(tail).map(|o| (state, Some(o)))
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

    #[test]
    fn dip_overrides_replace_only_the_forced_switches() {
        let v = OptionValues::read(|key| {
            (key == VS_DIP_KEYS[0])
                .then(|| "on".to_owned())
                .or_else(|| (key == VS_DIP_KEYS[7]).then(|| "off".to_owned()))
        });
        assert_eq!((v.dip_mask, v.dip_value), (0x81, 0x01));
        assert_eq!(v.dip_for(0x80), 0x01, "switch 8 forced off, 1 forced on");
        assert_eq!(
            v.dip_for(0x7E),
            0x7F,
            "the unforced switches keep the database's"
        );
        assert_eq!(OptionValues::STOCK.dip_for(0xA5), 0xA5);
    }

    #[test]
    fn the_overclock_reads_only_values_the_core_accepts() {
        for (text, k) in [("1", 1), ("2", 2), ("4", 4), ("0", 1), ("5", 1), ("x2", 1)] {
            let v = OptionValues::read(|key| (key == CPU_OVERCLOCK_KEY).then(|| text.to_owned()));
            assert_eq!(v.cpu_overclock, k, "{text:?}");
        }
        let v = OptionValues::read(|key| (key == SPRITE_LIMIT_KEY).then(|| "enabled".to_owned()));
        assert!(v.sprite_limit_disabled);
    }

    #[test]
    fn the_state_record_round_trips_and_refuses_what_no_menu_offers() {
        let options = OptionValues {
            four_score: true,
            cpu_overclock: 3,
            sprite_limit_disabled: true,
            dip_mask: 0x81,
            dip_value: 0x01,
        };
        let mut state = vec![7u8; 40];
        state.extend_from_slice(&options.trailer());
        let (core, back) = split_state(&state).expect("valid");
        assert_eq!(
            core,
            &[7u8; 40][..],
            "the core gets everything before the record"
        );
        assert_eq!(back, Some(options));

        // A state with no record, as every state before v3.6.0: untouched.
        let old = vec![0u8; 64];
        assert_eq!(split_state(&old), Ok((&old[..], None)));
        assert_eq!(split_state(&[1, 2, 3]), Ok((&[1u8, 2, 3][..], None)));

        for (byte, value, why) in [
            (8, 2, "version"),
            (9, 4, "flag"),
            (10, 0, "overclock"),
            (10, 5, "overclock"),
            (12, 0x02, "DIP position"),
            (15, 1, "reserved"),
        ] {
            let mut bad = state.clone();
            let at = bad.len() - STATE_TRAILER_LEN + byte;
            bad[at] = value;
            assert!(split_state(&bad).is_err(), "{why}: byte {byte} = {value}");
        }
    }
}
