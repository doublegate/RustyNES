import sys

filepath = sys.argv[1]

with open(filepath, 'r') as f:
    content = f.read()

old = """            #[cfg(not(feature = "mc-r1-dmc-phase-coherence"))]
            {
                self.dmc_dma_delay = if self.apu_phase { 4 } else { 3 };
            }"""

new = """            #[cfg(not(feature = "mc-r1-dmc-phase-coherence"))]
            {
                self.dmc_dma_delay = if self.apu_phase { 3 } else { 2 };
            }"""

content = content.replace(old, new)

with open(filepath, 'w') as f:
    f.write(content)
