//! Mapper accuracy tiering (v1.2.0).
//!
//! `RustyNES` classifies every supported mapper family into one of three tiers.
//! The tier is an **honesty marker**, not a behavioural one: a mapper's runtime
//! behaviour is identical regardless of tier — the tier records only how much
//! external evidence backs its correctness, so accuracy claims stay precise as
//! the long-tail mapper set grows.
//!
//! - [`MapperTier::Core`] — the original spec-implemented families that are
//!   gated by the `AccuracyCoin` / commercial-ROM oracle suites.
//! - [`MapperTier::Curated`] — long-tail families added with concrete game
//!   demand plus a redistributable fixture or spec; register-decode unit-tested
//!   and boot-smoked (oracle-gated where a free fixture exists).
//! - [`MapperTier::BestEffort`] — long-tail families ported from reference
//!   emulators (`GeraNES` / `Mesen2`) that have no redistributable test fixture.
//!   Register-decode unit-tested only, and **explicitly excluded** from the
//!   `AccuracyCoin` / oracle gate.
//!
//! The load-bearing invariant — *no `BestEffort` mapper may back a ROM in the
//! accuracy oracle corpus* — is enforced at the classifier level: `BestEffort`
//! is structurally never accuracy-gated, the three tier id-sets are disjoint,
//! and the byte-oracle corpus references only Core/Curated mappers by
//! construction. This [`mapper_tier`] classifier is the single source of truth.
//! See `docs/adr/0011-mapper-tiering.md`.

/// Accuracy-evidence tier for a supported mapper family. See the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MapperTier {
    /// Original families, `AccuracyCoin` / oracle-gated.
    Core,
    /// Curated long-tail: demand + redistributable fixture/spec, unit + smoke tested.
    Curated,
    /// Best-effort long-tail: reference-ported, register-decode tested only,
    /// never part of the accuracy gate.
    BestEffort,
}

impl MapperTier {
    /// Human-readable tier name (for docs generation, UI badges, and logs).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Core => "Core",
            Self::Curated => "Curated",
            Self::BestEffort => "BestEffort",
        }
    }

    /// Whether this tier is covered by the `AccuracyCoin` / commercial-ROM oracle
    /// gate. `Core` and `Curated` are; `BestEffort` is not.
    #[must_use]
    pub const fn is_accuracy_gated(self) -> bool {
        matches!(self, Self::Core | Self::Curated)
    }
}

/// Classify a mapper family by its iNES id (and NES 2.0 submapper, reserved for
/// future per-submapper tiering) into a [`MapperTier`].
///
/// Returns `None` for any id that [`crate::parse`] does not support — the two
/// sets are kept in lockstep, so a supported mapper always has a tier and an
/// unsupported one never does. The submapper argument is accepted now so a
/// future `Core` family with a `BestEffort` submapper variant can be expressed
/// without a signature change; today no family tiers on it.
#[must_use]
pub const fn mapper_tier(id: u16, _submapper: u8) -> Option<MapperTier> {
    match id {
        // --- Tier 0 / Core: the original 51 families (AccuracyCoin/oracle-gated).
        0 | 1 | 2 | 3 | 4 | 5 | 7 | 9 | 10 | 11 | 13 | 16 | 18 | 19 | 21 | 22 | 23 | 24 | 25
        | 26 | 32 | 33 | 34 | 48 | 64 | 65 | 66 | 67 | 68 | 69 | 70 | 71 | 73 | 75 | 78 | 80
        | 82 | 85 | 87 | 88 | 89 | 93 | 99 | 118 | 119 | 151 | 152 | 159 | 184 | 206 | 210 => {
            Some(MapperTier::Core)
        }

        // --- Tier 1 / Curated: discrete-logic long-tail boards (sprint5).
        // Notable games + decode-table spec; register-decode unit-tested.
        38 | 41 | 79 | 86 | 113 | 140 | 232 | 240 | 241 => Some(MapperTier::Curated),

        // --- Tier 2 / BestEffort: reference-ported long-tail sweep
        // (sprint6 + sprint7 + sprint8 + sprint9 + sprint10 + the v1.6.0
        // J.Y. Company ASIC 35/90/209/211 + the v1.6.0 "Studio" Workstream E
        // sprint11 batch: MMC3-clones 44/49/52/115/134/189/205/238/245/348/366,
        // Sachen 8259 A/B/C 138/139/141, and discrete multicarts
        // 42/46/50/51/57/104/120/290/301). Register-decode + save-state
        // unit-tested; NOT accuracy-gated.
        15 | 28 | 29 | 30 | 31 | 35 | 36 | 39 | 40 | 42 | 44 | 46 | 49 | 50 | 51 | 52 | 57 | 58
        | 60 | 61 | 62 | 63 | 72 | 76 | 77 | 81 | 90 | 92 | 94 | 95 | 96 | 97 | 101 | 104 | 107
        | 111 | 112 | 115 | 120 | 132 | 133 | 134 | 137 | 138 | 139 | 141 | 143 | 145 | 146
        | 147 | 148 | 149 | 150 | 156 | 162 | 174 | 177 | 178 | 179 | 180 | 185 | 189 | 200
        | 201 | 202 | 203 | 205 | 209 | 211 | 212 | 213 | 214 | 218 | 225 | 226 | 227 | 229
        | 231 | 233 | 234 | 238 | 242 | 244 | 245 | 246 | 250 | 290 | 301 | 348 | 366 => {
            Some(MapperTier::BestEffort)
        }

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 51 Core (Tier-0) families that shipped before v1.2.0. This list is
    /// the contract: every id here must classify as `Core`, and the count must
    /// stay at 51 until the curated/best-effort batches deliberately extend it.
    const CORE_IDS: &[u16] = &[
        0, 1, 2, 3, 4, 5, 7, 9, 10, 11, 13, 16, 18, 19, 21, 22, 23, 24, 25, 26, 32, 33, 34, 48, 64,
        65, 66, 67, 68, 69, 70, 71, 73, 75, 78, 80, 82, 85, 87, 88, 89, 93, 99, 118, 119, 151, 152,
        159, 184, 206, 210,
    ];

    #[test]
    fn all_core_ids_classify_as_core() {
        for &id in CORE_IDS {
            assert_eq!(
                mapper_tier(id, 0),
                Some(MapperTier::Core),
                "mapper {id} should be Tier-0 Core"
            );
        }
    }

    #[test]
    fn core_family_count_is_fifty_one() {
        assert_eq!(
            CORE_IDS.len(),
            51,
            "the Core tier is the original 51 families"
        );
    }

    /// The v1.2.0 curated (Tier-1) batch added in `sprint5.rs`. Must stay in
    /// lockstep with the `parse()` match arms for those ids.
    const CURATED_IDS: &[u16] = &[38, 41, 79, 86, 113, 140, 232, 240, 241];

    #[test]
    fn all_curated_ids_classify_as_curated() {
        for &id in CURATED_IDS {
            assert_eq!(
                mapper_tier(id, 0),
                Some(MapperTier::Curated),
                "mapper {id} should be Tier-1 Curated"
            );
        }
    }

    /// The best-effort (Tier-2) sweep added in `sprint6.rs` + `sprint7.rs`
    /// (v1.2.0), the v1.3.0 "Bedrock" Workstream D1 batch (`sprint8.rs`), the
    /// v1.4.0 "Fidelity" Workstream G batch (`sprint9.rs`), the v1.5.0 "Lens"
    /// Workstream F batch (`sprint10.rs`), the v1.6.0 "Studio" J.Y. Company
    /// ASIC (90/209/211 + the 35 sibling), and the v1.6.0 "Studio" Workstream E
    /// `sprint11.rs` batch (MMC3-clones, Sachen 8259 A/B/C, discrete
    /// multicarts).
    const BEST_EFFORT_IDS: &[u16] = &[
        15, 28, 29, 30, 31, 35, 36, 39, 40, 42, 44, 46, 49, 50, 51, 52, 57, 58, 60, 61, 62, 63, 72,
        76, 77, 81, 90, 92, 94, 95, 96, 97, 101, 104, 107, 111, 112, 115, 120, 132, 133, 134, 137,
        138, 139, 141, 143, 145, 146, 147, 148, 149, 150, 156, 162, 174, 177, 178, 179, 180, 185,
        189, 200, 201, 202, 203, 205, 209, 211, 212, 213, 214, 218, 225, 226, 227, 229, 231, 233,
        234, 238, 242, 244, 245, 246, 250, 290, 301, 348, 366,
    ];

    #[test]
    fn all_best_effort_ids_classify_as_best_effort() {
        for &id in BEST_EFFORT_IDS {
            assert_eq!(
                mapper_tier(id, 0),
                Some(MapperTier::BestEffort),
                "mapper {id} should be Tier-2 BestEffort"
            );
        }
    }

    #[test]
    fn best_effort_is_not_accuracy_gated() {
        for &id in BEST_EFFORT_IDS {
            assert!(
                !mapper_tier(id, 0).unwrap().is_accuracy_gated(),
                "BestEffort mapper {id} must not be accuracy-gated"
            );
        }
    }

    #[test]
    fn tiers_are_pairwise_disjoint() {
        // No mapper id may appear in more than one tier — a copy-paste guard for
        // the three classifier arms.
        for &id in CURATED_IDS {
            assert!(!CORE_IDS.contains(&id), "id {id} in both Core and Curated");
            assert!(
                !BEST_EFFORT_IDS.contains(&id),
                "id {id} in both Curated and BestEffort"
            );
        }
        for &id in BEST_EFFORT_IDS {
            assert!(
                !CORE_IDS.contains(&id),
                "id {id} in both Core and BestEffort"
            );
        }
    }

    #[test]
    fn unsupported_id_has_no_tier() {
        // A representative unsupported id; mapper 255 is not implemented.
        assert_eq!(mapper_tier(255, 0), None);
    }

    #[test]
    fn core_tier_is_accuracy_gated() {
        assert!(MapperTier::Core.is_accuracy_gated());
        assert!(MapperTier::Curated.is_accuracy_gated());
        assert!(!MapperTier::BestEffort.is_accuracy_gated());
    }
}
