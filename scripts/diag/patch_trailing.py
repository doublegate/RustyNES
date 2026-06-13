import re
import sys

filepath = sys.argv[1]

with open(filepath, 'r') as f:
    content = f.read()

old = """            if !self.in_dmc_dma {
                // First cycle of this DMA span: latch halt + the open-bus replay.
                self.in_dmc_dma = true;
                self.dmc_halt = true;
                self.capture_deferred_dma_replay();
            }"""

new = """            if !self.in_dmc_dma {
                // First cycle of this DMA span: latch halt + the open-bus replay.
                self.in_dmc_dma = true;
                self.dmc_halt = true;
                self.capture_deferred_dma_replay();
                if !self.apu.dmc_dma_short() {
                    self.dmc_trailing = true;
                }
            }"""

content = content.replace(old, new)

with open(filepath, 'w') as f:
    f.write(content)
