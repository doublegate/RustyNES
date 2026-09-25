.PHONY: all clean build release

# Libretro buildbot sets 'platform' and 'ARCH' variables
ifeq ($(platform),)
	platform = unix
	ifeq ($(shell uname -s),)
		platform = win
	else ifneq ($(findstring Darwin,$(shell uname -s)),)
		platform = osx
	else ifneq ($(findstring MINGW,$(shell uname -s)),)
		platform = win
	endif
endif

# v2.8.0 (libretro audit L-1.1): the core UNWINDS on panic. The workspace
# `[profile.release]` aborts, which makes every `catch_unwind` in the core dead
# code: a panic anywhere would end the frontend (RetroArch) with no save. Cargo
# cannot set `panic` per package, so the override is the profile environment
# variable, set here, in `.gitlab-ci.yml`'s `.core-defs`, and in the GitHub
# `libretro-cross` gate. The desktop and wasm builds keep aborting.
export CARGO_PROFILE_RELEASE_PANIC ?= unwind

# `win` is what the auto-detection above assigns on MinGW, and what callers
# commonly pass; the target table below is keyed on `windows`. Fold the two
# so `platform=win` builds the Windows target instead of the host's (libretro
# audit §3.1, v2.8.1).
# `override` is required: a command-line `platform=win` outranks any plain
# assignment in this file, so without it the fold is silently ignored.
ifeq ($(platform),win)
	override platform := windows
endif

# `make DEBUG=1` builds and copies the unoptimised core.
ifeq ($(DEBUG),1)
	PROFILE_DIR := debug
	PROFILE_FLAG :=
else
	PROFILE_DIR := release
	PROFILE_FLAG := --release
endif

# Map Libretro platforms to Rust target triples
RUST_TARGET :=
CARGO_CMD := cargo build -p rustynes-libretro

ifeq ($(platform),android)
	ifeq ($(ARCH),aarch64)
		RUST_TARGET := aarch64-linux-android
	else ifeq ($(ARCH),armv7)
		RUST_TARGET := armv7-linux-androideabi
	else ifeq ($(ARCH),x86_64)
		RUST_TARGET := x86_64-linux-android
	else ifeq ($(ARCH),x86)
		RUST_TARGET := i686-linux-android
	endif
else ifeq ($(platform),osx)
	# Only force an explicit cross-target when the buildbot tells us the arch
	# (ARCH set); a bare local `make` on macOS with no ARCH falls through to
	# native compile (empty RUST_TARGET), same as before this branch existed.
	ifeq ($(ARCH),arm64)
		RUST_TARGET := aarch64-apple-darwin
	else ifeq ($(ARCH),aarch64)
		RUST_TARGET := aarch64-apple-darwin
	else ifeq ($(ARCH),x86_64)
		RUST_TARGET := x86_64-apple-darwin
	endif
else ifeq ($(platform),ios)
	RUST_TARGET := aarch64-apple-ios
else ifeq ($(platform),tvos)
	RUST_TARGET := aarch64-apple-tvos
else ifeq ($(platform),libnx)
	RUST_TARGET := aarch64-nintendo-switch-freestanding
else ifeq ($(platform),windows)
	ifeq ($(ARCH),x86_64)
		RUST_TARGET := x86_64-pc-windows-gnu
	else ifeq ($(ARCH),x86)
		RUST_TARGET := i686-pc-windows-gnu
	endif
else
	# Fallback to native (empty RUST_TARGET means native host compile)
endif

ifneq ($(RUST_TARGET),)
	CARGO_CMD += --target $(RUST_TARGET)
endif

# Convert RUST_TARGET to UPPERCASE_UNDERSCORE for Cargo linker env var
ifneq ($(RUST_TARGET),)
	UPPER_TARGET := $(shell echo $(RUST_TARGET) | tr 'a-z-' 'A-Z_')
endif

# Pass CC to Cargo as the linker if CC is defined by the buildbot cross-toolchain
ifneq ($(CC),)
ifneq ($(RUST_TARGET),)
	export CARGO_TARGET_$(UPPER_TARGET)_LINKER=$(CC)
endif
endif

# Cargo's output directory: `CARGO_TARGET_DIR` when the caller sets it (a
# container build, a shared cache), else the workspace `target/`. A relative
# `CARGO_TARGET_DIR` resolves against this directory for Cargo and for the
# copy alike, because both run here.
TARGET_DIR := $(if $(CARGO_TARGET_DIR),$(CARGO_TARGET_DIR),../../target)
ifneq ($(RUST_TARGET),)
	OUT_DIR := $(TARGET_DIR)/$(RUST_TARGET)
else
	OUT_DIR := $(TARGET_DIR)
endif

# Native extension routing. `LIB_PREFIX` is the library FILE prefix (`lib`,
# or nothing for a DLL, which Cargo names without one). It was called `PREFIX`
# until v2.8.1, which a packager's `make PREFIX=/usr/local` silently overrode,
# turning the copy into `target/release//usr/localrustynes.so`.
ifeq ($(platform),osx)
    EXT := dylib
    LIB_PREFIX := lib
else ifeq ($(platform),ios)
    EXT := a
    LIB_PREFIX := lib
else ifeq ($(platform),tvos)
    EXT := a
    LIB_PREFIX := lib
else ifeq ($(platform),libnx)
    EXT := a
    LIB_PREFIX := lib
else ifeq ($(platform),windows)
    EXT := dll
    LIB_PREFIX :=
else
    EXT := so
    LIB_PREFIX := lib
endif

# `all` follows `DEBUG`; `build` and `release` stay available by name.
all:
	$(CARGO_CMD) $(PROFILE_FLAG)
	cp $(OUT_DIR)/$(PROFILE_DIR)/$(LIB_PREFIX)rustynes.$(EXT) rustynes_libretro.$(EXT)
	@echo "Copied $(PROFILE_DIR) core to rustynes_libretro.$(EXT)"

build:
	$(CARGO_CMD)
	cp $(OUT_DIR)/debug/$(LIB_PREFIX)rustynes.$(EXT) rustynes_libretro.$(EXT)
	@echo "Copied debug core to rustynes_libretro.$(EXT)"

release:
	$(CARGO_CMD) --release
	cp $(OUT_DIR)/release/$(LIB_PREFIX)rustynes.$(EXT) rustynes_libretro.$(EXT)
	@echo "Copied release core to rustynes_libretro.$(EXT)"

clean:
	cargo clean -p rustynes-libretro
	rm -f rustynes_libretro.so rustynes_libretro.dylib rustynes_libretro.dll rustynes_libretro.a
