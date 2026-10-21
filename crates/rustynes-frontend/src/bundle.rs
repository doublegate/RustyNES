//! v3.5.0 (`FE-04`) — the Multi-Disk Bundler: named slots over several images.
//!
//! A **bundle** is a small TOML manifest (`*.rnbundle`) that names a set of
//! image files and gives each one a name:
//!
//! ```toml
//! name = "Zelda no Densetsu"     # optional; the manifest's file stem otherwise
//!
//! [[slot]]
//! name = "Disk 1"                # optional; the image's file stem otherwise
//! path = "zelda-disk1.fds"       # relative to the manifest's directory
//!
//! [[slot]]
//! name = "Disk 2"
//! path = "zelda-disk2.fds"
//! ```
//!
//! Opening one (File → Open ROM, File → Open Recent, drag-and-drop, the command
//! line) depends on what its slots hold, and every slot must hold the same kind:
//!
//! - **FDS bundle** (every slot a `.fds`): a multi-disk game dumped as one
//!   file per disk becomes ONE disk set. Each slot's image is parsed with the
//!   core's own `parse_fds` (fwNES header or headerless, QD sides truncated)
//!   and the sides are concatenated in slot order into a headerless image,
//!   which then loads exactly as a single `.fds` does: BIOS, battery
//!   `.fds.sav` (keyed by the merged image's hash, so the same manifest finds
//!   the same save), disk swapping. Each side is NAMED for the Emulation →
//!   Famicom Disk System menu: `"<slot> side A"`, `"<slot> side B"`, ... in
//!   place of the bare "Side N". Before v3.5.0 such a game could not be played
//!   at all past its first disk: the menu swaps sides within one image only.
//! - **Cartridge bundle** (every slot a `.nes` / `.unf` / `.unif`): a
//!   "multicart" of separate ROM files. The first slot loads; Emulation →
//!   Multicart lists every slot by name and loads the one picked. Each slot is
//!   its own game (its own battery save, cheats, per-game settings): a slot
//!   switch is a ROM load, nothing more.
//!
//! File → New Multi-Disk Bundle… writes a manifest from a set of picked
//! images (each slot named by its file's stem, editable in the file) and opens
//! it. The manifest format is the whole interface: there is no hidden state,
//! so a bundle can be written by hand, shared, or kept beside the images.
//!
//! What a bundle does not do: it never modifies an image, it does not combine
//! cartridges into one console (each slot is a separate game), and it does not
//! mix FDS and cartridge slots. Native-only: the browser has no paths.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The manifest extension.
pub const BUNDLE_EXTENSION: &str = "rnbundle";

/// The most slots a bundle may list (an FDS disk set may not exceed 255 sides
/// either; the merge checks that separately).
pub const MAX_SLOTS: usize = 64;

/// The most bytes any one slot image may have: 16 MiB, the bound the mobile
/// bridge puts on a ROM image. Any real NES / FDS / UNIF image is far smaller.
const MAX_IMAGE_BYTES: u64 = 16 * 1024 * 1024;

/// The most bytes a manifest may have (it is a short list of names).
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

/// Whether `path` names a bundle manifest (`.rnbundle`, case-insensitive).
#[must_use]
pub fn is_bundle_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(BUNDLE_EXTENSION))
}

/// One named image in a manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotEntry {
    /// The slot's display name; the image's file stem when empty or absent.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// The image, relative to the manifest's directory (or absolute).
    pub path: PathBuf,
}

/// A parsed manifest (the TOML shape).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// The bundle's display name; the manifest's file stem when empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// The slots, in order.
    #[serde(rename = "slot", default)]
    pub slots: Vec<SlotEntry>,
}

/// What a bundle's slots hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleKind {
    /// Famicom Disk System images, merged into one disk set.
    Fds,
    /// Cartridge images, one game per slot.
    Cartridge,
}

/// Why a bundle could not be opened. Each carries enough to say which file.
#[derive(Debug)]
#[non_exhaustive]
pub enum BundleError {
    /// The manifest or a slot image could not be read.
    Io(PathBuf, std::io::Error),
    /// The manifest is not valid TOML of the expected shape.
    Parse(String),
    /// The manifest lists no slots, or more than [`MAX_SLOTS`].
    SlotCount(usize),
    /// A slot's extension is not one a bundle can hold.
    UnsupportedImage(PathBuf),
    /// The slots mix FDS and cartridge images.
    MixedKinds,
    /// A file is larger than a bundle accepts.
    TooLarge(PathBuf, u64),
    /// An FDS slot is not a disk image the core can parse.
    BadDisk(PathBuf, String),
    /// The merged disk set would have more than 255 sides.
    TooManySides(usize),
}

impl core::fmt::Display for BundleError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io(p, e) => write!(f, "cannot read {}: {e}", p.display()),
            Self::Parse(e) => write!(f, "not a valid bundle manifest: {e}"),
            Self::SlotCount(n) => {
                write!(
                    f,
                    "a bundle lists 1 to {MAX_SLOTS} slots; this one lists {n}"
                )
            }
            Self::UnsupportedImage(p) => write!(
                f,
                "{} is not a .fds, .nes, .unf or .unif image",
                p.display()
            ),
            Self::MixedKinds => {
                f.write_str("a bundle holds either FDS disks or cartridges, not both")
            }
            Self::TooLarge(p, n) => write!(f, "{} is too large ({n} bytes)", p.display()),
            Self::BadDisk(p, e) => write!(f, "{} is not a usable FDS disk: {e}", p.display()),
            Self::TooManySides(n) => {
                write!(f, "the disks have {n} sides together; at most 255 can load")
            }
        }
    }
}

impl std::error::Error for BundleError {}

/// A slot after resolution: its display name and its image's full path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    /// Display name (never empty).
    pub name: String,
    /// The image's path, resolved against the manifest's directory.
    pub path: PathBuf,
}

/// A resolved, checked bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    /// The manifest's own path.
    pub manifest_path: PathBuf,
    /// Display name (never empty).
    pub name: String,
    /// What the slots hold.
    pub kind: BundleKind,
    /// The slots, in order (at least one).
    pub slots: Vec<Slot>,
}

/// The file stem of `path` as a display name (`"?"` when it has none).
fn stem_name(path: &Path) -> String {
    path.file_stem()
        .map_or_else(|| "?".to_owned(), |s| s.to_string_lossy().into_owned())
}

/// The kind a slot image's extension implies, if any.
fn kind_of(path: &Path) -> Option<BundleKind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "fds" => Some(BundleKind::Fds),
        "nes" | "unf" | "unif" => Some(BundleKind::Cartridge),
        _ => None,
    }
}

impl Bundle {
    /// Parse and check a manifest's text. `manifest_path` is where it lives:
    /// relative slot paths resolve against its directory, and it names the
    /// bundle when the manifest does not.
    ///
    /// # Errors
    /// [`BundleError::Parse`], [`BundleError::SlotCount`],
    /// [`BundleError::UnsupportedImage`] or [`BundleError::MixedKinds`]. The
    /// images themselves are not read here.
    pub fn from_manifest(manifest_path: &Path, text: &str) -> Result<Self, BundleError> {
        let manifest: Manifest =
            toml::from_str(text).map_err(|e| BundleError::Parse(e.to_string()))?;
        if manifest.slots.is_empty() || manifest.slots.len() > MAX_SLOTS {
            return Err(BundleError::SlotCount(manifest.slots.len()));
        }
        let dir = manifest_path.parent().unwrap_or_else(|| Path::new(""));
        let mut kind = None;
        let mut slots = Vec::with_capacity(manifest.slots.len());
        for entry in manifest.slots {
            let path = if entry.path.is_absolute() {
                entry.path
            } else {
                dir.join(entry.path)
            };
            let this = kind_of(&path).ok_or_else(|| BundleError::UnsupportedImage(path.clone()))?;
            if kind.is_some_and(|k| k != this) {
                return Err(BundleError::MixedKinds);
            }
            kind = Some(this);
            let name = if entry.name.trim().is_empty() {
                stem_name(&path)
            } else {
                entry.name.trim().to_owned()
            };
            slots.push(Slot { name, path });
        }
        let name = if manifest.name.trim().is_empty() {
            stem_name(manifest_path)
        } else {
            manifest.name.trim().to_owned()
        };
        Ok(Self {
            manifest_path: manifest_path.to_path_buf(),
            name,
            // Non-empty slots, so a kind was found.
            kind: kind.unwrap_or(BundleKind::Cartridge),
            slots,
        })
    }

    /// Read and check the manifest at `path`.
    ///
    /// # Errors
    /// [`BundleError::Io`] / [`BundleError::TooLarge`] for the file, then as
    /// [`Self::from_manifest`].
    pub fn open(path: &Path) -> Result<Self, BundleError> {
        let text = read_bounded(path, MAX_MANIFEST_BYTES)?;
        let text = String::from_utf8(text)
            .map_err(|_| BundleError::Parse("the file is not UTF-8 text".into()))?;
        Self::from_manifest(path, &text)
    }
}

/// Read `path` whole, refusing a file larger than `limit` bytes.
fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, BundleError> {
    let len = std::fs::metadata(path)
        .map_err(|e| BundleError::Io(path.to_path_buf(), e))?
        .len();
    if len > limit {
        return Err(BundleError::TooLarge(path.to_path_buf(), len));
    }
    std::fs::read(path).map_err(|e| BundleError::Io(path.to_path_buf(), e))
}

/// The side letter for side `i` of one disk image: `A`, `B`, ... (a two-sided
/// disk is A / B; a longer image keeps counting).
fn side_letter(i: usize) -> char {
    // `i` is below 255 (the merge refuses more), so this stays in `A..`.
    char::from(b'A' + u8::try_from(i % 26).unwrap_or(0))
}

/// Merge FDS slot images into one headerless disk set, in slot order.
///
/// Each image is parsed with the core's `parse_fds` (so a fwNES header, a
/// headerless dump and a QD dump are all accepted, exactly as a single `.fds`
/// load accepts them) and its sides are appended. Returns the merged bytes and
/// one name per side: `"<slot> side A"`, `"<slot> side B"`, ...
///
/// # Errors
/// [`BundleError::BadDisk`] for an image `parse_fds` refuses;
/// [`BundleError::TooManySides`] past 255 sides.
pub fn merge_fds(
    images: &[(String, PathBuf, Vec<u8>)],
) -> Result<(Vec<u8>, Vec<String>), BundleError> {
    let mut merged = Vec::new();
    let mut names = Vec::new();
    for (slot_name, path, bytes) in images {
        let disk = rustynes_core::rustynes_mappers::parse_fds(bytes)
            .map_err(|e| BundleError::BadDisk(path.clone(), e.to_string()))?;
        for side in 0..disk.side_count() {
            names.push(format!("{slot_name} side {}", side_letter(side)));
        }
        merged.extend_from_slice(&disk.to_bytes());
    }
    if names.len() > 255 {
        return Err(BundleError::TooManySides(names.len()));
    }
    Ok((merged, names))
}

/// A bundle as the app holds it while one of its slots is loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedBundle {
    /// The bundle.
    pub bundle: Bundle,
    /// The loaded slot (always 0 for an FDS bundle, whose slots load together).
    pub current: usize,
    /// FDS only: one display name per disk side of the merged set.
    pub side_names: Vec<String>,
}

impl LoadedBundle {
    /// The window / status label: `"<bundle>: <slot>"` for a cartridge bundle,
    /// the bundle's name for an FDS one.
    #[must_use]
    pub fn label(&self) -> String {
        match self.bundle.kind {
            BundleKind::Fds => self.bundle.name.clone(),
            BundleKind::Cartridge => format!(
                "{}: {}",
                self.bundle.name,
                self.bundle.slots[self.current.min(self.bundle.slots.len() - 1)].name
            ),
        }
    }
}

/// The image bytes `path` stands for, and the bundle it opened if it is one.
///
/// - Not a manifest: the file itself, `None`.
/// - FDS bundle: the merged disk set, with its side names.
/// - Cartridge bundle: slot `slot`'s image (clamped to the last slot).
///
/// This is the single read every ROM load path makes in place of
/// `std::fs::read`, so a bundle opens wherever a ROM does.
///
/// # Errors
/// Any [`BundleError`]; for a plain file, [`BundleError::Io`] only.
pub fn read_rom_or_bundle(
    path: &Path,
    slot: usize,
) -> Result<(Vec<u8>, Option<LoadedBundle>), BundleError> {
    if !is_bundle_path(path) {
        let bytes = std::fs::read(path).map_err(|e| BundleError::Io(path.to_path_buf(), e))?;
        return Ok((bytes, None));
    }
    let bundle = Bundle::open(path)?;
    match bundle.kind {
        BundleKind::Fds => {
            let mut images = Vec::with_capacity(bundle.slots.len());
            for s in &bundle.slots {
                images.push((
                    s.name.clone(),
                    s.path.clone(),
                    read_bounded(&s.path, MAX_IMAGE_BYTES)?,
                ));
            }
            let (bytes, side_names) = merge_fds(&images)?;
            Ok((
                bytes,
                Some(LoadedBundle {
                    bundle,
                    current: 0,
                    side_names,
                }),
            ))
        }
        BundleKind::Cartridge => {
            let current = slot.min(bundle.slots.len() - 1);
            let bytes = read_bounded(&bundle.slots[current].path, MAX_IMAGE_BYTES)?;
            Ok((
                bytes,
                Some(LoadedBundle {
                    bundle,
                    current,
                    side_names: Vec::new(),
                }),
            ))
        }
    }
}

/// The manifest text for `images`, each slot named by its file stem and listed
/// relative to `manifest_dir` when it lies inside it (so the bundle can move
/// with its images), absolute otherwise.
#[must_use]
pub fn manifest_text_for(name: &str, manifest_dir: &Path, images: &[PathBuf]) -> String {
    let slots = images
        .iter()
        .map(|p| SlotEntry {
            name: stem_name(p),
            path: p
                .strip_prefix(manifest_dir)
                .map_or_else(|_| p.clone(), Path::to_path_buf),
        })
        .collect();
    let manifest = Manifest {
        name: name.to_owned(),
        slots,
    };
    // A `Manifest` of strings and paths always serializes; fall back to an
    // empty manifest (which then fails to open with a clear error) rather than
    // panic if a path is not valid UTF-8.
    toml::to_string_pretty(&manifest).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustynes_core::rustynes_mappers::FDS_SIDE_LEN;

    /// A headerless one-side disk whose side opens with the disk-info block
    /// and carries `tag` at byte 100 (so sides can be told apart).
    fn raw_side(tag: u8) -> Vec<u8> {
        let mut side = vec![0u8; FDS_SIDE_LEN];
        side[0] = 0x01;
        side[1..15].copy_from_slice(b"*NINTENDO-HVC*");
        side[100] = tag;
        side
    }

    /// A fwNES-headered image of `sides`.
    fn fwnes(sides: &[Vec<u8>]) -> Vec<u8> {
        let mut out = b"FDS\x1a".to_vec();
        out.push(u8::try_from(sides.len()).unwrap());
        out.resize(16, 0);
        for s in sides {
            out.extend_from_slice(s);
        }
        out
    }

    #[test]
    fn a_manifest_resolves_names_paths_and_kind() {
        let b = Bundle::from_manifest(
            Path::new("/games/zelda.rnbundle"),
            "name = \"Zelda\"\n[[slot]]\nname = \"Disk 1\"\npath = \"d1.fds\"\n\
             [[slot]]\npath = \"/abs/Disk Two.FDS\"\n",
        )
        .unwrap();
        assert_eq!(b.name, "Zelda");
        assert_eq!(b.kind, BundleKind::Fds);
        assert_eq!(b.slots[0].name, "Disk 1");
        assert_eq!(b.slots[0].path, PathBuf::from("/games/d1.fds"));
        assert_eq!(
            b.slots[1].name, "Disk Two",
            "the stem names an unnamed slot"
        );
        assert_eq!(b.slots[1].path, PathBuf::from("/abs/Disk Two.FDS"));

        let b = Bundle::from_manifest(
            Path::new("/m/Multi.rnbundle"),
            "[[slot]]\npath = \"a.nes\"\n[[slot]]\npath = \"b.UNIF\"\n",
        )
        .unwrap();
        assert_eq!((b.kind, b.name.as_str()), (BundleKind::Cartridge, "Multi"));
    }

    #[test]
    fn a_bad_manifest_is_refused_with_a_reason() {
        let m = Path::new("/m/x.rnbundle");
        assert!(matches!(
            Bundle::from_manifest(m, "name = \"empty\"\n"),
            Err(BundleError::SlotCount(0))
        ));
        assert!(matches!(
            Bundle::from_manifest(
                m,
                "[[slot]]\npath = \"a.fds\"\n[[slot]]\npath = \"b.nes\"\n"
            ),
            Err(BundleError::MixedKinds)
        ));
        assert!(matches!(
            Bundle::from_manifest(m, "[[slot]]\npath = \"a.zip\"\n"),
            Err(BundleError::UnsupportedImage(_))
        ));
        assert!(matches!(
            Bundle::from_manifest(m, "[[slot]\n"),
            Err(BundleError::Parse(_))
        ));
        let many = (0..=MAX_SLOTS).fold(String::new(), |mut acc, i| {
            use std::fmt::Write as _;
            let _ = write!(acc, "[[slot]]\npath = \"{i}.nes\"\n");
            acc
        });
        assert!(matches!(
            Bundle::from_manifest(m, &many),
            Err(BundleError::SlotCount(n)) if n == MAX_SLOTS + 1
        ));
    }

    /// Two dumps (one fwNES two-sider, one headerless one-sider) merge into one
    /// headerless three-side set in slot order, which the core parses back
    /// side for side, with one name per side.
    #[test]
    fn fds_slots_merge_into_one_named_disk_set() {
        let d1 = fwnes(&[raw_side(1), raw_side(2)]);
        let d2 = raw_side(3);
        let (bytes, names) = merge_fds(&[
            ("Disk 1".into(), "d1.fds".into(), d1),
            ("Disk 2".into(), "d2.fds".into(), d2),
        ])
        .unwrap();
        assert_eq!(names, ["Disk 1 side A", "Disk 1 side B", "Disk 2 side A"]);
        assert_eq!(bytes.len(), 3 * FDS_SIDE_LEN, "headerless, three sides");
        let disk = rustynes_core::rustynes_mappers::parse_fds(&bytes).unwrap();
        assert_eq!(disk.side_count(), 3);
        for (i, tag) in [1u8, 2, 3].into_iter().enumerate() {
            assert_eq!(disk.side(i)[100], tag, "side {i} in slot order");
        }
    }

    #[test]
    fn a_bad_disk_or_too_many_sides_is_refused() {
        let r = merge_fds(&[("x".into(), "x.fds".into(), b"NES\x1a".to_vec())]);
        assert!(matches!(r, Err(BundleError::BadDisk(..))));
        let sixty_four = fwnes(&vec![raw_side(0); 64]);
        let images: Vec<_> = (0..4)
            .map(|i| (format!("{i}"), PathBuf::from("d.fds"), sixty_four.clone()))
            .collect();
        assert!(matches!(
            merge_fds(&images),
            Err(BundleError::TooManySides(256))
        ));
    }

    /// The whole read through real files: an FDS bundle yields the merged set,
    /// a cartridge bundle yields the chosen slot's file (clamped), and a plain
    /// file is read as itself.
    #[test]
    fn read_rom_or_bundle_opens_both_kinds_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        std::fs::write(p.join("a.fds"), raw_side(7)).unwrap();
        std::fs::write(p.join("b.fds"), fwnes(&[raw_side(8), raw_side(9)])).unwrap();
        std::fs::write(
            p.join("set.rnbundle"),
            "[[slot]]\nname = \"Boot\"\npath = \"a.fds\"\n[[slot]]\npath = \"b.fds\"\n",
        )
        .unwrap();
        let (bytes, loaded) = read_rom_or_bundle(&p.join("set.rnbundle"), 5).unwrap();
        let loaded = loaded.unwrap();
        assert_eq!(bytes.len(), 3 * FDS_SIDE_LEN);
        assert_eq!(loaded.side_names, ["Boot side A", "b side A", "b side B"]);
        assert_eq!((loaded.current, loaded.label()), (0, "set".to_owned()));

        std::fs::write(p.join("one.nes"), b"first").unwrap();
        std::fs::write(p.join("two.nes"), b"second").unwrap();
        std::fs::write(
            p.join("multi.RNBUNDLE"),
            "name = \"Two games\"\n[[slot]]\npath = \"one.nes\"\n[[slot]]\nname = \"Game 2\"\npath = \"two.nes\"\n",
        )
        .unwrap();
        let (bytes, loaded) = read_rom_or_bundle(&p.join("multi.RNBUNDLE"), 1).unwrap();
        assert_eq!(bytes, b"second");
        assert_eq!(loaded.as_ref().unwrap().label(), "Two games: Game 2");
        let (bytes, loaded) = read_rom_or_bundle(&p.join("multi.RNBUNDLE"), 9).unwrap();
        assert_eq!(
            (bytes.as_slice(), loaded.unwrap().current),
            (&b"second"[..], 1)
        );

        let (bytes, loaded) = read_rom_or_bundle(&p.join("one.nes"), 0).unwrap();
        assert_eq!((bytes.as_slice(), loaded), (&b"first"[..], None));
        assert!(matches!(
            read_rom_or_bundle(&p.join("missing.rnbundle"), 0),
            Err(BundleError::Io(..))
        ));
    }

    /// A manifest written by the bundler opens to the same slots, with paths
    /// relative to the manifest when the images sit beside it.
    #[test]
    fn a_written_manifest_round_trips() {
        let dir = Path::new("/games/set");
        let images = [
            dir.join("Disk 1.fds"),
            PathBuf::from("/elsewhere/Disk 2.fds"),
        ];
        let text = manifest_text_for("My set", dir, &images);
        assert!(text.contains("path = \"Disk 1.fds\""), "{text}");
        let b = Bundle::from_manifest(&dir.join("my.rnbundle"), &text).unwrap();
        assert_eq!(b.name, "My set");
        assert_eq!(b.slots[0].path, images[0]);
        assert_eq!(b.slots[1].path, images[1]);
        assert_eq!(b.slots[1].name, "Disk 2");
    }

    /// The merged image is what the FDS load path builds a console from: the
    /// core accepts it as one disk set with every slot's sides, and each side
    /// can be inserted. (The BIOS content does not matter to construction; an
    /// 8 KiB zero image stands in for `disksys.rom`, which is not committed.)
    #[test]
    fn the_merged_set_builds_a_console_with_every_side() {
        let (bytes, names) = merge_fds(&[
            ("A".into(), "a.fds".into(), raw_side(1)),
            (
                "B".into(),
                "b.fds".into(),
                fwnes(&[raw_side(2), raw_side(3)]),
            ),
        ])
        .unwrap();
        let mut nes = rustynes_core::Nes::from_disk(&bytes, &[0u8; 0x2000]).unwrap();
        assert_eq!(nes.disk_side_count(), names.len());
        assert_eq!(nes.disk_side_count(), 3);
        nes.set_disk_side(Some(2));
        assert_eq!(nes.inserted_disk_side(), Some(2));
    }

    #[test]
    fn side_letters_count_from_a() {
        assert_eq!(
            [side_letter(0), side_letter(1), side_letter(2)],
            ['A', 'B', 'C']
        );
    }
}
