//! Header parser shared between iNES 1.0 and NES 2.0 paths.
//!
//! Encoding rules follow `docs/cartridge-format.md` §Header layout.

use crate::cartridge::{ConsoleType, Mirroring, Region, RomError, VsPpuType};
use alloc::format;

/// Magic bytes of an iNES / NES 2.0 file: `"NES\x1A"`.
pub const MAGIC: [u8; 4] = [b'N', b'E', b'S', 0x1A];

/// Header length in bytes.
pub const HEADER_LEN: usize = 16;

/// 16 KiB PRG-ROM unit size.
pub const PRG_UNIT: usize = 16 * 1024;

/// 8 KiB CHR-ROM unit size.
pub const CHR_UNIT: usize = 8 * 1024;

/// 512-byte trainer block size (when present).
pub const TRAINER_LEN: usize = 512;

/// Vs. System hardware type: NES 2.0 header byte 13, high nibble, when the
/// console type (byte 7 bits 0-1) is 1.
///
/// Values per the Nesdev wiki's "NES 2.0" page, §"Vs. System Type". Types 0-4 are
/// the single-board Vs. `UniSystem` with its non-PPU copy-protection variant;
/// 5 and 6 are the two-CPU / two-PPU Vs. `DualSystem`. Values 7-15 are not
/// assigned and are kept, unaltered, in [`VsHardwareType::Reserved`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum VsHardwareType {
    /// `$0`: Vs. `UniSystem` (normal).
    UniSystem,
    /// `$1`: Vs. `UniSystem`, RBI Baseball protection.
    UniSystemRbiBaseball,
    /// `$2`: Vs. `UniSystem`, TKO Boxing protection.
    UniSystemTkoBoxing,
    /// `$3`: Vs. `UniSystem`, Super Xevious protection.
    UniSystemSuperXevious,
    /// `$4`: Vs. `UniSystem`, Vs. Ice Climber Japan protection.
    UniSystemIceClimberJapan,
    /// `$5`: Vs. `DualSystem` (normal).
    DualSystem,
    /// `$6`: Vs. `DualSystem`, Raid on Bungeling Bay protection.
    DualSystemRaidOnBungelingBay,
    /// `$7-$F`: unassigned. The value is the nibble itself (`7..=15`).
    Reserved(u8),
}

impl VsHardwareType {
    /// Decode the byte-13 high nibble (`nibble` is masked to four bits).
    #[must_use]
    pub const fn from_nibble(nibble: u8) -> Self {
        match nibble & 0x0F {
            0 => Self::UniSystem,
            1 => Self::UniSystemRbiBaseball,
            2 => Self::UniSystemTkoBoxing,
            3 => Self::UniSystemSuperXevious,
            4 => Self::UniSystemIceClimberJapan,
            5 => Self::DualSystem,
            6 => Self::DualSystemRaidOnBungelingBay,
            n => Self::Reserved(n),
        }
    }

    /// The byte-13 high nibble this type encodes to (`0..=15`). A
    /// [`Self::Reserved`] value outside `7..=15` is masked to four bits.
    #[must_use]
    pub const fn to_nibble(self) -> u8 {
        match self {
            Self::UniSystem => 0,
            Self::UniSystemRbiBaseball => 1,
            Self::UniSystemTkoBoxing => 2,
            Self::UniSystemSuperXevious => 3,
            Self::UniSystemIceClimberJapan => 4,
            Self::DualSystem => 5,
            Self::DualSystemRaidOnBungelingBay => 6,
            Self::Reserved(n) => n & 0x0F,
        }
    }

    /// True for the two Vs. `DualSystem` types (5 and 6).
    #[must_use]
    pub const fn is_dual_system(self) -> bool {
        matches!(self, Self::DualSystem | Self::DualSystemRaidOnBungelingBay)
    }
}

/// Extended console type: NES 2.0 header byte 13, low nibble, when the
/// console type (byte 7 bits 0-1) is 3.
///
/// Values per the Nesdev wiki's "NES 2.0" page, §"Extended Console Type". `$0-$2`
/// duplicate what byte 7 can already say and exist so a console-type variable
/// can fold both fields together; they are kept distinct here so a header that
/// uses them round-trips. `$D-$F` are reserved and kept in
/// [`ExtendedConsoleType::Reserved`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum ExtendedConsoleType {
    /// `$0`: regular NES / Famicom / Dendy.
    Regular,
    /// `$1`: Nintendo Vs. System.
    VsSystem,
    /// `$2`: PlayChoice-10.
    Playchoice10,
    /// `$3`: regular Famiclone with a CPU that supports decimal mode.
    DecimalModeFamiclone,
    /// `$4`: regular NES / Famicom with an EPSM module or plug-through cartridge.
    Epsm,
    /// `$5`: V.R. Technology VT01 with red/cyan STN palette.
    Vt01,
    /// `$6`: V.R. Technology VT02.
    Vt02,
    /// `$7`: V.R. Technology VT03.
    Vt03,
    /// `$8`: V.R. Technology VT09.
    Vt09,
    /// `$9`: V.R. Technology VT32.
    Vt32,
    /// `$A`: V.R. Technology VT369.
    Vt369,
    /// `$B`: UMC UM6578.
    Um6578,
    /// `$C`: Famicom Network System.
    FamicomNetworkSystem,
    /// `$D-$F`: reserved. The value is the nibble itself (`13..=15`).
    Reserved(u8),
}

impl ExtendedConsoleType {
    /// Decode the byte-13 low nibble (`nibble` is masked to four bits).
    #[must_use]
    pub const fn from_nibble(nibble: u8) -> Self {
        match nibble & 0x0F {
            0x0 => Self::Regular,
            0x1 => Self::VsSystem,
            0x2 => Self::Playchoice10,
            0x3 => Self::DecimalModeFamiclone,
            0x4 => Self::Epsm,
            0x5 => Self::Vt01,
            0x6 => Self::Vt02,
            0x7 => Self::Vt03,
            0x8 => Self::Vt09,
            0x9 => Self::Vt32,
            0xA => Self::Vt369,
            0xB => Self::Um6578,
            0xC => Self::FamicomNetworkSystem,
            n => Self::Reserved(n),
        }
    }

    /// The byte-13 low nibble this type encodes to (`0..=15`).
    #[must_use]
    pub const fn to_nibble(self) -> u8 {
        match self {
            Self::Regular => 0x0,
            Self::VsSystem => 0x1,
            Self::Playchoice10 => 0x2,
            Self::DecimalModeFamiclone => 0x3,
            Self::Epsm => 0x4,
            Self::Vt01 => 0x5,
            Self::Vt02 => 0x6,
            Self::Vt03 => 0x7,
            Self::Vt09 => 0x8,
            Self::Vt32 => 0x9,
            Self::Vt369 => 0xA,
            Self::Um6578 => 0xB,
            Self::FamicomNetworkSystem => 0xC,
            Self::Reserved(n) => n & 0x0F,
        }
    }
}

/// Generates [`ExpansionDevice`] and its two code conversions from one table,
/// so the variant list, the decoder and the encoder cannot drift apart.
macro_rules! expansion_devices {
    ($( $(#[$doc:meta])* $variant:ident = $code:literal, )*) => {
        /// Default expansion device: NES 2.0 header byte 15, bits 0-6.
        ///
        /// The device the game expects at CPU `$4016`/`$4017`, per the Nesdev
        /// wiki's "NES 2.0" page, §"Default Expansion Device" (codes `$00-$4F`). An
        /// unassigned code -- `$06` (withdrawn) or `$50-$7F` -- is kept,
        /// unaltered, in [`ExpansionDevice::Unassigned`]. The header only
        /// *describes* the device: nothing in the core selects an input device
        /// from it.
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
        #[non_exhaustive]
        pub enum ExpansionDevice {
            $( $(#[$doc])* $variant, )*
            /// A code the page does not assign (`$06`, `$50-$7F`).
            Unassigned(u8),
        }

        impl ExpansionDevice {
            /// Decode header byte 15 (bit 7 is not part of the field and is
            /// ignored).
            #[must_use]
            pub const fn from_code(code: u8) -> Self {
                match code & 0x7F {
                    $( $code => Self::$variant, )*
                    n => Self::Unassigned(n),
                }
            }

            /// The 7-bit byte-15 code this device encodes to.
            #[must_use]
            pub const fn code(self) -> u8 {
                match self {
                    $( Self::$variant => $code, )*
                    Self::Unassigned(n) => n & 0x7F,
                }
            }
        }
    };
}

expansion_devices! {
    /// `$00`: unspecified (no information).
    Unspecified = 0x00,
    /// `$01`: standard NES / Famicom controllers.
    StandardControllers = 0x01,
    /// `$02`: NES Four Score / Satellite with two more standard controllers.
    FourScore = 0x02,
    /// `$03`: Famicom Four Players Adapter, "simple" protocol.
    FamicomFourPlayersAdapter = 0x03,
    /// `$04`: Vs. System, 1P via `$4016`.
    VsSystem4016 = 0x04,
    /// `$05`: Vs. System, 1P via `$4017`.
    VsSystem4017 = 0x05,
    /// `$07`: Vs. Zapper.
    VsZapper = 0x07,
    /// `$08`: Zapper (`$4017`).
    Zapper4017 = 0x08,
    /// `$09`: two Zappers.
    TwoZappers = 0x09,
    /// `$0A`: Bandai Hyper Shot light gun.
    BandaiHyperShot = 0x0A,
    /// `$0B`: Power Pad side A.
    PowerPadSideA = 0x0B,
    /// `$0C`: Power Pad side B.
    PowerPadSideB = 0x0C,
    /// `$0D`: Family Trainer side A.
    FamilyTrainerSideA = 0x0D,
    /// `$0E`: Family Trainer side B.
    FamilyTrainerSideB = 0x0E,
    /// `$0F`: Arkanoid Vaus controller (NES).
    VausNes = 0x0F,
    /// `$10`: Arkanoid Vaus controller (Famicom).
    VausFamicom = 0x10,
    /// `$11`: two Vaus controllers plus Famicom Data Recorder.
    TwoVausPlusDataRecorder = 0x11,
    /// `$12`: Konami Hyper Shot controller.
    KonamiHyperShot = 0x12,
    /// `$13`: Coconuts Pachinko controller.
    CoconutsPachinko = 0x13,
    /// `$14`: Exciting Boxing punching bag.
    ExcitingBoxingPunchingBag = 0x14,
    /// `$15`: Jissen Mahjong controller.
    JissenMahjong = 0x15,
    /// `$16`: Yonezawa Party Tap.
    PartyTap = 0x16,
    /// `$17`: Oeka Kids tablet.
    OekaKidsTablet = 0x17,
    /// `$18`: Sunsoft Barcode Battler.
    BarcodeBattler = 0x18,
    /// `$19`: Miracle Piano keyboard.
    MiraclePiano = 0x19,
    /// `$1A`: Pokkun Moguraa tap-tap mat.
    PokkunMoguraa = 0x1A,
    /// `$1B`: Top Rider handlebars.
    TopRider = 0x1B,
    /// `$1C`: double-fisted (two controllers per player).
    DoubleFisted = 0x1C,
    /// `$1D`: Famicom 3D System.
    Famicom3dSystem = 0x1D,
    /// `$1E`: Doremikko keyboard.
    DoremikkoKeyboard = 0x1E,
    /// `$1F`: R.O.B. Gyromite.
    RobGyromite = 0x1F,
    /// `$20`: Famicom Data Recorder ("silent" keyboard).
    FamicomDataRecorder = 0x20,
    /// `$21`: ASCII Turbo File.
    AsciiTurboFile = 0x21,
    /// `$22`: IGS Storage Battle Box.
    IgsBattleBox = 0x22,
    /// `$23`: Family BASIC keyboard plus Famicom Data Recorder.
    FamilyBasicKeyboard = 0x23,
    /// `$24`: Dongda PEC keyboard.
    DongdaPecKeyboard = 0x24,
    /// `$25`: Bit Corp. Bit-79 keyboard.
    Bit79Keyboard = 0x25,
    /// `$26`: Subor keyboard.
    SuborKeyboard = 0x26,
    /// `$27`: Subor keyboard plus Macro Winners mouse.
    SuborKeyboardMacroWinnersMouse = 0x27,
    /// `$28`: Subor keyboard plus Subor mouse via `$4016`.
    SuborKeyboardSuborMouse4016 = 0x28,
    /// `$29`: SNES mouse (`$4016`).
    SnesMouse4016 = 0x29,
    /// `$2A`: multicart.
    Multicart = 0x2A,
    /// `$2B`: two SNES controllers replacing the two standard controllers.
    TwoSnesControllers = 0x2B,
    /// `$2C`: `RacerMate` bicycle.
    RacerMateBicycle = 0x2C,
    /// `$2D`: U-Force.
    UForce = 0x2D,
    /// `$2E`: R.O.B. Stack-Up.
    RobStackUp = 0x2E,
    /// `$2F`: City Patrolman light gun.
    CityPatrolmanLightgun = 0x2F,
    /// `$30`: Sharp C1 cassette interface.
    SharpC1CassetteInterface = 0x30,
    /// `$31`: standard controller with swapped Left-Right / Up-Down / B-A.
    SwappedStandardController = 0x31,
    /// `$32`: Excalibur Sudoku pad.
    ExcaliburSudokuPad = 0x32,
    /// `$33`: ABL Pinball.
    AblPinball = 0x33,
    /// `$34`: Golden Nugget Casino extra buttons.
    GoldenNuggetCasino = 0x34,
    /// `$35`: Keda keyboard.
    KedaKeyboard = 0x35,
    /// `$36`: Subor keyboard plus Subor mouse via `$4017`.
    SuborKeyboardSuborMouse4017 = 0x36,
    /// `$37`: port test controller.
    PortTestController = 0x37,
    /// `$38`: Bandai Multi Game Player gamepad buttons.
    BandaiMultiGamePlayer = 0x38,
    /// `$39`: Venom TV Dance Mat.
    VenomTvDanceMat = 0x39,
    /// `$3A`: LG TV remote control.
    LgTvRemote = 0x3A,
    /// `$3B`: Famicom Network Controller.
    FamicomNetworkController = 0x3B,
    /// `$3C`: King Fishing controller.
    KingFishing = 0x3C,
    /// `$3D`: Croaky Karaoke controller.
    CroakyKaraoke = 0x3D,
    /// `$3E`: Kingwon keyboard.
    KingwonKeyboard = 0x3E,
    /// `$3F`: Zecheng keyboard.
    ZechengKeyboard = 0x3F,
    /// `$40`: Subor keyboard plus L90-rotated PS/2 mouse in `$4017`.
    SuborKeyboardL90Ps2Mouse = 0x40,
    /// `$41`: PS/2 keyboard in the UM6578 PS/2 port, PS/2 mouse via `$4017`.
    Um6578Ps2KeyboardAndMouse = 0x41,
    /// `$42`: PS/2 mouse in the UM6578 PS/2 port.
    Um6578Ps2Mouse = 0x42,
    /// `$43`: Yuxing mouse via `$4016`.
    YuxingMouse = 0x43,
    /// `$44`: Subor keyboard plus Yuxing mouse in `$4016`.
    SuborKeyboardYuxingMouse = 0x44,
    /// `$45`: Gigggle TV Pump.
    GiggleTvPump = 0x45,
    /// `$46`: BBK keyboard plus R90-rotated PS/2 mouse in `$4017`.
    BbkKeyboardR90Ps2Mouse = 0x46,
    /// `$47`: Magical Cooking.
    MagicalCooking = 0x47,
    /// `$48`: SNES mouse (`$4017`).
    SnesMouse4017 = 0x48,
    /// `$49`: Zapper (`$4016`).
    Zapper4016 = 0x49,
    /// `$4A`: Arkanoid Vaus controller (prototype).
    VausPrototype = 0x4A,
    /// `$4B`: TV Mahjong Game controller.
    TvMahjongGame = 0x4B,
    /// `$4C`: Mahjong Gekitou Densetsu controller.
    MahjongGekitouDensetsu = 0x4C,
    /// `$4D`: Subor keyboard plus X-inverted PS/2 mouse in `$4017`.
    SuborKeyboardXInvertedPs2Mouse = 0x4D,
    /// `$4E`: IBM PC/XT keyboard.
    IbmPcXtKeyboard = 0x4E,
    /// `$4F`: Subor keyboard plus Mega Book mouse.
    SuborKeyboardMegaBookMouse = 0x4F,
}

/// Parsed header view, format-detected.
///
/// Every field a 16-byte iNES / NES 2.0 header defines is modelled (since
/// v2.9.8): what is left over are the reserved bits (byte 12 bits 2-7, byte 13
/// for console types 0 and 2, byte 13 bits 4-7 for console type 3, byte 14
/// bits 2-7, byte 15 bit 7) and, on an iNES 1.0 header, bytes 8-15, which are
/// not part of that format. [`serialize_header_preserving`] keeps all of those.
///
/// **iNES 1.0.** The NES 2.0-only fields hold a fixed value on an iNES 1.0
/// header, whatever its bytes 8-15 contain (old dumpers wrote signatures
/// there): `submapper` 0, `region` NTSC, `console_type` NES,
/// `vs_hardware_type` / `extended_console_type` `None`, `vs_ppu_type`
/// [`VsPpuType::None`], `misc_rom_count` 0, `default_expansion_device`
/// [`ExpansionDevice::Unspecified`], both NVRAM sizes 0, and the two RAM sizes
/// the nominal values described on each field.
///
/// `#[non_exhaustive]`: outside this crate, build one with [`Header::default`]
/// and field assignment (or [`parse_header`]), so a field added later is not
/// an API break.
///
/// The 5 boolean flags directly mirror the iNES / NES 2.0 wire format and so
/// are not refactorable into an enum without losing parser fidelity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
#[non_exhaustive]
pub struct Header {
    /// True if the file is NES 2.0 (header byte 7 bits 2-3 == `10`).
    pub is_nes2: bool,
    /// 12-bit mapper id (iNES 1.0 fills only the low 8 bits).
    pub mapper_id: u16,
    /// 4-bit submapper id (NES 2.0 only; 0 on iNES 1.0).
    pub submapper: u8,
    /// PRG-ROM size in bytes.
    pub prg_size: usize,
    /// CHR-ROM size in bytes (0 if cart uses CHR-RAM).
    pub chr_size: usize,
    /// Effective initial mirroring.
    pub mirroring: Mirroring,
    /// Region from NES 2.0 byte 12; defaults to NTSC for iNES 1.0.
    pub region: Region,
    /// Console type from NES 2.0 byte 7; always [`ConsoleType::Nes`] for iNES 1.0.
    pub console_type: ConsoleType,
    /// Vs. System PPU type from NES 2.0 byte 13 low nibble, valid only when
    /// `console_type == ConsoleType::VsSystem` (otherwise [`VsPpuType::None`]).
    /// Resolves to the output palette + 2C05 quirks via [`VsPpuType::ppu_palette`]
    /// / [`VsPpuType::is_2c05`]. The reserved nibbles (`$1`, `$6`, `$7`,
    /// `$C-$F`) decode as [`VsPpuType::Rp2C03`], so they do not survive a
    /// canonical re-encode; [`serialize_header_preserving`] keeps them.
    pub vs_ppu_type: VsPpuType,
    /// Vs. hardware type from NES 2.0 byte 13 high nibble: `Some` exactly when
    /// the header is NES 2.0 and `console_type == ConsoleType::VsSystem`.
    /// Types 5 and 6 are the Vs. `DualSystem` boards; see
    /// [`Header::is_vs_dual_system`].
    pub vs_hardware_type: Option<VsHardwareType>,
    /// Extended console type from NES 2.0 byte 13 low nibble: `Some` exactly
    /// when the header is NES 2.0 and `console_type == ConsoleType::Extended`.
    pub extended_console_type: Option<ExtendedConsoleType>,
    /// Number of miscellaneous ROMs present (NES 2.0 byte 14 bits 0-1, so
    /// `0..=3`; 0 on iNES 1.0). The miscellaneous ROM area itself is whatever
    /// follows CHR-ROM in the file.
    pub misc_rom_count: u8,
    /// Default expansion device (NES 2.0 byte 15 bits 0-6;
    /// [`ExpansionDevice::Unspecified`] on iNES 1.0).
    pub default_expansion_device: ExpansionDevice,
    /// Volatile PRG-RAM size in bytes: NES 2.0 byte 10 low nibble. iNES 1.0
    /// has no size field, so it reports a nominal 8 KiB.
    ///
    /// Until v2.9.8 this field held the volatile **and** non-volatile sizes
    /// summed; that total is now [`Header::prg_ram_window`].
    pub prg_ram_size: u32,
    /// Non-volatile (battery-backed) PRG-RAM / EEPROM size in bytes: NES 2.0
    /// byte 10 high nibble. 0 on iNES 1.0, which has no NVRAM split (whether
    /// its nominal RAM is battery-backed is [`Header::has_battery`]).
    pub prg_nvram_size: u32,
    /// Volatile CHR-RAM size in bytes: NES 2.0 byte 11 low nibble. iNES 1.0
    /// reports a nominal 8 KiB when there is no CHR-ROM, otherwise 0.
    pub chr_ram_size: u32,
    /// Non-volatile CHR-RAM size in bytes: NES 2.0 byte 11 high nibble. 0 on
    /// iNES 1.0. No board in this crate allocates CHR-NVRAM from it.
    pub chr_nvram_size: u32,
    /// True when battery-backed PRG-RAM is present (`header[6]` bit 1).
    pub has_battery: bool,
    /// True when a 512-byte trainer follows the header (`header[6]` bit 2).
    pub has_trainer: bool,
    /// True when bit 3 of `header[6]` forces four-screen mode.
    pub four_screen: bool,
}

impl Default for Header {
    /// An iNES 1.0 header for mapper 0 with no ROM, horizontal mirroring, and
    /// every other field at the value [`parse_header`] gives an iNES 1.0 file
    /// with all-zero bytes 4-15, so `parse_header(&canonical(default))` is
    /// the default again.
    fn default() -> Self {
        Self {
            is_nes2: false,
            mapper_id: 0,
            submapper: 0,
            prg_size: 0,
            chr_size: 0,
            mirroring: Mirroring::Horizontal,
            region: Region::Ntsc,
            console_type: ConsoleType::Nes,
            vs_ppu_type: VsPpuType::None,
            vs_hardware_type: None,
            extended_console_type: None,
            misc_rom_count: 0,
            default_expansion_device: ExpansionDevice::Unspecified,
            prg_ram_size: INES1_NOMINAL_PRG_RAM,
            prg_nvram_size: 0,
            chr_ram_size: INES1_NOMINAL_CHR_RAM,
            chr_nvram_size: 0,
            has_battery: false,
            has_trainer: false,
            four_screen: false,
        }
    }
}

impl Header {
    /// The whole PRG-RAM window a board allocates at `$6000-$7FFF`: the
    /// volatile and non-volatile sizes together. Some carts (*`StarTropics`* /
    /// MMC6) declare their save RAM only in the NVRAM nibble, so reading the
    /// volatile size alone would leave them with none.
    #[must_use]
    pub const fn prg_ram_window(&self) -> u32 {
        self.prg_ram_size.saturating_add(self.prg_nvram_size)
    }

    /// True when the header names a Vs. `DualSystem` board (Vs. hardware type
    /// 5 or 6). Drives DUAL-system *detection*; see `docs/cartridge-format.md`.
    #[must_use]
    pub const fn is_vs_dual_system(&self) -> bool {
        match self.vs_hardware_type {
            Some(t) => t.is_dual_system(),
            None => false,
        }
    }
}

/// The PRG-RAM size an iNES 1.0 header reports, having no field for it: 8 KiB,
/// so the common save-RAM mappers (MMC1, MMC3, MMC5) get a plausible window.
const INES1_NOMINAL_PRG_RAM: u32 = 8 * 1024;

/// The CHR-RAM size an iNES 1.0 header reports when it has no CHR-ROM.
const INES1_NOMINAL_CHR_RAM: u32 = 8 * 1024;

/// Assemble the mapper number from header bytes 6-8 (see the comment
/// inside for the iNES 1.0 dirty-tail rule).
fn mapper_number(h: &[u8; HEADER_LEN], is_nes2: bool) -> u16 {
    // Mapper assembly:
    //   bits 0..=3 from header[6] high nibble,
    //   bits 4..=7 from header[7] high nibble,
    //   bits 8..=11 from header[8] low nibble (NES 2.0 only).
    //
    // iNES 1.0 only: old ROM tools wrote signatures ("DiskDude!" and its
    // variants) into bytes 7-15, which the original iNES emulator ignored.
    // Byte 7's high nibble then reads as mapper bits 4-7 and adds 64 (for
    // 'D' = 0x44) to the mapper number. The NESdev "iNES" page gives the rule
    // applied here: if bytes 12-15 are not all zero and the header is not NES
    // 2.0, mask off the upper four bits of the mapper number. A clean iNES 1.0
    // header always has zeros there, so a well-formed dump of any mapper from
    // 16 to 255 is unaffected; NES 2.0 headers are exempt because bytes 12-15
    // carry real fields. Byte 7's low nibble is already ignored on the iNES
    // 1.0 path (console type and the NES 2.0 marker), so nothing else in the
    // tail is read.
    let ines1_dirty_tail = !is_nes2 && h[12..16].iter().any(|&b| b != 0);
    let mapper_low = u16::from((h[6] >> 4) & 0x0F);
    let mapper_mid = if ines1_dirty_tail {
        0
    } else {
        u16::from(h[7] & 0xF0)
    };
    if is_nes2 {
        let mapper_hi = u16::from(h[8] & 0x0F) << 8;
        mapper_low | mapper_mid | mapper_hi
    } else {
        mapper_low | mapper_mid
    }
}

/// Parse a 16-byte header into a [`Header`].
///
/// # Errors
///
/// Returns [`RomError::Truncated`] if `bytes` is < 16 bytes; [`RomError::BadMagic`]
/// if the magic does not match. Header-internal inconsistencies are returned as
/// [`RomError::InvalidConfig`].
pub fn parse_header(bytes: &[u8]) -> Result<Header, RomError> {
    if bytes.len() < HEADER_LEN {
        return Err(RomError::Truncated {
            needed: HEADER_LEN,
            got: bytes.len(),
        });
    }
    if bytes[0..4] != MAGIC {
        return Err(RomError::BadMagic);
    }

    let h: [u8; HEADER_LEN] = bytes[..HEADER_LEN].try_into().expect("checked length");
    let is_nes2 = (h[7] & 0x0C) == 0x08;

    let mapper_id = mapper_number(&h, is_nes2);
    let submapper: u8 = if is_nes2 { (h[8] >> 4) & 0x0F } else { 0 };

    // PRG / CHR sizing.
    let prg_size = if is_nes2 {
        decoded_size(h[4], u16::from(h[9] & 0x0F), PRG_UNIT)?
    } else {
        usize::from(h[4]) * PRG_UNIT
    };
    let chr_size = if is_nes2 {
        decoded_size(h[5], u16::from((h[9] >> 4) & 0x0F), CHR_UNIT)?
    } else {
        usize::from(h[5]) * CHR_UNIT
    };

    // Mirroring.
    let four_screen = (h[6] & 0x08) != 0;
    let mirroring = if four_screen {
        Mirroring::FourScreen
    } else if (h[6] & 0x01) != 0 {
        Mirroring::Vertical
    } else {
        Mirroring::Horizontal
    };

    // Region (NES 2.0 byte 12 bits 0-1).
    let region = if is_nes2 {
        match h[12] & 0x03 {
            0 => Region::Ntsc,
            1 => Region::Pal,
            2 => Region::Multi,
            3 => Region::Dendy,
            _ => unreachable!(),
        }
    } else {
        // iNES 1.0 has only the unreliable byte 9 bit 0; assume NTSC.
        Region::Ntsc
    };

    // Console type (NES 2.0 byte 7 bits 0-1).
    let console_type = if is_nes2 {
        match h[7] & 0x03 {
            0 => ConsoleType::Nes,
            1 => ConsoleType::VsSystem,
            2 => ConsoleType::Playchoice10,
            3 => ConsoleType::Extended,
            _ => unreachable!(),
        }
    } else {
        ConsoleType::Nes
    };

    // Vs. System PPU type (NES 2.0 byte 13 low nibble, only when console = Vs).
    let vs_ppu_type = if is_nes2 && console_type == ConsoleType::VsSystem {
        VsPpuType::from_byte13_low_nibble(h[13] & 0x0F)
    } else {
        VsPpuType::None
    };

    let tail = decode_tail(&h, is_nes2, console_type);

    // RAM sizes. NES 2.0 bytes 10-11: low nibble = volatile shift, high nibble
    // = non-volatile shift, each `64 << shift` bytes (0 = none). A board's
    // PRG-RAM window is the two together (`Header::prg_ram_window`).
    //
    // iNES 1.0 has no reliable PRG-RAM size. We report 8 KiB so the common
    // mappers that use save RAM (MMC1, MMC3, MMC5) get a plausible window
    // allocated; mappers that override this on construction may. It has no
    // NVRAM split, so both NVRAM sizes are 0.
    let (prg_ram_size, prg_nvram_size, chr_ram_size, chr_nvram_size) = if is_nes2 {
        (
            ram_size_from_shift(h[10] & 0x0F),
            ram_size_from_shift(h[10] >> 4),
            ram_size_from_shift(h[11] & 0x0F),
            ram_size_from_shift(h[11] >> 4),
        )
    } else {
        let chr_ram = if chr_size == 0 {
            INES1_NOMINAL_CHR_RAM
        } else {
            0
        };
        (INES1_NOMINAL_PRG_RAM, 0, chr_ram, 0)
    };

    // Battery / trainer.
    let has_battery = (h[6] & 0x02) != 0;
    let has_trainer = (h[6] & 0x04) != 0;

    Ok(Header {
        is_nes2,
        mapper_id,
        submapper,
        prg_size,
        chr_size,
        mirroring,
        region,
        console_type,
        vs_ppu_type,
        vs_hardware_type: tail.vs_hardware_type,
        extended_console_type: tail.extended_console_type,
        misc_rom_count: tail.misc_rom_count,
        default_expansion_device: tail.default_expansion_device,
        prg_ram_size,
        prg_nvram_size,
        chr_ram_size,
        chr_nvram_size,
        has_battery,
        has_trainer,
        four_screen,
    })
}

/// The NES 2.0 fields of bytes 13-15 that `vs_ppu_type` does not cover.
struct Tail {
    vs_hardware_type: Option<VsHardwareType>,
    extended_console_type: Option<ExtendedConsoleType>,
    misc_rom_count: u8,
    default_expansion_device: ExpansionDevice,
}

/// Decode bytes 13-15 (see [`Header`] for the fixed iNES 1.0 values).
fn decode_tail(h: &[u8; HEADER_LEN], is_nes2: bool, console_type: ConsoleType) -> Tail {
    if !is_nes2 {
        return Tail {
            vs_hardware_type: None,
            extended_console_type: None,
            misc_rom_count: 0,
            default_expansion_device: ExpansionDevice::Unspecified,
        };
    }
    Tail {
        // Byte 13 HIGH nibble for a Vs. System: types 5 and 6 are the Vs.
        // DualSystem boards (two CPUs / two PPUs).
        vs_hardware_type: (console_type == ConsoleType::VsSystem)
            .then(|| VsHardwareType::from_nibble(h[13] >> 4)),
        // Byte 13 LOW nibble for console type 3.
        extended_console_type: (console_type == ConsoleType::Extended)
            .then(|| ExtendedConsoleType::from_nibble(h[13])),
        // Byte 14 bits 0-1 and byte 15 bits 0-6.
        misc_rom_count: h[14] & 0x03,
        default_expansion_device: ExpansionDevice::from_code(h[15]),
    }
}

/// Standard / exponent-multiplier sizing per NES 2.0.
///
/// `lsb` is header byte 4 or 5; `msb_nibble` is the matching nibble of byte 9.
fn decoded_size(lsb: u8, msb_nibble: u16, unit: usize) -> Result<usize, RomError> {
    if msb_nibble == 0x0F {
        // Exponent-multiplier: lsb = EEEEEEMM.
        let exponent = u32::from(lsb >> 2);
        if exponent >= 32 {
            return Err(RomError::InvalidConfig(format!(
                "exponent-multiplier exponent {exponent} overflows usize"
            )));
        }
        let multiplier_code = lsb & 0x03;
        let multiplier = u64::from(multiplier_code) * 2 + 1;
        let bytes = (1u64
            .checked_shl(exponent)
            .ok_or_else(|| RomError::InvalidConfig("exponent shift overflow".into()))?)
        .checked_mul(multiplier)
        .ok_or_else(|| RomError::InvalidConfig("multiplier overflow".into()))?;
        usize::try_from(bytes).map_err(|_| {
            RomError::InvalidConfig("exponent-multiplier size exceeds usize::MAX".into())
        })
    } else {
        let count = (msb_nibble << 8) | u16::from(lsb);
        let bytes = usize::from(count)
            .checked_mul(unit)
            .ok_or_else(|| RomError::InvalidConfig("rom size overflow".into()))?;
        Ok(bytes)
    }
}

/// NES 2.0 RAM-shift encoding: 0 → 0 bytes, otherwise `64 << shift`.
const fn ram_size_from_shift(shift: u8) -> u32 {
    if shift == 0 { 0 } else { 64u32 << shift }
}

/// Write the edits in `h` over `original`, the 16 header bytes `h` was parsed
/// from, and return the result.
///
/// Only the bits of fields whose value differs from `parse_header(original)`
/// are rewritten; every other bit of `original` comes back unchanged. So an
/// unedited header round-trips byte for byte, whatever it holds --
/// exponent-notation sizes, reserved bits, reserved Vs. PPU nibbles, and the
/// junk some dumpers left in bytes 8-15 of iNES 1.0 headers. This is the
/// public header writer (since v2.9.8 the only one) and what the header editor
/// writes to disk.
///
/// An edited field is written in its canonical encoding: a size in the
/// standard notation when that can express it and in exponent-multiplier
/// notation otherwise, a RAM size as its `64 << shift` nibble. Toggling
/// `is_nes2` changes what bytes 7-15 mean, so that edit re-encodes the whole
/// header canonically, which writes every reserved bit as zero. So does an
/// `original` that does not parse. To build a header from nothing, pass the
/// 16 bytes of an empty iNES 1.0 header (`"NES\x1A"` and twelve zeros) as
/// `original`.
#[must_use]
pub fn serialize_header_preserving(h: &Header, original: &[u8; HEADER_LEN]) -> [u8; HEADER_LEN] {
    let Ok(base) = parse_header(original) else {
        return canonical_header(h);
    };
    if h.is_nes2 != base.is_nes2 {
        return canonical_header(h);
    }
    // Every byte the canonical encoding produces for the edited header; each
    // changed field copies only its own bits from here.
    let c = canonical_header(h);
    let mut out = *original;
    let mut take = |byte: usize, mask: u8| out[byte] = (out[byte] & !mask) | (c[byte] & mask);

    if h.mapper_id != base.mapper_id {
        take(6, 0xF0);
        take(7, 0xF0);
        if h.is_nes2 {
            take(8, 0x0F);
        }
    }
    if h.mirroring != base.mirroring {
        take(6, 0x01);
    }
    if h.has_battery != base.has_battery {
        take(6, 0x02);
    }
    if h.has_trainer != base.has_trainer {
        take(6, 0x04);
    }
    if h.four_screen != base.four_screen {
        take(6, 0x08);
    }
    if h.prg_size != base.prg_size {
        take(4, 0xFF);
        if h.is_nes2 {
            take(9, 0x0F);
        }
    }
    if h.chr_size != base.chr_size {
        take(5, 0xFF);
        if h.is_nes2 {
            take(9, 0xF0);
        }
    }
    // Bytes 7 (console bits) and 8-13 carry these fields in NES 2.0 only;
    // `parse_header` ignores them in iNES 1.0, and so does this.
    if h.is_nes2 {
        if h.submapper != base.submapper {
            take(8, 0xF0);
        }
        if h.prg_ram_size != base.prg_ram_size {
            take(10, 0x0F);
        }
        if h.prg_nvram_size != base.prg_nvram_size {
            take(10, 0xF0);
        }
        if h.chr_ram_size != base.chr_ram_size {
            take(11, 0x0F);
        }
        if h.chr_nvram_size != base.chr_nvram_size {
            take(11, 0xF0);
        }
        if h.region != base.region {
            take(12, 0x03);
        }
        if h.console_type != base.console_type {
            // Byte 13 means something else for each console type, so a
            // console change re-encodes it.
            take(7, 0x03);
            take(13, 0xFF);
        } else if h.console_type == ConsoleType::VsSystem {
            if h.vs_ppu_type != base.vs_ppu_type {
                take(13, 0x0F);
            }
            if h.vs_hardware_type != base.vs_hardware_type {
                take(13, 0xF0);
            }
        } else if h.console_type == ConsoleType::Extended
            && h.extended_console_type != base.extended_console_type
        {
            take(13, 0x0F);
        }
        if h.misc_rom_count != base.misc_rom_count {
            take(14, 0x03);
        }
        if h.default_expansion_device != base.default_expansion_device {
            take(15, 0x7F);
        }
    }
    out
}

/// Encode a [`Header`] from scratch: the edited fields of
/// [`serialize_header_preserving`], and its whole output when the format
/// changes or the original does not parse.
///
/// Every field `parse_header` reads is written, so for any header it produced,
/// `parse_header(&canonical_header(&h)) == Ok(h)`
/// (`canonical_encoding_round_trips_every_parsed_header`). What is not a field
/// is written as zero: the reserved bits, the exact reserved Vs. PPU nibble,
/// and all of bytes 8-15 on iNES 1.0. A field iNES 1.0 cannot express (a size
/// above 255 units, any NES 2.0-only field) is dropped there.
///
/// Private since v2.9.8; until then `serialize_header` exposed it, and it lost
/// the exponent size notation, the NVRAM nibbles, the Vs. hardware type, the
/// extended console type and bytes 14-15.
// Serialization performs nibble extraction by mask + cast; the truncation is
// the documented encoding (not a bug), so we allow the cast lints narrowly on
// this function.
#[allow(clippy::cast_possible_truncation)]
fn canonical_header(h: &Header) -> [u8; HEADER_LEN] {
    let mut out = [0u8; HEADER_LEN];
    out[0..4].copy_from_slice(&MAGIC);

    // Sizing.
    let (prg_lsb, prg_msb_nibble) = encode_size(h.prg_size, PRG_UNIT, h.is_nes2);
    let (chr_lsb, chr_msb_nibble) = encode_size(h.chr_size, CHR_UNIT, h.is_nes2);
    out[4] = prg_lsb;
    out[5] = chr_lsb;

    // Flags 6.
    let mut flags6 = ((h.mapper_id & 0x0F) as u8) << 4;
    if matches!(h.mirroring, Mirroring::Vertical) {
        flags6 |= 0x01;
    }
    if h.has_battery {
        flags6 |= 0x02;
    }
    if h.has_trainer {
        flags6 |= 0x04;
    }
    if h.four_screen {
        flags6 |= 0x08;
    }
    out[6] = flags6;

    // Flags 7.
    // Mapper bits 4-7 sit in byte 7's high nibble as-is (no shift).
    let mut flags7 = (h.mapper_id as u8) & 0xF0;
    if h.is_nes2 {
        flags7 |= 0x08;
        flags7 |= match h.console_type {
            ConsoleType::Nes => 0,
            ConsoleType::VsSystem => 1,
            ConsoleType::Playchoice10 => 2,
            ConsoleType::Extended => 3,
        };
    }
    out[7] = flags7;

    if h.is_nes2 {
        // Mapper hi nibble + submapper.
        out[8] = (((h.mapper_id >> 8) as u8) & 0x0F) | ((h.submapper & 0x0F) << 4);
        out[9] = (prg_msb_nibble & 0x0F) | ((chr_msb_nibble & 0x0F) << 4);
        out[10] = ram_shift_for(h.prg_ram_size) | (ram_shift_for(h.prg_nvram_size) << 4);
        out[11] = ram_shift_for(h.chr_ram_size) | (ram_shift_for(h.chr_nvram_size) << 4);
        out[12] = match h.region {
            Region::Ntsc => 0,
            Region::Pal => 1,
            Region::Multi => 2,
            Region::Dendy => 3,
        };
        // Byte 13 means something different per console type: the Vs. PPU
        // type (low nibble) and Vs. hardware type (high nibble) for a Vs.
        // System, the extended console type (low nibble) for console type 3,
        // nothing otherwise.
        out[13] = match h.console_type {
            ConsoleType::VsSystem => {
                let hw = h.vs_hardware_type.map_or(0, VsHardwareType::to_nibble);
                vs_ppu_type_to_nibble(h.vs_ppu_type) | (hw << 4)
            }
            ConsoleType::Extended => h
                .extended_console_type
                .map_or(0, ExtendedConsoleType::to_nibble),
            ConsoleType::Nes | ConsoleType::Playchoice10 => 0,
        };
        out[14] = h.misc_rom_count & 0x03;
        out[15] = h.default_expansion_device.code();
    }

    out
}

/// Encode a ROM size as (byte 4/5, byte-9 nibble).
///
/// NES 2.0 uses the standard notation (a 12-bit count of `unit`s, nibble
/// `$0-$E`) when it can express `bytes` exactly, and the exponent-multiplier
/// notation (nibble `$F`, `bytes = 2^E * (2*MM + 1)`, E < 64) otherwise, which
/// is the order the Nesdev page prescribes. A size neither can express (not a
/// whole number of units above `$EFF` units, with an odd factor above 7) falls
/// back to the standard notation, truncated. iNES 1.0 has only the 8-bit count.
// Truncating cast: count is masked to 8 / 4 bits before the cast.
#[allow(clippy::cast_possible_truncation)]
const fn encode_size(bytes: usize, unit: usize, is_nes2: bool) -> (u8, u8) {
    let count = bytes / unit;
    if !is_nes2 {
        return ((count & 0xFF) as u8, 0);
    }
    if bytes.is_multiple_of(unit) && count <= 0xEFF {
        return ((count & 0xFF) as u8, ((count >> 8) & 0x0F) as u8);
    }
    if bytes != 0 {
        // bytes = 2^E * odd; the odd factor must be 1, 3, 5 or 7.
        let exponent = bytes.trailing_zeros();
        let odd = bytes >> exponent;
        if odd <= 7 && exponent < 64 {
            let multiplier = ((odd - 1) / 2) as u8;
            return (((exponent as u8) << 2) | multiplier, 0x0F);
        }
    }
    ((count & 0xFF) as u8, ((count >> 8) & 0x0F) as u8)
}

/// Encode a [`VsPpuType`] back to its NES 2.0 byte-13 low nibble.
const fn vs_ppu_type_to_nibble(t: VsPpuType) -> u8 {
    match t {
        VsPpuType::None | VsPpuType::Rp2C03 => 0x0,
        VsPpuType::Rp2C04_0001 => 0x2,
        VsPpuType::Rp2C04_0002 => 0x3,
        VsPpuType::Rp2C04_0003 => 0x4,
        VsPpuType::Rp2C04_0004 => 0x5,
        VsPpuType::Rc2C05_01 => 0x8,
        VsPpuType::Rc2C05_02 => 0x9,
        VsPpuType::Rc2C05_03 => 0xA,
        VsPpuType::Rc2C05_04 => 0xB,
    }
}

const fn ram_shift_for(size: u32) -> u8 {
    if size == 0 {
        return 0;
    }
    // shift = log2(size / 64).
    let mut shift = 0u8;
    let mut v = size / 64;
    while v > 1 {
        v >>= 1;
        shift += 1;
    }
    shift & 0x0F
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ines_header(prg_16k_units: u8, chr_8k_units: u8, mapper: u8, flags6: u8) -> [u8; 16] {
        let mut h = [0u8; 16];
        h[..4].copy_from_slice(&MAGIC);
        h[4] = prg_16k_units;
        h[5] = chr_8k_units;
        h[6] = (mapper << 4) | (flags6 & 0x0F);
        h[7] = mapper & 0xF0;
        h
    }

    /// A small deterministic PRNG (xorshift64) for header sweeps, so the
    /// sampled headers are the same on every run.
    fn next(state: &mut u64) -> u8 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        (*state >> 24).to_le_bytes()[0]
    }

    fn random_header(state: &mut u64) -> [u8; 16] {
        let mut h = [0u8; 16];
        h[..4].copy_from_slice(&MAGIC);
        for b in &mut h[4..] {
            *b = next(state);
        }
        h
    }

    #[test]
    fn preserving_round_trip_is_byte_identical_for_every_parsable_header() {
        // The header editor writes `serialize_header_preserving` output over
        // the ROM file, so an unedited header must come back exactly -- every
        // bit, modelled or not. Bytes 7 and 13 (format, console type, Vs.
        // PPU / hardware type, extended console type) are swept exhaustively
        // against sampled values of the rest; then 200,000 fully random
        // headers cover exponent sizes, NVRAM nibbles, reserved bits and iNES
        // 1.0 junk in bytes 8-15.
        let mut state = 0x9E37_79B9_7F4A_7C15;
        let mut checked = 0u32;
        let mut check = |h: &[u8; 16]| {
            if let Ok(parsed) = parse_header(h) {
                assert_eq!(&serialize_header_preserving(&parsed, h), h, "{h:02x?}");
                checked += 1;
            }
        };
        for b7 in 0..=255u8 {
            for b13 in 0..=255u8 {
                let mut h = random_header(&mut state);
                h[7] = b7;
                h[13] = b13;
                check(&h);
            }
        }
        for _ in 0..200_000 {
            check(&random_header(&mut state));
        }
        // Most random headers parse; a sweep that silently checked nothing
        // would pass, so say how much it covered.
        assert!(checked > 200_000, "only {checked} headers parsed");
    }

    #[test]
    fn canonical_encoding_round_trips_every_parsed_header() {
        // parse(canonical(h)) == h for every header `parse_header` can
        // produce: the canonical encoder must write back every field the
        // parser reads. Until v2.9.8 it lost exponent-notation sizes, the
        // NVRAM nibbles, the Vs. hardware type, the extended console type
        // and bytes 14-15.
        let mut state = 0x2545_F491_4F6C_DD1D;
        let mut checked = 0u32;
        let mut check = |h: &[u8; 16]| {
            if let Ok(parsed) = parse_header(h) {
                let again = parse_header(&canonical_header(&parsed))
                    .unwrap_or_else(|e| panic!("{h:02x?}: canonical output did not parse: {e:?}"));
                assert_eq!(again, parsed, "{h:02x?}");
                checked += 1;
            }
        };
        for b7 in 0..=255u8 {
            for b13 in 0..=255u8 {
                let mut h = random_header(&mut state);
                h[7] = b7;
                h[13] = b13;
                check(&h);
            }
        }
        for b9 in [0x00, 0x0F, 0xF0, 0xFF, 0x3E, 0xE3] {
            for b4 in 0..=255u8 {
                let mut h = random_header(&mut state);
                h[7] = (h[7] & !0x0C) | 0x08;
                h[9] = b9;
                h[4] = b4;
                h[5] = b4.rotate_left(3);
                check(&h);
            }
        }
        for _ in 0..200_000 {
            check(&random_header(&mut state));
        }
        assert!(checked > 200_000, "only {checked} headers parsed");
    }

    #[test]
    // Field assignment after `Default` is deliberate: it is the construction
    // pattern `#[non_exhaustive]` leaves other crates, so the test uses it.
    #[allow(clippy::field_reassign_with_default)]
    fn canonical_encoding_round_trips_constructed_nes2_headers() {
        // The same property over headers built with `Default` + field
        // assignment (the only way outside this crate, `Header` being
        // `#[non_exhaustive]`), sweeping every value of each byte 10-15
        // field in turn.
        let mut base = Header::default();
        base.is_nes2 = true;
        base.mapper_id = 0x123;
        base.submapper = 7;
        base.prg_size = 5 << 20; // 5 MiB: exponent notation
        base.chr_size = 0xEFF * CHR_UNIT; // the largest standard count
        base.region = Region::Multi;
        base.prg_ram_size = 0;
        base.chr_ram_size = 0;
        let mut headers = alloc::vec::Vec::new();
        for shift in 0..16u8 {
            let size = ram_size_from_shift(shift);
            for which in 0..4 {
                let mut h = base;
                match which {
                    0 => h.prg_ram_size = size,
                    1 => h.prg_nvram_size = size,
                    2 => h.chr_ram_size = size,
                    _ => h.chr_nvram_size = size,
                }
                headers.push(h);
            }
        }
        for nibble in 0..16u8 {
            let mut h = base;
            h.console_type = ConsoleType::VsSystem;
            h.vs_ppu_type = VsPpuType::Rc2C05_03;
            h.vs_hardware_type = Some(VsHardwareType::from_nibble(nibble));
            headers.push(h);
            let mut h = base;
            h.console_type = ConsoleType::Extended;
            h.extended_console_type = Some(ExtendedConsoleType::from_nibble(nibble));
            headers.push(h);
        }
        for count in 0..4u8 {
            let mut h = base;
            h.misc_rom_count = count;
            headers.push(h);
        }
        for code in 0..=0x7Fu8 {
            let mut h = base;
            h.default_expansion_device = ExpansionDevice::from_code(code);
            headers.push(h);
        }
        for h in headers {
            assert_eq!(parse_header(&canonical_header(&h)).unwrap(), h);
        }
        // And the default round-trips as the iNES 1.0 header it describes.
        let d = Header::default();
        assert_eq!(parse_header(&canonical_header(&d)).unwrap(), d);
    }

    #[test]
    fn enum_codes_round_trip() {
        for n in 0..16u8 {
            assert_eq!(VsHardwareType::from_nibble(n).to_nibble(), n);
            assert_eq!(ExtendedConsoleType::from_nibble(n).to_nibble(), n);
        }
        for code in 0..=0x7Fu8 {
            assert_eq!(ExpansionDevice::from_code(code).code(), code);
        }
        // Spot checks against the NESdev table.
        assert_eq!(
            ExpansionDevice::from_code(0x06),
            ExpansionDevice::Unassigned(0x06)
        );
        assert_eq!(
            ExpansionDevice::from_code(0x08),
            ExpansionDevice::Zapper4017
        );
        assert_eq!(
            ExpansionDevice::from_code(0x4F),
            ExpansionDevice::SuborKeyboardMegaBookMouse
        );
        assert_eq!(
            ExpansionDevice::from_code(0x50),
            ExpansionDevice::Unassigned(0x50)
        );
        // Bit 7 is not part of the field.
        assert_eq!(
            ExpansionDevice::from_code(0x88),
            ExpansionDevice::Zapper4017
        );
        assert_eq!(
            ExtendedConsoleType::from_nibble(0xC),
            ExtendedConsoleType::FamicomNetworkSystem
        );
        assert!(VsHardwareType::from_nibble(6).is_dual_system());
        assert!(!VsHardwareType::from_nibble(7).is_dual_system());
    }

    #[test]
    fn ines1_reports_fixed_values_for_the_nes2_fields() {
        // iNES 1.0 bytes 8-15 are not part of the format; a dumper's
        // signature there must not turn into a Vs. type or an expansion
        // device. The decision is documented on `Header`.
        let mut h = ines_header(2, 0, 1, 0x02);
        h[7] |= 0x01; // a Vs. bit iNES 1.0 parsing ignores
        h[8..16].copy_from_slice(b"Dump3r!!");
        let p = parse_header(&h).unwrap();
        assert_eq!(p.vs_hardware_type, None);
        assert_eq!(p.extended_console_type, None);
        assert_eq!(p.misc_rom_count, 0);
        assert_eq!(p.default_expansion_device, ExpansionDevice::Unspecified);
        assert_eq!((p.prg_ram_size, p.prg_nvram_size), (8 * 1024, 0));
        assert_eq!((p.chr_ram_size, p.chr_nvram_size), (8 * 1024, 0));
        assert_eq!(p.prg_ram_window(), 8 * 1024);
    }

    #[test]
    fn prg_ram_window_is_volatile_plus_nvram() {
        // The window every board allocates is what `prg_ram_size` alone held
        // before v2.9.8 split it, so ROM loading is unchanged. StarTropics
        // (MMC6) declares its save RAM only in the NVRAM nibble.
        let mut h = ines_header(8, 16, 4, 0x02);
        h[7] = 0x08;
        h[10] = 0x70; // 8 KiB PRG-NVRAM, no volatile PRG-RAM
        let p = parse_header(&h).unwrap();
        assert_eq!((p.prg_ram_size, p.prg_nvram_size), (0, 8 * 1024));
        assert_eq!(p.prg_ram_window(), 8 * 1024);
        h[10] = 0x75; // 2 KiB volatile + 8 KiB NVRAM
        let p = parse_header(&h).unwrap();
        assert_eq!(p.prg_ram_window(), 10 * 1024);
        h[11] = 0x57; // 8 KiB CHR-RAM, 2 KiB CHR-NVRAM
        let p = parse_header(&h).unwrap();
        assert_eq!((p.chr_ram_size, p.chr_nvram_size), (8 * 1024, 2 * 1024));
    }

    #[test]
    fn preserving_keeps_byte_13_high_nibble_and_bytes_14_15() {
        // Before v2.9.3 the editor wrote the canonical encoding, which
        // collapsed the Vs. hardware type (byte 13 high nibble) to 5 or 0 --
        // losing UniSystem protection types 1-4 and the 5/6 distinction --
        // and always wrote bytes 14-15 as zero.
        let mut h = ines_header(2, 1, 0, 0);
        h[7] = 0x08 | 0x01; // NES 2.0, Vs. System
        h[14] = 0x02; // two miscellaneous ROMs
        h[15] = 0x2A; // a default expansion device
        for hw in 0..16u8 {
            h[13] = (hw << 4) | 0x2; // PPU type 2 (RP2C04-0001)
            let mut p = parse_header(&h).unwrap();
            p.has_battery = true; // an unrelated edit
            let out = serialize_header_preserving(&p, &h);
            assert_eq!(out[13], h[13], "Vs. hardware type {hw}");
            assert_eq!(out[14..], h[14..], "bytes 14-15, hw {hw}");
            assert_eq!(out[6], h[6] | 0x02);
        }
    }

    #[test]
    fn preserving_keeps_the_extended_console_type() {
        // Console type 3 (Extended): byte 13's LOW nibble is the extended
        // console type (VT01-VT32, EPSM, ...), which `Header` does not model
        // (CodeRabbit on #571).
        let mut h = ines_header(2, 1, 0, 0);
        h[7] = 0x08 | 0x03;
        for ext in 0..16u8 {
            h[13] = ext;
            let mut p = parse_header(&h).unwrap();
            p.mapper_id = 4;
            let out = serialize_header_preserving(&p, &h);
            assert_eq!(out[13], ext, "extended console type {ext}");
            assert_eq!(parse_header(&out).unwrap().mapper_id, 4);
        }
    }

    #[test]
    fn editing_the_vs_hardware_type_rewrites_only_its_nibble() {
        let mut h = ines_header(2, 1, 0, 0);
        h[7] = 0x08 | 0x01;
        h[13] = 0x32; // UniSystem with protection type 3, PPU type 2
        let mut p = parse_header(&h).unwrap();
        assert_eq!(
            p.vs_hardware_type,
            Some(VsHardwareType::UniSystemSuperXevious)
        );
        assert!(!p.is_vs_dual_system());
        p.vs_hardware_type = Some(VsHardwareType::DualSystem);
        assert_eq!(serialize_header_preserving(&p, &h)[13], 0x52);
        // Type 6 is kept as 6: until v2.9.8 the header carried only a
        // DualSystem flag, and a canonical encode wrote every dual board as 5.
        h[13] = 0x62;
        let mut p = parse_header(&h).unwrap();
        assert!(p.is_vs_dual_system());
        assert_eq!(canonical_header(&p)[13], 0x62);
        p.vs_hardware_type = Some(VsHardwareType::UniSystem);
        assert_eq!(serialize_header_preserving(&p, &h)[13], 0x02);
        // Reserved types 7-15 keep their value too.
        h[13] = 0xB2;
        let p = parse_header(&h).unwrap();
        assert_eq!(p.vs_hardware_type, Some(VsHardwareType::Reserved(0xB)));
        assert_eq!(canonical_header(&p)[13], 0xB2);
    }

    #[test]
    fn editing_the_extended_console_type_rewrites_only_its_nibble() {
        let mut h = ines_header(2, 1, 0, 0);
        h[7] = 0x08 | 0x03; // NES 2.0, Extended
        h[13] = 0xF7; // reserved high nibble, VT03
        let mut p = parse_header(&h).unwrap();
        assert_eq!(p.extended_console_type, Some(ExtendedConsoleType::Vt03));
        assert_eq!(p.vs_hardware_type, None);
        p.extended_console_type = Some(ExtendedConsoleType::Um6578);
        assert_eq!(serialize_header_preserving(&p, &h)[13], 0xFB);
        assert_eq!(canonical_header(&p)[13], 0x0B);
    }

    /// Every bit that differs between `a` and `b`, as a 16-byte mask.
    fn changed(a: &[u8; 16], b: &[u8; 16]) -> [u8; 16] {
        core::array::from_fn(|i| a[i] ^ b[i])
    }

    #[test]
    fn each_edit_rewrites_only_its_own_bits() {
        // A NES 2.0 Vs. System header with every unmodelled bit set, so a
        // stray write anywhere shows up in the changed-bit mask.
        let mut h = [0xFFu8; 16];
        h[..4].copy_from_slice(&MAGIC);
        h[4] = 0x02; // 2 x 16 KiB PRG
        h[5] = 0x01; // 1 x 8 KiB CHR
        h[6] = 0x10; // mapper 1, horizontal
        h[7] = 0xF9; // mapper 0xF1 high nibble, NES 2.0, Vs. System
        h[8] = 0x30; // submapper 3
        h[9] = 0x00; // standard size notation
        h[10] = 0x77; // 8 KiB volatile + 8 KiB NV PRG-RAM
        h[11] = 0x77; // 8 KiB CHR-RAM, 8 KiB CHR-NVRAM
        h[12] = 0xFD; // PAL, reserved bits set
        h[13] = 0x31; // hardware type 3, PPU type 1 (reserved)
        h[14] = 0xFF; // 3 miscellaneous ROMs, reserved bits set
        h[15] = 0xFF; // unassigned device $7F, bit 7 set
        let base = parse_header(&h).unwrap();

        #[allow(clippy::type_complexity)]
        let edits: [(&str, fn(&mut Header), [u8; 16]); 16] = [
            (
                "mapper",
                |p| p.mapper_id = 0x2A4,
                mask(&[(6, 0xF0), (7, 0xF0), (8, 0x0F)]),
            ),
            (
                "mirroring",
                |p| p.mirroring = Mirroring::Vertical,
                mask(&[(6, 0x01)]),
            ),
            ("battery", |p| p.has_battery = true, mask(&[(6, 0x02)])),
            ("trainer", |p| p.has_trainer = true, mask(&[(6, 0x04)])),
            (
                "prg",
                |p| p.prg_size = 0x123 * PRG_UNIT,
                mask(&[(4, 0xFF), (9, 0x0F)]),
            ),
            (
                "chr",
                |p| p.chr_size = 0x201 * CHR_UNIT,
                mask(&[(5, 0xFF), (9, 0xF0)]),
            ),
            ("submapper", |p| p.submapper = 9, mask(&[(8, 0xF0)])),
            ("prg-ram", |p| p.prg_ram_size = 2048, mask(&[(10, 0x0F)])),
            (
                "prg-nvram",
                |p| p.prg_nvram_size = 1024,
                mask(&[(10, 0xF0)]),
            ),
            ("chr-ram", |p| p.chr_ram_size = 4096, mask(&[(11, 0x0F)])),
            ("chr-nvram", |p| p.chr_nvram_size = 0, mask(&[(11, 0xF0)])),
            ("region", |p| p.region = Region::Dendy, mask(&[(12, 0x03)])),
            (
                "vs-hardware",
                |p| p.vs_hardware_type = Some(VsHardwareType::DualSystem),
                mask(&[(13, 0xF0)]),
            ),
            ("misc-roms", |p| p.misc_rom_count = 1, mask(&[(14, 0x03)])),
            (
                "expansion",
                |p| p.default_expansion_device = ExpansionDevice::Zapper4017,
                mask(&[(15, 0x7F)]),
            ),
            (
                "prg-exponent",
                |p| p.prg_size = 3 << 12, // 12 KiB: 2^12 * 3, exponent notation only
                mask(&[(4, 0xFF), (9, 0x0F)]),
            ),
        ];
        for (name, edit, allowed) in edits {
            let mut p = base;
            edit(&mut p);
            let out = serialize_header_preserving(&p, &h);
            let diff = changed(&h, &out);
            for i in 0..16 {
                assert_eq!(diff[i] & !allowed[i], 0, "{name}: byte {i} {out:02x?}");
            }
            assert_ne!(diff, [0; 16], "{name}: the edit was not written");
            // And the edit reads back -- every field, not only the edited one.
            let back = parse_header(&out).unwrap();
            assert_eq!(serialize_header_preserving(&p, &out), out, "{name}");
            assert_eq!(back, p, "{name}");
        }
    }

    fn mask(bits: &[(usize, u8)]) -> [u8; 16] {
        let mut m = [0u8; 16];
        for &(i, b) in bits {
            m[i] |= b;
        }
        m
    }

    #[test]
    fn preserving_leaves_nes2_only_fields_alone_on_ines() {
        // iNES 1.0 bytes 8-15 are not part of the format and often hold a
        // dumper's signature; edits to NES 2.0-only fields must not touch them.
        let mut h = ines_header(2, 1, 1, 0);
        h[7] |= 0x01; // a Vs. bit iNES 1.0 parsing ignores
        h[8..].copy_from_slice(b"DiskDude");
        let mut p = parse_header(&h).unwrap();
        p.region = Region::Pal;
        p.submapper = 5;
        p.console_type = ConsoleType::Playchoice10;
        assert_eq!(serialize_header_preserving(&p, &h), h);
        p.has_battery = true;
        let out = serialize_header_preserving(&p, &h);
        assert_eq!(changed(&h, &out), mask(&[(6, 0x02)]));
    }

    #[test]
    fn a_format_change_or_unparsable_original_encodes_canonically() {
        let h = ines_header(2, 1, 1, 0);
        let mut p = parse_header(&h).unwrap();
        p.is_nes2 = true;
        assert_eq!(serialize_header_preserving(&p, &h), canonical_header(&p));
        let mut bad = h;
        bad[0] = b'X';
        let p = parse_header(&h).unwrap();
        assert_eq!(serialize_header_preserving(&p, &bad), canonical_header(&p));
    }

    #[test]
    fn canonical_encoding_round_trips_every_mapper_id() {
        // Byte 7's high nibble is mapper bits 4-7. Until v2.9.3 the encoder
        // wrote bits 8-11 there (`(mapper >> 4) & 0xF0`), so every mapper
        // from 16 up came back wrong -- mapper 66 (GxROM) as 2 -- and the
        // header editor wrote that to disk. Found by
        // `each_edit_rewrites_only_its_own_bits`.
        for is_nes2 in [false, true] {
            let top = if is_nes2 { 4095 } else { 255 };
            for id in 0..=top {
                let mut h = ines_header(2, 1, 0, 0);
                if is_nes2 {
                    h[7] = 0x08;
                }
                let mut p = parse_header(&h).unwrap();
                p.mapper_id = id;
                let back = parse_header(&canonical_header(&p)).unwrap();
                assert_eq!(back.mapper_id, id, "nes2={is_nes2}");
            }
        }
    }

    #[test]
    fn a_console_change_re_encodes_byte_13() {
        let mut h = ines_header(2, 1, 0, 0);
        h[7] = 0x08 | 0x03; // Extended
        h[13] = 0x0B; // an extended console type
        let mut p = parse_header(&h).unwrap();
        p.console_type = ConsoleType::Nes;
        let out = serialize_header_preserving(&p, &h);
        assert_eq!((out[7] & 0x03, out[13]), (0, 0));
    }

    #[test]
    fn rejects_bad_magic() {
        let mut h = ines_header(2, 1, 0, 0);
        h[0] = b'X';
        assert!(matches!(parse_header(&h), Err(RomError::BadMagic)));
    }

    #[test]
    fn truncated_header() {
        let bytes = [b'N', b'E', b'S'];
        assert!(matches!(
            parse_header(&bytes),
            Err(RomError::Truncated { needed: 16, got: 3 })
        ));
    }

    #[test]
    fn ines_basic_nrom_horizontal() {
        let h = ines_header(2, 1, 0, 0); // 32K PRG, 8K CHR, mapper 0, horizontal
        let p = parse_header(&h).unwrap();
        assert!(!p.is_nes2);
        assert_eq!(p.mapper_id, 0);
        assert_eq!(p.prg_size, 32 * 1024);
        assert_eq!(p.chr_size, 8 * 1024);
        assert_eq!(p.mirroring, Mirroring::Horizontal);
        assert!(!p.has_battery);
        assert!(!p.has_trainer);
        assert_eq!(p.region, Region::Ntsc);
    }

    #[test]
    fn ines_mapper_assembly() {
        // Mapper 1 (MMC1): low nibble 1, high nibble 0.
        let h = ines_header(1, 0, 1, 0);
        assert_eq!(parse_header(&h).unwrap().mapper_id, 1);
        // Mapper 4 (MMC3): low nibble 4, high nibble 0.
        let h = ines_header(1, 0, 4, 0);
        assert_eq!(parse_header(&h).unwrap().mapper_id, 4);
        // Mapper 0xCD: low nibble D, high nibble C.
        let h = ines_header(1, 0, 0xCD, 0);
        assert_eq!(parse_header(&h).unwrap().mapper_id, 0xCD);
    }

    #[test]
    fn nes2_vs_dualsystem_detected_from_byte13_high_nibble() {
        // NES 2.0 (h[7] bits 2-3 = 0b10) + Vs. System console (bits 0-1 = 01).
        let mut h = ines_header(2, 1, 0, 0);
        h[7] = 0x08 | 0x01;
        // byte 13: high nibble = Vs. hardware type, low nibble = Vs. PPU type.
        h[13] = 0x50; // hardware type 5 (DualSystem), 2C03 PPU
        let p = parse_header(&h).unwrap();
        assert!(p.is_nes2);
        assert_eq!(p.console_type, ConsoleType::VsSystem);
        assert!(p.is_vs_dual_system());
        // Hardware type 6 is also a DualSystem board.
        h[13] = 0x60;
        assert!(parse_header(&h).unwrap().is_vs_dual_system());
        // A normal Vs. UniSystem (hardware type 0-4) is NOT dual.
        h[13] = 0x00;
        assert!(!parse_header(&h).unwrap().is_vs_dual_system());
        h[13] = 0x40;
        assert!(!parse_header(&h).unwrap().is_vs_dual_system());
        // A non-Vs. NES 2.0 cart is never dual, even with a stray high nibble.
        h[7] = 0x08; // NES 2.0, console type = Nes
        h[13] = 0x50;
        assert!(!parse_header(&h).unwrap().is_vs_dual_system());
        // An iNES-1.0 cart (no NES 2.0 flag) is never dual.
        let mut h1 = ines_header(2, 1, 0, 0);
        h1[13] = 0x50;
        assert!(!parse_header(&h1).unwrap().is_vs_dual_system());
    }

    #[test]
    fn vs_dualsystem_round_trips_through_serialize() {
        let mut h = ines_header(2, 1, 0, 0);
        h[7] = 0x08 | 0x01; // NES 2.0 + Vs. System
        h[13] = 0x60; // DualSystem hardware type 6 + 2C03 PPU
        let parsed = parse_header(&h).unwrap();
        assert!(parsed.is_vs_dual_system());
        let out = canonical_header(&parsed);
        let reparsed = parse_header(&out).unwrap();
        // The bool round-trips (type 6 re-encodes as type 5, both DualSystem).
        assert!(reparsed.is_vs_dual_system());
        assert_eq!(reparsed.console_type, ConsoleType::VsSystem);
    }

    #[test]
    fn ines_vertical_battery_trainer() {
        let h = ines_header(2, 1, 0, 0b0111); // V mirroring + battery + trainer
        let p = parse_header(&h).unwrap();
        assert_eq!(p.mirroring, Mirroring::Vertical);
        assert!(p.has_battery);
        assert!(p.has_trainer);
    }

    #[test]
    fn ines_four_screen_overrides_mirroring_bit() {
        let h = ines_header(2, 1, 0, 0b1001);
        let p = parse_header(&h).unwrap();
        assert_eq!(p.mirroring, Mirroring::FourScreen);
        assert!(p.four_screen);
    }

    #[test]
    fn ines_chr_ram_when_chr_size_zero() {
        let h = ines_header(2, 0, 0, 0);
        let p = parse_header(&h).unwrap();
        assert_eq!(p.chr_size, 0);
        assert_eq!(p.chr_ram_size, 8 * 1024);
    }

    #[test]
    fn nes2_detection_and_extended_fields() {
        let mut h = [0u8; 16];
        h[..4].copy_from_slice(&MAGIC);
        h[4] = 1; // PRG LSB
        h[5] = 1; // CHR LSB
        h[6] = 0x10; // mapper low nibble = 1
        h[7] = 0x08; // NES 2.0 marker, console NES, mapper hi nibble 0
        h[8] = 0x21; // submapper 2, mapper hi 1
        h[9] = 0x00;
        h[10] = 0x07; // PRG RAM shift 7 -> 64<<7 = 8 KiB
        h[11] = 0x00;
        h[12] = 0x01; // PAL
        let p = parse_header(&h).unwrap();
        assert!(p.is_nes2);
        assert_eq!(p.mapper_id, 0x101); // bits: low=1, hi=1<<8
        assert_eq!(p.submapper, 2);
        assert_eq!(p.prg_ram_size, 8 * 1024);
        assert_eq!(p.region, Region::Pal);
        assert_eq!(p.console_type, ConsoleType::Nes);
    }

    #[test]
    fn nes2_exponent_multiplier_sizing() {
        let mut h = [0u8; 16];
        h[..4].copy_from_slice(&MAGIC);
        // exponent = 16, multiplier = 1: 2^16 = 65536 bytes
        h[4] = 16 << 2;
        h[5] = 0;
        h[6] = 0;
        h[7] = 0x08; // NES 2.0
        h[8] = 0;
        h[9] = 0x0F; // PRG MSB nibble = $F (exponent path)
        let p = parse_header(&h).unwrap();
        assert_eq!(p.prg_size, 65536);
    }

    #[test]
    fn round_trip_ines_header() {
        let h = ines_header(2, 1, 4, 0b0011); // mapper 4, V + battery
        let parsed = parse_header(&h).unwrap();
        let again = canonical_header(&parsed);
        assert_eq!(&h[0..8], &again[0..8]);
    }

    #[test]
    fn round_trip_nes2_header() {
        let mut h = [0u8; 16];
        h[..4].copy_from_slice(&MAGIC);
        h[4] = 2;
        h[5] = 1;
        h[6] = 0x41; // mapper low 4, vertical
        h[7] = 0x08; // NES 2.0, console NES, mapper mid 0
        h[8] = 0x10; // submapper 1
        h[9] = 0x00;
        h[10] = 0x07;
        h[11] = 0x00;
        h[12] = 0x01;
        h[14] = 0x01; // one miscellaneous ROM
        h[15] = 0x2A; // multicart
        let parsed = parse_header(&h).unwrap();
        assert_eq!(parsed.misc_rom_count, 1);
        assert_eq!(parsed.default_expansion_device, ExpansionDevice::Multicart);
        // Since v2.9.8 every byte of a header with no reserved bits set
        // round-trips through the canonical encoding.
        assert_eq!(canonical_header(&parsed), h);
    }

    #[test]
    fn nes_cart_has_no_vs_ppu_type() {
        // A standard NES 2.0 cart (console type Nes) parses to VsPpuType::None.
        let mut h = [0u8; 16];
        h[..4].copy_from_slice(&MAGIC);
        h[4] = 1;
        h[5] = 1;
        h[7] = 0x08; // NES 2.0, console = Nes
        h[13] = 0x09; // would be 2C05-02 IF this were a Vs. cart
        let p = parse_header(&h).unwrap();
        assert_eq!(p.console_type, ConsoleType::Nes);
        assert_eq!(p.vs_ppu_type, VsPpuType::None);
    }

    #[test]
    fn vs_byte13_parses_ppu_type() {
        // Console type Vs. System (byte 7 bits 0-1 = 1) + byte 13 low nibble.
        let mk = |nibble: u8| {
            let mut h = [0u8; 16];
            h[..4].copy_from_slice(&MAGIC);
            h[4] = 1;
            h[5] = 1;
            h[7] = 0x09; // NES 2.0 (bits 2-3 = 10) + console Vs (bits 0-1 = 01)
            h[13] = nibble;
            parse_header(&h).unwrap()
        };
        assert_eq!(mk(0x0).vs_ppu_type, VsPpuType::Rp2C03);
        assert_eq!(mk(0x2).vs_ppu_type, VsPpuType::Rp2C04_0001);
        assert_eq!(mk(0x5).vs_ppu_type, VsPpuType::Rp2C04_0004);
        assert_eq!(mk(0x9).vs_ppu_type, VsPpuType::Rc2C05_02);
        // The 2C05-02 resolves to the 2C03 palette + 2C05 quirks + $3D id.
        let t = mk(0x9).vs_ppu_type;
        assert_eq!(t.ppu_palette(), crate::cartridge::VsPpuPalette::Rgb2C05);
        assert!(t.is_2c05());
        assert_eq!(t.ppu_2c05_id(), 0x3D);
        // High nibble (Vs. hardware type) does not change the PPU type.
        let mut h = [0u8; 16];
        h[..4].copy_from_slice(&MAGIC);
        h[7] = 0x09;
        h[13] = 0x52; // hw type 5 (Dual System), PPU type 2 = 2C04-0001
        assert_eq!(
            parse_header(&h).unwrap().vs_ppu_type,
            VsPpuType::Rp2C04_0001
        );
    }

    #[test]
    fn vs_byte13_round_trips() {
        let mut h = [0u8; 16];
        h[..4].copy_from_slice(&MAGIC);
        h[4] = 1;
        h[5] = 1;
        h[7] = 0x09; // NES 2.0 + Vs. System
        h[13] = 0x0A; // 2C05-03
        let parsed = parse_header(&h).unwrap();
        assert_eq!(parsed.vs_ppu_type, VsPpuType::Rc2C05_03);
        let again = canonical_header(&parsed);
        assert_eq!(again[13] & 0x0F, 0x0A);
    }

    #[test]
    fn ines1_garbage_tail_masks_the_mapper_high_nibble() {
        // NESdev "iNES", Flags 7-15: old tools wrote signatures such as
        // "DiskDude!" into bytes 7-15, which adds 64 to the mapper number,
        // and "if the last 4 bytes are not all zero, and the header is not
        // marked for NES 2.0 format, an emulator should either mask off the
        // upper 4 bits of the mapper number or simply refuse to load the ROM."
        //
        // The staged Russian Balloon Fight translation carries "@iskDude!"
        // (byte 7 = 0x40): an NROM image that loaded as mapper 64 and filled
        // the sky with RAMBO-1-banked tiles.
        let mut h = ines_header(1, 1, 0, 0x01);
        h[7..16].copy_from_slice(b"@iskDude!");
        assert_eq!(parse_header(&h).unwrap().mapper_id, 0);
        // The textbook "DiskDude!" form on an MMC1 image: 65 -> 1.
        let mut h = ines_header(8, 16, 1, 0);
        h[7..16].copy_from_slice(b"DiskDude!");
        assert_eq!(parse_header(&h).unwrap().mapper_id, 1);
        // A clean iNES 1.0 tail keeps the full 8-bit mapper number...
        let h = ines_header(8, 8, 64, 0);
        assert_eq!(parse_header(&h).unwrap().mapper_id, 64);
        // ...and garbage confined to bytes 8-11 does not trigger the rule,
        // which keys on bytes 12-15 only.
        let mut h = ines_header(8, 8, 64, 0);
        h[8..12].copy_from_slice(b"junk");
        assert_eq!(parse_header(&h).unwrap().mapper_id, 64);
        // NES 2.0 headers are exempt: bytes 12-15 are real fields there.
        let mut h = ines_header(8, 8, 64, 0);
        h[7] |= 0x08;
        h[12] = 0x01; // PAL
        h[15] = 0x01; // default expansion device
        assert_eq!(parse_header(&h).unwrap().mapper_id, 64);
    }

    #[test]
    fn ram_shift_helper_zero_returns_zero() {
        assert_eq!(ram_size_from_shift(0), 0);
    }

    #[test]
    fn ram_shift_helper_round_trip() {
        // shift=7 => 8 KiB
        assert_eq!(ram_size_from_shift(7), 8192);
        assert_eq!(ram_shift_for(8192), 7);
        // shift=10 => 64 KiB
        assert_eq!(ram_size_from_shift(10), 65536);
        assert_eq!(ram_shift_for(65536), 10);
    }
}
