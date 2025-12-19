---
name: Mapper Request
about: Request implementation of a specific NES mapper
title: '[MAPPER] Mapper XX - Name'
labels: mapper, enhancement
assignees: ''
---

## Mapper Information

**Mapper Number:**

- iNES Mapper: [e.g., 5]
- NES 2.0 Submapper: [if applicable]

**Mapper Name:**
[e.g., MMC5 (ExROM)]

**Alternative Names:**
[e.g., Nintendo MMC5, ExROM]

## Games Requiring This Mapper

List games that use this mapper (prioritize by popularity/importance):

1. **Game Name** - [Region] - [Importance: High/Medium/Low]
2. **Game Name** - [Region] - [Importance: High/Medium/Low]
3. ...

**Example:**

1. **Castlevania III: Dracula's Curse** - USA - High
2. **Just Breed** - Japan - Medium

## Technical References

**NESdev Wiki:**

- Mapper Page: [URL to NESdev wiki page]
- Related Hardware: [URL if applicable]

**Other Documentation:**

- [Link to technical documents]
- [Link to reference implementations]
- [Link to test ROMs if available]

## Mapper Features

**PRG ROM Banking:**

- [ ] Fixed banks
- [ ] Switchable banks
- Number of banks: [e.g., 8KB, 16KB, 32KB]
- Banking configuration: [Description]

**CHR ROM/RAM Banking:**

- [ ] CHR ROM
- [ ] CHR RAM
- Number of banks: [e.g., 1KB, 2KB, 8KB]
- Banking configuration: [Description]

**Special Features:**

- [ ] IRQ counter (scanline/cycle)
- [ ] Expansion audio
- [ ] Battery-backed save RAM
- [ ] Extra nametable RAM
- [ ] Custom mirroring modes
- [ ] Other: [Description]

**Expansion Audio (if applicable):**

- Channels: [e.g., 2 pulse + 1 sawtooth for VRC6]
- Registers: [Brief description]
- Reference: [Link to audio specification]

## Priority Level

**How important is this mapper?**

- [ ] **Critical** - Blocks multiple popular games
- [ ] **High** - Required for well-known games
- [ ] **Medium** - Needed for niche but notable games
- [ ] **Low** - Obscure or homebrew only

**Games affected:** [X games total, Y popular titles]

## Implementation Complexity

**Estimated Difficulty:**

- [ ] Simple (basic banking, no special features)
- [ ] Moderate (IRQ timing, complex banking)
- [ ] Complex (expansion audio, advanced features)
- [ ] Very Complex (MMC5-level complexity)

**Complexity Factors:**

- [e.g., Requires accurate PPU scanline tracking for IRQ]
- [e.g., Expansion audio needs additional synthesis]

## Test ROMs

**Available Test ROMs:**

- [ ] Official test ROM exists: [Link/Name]
- [ ] Community test ROMs: [Link/Name]
- [ ] No known test ROMs

**Test Games:**
List specific games that can be used for testing:

1. **Game Name** - Tests [specific feature]
2. **Game Name** - Tests [specific feature]

## Reference Implementations

**Existing Emulators with This Mapper:**

- [ ] Mesen/Mesen2: [Link to implementation]
- [ ] FCEUX: [Link to implementation]
- [ ] puNES: [Link to implementation]
- [ ] Nestopia: [Link to implementation]
- [ ] Other: [Name and link]

## Roadmap Alignment

**Which development phase should this be implemented in?**

See [ROADMAP.md](https://github.com/doublegate/RustyNES/blob/main/ROADMAP.md) for phase descriptions.

- [ ] Phase 1 (MVP) - Top 5 mappers only
- [ ] Phase 2 (Advanced Features) - Top 15 mappers
- [ ] Phase 3 (Expansion) - 50+ mappers
- [ ] Phase 4 (Polish) - 300+ mappers
- [ ] Post-v1.0

**Justification:**
[Why this mapper should be prioritized for the selected phase]

## Additional Context

**Unique Characteristics:**
[Anything unusual about this mapper that implementers should know]

**Known Issues in Other Emulators:**
[Document any known bugs or edge cases in existing implementations]

**Homebrew Usage:**

- [ ] Used by modern homebrew games
- [ ] Examples: [List homebrew games]

## Checklist

- [ ] I searched existing issues to avoid duplicates
- [ ] I provided the mapper number and name
- [ ] I listed at least 3 games that use this mapper
- [ ] I included links to NESdev wiki or technical documentation
- [ ] I indicated the priority/importance level
- [ ] I checked the roadmap for appropriate phase
