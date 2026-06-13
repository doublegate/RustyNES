//! Property test for ADC: random A, operand, carry-in. Compare flag updates
//! against a hand-rolled 6502 reference.

#![allow(clippy::large_stack_arrays, clippy::cast_possible_truncation)]

use proptest::prelude::*;
use rustynes_cpu::{Bus, Cpu, Status};

struct StubBus(Box<[u8; 0x1_0000]>);

impl Bus for StubBus {
    fn cpu_read(&mut self, addr: u16) -> u8 {
        self.0[addr as usize]
    }
    fn cpu_write(&mut self, addr: u16, value: u8) {
        self.0[addr as usize] = value;
    }
}

fn ref_adc(a: u8, m: u8, c_in: bool) -> (u8, bool, bool, bool, bool) {
    // Returns (result, N, Z, C, V).
    let sum = u16::from(a) + u16::from(m) + u16::from(c_in);
    let r = sum as u8;
    let carry = sum > 0xFF;
    let overflow = ((a ^ r) & (m ^ r) & 0x80) != 0;
    let zero = r == 0;
    let neg = r & 0x80 != 0;
    (r, neg, zero, carry, overflow)
}

fn run_adc(a: u8, m: u8, c_in: bool) -> (u8, bool, bool, bool, bool) {
    let mut bus = StubBus(Box::new([0u8; 0x1_0000]));
    // SEC/CLC; LDA #a; ADC #m
    let prog: [u8; 5] = [if c_in { 0x38 } else { 0x18 }, 0xA9, a, 0x69, m];
    bus.0[0x8000..0x8000 + prog.len()].copy_from_slice(&prog);
    let mut cpu = Cpu::new();
    cpu.set_pc(0x8000);
    for _ in 0..3 {
        cpu.step(&mut bus);
    }
    let neg = cpu.p.contains(Status::NEGATIVE);
    let zero = cpu.p.contains(Status::ZERO);
    let carry = cpu.p.contains(Status::CARRY);
    let overflow = cpu.p.contains(Status::OVERFLOW);
    (cpu.a, neg, zero, carry, overflow)
}

proptest! {
    #[test]
    fn adc_matches_reference(a in any::<u8>(), m in any::<u8>(), c_in in any::<bool>()) {
        let (ra, rn, rz, rc, rv) = ref_adc(a, m, c_in);
        let (ga, gn, gz, gc, gv) = run_adc(a, m, c_in);
        prop_assert_eq!(ga, ra);
        prop_assert_eq!(gn, rn);
        prop_assert_eq!(gz, rz);
        prop_assert_eq!(gc, rc);
        prop_assert_eq!(gv, rv);
    }
}
