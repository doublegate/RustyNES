# [Milestone 5] Sprint 5.6: Input Handling

**Status:** ✅ COMPLETED
**Started:** December 19, 2025
**Completed:** December 19, 2025
**Duration:** 1 day (part of M5 integration)
**Assignee:** Claude Code / Developer
**Sprint:** M5-S6 (Integration - Input Handling)
**Progress:** 100%

---

## Overview

This sprint implements the **NES controller input system**, enabling games to read player input through the standard controller protocol. This includes strobe-based polling, shift register emulation, button state management, and proper integration with the bus system.

### Goals

- ⏳ Standard controller emulation (8 buttons)
- ⏳ Strobe protocol implementation
- ⏳ Shift register behavior
- ⏳ Bus integration ($4016, $4017)
- ⏳ Open bus behavior
- ⏳ Frontend input API
- ⏳ Two-player support
- ⏳ Zero unsafe code

### Prerequisites

- ✅ M5-S2 Bus system complete (memory routing)
- ✅ APU crate exists (shares $4017 register)

---

## Tasks

### Task 1: Define Controller Structure (2 hours)

**File:** `crates/rustynes-core/src/input/controller.rs`

**Objective:** Create controller struct with shift register emulation.

#### Subtasks

1. Create `Controller` struct with:
   - Button state (8-bit field)
   - Shift register (current serial output)
   - Strobe state (latch mode)
   - Bit index (0-8+)

2. Define `Button` enum with bitflags

3. Implement state management methods

**Acceptance Criteria:**

- [ ] Controller struct holds all necessary state
- [ ] Button enum covers all 8 buttons
- [ ] Shift register emulation is accurate

**Implementation:**

```rust
/// NES standard controller (8 buttons)
pub struct Controller {
    /// Current button states (bit field)
    buttons: u8,

    /// Shift register for serial reads
    shift_register: u8,

    /// Strobe state (true = latching, false = serial mode)
    strobe: bool,

    /// Current bit index (0-8, then 8+ returns 1)
    bit_index: u8,
}

/// NES controller buttons
#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Button {
    A      = 0b0000_0001,
    B      = 0b0000_0010,
    Select = 0b0000_0100,
    Start  = 0b0000_1000,
    Up     = 0b0001_0000,
    Down   = 0b0010_0000,
    Left   = 0b0100_0000,
    Right  = 0b1000_0000,
}

impl Controller {
    /// Create new controller (all buttons released)
    pub fn new() -> Self {
        Self {
            buttons: 0,
            shift_register: 0,
            strobe: false,
            bit_index: 0,
        }
    }

    /// Set button state
    pub fn set_button(&mut self, button: Button, pressed: bool) {
        if pressed {
            self.buttons |= button as u8;
        } else {
            self.buttons &= !(button as u8);
        }
    }

    /// Set all button states at once
    pub fn set_buttons(&mut self, buttons: u8) {
        self.buttons = buttons;
    }

    /// Get current button state
    pub fn get_button(&self, button: Button) -> bool {
        (self.buttons & (button as u8)) != 0
    }

    /// Get all button states
    pub fn buttons(&self) -> u8 {
        self.buttons
    }

    /// Power-on reset
    pub fn reset(&mut self) {
        self.buttons = 0;
        self.shift_register = 0;
        self.strobe = false;
        self.bit_index = 0;
    }
}

impl Default for Controller {
    fn default() -> Self {
        Self::new()
    }
}
```

---

### Task 2: Implement Strobe Protocol (2 hours)

**File:** `crates/rustynes-core/src/input/controller.rs` (continued)

**Objective:** Implement $4016 strobe write and serial read logic.

#### Subtasks

1. Implement `write_strobe(value: u8)`:
   - Detect strobe edges (high → low)
   - Latch buttons into shift register on falling edge
   - Reset bit index

2. Implement `read() -> u8`:
   - Return current bit + open bus ($40)
   - Advance bit index
   - Return 1 for bits 8+

3. Handle continuous strobe (always return A button)

**Acceptance Criteria:**

- [ ] Strobe latches button state correctly
- [ ] Serial reads return buttons in correct order (A, B, Select, Start, Up, Down, Left, Right)
- [ ] Open bus behavior matches hardware ($40)
- [ ] Reads beyond bit 8 return 1

**Implementation:**

```rust
impl Controller {
    /// Write to strobe register ($4016)
    ///
    /// Bit 0: Strobe (1 = latch buttons, 0 = serial mode)
    ///
    /// Falling edge (1 → 0) latches current button states into shift register.
    pub fn write_strobe(&mut self, value: u8) {
        let new_strobe = (value & 0x01) != 0;

        // Detect falling edge: strobe goes from 1 → 0
        if self.strobe && !new_strobe {
            // Latch current button states into shift register
            self.shift_register = self.buttons;
            self.bit_index = 0;
        }

        self.strobe = new_strobe;
    }

    /// Read from controller data register ($4016 or $4017)
    ///
    /// Returns:
    /// - Bit 0: Current button bit (1 = pressed, 0 = not pressed)
    /// - Bits 1-4: Expansion port data (unused, varies)
    /// - Bits 5-7: Open bus (typically $40)
    ///
    /// Reading sequence (after strobe):
    /// 1. A
    /// 2. B
    /// 3. Select
    /// 4. Start
    /// 5. Up
    /// 6. Down
    /// 7. Left
    /// 8. Right
    /// 9+: Always 1
    pub fn read(&mut self) -> u8 {
        if self.strobe {
            // While strobing, always return A button state
            // (hardware continuously reloads shift register)
            return (self.buttons & 0x01) | 0x40;
        }

        // Read current bit from shift register
        let bit = if self.bit_index < 8 {
            (self.shift_register >> self.bit_index) & 0x01
        } else {
            // Bits 8+ always return 1 (open bus behavior)
            1
        };

        // Advance to next bit
        if self.bit_index < 255 {
            self.bit_index += 1;
        }

        // Return bit 0 + open bus $40
        bit | 0x40
    }

    /// Peek at current read value without advancing shift register
    /// (useful for debugging, not used by hardware)
    #[cfg(test)]
    pub fn peek(&self) -> u8 {
        if self.strobe {
            return (self.buttons & 0x01) | 0x40;
        }

        let bit = if self.bit_index < 8 {
            (self.shift_register >> self.bit_index) & 0x01
        } else {
            1
        };

        bit | 0x40
    }
}
```

---

### Task 3: Bus Integration (1 hour)

**File:** `crates/rustynes-core/src/bus.rs`

**Objective:** Wire controllers to $4016 and $4017 registers.

#### Subtasks

1. Add `controller1` and `controller2` fields to `Bus`
2. Route $4016 reads → `controller1.read()`
3. Route $4017 reads → `controller2.read()` (shares with APU)
4. Route $4016 writes → both controllers' `write_strobe()`
5. Ensure $4017 writes go to APU (not controller)

**Acceptance Criteria:**

- [ ] $4016 write strobes both controllers
- [ ] $4016 read returns controller 1 data
- [ ] $4017 read returns controller 2 data
- [ ] $4017 write goes to APU, not controller 2

**Implementation:**

```rust
// In bus.rs

pub struct Bus {
    // ... existing fields ...
    pub controller1: Controller,
    pub controller2: Controller,
}

impl Bus {
    pub fn new(/* ... */) -> Self {
        Self {
            // ... existing initialization ...
            controller1: Controller::new(),
            controller2: Controller::new(),
        }
    }

    pub fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],
            0x2000..=0x3FFF => self.ppu.read_register(addr),
            0x4000..=0x4015 => self.apu.read_register(addr),
            0x4016 => self.controller1.read(),
            0x4017 => self.controller2.read(),
            0x4020..=0xFFFF => self.cartridge.read_prg(addr),
            _ => 0, // Open bus
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize] = value,
            0x2000..=0x3FFF => self.ppu.write_register(addr, value),
            0x4000..=0x4013 => self.apu.write_register(addr, value),
            0x4014 => self.oam_dma(value),
            0x4015 => self.apu.write_register(addr, value),
            0x4016 => {
                // Strobe write affects BOTH controllers
                self.controller1.write_strobe(value);
                self.controller2.write_strobe(value);
            }
            0x4017 => {
                // $4017 write goes to APU frame counter (NOT controller 2)
                self.apu.write_register(addr, value);
            }
            0x4020..=0xFFFF => self.cartridge.write_prg(addr, value),
            _ => {}
        }
    }

    pub fn reset(&mut self) {
        self.controller1.reset();
        self.controller2.reset();
        // ... reset other components ...
    }
}
```

---

### Task 4: Frontend Input API (2 hours)

**File:** `crates/rustynes-core/src/input/mod.rs`

**Objective:** Provide high-level API for frontends to inject input.

#### Subtasks

1. Create `InputManager` struct (optional abstraction layer)
2. Add methods to `Console`:
   - `set_button_1(button: Button, pressed: bool)`
   - `set_button_2(button: Button, pressed: bool)`
   - `set_controller_1(buttons: u8)`
   - `set_controller_2(buttons: u8)`

3. Document button mapping examples (keyboard, gamepad)

**Acceptance Criteria:**

- [ ] Console exposes input API
- [ ] Frontend can set individual buttons
- [ ] Frontend can set all buttons at once
- [ ] Two-player support works

**Implementation:**

```rust
// In console.rs

impl Console {
    /// Set controller 1 button state
    pub fn set_button_1(&mut self, button: Button, pressed: bool) {
        self.bus.controller1.set_button(button, pressed);
    }

    /// Set controller 2 button state
    pub fn set_button_2(&mut self, button: Button, pressed: bool) {
        self.bus.controller2.set_button(button, pressed);
    }

    /// Set all controller 1 buttons at once
    ///
    /// # Arguments
    ///
    /// * `buttons` - 8-bit field where each bit represents a button:
    ///   - Bit 0: A
    ///   - Bit 1: B
    ///   - Bit 2: Select
    ///   - Bit 3: Start
    ///   - Bit 4: Up
    ///   - Bit 5: Down
    ///   - Bit 6: Left
    ///   - Bit 7: Right
    pub fn set_controller_1(&mut self, buttons: u8) {
        self.bus.controller1.set_buttons(buttons);
    }

    /// Set all controller 2 buttons at once
    pub fn set_controller_2(&mut self, buttons: u8) {
        self.bus.controller2.set_buttons(buttons);
    }

    /// Get controller 1 button state
    pub fn get_button_1(&self, button: Button) -> bool {
        self.bus.controller1.get_button(button)
    }

    /// Get controller 2 button state
    pub fn get_button_2(&self, button: Button) -> bool {
        self.bus.controller2.get_button(button)
    }

    /// Get all controller 1 buttons
    pub fn controller_1_buttons(&self) -> u8 {
        self.bus.controller1.buttons()
    }

    /// Get all controller 2 buttons
    pub fn controller_2_buttons(&self) -> u8 {
        self.bus.controller2.buttons()
    }
}
```

**Frontend Example (egui/keyboard):**

```rust
// Example keyboard mapping for desktop frontend

impl DesktopApp {
    fn handle_keyboard(&mut self, key: KeyCode, pressed: bool) {
        let button = match key {
            // Controller 1 (Player 1)
            KeyCode::Z     => Some((1, Button::B)),      // Z = B
            KeyCode::X     => Some((1, Button::A)),      // X = A
            KeyCode::A     => Some((1, Button::Y)),      // (if SNES)
            KeyCode::S     => Some((1, Button::X)),      // (if SNES)
            KeyCode::Return => Some((1, Button::Start)),
            KeyCode::RShift => Some((1, Button::Select)),
            KeyCode::Up    => Some((1, Button::Up)),
            KeyCode::Down  => Some((1, Button::Down)),
            KeyCode::Left  => Some((1, Button::Left)),
            KeyCode::Right => Some((1, Button::Right)),

            // Controller 2 (Player 2) - NumPad
            KeyCode::Numpad1 => Some((2, Button::B)),
            KeyCode::Numpad2 => Some((2, Button::A)),
            KeyCode::NumpadEnter => Some((2, Button::Start)),
            KeyCode::NumpadPlus  => Some((2, Button::Select)),
            KeyCode::Numpad8 => Some((2, Button::Up)),
            KeyCode::Numpad5 => Some((2, Button::Down)),
            KeyCode::Numpad4 => Some((2, Button::Left)),
            KeyCode::Numpad6 => Some((2, Button::Right)),

            _ => None,
        };

        if let Some((player, button)) = button {
            if player == 1 {
                self.console.set_button_1(button, pressed);
            } else {
                self.console.set_button_2(button, pressed);
            }
        }
    }
}
```

---

### Task 5: Serialization Support (1 hour)

**File:** `crates/rustynes-core/src/input/controller.rs` (continued)

**Objective:** Add save state support for controller state.

#### Subtasks

1. Implement `Serializable` trait for `Controller`
2. Save: buttons, shift_register, strobe, bit_index (4 bytes)
3. Restore: all fields

**Acceptance Criteria:**

- [ ] Controller state serializes to 4 bytes
- [ ] Deserialization restores exact state
- [ ] Save states preserve controller state correctly

**Implementation:**

```rust
use crate::save_state::{Serializable, SerializeError};

impl Serializable for Controller {
    fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        let mut data = Vec::with_capacity(4);

        data.push(self.buttons);
        data.push(self.shift_register);
        data.push(self.strobe as u8);
        data.push(self.bit_index);

        Ok(data)
    }

    fn deserialize(&mut self, data: &[u8]) -> Result<usize, SerializeError> {
        if data.len() < 4 {
            return Err(SerializeError::InsufficientData {
                needed: 4,
                available: data.len(),
            });
        }

        self.buttons = data[0];
        self.shift_register = data[1];
        self.strobe = data[2] != 0;
        self.bit_index = data[3];

        Ok(4)
    }

    fn serialized_size(&self) -> usize {
        4
    }
}
```

---

### Task 6: Unit Tests (2 hours)

**File:** `crates/rustynes-core/src/input/controller.rs` (tests module)

**Objective:** Comprehensive unit tests for controller behavior.

#### Subtasks

1. Test: button state management
2. Test: strobe protocol (latch on falling edge)
3. Test: serial read sequence (8 buttons)
4. Test: continuous strobe (returns A)
5. Test: open bus behavior (reads 8+ return 1)
6. Test: serialization roundtrip

**Acceptance Criteria:**

- [ ] All tests pass
- [ ] Edge cases covered (strobe timing, multiple reads)
- [ ] Regression tests for common game polling patterns

**Implementation:**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_button_state() {
        let mut controller = Controller::new();

        // Initially all buttons released
        assert_eq!(controller.buttons(), 0);

        // Press A
        controller.set_button(Button::A, true);
        assert!(controller.get_button(Button::A));
        assert_eq!(controller.buttons(), 0b0000_0001);

        // Press Start
        controller.set_button(Button::Start, true);
        assert!(controller.get_button(Button::Start));
        assert_eq!(controller.buttons(), 0b0000_1001);

        // Release A
        controller.set_button(Button::A, false);
        assert!(!controller.get_button(Button::A));
        assert_eq!(controller.buttons(), 0b0000_1000);
    }

    #[test]
    fn test_strobe_latch() {
        let mut controller = Controller::new();

        // Press A and Start
        controller.set_button(Button::A, true);
        controller.set_button(Button::Start, true);

        // Strobe high
        controller.write_strobe(0x01);
        assert_eq!(controller.strobe, true);

        // Strobe low (falling edge = latch)
        controller.write_strobe(0x00);
        assert_eq!(controller.strobe, false);

        // Shift register should now contain button state
        assert_eq!(controller.shift_register, 0b0000_1001);
        assert_eq!(controller.bit_index, 0);
    }

    #[test]
    fn test_serial_read_sequence() {
        let mut controller = Controller::new();

        // Press A, Select, Down, Right
        controller.set_button(Button::A, true);
        controller.set_button(Button::Select, true);
        controller.set_button(Button::Down, true);
        controller.set_button(Button::Right, true);
        // buttons = 0b1010_0101

        // Strobe
        controller.write_strobe(0x01);
        controller.write_strobe(0x00);

        // Read 8 buttons in order
        let reads = [
            controller.read() & 0x01,  // A      = 1
            controller.read() & 0x01,  // B      = 0
            controller.read() & 0x01,  // Select = 1
            controller.read() & 0x01,  // Start  = 0
            controller.read() & 0x01,  // Up     = 0
            controller.read() & 0x01,  // Down   = 1
            controller.read() & 0x01,  // Left   = 0
            controller.read() & 0x01,  // Right  = 1
        ];

        assert_eq!(reads, [1, 0, 1, 0, 0, 1, 0, 1]);
    }

    #[test]
    fn test_continuous_strobe() {
        let mut controller = Controller::new();

        controller.set_button(Button::A, true);
        controller.set_button(Button::B, true);

        // Set strobe high
        controller.write_strobe(0x01);

        // While strobing, reads always return A button
        for _ in 0..10 {
            let value = controller.read();
            assert_eq!(value & 0x01, 1);  // A is pressed
        }
    }

    #[test]
    fn test_open_bus_behavior() {
        let mut controller = Controller::new();

        // Strobe and latch
        controller.write_strobe(0x01);
        controller.write_strobe(0x00);

        // Read 8 buttons
        for _ in 0..8 {
            let value = controller.read();
            assert_eq!(value & 0x40, 0x40);  // Open bus $40
        }

        // Read bits 9-15 (should return 1 + open bus)
        for _ in 0..7 {
            let value = controller.read();
            assert_eq!(value, 0x41);  // Bit 0 = 1, open bus $40
        }
    }

    #[test]
    fn test_strobe_edge_detection() {
        let mut controller = Controller::new();

        controller.set_button(Button::A, true);

        // Multiple high writes don't latch
        controller.write_strobe(0x01);
        controller.write_strobe(0x01);
        controller.write_strobe(0x01);
        assert_eq!(controller.bit_index, 0);
        assert_eq!(controller.shift_register, 0);  // Not latched yet

        // Falling edge latches
        controller.write_strobe(0x00);
        assert_eq!(controller.shift_register, 0b0000_0001);

        // Multiple low writes don't re-latch
        let prev = controller.shift_register;
        controller.write_strobe(0x00);
        controller.write_strobe(0x00);
        assert_eq!(controller.shift_register, prev);
    }

    #[test]
    fn test_serialization() {
        let mut controller = Controller::new();

        controller.set_button(Button::A, true);
        controller.set_button(Button::Start, true);
        controller.write_strobe(0x01);
        controller.write_strobe(0x00);
        controller.read();  // Advance bit index

        // Serialize
        let data = controller.serialize().unwrap();
        assert_eq!(data.len(), 4);

        // Deserialize into new controller
        let mut controller2 = Controller::new();
        let consumed = controller2.deserialize(&data).unwrap();
        assert_eq!(consumed, 4);

        // Verify state matches
        assert_eq!(controller2.buttons, controller.buttons);
        assert_eq!(controller2.shift_register, controller.shift_register);
        assert_eq!(controller2.strobe, controller.strobe);
        assert_eq!(controller2.bit_index, controller.bit_index);
    }

    #[test]
    fn test_reset() {
        let mut controller = Controller::new();

        // Set some state
        controller.set_button(Button::A, true);
        controller.write_strobe(0x01);
        controller.write_strobe(0x00);
        controller.read();

        // Reset
        controller.reset();

        // All state cleared
        assert_eq!(controller.buttons, 0);
        assert_eq!(controller.shift_register, 0);
        assert_eq!(controller.strobe, false);
        assert_eq!(controller.bit_index, 0);
    }
}
```

---

### Task 7: Documentation (1 hour)

**File:** `crates/rustynes-core/src/input/mod.rs`

**Objective:** Comprehensive module documentation with examples.

#### Subtasks

1. Module-level doc comments
2. API usage examples
3. Hardware protocol explanation
4. Frontend integration guide

**Acceptance Criteria:**

- [ ] All public items documented
- [ ] Examples compile
- [ ] Hardware protocol explained clearly

**Implementation:**

```rust
//! NES controller input handling.
//!
//! This module emulates the NES standard controller protocol, which uses a
//! **strobe-based parallel-to-serial shift register** (4021 IC) to read
//! 8 button states sequentially.
//!
//! # Hardware Protocol
//!
//! The NES controller protocol works as follows:
//!
//! 1. **Strobe** ($4016 write, bit 0):
//!    - Write 1: Continuously reload shift register (parallel mode)
//!    - Write 0: Enable serial reads (shift mode)
//!    - Falling edge (1 → 0) latches current button states
//!
//! 2. **Serial Read** ($4016/$4017 read):
//!    - Returns one button bit per read
//!    - Order: A, B, Select, Start, Up, Down, Left, Right
//!    - Reads 9+ always return 1
//!
//! # Registers
//!
//! - **$4016**: Controller 1 data (read) / Strobe (write)
//! - **$4017**: Controller 2 data (read) / APU Frame Counter (write)
//!
//! **Note**: $4016 writes strobe BOTH controllers simultaneously.
//!
//! # Usage Example
//!
//! ```no_run
//! use rustynes_core::{Console, Button};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let rom = std::fs::read("game.nes")?;
//! let mut console = Console::from_rom_bytes(&rom)?;
//!
//! // Set controller 1 button state
//! console.set_button_1(Button::A, true);       // Press A
//! console.set_button_1(Button::Start, true);   // Press Start
//!
//! // Step frames
//! for _ in 0..60 {
//!     console.step_frame();
//! }
//!
//! // Release buttons
//! console.set_button_1(Button::A, false);
//! console.set_button_1(Button::Start, false);
//! # Ok(())
//! # }
//! ```
//!
//! # Frontend Integration
//!
//! Frontends should map keyboard/gamepad inputs to NES buttons:
//!
//! ```no_run
//! # use rustynes_core::{Console, Button};
//! # struct KeyCode;
//! # impl KeyCode {
//! #     const Z: Self = Self; const X: Self = Self;
//! #     const Return: Self = Self; const RShift: Self = Self;
//! #     const Up: Self = Self; const Down: Self = Self;
//! #     const Left: Self = Self; const Right: Self = Self;
//! # }
//! fn handle_keyboard(console: &mut Console, key: KeyCode, pressed: bool) {
//!     let button = match key {
//!         KeyCode::Z     => Button::B,
//!         KeyCode::X     => Button::A,
//!         KeyCode::Return => Button::Start,
//!         KeyCode::RShift => Button::Select,
//!         KeyCode::Up    => Button::Up,
//!         KeyCode::Down  => Button::Down,
//!         KeyCode::Left  => Button::Left,
//!         KeyCode::Right => Button::Right,
//!         _ => return,
//!     };
//!
//!     console.set_button_1(button, pressed);
//! }
//! ```
//!
//! # Two-Player Support
//!
//! ```no_run
//! # use rustynes_core::{Console, Button};
//! # let mut console: Console = unimplemented!();
//! // Player 1 (keyboard)
//! console.set_button_1(Button::A, true);
//!
//! // Player 2 (numpad)
//! console.set_button_2(Button::A, true);
//! ```
//!
//! # Performance
//!
//! Controller reads are extremely fast (<1ns). There is no performance
//! impact from accurate emulation.

pub mod controller;

pub use controller::{Controller, Button};
```

---

## Acceptance Criteria

### Functionality

- [ ] Controller struct emulates shift register correctly
- [ ] Strobe protocol matches hardware (falling edge latches)
- [ ] Serial reads return buttons in correct order
- [ ] Open bus behavior correct ($40 + bit)
- [ ] Reads beyond bit 8 return 1
- [ ] Two controllers work independently
- [ ] Bus integration at $4016/$4017 correct

### Quality

- [ ] Zero unsafe code
- [ ] No panics on any input sequence
- [ ] Unit tests cover all edge cases
- [ ] Integration tests verify game polling works
- [ ] All public APIs documented
- [ ] Frontend API is ergonomic

---

## Dependencies

### External Crates

None (uses only std library)

### Internal Dependencies

- rustynes-core (bus, console)
- rustynes-apu (shares $4017 register)

---

## Related Documentation

- [INPUT_HANDLING.md](../../../docs/input/INPUT_HANDLING.md) - Complete hardware reference
- [MEMORY_MAP.md](../../../docs/bus/MEMORY_MAP.md) - Register locations
- [M5-S2-bus-memory-routing.md](M5-S2-bus-memory-routing.md) - Bus implementation

---

## Technical Notes

### Open Bus Behavior

The NES has **open bus** on unused address/data lines. For controller reads:

- Bits 5-7 typically read as $40 (0b0100_0000)
- This is implementation-defined but should be consistent

### DMC Conflict

When the **APU DMC channel** is playing samples, it can cause spurious controller reads:

- DMC DMA steals CPU cycles
- Extra clock edge on shift register
- Results in dropped bit

**Games work around this** by polling multiple times and using majority vote. Emulators can optionally implement this conflict for accuracy.

### Zapper Support

Future expansion can add Zapper (light gun) emulation:

- Reads from bits 3-4 of $4016/$4017
- Bit 3: Light sense (1 = light detected)
- Bit 4: Trigger (1 = pulled)

This sprint focuses on **standard controllers only**. Zapper support deferred to Phase 2.

### Four Score

**Four Score** is a 4-player adapter that extends the serial read sequence beyond 8 bits to encode 4 controllers. Detection:

- Read bits 17-24 from $4016/$4017
- Signature: $08 indicates Four Score presence

Deferred to Phase 2 (advanced features).

---

## Performance Targets

- **Memory:** 4 bytes per controller (8 bytes total)
- **Read Latency:** <1ns (trivial bit shift)
- **No heap allocations**

---

## Success Criteria

- [ ] All tasks complete
- [ ] Unit tests pass (100% coverage)
- [ ] Integration test: load ROM, inject input, verify response
- [ ] Super Mario Bros. responds to controller input correctly
- [ ] Two-player games work (e.g., Contra)
- [ ] Zero unsafe code
- [ ] Documentation complete
- [ ] Frontend API is simple and clear

---

**Sprint Status:** ⏳ PENDING
**Blocked By:** M5-S2 (Bus system)
**Next Milestone:** [Milestone 6: Desktop GUI](../milestone-6-gui/M6-OVERVIEW.md)
