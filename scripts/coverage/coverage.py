#!/usr/bin/env python3
"""RustyNES mapper/ROM/screenshot coverage tool — the one master CLI.

Stdlib-only. Consolidates every developer-local coverage helper that used to
live as separate throwaway scripts (``mapper_index.py``, ``scan_roots.py``,
``survey.py``, ``stage_rom.py`` from /tmp; ``scripts/rom-survey/{rom_discover,
rom_extract}.py``; ``scripts/screenshots/{categorize_screenshots.py,
organize_screenshots.sh,build_montage.sh}``) into one subcommand-driven tool.

Pipeline (see scripts/coverage/README.md)::

    index -> survey -> discover -> stage -> [Rust capture] -> categorize -> montage/report

Nothing here ever COMMITS ROMs: ``tests/roms/external/**`` is gitignored and the
``stage`` subcommand only writes there. Only the emulator's *output*
(screenshots / .snap hashes) is committed, by the Rust harness, never by this
tool.

The default ROM-library root is the developer's No-Intro/GoodNES NES set::

    ~/Dropbox/ROMs/Nintendo Entertainment System - Famicom (2020)/

Override or extend it with ``--root PATH`` (repeatable). The 25k-header library
scan is cached to ``scripts/coverage/.library-index.json`` so ``survey`` /
``discover`` / ``stage`` re-use it instead of re-walking the tree.

Subcommands
-----------
``index``      Build/refresh the cached library mapper->[titles] index (JSON).
``survey``     Per-mapper avail(library) vs staged vs committed-screenshots
               report; flags mappers under ``--target`` (default 5).
``discover``   Candidate-title-per-mapper report (curated + by-mapper picks).
``stage``      Extract >=N distinct header-verified ROMs per mapper into the
               gitignored ``tests/roms/external/mapper-NNN-Board/`` tree
               (``--dry-run`` is the DEFAULT; pass ``--execute`` to write).
``categorize`` Tier-split the screenshot tree (Core/Curated -> external/,
               BestEffort -> besteffort/) per ADR 0011.
``montage``    Build the showcase montage from committed screenshots.
``report``     One-shot human summary (index stats + survey + tier coverage).

Examples
--------
::

    python3 scripts/coverage/coverage.py index --refresh
    python3 scripts/coverage/coverage.py survey --target 5
    python3 scripts/coverage/coverage.py discover --mapper 33
    python3 scripts/coverage/coverage.py stage --ines 5 --target 5            # dry-run
    python3 scripts/coverage/coverage.py stage --ines 5 --execute
    python3 scripts/coverage/coverage.py stage --unif --execute               # gap-fill UNIFs
    python3 scripts/coverage/coverage.py categorize --dry-run
    python3 scripts/coverage/coverage.py montage
    python3 scripts/coverage/coverage.py report
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import zipfile
from collections import defaultdict

# --------------------------------------------------------------------------- #
# Paths
# --------------------------------------------------------------------------- #


def repo_root() -> str:
    """Derive the repo root from THIS file's location (scripts/coverage/)."""
    here = os.path.dirname(os.path.abspath(__file__))
    return os.path.abspath(os.path.join(here, "..", ".."))


REPO = repo_root()
DEFAULT_LIB = os.path.expanduser(
    "~/Dropbox/ROMs/Nintendo Entertainment System - Famicom (2020)"
)
CACHE_PATH = os.path.join(REPO, "scripts", "coverage", ".library-index.json")
EXTERNAL = os.path.join(REPO, "tests", "roms", "external")
SCREENSHOTS = os.path.join(REPO, "screenshots")


# --------------------------------------------------------------------------- #
# iNES / NES 2.0 / UNIF header parsing  (absorbs mapper_index/scan_roots/stage_rom)
# --------------------------------------------------------------------------- #


def ines_mapper(hdr: bytes) -> int | None:
    """iNES / NES 2.0 mapper number from the 16-byte header, else None."""
    if len(hdr) < 16 or hdr[0:4] != b"NES\x1a":
        return None
    mapper = ((hdr[6] >> 4) & 0x0F) | (hdr[7] & 0xF0)
    # NES 2.0: byte 7 bits 2-3 == 0b10 -> byte 8 low nibble extends the mapper.
    if (hdr[7] & 0x0C) == 0x08:
        mapper |= (hdr[8] & 0x0F) << 8
    return mapper


def ines_submapper(hdr: bytes) -> int:
    """NES 2.0 submapper (byte 8 high nibble); 0 for plain iNES."""
    if len(hdr) < 16 or hdr[0:4] != b"NES\x1a":
        return 0
    if (hdr[7] & 0x0C) == 0x08:
        return (hdr[8] >> 4) & 0x0F
    return 0


# UNIF MAPR (board name) -> iNES mapper number.
#
# Sourced from the Mesen2 UNIF board table and puNES `src/core/unif.c`, then
# cross-checked against docs/mappers.md. UNIF is board-name keyed (no mapper
# byte), so a board->iNES map is required to slot a `.unf` into the
# mapper-NNN-Board tree. See scripts/coverage/UNIF_BOARD_MAP.md for the full
# table + provenance.  NB: the Sachen 8259 variants differ by board suffix
# (A=141, B=138, C=139, D=137) — verified against both references.
UNIF_BOARD_MAP: dict[str, int] = {
    # Nintendo discrete / first-party
    "NROM": 0,
    "NROM-128": 0,
    "NROM-256": 0,
    "RROM": 0,
    "RROM-128": 0,
    "SLROM": 1,
    "SKROM": 1,
    "SAROM": 1,
    "SBROM": 1,
    "SCROM": 1,
    "SEROM": 1,
    "SFROM": 1,
    "SGROM": 1,
    "SHROM": 1,
    "SJROM": 1,
    "SKROM-MMC1B2": 1,
    "SLROM-MMC1B2": 1,
    "SNROM": 1,
    "SOROM": 1,
    "SUROM": 1,
    "SXROM": 1,
    "UNROM": 2,
    "UOROM": 2,
    "UNROM-512-8": 30,
    "UNROM-512-16": 30,
    "UNROM-512-32": 30,
    "CNROM": 3,
    "CPROM": 13,
    "TLROM": 4,
    "TSROM": 4,
    "TKROM": 4,
    "TKSROM": 4,
    "TLSROM": 118,
    "TQROM": 119,
    "TBROM": 4,
    "TFROM": 4,
    "TGROM": 4,
    "TNROM": 4,
    "TR1ROM": 64,
    "TVROM": 4,
    "B4": 4,
    "HKROM": 4,
    "DRROM": 206,
    "EKROM": 5,
    "ELROM": 5,
    "ETROM": 5,
    "EWROM": 5,
    "AMROM": 7,
    "ANROM": 7,
    "AN1ROM": 7,
    "AOROM": 7,
    "PNROM": 9,
    "PEEOROM": 9,
    "FJROM": 10,
    "FKROM": 10,
    "GNROM": 66,
    "MHROM": 66,
    "BNROM": 34,
    "NINA-001": 34,
    "NINA-002": 34,
    "NINA-03": 79,
    "NINA-06": 79,
    "NINA-07": 11,
    # Konami VRC
    "KONAMI-VRC-1": 75,
    "KONAMI-VRC-2": 23,
    "KONAMI-VRC-3": 73,
    "KONAMI-VRC-4": 21,
    "KONAMI-VRC-6": 24,
    "KONAMI-VRC-7": 85,
    "VRC7": 85,
    # Nanjing / Waixing / pirate-ish
    "MMC3": 4,
    "Mapper245": 245,
    # Color Dreams / Wisdom Tree
    "COLORDREAMS": 11,
    "CDREAM": 11,
    # Bandai
    "BANDAI-74*161/161/32": 152,
    "BANDAI-FCG": 16,
    "BANDAI-LZ93D50": 16,
    "BANDAI-LZ93D50+24C01": 159,
    "BANDAI-LZ93D50+24C02": 16,
    # Sunsoft
    "SUNSOFT_UNROM": 93,
    "SUNSOFT-1": 184,
    "SUNSOFT-2": 89,
    "SUNSOFT-3": 67,
    "SUNSOFT-4": 68,
    "SUNSOFT-5B": 69,
    "SUNSOFT-FME-7": 69,
    "JF-16": 78,
    # Irem
    "IREM-G101": 32,
    "IREM-H3001": 65,
    "IREM-74*161/161/21/138": 77,
    "IREM-HOLYDIVER": 78,
    "HVC-UN1ROM": 94,
    # Jaleco
    "JALECO-JF-11": 140,
    "JALECO-JF-13": 86,
    "JALECO-JF-14": 140,
    "JALECO-JF-16": 78,
    "JALECO-JF-17": 72,
    "JALECO-JF-19": 92,
    "JALECO-SS88006": 18,
    # Namco
    "NAMCOT-3433": 88,
    "NAMCOT-3443": 88,
    "NAMCOT-3446": 76,
    "NAMCOT-3453": 88,
    "NAMCOT-163": 19,
    "NAMCOT-175": 210,
    "NAMCOT-340": 210,
    # Taito
    "TAITO-TC0190FMC": 33,
    "TAITO-TC0190FMR": 33,
    "TC0190FMC+PAL16R4": 48,
    "TAITO-X1-005": 80,
    "TAITO-X1-017": 82,
    # Camerica / Codemasters
    "CAMERICA-BF9093": 71,
    "CAMERICA-BF9096": 232,
    "CAMERICA-ALGN": 71,
    "CAMERICA-ALGQ": 232,
    "BF9097": 71,
    # AVE
    "AVE-NINA-01": 34,
    "AVE-NINA-02": 34,
    "AVE-NINA-03": 79,
    "AVE-NINA-06": 79,
    # Sachen — board-suffix-disambiguated 8259 family (verified vs puNES/Mesen2)
    "SACHEN-8259A": 141,
    "SACHEN-8259B": 138,
    "SACHEN-8259C": 139,
    "SACHEN-8259D": 137,
    "SACHEN-74LS374N": 150,
    "SA-016-1M": 79,
    "SA-72007": 145,
    "SA-72008": 133,
    "SA-NROM": 143,
    "SA-0036": 149,
    "SA-0037": 148,
    "TCA01": 143,
    "TCU01": 147,
    "TC-U01-1.5M": 147,
    # Misc multicarts / homebrew
    "GTROM": 111,
    "CHEAPOCABRA": 111,
    "ACTION52": 228,
    "CALTRON6IN1": 41,
    "MAGICFLOOR": 218,
    "RET-CUFROM": 29,
}


# --------------------------------------------------------------------------- #
# Mapper -> board / family name  (absorbs + extends rom_extract.FAMILY)
# --------------------------------------------------------------------------- #
#
# Cross-checked against docs/mappers.md (the authoritative board-name table).
# Used to name the mapper-NNN-Board/ stage + screenshot dirs. Names are
# sanitized (alnum + dash) so they survive as directory names.
FAMILY: dict[int, str] = {
    0: "NROM",
    1: "MMC1",
    2: "UxROM",
    3: "CNROM",
    4: "MMC3",
    5: "MMC5",
    7: "AxROM",
    9: "MMC2",
    10: "MMC4",
    11: "ColorDreams",
    13: "CPROM",
    15: "Multicart15",
    16: "BandaiFCG",
    18: "JalecoSS88006",
    19: "Namco163",
    21: "VRC4a-c",
    22: "VRC2a",
    23: "VRC4e-f-VRC2b",
    24: "VRC6a",
    25: "VRC4b-d-VRC2c",
    26: "VRC6b",
    28: "Action53",
    29: "RET-CUFROM",
    30: "UNROM512",
    31: "INL-NSF",
    32: "IremG101",
    33: "TaitoTC0190",
    34: "BNROM-NINA001",
    36: "TXC36",
    38: "BitCorp-PCI556",
    39: "Multicart39",
    40: "NTDEC2722",
    41: "Caltron6in1",
    48: "TaitoTC0690",
    58: "Multicart58",
    60: "Multicart60",
    61: "Multicart61",
    62: "Multicart62",
    63: "NTDEC0324",
    64: "TengenRAMBO1",
    65: "IremH3001",
    66: "GxROM",
    67: "Sunsoft3",
    68: "Sunsoft4",
    69: "FME7-Sunsoft5B",
    70: "BandaiDiscrete",
    71: "Camerica-BF9093",
    72: "Jaleco72",
    73: "VRC3",
    75: "VRC1",
    76: "Namcot3446",
    77: "Irem77",
    78: "HolyDiver",
    79: "AVE-NINA03-06",
    80: "TaitoX1-005",
    81: "NTDEC-SuperGun",
    82: "TaitoX1-017",
    85: "VRC7",
    86: "JalecoJF13",
    87: "JalecoKonami-CNROM",
    88: "Namcot118",
    89: "Sunsoft2",
    92: "JalecoJF19",
    93: "Sunsoft3R",
    94: "UN1ROM",
    95: "Namcot3425",
    96: "Multicart96",
    97: "Irem-TamSan",
    99: "VsSystem",
    101: "JalecoJF10",
    107: "MagicDragon",
    111: "GTROM-Cheapocabra",
    112: "NTDEC-Asder",
    113: "NINA006-MB91",
    118: "TxSROM-TLSROM",
    119: "TQROM",
    132: "TXC132",
    133: "SachenSA72008",
    137: "Sachen8259D",
    140: "JalecoJF11-14",
    143: "SachenTCA01",
    145: "SachenSA72007",
    146: "Sachen-NINA",
    147: "Sachen3018-JV001",
    148: "SachenSA0037",
    149: "SachenSA0036",
    150: "Sachen74LS374N",
    151: "Konami-VS-VRC1",
    152: "Bandai74161",
    156: "DIS23C01-DAOU",
    159: "BandaiLZ93D50-24C01",
    162: "WaixingFS304",
    174: "NTDEC-5in1",
    177: "Hengedianzi",
    178: "WaixingEdu",
    179: "Hengedianzi-Variant",
    180: "UNROM-Nichibutsu",
    184: "Sunsoft1",
    185: "CNROM-Lock",
    200: "Multicart200",
    201: "Multicart201",
    202: "Multicart202",
    203: "Multicart203",
    206: "Namcot118-DxROM",
    210: "Namco175-340",
    212: "Multicart212",
    213: "Multicart213",
    214: "Multicart214",
    218: "MagicFloor",
    225: "ColorDreams72in1",
    226: "BMC-76in1",
    227: "BMC-1200in1",
    229: "BMC-31in1",
    231: "BMC-20in1",
    232: "Camerica-Quattro",
    233: "BMC-42in1",
    234: "Maxi15",
    240: "C-E-Multicart",
    241: "BxROM-Pirate",
    242: "Waixing43in1",
    244: "Decathlon",
    246: "FongShenBang",
    250: "Nitra",
}


def family_name(mapper: int) -> str:
    return FAMILY.get(mapper, f"m{mapper}")


def mapper_dir_name(mapper: int) -> str:
    fam = re.sub(r"[^A-Za-z0-9]+", "-", family_name(mapper)).strip("-")
    return f"mapper-{mapper:03d}-{fam}"


# --------------------------------------------------------------------------- #
# Tier table — parsed live from crates/rustynes-mappers/src/tier.rs, with an
# embedded fallback (absorbs categorize_screenshots' static table).
# --------------------------------------------------------------------------- #

_FALLBACK_CORE = {
    0, 1, 2, 3, 4, 5, 7, 9, 10, 11, 13, 16, 18, 19, 21, 22, 23, 24, 25, 26, 32,
    33, 34, 48, 64, 65, 66, 67, 68, 69, 70, 71, 73, 75, 78, 80, 82, 85, 87, 88,
    89, 93, 99, 118, 119, 151, 152, 159, 184, 206, 210,
}
_FALLBACK_CURATED = {38, 41, 79, 86, 113, 140, 232, 240, 241}
_FALLBACK_BEST_EFFORT = {
    15, 28, 29, 30, 31, 36, 39, 40, 58, 60, 61, 62, 63, 72, 76, 77, 81, 92, 94,
    95, 96, 97, 101, 107, 111, 112, 132, 133, 137, 143, 145, 146, 147, 148, 149,
    150, 156, 162, 174, 177, 178, 179, 180, 185, 200, 201, 202, 203, 212, 213,
    214, 218, 225, 226, 227, 229, 231, 233, 234, 242, 244, 246, 250,
}

SPECIAL_EXTERNAL = {"fds", "pc10", "vs-system"}


def _parse_tier_block(text: str, marker: str) -> set[int]:
    """Pull the integer ids out of one match-arm block in tier.rs.

    The block runs from `marker` up to the next `// ---` comment or the closing
    of the arm. We just scan the region after the marker for the next run of
    `N | N | ... =>` ids.
    """
    i = text.find(marker)
    if i < 0:
        return set()
    # The marker sits inside a `// ...` comment line whose prose may contain
    # numbers ("the original 51 families"). Skip to AFTER the end of the comment
    # block (the first non-comment line), then grab ids up to this arm's `=>`.
    region = text[i:]
    lines = region.splitlines(keepends=True)
    body_lines = []
    started = False
    for ln in lines[1:]:
        stripped = ln.lstrip()
        if not started:
            if stripped.startswith("//"):
                continue  # still in the marker's comment block
            started = True
        body_lines.append(ln)
    body = "".join(body_lines)
    arrow = body.find("=>")
    if arrow < 0:
        return set()
    ids = re.findall(r"\b(\d+)\b", body[:arrow])
    return {int(x) for x in ids}


def load_tiers() -> tuple[set[int], set[int], set[int]]:
    tier_rs = os.path.join(
        REPO, "crates", "rustynes-mappers", "src", "tier.rs"
    )
    try:
        with open(tier_rs, encoding="utf-8") as f:
            text = f.read()
        core = _parse_tier_block(text, "/ Core:")
        curated = _parse_tier_block(text, "/ Curated:")
        best = _parse_tier_block(text, "/ BestEffort:")
        if core and curated and best:
            return core, curated, best
    except OSError:
        pass
    return set(_FALLBACK_CORE), set(_FALLBACK_CURATED), set(_FALLBACK_BEST_EFFORT)


def implemented_mappers() -> list[int]:
    core, curated, best = load_tiers()
    return sorted(core | curated | best)


def tier_of(mapper: int) -> str | None:
    core, curated, best = load_tiers()
    if mapper in core:
        return "Core"
    if mapper in curated:
        return "Curated"
    if mapper in best:
        return "BestEffort"
    return None


def screenshot_category(mapper: int) -> str | None:
    t = tier_of(mapper)
    if t in ("Core", "Curated"):
        return "external"
    if t == "BestEffort":
        return "besteffort"
    return None


# --------------------------------------------------------------------------- #
# Library scanning + cache  (absorbs mapper_index.py / scan_roots.py / survey.py)
# --------------------------------------------------------------------------- #


def _unif_board(data: bytes) -> str | None:
    """Extract the MAPR (board name) chunk from UNIF data, else None."""
    if data[0:4] != b"UNIF":
        return None
    off = 32  # UNIF header is 32 bytes
    n = len(data)
    while off + 8 <= n:
        cid = data[off:off + 4]
        (clen,) = struct.unpack("<I", data[off + 4:off + 8])
        body = data[off + 8:off + 8 + clen]
        if cid == b"MAPR":
            return body.split(b"\x00", 1)[0].decode("ascii", "replace")
        off += 8 + clen
    return None


def _scan_path(path: str, on_ines, on_unif) -> None:
    """Scan one .nes/.zip/.7z/.unf path, dispatching headers to callbacks."""
    low = path.lower()
    try:
        if low.endswith(".nes"):
            with open(path, "rb") as f:
                on_ines(os.path.basename(path), f.read(16), path, None)
        elif low.endswith((".unf", ".unif")):
            with open(path, "rb") as f:
                data = f.read()
            board = _unif_board(data)
            if board is not None:
                on_unif(os.path.basename(path), board, path, None)
        elif low.endswith(".zip"):
            with zipfile.ZipFile(path) as z:
                for n in z.namelist():
                    nl = n.lower()
                    if nl.endswith(".nes"):
                        with z.open(n) as f:
                            on_ines(os.path.basename(path), f.read(16), path, n)
                    elif nl.endswith((".unf", ".unif")):
                        board = _unif_board(z.read(n))
                        if board is not None:
                            on_unif(os.path.basename(path), board, path, n)
        elif low.endswith(".7z"):
            # Needs the `7z` CLI; absorbed from scan_roots.py.
            try:
                listing = subprocess.run(
                    ["7z", "l", "-slt", path],
                    capture_output=True, text=True, timeout=30,
                ).stdout
            except (FileNotFoundError, subprocess.SubprocessError):
                return
            if ".nes" not in listing.lower():
                return
            with tempfile.TemporaryDirectory() as td:
                subprocess.run(
                    ["7z", "e", "-y", f"-o{td}", path, "*.nes", "-r"],
                    capture_output=True, timeout=120,
                )
                for e in os.listdir(td):
                    if e.lower().endswith(".nes"):
                        with open(os.path.join(td, e), "rb") as f:
                            on_ines(os.path.basename(path), f.read(16), path, e)
    except (zipfile.BadZipFile, OSError, struct.error):
        pass


def scan_library(roots: list[str], full_titles: bool = True) -> dict:
    """Walk every root; return the mapper-index structure.

    {
      "roots": [...],
      "scanned": int, "unif_seen": int,
      "ines": { "<mapper>": [ {"name","path","entry","sub"} ... ] },
      "unif": { "<board>": [ {"name","path","entry","mapper"} ... ] },
    }
    """
    ines: dict[int, list] = defaultdict(list)
    unif: dict[str, list] = defaultdict(list)
    stats = {"scanned": 0, "unif_seen": 0}

    def on_ines(name, hdr, path, entry):
        m = ines_mapper(hdr)
        stats["scanned"] += 1
        if m is None:
            return
        rec = {"name": name if entry is None else f"{name}:{entry}"}
        if full_titles:
            rec["sub"] = ines_submapper(hdr)
        ines[m].append(rec)

    def on_unif(name, board, path, entry):
        stats["unif_seen"] += 1
        rec = {
            "name": name if entry is None else f"{name}:{entry}",
            "mapper": UNIF_BOARD_MAP.get(board),
        }
        unif[board].append(rec)

    for root in roots:
        if not os.path.isdir(root):
            sys.stderr.write(f"warning: root not found: {root}\n")
            continue
        for dp, _, files in os.walk(root):
            if "/target/" in dp or "/.git/" in dp:
                continue
            for fn in files:
                _scan_path(os.path.join(dp, fn), on_ines, on_unif)

    return {
        "roots": roots,
        "scanned": stats["scanned"],
        "unif_seen": stats["unif_seen"],
        "ines": {str(m): ines[m] for m in sorted(ines)},
        "unif": {b: unif[b] for b in sorted(unif)},
    }


def load_index(args, build_if_missing: bool = True) -> dict:
    """Load the cached library index, building it on demand."""
    roots = [args.root] if getattr(args, "root", None) else []
    roots = (getattr(args, "roots", None) or roots) or [DEFAULT_LIB]
    if not getattr(args, "refresh", False) and os.path.exists(CACHE_PATH):
        try:
            with open(CACHE_PATH, encoding="utf-8") as f:
                idx = json.load(f)
            # Honor added roots by merging a delta scan if they're new.
            extra = [r for r in roots if r not in idx.get("roots", [])]
            if not extra:
                return idx
            sys.stderr.write(f"indexing additional roots: {extra}\n")
            delta = scan_library(extra)
            return _merge_index(idx, delta)
        except (OSError, json.JSONDecodeError):
            pass
    if not build_if_missing:
        raise SystemExit(
            "no library index cached; run `coverage.py index` first"
        )
    sys.stderr.write(f"scanning library roots {roots} ...\n")
    idx = scan_library(roots)
    save_index(idx)
    return idx


def _merge_index(a: dict, b: dict) -> dict:
    out = {
        "roots": sorted(set(a.get("roots", [])) | set(b.get("roots", []))),
        "scanned": a.get("scanned", 0) + b.get("scanned", 0),
        "unif_seen": a.get("unif_seen", 0) + b.get("unif_seen", 0),
        "ines": defaultdict(list),
        "unif": defaultdict(list),
    }
    for src in (a, b):
        for m, recs in src.get("ines", {}).items():
            out["ines"][m].extend(recs)
        for board, recs in src.get("unif", {}).items():
            out["unif"][board].extend(recs)
    out["ines"] = dict(out["ines"])
    out["unif"] = dict(out["unif"])
    return out


def save_index(idx: dict) -> None:
    os.makedirs(os.path.dirname(CACHE_PATH), exist_ok=True)
    with open(CACHE_PATH, "w", encoding="utf-8") as f:
        json.dump(idx, f, indent=0)


def avail_counts(idx: dict) -> dict[int, int]:
    return {int(m): len(recs) for m, recs in idx.get("ines", {}).items()}


# --------------------------------------------------------------------------- #
# Repo-state scanning: staged ROMs + committed screenshots (from survey.py)
# --------------------------------------------------------------------------- #

_MAPPER_DIR_RE = re.compile(r"mapper-(\d+)")


def _dir_mapper(name: str) -> int | None:
    m = _MAPPER_DIR_RE.search(name)
    return int(m.group(1)) if m else None


def staged_counts() -> dict[int, int]:
    out: dict[int, int] = defaultdict(int)
    if not os.path.isdir(EXTERNAL):
        return out
    for d in os.listdir(EXTERNAL):
        mid = _dir_mapper(d)
        full = os.path.join(EXTERNAL, d)
        if mid is None or not os.path.isdir(full):
            continue
        out[mid] += len(
            [f for f in os.listdir(full)
             if f.lower().endswith((".nes", ".zip", ".fds", ".unf", ".unif"))]
        )
    return out


def screenshot_counts() -> dict[int, int]:
    out: dict[int, int] = defaultdict(int)
    for tree in ("external", "besteffort"):
        base = os.path.join(SCREENSHOTS, tree)
        if not os.path.isdir(base):
            continue
        for d in os.listdir(base):
            mid = _dir_mapper(d)
            full = os.path.join(base, d)
            if mid is None or not os.path.isdir(full):
                continue
            out[mid] += len(
                [f for f in os.listdir(full) if f.lower().endswith(".png")]
            )
    return out


# --------------------------------------------------------------------------- #
# Subcommand: index
# --------------------------------------------------------------------------- #


def cmd_index(args) -> int:
    args.refresh = True
    idx = load_index(args)
    distinct = len(idx["ines"])
    print(
        f"indexed {idx['scanned']} iNES headers across {len(idx['roots'])} "
        f"root(s); {distinct} distinct mappers; {idx['unif_seen']} UNIF files"
    )
    print(f"cache: {os.path.relpath(CACHE_PATH, REPO)}")
    if args.wanted:
        wanted = {int(x) for x in args.wanted}
        for m in sorted(wanted):
            recs = idx["ines"].get(str(m), [])
            ex = ", ".join(r["name"] for r in recs[:3])
            print(f"  mapper {m:>3}: {len(recs):>4}  {ex}")
    return 0


# --------------------------------------------------------------------------- #
# Subcommand: survey  (absorbs survey.py)
# --------------------------------------------------------------------------- #


def cmd_survey(args) -> int:
    idx = load_index(args)
    avail = avail_counts(idx)
    staged = staged_counts()
    shots = screenshot_counts()
    target = args.target

    print(f"{'map':>4} {'tier':>10} {'avail':>6} {'staged':>6} {'shots':>5}  status")
    deficient = []
    for m in implemented_mappers():
        a, s, sh = avail.get(m, 0), staged.get(m, 0), shots.get(m, 0)
        ok = sh >= target
        if not ok:
            deficient.append((m, a, s, sh))
        flag = "" if ok else (
            f"  <-- under {target} shots"
            + (f" (avail {a})" if a > 0 else " (NO library iNES)")
        )
        print(f"{m:>4} {str(tier_of(m)):>10} {a:>6} {s:>6} {sh:>5}{flag}")

    print(f"\nDEFICIENT (<{target} screenshots): {len(deficient)} mappers")
    print(
        "  fillable from library (avail>=%d):" % target,
        " ".join(str(m) for m, a, s, sh in deficient if a >= target),
    )
    print(
        "  partial (1<=avail<%d):" % target,
        " ".join(f"{m}:{a}" for m, a, s, sh in deficient if 1 <= a < target),
    )
    print(
        "  no library iNES (avail=0):",
        " ".join(str(m) for m, a, s, sh in deficient if a == 0),
    )
    return 0


# --------------------------------------------------------------------------- #
# Subcommand: discover  (absorbs rom_discover.py — generalized to all mappers)
# --------------------------------------------------------------------------- #


def cmd_discover(args) -> int:
    idx = load_index(args)
    impl = set(implemented_mappers())
    wanted = (
        {int(x) for x in args.mapper}
        if args.mapper
        else (impl if args.all else sorted(impl))
    )
    wanted = sorted(set(wanted))

    print(f"{'map':>4} {'tier':>10} {'avail':>6}  candidate titles (header-verified)")
    for m in wanted:
        recs = idx["ines"].get(str(m), [])
        # Prefer clean [!] dumps and distinct base names.
        ranked = _rank_candidates(recs, args.count)
        names = ", ".join(r["name"] for r in ranked)
        tag = "" if m in impl else "  [UNIMPLEMENTED]"
        print(f"{m:>4} {str(tier_of(m)):>10} {len(recs):>6}  {names}{tag}")
    return 0


def _base_title(name: str) -> str:
    """Strip archive entry + region/version parens to a comparable base name."""
    name = name.split(":")[-1]
    name = re.sub(r"\.(nes|zip|unf|unif)$", "", name, flags=re.I)
    name = re.sub(r"\s*\([^)]*\)", "", name)
    name = re.sub(r"\s*\[[^]]*\]", "", name)
    return name.strip().lower()


def _rank_candidates(recs: list, count: int) -> list:
    """Prefer clean ([!]) dumps and distinct base titles."""
    def score(r):
        n = r["name"]
        clean = "[!]" in n
        # Penalize obvious bad/overdump/pirate dump flags.
        bad = any(t in n for t in ("[b", "[o", "[p", "[h", "[a"))
        return (0 if clean else 1, 1 if bad else 0, len(n))

    seen: set[str] = set()
    out = []
    for r in sorted(recs, key=score):
        bt = _base_title(r["name"])
        if bt in seen:
            continue
        seen.add(bt)
        out.append(r)
        if len(out) >= count:
            break
    return out


# --------------------------------------------------------------------------- #
# Subcommand: stage  (absorbs stage_rom.py + rom_extract.py)
# --------------------------------------------------------------------------- #


def cmd_stage(args) -> int:
    idx = _STAGE_IDX  # already loaded in main() and shared with _stage_one
    impl = set(implemented_mappers())
    target = args.target
    want_n = args.ines if args.ines is not None else target

    selected = (
        {int(x) for x in args.mapper} if args.mapper else impl
    )
    selected = sorted(selected & impl)
    shots = screenshot_counts()
    staged = staged_counts()

    execute = args.execute  # dry-run is the default
    mode = "EXECUTE" if execute else "DRY-RUN (pass --execute to write)"
    print(f"== stage {mode}: >={want_n} distinct iNES per mapper into "
          f"{os.path.relpath(EXTERNAL, REPO)}/ ==\n")

    total_planned = 0
    for m in selected:
        # Skip well-covered mappers unless --force.
        have = max(staged.get(m, 0), shots.get(m, 0))
        if not args.force and have >= want_n:
            continue
        need = want_n - staged.get(m, 0)
        if need <= 0:
            continue
        recs = idx["ines"].get(str(m), [])
        picks = _rank_candidates(recs, need)
        if not picks:
            if args.unif:
                picks = _unif_picks_for(idx, m, need)
            if not picks:
                print(f"mapper {m:>3} ({family_name(m)}): no library ROM (need {need})")
                continue
        dest_dir = os.path.join(EXTERNAL, mapper_dir_name(m))
        for r in picks:
            ok = _stage_one(r, m, dest_dir, execute)
            if ok:
                total_planned += 1

    # Optional dedicated UNIF gap-filler pass.
    if args.unif and not args.mapper:
        _stage_unif_gaps(idx, impl, want_n, staged, shots, execute)

    print(f"\n{'staged' if execute else 'would stage'} {total_planned} ROM(s).")
    if not execute:
        print("re-run with --execute to write (tests/roms/external/** is gitignored).")
    return 0


def _open_ines_bytes(path: str, entry: str | None) -> bytes | None:
    try:
        if entry is None:
            with open(path, "rb") as f:
                return f.read()
        with zipfile.ZipFile(path) as z:
            return z.read(entry)
    except (OSError, KeyError, zipfile.BadZipFile):
        return None


def _record_src(idx: dict, rec: dict) -> tuple[str, str | None] | None:
    """Resolve a {name} record back to a (path, zip-entry) source.

    The cached index stores `name` = "<archive>:<entry>" or "<file.nes>". We
    re-walk the library roots lazily to find the matching path. To keep this
    cheap, scan_library stamps each record's archive basename; here we search
    the configured roots for that basename.
    """
    name = rec["name"]
    if ":" in name:
        arc, entry = name.split(":", 1)
    else:
        arc, entry = name, None
    for root in idx.get("roots", [DEFAULT_LIB]):
        for dp, _, files in os.walk(root):
            if "/target/" in dp or "/.git/" in dp:
                continue
            if arc in files:
                return os.path.join(dp, arc), entry
    return None


def _stage_one(rec: dict, target_mapper: int, dest_dir: str, execute: bool) -> bool:
    src = _record_src(_STAGE_IDX, rec)
    if src is None:
        print(f"  ?? could not locate source for {rec['name']}")
        return False
    path, entry = src
    data = _open_ines_bytes(path, entry)
    if data is None:
        print(f"  ?? unreadable: {rec['name']}")
        return False
    m = ines_mapper(data[:16])
    if m != target_mapper:
        print(f"  !! mapper mismatch got={m} want={target_mapper}: {rec['name']}")
        return False
    base = re.sub(r"\.(nes|zip)$", "", os.path.basename(rec["name"].split(":")[-1]), flags=re.I)
    out = os.path.join(dest_dir, base + ".nes")
    rel = os.path.relpath(out, REPO)
    if execute:
        os.makedirs(dest_dir, exist_ok=True)
        with open(out, "wb") as f:
            f.write(data)
        print(f"  + {rel}  ({len(data)//1024}K)")
    else:
        print(f"  ~ {rel}  ({len(data)//1024}K)")
    return True


def _unif_picks_for(idx: dict, mapper: int, need: int) -> list:
    out = []
    for board, recs in idx.get("unif", {}).items():
        if UNIF_BOARD_MAP.get(board) == mapper:
            out.extend(recs)
            if len(out) >= need:
                break
    return out[:need]


def _stage_unif_gaps(idx, impl, want_n, staged, shots, execute) -> None:
    print("\n== UNIF gap-filler ==")
    for board, recs in sorted(idx.get("unif", {}).items()):
        m = UNIF_BOARD_MAP.get(board)
        if m is None:
            print(f"  unknown UNIF board (no iNES map): {board}  x{len(recs)}")
            continue
        if m not in impl:
            continue
        if max(staged.get(m, 0), shots.get(m, 0)) >= want_n:
            continue
        print(f"  board {board} -> mapper {m} ({family_name(m)}): {len(recs)} file(s)")


# module-global so _stage_one can resolve sources without threading idx through
_STAGE_IDX: dict = {}


# --------------------------------------------------------------------------- #
# Subcommand: categorize  (absorbs categorize_screenshots.py)
# --------------------------------------------------------------------------- #


def cmd_categorize(args) -> int:
    ext = os.path.join(SCREENSHOTS, "external")
    be = os.path.join(SCREENSHOTS, "besteffort")
    os.makedirs(ext, exist_ok=True)
    os.makedirs(be, exist_ok=True)

    moves: list = []
    normalizations: list = []
    flagged: list = []

    def move(src: str, dst: str) -> None:
        rel_s, rel_d = os.path.relpath(src, REPO), os.path.relpath(dst, REPO)
        if args.dry_run:
            print(f"  MOVE  {rel_s}  ->  {rel_d}")
            return
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        if os.path.isdir(src) and os.path.isdir(dst):
            for item in os.listdir(src):
                shutil.move(os.path.join(src, item), os.path.join(dst, item))
            os.rmdir(src)
        else:
            shutil.move(src, dst)

    # 1. Normalize legacy FLAT besteffort PNGs into per-mapper sub-dirs.
    for entry in sorted(os.listdir(be)):
        full = os.path.join(be, entry)
        if not os.path.isfile(full) or not entry.endswith(".png"):
            continue
        stem = entry[:-4]
        dst = os.path.join(be, stem, stem + ".png")
        normalizations.append((full, dst))
        move(full, dst)

    # 2. Relocate dirs whose tier-category disagrees with their current tree.
    for tree, name in ((ext, "external"), (be, "besteffort")):
        for d in sorted(os.listdir(tree)):
            full = os.path.join(tree, d)
            if not os.path.isdir(full):
                continue
            if d in SPECIAL_EXTERNAL:
                if name != "external":
                    flagged.append(f"special dir {d} under {name}/ (want external/)")
                continue
            mid = _dir_mapper(d)
            if mid is None:
                flagged.append(f"unrecognized screenshot dir: {name}/{d}")
                continue
            cat = screenshot_category(mid)
            if cat is None:
                flagged.append(f"{name}/{d}: mapper {mid} unclassified by tier table")
                continue
            if cat != name:
                moves.append((full, os.path.join(SCREENSHOTS, cat, d)))
                move(full, os.path.join(SCREENSHOTS, cat, d))

    print("\n=== relocations (tier mismatch) ===")
    for s, _ in moves:
        cat = screenshot_category(_dir_mapper(os.path.basename(s)))
        print(f"  {os.path.relpath(s, REPO)}  ->  {cat}/")
    if not moves:
        print("  (none — every screenshot dir already sits in the right tree)")
    print(f"\n=== besteffort flat->subdir normalizations: {len(normalizations)} ===")
    if flagged:
        print("\n=== FLAGGED (manual review) ===")
        for f in flagged:
            print(f"  {f}")

    def summarize(tree: str, label: str) -> None:
        if not os.path.isdir(tree):
            return
        dirs = sorted(d for d in os.listdir(tree) if os.path.isdir(os.path.join(tree, d)))
        print(f"\n=== {label}/ ({len(dirs)} dirs) ===")
        for d in dirs:
            n = len([f for f in os.listdir(os.path.join(tree, d)) if f.endswith(".png")])
            print(f"  {d:40} {n} png")

    summarize(ext, "screenshots/external")
    summarize(be, "screenshots/besteffort")
    return 1 if flagged else 0


# --------------------------------------------------------------------------- #
# Subcommand: montage  (absorbs build_montage.sh, ImageMagick-driven)
# --------------------------------------------------------------------------- #

MONTAGE_ROSTER = [
    "mapper-000-NROM/Super Mario Bros.png",
    "mapper-000-NROM/Donkey Kong.png",
    "mapper-000-NROM/Excitebike.png",
    "mapper-001-MMC1/Legend of Zelda, The.png",
    "mapper-001-MMC1/Mega Man 2.png",
    "mapper-001-MMC1/Castlevania II - Simon's Quest.png",
    "mapper-001-MMC1/Ninja Gaiden.png",
    "mapper-001-MMC1/Faxanadu.png",
    "mapper-001-MMC1/Bionic Commando.png",
    "mapper-002-UxROM/Mega Man.png",
    "mapper-002-UxROM/Contra.png",
    "mapper-002-UxROM/Castlevania.png",
    "mapper-002-UxROM/Disney's DuckTales.png",
    "mapper-002-UxROM/Life Force.png",
    "mapper-003-CNROM/Gradius.png",
    "mapper-004-MMC3/Super Mario Bros. 3.png",
    "mapper-004-MMC3/Mega Man 3.png",
    "mapper-004-MMC3/Kirby's Adventure.png",
    "mapper-004-MMC3/Crystalis.png",
    "mapper-004-MMC3/Ninja Gaiden II - The Dark Sword of Chaos.png",
    "mapper-069-FME7-Sunsoft5B/Batman - Return of the Joker.png",
    "mapper-009-MMC2/Mike Tyson's Punch-Out!!.png",
    "fds/Zelda no Densetsu - The Hyrule Fantasy (Japan) (Rev 1).png",
    "fds/Metroid (Japan) (Rev 3).png",
    "mapper-005-MMC5/Castlevania III - Dracula's Curse.png",
    "vs-system/VS Castlevania.png",
    "vs-system/VS Excitebike.png",
    "mapper-085-VRC7/Lagrange Point (Japan) (En) (1.01).png",
]


def cmd_montage(args) -> int:
    ext = os.path.join(SCREENSHOTS, "external")
    out = os.path.join(SCREENSHOTS, "montage.png")
    if not shutil.which("montage"):
        print("error: ImageMagick `montage` not on PATH", file=sys.stderr)
        return 2
    tiles = []
    for entry in MONTAGE_ROSTER:
        src = os.path.join(ext, entry)
        if os.path.exists(src):
            tiles.append(src)
        else:
            print(f"MISSING: {entry}", file=sys.stderr)
    print(f"montage tiles: {len(tiles)}")
    if not tiles:
        return 1
    subprocess.run(
        ["montage", *tiles, "-tile", "7x4", "-geometry", "256x240+3+3",
         "-background", "#101014", out],
        check=True,
    )
    subprocess.run(
        ["identify", "-format", "montage: %wx%h  %B bytes\n", out], check=False
    )
    return 0


# --------------------------------------------------------------------------- #
# Subcommand: report
# --------------------------------------------------------------------------- #


def cmd_report(args) -> int:
    idx = load_index(args)
    avail = avail_counts(idx)
    staged = staged_counts()
    shots = screenshot_counts()
    core, curated, best = load_tiers()
    impl = implemented_mappers()
    target = args.target

    print("=== RustyNES coverage report ===")
    print(f"library: {idx['scanned']} iNES headers, {len(idx['ines'])} distinct "
          f"mappers, {idx['unif_seen']} UNIF files")
    print(f"tiers: Core={len(core)} Curated={len(curated)} BestEffort={len(best)} "
          f"(implemented total={len(impl)})")
    by_tier = defaultdict(lambda: [0, 0])
    for m in impl:
        t = tier_of(m)
        by_tier[t][0] += 1
        if shots.get(m, 0) >= target:
            by_tier[t][1] += 1
    for t in ("Core", "Curated", "BestEffort"):
        tot, ok = by_tier[t]
        print(f"  {t:>10}: {ok}/{tot} mappers at >={target} screenshots")
    missing = [m for m in impl if shots.get(m, 0) < target]
    print(f"under-{target}-shots mappers: {len(missing)}")
    print("  with library ROMs to fill:",
          " ".join(str(m) for m in missing if avail.get(m, 0) >= target))
    print("  no library iNES:",
          " ".join(str(m) for m in missing if avail.get(m, 0) == 0))
    total_staged = sum(staged.values())
    total_shots = sum(shots.values())
    print(f"staged ROMs: {total_staged}; committed screenshots: {total_shots}")
    return 0


# --------------------------------------------------------------------------- #
# Argument parsing
# --------------------------------------------------------------------------- #


def add_root_arg(p) -> None:
    p.add_argument(
        "--root", action="append", dest="roots", metavar="PATH",
        help="library root to scan (repeatable; adds to/overrides the default)",
    )
    p.add_argument(
        "--refresh", action="store_true",
        help="force a fresh library scan instead of using the cache",
    )


def build_parser() -> argparse.ArgumentParser:
    ap = argparse.ArgumentParser(
        prog="coverage.py",
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("index", help="build/cache the library mapper index")
    add_root_arg(p)
    p.add_argument("wanted", nargs="*", help="mapper numbers to summarize")
    p.set_defaults(func=cmd_index)

    p = sub.add_parser("survey", help="avail/staged/shots coverage report")
    add_root_arg(p)
    p.add_argument("--target", type=int, default=5, help="screenshot target (default 5)")
    p.set_defaults(func=cmd_survey)

    p = sub.add_parser("discover", help="candidate titles per mapper")
    add_root_arg(p)
    p.add_argument("--mapper", action="append", help="restrict to mapper N (repeatable)")
    p.add_argument("--count", type=int, default=5, help="candidates per mapper (default 5)")
    p.add_argument("--all", action="store_true", help="include unimplemented mappers seen in library")
    p.set_defaults(func=cmd_discover)

    p = sub.add_parser("stage", help="extract header-verified ROMs into external/")
    add_root_arg(p)
    p.add_argument("--ines", type=int, default=None, help="distinct iNES per mapper (default = --target)")
    p.add_argument("--unif", action="store_true", help="also use/report UNIF gap-fillers")
    p.add_argument("--target", type=int, default=5, help="coverage target (default 5)")
    p.add_argument("--mapper", action="append", help="restrict to mapper N (repeatable)")
    p.add_argument("--force", action="store_true", help="stage even well-covered mappers")
    g = p.add_mutually_exclusive_group()
    g.add_argument("--dry-run", action="store_true", default=True, help="preview only (DEFAULT)")
    g.add_argument("--execute", action="store_true", help="actually write the ROMs")
    p.set_defaults(func=cmd_stage)

    p = sub.add_parser("categorize", help="tier-split the screenshot tree")
    p.add_argument("--dry-run", action="store_true", help="preview the moves")
    p.set_defaults(func=cmd_categorize)

    p = sub.add_parser("montage", help="build the showcase montage")
    p.set_defaults(func=cmd_montage)

    p = sub.add_parser("report", help="one-shot coverage summary")
    add_root_arg(p)
    p.add_argument("--target", type=int, default=5, help="screenshot target (default 5)")
    p.set_defaults(func=cmd_report)

    return ap


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    # stage resolves sources back through the index; expose it module-wide.
    if args.cmd == "stage":
        global _STAGE_IDX
        _STAGE_IDX = load_index(args)
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
