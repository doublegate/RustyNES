//! Controller input as an agent spells it.
//!
//! A pad is a list of button names (`["A", "Right"]`), case-insensitive. The
//! list form is chosen over a bitmask because a language model writes names
//! far more reliably than it composes `0x81`, and because an unknown name can
//! be refused with a message that lists the valid ones — a misspelt `"Strat"`
//! must not silently become "no button", which would turn a typo into a
//! perfectly reproducible but wrong experiment.

use rustynes_core::{Buttons, FrameInput};
use schemars::JsonSchema;
use serde::Deserialize;

/// The eight standard-controller button names, in the order the shift
/// register reports them.
pub const BUTTON_NAMES: [&str; 8] = ["A", "B", "Select", "Start", "Up", "Down", "Left", "Right"];

/// Buttons held on each of the four ports. An omitted port means "released".
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PadsArg {
    /// Player 1 buttons, e.g. `["A", "Right"]`.
    #[serde(default)]
    pub p1: Vec<String>,
    /// Player 2 buttons.
    #[serde(default)]
    pub p2: Vec<String>,
    /// Player 3 buttons (read only with a Four Score).
    #[serde(default)]
    pub p3: Vec<String>,
    /// Player 4 buttons (read only with a Four Score).
    #[serde(default)]
    pub p4: Vec<String>,
}

impl PadsArg {
    /// Resolve every port's names to a [`FrameInput`].
    ///
    /// # Errors
    ///
    /// The first unknown button name, with the list of valid ones.
    pub fn to_frame_input(&self) -> Result<FrameInput, String> {
        Ok(FrameInput::four_players(
            parse_buttons(&self.p1)?,
            parse_buttons(&self.p2)?,
            parse_buttons(&self.p3)?,
            parse_buttons(&self.p4)?,
        ))
    }
}

/// Parse one pad's button names.
///
/// # Errors
///
/// An unknown name (the message lists the valid ones).
pub fn parse_buttons(names: &[String]) -> Result<Buttons, String> {
    let mut b = Buttons::empty();
    for name in names {
        b |= match name.to_ascii_lowercase().as_str() {
            "a" => Buttons::A,
            "b" => Buttons::B,
            "select" => Buttons::SELECT,
            "start" => Buttons::START,
            "up" => Buttons::UP,
            "down" => Buttons::DOWN,
            "left" => Buttons::LEFT,
            "right" => Buttons::RIGHT,
            _ => {
                return Err(format!(
                    "unknown button {name:?}; valid names are {}",
                    BUTTON_NAMES.join(", ")
                ));
            }
        };
    }
    Ok(b)
}

/// Render one pad's buttons as names (the inverse of [`parse_buttons`]).
#[must_use]
pub fn button_names(b: Buttons) -> Vec<&'static str> {
    let bits = [
        Buttons::A,
        Buttons::B,
        Buttons::SELECT,
        Buttons::START,
        Buttons::UP,
        Buttons::DOWN,
        Buttons::LEFT,
        Buttons::RIGHT,
    ];
    BUTTON_NAMES
        .iter()
        .zip(bits)
        .filter(|(_, bit)| b.contains(*bit))
        .map(|(name, _)| *name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_are_case_insensitive() {
        let b = parse_buttons(&["a".into(), "RIGHT".into(), "Start".into()]).unwrap();
        assert_eq!(b, Buttons::A | Buttons::RIGHT | Buttons::START);
        assert_eq!(button_names(b), ["A", "Start", "Right"]);
    }

    #[test]
    fn an_unknown_name_is_refused_not_dropped() {
        let err = parse_buttons(&["Strat".into()]).unwrap_err();
        assert!(err.contains("Strat") && err.contains("Start"), "{err}");
    }
}
