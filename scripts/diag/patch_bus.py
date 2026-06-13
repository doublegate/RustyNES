import re
import sys

filepath = 'crates/rustynes-core/src/bus.rs'

with open(filepath, 'r') as f:
    content = f.read()

old = """            let get_cycle = !self.apu.put_cycle();
            if !need_halt && !need_dummy && get_cycle {
                // The GET: DMC ready (halt + dummy done) and a get cycle.
                let addr = self.apu.dmc_dma_addr();
                let byte = self.dmc_dma_read(addr, halted_addr);
                #[cfg(feature = "irq-timing-trace")]
                self.set_trace_dma_access(BusAccess::DmaRead, addr, byte);
                self.apu.complete_dmc_dma(byte);
                // Trailing-stall: hold the CPU halted one more cycle (dmc_trailing
                // keeps dmc_dma_pending() true) so the duration is 4 not 3.
                #[cfg(feature = "mc-r1-dmc-trailing-stall")]
                {
                    self.dmc_trailing = true;
                }
                #[cfg(not(feature = "mc-r1-dmc-trailing-stall"))]
                {
                    self.in_dmc_dma = false;
                }
            } else {"""

new = """            let get_cycle = !self.apu.put_cycle();
            if !need_halt && !need_dummy && get_cycle {
                // The GET: DMC ready (halt + dummy done) and a get cycle.
                let addr = self.apu.dmc_dma_addr();
                let byte = self.dmc_dma_read(addr, halted_addr);
                #[cfg(feature = "irq-timing-trace")]
                self.set_trace_dma_access(BusAccess::DmaRead, addr, byte);
                self.apu.complete_dmc_dma(byte);
                // accuracycoin-100: For reloads, force a 4-cycle span by adding a trailing stall
                // if it was a short load (i.e. started on GET and took 3 cycles).
                if !self.apu.dmc_dma_short() {
                    self.dmc_trailing = true;
                } else {
                    self.in_dmc_dma = false;
                }
            } else {"""

content = content.replace(old, new)

with open(filepath, 'w') as f:
    f.write(content)
