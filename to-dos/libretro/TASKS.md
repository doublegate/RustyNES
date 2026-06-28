# RustyNES Libretro Core - Execution Tasks

- `[x]` **Phase 1: Build System & Dependency Wiring**
  - `[x]` Initialize new `rustynes-libretro` crate as a library
  - `[x]` Configure `Cargo.toml` to compile as `cdylib`
- [x] **Libretro-Super Integration**
  - [x] Copy `rustynes_libretro.info` to `dist/info/`
  - [x] Add `rustynes` to core arrays (e.g. `recipes/linux/cores-linux-x64-generic` or `libretro-config.sh`)
  - [x] Create `Makefile` wrapper in RustyNES repo for buildbot execution
- [x] **Libretro-Docs Integration**
  - [x] Create `docs/library/rustynes.md`
  - [x] Add RustyNES to `mkdocs.yml` navigation
- [x] **Submit Changes**
  - [x] Generate a script `submit_libretro_prs.sh` to handle forking and pushing from the user's authenticated environment
- `[ ]` **Final Verification & Walkthrough**
