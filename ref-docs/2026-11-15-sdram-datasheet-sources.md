# Alliance Memory SDRAM datasheets: where to get them

**Dated supplemental reference, 2026-11-15.** Until this date the repository
carried three revisions of the Alliance Memory AS4C32M16SB datasheet under
`ref-docs/Datasheets/`. Every page footer of those PDFs reads "Confidential",
and none of them states a redistribution grant, so they were removed from the
tracked tree (the files stay on the maintainer's disk; `/ref-docs/Datasheets/*.pdf`
is now gitignored). This note replaces them with the vendor's public download
locations. Earlier `ref-docs/` files that cite a `ref-docs/Datasheets/...` path
are immutable and stay as written; read such a path as a pointer to the
matching row below.

Product page: <https://www.alliancememory.com/as4c32m16sb/> (lists the current
datasheet; read 2026-11-15).

| Former path (`ref-docs/Datasheets/`) | Part numbers covered | Revision | Public download (checked 2026-11-15) |
| --- | --- | --- | --- |
| `AllianceMemory_512M_SDRAM_Bdie_AS4C32M16SB-7TXN-6TIN-7BIN_Rev1.2_March2020.pdf` | AS4C32M16SB-7TCN / -7TIN / -6TIN / -7BIN (512 Mbit, 32M x 16, B die) | 1.2, March 2020 | <https://www.alliancememory.com/wp-content/uploads/AllianceMemory_512M_SDRAM_Bdie_AS4C32M16SB-7TXN-6TIN-7BIN_Rev1.2_March2020.pdf> (byte-identical to the removed copy, md5 `bc7a91dc`) |
| `AllianceMemory_512M_SDRAM_Bdie_AS4C32M16SB-7TXN-6TIN-7BIN_Rev1.3_August2022_x.pdf` | as above | 1.3, August 2022 | **Not found.** The same file name under `wp-content/uploads/` redirects to the site's home page, and the product page lists only revision 1.4. Superseded by 1.4 |
| `AllianceMemory_512M_SDRAM_Bdie_AS4C32M16SB-7TXN-6TIN-7BIN__Rev1.4_June2024NK.pdf` | as above | 1.4, June 2024 | <https://www.alliancememory.com/wp-content/uploads/AllianceMemory_512M_SDRAM_Bdie_AS4C32M16SB-7TXN-6TIN-7BIN__Rev1.4_June2024NK.pdf> (byte-identical to the removed copy, md5 `ccfb1eda`; linked from the product page) |

Revision 1.4 is the one the MiSTer RTL and the SDC constraints were written
from (see `ref-docs/2026-09-02-mister-sdram-hardware-reference.md` and
`docs/adr/0039-sdram-clock-domain.md`).
