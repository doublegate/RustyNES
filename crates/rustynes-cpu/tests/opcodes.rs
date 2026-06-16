//! Unit-level integration tests for the CPU using a flat 64 KiB RAM bus.
//!
//! These verify a representative slice of every opcode family. Comprehensive
//! coverage comes from the blargg `instr_test_v5` harness in `rustynes-test-harness`.

#![allow(clippy::large_stack_arrays, clippy::missing_const_for_fn)]

use rustynes_cpu::{Bus, Cpu, Status};

/// Flat 64 KiB RAM bus.
struct RamBus {
    ram: Box<[u8; 0x1_0000]>,
    cycles: u64,
}

impl RamBus {
    fn new() -> Self {
        Self {
            ram: Box::new([0u8; 0x1_0000]),
            cycles: 0,
        }
    }
    fn load(&mut self, addr: u16, bytes: &[u8]) {
        let base = addr as usize;
        self.ram[base..base + bytes.len()].copy_from_slice(bytes);
    }
}

impl Bus for RamBus {
    fn cpu_read(&mut self, addr: u16) -> u8 {
        self.ram[addr as usize]
    }
    fn cpu_write(&mut self, addr: u16, value: u8) {
        self.ram[addr as usize] = value;
    }
    fn on_cpu_cycle(&mut self) {
        self.cycles += 1;
    }
}

fn run(cpu: &mut Cpu, bus: &mut RamBus, instructions: usize) {
    for _ in 0..instructions {
        cpu.step(bus);
    }
}

fn cpu_at(pc: u16) -> Cpu {
    let mut c = Cpu::new();
    c.set_pc(pc);
    c
}

#[test]
fn lda_immediate_sets_n_z() {
    let mut bus = RamBus::new();
    bus.load(0x8000, &[0xA9, 0x00, 0xA9, 0x80, 0xA9, 0x42]);
    let mut cpu = cpu_at(0x8000);
    cpu.step(&mut bus);
    assert_eq!(cpu.a, 0x00);
    assert!(cpu.p.contains(Status::ZERO));
    cpu.step(&mut bus);
    assert_eq!(cpu.a, 0x80);
    assert!(cpu.p.contains(Status::NEGATIVE));
    cpu.step(&mut bus);
    assert_eq!(cpu.a, 0x42);
    assert!(!cpu.p.contains(Status::ZERO));
    assert!(!cpu.p.contains(Status::NEGATIVE));
}

#[test]
fn sta_zero_page() {
    let mut bus = RamBus::new();
    bus.load(0x8000, &[0xA9, 0xCD, 0x85, 0x42]);
    let mut cpu = cpu_at(0x8000);
    run(&mut cpu, &mut bus, 2);
    assert_eq!(bus.ram[0x42], 0xCD);
}

#[test]
fn adc_with_carry_and_overflow() {
    // A=0x50, +0x50 => 0xA0 with overflow set (positive + positive = negative).
    let mut bus = RamBus::new();
    bus.load(0x8000, &[0xA9, 0x50, 0x69, 0x50]);
    let mut cpu = cpu_at(0x8000);
    run(&mut cpu, &mut bus, 2);
    assert_eq!(cpu.a, 0xA0);
    assert!(cpu.p.contains(Status::OVERFLOW));
    assert!(!cpu.p.contains(Status::CARRY));
    assert!(cpu.p.contains(Status::NEGATIVE));
}

#[test]
fn sbc_basic() {
    // A=0x50; SEC; SBC #$30 => 0x20.
    let mut bus = RamBus::new();
    bus.load(0x8000, &[0xA9, 0x50, 0x38, 0xE9, 0x30]);
    let mut cpu = cpu_at(0x8000);
    run(&mut cpu, &mut bus, 3);
    assert_eq!(cpu.a, 0x20);
    assert!(cpu.p.contains(Status::CARRY)); // No borrow.
}

#[test]
fn jsr_rts_round_trip() {
    let mut bus = RamBus::new();
    // $8000: JSR $9000; $8003: NOP
    bus.load(0x8000, &[0x20, 0x00, 0x90, 0xEA]);
    bus.load(0x9000, &[0x60]); // RTS
    let mut cpu = cpu_at(0x8000);
    cpu.s = 0xFD;
    cpu.step(&mut bus); // JSR
    assert_eq!(cpu.pc, 0x9000);
    cpu.step(&mut bus); // RTS
    assert_eq!(cpu.pc, 0x8003);
}

#[test]
fn jmp_indirect_page_bug() {
    let mut bus = RamBus::new();
    bus.load(0x8000, &[0x6C, 0xFF, 0x10]); // JMP ($10FF)
    bus.ram[0x10FF] = 0x34;
    bus.ram[0x1000] = 0x12; // bug: high byte fetched from $1000, not $1100
    let mut cpu = cpu_at(0x8000);
    cpu.step(&mut bus);
    assert_eq!(cpu.pc, 0x1234);
}

#[test]
fn branch_taken_no_cross() {
    let mut bus = RamBus::new();
    bus.load(0x8000, &[0xA9, 0x00, 0xF0, 0x05]); // LDA #0; BEQ +5
    let mut cpu = cpu_at(0x8000);
    let _ = cpu.step(&mut bus);
    let cyc = cpu.step(&mut bus);
    assert_eq!(cpu.pc, 0x8009);
    assert_eq!(cyc, 3);
}

#[test]
fn branch_taken_page_cross() {
    let mut bus = RamBus::new();
    // Place BNE at $80FB so taken +5 crosses to $8101.
    bus.load(0x80FA, &[0xA9, 0x01, 0xD0, 0x04]);
    let mut cpu = cpu_at(0x80FA);
    let _ = cpu.step(&mut bus);
    let cyc = cpu.step(&mut bus);
    assert_eq!(cyc, 4);
}

#[test]
fn pha_php_round_trip() {
    // Push P first, then A: pull A first (gets last-pushed = A), then pull P.
    let mut bus = RamBus::new();
    // LDA #$AA; PHP; PHA; LDA #$00; PLA; PLP
    bus.load(0x8000, &[0xA9, 0xAA, 0x08, 0x48, 0xA9, 0x00, 0x68, 0x28]);
    let mut cpu = cpu_at(0x8000);
    cpu.s = 0xFD;
    run(&mut cpu, &mut bus, 4);
    let _ = cpu.step(&mut bus); // PLA -> 0xAA
    assert_eq!(cpu.a, 0xAA);
    let _ = cpu.step(&mut bus); // PLP
    // Specifically: push set B+U; pull clears B, sets U.
    assert!(!cpu.p.contains(Status::BREAK));
    assert!(cpu.p.contains(Status::UNUSED));
}

#[test]
fn dcp_decrements_then_compares() {
    let mut bus = RamBus::new();
    bus.load(0x8000, &[0xA9, 0x10, 0xC7, 0x42]); // LDA #$10; DCP $42
    bus.ram[0x42] = 0x11;
    let mut cpu = cpu_at(0x8000);
    run(&mut cpu, &mut bus, 2);
    assert_eq!(bus.ram[0x42], 0x10);
    assert!(cpu.p.contains(Status::ZERO));
    assert!(cpu.p.contains(Status::CARRY));
}

#[test]
fn isc_increments_then_subtracts() {
    let mut bus = RamBus::new();
    bus.load(0x8000, &[0xA9, 0x05, 0x38, 0xE7, 0x20]); // LDA #5; SEC; ISC $20
    bus.ram[0x20] = 0x01; // After INC -> 0x02, then SBC: A=5-2=3
    let mut cpu = cpu_at(0x8000);
    run(&mut cpu, &mut bus, 3);
    assert_eq!(bus.ram[0x20], 0x02);
    assert_eq!(cpu.a, 0x03);
}

#[test]
fn slo_shifts_then_ors() {
    let mut bus = RamBus::new();
    bus.load(0x8000, &[0xA9, 0x10, 0x07, 0x40]); // LDA #$10; SLO $40
    bus.ram[0x40] = 0x21; // After ASL -> 0x42, then ORA -> 0x52
    let mut cpu = cpu_at(0x8000);
    run(&mut cpu, &mut bus, 2);
    assert_eq!(bus.ram[0x40], 0x42);
    assert_eq!(cpu.a, 0x52);
}

#[test]
fn jam_halts_cpu() {
    let mut bus = RamBus::new();
    bus.load(0x8000, &[0x02, 0xA9, 0xFF]);
    let mut cpu = cpu_at(0x8000);
    cpu.step(&mut bus);
    assert!(cpu.is_jammed());
    let cyc = cpu.step(&mut bus);
    assert_eq!(cyc, 0);
    assert_eq!(cpu.a, 0); // LDA after JAM never executed
}

#[test]
fn brk_pushes_pc_plus_2_and_status_with_b() {
    let mut bus = RamBus::new();
    bus.load(0x8000, &[0x00]); // BRK
    bus.load(0xFFFE, &[0x00, 0x90]); // IRQ vector -> $9000
    let mut cpu = cpu_at(0x8000);
    cpu.s = 0xFD;
    cpu.p.remove(Status::INTERRUPT_DISABLE);
    cpu.step(&mut bus);
    assert_eq!(cpu.pc, 0x9000);
    assert!(cpu.p.contains(Status::INTERRUPT_DISABLE));
    // Pushed P should have B and U set; layout on stack: [PCH][PCL][P].
    let p_pushed = bus.ram[0x0100 | (cpu.s as usize + 1)];
    assert!(Status::from_bits_truncate(p_pushed).contains(Status::BREAK));
    assert!(Status::from_bits_truncate(p_pushed).contains(Status::UNUSED));
    let pcl = bus.ram[0x0100 | (cpu.s as usize + 2)];
    let pch = bus.ram[0x0100 | (cpu.s as usize + 3)];
    let pushed_pc = u16::from(pcl) | (u16::from(pch) << 8);
    assert_eq!(pushed_pc, 0x8002); // PC of BRK + 2
}

#[test]
fn cmp_flags() {
    let mut bus = RamBus::new();
    bus.load(0x8000, &[0xA9, 0x40, 0xC9, 0x40, 0xC9, 0x50, 0xC9, 0x30]);
    let mut cpu = cpu_at(0x8000);
    let _ = cpu.step(&mut bus);
    let _ = cpu.step(&mut bus); // CMP #$40 -> Z=1, C=1
    assert!(cpu.p.contains(Status::ZERO));
    assert!(cpu.p.contains(Status::CARRY));
    let _ = cpu.step(&mut bus); // CMP #$50 -> A<m -> N, !C
    assert!(!cpu.p.contains(Status::CARRY));
    assert!(cpu.p.contains(Status::NEGATIVE));
    let _ = cpu.step(&mut bus); // CMP #$30 -> A>m -> C
    assert!(cpu.p.contains(Status::CARRY));
}

#[test]
fn inx_dey_flags() {
    let mut bus = RamBus::new();
    bus.load(0x8000, &[0xA2, 0xFF, 0xE8, 0xA0, 0x01, 0x88]); // LDX #$FF; INX; LDY #$01; DEY
    let mut cpu = cpu_at(0x8000);
    run(&mut cpu, &mut bus, 4);
    assert_eq!(cpu.x, 0x00);
    assert_eq!(cpu.y, 0x00);
    assert!(cpu.p.contains(Status::ZERO));
}

/// Bus that asserts the IRQ line continuously starting at a configurable
/// instruction count. Used to probe the branch-IRQ-delay quirk.
struct IrqGateBus {
    ram: Box<[u8; 0x1_0000]>,
    cycles: u64,
    /// IRQ line goes high after this many `on_cpu_cycle` ticks have fired.
    irq_high_after_cycles: u64,
}

impl IrqGateBus {
    fn new(irq_high_after_cycles: u64) -> Self {
        Self {
            ram: Box::new([0u8; 0x1_0000]),
            cycles: 0,
            irq_high_after_cycles,
        }
    }
    fn load(&mut self, addr: u16, bytes: &[u8]) {
        let base = addr as usize;
        self.ram[base..base + bytes.len()].copy_from_slice(bytes);
    }
}

impl Bus for IrqGateBus {
    fn cpu_read(&mut self, addr: u16) -> u8 {
        self.ram[addr as usize]
    }
    fn cpu_write(&mut self, addr: u16, value: u8) {
        self.ram[addr as usize] = value;
    }
    fn poll_irq(&mut self) -> bool {
        self.cycles >= self.irq_high_after_cycles
    }
    fn on_cpu_cycle(&mut self) {
        self.cycles += 1;
    }
}

/// Branch-IRQ-delay quirk: a *taken* branch (no page cross) polls IRQ at the
/// same point a 2-cycle untaken branch would (the operand-fetch cycle).  The
/// extra "branch taken" cycle does NOT re-poll IRQ.  So an IRQ that asserts
/// during cycle 2 of a 3-cycle taken branch must NOT be serviced before the
/// next instruction; it must be deferred by one further instruction.
///
/// Setup: `CLI; BEQ +0; LDA #$AA; LDA #$BB`. CLI = 2 cycles, BEQ taken-no-cross
/// = 3 cycles. We arm the IRQ line to go high starting at bus cycle 4 (i.e.
/// during cycle 2 of BEQ — the "taken" cycle, AFTER the operand fetch poll on
/// cycle 1). Without the quirk, the IRQ would be first-sampled at BEQ's cycle 2
/// (tick 1, less than `last_tick` 2) and arm immediately — servicing before
/// LDA #$AA. With the quirk, BEQ does NOT re-sample IRQ on its taken / page-
/// cross cycles, so the next instruction's sample window catches it and the
/// LDA #$AA executes before IRQ is serviced.
#[test]
#[ignore = "permanent-by-design: pins the SUPERSEDED pre-master-clock interrupt-dispatch granularity on the mock bus. The master-clock core (now the default and ONLY scheduler) legitimately shifts when the mock-bus interrupt is recognized, so this unit assertion is kept ignored as a historical pin and cannot be un-ignored. Real-ROM coverage is cpu_interrupts_v2 (5/5 strict on the default build) + AccuracyCoin 100%."]
fn branch_taken_no_cross_delays_irq_one_instruction() {
    // bus.cycles ticks: CLI consumes 2 (cycles=2). BEQ cycle 1 = fetch_pc =
    // tick 3. BEQ cycle 2 = taken-cycle = tick 4. We want IRQ to *first* go
    // high during BEQ cycle 2.
    let mut bus = IrqGateBus::new(4);
    // CLI (1 byte, 2 cycles) ; BEQ +0 (2 bytes, 3 cycles taken) ;
    // LDA #$AA (2 bytes, 2 cycles) ; LDA #$BB (2 bytes, 2 cycles)
    bus.load(0x8000, &[0x58, 0xF0, 0x00, 0xA9, 0xAA, 0xA9, 0xBB]);
    // IRQ vector -> $9000 with `LDA #$CC; RTI`
    bus.load(0xFFFE, &[0x00, 0x90]);
    bus.load(0x9000, &[0xA9, 0xCC, 0x40]);
    let mut cpu = cpu_at(0x8000);
    cpu.s = 0xFD;
    cpu.p.insert(Status::INTERRUPT_DISABLE);

    cpu.step(&mut bus); // CLI. After: I=0, bus.cycles=2.
    cpu.p.insert(Status::ZERO);
    let beq_cyc = cpu.step(&mut bus);
    assert_eq!(beq_cyc, 3, "BEQ taken no cross is 3 cycles");
    // Without the quirk, IRQ first-sampled at BEQ cycle 2 would arm and
    // service before LDA #$AA; A would be $CC here. With the quirk, the
    // LDA #$AA executes first.
    cpu.step(&mut bus); // LDA #$AA — its second-to-last cycle samples IRQ
    assert_eq!(cpu.a, 0xAA, "LDA #$AA must execute before IRQ is serviced");
    // The IRQ should now be armed and service before the next instruction.
    cpu.step(&mut bus); // services IRQ -> jumps to $9000.
    cpu.step(&mut bus); // LDA #$CC inside handler.
    assert_eq!(cpu.a, 0xCC, "IRQ handler runs after LDA #$AA");
}

// Phase A — Session-13 cold-boot SP path. See
// `docs/audit/session-13-cpu-boot-fix-2026-05-21.md`.

#[test]
fn power_on_then_reset_lands_sp_fd_matching_mesen2() {
    // Cold-boot path: `Cpu::power_on()` seeds `S=$00`; the reset sequence
    // unconditionally decrements `S` by 3 (wrapping), landing at `$FD` —
    // matching Mesen2's `Core/NES/NesCpu.cpp::NesCpu::Reset(softReset=false)`
    // which directly assigns `SP = $FD`.
    let mut bus = RamBus::new();
    // Stub reset vector at $FFFC/D so `cpu.reset(bus)` reads valid bytes.
    bus.load(0xFFFC, &[0x00, 0x80]);
    let mut cpu = Cpu::power_on();
    assert_eq!(
        cpu.s, 0x00,
        "power-on initial S must be $00 (Mesen2 parity)"
    );
    cpu.reset(&mut bus);
    assert_eq!(
        cpu.s, 0xFD,
        "after first reset on a cold-boot CPU, S must land at $FD"
    );
    assert_eq!(cpu.pc, 0x8000, "reset vector $FFFC/D = $8000");
    assert!(cpu.p.contains(Status::INTERRUPT_DISABLE), "reset sets I=1");
    assert!(
        cpu.p.contains(Status::UNUSED),
        "UNUSED bit always set on 6502"
    );
}

#[test]
fn power_on_then_two_resets_models_soft_reset_decrement() {
    // Subsequent soft reset on top of cold-boot's S=$FD should decrement
    // another 3, landing at $FA — matching Mesen2's softReset code path
    // (`_state.SP -= 0x03`).
    let mut bus = RamBus::new();
    bus.load(0xFFFC, &[0x00, 0x80]);
    let mut cpu = Cpu::power_on();
    cpu.reset(&mut bus);
    assert_eq!(cpu.s, 0xFD, "cold boot lands at $FD");
    cpu.reset(&mut bus);
    assert_eq!(cpu.s, 0xFA, "soft reset decrements by 3, $FD -> $FA");
}

#[test]
fn cpu_new_then_reset_preserves_fixture_sp_fa() {
    // `Cpu::new()` remains the test-fixture convenience entry-point and
    // continues to model the post-reset state directly: `S=$FD`. Composing
    // `Cpu::new() + reset()` then yields `S=$FA` (the legacy behaviour that
    // `tests/opcodes.rs` and `rustynes-test-harness/src/lib.rs::cpu_for_nestest`
    // rely on for their fixtures).
    let mut bus = RamBus::new();
    bus.load(0xFFFC, &[0x00, 0x80]);
    let mut cpu = Cpu::new();
    assert_eq!(
        cpu.s, 0xFD,
        "Cpu::new() retains the legacy post-reset SP=$FD"
    );
    cpu.reset(&mut bus);
    assert_eq!(
        cpu.s, 0xFA,
        "Cpu::new() + reset() preserves the legacy fixture S=$FA"
    );
}

// ───────────────────────────────────────────────────────────────────────
// T-72-002 (Phase 7): NMI/IRQ vector + B-flag stack-value evidence.
//
// The B flag (bit 4) only exists on stack pushes: PHP and BRK push it SET,
// while the IRQ and NMI interrupt sequences push it CLEAR. Bit 5 (UNUSED)
// is always set on a push. `brk_pushes_pc_plus_2_and_status_with_b` above
// covers the BRK-set case; these cover the IRQ/NMI-clear discriminator and
// the correct vector ($FFFE for IRQ, $FFFA for NMI). The cycle-precise
// NMI-hijacks-BRK window (cpu_interrupts_v2/2) is on the deferred C1 axis;
// this is the architectural B-flag/vector contract, which is C1-independent.
// ───────────────────────────────────────────────────────────────────────

/// Bus that holds the NMI line high continuously.
struct NmiBus {
    ram: Box<[u8; 0x1_0000]>,
}

impl NmiBus {
    fn new() -> Self {
        Self {
            ram: Box::new([0u8; 0x1_0000]),
        }
    }
    fn load(&mut self, addr: u16, bytes: &[u8]) {
        let base = addr as usize;
        self.ram[base..base + bytes.len()].copy_from_slice(bytes);
    }
}

impl Bus for NmiBus {
    fn cpu_read(&mut self, addr: u16) -> u8 {
        self.ram[addr as usize]
    }
    fn cpu_write(&mut self, addr: u16, value: u8) {
        self.ram[addr as usize] = value;
    }
    fn poll_nmi(&mut self) -> bool {
        true
    }
}

#[test]
#[ignore = "permanent-by-design: pins the SUPERSEDED pre-master-clock interrupt-dispatch granularity on the mock bus. The master-clock core (now the default and ONLY scheduler) legitimately shifts when the mock-bus interrupt is recognized, so this unit assertion is kept ignored as a historical pin and cannot be un-ignored. Real-ROM coverage is cpu_interrupts_v2 (5/5 strict on the default build) + AccuracyCoin 100%."]
fn nmi_pushes_status_with_b_clear_and_takes_fffa_vector() {
    let mut bus = NmiBus::new();
    bus.load(0x8000, &[0xEA, 0xEA, 0xEA]); // NOP sled
    bus.load(0xFFFA, &[0x00, 0x90]); // NMI vector -> $9000
    let mut cpu = cpu_at(0x8000);
    cpu.s = 0xFD;
    // First instruction samples the NMI line (pending -> armed after it);
    // the next step services the armed NMI before fetching another opcode.
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(cpu.pc, 0x9000, "NMI vectors through $FFFA");
    let p_pushed = bus.ram[0x0100 | (cpu.s as usize + 1)];
    let st = Status::from_bits_truncate(p_pushed);
    assert!(!st.contains(Status::BREAK), "NMI pushes the B flag CLEAR");
    assert!(
        st.contains(Status::UNUSED),
        "bit 5 (unused) is always set on push"
    );
}

#[test]
#[ignore = "permanent-by-design: pins the SUPERSEDED pre-master-clock interrupt-dispatch granularity on the mock bus. The master-clock core (now the default and ONLY scheduler) legitimately shifts when the mock-bus interrupt is recognized, so this unit assertion is kept ignored as a historical pin and cannot be un-ignored. Real-ROM coverage is cpu_interrupts_v2 (5/5 strict on the default build) + AccuracyCoin 100%."]
fn irq_pushes_status_with_b_clear_and_takes_fffe_vector() {
    let mut bus = IrqGateBus::new(0); // IRQ asserted from the first cycle
    bus.load(0x8000, &[0xEA, 0xEA, 0xEA]);
    bus.load(0xFFFE, &[0x00, 0x90]); // IRQ vector -> $9000
    let mut cpu = cpu_at(0x8000);
    cpu.s = 0xFD;
    cpu.p.remove(Status::INTERRUPT_DISABLE);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(cpu.pc, 0x9000, "IRQ vectors through $FFFE");
    let p_pushed = bus.ram[0x0100 | (cpu.s as usize + 1)];
    let st = Status::from_bits_truncate(p_pushed);
    assert!(!st.contains(Status::BREAK), "IRQ pushes the B flag CLEAR");
    assert!(
        st.contains(Status::UNUSED),
        "bit 5 (unused) is always set on push"
    );
    assert!(
        cpu.p.contains(Status::INTERRUPT_DISABLE),
        "I flag is set entering the IRQ handler"
    );
}
