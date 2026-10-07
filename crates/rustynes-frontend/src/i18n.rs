//! v1.7.0 "Forge" Workstream H5 — frontend internationalization (i18n).
//!
//! `RustyNES`'s one systemic gap was that every user-facing string was a hard
//! literal. This module adds a *lightweight, compile-time string-catalog*
//! layer so the UI can be localized without touching the deterministic core.
//!
//! ## Design (see ADR 0023)
//!
//! - **Compile-time catalogs, no runtime I/O.** Every translation is a
//!   `&'static str` baked into the binary via plain `match` arms. There is no
//!   TOML/RON/Fluent parsing at startup, no embedded data file to load, and —
//!   critically for the wasm build — no `unic-langid` / `fluent` / ICU
//!   machinery to bloat the bundle past the 5 MiB Pages budget
//!   (`scripts/wasm_size_budget.sh`). A `&'static str` table compiles to read-
//!   only data the linker can dead-strip if unused; the whole layer costs a few
//!   KiB of string bytes. This is wasm-safe (`no_std`-shaped, no `std::fs`).
//! - **English is the default and the fallback.** [`Locale::English`] is the
//!   [`Default`], and [`tr`] falls back to the English arm for any key a
//!   non-English catalog has not translated yet. The English values are the
//!   *verbatim* strings the UI rendered before this module existed, so with the
//!   default locale every label is byte-identical to v1.6.0.
//! - **Scope.** v1.7.0 wired the high-visibility surfaces (menu bar, Settings
//!   tabs/headers, status bar, common dialog buttons). v2.9.7 "Tandem" extends
//!   that to every user-facing panel: the whole shell, the Settings sections,
//!   input bindings, netplay (native and the browser lobby), cheats, ROM Info
//!   and the header editor. Debugger-internal panels stay English by design.
//!   The conversion pattern and the list of deliberately untranslated strings
//!   are in the i18n section of `docs/frontend.md`.
//! - **One table.** Every key, its English source and its Spanish translation
//!   sit on one row of the `catalog!` table below, which generates [`Key`],
//!   [`Key::ALL`] and both catalogs.
//! - **Parameters.** A `format!` becomes a keyed template with positional
//!   `{0}`, `{1}`, ... placeholders, filled by [`tr_fmt`] / `tf!`. Nothing
//!   richer (plurals, named arguments) exists, on purpose.
//! - **The Spanish is machine-drafted.** Everything added at v2.9.7 awaits a
//!   native speaker's review; see the note above the table.
//!
//! ## Runtime selection
//!
//! The active locale is a process-global (`CURRENT_LOCALE`) seeded from the
//! `[ui] locale` config field at startup and updated by the Settings language
//! picker. egui re-renders the shell every frame, so a change takes effect on
//! the next frame with no explicit invalidation. Reads are a single relaxed
//! atomic load — cheap enough to call once per rendered string.

use core::fmt;
use core::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

/// The set of UI locales `RustyNES` ships catalogs for.
///
/// English is the default + fallback. Spanish (`es`) is included as a real
/// second locale to prove the mechanism end-to-end; further locales are added
/// by appending a variant here and a `match` arm to each catalog.
///
/// Serialized lowercase (`"english"` / `"spanish"`), so a hand-edited config or
/// an older config that omits the field both resolve correctly (the missing
/// field falls back to [`Locale::English`] via `#[serde(default)]` on the
/// `[ui] locale` config key).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Locale {
    /// English (United States). The default and the fallback for any key a
    /// non-English catalog has not translated.
    #[default]
    English,
    /// Spanish (Español).
    Spanish,
}

impl Locale {
    /// Human-readable, *native-language* label for the language picker (each
    /// language names itself, the convention for language selectors).
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Spanish => "Español",
        }
    }

    /// All locales in display order — single source of truth for the Settings
    /// language combo box so it never drifts from the enum.
    #[must_use]
    pub const fn all() -> [Self; 2] {
        [Self::English, Self::Spanish]
    }

    /// Stable numeric tag for the atomic global (round-trips through
    /// [`Locale::from_u8`]).
    #[must_use]
    const fn as_u8(self) -> u8 {
        match self {
            Self::English => 0,
            Self::Spanish => 1,
        }
    }

    /// Inverse of [`Locale::as_u8`]; any unknown byte falls back to English so a
    /// corrupted global can never panic.
    #[must_use]
    const fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Spanish,
            _ => Self::English,
        }
    }
}

/// Process-global active locale, stored as the [`Locale::as_u8`] tag.
///
/// Defaults to English (`0`) so any string resolved before [`set_locale`] runs
/// (e.g. a very early log line) is English — identical to the pre-i18n binary.
static CURRENT_LOCALE: AtomicU8 = AtomicU8::new(0);

/// Set the process-global active locale.
///
/// Called once at startup from the `[ui] locale` config value, and again
/// whenever the user picks a language in Settings. A relaxed store is
/// sufficient: egui reads it on the next frame and there is no cross-thread
/// ordering dependency on the value.
pub fn set_locale(locale: Locale) {
    CURRENT_LOCALE.store(locale.as_u8(), Ordering::Relaxed);
}

/// The current process-global active locale.
#[must_use]
pub fn current_locale() -> Locale {
    Locale::from_u8(CURRENT_LOCALE.load(Ordering::Relaxed))
}

/// Generates the whole string catalog from one table.
///
/// Each row is `Key => "english", spanish;` where `spanish` is `Some("...")` or
/// `None` (fall back to English). From that single table the macro emits:
///
/// - the public [`Key`] enum, one variant per row, each documented with its
///   English source string;
/// - [`Key::ALL`], every variant in table order — built from the same rows, so
///   it can never miss a key (the coverage tests iterate it);
/// - the private `english` catalog (a `match` that must be exhaustive, so a
///   key without an English string cannot compile);
/// - the private `spanish` catalog (`Option`, `None` = English fallback).
///
/// One table rather than three parallel `match` blocks keeps a key, its
/// English source and its translation on adjacent lines, which is what a
/// reviewer of a translation actually needs to see.
macro_rules! catalog {
    ( $( $key:ident => $en:literal, $es:expr; )* ) => {
        /// Translatable string keys.
        ///
        /// Each variant maps to a `&'static str` in every catalog; its rustdoc is
        /// the English source string. Adding a key means adding one row to the
        /// `catalog!` table in `i18n.rs` (the verbatim English string plus the
        /// Spanish translation, or `None` to fall back to English).
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[non_exhaustive]
        pub enum Key {
            $(
                #[doc = concat!("English: `", $en, "`")]
                $key,
            )*
        }

        impl Key {
            /// Every key, in catalog order. Generated from the same table as the
            /// enum, so it is complete by construction; the coverage tests
            /// iterate it.
            pub const ALL: &'static [Key] = &[ $( Key::$key, )* ];
        }

        /// English catalog — the verbatim source strings. This is also the
        /// fallback for every other locale, so it defines a value for *every*
        /// [`Key`] (the `match` is exhaustive by construction).
        //
        // `match_same_arms`: two distinct keys may share a string (the
        // "Emulation" menu and the "Emulation" settings tab), but they are
        // independent strings that can diverge in another locale or a later
        // edit, so each keeps its own arm rather than being merged.
        #[allow(clippy::match_same_arms)]
        const fn english(key: Key) -> &'static str {
            match key {
                $( Key::$key => $en, )*
            }
        }

        /// Spanish catalog. Returns `None` for any key not translated, which
        /// [`tr`] resolves to the English value (fallback).
        ///
        /// **MACHINE-DRAFTED — AWAITS NATIVE-SPEAKER REVIEW.** Apart from the
        /// v1.7.0 menu/status entries, every Spanish string in the table was
        /// machine-drafted at v2.9.7 in neutral Latin American / international
        /// Spanish and has not been reviewed by a native speaker. See the i18n
        /// section of `docs/frontend.md`.
        //
        // `match_same_arms`: see `english`.
        #[allow(clippy::match_same_arms)]
        const fn spanish(key: Key) -> Option<&'static str> {
            match key {
                $( Key::$key => $es, )*
            }
        }
    };
}

// The catalog. Column 1 is the key, column 2 the verbatim English string the
// UI rendered before the key existed (so the default locale is byte-identical),
// column 3 the Spanish.
//
// SPANISH: MACHINE-DRAFTED, AWAITING NATIVE-SPEAKER REVIEW. Every Spanish entry
// added at v2.9.7 (everything below the "v2.9.7" banner) was drafted by
// machine translation in neutral Latin American / international Spanish and
// has not been reviewed by a native speaker. The v1.7.0 entries above the
// banner predate that and are unchanged.
//
// Placeholders: `{0}`, `{1}`, ... are filled by [`tr_fmt`] / `tf!`; the
// Spanish must use the same set of placeholders as the English (a test checks).
catalog! {
    // ----- Menu bar: top-level menus -----
    MenuFile => "File", Some("Archivo");
    MenuEmulation => "Emulation", Some("Emulación");
    MenuTools => "Tools", Some("Herramientas");
    MenuView => "View", Some("Ver");
    MenuDebug => "Debug", Some("Depurar");
    MenuHelp => "Help", Some("Ayuda");

    // ----- Menu bar: common items -----
    MenuOpenRom => "Open ROM...", Some("Abrir ROM...");
    MenuOpenRecent => "Open Recent", Some("Abrir reciente");
    MenuNoRecentRoms => "No recent ROMs", Some("Sin ROMs recientes");
    MenuSaveStates => "Save States", Some("Estados guardados");
    MenuQuit => "Quit", Some("Salir");
    MenuTheme => "Theme", Some("Tema");
    MenuWindowSize => "Window Size", Some("Tamaño de ventana");

    // ----- Settings window: tabs + section headers -----
    SettingsTitle => "Settings", Some("Configuración");
    SettingsTabVideo => "Video", Some("Vídeo");
    // "Shaders" and "Audio" are kept untranslated by returning `None`: both
    // are loanwords used verbatim in Spanish UIs, so they fall through to the
    // English catalog. This also keeps the English-fallback path live and
    // unit-tested (see `missing_key_falls_back_to_english`).
    SettingsTabShaders => "Shaders", None;
    SettingsTabAudio => "Audio", None;
    SettingsTabInput => "Input", Some("Controles");
    SettingsTabEmulation => "Emulation", Some("Emulación");
    SettingsHeadingDisplay => "Display", Some("Pantalla");
    SettingsHeadingAccessibility => "Accessibility", Some("Accesibilidad");
    SettingsTheme => "Theme:", Some("Tema:");
    SettingsLanguage => "Language:", Some("Idioma:");

    // ----- Status bar -----
    StatusNoRom => "No ROM loaded", Some("Sin ROM cargada");
    StatusIdle => "Idle", Some("Inactivo");
    StatusRunning => "Running", Some("En ejecución");
    StatusPaused => "Paused", Some("Pausado");
    StatusNetplay => "Netplay", Some("Juego en red");

    // ----- Common dialog buttons -----
    ButtonOk => "OK", Some("Aceptar");
    ButtonCancel => "Cancel", Some("Cancelar");
    ButtonSave => "Save", Some("Guardar");
    ButtonLoad => "Load", Some("Cargar");
    ButtonApply => "Apply", Some("Aplicar");
    ButtonReset => "Reset", Some("Restablecer");

    // =====================================================================
    // v2.9.7 "Tandem": the user-facing panels. Spanish MACHINE-DRAFTED,
    // awaiting native-speaker review (see the note above `catalog!`).
    // =====================================================================
    // ----- Cartridge Info / header editor (debugger/header_editor.rs) -----
    HdrTitle => "Cartridge Info / Header", Some("Información del cartucho / Cabecera");
    HdrOpenRomFile => "Open ROM file...", Some("Abrir archivo ROM...");
    HdrOpenHint => "Open a .nes / NES 2.0 ROM file to inspect its header.", Some("Abre un archivo ROM .nes / NES 2.0 para inspeccionar su cabecera.");
    HdrEditToggle => "Edit header (writes the file)", Some("Editar cabecera (escribe el archivo)");
    HdrWriteToFile => "Write header to file", Some("Escribir cabecera en el archivo");
    HdrFormat => "Format", Some("Formato");
    HdrMirroring => "Mirroring", Some("Espejado");
    HdrBattery => "Battery", Some("Batería");
    HdrYes => "yes", Some("sí");
    HdrNo => "no", Some("no");
    HdrRegion => "Region", Some("Región");
    HdrConsole => "Console", Some("Consola");
    HdrNes2Toggle => "NES 2.0 (vs iNES 1.0)", Some("NES 2.0 (frente a iNES 1.0)");
    HdrPrgUnits => "PRG (16 KiB units):", Some("PRG (unidades de 16 KiB):");
    HdrChrUnits => "CHR (8 KiB units):", Some("CHR (unidades de 8 KiB):");
    HdrBatteryRam => "Battery-backed save RAM", Some("RAM de guardado con batería");
    HdrTrainerPresent => "512-byte trainer present", Some("Trainer de 512 bytes presente");
    HdrVsDualBoard => "Vs. DualSystem board", Some("Placa Vs. DualSystem");
    HdrStatusLoaded => "header loaded", Some("cabecera cargada");
    HdrStatusWritten => "header written", Some("cabecera escrita");
    HdrFile => "file: {0}", Some("archivo: {0}");
    HdrBytesKib => "{0} bytes ({1} KiB)", Some("{0} bytes ({1} KiB)");
    HdrBytes => "{0} bytes", Some("{0} bytes");
    HdrEditNote => "Edits the 16-byte header of the file on disk only (not the running core). Sizes are stored in standard 16K/8K-unit notation.", Some("Edita solo la cabecera de 16 bytes del archivo en disco (no el núcleo en ejecución). Los tamaños se guardan en la notación estándar de unidades de 16K/8K.");
    HdrStatusInvalid => "not a valid iNES/NES2.0 header: {0}", Some("no es una cabecera iNES/NES2.0 válida: {0}");
    HdrStatusReadFailed => "read failed: {0}", Some("error de lectura: {0}");
    HdrStatusWriteFailed => "write failed: {0}", Some("error de escritura: {0}");

    // ----- ROM Info panel (debugger/rom_info_panel.rs) -----
    RomInfoTitle => "ROM Info", Some("Información de la ROM");
    RomInfoIdentity => "Identity", Some("Identidad");
    RomInfoTitleDb => "Title (game DB)", Some("Título (BD de juegos)");
    RomInfoNotInDb => "(not in database)", Some("(no está en la base de datos)");
    RomInfoCrcDbKey => "CRC32 (game-DB key)", Some("CRC32 (clave de la BD de juegos)");
    RomInfoNoCartCrc => "(no cartridge CRC)", Some("(sin CRC de cartucho)");
    RomInfoCrcNoIntro => "CRC32 (No-Intro, full file)", Some("CRC32 (No-Intro, archivo completo)");
    RomInfoUnavailable => "(unavailable)", Some("(no disponible)");
    RomInfoCartridge => "Cartridge", Some("Cartucho");
    RomInfoChrRamNoRom => "CHR-RAM (no CHR ROM)", Some("CHR-RAM (sin CHR ROM)");
    RomInfoMirroringDb => "Mirroring (DB)", Some("Espejado (BD)");
    RomInfoSubmapperDb => "Submapper (DB)", Some("Submapper (BD)");
    RomInfoKibBytes => "{0} KiB ({1} bytes)", Some("{0} KiB ({1} bytes)");
    RomInfoMapperDb => "{0} (DB: {1})", Some("{0} (BD: {1})");
    RomInfoFooter => "Read-only. Metadata from the vendored per-game database + the cartridge header. Edit corrections in Tools -> ROM Database.", Some("Solo lectura. Metadatos de la base de datos por juego incluida + la cabecera del cartucho. Edita las correcciones en Herramientas -> Base de datos de ROM.");

    // ----- Netplay panel, native UDP (debugger/netplay_panel.rs) -----
    NpUseBrowserPanel => "Use the \"Netplay (browser)\" panel", Some("Usa el panel \"Juego en red (navegador)\"");
    NpStatus => "Status", Some("Estado");
    NpSinglePlayer => "Single-player (not connected).", Some("Un jugador (sin conexión).");
    NpHostP1 => "host (P1)", Some("anfitrión (P1)");
    NpJoinerP2 => "joiner (P2)", Some("invitado (P2)");
    NpStalled => "stalled (time-sync)", Some("detenido (sincronización de tiempo)");
    NpSpectating => "Spectating (read-only)", Some("Observando (solo lectura)");
    NpHostHeading => "Host (player 1)", Some("Anfitrión (jugador 1)");
    NpLocalPort => "local port:", Some("puerto local:");
    NpPlayers => "players:", Some("jugadores:");
    NpFourScoreNote => "3-4 players use the Four Score adapter.", Some("3-4 jugadores usan el adaptador Four Score.");
    NpHostButton => "Host", Some("Hospedar");
    NpJoinHeading => "Join (player 2)", Some("Unirse (jugador 2)");
    NpHostPort => "host:port:", Some("anfitrión:puerto:");
    NpIpPortHint => "ip:port", Some("ip:puerto");
    NpJoinButton => "Join", Some("Unirse");
    NpSpectateHeading => "Spectate (watch, read-only)", Some("Observar (ver, solo lectura)");
    NpSpectateButton => "Spectate", Some("Observar");
    NpLeave => "Leave", Some("Abandonar");
    NpDiagnostics => "Diagnostics", Some("Diagnóstico");
    NpTopology => "Topology", Some("Topología");
    NpNoSession => "(no active session)", Some("(sin sesión activa)");
    NpStateChecksums => "State checksums", Some("Sumas de verificación del estado");
    NpKindMatch => "match", Some("coincide");
    NpKindTiming => "timing (same picture)", Some("temporización (misma imagen)");
    NpKindState => "state (picture differs)", Some("estado (la imagen difiere)");
    NpRecentCrc => "Recent CRC history", Some("Historial reciente de CRC");
    NpColFrame => "frame", Some("fotograma");
    NpColLocal => "local", Some("local");
    NpColRemote => "remote", Some("remoto");
    NpColOk => "ok", Some("ok");
    NpNoUpper => "NO", Some("NO");
    NpYes => "yes", Some("sí");
    NpWasmNativeOnly => "This UDP netplay panel is native-only (a browser cannot open a raw UDP socket). In the browser, use the separate \"Netplay (browser)\" panel, which runs the same rollback netcode over WebRTC via a signaling server (2-4 players).", Some("Este panel de juego en red UDP es solo nativo (un navegador no puede abrir un socket UDP directo). En el navegador, usa el panel separado \"Juego en red (navegador)\", que ejecuta el mismo código de red con rollback sobre WebRTC mediante un servidor de señalización (2-4 jugadores).");
    NpWasmTip => "Tip: keep BOTH browser windows visible side-by-side — a backgrounded tab is rAF-throttled by the browser and will desync the session.", Some("Consejo: mantén AMBAS ventanas del navegador visibles lado a lado; el navegador limita requestAnimationFrame en una pestaña en segundo plano y la sesión se desincronizará.");
    NpConnectingAs => "Connecting as {0}...", Some("Conectando como {0}...");
    NpPingMs => "ping: {0} ms", Some("ping: {0} ms");
    NpInGameAs => "In game as {0}", Some("En partida como {0}");
    NpInGameStats => "ping: {0}   frame: {1}   confirmed: {2}", Some("ping: {0}   fotograma: {1}   confirmado: {2}");
    NpRollback => "rollback x{0}", Some("rollback x{0}");
    NpSpectateStats => "frame: {0}   confirmed: {1}   behind: {2}", Some("fotograma: {0}   confirmado: {1}   atrasado: {2}");
    NpSpectateNote => "You are watching the match's confirmed input stream. Your controls do nothing and you send no input.", Some("Estás viendo el flujo de entradas confirmadas de la partida. Tus controles no hacen nada y no envías ninguna entrada.");
    NpError => "Error: {0}", Some("Error: {0}");
    NpHostNote => "Share your IP:port with the joiner. The host waits and learns the joiner's address from its first connect.", Some("Comparte tu IP:puerto con quien se une. El anfitrión espera y obtiene la dirección del invitado en su primera conexión.");
    NpSpectateHint => "Watch a running match without joining it. You replay the confirmed input stream locally and send no input, so you cannot affect the players.", Some("Mira una partida en curso sin unirte. Reproduces localmente el flujo de entradas confirmadas y no envías ninguna entrada, así que no puedes afectar a los jugadores.");
    NpPeersNote => "Both peers must run the SAME ROM (the handshake checks the SHA-256). The host is P1, the joiner is P2; both use their own player-1 controls.", Some("Ambos participantes deben ejecutar la MISMA ROM (la negociación comprueba el SHA-256). El anfitrión es P1 y el invitado es P2; ambos usan sus propios controles de jugador 1.");
    NpTopologyPlayers => "{0} players (mesh){1}", Some("{0} jugadores (malla){1}");
    NpYouDrive => "you drive: player {0} = {1}", Some("controlas: jugador {0} = {1}");
    NpInSync => "in sync ({0} compares OK)", Some("sincronizado ({0} comparaciones correctas)");
    NpDesynced => "DESYNCED at frame {0} ({1} mismatches / {2} compares)", Some("DESINCRONIZADO en el fotograma {0} ({1} discrepancias / {2} comparaciones)");
    NpConsecutiveMismatches => "consecutive mismatches: {0}", Some("discrepancias consecutivas: {0}");
    NpLastCompare => "last @ frame {0}: local {1} vs remote {2} [{3}]", Some("último @ fotograma {0}: local {1} frente a remoto {2} [{3}]");

    // ----- Browser netplay lobby, wasm (wasm_lobby.rs) -----
    LobbyTitle => "Netplay (browser)", Some("Juego en red (navegador)");
    LobbyConnecting => "Connecting (signaling + WebRTC handshake)...", Some("Conectando (señalización + negociación WebRTC)...");
    LobbyRoleHost => "host", Some("anfitrión");
    LobbyRoleJoiner => "joiner", Some("invitado");
    LobbySignalingServer => "Signaling server", Some("Servidor de señalización");
    LobbyRoom => "room:", Some("sala:");
    LobbyCodeHint => "lobby code", Some("código de sala");
    LobbyRole => "role:", Some("rol:");
    LobbyHostP1 => "Host (P1)", Some("Anfitrión (P1)");
    LobbyJoinP2 => "Join (P2)", Some("Unirse (P2)");
    LobbyConnect => "Connect", Some("Conectar");
    LobbyNeedUrl => "enter a signaling-server URL first", Some("primero introduce la URL del servidor de señalización");
    LobbyNeedRoom => "enter a room code first", Some("primero introduce un código de sala");
    LobbyInGame => "In game ({0} players, joined as {1})", Some("En partida ({0} jugadores, unido como {1})");
    LobbyFourScoreNote => "3-4 players use the Four Score adapter and form a full WebRTC mesh (every peer connected to every other). All players must share the room code; each gets the next free slot.", Some("3-4 jugadores usan el adaptador Four Score y forman una malla WebRTC completa (cada participante conectado con todos los demás). Todos los jugadores deben compartir el código de sala; cada uno obtiene la siguiente posición libre.");
    LobbyPeersNote => "Both peers must run the SAME ROM (the signaling handshake checks the SHA-256) and point at the same signaling server + room code. A live session needs the server deployed (see deploy/) and two browsers.", Some("Ambos participantes deben ejecutar la MISMA ROM (la negociación de señalización comprueba el SHA-256) y apuntar al mismo servidor de señalización + código de sala. Una sesión real necesita el servidor desplegado (ver deploy/) y dos navegadores.");
    LobbyVisibleNote => "IMPORTANT: keep every player's window VISIBLE (side-by-side, not a background tab). Browsers throttle requestAnimationFrame in hidden tabs, which stalls and desyncs the rollback session.", Some("IMPORTANTE: mantén VISIBLE la ventana de cada jugador (lado a lado, no en una pestaña en segundo plano). Los navegadores limitan requestAnimationFrame en pestañas ocultas, lo que detiene y desincroniza la sesión con rollback.");

    // ----- Cheats panel (debugger/cheat_panel.rs) -----
    CheatTitle => "Cheats (Game Genie)", Some("Trucos (Game Genie)");
    CheatAdd => "Add", Some("Agregar");
    CheatNoGenie => "No Game Genie cheats. Enter a 6- or 8-character code above.", Some("No hay trucos de Game Genie. Escribe arriba un código de 6 u 8 caracteres.");
    CheatColOn => "On", Some("Activo");
    CheatColCode => "Code", Some("Código");
    CheatColEffect => "Effect", Some("Efecto");
    CheatEncoder => "Game Genie encoder", Some("Codificador de Game Genie");
    CheatRamHeading => "RAM cheats", Some("Trucos de RAM");
    CheatAddrDollar => "Addr $", Some("Dir. $");
    CheatAddr => "Addr", Some("Dir.");
    CheatIf => "if", Some("si");
    CheatAnyHint => "(any)", Some("(cualquiera)");
    CheatEncode => "Encode", Some("Codificar");
    CheatAddToList => "Add to list", Some("Agregar a la lista");
    CheatErrAddrHex => "address must be hex", Some("la dirección debe ser hexadecimal");
    CheatErrAddrRange => "address must be in $8000-$FFFF", Some("la dirección debe estar en $8000-$FFFF");
    CheatErrDataHex => "data must be a hex byte", Some("el dato debe ser un byte hexadecimal");
    CheatErrCompareHex => "compare must be a hex byte (or blank)", Some("la comparación debe ser un byte hexadecimal (o quedar vacía)");
    CheatNoRam => "No RAM cheats. Enter a hex address ($0000-$1FFF) and value above.", Some("No hay trucos de RAM. Escribe arriba una dirección hexadecimal ($0000-$1FFF) y un valor.");
    CheatKnownCodes => "Known codes:", Some("Códigos conocidos:");
    CheatPickCode => "Pick a code…", Some("Elige un código…");
    CheatEncoderNote => "Address is the PRG byte the code substitutes ($8000-$FFFF). With a compare byte you get an 8-character (bank-specific) code.", Some("La dirección es el byte de PRG que sustituye el código ($8000-$FFFF). Con un byte de comparación obtienes un código de 8 caracteres (específico del banco).");
    CheatAlreadyInList => "{0} is already in the list", Some("{0} ya está en la lista");
    CheatGamePickCode => "{0} — pick a code…", Some("{0} — elige un código…");
    CheatErrBadAddr => "invalid hex address '{0}'", Some("dirección hexadecimal no válida '{0}'");
    CheatErrAddrOutOfRange => "address ${0} out of range (must be $0000-$1FFF)", Some("dirección ${0} fuera de rango (debe estar en $0000-$1FFF)");
    CheatErrBadValue => "invalid hex value '{0}'", Some("valor hexadecimal no válido '{0}'");
    CheatErrBadCompare => "invalid hex compare '{0}'", Some("comparación hexadecimal no válida '{0}'");
    CheatNotSaved => "cheats not saved: {0}", Some("trucos no guardados: {0}");

    // ----- Input bindings panel (debugger/input_rebind_panel.rs) -----
    RebindTitle => "Input bindings", Some("Asignación de controles");
    RebindCancelled => "(cancelled)", Some("(cancelado)");
    RebindSnesMouse => "SNES mouse", Some("Ratón de SNES");
    RebindReportedSensitivity => "Reported sensitivity", Some("Sensibilidad informada");
    RebindLow => "Low", Some("Baja");
    RebindMedium => "Medium", Some("Media");
    RebindHigh => "High", Some("Alta");
    RebindVausPaddle => "Arkanoid Vaus paddle", Some("Control Vaus de Arkanoid");
    RebindMatLayout => "Mat layout", Some("Disposición del tapete");
    RebindPointerSpeed => "Pointer speed", Some("Velocidad del puntero");
    RebindPressPad => "Press any gamepad button (rebind again to cancel)", Some("Presiona cualquier botón del mando (vuelve a reasignar para cancelar)");
    RebindPressKey => "Press any key (Esc to cancel)", Some("Presiona cualquier tecla (Esc para cancelar)");
    RebindExportConfig => "Export config...", Some("Exportar configuración...");
    RebindExportDialogTitle => "Export RustyNES config", Some("Exportar configuración de RustyNES");
    RebindResetDefaults => "Reset to defaults", Some("Restablecer valores predeterminados");
    RebindDefaultsRestored => "Defaults restored.", Some("Valores predeterminados restaurados.");
    RebindFourScore => "Four Score (4-player)", Some("Four Score (4 jugadores)");
    RebindStickDeadzone => "Gamepad stick deadzone", Some("Zona muerta del stick del mando");
    RebindTurboHeading => "Turbo / autofire", Some("Turbo / disparo automático");
    RebindTurboSpeed => "Turbo speed", Some("Velocidad del turbo");
    RebindFrames => "frames", Some("fotogramas");
    RebindAllowOpposing => "Allow opposing directions (Up+Down, Left+Right)", Some("Permitir direcciones opuestas (Arriba+Abajo, Izquierda+Derecha)");
    RebindPort2Device => "Port 2 device ($4017)", Some("Dispositivo del puerto 2 ($4017)");
    RebindStandardController => "Standard controller", Some("Control estándar");
    RebindZapper => "Zapper (light gun)", Some("Zapper (pistola de luz)");
    RebindVaus => "Vaus (Arkanoid paddle)", Some("Vaus (control de Arkanoid)");
    RebindPowerPad => "Power Pad (mat)", Some("Power Pad (tapete)");
    RebindFamilyKeyboard => "Family BASIC keyboard", Some("Teclado Family BASIC");
    RebindFamilyTrainer => "Family Trainer (mat)", Some("Family Trainer (tapete)");
    RebindSuborKeyboard => "Subor keyboard", Some("Teclado Subor");
    RebindKeyboard => "Keyboard", Some("Teclado");
    RebindColKey => "Key", Some("Tecla");
    RebindGamepad => "Gamepad", Some("Mando");
    RebindColButton => "Button", Some("Botón");
    RebindColAction => "Action", Some("Acción");
    RebindButton => "rebind", Some("reasignar");
    RebindExportHover => "Settings auto-save continuously. Use this to export a copy of the whole config to a chosen .toml file.", Some("La configuración se guarda automáticamente en todo momento. Usa esto para exportar una copia de toda la configuración a un archivo .toml que elijas.");
    RebindExportedTo => "Exported to {0}", Some("Exportado a {0}");
    RebindExportError => "export error: {0}", Some("error al exportar: {0}");
    RebindAllowOpposingHover => "Off: holding opposite directions together reads as neither, as on a real NES pad. On: both reach the game (some games glitch on it).", Some("Desactivado: mantener direcciones opuestas a la vez no se lee como ninguna, como en un control real de NES. Activado: ambas llegan al juego (algunos juegos fallan con ello).");
    RebindRebound => "Rebound {0} -> {1}", Some("Reasignado {0} -> {1}");
    RebindRowPlayer => "Player{0} {1}", Some("Jugador{0} {1}");
    RebindRowPad => "Pad{0} {1}", Some("Mando{0} {1}");
    RebindDirUp => "Up", Some("Arriba");
    RebindDirDown => "Down", Some("Abajo");
    RebindDirLeft => "Left", Some("Izquierda");
    RebindDirRight => "Right", Some("Derecha");
    RebindActQuit => "Quit", Some("Salir");
    RebindActSaveState => "Save state", Some("Guardar estado");
    RebindActLoadState => "Load state", Some("Cargar estado");
    RebindActRewind => "Rewind (hold)", Some("Rebobinar (mantener)");
    RebindActReset => "Reset", Some("Reiniciar");
    RebindActPowerCycle => "Power cycle", Some("Apagar y encender");
    RebindActDebugOverlay => "Debug overlay", Some("Superposición de depuración");
    RebindActOpenRom => "Open ROM", Some("Abrir ROM");
    RebindActPause => "Pause / resume", Some("Pausar / reanudar");
    RebindActFrameAdvance => "Frame advance", Some("Avanzar fotograma");
    RebindActFastForward => "Fast forward (hold)", Some("Avance rápido (mantener)");
    RebindActFullscreen => "Fullscreen", Some("Pantalla completa");
    RebindActToggleMenuBar => "Toggle menu bar", Some("Mostrar/ocultar barra de menús");
    RebindActSpeedUp => "Speed up", Some("Aumentar velocidad");
    RebindActSpeedDown => "Speed down", Some("Reducir velocidad");
    RebindActSpeedReset => "Speed reset", Some("Restablecer velocidad");
    RebindActMovieRecord => "Movie record", Some("Grabar película");
    RebindActMoviePlay => "Movie play", Some("Reproducir película");
    RebindActMovieBranch => "Movie branch", Some("Ramificar película");
    RebindActDiskSwap => "Swap disk side (FDS)", Some("Cambiar lado del disco (FDS)");
    RebindActInsertCoin => "Insert coin (Vs.)", Some("Insertar moneda (Vs.)");

    // ----- Shell: menu bar, status bar, Settings chrome, welcome/about/shortcuts (ui_shell.rs) -----
    ShellUnknown => "Unknown", Some("Desconocido");
    ShellClearRecent => "Clear Recent", Some("Borrar recientes");
    ShellCloseRom => "Close ROM", Some("Cerrar ROM");
    ShellSaveState => "Save State", Some("Guardar estado");
    ShellLoadState => "Load State", Some("Cargar estado");
    ShellActiveSlot => "Active Slot", Some("Ranura activa");
    ShellSaveToSlot => "Save to Slot", Some("Guardar en ranura");
    ShellLoadFromSlot => "Load from Slot", Some("Cargar desde ranura");
    ShellManageStates => "Manage States...", Some("Administrar estados...");
    ShellTakeScreenshot => "Take Screenshot", Some("Tomar captura de pantalla");
    ShellCopyScreenshot => "Copy Screenshot to Clipboard", Some("Copiar captura al portapapeles");
    ShellResume => "Resume", Some("Reanudar");
    ShellPause => "Pause", Some("Pausar");
    ShellPowerCycle => "Power Cycle", Some("Apagar y encender");
    ShellFrameAdvance => "Frame Advance", Some("Avanzar fotograma");
    ShellVsInsertCoin => "Vs. Insert Coin", Some("Vs. Insertar moneda");
    ShellSwapDiskSide => "Swap Disk Side", Some("Cambiar lado del disco");
    ShellEject => "Eject", Some("Expulsar");
    ShellSettingsItem => "Settings...", Some("Configuración...");
    ShellPixelAspect => "8:7 Pixel Aspect", Some("Relación de píxel 8:7");
    ShellHideOverscan => "Hide Overscan", Some("Ocultar overscan");
    ShellShowFps => "Show FPS", Some("Mostrar FPS");
    ShellShowLagFrames => "Show Lag Frames", Some("Mostrar fotogramas de retraso");
    ShellPauseUnfocused => "Pause When Unfocused", Some("Pausar sin foco");
    ShellShowMenuBar => "Show Menu Bar", Some("Mostrar barra de menús");
    ShellCheatsItem => "Cheats...", Some("Trucos...");
    ShellMovies => "Movies & Recording", Some("Películas y grabación");
    ShellStopRecording => "Stop Recording", Some("Detener grabación");
    ShellRecord => "Record", Some("Grabar");
    ShellStopPlayback => "Stop Playback", Some("Detener reproducción");
    ShellPlay => "Play", Some("Reproducir");
    ShellBranch => "Branch", Some("Ramificar");
    ShellImportMovie => "Import (.fm2 / .bk2)", Some("Importar (.fm2 / .bk2)");
    ShellExportMovie => "Export (.fm2 / .bk2)", Some("Exportar (.fm2 / .bk2)");
    ShellExportSubtitles => "Export subtitles (.srt)", Some("Exportar subtítulos (.srt)");
    ShellReplayTas => "Replay / TAS", Some("Repetición / TAS");
    ShellStopAv => "Stop A/V Recording", Some("Detener grabación A/V");
    ShellRecordAv => "Record A/V...", Some("Grabar A/V...");
    ShellExportLast30 => "Export Last 30s (.rnm)", Some("Exportar últimos 30 s (.rnm)");
    ShellNsfPlayer => "NSF Player", Some("Reproductor NSF");
    ShellAudioMixer => "Audio Mixer", Some("Mezclador de audio");
    ShellInputDisplay => "Input Display", Some("Visualización de controles");
    ShellVirtualPad => "Virtual Pad", Some("Control virtual");
    ShellGameData => "Game Data", Some("Datos del juego");
    ShellRomDatabase => "ROM Database", Some("Base de datos de ROM");
    ShellAnalysis => "Analysis", Some("Análisis");
    ShellLatencyOracle => "Latency Oracle", Some("Oráculo de latencia");
    ShellPixelProvenance => "Pixel Provenance", Some("Procedencia de píxeles");
    ShellAudioProvenance => "Audio Provenance", Some("Procedencia del audio");
    ShellRamAtlas => "RAM Atlas", Some("Atlas de RAM");
    ShellDivergenceLens => "Divergence Lens", Some("Lente de divergencia");
    ShellLoadHdPack => "Load HD Pack...", Some("Cargar HD Pack...");
    ShellUnloadHdPack => "Unload HD Pack", Some("Descargar HD Pack");
    ShellPixelInspector => "Pixel Inspector", Some("Inspector de píxeles");
    ShellStopSaveHdPack => "Stop & Save HD Pack...", Some("Detener y guardar HD Pack...");
    ShellBuildHdPack => "Build HD Pack (Record)", Some("Crear HD Pack (grabar)");
    ShellNetplayItem => "Netplay...", Some("Juego en red...");
    ShellNetplayBrowserItem => "Netplay (browser)...", Some("Juego en red (navegador)...");
    ShellHeaderEditorItem => "Cartridge Info / Header Editor...", Some("Información del cartucho / Editor de cabecera...");
    ShellDocumentation => "Documentation...", Some("Documentación...");
    ShellDocumentationHover => "Searchable in-app manual, About, and changelog", Some("Manual integrado con búsqueda, Acerca de y registro de cambios");
    ShellKeyboardShortcuts => "Keyboard Shortcuts", Some("Atajos de teclado");
    ShellAbout => "About", Some("Acerca de");
    ShellNetplayStatusHover => "Netplay session status", Some("Estado de la sesión de juego en red");
    ShellRecMovie => "REC movie", Some("GRAB. película");
    ShellPlayMovie => "PLAY movie", Some("REPR. película");
    ShellRecAv => "REC A/V", Some("GRAB. A/V");
    ShellHdPackRec => "HD-Pack REC", Some("GRAB. HD-Pack");
    ShellRaHover => "RetroAchievements (Tools -> RetroAchievements)", Some("RetroAchievements (Herramientas -> RetroAchievements)");
    ShellPixelAspectRatio => "8:7 Pixel Aspect Ratio (NES native)", Some("Relación de aspecto de píxel 8:7 (nativa de NES)");
    ShellShowFpsStatus => "Show FPS in status bar", Some("Mostrar FPS en la barra de estado");
    ShellShowLagStatus => "Show lag-frame counter in status bar", Some("Mostrar contador de fotogramas de retraso en la barra de estado");
    ShellUiScale => "UI scale:", Some("Escala de la interfaz:");
    ShellResetUiScaleHover => "Reset UI scale to 100%", Some("Restablecer la escala de la interfaz al 100%");
    ShellWelcomeTitle => "Welcome to RustyNES", Some("Bienvenido a RustyNES");
    ShellWelcomeBlurb => "A cycle-accurate NES emulator written in Rust.", Some("Un emulador de NES con precisión de ciclo escrito en Rust.");
    ShellQuickStart => "Quick start:", Some("Inicio rápido:");
    ShellGetStarted => "Get Started", Some("Comenzar");
    ShellAboutTitle => "About RustyNES", Some("Acerca de RustyNES");
    ShellAboutBlurb => "A cycle-accurate NES emulator written in pure Rust.", Some("Un emulador de NES con precisión de ciclo escrito en Rust puro.");
    ShellCreatedBy => "Created by DoubleGate", Some("Creado por DoubleGate");
    ShellEmulatorHotkeys => "Emulator hotkeys", Some("Teclas rápidas del emulador");
    ShellDevice => "Device:", Some("Dispositivo:");
    ShellToggleRaDetail => "Toggle RA status detail", Some("Alternar detalle de estado de RA");
    ShellDebuggerPanels => "Debugger panels", Some("Paneles del depurador");
    ShellDebugMenu => "Debug menu", Some("menú Depurar");
    ShellQuitExitFullscreen => "Quit / exit fullscreen", Some("Salir / salir de pantalla completa");
    ShellPowerPadTopRow => "Top row (1-4)", Some("Fila superior (1-4)");
    ShellPowerPadMiddleRow => "Middle row (5-8)", Some("Fila central (5-8)");
    ShellPowerPadBottomRow => "Bottom row (9-12)", Some("Fila inferior (9-12)");
    ShellNote => "Note", Some("Nota");
    ShellPowerPadNote => "12-button mat; fixed default keys", Some("Tapete de 12 botones; teclas predeterminadas fijas");
    ShellLayout => "Layout", Some("Distribución");
    ShellKeyboardMatrix => "Host keyboard maps to the matrix", Some("El teclado del equipo se asigna a la matriz");
    ShellLettersDigits => "Letters / digits", Some("Letras / dígitos");
    ShellAsLabelled => "as labelled on your keyboard", Some("según las etiquetas de tu teclado");
    ShellKeyboardDeviceNote => "Active only with the Family BASIC / Subor keyboard device", Some("Activo solo con el dispositivo de teclado Family BASIC / Subor");
    ShellOpenRomKeys => "F12 / drag & drop", Some("F12 / arrastrar y soltar");
    ShellArrowKeys => "Arrow keys", Some("Teclas de flecha");
    ShellAButton => "A button", Some("Botón A");
    ShellBButton => "B button", Some("Botón B");
    ShellSlotN => "Slot {0}", Some("Ranura {0}");
    ShellFastForwardOn => "Fast Forward: ON (hold {0})", Some("Avance rápido: ACTIVADO (mantén {0})");
    ShellFastForwardHold => "Fast Forward (hold {0})", Some("Avance rápido (mantén {0})");
    ShellFastForwardHover => "Hold the bound key to run unthrottled (audio muted). Rebind in Settings -> Input.", Some("Mantén presionada la tecla asignada para ejecutar sin límite de velocidad (audio silenciado). Reasígnala en Configuración -> Controles.");
    ShellSpeedPct => "Speed: {0}%", Some("Velocidad: {0}%");
    ShellRegion => "Region: {0}", Some("Región: {0}");
    ShellSideN => "Side {0}", Some("Lado {0}");
    ShellShowLagFramesHover => "Show a counter of frames where the game polled no controller (a TAS / debug diagnostic).", Some("Muestra un contador de fotogramas en los que el juego no leyó ningún control (un diagnóstico de TAS / depuración).");
    ShellLagCount => "Lag: {0}", Some("Retraso: {0}");
    ShellLagHover => "Lag frames since ROM load (no controller polled). Toggle in View -> Show Lag Frames.", Some("Fotogramas de retraso desde que se cargó la ROM (sin leer ningún control). Actívalo en Ver -> Mostrar fotogramas de retraso.");
    ShellThemeHover => "High Contrast and Colorblind-Safe are accessibility themes (WCAG AA contrast / Okabe-Ito palette).", Some("High Contrast y Colorblind-Safe son temas de accesibilidad (contraste WCAG AA / paleta Okabe-Ito).");
    ShellLanguageHover => "Untranslated strings fall back to English.", Some("Los textos sin traducir se muestran en inglés.");
    ShellUiScaleNote => "Scales the menus, Settings, and debugger UI. The game image is not affected.", Some("Escala los menús, la configuración y la interfaz del depurador. La imagen del juego no se ve afectada.");
    ShellPlayer1 => "Player 1", Some("Jugador 1");
    ShellPlayer2 => "Player 2", Some("Jugador 2");
    ShellPlayer3 => "Player 3 (Four Score)", Some("Jugador 3 (Four Score)");
    ShellPlayer4 => "Player 4 (Four Score)", Some("Jugador 4 (Four Score)");

    // ----- Settings panel sections (debugger/settings_panel.rs) -----
    SetResetToDefaults => "Reset to Defaults", Some("Restablecer valores predeterminados");
    SetSaveToConfig => "Save to config.toml", Some("Guardar en config.toml");
    SetSaved => "Saved.", Some("Guardado.");
    SetWebNoSave => "(config save unavailable on web — changes are in-memory only)", Some("(guardar la configuración no está disponible en la web: los cambios solo se mantienen en memoria)");
    SetNotSet => "(not set)", Some("(sin definir)");
    SetBrowseBios => "Browse for disksys.rom\u{2026}", Some("Buscar disksys.rom\u{2026}");
    SetFdsNote => "Required to boot .fds disk images. Takes effect on the next FDS load.", Some("Necesaria para arrancar imágenes de disco .fds. Se aplica en la próxima carga de FDS.");
    SetRecordingHeader => "Recording (A/V codec depth)", Some("Grabación (códec A/V)");
    SetVideoCodec => "Video codec", Some("Códec de vídeo");
    SetCrf => "CRF (lower = better)", Some("CRF (menor = mejor)");
    SetPreset => "Preset", Some("Preajuste");
    SetX264Only => "(x264/x265 only)", Some("(solo x264/x265)");
    SetGraphics => "Graphics", Some("Gráficos");
    SetPresentMode => "Present mode", Some("Modo de presentación");
    SetRestartToApply => "(restart to apply)", Some("(reinicia para aplicar)");
    SetPacing => "Pacing", Some("Ritmo de fotogramas");
    SetPacingAuto => "auto (display-sync when refresh matches)", Some("auto (sincroniza con la pantalla si la frecuencia coincide)");
    SetPacingDisplay => "display (sync to vsync)", Some("display (sincroniza con vsync)");
    SetPacingWallclock => "wallclock (classic)", Some("wallclock (clásico)");
    SetMaxFrameLatency => "Max frame latency", Some("Latencia máxima de fotogramas");
    SetMaxFrameLatencyNote => "(1 = lowest latency; restart to apply)", Some("(1 = menor latencia; reinicia para aplicar)");
    SetNtscFilter => "NTSC filter", Some("Filtro NTSC");
    SetContrast => "Contrast", Some("Contraste");
    SetSaturation => "Saturation", Some("Saturación");
    SetBrightness => "Brightness", Some("Brillo");
    SetHue => "Hue", Some("Tono");
    SetHuePhase => "Hue (phase units)", Some("Tono (unidades de fase)");
    SetCrtScanlines => "CRT / scanlines", Some("CRT / líneas de barrido");
    SetScanlineIntensity => "Scanline intensity", Some("Intensidad de las líneas de barrido");
    SetHideOverscan => "Hide overscan (crop top + bottom 8 scanlines)", Some("Ocultar overscan (recortar 8 líneas arriba y abajo)");
    SetOverscanHeader => "Overscan (per-side, live)", Some("Overscan (por lado, en vivo)");
    SetEdgeTop => "Top", Some("Superior");
    SetEdgeBottom => "Bottom", Some("Inferior");
    SetEdgeLeft => "Left", Some("Izquierdo");
    SetEdgeRight => "Right", Some("Derecho");
    SetResetOverscan => "Reset overscan (0,0,0,0)", Some("Restablecer overscan (0,0,0,0)");
    SetPalette => "Palette", Some("Paleta");
    SetBuiltIn => "Built-in", Some("Integrada");
    SetLegacyPal => "Legacy .pal:", Some(".pal heredado:");
    SetNone => "none", Some("ninguno");
    SetLoadPal => "Load .pal…", Some("Cargar .pal…");
    SetClearPal => "Clear .pal", Some("Quitar .pal");
    SetNativeOnly => "(native only)", Some("(solo nativo)");
    SetGeneratedPalette => "Generated NTSC palette", Some("Paleta NTSC generada");
    SetUseGenerated => "Use generated palette (overrides built-in / .pal)", Some("Usar paleta generada (reemplaza la integrada / .pal)");
    SetPreview => "Preview (generated base, 16 x 4):", Some("Vista previa (base generada, 16 x 4):");
    SetPaletteEditor => "Palette editor", Some("Editor de paleta");
    SetPaletteName => "Palette name", Some("Nombre de la paleta");
    SetSaveAs => "Save as", Some("Guardar como");
    SetResetToBuiltIn => "Reset to built-in", Some("Restablecer a la integrada");
    SetImportPal => "Import .pal into bank…", Some("Importar .pal al banco…");
    SetNesPaletteFilter => "NES palette", Some("Paleta de NES");
    SetShaderStack => "Shader stack (composable)", Some("Pila de shaders (combinable)");
    SetAddPass => "Add pass", Some("Agregar pase");
    SetClearStack => "Clear stack", Some("Vaciar pila");
    SetNoPasses => "(no passes — using the default blit)", Some("(sin pases: se usa la copia directa predeterminada)");
    SetPresets => "Presets", Some("Preajustes");
    SetAddCrtPresets => "Add built-in CRT presets", Some("Agregar preajustes CRT integrados");
    SetPresetName => "Preset name", Some("Nombre del preajuste");
    SetSavePreset => "Save preset", Some("Guardar preajuste");
    SetLoadPreset => "Load preset…", Some("Cargar preajuste…");
    SetDelete => "Delete", Some("Eliminar");
    SetImportRaPreset => "Import RetroArch preset (constrained)", Some("Importar preajuste de RetroArch (limitado)");
    SetImportSlangp => "Import .slangp / .cgp…", Some("Importar .slangp / .cgp…");
    SetRaPresetFilter => "RetroArch preset", Some("Preajuste de RetroArch");
    SetVolume => "Volume", Some("Volumen");
    SetMute => "Mute", Some("Silenciar");
    SetFilterModel => "Filter model", Some("Modelo de filtro");
    SetFilterFamicom => "Famicom (37 Hz HPF — fuller)", Some("Famicom (HPF de 37 Hz: más cuerpo)");
    SetFilterClean => "Clean (full-range — fullest)", Some("Limpio (rango completo: máximo cuerpo)");
    SetFilterNes => "NES front-loader (authentic)", Some("NES de carga frontal (auténtico)");
    SetChannels => "Channels", Some("Canales");
    SetPulse1 => "Pulse 1", Some("Pulso 1");
    SetPulse2 => "Pulse 2", Some("Pulso 2");
    SetTriangle => "Triangle", Some("Triángulo");
    SetNoise => "Noise", Some("Ruido");
    SetMapperAudio => "Mapper Audio", Some("Audio del mapper");
    SetChannelVolume => "Channel volume", Some("Volumen por canal");
    SetResetVolumes => "Reset volumes (1.0)", Some("Restablecer volúmenes (1.0)");
    SetGraphicEq => "Graphic EQ", Some("Ecualizador gráfico");
    SetEq20 => "20-band graphic EQ", Some("Ecualizador gráfico de 20 bandas");
    SetEq20Hover => "ISO third-octave bands (25 Hz–20 kHz); off uses the classic 5 bands", Some("Bandas ISO de tercio de octava (25 Hz–20 kHz); desactivado usa las 5 bandas clásicas");
    SetResetEq => "Reset EQ (flat)", Some("Restablecer ecualizador (plano)");
    SetStereo => "Stereo", Some("Estéreo");
    SetReverb => "Reverb", Some("Reverberación");
    SetRoom => "room", Some("sala");
    SetCrossfeed => "Crossfeed", Some("Mezcla cruzada");
    SetCrossfeedHover => "Headphone L/R blend; 0 = off", Some("Mezcla I/D para auriculares; 0 = desactivado");
    SetResetStereo => "Reset stereo (center / dry)", Some("Restablecer estéreo (centro / seco)");
    SetContextVolume => "Context volume", Some("Volumen por contexto");
    SetMaster => "Master", Some("Maestro");
    SetGame => "Game", Some("Juego");
    SetMenu => "Menu", Some("Menú");
    SetOutputDevice => "Output device", Some("Dispositivo de salida");
    SetSystemDefault => "System default", Some("Predeterminado del sistema");
    SetSaveAudio => "Save audio settings", Some("Guardar configuración de audio");
    SetSampleRate => "Sample rate", Some("Frecuencia de muestreo");
    SetAudioLatency => "Audio latency", Some("Latencia de audio");
    SetDrc => "Dynamic rate control", Some("Control dinámico de frecuencia");
    SetDrcNote => "(±0.5% drift compensation; restart to apply)", Some("(compensación de deriva de ±0.5%; reinicia para aplicar)");
    SetLatency => "Latency", Some("Latencia");
    SetRunAhead => "Run-ahead (frames)", Some("Run-ahead (fotogramas)");
    SetRunAheadNote => "(removes the game's internal input lag; 1 fits most games)", Some("(elimina el retraso de entrada interno del juego; 1 sirve para la mayoría de los juegos)");
    SetRewind => "Rewind", Some("Rebobinado");
    SetEnabled => "Enabled", Some("Activado");
    SetWindowSeconds => "Window (seconds)", Some("Ventana (segundos)");
    SetKeyframePeriod => "Keyframe period (frames)", Some("Periodo de fotogramas clave (fotogramas)");
    SetAccuracy => "Accuracy", Some("Precisión");
    SetOamDecay => "OAM decay (accuracy)", Some("Degradación de OAM (precisión)");
    SetFamicomConsole => "Famicom console (PPU leaves reset early)", Some("Consola Famicom (la PPU sale antes del reinicio)");
    SetFastDotPath => "Fast PPU dot path (performance, not accuracy)", Some("Ruta rápida de puntos de la PPU (rendimiento, no precisión)");
    SetEnhancements => "Enhancements (non-accuracy)", Some("Mejoras (ajenas a la precisión)");
    SetDisableSpriteLimit => "Disable 8-sprite-per-scanline limit (reduces flicker)", Some("Desactivar el límite de 8 sprites por línea (reduce el parpadeo)");
    SetOverclock => "Overclock (extra scanlines)", Some("Overclock (líneas extra)");
    SetMaxRewind => "Max rewind (seconds)", Some("Rebobinado máximo (segundos)");
    SetMaxRewindNote => "(also in Rewind; restart to resize the buffer)", Some("(también en Rebobinado; reinicia para redimensionar el búfer)");
    SetConfirmReset => "Confirm reset {0}?", Some("¿Confirmar restablecimiento de {0}?");
    SetSectionGraphics => "graphics", Some("gráficos");
    SetSectionAudio => "audio", Some("audio");
    SetSectionLatencyRewind => "latency/rewind", Some("latencia/rebobinado");
    SetSaveError => "save error: {0}", Some("error al guardar: {0}");
    SetFdsWrongSize => "Not an FDS BIOS: {0} bytes (need 8192) - path NOT changed.", Some("No es una BIOS de FDS: {0} bytes (se necesitan 8192) - la ruta NO cambió.");
    SetFdsRecognized => "Recognized: {0} - path set.", Some("Reconocida: {0} - ruta establecida.");
    SetFdsUnverified => "8 KiB, unverified dump (sha256 {0}\u{2026}) - path set.", Some("8 KiB, volcado no verificado (sha256 {0}\u{2026}) - ruta establecida.");
    SetReadError => "read error: {0}", Some("error de lectura: {0}");
    SetOverscanNote => "Trim each edge independently (NES pixels). Combined with the toggle above; preview updates live.", Some("Recorta cada borde por separado (píxeles de NES). Se combina con la opción de arriba; la vista previa se actualiza en vivo.");
    SetPaletteEditorNote => "Click a swatch to edit its colour. 8 columns x 8 rows = the 64 NES base colours; emphasis is applied by the renderer.", Some("Haz clic en una muestra para editar su color. 8 columnas x 8 filas = los 64 colores base de NES; el énfasis lo aplica el renderizador.");
    SetDeletePalette => "Delete \"{0}\"", Some("Eliminar \"{0}\"");
    SetShaderStackNote => "Passes run top to bottom. An empty / all-disabled stack uses the default direct blit (no change to the image).", Some("Los pases se ejecutan de arriba abajo. Una pila vacía o con todo desactivado usa la copia directa predeterminada (sin cambios en la imagen).");
    SetUnknownPass => "{0} (unknown)", Some("{0} (desconocido)");
    SetRaImportNote => "Recognizes common crt / ntsc / hqx / xbr preset names and maps them onto the built-in passes. Source shaders are not translated; unrecognized passes are reported.", Some("Reconoce nombres comunes de preajustes crt / ntsc / hqx / xbr y los asigna a los pases integrados. Los shaders de origen no se traducen; los pases no reconocidos se informan.");
    SetImportedPasses => "Imported {0} pass(es); {1} unsupported.", Some("Se importaron {0} pase(s); {1} no compatibles.");
    SetImportFailed => "Import failed: {0}", Some("Error al importar: {0}");
    SetCouldNotRead => "Could not read file: {0}", Some("No se pudo leer el archivo: {0}");
    SetFilterModelHover => "NES front-loader high-passes hard (thin bass — authentic). Famicom/Clean keep more low end (closer to Mesen2).", Some("La NES de carga frontal aplica un filtro paso alto fuerte (graves débiles: auténtico). Famicom/Limpio conservan más graves (más cerca de Mesen2).");
    SetOamDecayHover => "Model the 2C02's dynamic OAM losing un-refreshed sprite rows to a garbage pattern when rendering stays off (à la Mesen2). NTSC/Dendy only. Off is byte-identical to today's core.", Some("Modela cómo la OAM dinámica de la 2C02 pierde las filas de sprites no refrescadas y las convierte en un patrón basura cuando el renderizado permanece desactivado (como Mesen2). Solo NTSC/Dendy. Desactivado es idéntico byte a byte al núcleo actual.");
    SetFamicomConsoleHover => "Model the Famicom's reset wiring instead of the front-loading NES's: the PPU is never held in reset, so it is past its ~29,658-cycle warm-up when the game starts, and the Reset button reaches only the CPU. Some Famicom carts (the 999-in-1 multicart) need it. Takes full effect from the next power cycle or ROM load. Off is byte-identical to today's core.", Some("Modela el cableado de reinicio de la Famicom en lugar del de la NES de carga frontal: la PPU nunca se mantiene en reinicio, así que ya ha terminado su calentamiento de ~29.658 ciclos cuando empieza el juego, y el botón Reset solo llega a la CPU. Algunos cartuchos de Famicom (el multicartucho 999-in-1) lo necesitan. Surte pleno efecto desde el siguiente apagado y encendido o carga de ROM. Desactivado es idéntico byte a byte al núcleo actual.");
    SetFastDotPathHover =>"Run the specialized straight-line handler for undisturbed visible background dots. Emits the identical frame either way (verified bit-for-bit every frame) and is ~11% faster on rendering-heavy games. Leave on unless you are diagnosing a suspected PPU difference.", Some("Ejecuta el manejador especializado en línea recta para los puntos de fondo visibles sin alteraciones. Produce el mismo fotograma en ambos casos (verificado bit a bit en cada fotograma) y es ~11% más rápido en juegos con mucho renderizado. Déjalo activado salvo que estés diagnosticando una posible diferencia de la PPU.");
    SetEnhancementsNote => "Off-by-default enhancement modes. These are NEVER applied while accuracy tests / TAS replay / netplay run.", Some("Modos de mejora desactivados por defecto. NUNCA se aplican durante pruebas de precisión / reproducción de TAS / juego en red.");
    SetEnhSpriteInert => "Experimental: staged for the v2.0 core pass (currently inert).", Some("Experimental: preparado para la fase del núcleo v2.0 (actualmente sin efecto).");
    SetEnhOverclockNote => "Adds idle scanlines after the visible frame to reduce slowdown in some games. Changes timing, so it is ignored while recording or playing a movie and during netplay.", Some("Agrega líneas de barrido inactivas después del fotograma visible para reducir la ralentización en algunos juegos. Cambia la temporización, por lo que se ignora al grabar o reproducir una película y durante el juego en red.");

}

/// Keys whose Spanish entry is deliberately `None` (English fallback). The
/// coverage test asserts that every other key has a Spanish string, so a new
/// key cannot silently ship untranslated.
#[cfg(test)]
const SPANISH_FALLBACK_BY_DESIGN: &[Key] = &[Key::SettingsTabShaders, Key::SettingsTabAudio];

/// Resolve `key` in `locale`, falling back to English for any key the locale's
/// catalog has not translated. English itself always resolves directly.
#[must_use]
pub const fn tr_in(locale: Locale, key: Key) -> &'static str {
    match locale {
        Locale::English => english(key),
        Locale::Spanish => match spanish(key) {
            Some(s) => s,
            None => english(key),
        },
    }
}

/// Resolve `key` in the current process-global locale (see [`set_locale`]),
/// with English fallback. This is the primary entry point the UI calls.
#[must_use]
pub fn tr(key: Key) -> &'static str {
    tr_in(current_locale(), key)
}

/// Ergonomic wrapper around [`tr`]: `t!(MenuFile)` == `tr(Key::MenuFile)`.
///
/// Keeps call sites terse without importing the `Key` enum everywhere. Use
/// [`tr`] directly when a `Key` value is computed dynamically.
#[macro_export]
macro_rules! t {
    ($key:ident) => {
        $crate::i18n::tr($crate::i18n::Key::$key)
    };
}

/// Fill the `{0}`, `{1}`, ... placeholders of `template` with `args`.
///
/// This is the whole of the catalog's parameter support, and it is kept that
/// small on purpose (ADR 0023: fixed strings, no message-format engine). A
/// placeholder is `{` + decimal digits + `}`; the digits index `args`. Anything
/// else — a brace not followed by digits and `}`, or an index past the end of
/// `args` — is copied through verbatim, so a malformed template degrades to
/// visible text rather than a panic. Positional (not named) placeholders let a
/// translation reorder its arguments, which word order in another language
/// routinely needs.
fn fill(template: &str, args: &[&dyn fmt::Display]) -> String {
    use fmt::Write as _;
    let mut out = String::with_capacity(template.len() + 8 * args.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        let arg = (digits > 0 && after.as_bytes().get(digits) == Some(&b'}'))
            .then(|| after[..digits].parse::<usize>().ok())
            .flatten()
            .and_then(|i| args.get(i));
        if let Some(arg) = arg {
            // Writing to a `String` cannot fail.
            let _ = write!(out, "{arg}");
            rest = &after[digits + 1..];
        } else {
            out.push('{');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// Resolve `key` in `locale` (with English fallback) and fill its `{N}`
/// placeholders from `args`. See [`tr_fmt`].
#[must_use]
pub fn tr_fmt_in(locale: Locale, key: Key, args: &[&dyn fmt::Display]) -> String {
    fill(tr_in(locale, key), args)
}

/// Resolve a keyed *template* in the current locale and fill its `{0}`,
/// `{1}`, ... placeholders from `args`.
///
/// The keyed-template form of a UI `format!`: the English catalog entry is the
/// original format string with its `{}` holes numbered, so with the default
/// locale the rendered text is byte-identical to the old `format!` output. It
/// allocates one `String`, exactly as the `format!` it replaces did.
#[must_use]
pub fn tr_fmt(key: Key, args: &[&dyn fmt::Display]) -> String {
    tr_fmt_in(current_locale(), key, args)
}

/// Ergonomic wrapper around [`tr_fmt`]: `tf!(RomInfoBytes, n)` ==
/// `tr_fmt(Key::RomInfoBytes, &[&n])`. Every argument must implement
/// [`core::fmt::Display`].
#[macro_export]
macro_rules! tf {
    ($key:ident $(, $arg:expr)* $(,)?) => {
        $crate::i18n::tr_fmt(
            $crate::i18n::Key::$key,
            &[$(&$arg as &dyn ::core::fmt::Display),*],
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_is_default() {
        assert_eq!(Locale::default(), Locale::English);
    }

    #[test]
    fn default_locale_strings_are_verbatim() {
        // The English catalog must reproduce the pre-i18n literals byte-for-byte
        // so the shipped default UI is unchanged. Asserted via `tr_in` against
        // the explicit English locale (and `english` directly) so this test
        // never touches the process-global `CURRENT_LOCALE` — keeping it sound
        // under parallel test execution.
        assert_eq!(tr_in(Locale::English, Key::MenuFile), "File");
        assert_eq!(tr_in(Locale::English, Key::MenuEmulation), "Emulation");
        assert_eq!(tr_in(Locale::English, Key::MenuHelp), "Help");
        assert_eq!(tr_in(Locale::English, Key::SettingsTitle), "Settings");
        assert_eq!(
            tr_in(Locale::English, Key::SettingsHeadingDisplay),
            "Display"
        );
        assert_eq!(tr_in(Locale::English, Key::StatusRunning), "Running");
        assert_eq!(tr_in(Locale::English, Key::ButtonReset), "Reset");
        // The same strings via the per-locale catalog const fn.
        assert_eq!(english(Key::MenuFile), "File");
        assert_eq!(english(Key::ButtonReset), "Reset");
    }

    #[test]
    fn second_locale_translates() {
        assert_eq!(tr_in(Locale::Spanish, Key::MenuFile), "Archivo");
        assert_eq!(tr_in(Locale::Spanish, Key::StatusPaused), "Pausado");
        assert_eq!(tr_in(Locale::Spanish, Key::ButtonCancel), "Cancelar");
    }

    #[test]
    fn missing_key_falls_back_to_english() {
        // `Shaders`/`Audio` are intentionally untranslated in the Spanish
        // catalog (`spanish` returns `None`), so they MUST resolve to the
        // verbatim English value via the `None => english(key)` fallback arm.
        assert_eq!(spanish(Key::SettingsTabShaders), None);
        assert_eq!(spanish(Key::SettingsTabAudio), None);
        assert_eq!(tr_in(Locale::Spanish, Key::SettingsTabShaders), "Shaders");
        assert_eq!(tr_in(Locale::Spanish, Key::SettingsTabAudio), "Audio");
        assert_eq!(
            tr_in(Locale::Spanish, Key::SettingsTabShaders),
            english(Key::SettingsTabShaders),
        );
        // No resolved string is ever empty, in either locale, for any catalog
        // key — the fallback guarantees a value.
        for key in [
            Key::MenuFile,
            Key::SettingsTabShaders,
            Key::ButtonOk,
            Key::StatusIdle,
        ] {
            assert_ne!(tr_in(Locale::Spanish, key), "");
            assert_ne!(tr_in(Locale::English, key), "");
        }
    }

    #[test]
    fn locale_tag_round_trips() {
        for loc in Locale::all() {
            assert_eq!(Locale::from_u8(loc.as_u8()), loc);
        }
        // Unknown byte falls back to English (never panics).
        assert_eq!(Locale::from_u8(200), Locale::English);
    }

    #[test]
    fn macro_matches_tr() {
        // The `t!` macro expands to `tr(Key::..)`, which reads the process-
        // global locale. Validate the expansion against `tr_in(current_locale(),
        // ..)` — the exact value `tr` resolves to — WITHOUT calling `set_locale`,
        // so the test reads but never mutates the global and is sound under
        // parallel execution regardless of which locale happens to be active.
        let loc = current_locale();
        assert_eq!(t!(MenuFile), tr_in(loc, Key::MenuFile));
        assert_eq!(t!(ButtonReset), tr_in(loc, Key::ButtonReset));
    }

    /// The `{N}` placeholders a template uses, sorted and de-duplicated.
    fn placeholders(s: &str) -> Vec<usize> {
        let mut out = Vec::new();
        let mut rest = s;
        while let Some(open) = rest.find('{') {
            let after = &rest[open + 1..];
            let digits = after.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0 && after.as_bytes().get(digits) == Some(&b'}') {
                out.push(after[..digits].parse().unwrap());
            }
            rest = after;
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    #[test]
    fn every_key_has_a_non_empty_english_string() {
        for &key in Key::ALL {
            assert!(
                !english(key).is_empty(),
                "{key:?} has an empty English string"
            );
            assert_ne!(tr_in(Locale::English, key), "");
        }
    }

    #[test]
    fn all_lists_every_key_once() {
        // `Key::ALL` is generated from the same table as the enum; check it has
        // no duplicate (a copy-pasted row would still compile as a distinct
        // variant name only if renamed, so this guards the table's shape).
        for (i, a) in Key::ALL.iter().enumerate() {
            for b in &Key::ALL[i + 1..] {
                assert_ne!(a, b, "{a:?} listed twice in Key::ALL");
            }
        }
    }

    #[test]
    fn spanish_covers_every_user_facing_key() {
        // Coverage report: how many keys the (machine-drafted) Spanish catalog
        // translates. Run with `--nocapture` to see it.
        let total = Key::ALL.len();
        let translated = Key::ALL.iter().filter(|&&k| spanish(k).is_some()).count();
        println!(
            "i18n coverage: {translated}/{total} keys have a Spanish string \
             ({} fall back to English by design)",
            total - translated
        );
        // Every key used by the user-facing panels must be translated; the only
        // English fallbacks are the ones listed as deliberate.
        for &key in Key::ALL {
            if SPANISH_FALLBACK_BY_DESIGN.contains(&key) {
                assert_eq!(spanish(key), None, "{key:?} is listed as a fallback");
            } else {
                let es = spanish(key).unwrap_or_else(|| panic!("{key:?} has no Spanish string"));
                assert!(!es.is_empty(), "{key:?} has an empty Spanish string");
            }
        }
        assert_eq!(translated + SPANISH_FALLBACK_BY_DESIGN.len(), total);
    }

    #[test]
    fn spanish_uses_the_same_placeholders_as_english() {
        // A translation that drops or invents a `{N}` would silently lose an
        // argument (or print a literal `{N}`), so both sides must agree.
        for &key in Key::ALL {
            if let Some(es) = spanish(key) {
                assert_eq!(
                    placeholders(english(key)),
                    placeholders(es),
                    "{key:?}: placeholder mismatch between {:?} and {es:?}",
                    english(key)
                );
            }
        }
    }

    #[test]
    fn fill_substitutes_positional_placeholders() {
        assert_eq!(
            fill("{0} bytes ({1} KiB)", &[&2048, &2]),
            "2048 bytes (2 KiB)"
        );
        // Reordering: a translation may use the arguments in another order.
        assert_eq!(fill("{1}, {0}", &[&"a", &"b"]), "b, a");
        // Repeated placeholder.
        assert_eq!(fill("{0}{0}", &[&7]), "77");
        // No placeholders: the template is returned verbatim.
        assert_eq!(fill("plain", &[]), "plain");
    }

    #[test]
    fn fill_passes_malformed_placeholders_through() {
        // Out-of-range index, non-digit braces, and an unterminated brace are
        // copied verbatim rather than panicking.
        assert_eq!(fill("{3}", &[&1]), "{3}");
        assert_eq!(fill("{x} {}", &[&1]), "{x} {}");
        assert_eq!(fill("tail {0", &[&1]), "tail {0");
        assert_eq!(fill("{{0}", &[&1]), "{1");
        // Multi-byte text around a placeholder survives intact.
        assert_eq!(fill("año {0} ñ", &[&"x"]), "año x ñ");
    }

    #[test]
    fn tr_fmt_in_resolves_then_fills() {
        assert_eq!(
            tr_fmt_in(Locale::English, Key::MenuFile, &[&1]),
            "File",
            "a template with no placeholders ignores its arguments"
        );
        let loc = current_locale();
        assert_eq!(
            tf!(MenuFile),
            tr_fmt_in(loc, Key::MenuFile, &[]),
            "the tf! macro expands to tr_fmt in the current locale"
        );
    }
}
