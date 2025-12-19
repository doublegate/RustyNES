//! Keyboard input mapping for NES controllers.
//!
//! Provides default keyboard mappings for Player 1 and Player 2, following
//! common emulator conventions.

use iced::keyboard::{key::Named, Key};
use rustynes_core::Button as NesButton;
use std::collections::HashMap;

/// Keyboard mapper for both players
#[derive(Debug, Clone)]
pub struct KeyboardMapper {
    /// Player 1 key mappings
    player1: HashMap<Key, NesButton>,
    /// Player 2 key mappings
    player2: HashMap<Key, NesButton>,
}

impl KeyboardMapper {
    /// Create new keyboard mapper with default mappings
    #[must_use]
    pub fn new() -> Self {
        Self {
            player1: Self::default_player1_mapping(),
            player2: Self::default_player2_mapping(),
        }
    }

    /// Default Player 1 mapping (Arrow keys + Z/X)
    ///
    /// # Mappings
    ///
    /// - Arrow keys: D-pad
    /// - Z: B button
    /// - X: A button
    /// - Right Shift: Select
    /// - Enter: Start
    fn default_player1_mapping() -> HashMap<Key, NesButton> {
        let mut map = HashMap::new();

        // D-pad (Arrow keys)
        map.insert(Key::Named(Named::ArrowUp), NesButton::Up);
        map.insert(Key::Named(Named::ArrowDown), NesButton::Down);
        map.insert(Key::Named(Named::ArrowLeft), NesButton::Left);
        map.insert(Key::Named(Named::ArrowRight), NesButton::Right);

        // Face buttons (Z/X - common emulator convention)
        map.insert(Key::Character("z".into()), NesButton::B);
        map.insert(Key::Character("x".into()), NesButton::A);

        // System buttons
        map.insert(Key::Named(Named::Shift), NesButton::Select);
        map.insert(Key::Named(Named::Enter), NesButton::Start);

        map
    }

    /// Default Player 2 mapping (WASD + I/O)
    ///
    /// # Mappings
    ///
    /// - WASD: D-pad
    /// - I: B button
    /// - O: A button
    /// - Tab: Select
    /// - Space: Start
    fn default_player2_mapping() -> HashMap<Key, NesButton> {
        let mut map = HashMap::new();

        // D-pad (WASD)
        map.insert(Key::Character("w".into()), NesButton::Up);
        map.insert(Key::Character("s".into()), NesButton::Down);
        map.insert(Key::Character("a".into()), NesButton::Left);
        map.insert(Key::Character("d".into()), NesButton::Right);

        // Face buttons
        map.insert(Key::Character("i".into()), NesButton::B);
        map.insert(Key::Character("o".into()), NesButton::A);

        // System buttons
        map.insert(Key::Named(Named::Tab), NesButton::Select);
        map.insert(Key::Named(Named::Space), NesButton::Start);

        map
    }

    /// Map a key to Player 1 NES button
    #[must_use]
    pub fn map_player1(&self, key: &Key) -> Option<NesButton> {
        self.player1.get(key).copied()
    }

    /// Map a key to Player 2 NES button
    #[must_use]
    pub fn map_player2(&self, key: &Key) -> Option<NesButton> {
        self.player2.get(key).copied()
    }

    /// Set custom Player 1 mapping
    #[allow(dead_code)] // Future: configurable controls in settings
    pub fn set_player1_mapping(&mut self, key: Key, button: NesButton) {
        self.player1.insert(key, button);
    }

    /// Set custom Player 2 mapping
    #[allow(dead_code)] // Future: configurable controls in settings
    pub fn set_player2_mapping(&mut self, key: Key, button: NesButton) {
        self.player2.insert(key, button);
    }

    /// Clear Player 1 mapping for a key
    #[allow(dead_code)] // Future: configurable controls in settings
    pub fn clear_player1_mapping(&mut self, key: &Key) {
        self.player1.remove(key);
    }

    /// Clear Player 2 mapping for a key
    #[allow(dead_code)] // Future: configurable controls in settings
    pub fn clear_player2_mapping(&mut self, key: &Key) {
        self.player2.remove(key);
    }

    /// Reset to default mappings
    #[allow(dead_code)] // Future: reset button in settings
    pub fn reset(&mut self) {
        self.player1 = Self::default_player1_mapping();
        self.player2 = Self::default_player2_mapping();
    }
}

impl Default for KeyboardMapper {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_player1_mapping() {
        let mapper = KeyboardMapper::new();

        // Test D-pad
        assert_eq!(
            mapper.map_player1(&Key::Named(Named::ArrowUp)),
            Some(NesButton::Up)
        );
        assert_eq!(
            mapper.map_player1(&Key::Named(Named::ArrowDown)),
            Some(NesButton::Down)
        );

        // Test face buttons
        assert_eq!(
            mapper.map_player1(&Key::Character("z".into())),
            Some(NesButton::B)
        );
        assert_eq!(
            mapper.map_player1(&Key::Character("x".into())),
            Some(NesButton::A)
        );

        // Test system buttons
        assert_eq!(
            mapper.map_player1(&Key::Named(Named::Enter)),
            Some(NesButton::Start)
        );
    }

    #[test]
    fn test_player2_mapping() {
        let mapper = KeyboardMapper::new();

        // Test WASD
        assert_eq!(
            mapper.map_player2(&Key::Character("w".into())),
            Some(NesButton::Up)
        );
        assert_eq!(
            mapper.map_player2(&Key::Character("s".into())),
            Some(NesButton::Down)
        );

        // Test face buttons
        assert_eq!(
            mapper.map_player2(&Key::Character("i".into())),
            Some(NesButton::B)
        );
    }

    #[test]
    fn test_custom_mapping() {
        let mut mapper = KeyboardMapper::new();

        // Set custom Player 1 mapping
        mapper.set_player1_mapping(Key::Character("j".into()), NesButton::A);
        assert_eq!(
            mapper.map_player1(&Key::Character("j".into())),
            Some(NesButton::A)
        );

        // Clear it
        mapper.clear_player1_mapping(&Key::Character("j".into()));
        assert_eq!(mapper.map_player1(&Key::Character("j".into())), None);
    }

    #[test]
    fn test_reset() {
        let mut mapper = KeyboardMapper::new();

        // Modify mapping
        mapper.set_player1_mapping(Key::Character("q".into()), NesButton::Start);

        // Reset to defaults
        mapper.reset();

        // Custom mapping should be gone
        assert_eq!(mapper.map_player1(&Key::Character("q".into())), None);

        // Default should still work
        assert_eq!(
            mapper.map_player1(&Key::Named(Named::Enter)),
            Some(NesButton::Start)
        );
    }
}
