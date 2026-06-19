# Screenshot curation manifest (task #58)

Canonical-name rule: the committed screenshot PNG name == the source ROM
filename stem; the `external_coverage` baseline name ==
`external_coverage__sanitize(<mapper-dir>/<rom-stem>).snap`
(`sanitize` = collapse every non-alphanumeric run to a single `_`). A durable
rename therefore moves three artifacts in lockstep: the PNG, the staged
`.nes`/`.fds` ROM (local, gitignored), and the `.snap`.

This branch standardized + deduped the committed **PNGs**, and renamed the
matching `.snap` baselines in tandem **wherever a discoverable local ROM
(`.nes`/`.fds`/`.unf`) existed** to keep the rename self-consistent. The
items below are the residue the local checkout could not safely move (the
`.snap` exists but the source ROM is staged only as a `.zip`, so the blessed
`.nes` lives in the maintainer's authoritative set under a name this branch
must not touch). Apply these to that set, then re-bless
(`scripts/coverage/bless.sh` + `cargo insta accept` + `coverage.py categorize`).

## Deferred dedup (remove losing ROM + its snap)

- DELETE `mapper-046-m46/RumbleStation 15-in-1 (Unl).*` + `external_coverage__mapper_046_m46_RumbleStation_15_in_1_Unl.snap` (dup of `Rumble Station - 15 in 1 (USA) (Unl)`)

## Deferred rename (ROM + snap) to match the standardized PNG

- `mapper-042-m42/`: ROM `Ai Senshi Nicol (FDS Conversion) [p1][!].*` -> `Ai Senshi Nicol (FDS Conversion).*`; snap `external_coverage__mapper_042_m42_Ai_Senshi_Nicol_FDS_Conversion_p1.snap` -> `external_coverage__mapper_042_m42_Ai_Senshi_Nicol_FDS_Conversion.snap`
- `mapper-044-m44/`: ROM `Super Big 7-in-1 [p1][!].*` -> `Super Big 7-in-1.*`; snap `external_coverage__mapper_044_m44_Super_Big_7_in_1_p1.snap` -> `external_coverage__mapper_044_m44_Super_Big_7_in_1.snap`
- `mapper-049-m49/`: ROM `Super HIK 4-in-1 [p1][!].*` -> `Super HIK 4-in-1.*`; snap `external_coverage__mapper_049_m49_Super_HIK_4_in_1_p1.snap` -> `external_coverage__mapper_049_m49_Super_HIK_4_in_1.snap`
- `mapper-052-m52/`: ROM `Jackie Chan (J) [!].*` -> `Jackie Chan (Japan).*`; snap `external_coverage__mapper_052_m52_Jackie_Chan_J.snap` -> `external_coverage__mapper_052_m52_Jackie_Chan_Japan.snap`
- `mapper-057-m57/`: ROM `54-in-1 (Game Star - GK-54) [p1][!].*` -> `54-in-1 (Game Star - GK-54).*`; snap `external_coverage__mapper_057_m57_54_in_1_Game_Star_GK_54_p1.snap` -> `external_coverage__mapper_057_m57_54_in_1_Game_Star_GK_54.snap`
- `mapper-057-m57/`: ROM `6-in-1 (SuperGK-L02A) [p1][!].*` -> `6-in-1 (SuperGK-L02A).*`; snap `external_coverage__mapper_057_m57_6_in_1_SuperGK_L02A_p1.snap` -> `external_coverage__mapper_057_m57_6_in_1_SuperGK_L02A.snap`
- `mapper-061-Multicart61/`: ROM `Tetris Family 9-in-1 (Hwang Shinwei) (Unl) [!].*` -> `Tetris Family 9-in-1 (Hwang Shinwei) (Unl).*`; snap `external_coverage__mapper_061_Multicart61_Tetris_Family_9_in_1_Hwang_Shinwei_Unl.snap` -> `external_coverage__mapper_061_Multicart61_Tetris_Family_9_in_1_Hwang_Shinwei_Unl.snap`
- `mapper-085-VRC7/`: ROM `Super Mario Bros 14 (Unl) [!].*` -> `Super Mario Bros 14 (Unl).*`; snap `external_coverage__mapper_085_VRC7_Super_Mario_Bros_14_Unl.snap` -> `external_coverage__mapper_085_VRC7_Super_Mario_Bros_14_Unl.snap`
- `mapper-099-VsSystem/`: ROM `Stroke & Match Golf (VS) [!].*` -> `Stroke & Match Golf (VS).*`; snap `external_coverage__mapper_099_VsSystem_Stroke_Match_Golf_VS.snap` -> `external_coverage__mapper_099_VsSystem_Stroke_Match_Golf_VS.snap`
- `mapper-115-m115/`: ROM `Shisen Mahjong 2 (Ch) [!].*` -> `Shisen Mahjong 2 (China).*`; snap `external_coverage__mapper_115_m115_Shisen_Mahjong_2_Ch.snap` -> `external_coverage__mapper_115_m115_Shisen_Mahjong_2_China.snap`
- `mapper-115-m115/`: ROM `Thunderbolt 2 (Ch) [!].*` -> `Thunderbolt 2 (China).*`; snap `external_coverage__mapper_115_m115_Thunderbolt_2_Ch.snap` -> `external_coverage__mapper_115_m115_Thunderbolt_2_China.snap`
- `mapper-132-TXC132/`: ROM `Creatom (Unl) [!].*` -> `Creatom (Unl).*`; snap `external_coverage__mapper_132_TXC132_Creatom_Unl.snap` -> `external_coverage__mapper_132_TXC132_Creatom_Unl.snap`
- `mapper-133-SachenSA72008/`: ROM `Jovial Race (Sachen) [!].*` -> `Jovial Race (Sachen).*`; snap `external_coverage__mapper_133_SachenSA72008_Jovial_Race_Sachen.snap` -> `external_coverage__mapper_133_SachenSA72008_Jovial_Race_Sachen.snap`
- `mapper-134-m134/`: ROM `2-in-1 - Family Kid & Aladdin 4 (Ch) [!].*` -> `2-in-1 - Family Kid & Aladdin 4 (China).*`; snap `external_coverage__mapper_134_m134_2_in_1_Family_Kid_Aladdin_4_Ch.snap` -> `external_coverage__mapper_134_m134_2_in_1_Family_Kid_Aladdin_4_China.snap`
- `mapper-137-Sachen8259D/`: ROM `Great Wall, The (Sachen) [!].*` -> `Great Wall, The (Sachen).*`; snap `external_coverage__mapper_137_Sachen8259D_Great_Wall_The_Sachen.snap` -> `external_coverage__mapper_137_Sachen8259D_Great_Wall_The_Sachen.snap`
- `mapper-138-m138/`: ROM `Silver Eagle (Sachen) [!].*` -> `Silver Eagle (Sachen).*`; snap `external_coverage__mapper_138_m138_Silver_Eagle_Sachen.snap` -> `external_coverage__mapper_138_m138_Silver_Eagle_Sachen.snap`
- `mapper-139-m139/`: ROM `Hell Fighter (Sachen) [!].*` -> `Hell Fighter (Sachen).*`; snap `external_coverage__mapper_139_m139_Hell_Fighter_Sachen.snap` -> `external_coverage__mapper_139_m139_Hell_Fighter_Sachen.snap`
- `mapper-141-m141/`: ROM `Po Po Team (Sachen) [!].*` -> `Po Po Team (Sachen).*`; snap `external_coverage__mapper_141_m141_Po_Po_Team_Sachen.snap` -> `external_coverage__mapper_141_m141_Po_Po_Team_Sachen.snap`
- `mapper-141-m141/`: ROM `Rockball (Sachen) [!].*` -> `Rockball (Sachen).*`; snap `external_coverage__mapper_141_m141_Rockball_Sachen.snap` -> `external_coverage__mapper_141_m141_Rockball_Sachen.snap`
- `mapper-143-SachenTCA01/`: ROM `Dancing Blocks (Sachen) [!].*` -> `Dancing Blocks (Sachen).*`; snap `external_coverage__mapper_143_SachenTCA01_Dancing_Blocks_Sachen.snap` -> `external_coverage__mapper_143_SachenTCA01_Dancing_Blocks_Sachen.snap`
- `mapper-145-SachenSA72007/`: ROM `Sidewinder (Sachen) [!].*` -> `Sidewinder (Sachen).*`; snap `external_coverage__mapper_145_SachenSA72007_Sidewinder_Sachen.snap` -> `external_coverage__mapper_145_SachenSA72007_Sidewinder_Sachen.snap`
- `mapper-148-SachenSA0037/`: ROM `Mahjong World (Sachen) [!].*` -> `Mahjong World (Sachen).*`; snap `external_coverage__mapper_148_SachenSA0037_Mahjong_World_Sachen.snap` -> `external_coverage__mapper_148_SachenSA0037_Mahjong_World_Sachen.snap`
- `mapper-150-Sachen74LS374N/`: ROM `Chess Academys (Sachen-JAP) [!].*` -> `Chess Academys (Sachen-JAP).*`; snap `external_coverage__mapper_150_Sachen74LS374N_Chess_Academys_Sachen_JAP.snap` -> `external_coverage__mapper_150_Sachen74LS374N_Chess_Academys_Sachen_JAP.snap`
- `mapper-150-Sachen74LS374N/`: ROM `Chinese Checkers (Sachen-JAP) [!].*` -> `Chinese Checkers (Sachen-JAP).*`; snap `external_coverage__mapper_150_Sachen74LS374N_Chinese_Checkers_Sachen_JAP.snap` -> `external_coverage__mapper_150_Sachen74LS374N_Chinese_Checkers_Sachen_JAP.snap`
- `mapper-150-Sachen74LS374N/`: ROM `Mahjong Academy (Sachen) [!].*` -> `Mahjong Academy (Sachen).*`; snap `external_coverage__mapper_150_Sachen74LS374N_Mahjong_Academy_Sachen.snap` -> `external_coverage__mapper_150_Sachen74LS374N_Mahjong_Academy_Sachen.snap`
- `mapper-150-Sachen74LS374N/`: ROM `Mei Nu Quan (Honey Peach) (Sachen) [!].*` -> `Mei Nu Quan (Honey Peach) (Sachen).*`; snap `external_coverage__mapper_150_Sachen74LS374N_Mei_Nu_Quan_Honey_Peach_Sachen.snap` -> `external_coverage__mapper_150_Sachen74LS374N_Mei_Nu_Quan_Honey_Peach_Sachen.snap`
- `mapper-150-Sachen74LS374N/`: ROM `Taiwan Mahjong 2 (Sachen) [!].*` -> `Taiwan Mahjong 2 (Sachen).*`; snap `external_coverage__mapper_150_Sachen74LS374N_Taiwan_Mahjong_2_Sachen.snap` -> `external_coverage__mapper_150_Sachen74LS374N_Taiwan_Mahjong_2_Sachen.snap`
- `mapper-178-WaixingEdu/`: ROM `Education Computer 32-in-1 (Game Star) [!].*` -> `Education Computer 32-in-1 (Game Star).*`; snap `external_coverage__mapper_178_WaixingEdu_Education_Computer_32_in_1_Game_Star.snap` -> `external_coverage__mapper_178_WaixingEdu_Education_Computer_32_in_1_Game_Star.snap`
- `mapper-178-WaixingEdu/`: ROM `Education Computer 48-in-1 (Game Star) [!].*` -> `Education Computer 48-in-1 (Game Star).*`; snap `external_coverage__mapper_178_WaixingEdu_Education_Computer_48_in_1_Game_Star.snap` -> `external_coverage__mapper_178_WaixingEdu_Education_Computer_48_in_1_Game_Star.snap`
- `mapper-189-m189/`: ROM `Mario Fighter III (Unl) [!].*` -> `Mario Fighter III (Unl).*`; snap `external_coverage__mapper_189_m189_Mario_Fighter_III_Unl.snap` -> `external_coverage__mapper_189_m189_Mario_Fighter_III_Unl.snap`
- `mapper-189-m189/`: ROM `Master Fighter II (Unl) [!].*` -> `Master Fighter II (Unl).*`; snap `external_coverage__mapper_189_m189_Master_Fighter_II_Unl.snap` -> `external_coverage__mapper_189_m189_Master_Fighter_II_Unl.snap`
- `mapper-234-Maxi15/`: ROM `Maxi 15 (AVE) [!].*` -> `Maxi 15 (AVE).*`; snap `external_coverage__mapper_234_Maxi15_Maxi_15_AVE.snap` -> `external_coverage__mapper_234_Maxi15_Maxi_15_AVE.snap`
- `mapper-250-Nitra/`: ROM `Queen Bee V (Unl) [!].*` -> `Queen Bee V (Unl).*`; snap `external_coverage__mapper_250_Nitra_Queen_Bee_V_Unl.snap` -> `external_coverage__mapper_250_Nitra_Queen_Bee_V_Unl.snap`

## Unconfident PNG names left untouched (messy translation/homebrew blobs)

- `screenshots/besteffort/mapper-061-Multicart61/20-in-1 [a1][p1][!].png` (suggest review; likely `20-in-1.png`)
- `screenshots/besteffort/mapper-101-JalecoJF10/Urusei Yatsura - Lum no Wedding Bell (J) [a1][T+Fre].png` (suggest review; likely `Urusei Yatsura - Lum no Wedding Bell (Japan).png`)
- `screenshots/besteffort/mapper-101-JalecoJF10/Urusei Yatsura - Lum no Wedding Bell _J_ _a1__T_Eng1.0_Stardust Crusaders_.png` (suggest review; likely `Urusei Yatsura - Lum no Wedding Bell J a1 T Eng1.0 Stardust Crusaders.png`)
- `screenshots/besteffort/mapper-218-MagicFloor/Magic Floor by Martin Korth _2012_ _PC10 Version_ _PD_ _a1_.png` (suggest review; likely `Magic Floor by Martin Korth 2012 PC10 Version PD a1.png`)
- `screenshots/external/mapper-048-TaitoTC0690/Bubble Bobble 2 (J) [T+Chi_NOKOH].png` (suggest review; likely `Bubble Bobble 2 (Japan).png`)
- `screenshots/external/mapper-064-TengenRAMBO1/Balloon Fight (J) [T+Rus_Mario Soft].png` (suggest review; likely `Balloon Fight (Japan).png`)
- `screenshots/external/mapper-078-HolyDiver/Portopia Renzoku Satsujin Jiken (J) [T-Eng_DvD Translations].png` (suggest review; likely `Portopia Renzoku Satsujin Jiken (Japan).png`)
- `screenshots/external/mapper-086-Jaleco86/Urusei Yatsura - Lum no Wedding Bell (J) [a1].png` (suggest review; likely `Urusei Yatsura - Lum no Wedding Bell (Japan).png`)
- `screenshots/external/mapper-140-Jaleco140/Arkanoid (J) [T+Chi_NOKOH].png` (suggest review; likely `Arkanoid (Japan).png`)
