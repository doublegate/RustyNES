# NES Input Handling

**Table of Contents**

- [Overview](#overview)
- [Standard Controller](#standard-controller)
  - [Button Layout](#button-layout)
  - [Hardware Implementation](#hardware-implementation)
  - [Register Interface](#register-interface)
- [Controller Polling](#controller-polling)
- [Expansion Port Devices](#expansion-port-devices)
- [Implementation](#implementation)
- [DPCM Conflict](#dpcm-conflict)
- [Testing](#testing)
- [References](#references)

---

## Overview

The NES provides two controller ports accessed through memory-mapped registers at **$4016** (Controller 1) and **$4017** (Controller 2 / Frame Counter). Controllers use a **parallel-to-serial shift register** (4021 IC) to report button states one bit at a time.

### Key Characteristics

- **Register**: $4016 (Controller 1), $4017 (Controller 2)
- **Protocol**: Strobe + 8 serial reads
- **Button Order**: A, B, Select, Start, Up, Down, Left, Right
- **Reading**: 1 = pressed, 0 = not pressed
- **Clock Rate**: CPU speed (1.789773 MHz)

---

## Standard Controller

### Button Layout

```
      SELECT  START
        [=]    [=]

    [D-PAD]        [B] [A]
      ↑
    ← + →
      ↓
```

**8 Buttons**:

1. A
2. B
3. Select
4. Start
5. Up
6. Down
7. Left
8. Right

### Hardware Implementation

**4021 Shift Register**:

- **Parallel Load**: When strobe is high, button states continuously load
- **Serial Output**: When strobe is low, buttons read out one bit at a time

**Electrical Properties**:

- **Active Low**: Button pressed = low signal on data line
- **Inverted Reading**: Low signal reads as 1, high signal reads as 0
- **Pull-up**: Data line pulled high when no button pressed

### Register Interface

#### $4016: Controller Port 1 / Strobe

**Write** (sets strobe for both controllers):

```
Bit 0: Strobe (1 = latch buttons, 0 = serial mode)
Bits 1-7: Unused (some expansion devices may use)
```

**Read** (Controller 1 data):

```
Bit 0: Controller 1 button state (current bit)
Bits 1-4: Expansion port data (varies by device)
Bits 5-7: Open bus (typically $40)
```

#### $4017: Controller Port 2 / Frame Counter

**Write** (APU Frame Counter):

```
Used by APU for frame counter mode
```

**Read** (Controller 2 data):

```
Bit 0: Controller 2 button state (current bit)
Bits 1-4: Expansion port data
Bits 5-7: Open bus (typically $40 or $41)
```

---

## Controller Polling

### Polling Sequence

**Step 1: Strobe** (latch current button states)

```assembly
LDA #$01
STA $4016       ; Strobe = 1 (latch buttons)
LDA #$00
STA $4016       ; Strobe = 0 (enable serial reads)
```

**Step 2: Read 8 Buttons**

```assembly
; Read 8 buttons for Controller 1
LDX #8
ReadLoop:
    LDA $4016   ; Read one bit
    LSR A       ; Shift bit 0 into carry
    ROR Buttons ; Rotate carry into Buttons variable
    DEX
    BNE ReadLoop

; Buttons now contains: Right, Left, Down, Up, Start, Select, B, A
```

### Timing Considerations

**Minimum Strobe Duration**: 10-12 CPU cycles
**Reads**: Space reads by 2+ CPU cycles each

**Good Practice**: Strobe once per frame during VBlank

```assembly
NMI_Handler:
    ; ... other VBlank tasks ...

    ; Poll controllers
    JSR ReadControllers

    RTI
```

### Button State Format

After polling, buttons are typically stored as:

```
Bit 7: A
Bit 6: B
Bit 5: Select
Bit 4: Start
Bit 3: Up
Bit 2: Down
Bit 1: Left
Bit 0: Right
```

---

## Expansion Port Devices

### Zapper (Light Gun)

**$4016/$4017 bits**:

- **Bit 3**: Light sense (1 = light detected)
- **Bit 4**: Trigger (1 = pulled)

**Usage**: Games like Duck Hunt, Hogan's Alley

### Arkanoid Paddle

**Clocked serial interface** for analog position data

### Power Pad

**8×4 button mat** for games like World Class Track Meet

### Four Score

**4-player adapter** allowing 4 controllers:

- Controllers 3 and 4 read through extended bit sequence

**Detection**:

```
Read bits 9-24 from $4016/$4017
Four Score signature: $08 in bits 17-24
```

### Famicom Expansion Port

**Additional Devices**:

- Famicom Keyboard
- Mahjong Controller
- Barcode Battler
- Datach Joint ROM System

---

## Implementation

### Controller Structure

```rust
pub struct Controller {
    buttons: u8,         // Current button states
    shift_register: u8,  // Serial shift register
    strobe: bool,        // Strobe state
    bit_index: u8,       // Current bit being read
}

impl Controller {
    pub fn new() -> Self {
        Self {
            buttons: 0,
            shift_register: 0,
            strobe: false,
            bit_index: 0,
        }
    }

    pub fn set_button(&mut self, button: Button, pressed: bool) {
        if pressed {
            self.buttons |= button as u8;
        } else {
            self.buttons &= !(button as u8);
        }
    }

    pub fn write_strobe(&mut self, value: u8) {
        let new_strobe = (value & 0x01) != 0;

        if self.strobe && !new_strobe {
            // Falling edge: Latch buttons into shift register
            self.shift_register = self.buttons;
            self.bit_index = 0;
        }

        self.strobe = new_strobe;
    }

    pub fn read(&mut self) -> u8 {
        if self.strobe {
            // While strobing, always return button A
            return (self.buttons & 0x01) | 0x40; // Open bus $40
        }

        // Read current bit
        let value = ((self.shift_register >> self.bit_index) & 0x01) | 0x40;

        // Advance to next bit
        self.bit_index += 1;
        if self.bit_index > 7 {
            // After 8 bits, always return 1
            self.bit_index = 8;
        }

        value
    }
}

#[repr(u8)]
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
```

### Bus Integration

```rust
impl Bus {
    pub fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x4016 => self.controller1.read(),
            0x4017 => self.controller2.read(),
            _ => { /* ... other reads ... */ }
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x4016 => {
                self.controller1.write_strobe(value);
                self.controller2.write_strobe(value);
            }
            0x4017 => {
                // APU Frame Counter (not controller write)
                self.apu.write_register(addr, value);
            }
            _ => { /* ... other writes ... */ }
        }
    }
}
```

### Frontend Integration

```rust
// Example: egui frontend
pub fn handle_input(&mut self, key: KeyCode, pressed: bool) {
    let button = match key {
        KeyCode::Z     => Button::A,
        KeyCode::X     => Button::B,
        KeyCode::Return => Button::Start,
        KeyCode::RShift => Button::Select,
        KeyCode::Up    => Button::Up,
        KeyCode::Down  => Button::Down,
        KeyCode::Left  => Button::Left,
        KeyCode::Right => Button::Right,
        _ => return,
    };

    self.console.controller1.set_button(button, pressed);
}
```

---

## DPCM Conflict

### The Problem

When the **APU DMC channel** is playing samples, it can cause **false reads** from controller registers.

**Cause**: DMC DMA steals CPU cycles, causing an extra clock edge on the shift register during a $4016/$4017 read, resulting in a **dropped bit**.

### Manifestations

- Spurious button presses
- Missed inputs
- "Phantom" movements

### Workarounds

**Game Side** (original games):

- Poll controllers multiple times and compare
- Avoid DPCM during critical input moments
- Use majority vote from multiple polls

**Emulator Side**:

- Accurately emulate DMC DMA timing
- Detect simultaneous DMC read + controller read
- Optionally provide "conflict-free" mode for convenience

### Implementation

```rust
impl Bus {
    pub fn read_controller(&mut self, addr: u16) -> u8 {
        if self.dmc_dma_active && self.accurate_dmc_conflicts {
            // Simulate double-clock: drop a bit
            self.controller1.read(); // First read (dropped)
        }

        self.controller1.read() // Actual read
    }
}
```

---

## Testing

### Unit Tests

```rust
#[test]
fn test_controller_polling() {
    let mut controller = Controller::new();

    // Press A and Start
    controller.set_button(Button::A, true);
    controller.set_button(Button::Start, true);

    // Strobe
    controller.write_strobe(0x01);
    controller.write_strobe(0x00);

    // Read buttons
    let a = controller.read() & 0x01;
    let b = controller.read() & 0x01;
    let select = controller.read() & 0x01;
    let start = controller.read() & 0x01;

    assert_eq!(a, 1);      // A pressed
    assert_eq!(b, 0);      // B not pressed
    assert_eq!(select, 0); // Select not pressed
    assert_eq!(start, 1);  // Start pressed
}

#[test]
fn test_open_bus_behavior() {
    let mut controller = Controller::new();

    controller.write_strobe(0x01);
    controller.write_strobe(0x00);

    for _ in 0..8 {
        controller.read();
    }

    // Bit 9 and beyond should return 1 with open bus $40
    let beyond = controller.read();
    assert_eq!(beyond, 0x41); // Bit 0 = 1, bits 6-7 = $40
}
```

### Integration Tests

**Test ROM**: `controller_test.nes`

- Verifies polling sequence
- Tests button combinations
- Checks open bus values

---

## References

- [NesDev Wiki: Standard Controller](https://www.nesdev.org/wiki/Standard_controller)
- [NesDev Wiki: Controller Reading](https://www.nesdev.org/wiki/Controller_reading)
- [NesDev Wiki: Controller Reading Code](https://www.nesdev.org/wiki/Controller_reading_code)
- [NesDev Wiki: Input Devices](https://www.nesdev.org/wiki/Input_devices)
- [NesDev Wiki: Four Score](https://www.nesdev.org/wiki/Four_score)

---

**Related Documents**:

- [MEMORY_MAP.md](../bus/MEMORY_MAP.md) - Register locations
- [APU_OVERVIEW.md](../apu/APU_OVERVIEW.md) - DMC channel details
- [ARCHITECTURE.md](../ARCHITECTURE.md) - System overview
