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

# Target directory is relative to the workspace root
TARGET_DIR := ../../target
ifneq ($(RUST_TARGET),)
	OUT_DIR := $(TARGET_DIR)/$(RUST_TARGET)
else
	OUT_DIR := $(TARGET_DIR)
endif

# Native extension routing
ifeq ($(platform),osx)
    EXT := dylib
    PREFIX := lib
else ifeq ($(platform),ios)
    EXT := a
    PREFIX := lib
else ifeq ($(platform),tvos)
    EXT := a
    PREFIX := lib
else ifeq ($(platform),libnx)
    EXT := a
    PREFIX := lib
else ifeq ($(platform),win)
    EXT := dll
    PREFIX :=
else ifeq ($(platform),windows)
    EXT := dll
    PREFIX :=
else
    EXT := so
    PREFIX := lib
endif

all: release

build:
	$(CARGO_CMD)
	cp $(OUT_DIR)/debug/$(PREFIX)rustynes_libretro.$(EXT) rustynes_libretro.$(EXT)
	@echo "Copied debug core to rustynes_libretro.$(EXT)"

release:
	$(CARGO_CMD) --release
	cp $(OUT_DIR)/release/$(PREFIX)rustynes_libretro.$(EXT) rustynes_libretro.$(EXT)
	@echo "Copied release core to rustynes_libretro.$(EXT)"

clean:
	cargo clean -p rustynes-libretro
	rm -f rustynes_libretro.so rustynes_libretro.dylib rustynes_libretro.dll rustynes_libretro.a
