.PHONY: build release clean

OS := $(shell uname -s)
ifeq ($(OS),Darwin)
    EXT := dylib
    PREFIX := lib
else ifeq ($(OS),Linux)
    EXT := so
    PREFIX := lib
else
    EXT := dll
    PREFIX :=
endif

# Target directory is relative to the workspace root
TARGET_DIR := ../../target

build:
	cargo build -p rustynes-libretro
	cp $(TARGET_DIR)/debug/$(PREFIX)rustynes_libretro.$(EXT) rustynes_libretro.$(EXT)
	@echo "Copied debug core to rustynes_libretro.$(EXT)"

release:
	cargo build --release -p rustynes-libretro
	cp $(TARGET_DIR)/release/$(PREFIX)rustynes_libretro.$(EXT) rustynes_libretro.$(EXT)
	@echo "Copied release core to rustynes_libretro.$(EXT)"

clean:
	cargo clean -p rustynes-libretro
	rm -f rustynes_libretro.so rustynes_libretro.dylib rustynes_libretro.dll
