# Milestone 14: Plugin Architecture & Mobile Support

**Phase:** 3 (Expansion)
**Duration:** Months 18-20 (3 months)
**Status:** Planned
**Target:** August 2027
**Prerequisites:** M6 MVP Complete, Core emulation stable

---

## Overview

Milestone 14 delivers a **Plugin Architecture** enabling community extensions and custom functionality, plus **optional Mobile Support** for Android/iOS native applications.

**Plugin Architecture** provides a safe, sandboxed environment for user-created plugins (shaders, input handlers, achievements, netplay protocols) via WASM-based plugins or dynamic library loading.

**Mobile Support** (optional) builds native mobile applications with touch controls, performance tuning for mobile CPUs, and battery life optimization.

---

## Part 1: Plugin Architecture

### Core Plugin Features

- [ ] **Plugin API (Rust FFI)**
  - Stable ABI (C-compatible)
  - Version negotiation
  - Error handling (Result types)
  - Documentation (API reference)

- [ ] **WASM Plugin Support**
  - WASI runtime (wasmtime)
  - Sandboxed execution
  - Memory isolation
  - Capability-based security

- [ ] **Plugin Types**
  - **Shader Plugins:** Custom CRT filters (WGSL)
  - **Input Handlers:** Custom controller mappings
  - **Achievement Plugins:** Custom achievement sets
  - **Netplay Protocols:** Custom rollback implementations
  - **Audio Filters:** Custom equalizers, reverb
  - **Save State Converters:** Import from other emulators

- [ ] **Plugin Discovery**
  - Plugin directory scanning (`~/.config/rustynes/plugins/`)
  - Metadata parsing (plugin.toml)
  - Dependency resolution
  - Version compatibility checks

- [ ] **Plugin Lifecycle**
  - Load (initialization)
  - Activate (enable functionality)
  - Deactivate (disable functionality)
  - Unload (cleanup)
  - Hot reload (development mode)

- [ ] **Plugin Security**
  - Sandboxing (WASM or OS-level)
  - Capability permissions (file I/O, network, GPU)
  - Code signing (trusted plugins)
  - Malware scanning (SHA256 checksums)

- [ ] **Plugin UI Integration**
  - Settings panel (per-plugin configuration)
  - Enable/disable toggles
  - Plugin marketplace (future)

---

## Part 2: Mobile Support (Optional)

### Core Mobile Features

- [ ] **Android Native App**
  - NDK integration (Rust → JNI)
  - OpenGL ES 3.0 rendering
  - Touch controls (virtual D-pad)
  - Performance tuning (ARM NEON)

- [ ] **iOS Native App**
  - Swift/Objective-C bindings
  - Metal rendering
  - Touch controls (virtual D-pad)
  - Performance tuning (Apple Silicon)

- [ ] **Touch Controls**
  - Virtual D-pad (transparency, size, position)
  - Multi-touch gestures (pinch-to-zoom, swipe-to-save)
  - Haptic feedback
  - Customizable layouts

- [ ] **Performance Optimization**
  - Frame skip (30 FPS fallback)
  - Power-efficient rendering
  - Battery drain monitoring
  - Thermal throttling detection

- [ ] **Mobile UX**
  - Portrait/landscape orientation
  - ROM library (file picker)
  - Save state management
  - Settings UI (mobile-optimized)

---

## Plugin Architecture

### Plugin API Design

**File:** `crates/rustynes-plugin-api/src/lib.rs`

```rust
/// Plugin API version (semver)
pub const PLUGIN_API_VERSION: &str = "1.0.0";

/// Plugin metadata (from plugin.toml)
#[repr(C)]
pub struct PluginMetadata {
    pub name: *const c_char,
    pub version: *const c_char,
    pub author: *const c_char,
    pub description: *const c_char,
    pub plugin_type: PluginType,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub enum PluginType {
    Shader = 0,
    InputHandler = 1,
    Achievement = 2,
    NetplayProtocol = 3,
    AudioFilter = 4,
    SaveStateConverter = 5,
}

/// Plugin initialization (called once on load)
#[no_mangle]
pub extern "C" fn plugin_init() -> *mut PluginMetadata {
    // Return metadata
}

/// Plugin activation (called when enabled)
#[no_mangle]
pub extern "C" fn plugin_activate() -> i32 {
    // 0 = success, non-zero = error
    0
}

/// Plugin deactivation (called when disabled)
#[no_mangle]
pub extern "C" fn plugin_deactivate() -> i32 {
    0
}

/// Plugin cleanup (called on unload)
#[no_mangle]
pub extern "C" fn plugin_cleanup() {
    // Free resources
}

/// Shader plugin: Get WGSL source code
#[no_mangle]
pub extern "C" fn shader_get_source() -> *const c_char {
    // Return WGSL shader code
}

/// Input handler plugin: Map controller input
#[no_mangle]
pub extern "C" fn input_map_button(button: u8) -> u8 {
    // Map physical button to NES button
    button
}
```

### WASM Plugin Example

**File:** `examples/plugins/custom-shader/src/lib.rs`

```rust
// WASM plugin (compiles to .wasm)
use rustynes_plugin_api::*;

static METADATA: PluginMetadata = PluginMetadata {
    name: b"Custom CRT Shader\0".as_ptr() as *const i8,
    version: b"1.0.0\0".as_ptr() as *const i8,
    author: b"John Doe\0".as_ptr() as *const i8,
    description: b"Custom phosphor glow shader\0".as_ptr() as *const i8,
    plugin_type: PluginType::Shader,
};

#[no_mangle]
pub extern "C" fn plugin_init() -> *mut PluginMetadata {
    &METADATA as *const _ as *mut _
}

#[no_mangle]
pub extern "C" fn plugin_activate() -> i32 {
    // Initialize shader resources
    0
}

#[no_mangle]
pub extern "C" fn shader_get_source() -> *const c_char {
    // Return custom WGSL shader
    include_str!("shader.wgsl").as_ptr() as *const i8
}
```

### Plugin Configuration (TOML)

**File:** `~/.config/rustynes/plugins/custom-shader/plugin.toml`

```toml
[plugin]
name = "Custom CRT Shader"
version = "1.0.0"
author = "John Doe"
description = "Custom phosphor glow shader"
type = "shader"

[api]
min_version = "1.0.0"
max_version = "1.9.9"

[permissions]
gpu = true
filesystem = false
network = false

[dependencies]
# Other plugins this plugin depends on
```

### Plugin Manager

**File:** `crates/rustynes-desktop/src/plugins/manager.rs`

```rust
use libloading::{Library, Symbol};
use wasmtime::{Engine, Module, Store};
use std::path::{Path, PathBuf};

pub struct PluginManager {
    /// Native plugins (dynamic libraries)
    native_plugins: Vec<NativePlugin>,

    /// WASM plugins (sandboxed)
    wasm_plugins: Vec<WasmPlugin>,

    /// Plugin directory
    plugin_dir: PathBuf,
}

struct NativePlugin {
    library: Library,
    metadata: PluginMetadata,
    active: bool,
}

struct WasmPlugin {
    module: Module,
    metadata: PluginMetadata,
    active: bool,
}

impl PluginManager {
    pub fn new(plugin_dir: PathBuf) -> Self {
        Self {
            native_plugins: Vec::new(),
            wasm_plugins: Vec::new(),
            plugin_dir,
        }
    }

    pub fn scan_plugins(&mut self) -> Result<(), PluginError> {
        // Scan plugin directory for .so/.dylib/.dll (native) or .wasm (WASM)
        for entry in std::fs::read_dir(&self.plugin_dir)? {
            let path = entry?.path();

            if path.extension().map_or(false, |ext| ext == "wasm") {
                self.load_wasm_plugin(&path)?;
            } else if path.extension().map_or(false, |ext| {
                ext == "so" || ext == "dylib" || ext == "dll"
            }) {
                self.load_native_plugin(&path)?;
            }
        }

        Ok(())
    }

    fn load_native_plugin(&mut self, path: &Path) -> Result<(), PluginError> {
        unsafe {
            let library = Library::new(path)?;

            // Get plugin_init function
            let init: Symbol<extern "C" fn() -> *mut PluginMetadata> =
                library.get(b"plugin_init")?;

            let metadata_ptr = init();
            let metadata = std::ptr::read(metadata_ptr);

            self.native_plugins.push(NativePlugin {
                library,
                metadata,
                active: false,
            });
        }

        Ok(())
    }

    fn load_wasm_plugin(&mut self, path: &Path) -> Result<(), PluginError> {
        let engine = Engine::default();
        let module = Module::from_file(&engine, path)?;

        // Extract metadata from WASM exports
        // ... wasmtime setup ...

        Ok(())
    }

    pub fn activate_plugin(&mut self, name: &str) -> Result<(), PluginError> {
        // Find plugin by name
        if let Some(plugin) = self.native_plugins.iter_mut().find(|p| {
            unsafe { std::ffi::CStr::from_ptr(p.metadata.name).to_str().unwrap() == name }
        }) {
            unsafe {
                let activate: Symbol<extern "C" fn() -> i32> =
                    plugin.library.get(b"plugin_activate")?;

                if activate() == 0 {
                    plugin.active = true;
                    Ok(())
                } else {
                    Err(PluginError::ActivationFailed)
                }
            }
        } else {
            Err(PluginError::NotFound)
        }
    }

    pub fn list_plugins(&self) -> Vec<PluginInfo> {
        let mut plugins = Vec::new();

        for plugin in &self.native_plugins {
            plugins.push(PluginInfo {
                name: unsafe {
                    std::ffi::CStr::from_ptr(plugin.metadata.name)
                        .to_string_lossy()
                        .into_owned()
                },
                active: plugin.active,
                plugin_type: plugin.metadata.plugin_type,
            });
        }

        plugins
    }
}

pub struct PluginInfo {
    pub name: String,
    pub active: bool,
    pub plugin_type: PluginType,
}

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("Plugin not found")]
    NotFound,
    #[error("Plugin activation failed")]
    ActivationFailed,
    #[error("Library loading error: {0}")]
    LibraryError(#[from] libloading::Error),
    #[error("WASM error: {0}")]
    WasmError(#[from] wasmtime::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
```

---

## Mobile Architecture (Optional)

### Android Integration

**File:** `crates/rustynes-android/src/lib.rs`

```rust
use jni::JNIEnv;
use jni::objects::{JClass, JString};
use jni::sys::{jlong, jint};
use rustynes_core::Console;

/// Initialize emulator (called from Java/Kotlin)
#[no_mangle]
pub extern "C" fn Java_com_rustynes_NativeEmulator_init(
    env: JNIEnv,
    _class: JClass,
    rom_path: JString,
) -> jlong {
    let rom_path: String = env.get_string(rom_path)
        .expect("Invalid ROM path")
        .into();

    // Load ROM and create console
    let console = Console::new(&rom_path).expect("Failed to load ROM");

    Box::into_raw(Box::new(console)) as jlong
}

/// Run one frame (called from Java/Kotlin)
#[no_mangle]
pub extern "C" fn Java_com_rustynes_NativeEmulator_runFrame(
    _env: JNIEnv,
    _class: JClass,
    console_ptr: jlong,
    framebuffer: *mut u8,
) {
    let console = unsafe { &mut *(console_ptr as *mut Console) };

    // Run one frame
    console.clock_frame();

    // Copy framebuffer to Java array
    let fb = console.framebuffer();
    unsafe {
        std::ptr::copy_nonoverlapping(fb.as_ptr(), framebuffer, fb.len());
    }
}

/// Set controller input (called from Java/Kotlin)
#[no_mangle]
pub extern "C" fn Java_com_rustynes_NativeEmulator_setInput(
    _env: JNIEnv,
    _class: JClass,
    console_ptr: jlong,
    buttons: jint,
) {
    let console = unsafe { &mut *(console_ptr as *mut Console) };
    console.set_input(buttons as u8);
}
```

### iOS Integration

**File:** `crates/rustynes-ios/src/lib.rs`

```rust
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use rustynes_core::Console;

/// Initialize emulator (called from Swift/Objective-C)
#[no_mangle]
pub extern "C" fn rustynes_init(rom_path: *const c_char) -> *mut Console {
    let rom_path = unsafe { CStr::from_ptr(rom_path).to_str().unwrap() };

    let console = Console::new(rom_path).expect("Failed to load ROM");

    Box::into_raw(Box::new(console))
}

/// Run one frame (called from Swift/Objective-C)
#[no_mangle]
pub extern "C" fn rustynes_run_frame(
    console_ptr: *mut Console,
    framebuffer: *mut u8,
) {
    let console = unsafe { &mut *console_ptr };

    console.clock_frame();

    let fb = console.framebuffer();
    unsafe {
        std::ptr::copy_nonoverlapping(fb.as_ptr(), framebuffer, fb.len());
    }
}
```

---

## Implementation Plan

### Sprint 1: Plugin API Foundation

**Duration:** 3 weeks

- [ ] Define stable ABI (C-compatible)
- [ ] Plugin metadata (plugin.toml)
- [ ] Plugin lifecycle (init, activate, deactivate, cleanup)
- [ ] Error handling

### Sprint 2: WASM Plugin Support

**Duration:** 3 weeks

- [ ] wasmtime integration
- [ ] WASM plugin loading
- [ ] Sandboxed execution
- [ ] Capability-based security

### Sprint 3: Plugin Types

**Duration:** 3 weeks

- [ ] Shader plugins (WGSL)
- [ ] Input handler plugins
- [ ] Achievement plugins
- [ ] Audio filter plugins

### Sprint 4: Plugin Manager UI

**Duration:** 2 weeks

- [ ] Plugin discovery (directory scanning)
- [ ] Settings panel (enable/disable)
- [ ] Plugin configuration UI
- [ ] Hot reload (development mode)

### Sprint 5-6: Mobile Support (Optional)

**Duration:** 4 weeks (if approved)

- [ ] Android NDK integration
- [ ] iOS Swift/Objective-C bindings
- [ ] Touch controls
- [ ] Performance tuning

---

## Acceptance Criteria

### Plugin Architecture

- [ ] WASM plugins load correctly
- [ ] Native plugins load correctly (dynamic libraries)
- [ ] Shader plugins render correctly
- [ ] Input handler plugins work
- [ ] Plugin sandboxing prevents malicious code
- [ ] Hot reload works (development mode)
- [ ] Plugin UI integrates with settings panel

### Mobile Support (Optional)

- [ ] Android app runs at 60 FPS (flagship devices)
- [ ] iOS app runs at 60 FPS (flagship devices)
- [ ] Touch controls work correctly
- [ ] Battery drain acceptable (<10% per hour)
- [ ] ROM loading works (file picker)

---

## Dependencies

### Prerequisites

- **M6 MVP Complete:** Core emulation stable
- **Rust Stable:** ABI stability for FFI

### Crate Dependencies

```toml
# crates/rustynes-desktop/Cargo.toml

[dependencies.libloading]
version = "0.8"  # Dynamic library loading

[dependencies.wasmtime]
version = "17.0"  # WASM runtime

[dependencies.serde]
version = "1.0"
features = ["derive"]  # Plugin metadata (TOML)

# Android (optional)
[target.'cfg(target_os = "android")'.dependencies]
jni = "0.21"
ndk = "0.8"
ndk-glue = "0.8"

# iOS (optional)
[target.'cfg(target_os = "ios")'.dependencies]
objc = "0.2"
```

---

## Related Documentation

- [M6-S1-iced-application.md](../../phase-1-mvp/milestone-6-gui/M6-S1-iced-application.md) - Iced GUI integration
- [M11 CRT Shaders](../milestone-11-webassembly/README.md) - Shader plugin examples
- [wasmtime Documentation](https://docs.wasmtime.dev/) - WASM runtime reference

---

## Success Criteria

1. Plugin API stable (1.0.0 release)
2. WASM plugins load and execute safely
3. Native plugins load correctly (dynamic libraries)
4. Shader plugins render correctly
5. Input handler plugins work
6. Plugin sandboxing prevents malicious code
7. Plugin UI integrates seamlessly
8. (Optional) Android/iOS apps run at 60 FPS
9. (Optional) Touch controls work correctly
10. M14 milestone marked as ✅ COMPLETE

---

**Milestone Status:** ⏳ PLANNED
**Blocked By:** M6 MVP Complete
**Next Milestone:** M15 (Advanced Shader Pipeline & Video Filters)

---

## Design Notes

### Plugin Architecture Philosophy

**Why Plugins?**

- Community extensions (custom shaders, achievements)
- User customization (input handlers, audio filters)
- Third-party integrations (Discord, Twitch)
- Experimental features (without core bloat)

**Security First:**

- WASM sandboxing (memory isolation)
- Capability-based permissions (file I/O, network, GPU)
- Code signing (trusted plugins)
- Malware scanning (SHA256 checksums)

**Developer Experience:**

- Simple API (stable ABI)
- Hot reload (development mode)
- Plugin templates (examples/)
- Documentation (API reference)

### Mobile Support Rationale

**Why Mobile?**

- Large user base (iOS/Android)
- Touch controls (natural for NES)
- Portable emulation

**Why Optional?**

- WebAssembly may suffice (browser-based)
- Maintenance burden (two platforms)
- App store policies (emulator restrictions)

**Decision Point:**

- Evaluate after M11 (WebAssembly) completion
- Measure browser performance on mobile
- Assess demand from community

---

## Future Enhancements (Phase 4)

Advanced features deferred to Phase 4:

1. **Plugin Marketplace:**
   - Centralized plugin repository
   - Community ratings/reviews
   - Auto-updates

2. **Visual Plugin Editor:**
   - Node-based shader editor
   - Real-time preview
   - Export to WASM

3. **Cloud Sync:**
   - Plugin sync across devices
   - Cloud-based plugin storage

---

**Migration Note:** Plugin architecture features added from M6 reorganization. Mobile support retains original optional status.
